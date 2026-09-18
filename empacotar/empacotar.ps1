#Requires -Version 5.1
<#
.SYNOPSIS
Gera os instaladores do InputRemote em dist/, num comando so.

.DESCRIPTION
    .\empacotar\empacotar.ps1

Produz:

    dist\InputRemote-<versao>-x64.msi            instalador do Windows, assinado
    dist\inputremote-<versao>.fc44.x86_64.rpm    pacote do Fedora 44
    dist\MANIFESTO.txt                           o que saiu, com impressao digital

O RPM sai de dentro de um container do Fedora 44, e nao de compilacao cruzada. Um pacote que
nunca viu o sistema de destino descobre as dependencias erradas na maquina de quem instala; o
gerador de dependencias do RPM precisa ler o ELF no proprio Fedora para acertar os Requires.

A assinatura e autoassinada, para teste e uso local. Nao substitui certificado de verdade, e o
manifesto diz isso em letras maiusculas.

.PARAMETER Alvo
Tudo (padrao), Windows ou Linux.

.PARAMETER PularCompilacao
No lado Windows, reaproveita o que ja estiver em target/release.

.PARAMETER SemAssinatura
Nao assina. O MSI sai, e o manifesto registra a ausencia.
#>
param(
    [ValidateSet('Tudo', 'Windows', 'Linux')]
    [string]$Alvo = 'Tudo',

    [switch]$PularCompilacao,
    [switch]$SemAssinatura
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$raiz = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $raiz 'dist'
$ferramentas = Join-Path $PSScriptRoot 'ferramentas'
$trabalho = Join-Path $ferramentas 'trabalho'

# WiX 3.14.1, binarios avulsos. Nao exige instalar SDK do .NET nem mexer na maquina: o zip e
# baixado uma vez, conferido por hash e usado de dentro da propria arvore.
$wixUrl = 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip'
$wixHash = '6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31'
$imagemDoFedora = 'inputremote-fedora44:latest'

# Onde cada etapa deixa o que produziu. Variavel, e nao valor de retorno: uma funcao do
# PowerShell devolve tudo que escapou para a saida, e com candle, light e docker no meio o
# retorno viraria um vetor com o ruido das ferramentas junto.
$script:msiGerado = $null
$script:rpmGerado = $null

$componentes = @(
    @{ Arquivo = 'inputremote-daemon.exe'; Papel = 'servico privilegiado'; Obrigatorio = $true },
    @{ Arquivo = 'inputremote-agent.exe';  Papel = 'agente de sessao';     Obrigatorio = $true },
    @{ Arquivo = 'inputremote-ui.exe';     Papel = 'interface';            Obrigatorio = $false }
)

function Escrever-Titulo([string]$t) { Write-Host ''; Write-Host "### $t" -ForegroundColor Green }
function Escrever-Passo([string]$t) { Write-Host "==> $t" -ForegroundColor Cyan }
function Escrever-Aviso([string]$t) { Write-Host "!!! $t" -ForegroundColor Yellow }

function Ler-Versao {
    $linha = Select-String -Path (Join-Path $raiz 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1
    if (-not $linha) { throw 'nao achei a versao em Cargo.toml' }
    return $linha.Matches[0].Groups[1].Value
}

function Ler-Commit {
    try {
        $commit = & git -C $raiz rev-parse --short HEAD 2>$null
        if ($LASTEXITCODE -ne 0) { return 'sem git' }
        if (& git -C $raiz status --porcelain 2>$null) { return "$commit (arvore suja)" }
        return $commit
    } catch { return 'sem git' }
}

function Descrever-Assinatura([string]$caminho) {
    $estado = (Get-AuthenticodeSignature -FilePath $caminho).Status
    switch ($estado) {
        'Valid'        { return 'assinado e confiavel nesta maquina' }
        'NotSigned'    { return 'NAO ASSINADO' }
        'UnknownError' { return 'assinado, mas o certificado nao e confiavel aqui' }
        default        { return "assinado ($estado)" }
    }
}

function Escrever-Texto([string]$Caminho, [string[]]$Linhas) {
    # Sem BOM. `Set-Content -Encoding utf8` no PowerShell 5.1 escreve os tres bytes de marca, e um
    # manifesto que comeca com lixo invisivel aparece sujo em qualquer leitor de Linux.
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllLines($Caminho, $Linhas, $utf8)
}

function Executar-Ferramenta {
    param([string]$Programa, [string[]]$Argumentos, [string]$Nome)

    # A saida e capturada para poder ser mostrada em caso de falha. Engolir a mensagem da
    # ferramenta troca uma falha explicada por uma falha misteriosa -- e quem empacota fica
    # sabendo que "light falhou", que e exatamente a informacao que nao ajuda.
    $anterior = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $saida = & $Programa @Argumentos 2>&1
    } finally {
        $ErrorActionPreference = $anterior
    }

    if ($LASTEXITCODE -ne 0) {
        foreach ($linha in $saida) { Write-Host "    $linha" -ForegroundColor Red }
        throw "$Nome falhou (codigo $LASTEXITCODE)"
    }
    return $saida
}

function Garantir-Wix {
    $candle = Join-Path $ferramentas 'wix314\candle.exe'
    if (Test-Path $candle) { return (Split-Path -Parent $candle) }

    Escrever-Passo 'baixando o WiX 3.14.1 (uma vez so)'
    New-Item -ItemType Directory -Force -Path $ferramentas | Out-Null
    $zip = Join-Path $ferramentas 'wix314.zip'
    Invoke-WebRequest -Uri $wixUrl -OutFile $zip -UseBasicParsing

    $obtido = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLower()
    if ($obtido -ne $wixHash) {
        Remove-Item -Force $zip
        throw "o zip do WiX nao confere: esperado $wixHash, obtido $obtido"
    }

    Expand-Archive -Path $zip -DestinationPath (Join-Path $ferramentas 'wix314') -Force
    return (Join-Path $ferramentas 'wix314')
}

function Converter-Para-Rtf([string]$origem, [string]$destino) {
    # O WiX exige RTF na tela de licenca. Converter aqui evita manter dois arquivos com o mesmo
    # texto -- e dois arquivos com o mesmo texto sempre acabam diferentes.
    $texto = Get-Content -Path $origem -Raw
    $texto = $texto -replace '\\', '\\\\' -replace '\{', '\{' -replace '\}', '\}'
    $texto = $texto -replace "`r`n", "`n"
    $corpo = ($texto -split "`n") -join '\par' + '\par'
    $rtf = '{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0\fnil\fcharset0 Segoe UI;}}\fs18 ' + $corpo + '}'
    Set-Content -Path $destino -Value $rtf -Encoding ASCII
}

# =================================================================================================
# Windows
# =================================================================================================

function Empacotar-Windows([string]$versao, [string]$commit) {
    Escrever-Titulo 'Windows'

    if (-not $PularCompilacao) {
        Escrever-Passo 'compilando em release'
        & cargo build --release --manifest-path (Join-Path $raiz 'Cargo.toml')
        if ($LASTEXITCODE -ne 0) { throw 'a compilacao falhou' }
    }

    # Onde o cargo pos os binarios. Com `CARGO_TARGET_DIR` definido -- necessario quando o disco do
    # repositorio esta cheio --, eles NAO estao em `target\release`, e procurar la empacotaria em
    # silencio os binarios de uma compilacao anterior. Apontado pela sessao paralela (log 21).
    $release = if ($env:CARGO_TARGET_DIR) {
        Join-Path $env:CARGO_TARGET_DIR 'release'
    } else {
        Join-Path $raiz 'target\release'
    }
    $binarios = Join-Path $trabalho 'binarios'
    $recursos = Join-Path $trabalho 'recursos'
    foreach ($pasta in @($binarios, $recursos)) {
        if (Test-Path $pasta) { Remove-Item -Recurse -Force $pasta }
        New-Item -ItemType Directory -Force -Path $pasta | Out-Null
    }

    $presentes = @()
    $ausentes = @()
    foreach ($componente in $componentes) {
        $origem = Join-Path $release $componente.Arquivo
        if (Test-Path $origem) {
            Copy-Item $origem -Destination $binarios
            $presentes += $componente
        } else {
            $ausentes += $componente
        }
    }
    if ($presentes.Count -eq 0) { throw "nenhum binario do produto em $release" }
    Escrever-Passo "$($presentes.Count) componente(s), $($ausentes.Count) ausente(s)"

    # --- Assinatura ---------------------------------------------------------------------------
    if (-not $SemAssinatura) {
        Escrever-Passo 'assinando os executaveis (certificado autoassinado de teste)'
        $assinar = Join-Path $PSScriptRoot 'assinar.ps1'
        & $assinar -Acao Criar | Out-Null
        $alvos = @(Get-ChildItem $binarios -Filter '*.exe' | ForEach-Object { $_.FullName })
        & $assinar -Acao Assinar -Arquivos $alvos
    } else {
        Escrever-Aviso 'assinatura desligada a pedido'
    }

    # --- Recursos que entram no MSI ------------------------------------------------------------
    Copy-Item (Join-Path $PSScriptRoot 'windows\LEIAME.txt') -Destination $recursos
    Copy-Item (Join-Path $raiz 'recursos\icone.ico') -Destination $recursos
    Copy-Item (Join-Path $raiz 'LICENSE') -Destination (Join-Path $recursos 'LICENSE.txt')
    Converter-Para-Rtf (Join-Path $raiz 'LICENSE') (Join-Path $recursos 'LICENSE.rtf')

    Escrever-Manifesto -Destino (Join-Path $recursos 'MANIFESTO.txt') `
        -Versao $versao -Commit $commit -Pasta $binarios -Presentes $presentes -Ausentes $ausentes |
        Out-Null

    # --- MSI ------------------------------------------------------------------------------------
    $wix = Garantir-Wix
    $obj = Join-Path $trabalho 'obj'
    if (Test-Path $obj) { Remove-Item -Recurse -Force $obj }
    New-Item -ItemType Directory -Force -Path $obj | Out-Null

    # O MSI so aceita versao numerica x.y.z; o sufixo de desenvolvimento vai para a descricao e
    # para o nome do arquivo, onde ele continua visivel.
    $versaoDoMsi = ($versao -split '-')[0]
    $temDaemon = if ($presentes | Where-Object { $_.Arquivo -eq 'inputremote-daemon.exe' }) { 1 } else { 0 }
    $temAgente = if ($presentes | Where-Object { $_.Arquivo -eq 'inputremote-agent.exe' }) { 1 } else { 0 }

    Escrever-Passo "compilando o MSI (versao $versaoDoMsi, daemon=$temDaemon, agente=$temAgente)"
    # `-out` recebe o arquivo, e nao a pasta. Passar a pasta exigiria barra no fim, e um
    # argumento que termina em barra faz o Windows tratar a aspa de fechamento como escapada --
    # o que so aparece quando o caminho tem espaco, isto e, na maquina de outra pessoa.
    Executar-Ferramenta -Nome 'candle' -Programa (Join-Path $wix 'candle.exe') -Argumentos @(
        '-nologo', '-arch', 'x64', '-ext', 'WixUIExtension',
        "-dVersao=$versaoDoMsi", "-dRotulo=$versao",
        "-dBinarios=$binarios", "-dRecursos=$recursos",
        "-dTemDaemon=$temDaemon", "-dTemAgente=$temAgente",
        '-out', (Join-Path $obj 'Produto.wixobj'),
        (Join-Path $PSScriptRoot 'windows\Produto.wxs')
    ) | Out-Null

    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $msi = Join-Path $dist "InputRemote-$versao-x64.msi"
    Executar-Ferramenta -Nome 'light' -Programa (Join-Path $wix 'light.exe') -Argumentos @(
        # ICE61 reclama que o produto poderia remover uma versao igual a si mesmo. E
        # exatamente o que AllowSameVersionUpgrades pede, e e o que se quer enquanto a versao
        # for 0.1.0-dev e mudar varias vezes por dia sem mudar de numero.
        '-nologo', '-ext', 'WixUIExtension', '-sw1076', '-spdb',
        '-out', $msi,
        (Join-Path $obj 'Produto.wixobj')
    ) | Out-Null

    if (-not $SemAssinatura) {
        Escrever-Passo 'assinando o MSI'
        & (Join-Path $PSScriptRoot 'assinar.ps1') -Acao Assinar -Arquivos @($msi)
    }

    Escrever-Passo "MSI: $msi"
    $script:msiGerado = $msi
}

function Escrever-Manifesto {
    param(
        [string]$Destino, [string]$Versao, [string]$Commit, [string]$Pasta,
        [object[]]$Presentes, [object[]]$Ausentes
    )

    $linhas = New-Object System.Collections.Generic.List[string]
    $linhas.Add("InputRemote $Versao - pacote para Windows x64")
    $linhas.Add("commit: $Commit")
    $linhas.Add("montado em: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')")
    $linhas.Add('')
    $linhas.Add('CONTEUDO')
    foreach ($componente in $Presentes) {
        $caminho = Join-Path $Pasta $componente.Arquivo
        $resumo = (Get-FileHash -Algorithm SHA256 -Path $caminho).Hash.ToLower()
        $tamanho = [math]::Round((Get-Item $caminho).Length / 1MB, 2)
        $linhas.Add("  $($componente.Arquivo)  ($($componente.Papel), $tamanho MB)")
        $linhas.Add("    $(Descrever-Assinatura $caminho)")
        $linhas.Add("    sha256 $resumo")
    }

    if ($Ausentes.Count -gt 0) {
        $linhas.Add('')
        $linhas.Add('NAO INCLUIDO NESTE PACOTE')
        foreach ($componente in $Ausentes) {
            $peso = if ($componente.Obrigatorio) { 'exigido pelo requisito R1' } else { 'opcional' }
            $linhas.Add("  $($componente.Arquivo)  ($($componente.Papel), $peso) - ainda nao implementado")
        }
        $linhas.Add('')
        $linhas.Add('Sem o servico e o agente, este pacote instala apenas a interface, e ela roda')
        $linhas.Add('em modo de demonstracao contra um servico simulado. Nada e digitado em')
        $linhas.Add('computador nenhum. A propria janela avisa isso em cima.')
    }

    $linhas.Add('')
    $linhas.Add('ASSINATURA')
    $linhas.Add('O certificado e AUTOASSINADO, para teste e uso local. Ele nao substitui um')
    $linhas.Add('certificado de verdade e so vale nas maquinas onde for instalado como confiavel')
    $linhas.Add('(empacotar/assinar.ps1 -Acao Confiar). Sem confianca no certificado o Windows')
    $linhas.Add('nao concede UIAccess, e digitar na tela de bloqueio nao funciona.')
    $linhas.Add('Ver docs/05-windows.md, secao 4.4.')

    Escrever-Texto -Caminho $Destino -Linhas $linhas
}

# =================================================================================================
# Linux
# =================================================================================================

function Empacotar-Linux {
    Escrever-Titulo 'Linux (Fedora 44)'

    if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
        throw 'docker nao encontrado; o RPM e construido dentro de um Fedora 44 de verdade'
    }
    & docker info --format '{{.ServerVersion}}' 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'o docker esta instalado mas nao esta respondendo' }

    $existe = & docker images -q $imagemDoFedora
    if (-not $existe) {
        Escrever-Passo 'construindo a imagem de compilacao do Fedora (uma vez so)'
        & docker build -t $imagemDoFedora -f (Join-Path $PSScriptRoot 'linux\Dockerfile') (Join-Path $PSScriptRoot 'linux')
        if ($LASTEXITCODE -ne 0) { throw 'docker build falhou' }
    }

    Escrever-Passo 'compilando e empacotando dentro do container'
    # A fonte entra somente leitura: o empacotamento nao pode sujar a arvore de quem o chamou, e
    # um target/ de Linux escrito por cima do de Windows seria uma tarde perdida.
    #
    # Com teto de CPU e memoria, e a troca igual a memoria (sem swap). Sem teto, o rpmbuild compila
    # o workspace inteiro com LTO e um rustc por thread, a VM do WSL cresce ate paginar, e a maquina
    # inteira trava junto -- aconteceu, com o Docker parando de responder. Com teto, faltar memoria
    # mata um rustc e a construcao falha com motivo, que e melhor que um computador parado.
    $nucleos = [Math]::Max(2, [Math]::Floor([Environment]::ProcessorCount / 2))
    & docker run --rm `
        --cpus $nucleos `
        --memory 12g `
        --memory-swap 12g `
        -e "CARGO_BUILD_JOBS=$nucleos" `
        -v "${raiz}:/fonte:ro" `
        -v "${dist}:/saida" `
        -v inputremote-cargo:/opt/cargo/registry `
        $imagemDoFedora `
        bash /fonte/empacotar/linux/construir-rpm.sh
    if ($LASTEXITCODE -ne 0) { throw 'a construcao do RPM falhou' }

    $rpm = Get-ChildItem $dist -Filter '*.rpm' | Select-Object -First 1
    if (-not $rpm) { throw 'o container terminou sem produzir .rpm' }
    Escrever-Passo "RPM: $($rpm.FullName)"
    $script:rpmGerado = $rpm.FullName
}

# =================================================================================================

$versao = Ler-Versao
$commit = Ler-Commit

# Limpa so o que esta sendo refeito. Apagar dist/ inteiro faria uma rodada `-Alvo Windows` sumir
# com o RPM da anterior; deixar o antigo no lugar faria alguem instalar a versao errada na hora de
# demonstrar. Apagar por alvo resolve as duas coisas.
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$padroes = @()
if ($Alvo -in @('Tudo', 'Windows')) { $padroes += '*.msi' }
if ($Alvo -in @('Tudo', 'Linux')) { $padroes += '*.rpm' }
foreach ($padrao in $padroes) {
    Get-ChildItem $dist -Filter $padrao -ErrorAction SilentlyContinue | Remove-Item -Force
}

if ($Alvo -in @('Tudo', 'Windows')) { Empacotar-Windows -versao $versao -commit $commit }
if ($Alvo -in @('Tudo', 'Linux')) { Empacotar-Linux }

# --- Manifesto de dist ----------------------------------------------------------------------------

Escrever-Titulo 'Resultado'

# Lista o que esta em dist/, e nao so o que esta rodada produziu: quem roda `-Alvo Windows` em
# cima de um RPM existente precisa de um manifesto que descreva a pasta de verdade.
$saidas = @(Get-ChildItem $dist -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Extension -in @('.msi', '.rpm') } |
    Sort-Object Name | ForEach-Object { $_.FullName })

$resumo = New-Object System.Collections.Generic.List[string]
$resumo.Add("InputRemote $versao")
$resumo.Add("commit: $commit")
$resumo.Add("montado em: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')")
$resumo.Add('')
foreach ($saida in $saidas) {
    $hash = (Get-FileHash -Algorithm SHA256 -Path $saida).Hash.ToLower()
    $tamanho = [math]::Round((Get-Item $saida).Length / 1MB, 2)
    $resumo.Add("$(Split-Path -Leaf $saida)  ($tamanho MB)")
    if ($saida -like '*.msi') { $resumo.Add("  $(Descrever-Assinatura $saida)") }
    $resumo.Add("  sha256 $hash")
}
$resumo.Add('')
$resumo.Add('O RPM nao e assinado com GPG. Assinar exige uma chave do projeto, e uma chave')
$resumo.Add('gerada pelo empacotador nao provaria nada a quem instala.')

$manifesto = Join-Path $dist 'MANIFESTO.txt'
Escrever-Texto -Caminho $manifesto -Linhas $resumo
Get-Content $manifesto
