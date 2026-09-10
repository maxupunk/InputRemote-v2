#Requires -Version 5.1
<#
.SYNOPSIS
Certificado autoassinado e assinatura Authenticode, para teste e uso local.

.DESCRIPTION
Isto NAO substitui um certificado de verdade. Um certificado autoassinado vale na maquina onde
voce mandar confiar nele, e em lugar nenhum mais. Serve para dois propositos concretos:

  1. exercitar o caminho de assinatura do empacotador antes de existir certificado comprado;
  2. permitir testar UIAccess -- que exige binario assinado -- numa maquina de bancada.

A chave privada fica na loja do usuario e e criada como NAO exportavel. Nao existe .pfx em disco
para vazar; o que se exporta e so a parte publica.

-Acao Confiar instala a parte publica em Raizes Confiaveis da MAQUINA. Isso e uma decisao de
seguranca de verdade: dali em diante, qualquer binario assinado com essa chave passa a ser
confiavel neste computador. Faca numa bancada, nao na maquina de trabalho, e use -Acao Remover
quando terminar.

.PARAMETER Acao
Criar   cria o certificado se ainda nao existir e exporta a parte publica
Assinar assina os arquivos de -Arquivos
Confiar instala a parte publica nas lojas da maquina (exige administrador)
Remover tira o certificado de todas as lojas (exige administrador)
Estado  diz o que existe hoje

