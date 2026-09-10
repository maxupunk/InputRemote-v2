#Requires -Version 5.1
<#
.SYNOPSIS
Remove o InputRemote desta maquina.

.DESCRIPTION
Para e apaga o servico, encerra os processos, remove a pasta de instalacao e o atalho.

A configuracao e o registro de diagnostico ficam, salvo pedido explicito: quem desinstala para
resolver um problema costuma reinstalar em seguida, e jogar fora o registro junto com o programa
apaga justamente o que explicaria a falha.

.PARAMETER Destino
Onde esta instalado. O padrao e %ProgramFiles%\InputRemote.

.PARAMETER ApagarConfiguracao
Remove tambem a configuracao e o registro de diagnostico.
#>
param(
    [string]$Destino = (Join-Path $env:ProgramFiles 'InputRemote'),
    [switch]$ApagarConfiguracao
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$nomeDoServico = 'InputRemote'
$dados = Join-Path $env:ProgramData 'InputRemote'

function Escrever-Passo([string]$texto) { Write-Host "==> $texto" -ForegroundColor Cyan }

function Testar-Administrador {
    $identidade = [Security.Principal.WindowsIdentity]::GetCurrent()
    $papel = New-Object Security.Principal.WindowsPrincipal($identidade)
    return $papel.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Testar-Administrador)) {
    throw 'precisa ser executado como administrador'
}

if (Get-Service -Name $nomeDoServico -ErrorAction SilentlyContinue) {
    Escrever-Passo 'parando e removendo o servico'
    try { Stop-Service -Name $nomeDoServico -Force -ErrorAction Stop } catch {}
    & sc.exe delete $nomeDoServico | Out-Null
}

Escrever-Passo 'encerrando os processos'
Get-Process -Name 'inputremote-ui', 'inputremote-agent' -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue

$atalho = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\InputRemote.lnk'
if (Test-Path $atalho) {
    Escrever-Passo 'removendo o atalho'
    Remove-Item -Force $atalho
}

if (Test-Path $Destino) {
    Escrever-Passo "removendo $Destino"
    Remove-Item -Recurse -Force $Destino
}

if ($ApagarConfiguracao) {
    if (Test-Path $dados) {
        Escrever-Passo "removendo a configuracao e o registro em $dados"
        Remove-Item -Recurse -Force $dados
    }
} elseif (Test-Path $dados) {
    Write-Host "    configuracao e registro preservados em $dados"
    Write-Host '    para apagar tambem: desinstalar.ps1 -ApagarConfiguracao'
}

Escrever-Passo 'removido'
