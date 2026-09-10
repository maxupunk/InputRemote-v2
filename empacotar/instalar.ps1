#Requires -Version 5.1
<#
.SYNOPSIS
Instala o InputRemote nesta maquina.

.DESCRIPTION
Copia os binarios para %ProgramFiles%\InputRemote, restringe a escrita a administradores e ao
SYSTEM, cria o atalho no menu iniciar e registra o servico -- quando o servico existir no pacote.

A pasta protegida nao e capricho. No Windows, um processo so recebe UIAccess se estiver assinado
E numa pasta que usuarios comuns nao possam escrever. Instalar em pasta gravavel faz o passo do
token passar sem o privilegio ser concedido, e o produto falha depois, na tela de bloqueio.
Ver docs/05-windows.md, secao 4.4.

.PARAMETER Destino
Onde instalar. O padrao e %ProgramFiles%\InputRemote e mudar isso provavelmente quebra o
UIAccess.

.PARAMETER SemAtalho
Nao cria o atalho no menu iniciar.
#>
param(
    [string]$Destino = (Join-Path $env:ProgramFiles 'InputRemote'),
    [switch]$SemAtalho
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$origem = $PSScriptRoot
$nomeDoServico = 'InputRemote'

function Escrever-Passo([string]$texto) { Write-Host "==> $texto" -ForegroundColor Cyan }
function Escrever-Aviso([string]$texto) { Write-Host "!!! $texto" -ForegroundColor Yellow }

function Testar-Administrador {
    $identidade = [Security.Principal.WindowsIdentity]::GetCurrent()
    $papel = New-Object Security.Principal.WindowsPrincipal($identidade)
    return $papel.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Testar-Administrador)) {
    throw 'precisa ser executado como administrador: a instalacao escreve em %ProgramFiles% e registra um servico'
}

# --- O que ha no pacote -----------------------------------------------------------------------

$servico = Join-Path $origem 'inputremote-daemon.exe'
$agente = Join-Path $origem 'inputremote-agent.exe'
$interface = Join-Path $origem 'inputremote-ui.exe'

$temServico = Test-Path $servico
$temAgente = Test-Path $agente
$temInterface = Test-Path $interface

if (-not ($temServico -or $temAgente -or $temInterface)) {
    throw "nenhum binario do InputRemote encontrado em $origem"
}

# --- Parar o que estiver rodando ----------------------------------------------------------------

if (Get-Service -Name $nomeDoServico -ErrorAction SilentlyContinue) {
    Escrever-Passo 'parando a instalacao anterior'
    try { Stop-Service -Name $nomeDoServico -Force -ErrorAction Stop } catch {}
}
Get-Process -Name 'inputremote-ui', 'inputremote-agent' -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

# --- Copia ---------------------------------------------------------------------------------------

Escrever-Passo "instalando em $Destino"
New-Item -ItemType Directory -Force -Path $Destino | Out-Null

foreach ($arquivo in Get-ChildItem -Path $origem -File) {
    if ($arquivo.Extension -in @('.exe', '.txt')) {
        Copy-Item $arquivo.FullName -Destination $Destino -Force
    }
}

# O desinstalador vai junto. Sem ele aqui, so consegue desinstalar quem ainda tiver o zip -- e
# quem perdeu o zip fica com um servico privilegiado que nao sabe como tirar.
Copy-Item (Join-Path $origem 'desinstalar.ps1') -Destination $Destino -Force

# --- ACL ------------------------------------------------------------------------------------------

# Por SID, e nao por nome: "Administrators" nao existe num Windows em portugues, onde o grupo se
# chama "Administradores". Um instalador que so funciona no Windows em ingles falha na maquina de
# quem mais precisa dele -- e falha depois de ja ter copiado os binarios.
Escrever-Passo 'restringindo a escrita a administradores e ao SYSTEM'
$administradores = '*S-1-5-32-544'   # grupo interno Administradores
$sistema = '*S-1-5-18'               # conta do sistema operacional
$usuarios = '*S-1-5-32-545'          # grupo interno Usuarios
& icacls $Destino /inheritance:r /grant:r "$($administradores):(OI)(CI)F" "$($sistema):(OI)(CI)F" "$($usuarios):(OI)(CI)RX" | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'icacls falhou; a pasta ficaria gravavel e o UIAccess nao seria concedido' }

# --- Assinatura -----------------------------------------------------------------------------------

$semAssinatura = @()
foreach ($executavel in Get-ChildItem -Path $Destino -Filter '*.exe') {
    if ((Get-AuthenticodeSignature -FilePath $executavel.FullName).Status -ne 'Valid') {
        $semAssinatura += $executavel.Name
    }
}
if ($semAssinatura.Count -gt 0) {
    Escrever-Aviso "sem assinatura Authenticode: $($semAssinatura -join ', ')"
    Escrever-Aviso 'o agente nao vai receber UIAccess, e digitar na tela de bloqueio nao vai funcionar'
    Escrever-Aviso 'para um teste da interface isso nao atrapalha; para o produto, atrapalha tudo'
}

# --- Servico --------------------------------------------------------------------------------------

if ($temServico) {
    Escrever-Passo 'registrando o servico'
    $caminhoDoServico = Join-Path $Destino 'inputremote-daemon.exe'
    if (Get-Service -Name $nomeDoServico -ErrorAction SilentlyContinue) {
        & sc.exe config $nomeDoServico binPath= "`"$caminhoDoServico`"" start= auto | Out-Null
    } else {
        & sc.exe create $nomeDoServico binPath= "`"$caminhoDoServico`"" start= auto DisplayName= 'InputRemote' | Out-Null
    }
    if ($LASTEXITCODE -ne 0) { throw 'sc.exe falhou ao registrar o servico' }
    & sc.exe description $nomeDoServico 'Compartilha teclado e mouse com outro computador.' | Out-Null
    Start-Service -Name $nomeDoServico
    Escrever-Passo 'servico no ar'
} else {
    Escrever-Aviso 'o servico (inputremote-daemon.exe) nao esta neste pacote'
    Escrever-Aviso 'a interface vai abrir em modo de demonstracao, contra um servico simulado'
    Escrever-Aviso 'nada e digitado em computador nenhum, e a propria janela avisa isso'
}

if (-not $temAgente) {
    Escrever-Aviso 'o agente (inputremote-agent.exe) nao esta neste pacote'
}

# --- Atalho ----------------------------------------------------------------------------------------

if ($temInterface -and -not $SemAtalho) {
    Escrever-Passo 'criando o atalho no menu iniciar'
    $menu = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs'
    $atalho = Join-Path $menu 'InputRemote.lnk'
    $shell = New-Object -ComObject WScript.Shell
    $item = $shell.CreateShortcut($atalho)
    $item.TargetPath = Join-Path $Destino 'inputremote-ui.exe'
    $item.WorkingDirectory = $Destino
    $item.Description = 'Compartilha teclado e mouse com outro computador'
    $item.Save()
}

Escrever-Passo 'instalado'
Write-Host ''
Write-Host "  pasta:     $Destino"
if ($temInterface) { Write-Host '  interface: menu iniciar > InputRemote' }
if ($temServico)   { Write-Host "  servico:   $nomeDoServico (automatico)" }
Write-Host ''
Write-Host '  para remover: desinstalar.ps1 (como administrador)'