.PARAMETER Arquivos
Os arquivos a assinar, com -Acao Assinar.
#>
param(
    [ValidateSet('Criar', 'Assinar', 'Confiar', 'Remover', 'Estado')]
    [string]$Acao = 'Estado',

    [string[]]$Arquivos = @()
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$assunto = 'CN=InputRemote (certificado de teste), O=Projeto InputRemote'
$amigavel = 'InputRemote - assinatura de teste'
$pastaDoCertificado = Join-Path $PSScriptRoot 'certificado'
$arquivoPublico = Join-Path $pastaDoCertificado 'inputremote-teste.cer'

function Escrever-Passo([string]$texto) { Write-Host "==> $texto" -ForegroundColor Cyan }
function Escrever-Aviso([string]$texto) { Write-Host "!!! $texto" -ForegroundColor Yellow }

function Testar-Administrador {
    $identidade = [Security.Principal.WindowsIdentity]::GetCurrent()
    $papel = New-Object Security.Principal.WindowsPrincipal($identidade)
    return $papel.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Achar-Certificado {
    return Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Subject -eq $assunto -and $_.NotAfter -gt (Get-Date) } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
}

function Achar-Signtool {
    $comando = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($comando) { return $comando.Source }

    $kits = @("${env:ProgramFiles(x86)}\Windows Kits\10\bin", "$env:ProgramFiles\Windows Kits\10\bin")
    foreach ($kit in $kits) {
        if (-not (Test-Path $kit)) { continue }
        $achado = Get-ChildItem $kit -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($achado) { return $achado.FullName }
    }
    return $null
}

function Criar-Certificado {
    $existente = Achar-Certificado
    if ($existente) {
        Escrever-Passo "certificado ja existe, valido ate $($existente.NotAfter.ToString('yyyy-MM-dd'))"
        $certificado = $existente
    } else {
        Escrever-Passo 'criando o certificado autoassinado'
        # Chave nao exportavel: sem .pfx em disco, nao ha o que vazar de uma pasta de build.
        $certificado = New-SelfSignedCertificate `
            -Type CodeSigningCert `
            -Subject $assunto `
            -FriendlyName $amigavel `
            -KeyAlgorithm RSA `
            -KeyLength 3072 `
            -HashAlgorithm SHA256 `
            -KeyExportPolicy NonExportable `
            -CertStoreLocation Cert:\CurrentUser\My `
            -NotAfter (Get-Date).AddYears(3)
    }

    New-Item -ItemType Directory -Force -Path $pastaDoCertificado | Out-Null
    Export-Certificate -Cert $certificado -FilePath $arquivoPublico -Force | Out-Null
    Escrever-Passo "parte publica em $arquivoPublico"
    return $certificado
}

function Assinar-Arquivos([string[]]$alvos) {
    if ($alvos.Count -eq 0) { throw 'nada para assinar: informe -Arquivos' }

    $certificado = Achar-Certificado
    if (-not $certificado) { throw "certificado nao encontrado; rode antes: assinar.ps1 -Acao Criar" }

    $signtool = Achar-Signtool
    foreach ($alvo in $alvos) {
        if (-not (Test-Path $alvo)) { throw "arquivo inexistente: $alvo" }

        if ($signtool) {
            # Sem carimbo de tempo de proposito. Carimbo serve para a assinatura sobreviver ao
            # vencimento do certificado, e um certificado de bancada que vence em tres anos nao
            # tem nada a preservar -- em compensacao, exige rede e falha a build quando a
            # autoridade de carimbo esta fora do ar.
            # A saida do signtool e guardada e so aparece se ele falhar. Em dia de sucesso ela
            # e ruido ("Done Adding Additional Store"); em dia de falha e a unica pista.
            $anterior = $ErrorActionPreference
            $ErrorActionPreference = 'Continue'
            try {
                $saida = & $signtool sign /fd SHA256 /sha1 $certificado.Thumbprint /q $alvo 2>&1
            } finally {
                $ErrorActionPreference = $anterior
            }
            if ($LASTEXITCODE -ne 0) {
                foreach ($linha in $saida) { Write-Host "    $linha" -ForegroundColor Red }
                throw "signtool falhou em $alvo (codigo $LASTEXITCODE)"
            }
        } else {
            Set-AuthenticodeSignature -FilePath $alvo -Certificate $certificado -HashAlgorithm SHA256 | Out-Null
        }

        $estado = (Get-AuthenticodeSignature -FilePath $alvo).Status
        Write-Host "    assinado: $(Split-Path -Leaf $alvo)  [$estado]"
    }

    if (-not $signtool) {
        Escrever-Aviso 'signtool nao encontrado; assinado por Set-AuthenticodeSignature'
    }
}

function Confiar-No-Certificado {
    if (-not (Testar-Administrador)) {
        throw 'precisa ser administrador: instalar em Raizes Confiaveis mexe na loja da maquina'
    }
    if (-not (Test-Path $arquivoPublico)) { throw "nao achei $arquivoPublico; rode antes: -Acao Criar" }

    Escrever-Aviso 'a partir de agora, QUALQUER binario assinado com esta chave sera confiavel nesta maquina'
    Escrever-Aviso 'faca isso numa bancada de teste; para desfazer, use: assinar.ps1 -Acao Remover'

    foreach ($loja in @('Root', 'TrustedPublisher')) {
        Import-Certificate -FilePath $arquivoPublico -CertStoreLocation "Cert:\LocalMachine\$loja" | Out-Null
        Escrever-Passo "instalado em LocalMachine\$loja"
    }
}

function Remover-Certificado {
    if (-not (Testar-Administrador)) {
        throw 'precisa ser administrador: a remocao mexe nas lojas da maquina'
    }
    $lojas = @('Cert:\LocalMachine\Root', 'Cert:\LocalMachine\TrustedPublisher', 'Cert:\CurrentUser\My')
    foreach ($loja in $lojas) {
        $achados = @(Get-ChildItem $loja -ErrorAction SilentlyContinue | Where-Object { $_.Subject -eq $assunto })
        foreach ($certificado in $achados) {
            Remove-Item $certificado.PSPath -Force
            Escrever-Passo "removido de $loja"
        }
    }
    if (Test-Path $pastaDoCertificado) { Remove-Item -Recurse -Force $pastaDoCertificado }
    Escrever-Passo 'removido'
}

function Mostrar-Estado {
    $certificado = Achar-Certificado
    if ($certificado) {
        Write-Host "certificado:  $($certificado.Thumbprint)"
        Write-Host "valido ate:   $($certificado.NotAfter.ToString('yyyy-MM-dd'))"
    } else {
        Write-Host 'certificado:  nenhum (rode: assinar.ps1 -Acao Criar)'
    }

    foreach ($loja in @('Root', 'TrustedPublisher')) {
        $confiavel = @(Get-ChildItem "Cert:\LocalMachine\$loja" -ErrorAction SilentlyContinue |
            Where-Object { $_.Subject -eq $assunto })
        $situacao = if ($confiavel.Count -gt 0) { 'sim' } else { 'nao' }
        Write-Host "confiavel em LocalMachine\$($loja): $situacao"
    }

    $signtool = Achar-Signtool
    Write-Host "signtool:     $(if ($signtool) { $signtool } else { 'nao encontrado' })"
}

switch ($Acao) {
    'Criar'   { Criar-Certificado | Out-Null }
    'Assinar' { Assinar-Arquivos $Arquivos }
    'Confiar' { Confiar-No-Certificado }
    'Remover' { Remover-Certificado }
    'Estado'  { Mostrar-Estado }
}
