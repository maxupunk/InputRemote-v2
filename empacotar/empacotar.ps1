#Requires -Version 5.1
<#
.SYNOPSIS
Monta o pacote do InputRemote para Windows.

.DESCRIPTION
Compila em release, junta os binarios que existem, escreve um manifesto com a impressao digital
de cada arquivo e produz um .zip em dist/.

O manifesto lista tambem o que **nao** entrou. Um pacote que omite o que falta e um pacote que
mente: quem o instalar vai descobrir a ausencia na tela de bloqueio, que e o pior lugar.

.PARAMETER PularCompilacao
Usa os binarios que ja estiverem em target/release, sem recompilar.
#>
param(
    [switch]$PularCompilacao
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$raiz = Split-Path -Parent $PSScriptRoot
$release = Join-Path $raiz 'target\release'

# Os tres binarios do produto, na ordem de docs/02-arquitetura.md. `Obrigatorio` marca o que
# precisa existir para o produto cumprir o requisito R1 (digitar na tela de bloqueio).
$componentes = @(
    @{ Arquivo = 'inputremote-daemon.exe'; Papel = 'servico privilegiado'; Obrigatorio = $true },
    @{ Arquivo = 'inputremote-agent.exe';  Papel = 'agente de sessao';     Obrigatorio = $true },
    @{ Arquivo = 'inputremote-ui.exe';     Papel = 'interface';            Obrigatorio = $false }
)

function Escrever-Passo([string]$texto) {
    Write-Host "==> $texto" -ForegroundColor Cyan
}

function Ler-Versao {
    $manifesto = Join-Path $raiz 'Cargo.toml'
    $linha = Select-String -Path $manifesto -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $linha) { throw "nao achei a versao em $manifesto" }
    return $linha.Matches[0].Groups[1].Value
}

function Ler-Commit {
    try {
        $commit = & git -C $raiz rev-parse --short HEAD 2>$null
        if ($LASTEXITCODE -ne 0) { return 'sem git' }
        $sujo = & git -C $raiz status --porcelain 2>$null
        if ($sujo) { return "$commit (arvore suja)" }
        return $commit
    } catch {
        return 'sem git'
    }
}

function Testar-Assinatura([string]$caminho) {
    $assinatura = Get-AuthenticodeSignature -FilePath $caminho
    return $assinatura.Status -eq 'Valid'
}

# --- Compilacao -------------------------------------------------------------------------------

if (-not $PularCompilacao) {
    Escrever-Passo 'compilando em release'
    & cargo build --release --manifest-path (Join-Path $raiz 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'a compilacao falhou' }
} else {
    Escrever-Passo 'pulando a compilacao a pedido'
}

# --- Coleta -----------------------------------------------------------------------------------

$versao = Ler-Versao
$commit = Ler-Commit
$nomeDoPacote = "InputRemote-$versao-windows-x64"
$destino = Join-Path $raiz "dist\$nomeDoPacote"

if (Test-Path $destino) { Remove-Item -Recurse -Force $destino }
New-Item -ItemType Directory -Force -Path $destino | Out-Null

$presentes = @()
$ausentes = @()

foreach ($componente in $componentes) {
    $origem = Join-Path $release $componente.Arquivo
    if (Test-Path $origem) {
        Copy-Item $origem -Destination $destino
        $presentes += $componente
    } else {
        $ausentes += $componente
    }
}

if ($presentes.Count -eq 0) {
    throw "nenhum binario do produto foi encontrado em $release"
}

Escrever-Passo "$($presentes.Count) componente(s) no pacote, $($ausentes.Count) ausente(s)"

foreach ($extra in @('instalar.ps1', 'desinstalar.ps1', 'LEIAME.txt')) {
    Copy-Item (Join-Path $PSScriptRoot $extra) -Destination $destino
}
Copy-Item (Join-Path $raiz 'LICENSE') -Destination (Join-Path $destino 'LICENSE.txt')

# --- Manifesto --------------------------------------------------------------------------------

$linhas = New-Object System.Collections.Generic.List[string]
$linhas.Add("InputRemote $versao - pacote para Windows x64")
$linhas.Add("commit: $commit")
$linhas.Add("montado em: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz')")
$linhas.Add('')
$linhas.Add('CONTEUDO')

foreach ($componente in $presentes) {
    $caminho = Join-Path $destino $componente.Arquivo
    $resumo = (Get-FileHash -Algorithm SHA256 -Path $caminho).Hash.ToLower()
    $tamanho = [math]::Round((Get-Item $caminho).Length / 1MB, 2)
    $assinado = if (Testar-Assinatura $caminho) { 'assinado' } else { 'NAO ASSINADO' }
    $linhas.Add("  $($componente.Arquivo)  ($($componente.Papel), $tamanho MB, $assinado)")
    $linhas.Add("    sha256 $resumo")
}

$linhas.Add('')
if ($ausentes.Count -gt 0) {
    $linhas.Add('NAO INCLUIDO NESTE PACOTE')
    foreach ($componente in $ausentes) {
        $peso = if ($componente.Obrigatorio) { 'exigido pelo requisito R1' } else { 'opcional' }
        $linhas.Add("  $($componente.Arquivo)  ($($componente.Papel), $peso) - ainda nao implementado")
    }
    $linhas.Add('')
    $linhas.Add('Sem o servico e o agente, este pacote instala apenas a interface, e ela roda')
    $linhas.Add('em modo de demonstracao contra um servico simulado. Nada e digitado em')
    $linhas.Add('computador nenhum. A propria janela avisa isso em cima.')
} else {
    $linhas.Add('Pacote completo: servico, agente e interface.')
}

$naoAssinados = @($presentes | Where-Object { -not (Testar-Assinatura (Join-Path $destino $_.Arquivo)) })
if ($naoAssinados.Count -gt 0) {
    $linhas.Add('')
    $linhas.Add('AVISO DE ASSINATURA')
    $linhas.Add('Ha binarios sem assinatura Authenticode. No Windows, o agente so recebe')
    $linhas.Add('UIAccess se estiver assinado e numa pasta protegida; sem isso a digitacao na')
    $linhas.Add('tela de bloqueio nao funciona, e o nivel de capacidade fica em N1 ou N0.')
    $linhas.Add('Ver docs/05-windows.md, secao 4.4.')
}

$manifesto = Join-Path $destino 'MANIFESTO.txt'
Set-Content -Path $manifesto -Value $linhas -Encoding utf8

# --- Zip --------------------------------------------------------------------------------------

$zip = Join-Path $raiz "dist\$nomeDoPacote.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$destino\*" -DestinationPath $zip

Escrever-Passo 'pronto'
Get-Content $manifesto
Write-Host ''
Write-Host "pasta: $destino"
Write-Host "zip:   $zip"
