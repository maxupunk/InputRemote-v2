# Remove a PoC-1 sem deixar resíduo. Precisa de administrador.
#
# Desinstalar limpo faz parte da PoC: um serviço SYSTEM que sobra depois do teste é o tipo de
# coisa que ninguém lembra de tirar, e que continua rodando meses depois.

#Requires -RunAsAdministrator
$ErrorActionPreference = 'Continue'

$destino = Join-Path $env:ProgramFiles 'poc1'

# --- Para e remove o serviço --------------------------------------------------------------
$servico = Get-Service -Name 'poc1' -ErrorAction SilentlyContinue
if ($servico) {
    if ($servico.Status -ne 'Stopped') {
        Stop-Service -Name 'poc1' -Force
        Write-Host 'Serviço parado.'
    }
    sc.exe delete poc1 | Out-Null
    Write-Host 'Serviço removido.'
} else {
    Write-Host 'Serviço não estava registrado.'
}

# --- Mata o agente, que sobrevive ao serviço ----------------------------------------------
$agentes = Get-Process -Name 'poc1-agent' -ErrorAction SilentlyContinue
if ($agentes) {
    $agentes | Stop-Process -Force
    Write-Host "Agente encerrado ($($agentes.Count) processo(s))."
}

# --- Remove os arquivos -------------------------------------------------------------------
if (Test-Path $destino) {
    Remove-Item -Recurse -Force $destino
    Write-Host "Removido $destino"
}

# O registro fica de propósito: é o resultado da PoC, e apagá-lo junto com a instalação seria
# jogar fora justamente o que se foi medir.
$log = Join-Path $env:ProgramData 'poc1\poc1.log'
if (Test-Path $log) {
    Write-Host ''
    Write-Host "O registro ficou em $log" -ForegroundColor Cyan
    Write-Host 'Apague à mão depois de anotar o resultado em LOG.md.'
}
