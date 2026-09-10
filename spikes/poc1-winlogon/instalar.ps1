# Instala a PoC-1. Precisa de administrador.
#
# O destino é %ProgramFiles% de propósito: o UIAccess só é concedido a binário assinado em
# local gravável apenas por administradores (docs/05-windows.md §4.4). Instalar em %TEMP% ou
# na pasta do repositório faria o passo do TokenUIAccess passar e o privilégio não ser
# concedido — e o resultado da PoC seria um falso negativo.

param(
    # Qual das quatro configurações da matriz de origem confiável testar.
    [ValidateSet('system-uiaccess', 'system', 'elevated', 'user')]
    [string]$Modo = 'system-uiaccess'
)

#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'


$destino = Join-Path $env:ProgramFiles 'poc1'
$origem = Join-Path $PSScriptRoot 'target\release'

if (-not (Test-Path (Join-Path $origem 'poc1-service.exe'))) {
    Write-Host 'Compile primeiro: cargo build --release' -ForegroundColor Yellow
    exit 1
}

# --- Confere o build antes de qualquer coisa ---------------------------------------------
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$build = [int]$cv.CurrentBuild
$ubr = [int]$cv.UBR
Write-Host "Build do Windows: $build.$ubr"

$minimos = @{ 22631 = 6491; 26100 = 7623; 26200 = 7623 }
if ($minimos.ContainsKey($build) -and $ubr -lt $minimos[$build]) {
    Write-Host ''
    Write-Host 'ATENÇÃO: este build é ANTERIOR ao endurecimento de janeiro de 2026.' -ForegroundColor Red
    Write-Host 'O resultado da PoC não vale — ela aprovaria aqui e o produto falharia depois.' -ForegroundColor Red
    Write-Host "Mínimo para este ramo: $build.$($minimos[$build])" -ForegroundColor Red
    Write-Host ''
    exit 1
}

# --- Copia para local seguro -------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $destino | Out-Null
Copy-Item (Join-Path $origem 'poc1-service.exe') $destino -Force
Copy-Item (Join-Path $origem 'poc1-agent.exe') $destino -Force

# Só administradores e o SYSTEM podem escrever, que é o que o UIAccess exige.
icacls $destino /inheritance:r /grant:r 'BUILTIN\Administrators:(OI)(CI)F' /grant:r 'NT AUTHORITY\SYSTEM:(OI)(CI)F' /grant:r 'BUILTIN\Users:(OI)(CI)RX' | Out-Null
Write-Host "Instalado em $destino"

# --- Registra o serviço ------------------------------------------------------------------
$existente = Get-Service -Name 'poc1' -ErrorAction SilentlyContinue
if ($existente) {
    Write-Host 'Serviço já existe; removendo antes de reinstalar.'
    & (Join-Path $PSScriptRoot 'desinstalar.ps1')
}

$exe = Join-Path $destino 'poc1-service.exe'
sc.exe create poc1 binPath= "`"$exe`"" start= demand obj= 'LocalSystem' DisplayName= 'InputRemote PoC-1' | Out-Null

# O modo vai por variável de ambiente do serviço.
$chave = 'HKLM:\SYSTEM\CurrentControlSet\Services\poc1'
Set-ItemProperty -Path $chave -Name 'Environment' -Value @("POC1_MODE=$Modo") -Type MultiString

Write-Host "Serviço registrado no modo: $Modo"
Write-Host ''
Write-Host 'Assinatura:' -ForegroundColor Cyan
$assinatura = Get-AuthenticodeSignature $exe
if ($assinatura.Status -ne 'Valid') {
    Write-Host "  NÃO ASSINADO ($($assinatura.Status))." -ForegroundColor Yellow
    Write-Host '  O UIAccess não vai ser concedido. Isso é um resultado válido a registrar,' -ForegroundColor Yellow
    Write-Host '  mas não é a configuração (a) da matriz — ela exige assinatura.' -ForegroundColor Yellow
} else {
    Write-Host "  válida: $($assinatura.SignerCertificate.Subject)" -ForegroundColor Green
}

Write-Host ''
Write-Host 'Agora:' -ForegroundColor Cyan
Write-Host '  1. Start-Service poc1'
Write-Host '  2. rundll32.exe user32.dll,LockWorkStation'
Write-Host '  3. espere 20 s olhando a tela: a sequência `poc1` deve aparecer no campo de senha'
Write-Host "  4. Get-Content `"$env:ProgramData\poc1\poc1.log`""
