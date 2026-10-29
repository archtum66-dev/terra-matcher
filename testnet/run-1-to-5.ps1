<#
.SYNOPSIS
  Run testnet setup steps 1 through 5 in order.
.PARAMETER Prefix
  Account prefix for step 1.
.PARAMETER DryRun
  Pass -DryRun to each step.
.PARAMETER WasmDir
  Optional WASM directory for steps 2 and 3.
.PARAMETER KeysDir
  Optional keys directory for step 4.
.EXAMPLE
  .\run-1-to-5.ps1 -Prefix terra1
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Prefix,
    [switch]$DryRun,
    [string]$WasmDir,
    [string]$KeysDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Step {
    param(
        [string]$ScriptName,
        [hashtable]$Extra
    )
    $path = Join-Path $PSScriptRoot $ScriptName
    $params = @{}
    if ($DryRun) {
        $params['DryRun'] = $true
    }
    foreach ($k in $Extra.Keys) {
        $params[$k] = $Extra[$k]
    }
    & $path @params
    if ($LASTEXITCODE -and $LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

$step1 = @{ Prefix = $Prefix }
Invoke-Step -ScriptName '1-create-accounts.ps1' -Extra $step1

$step2 = @{}
if ($WasmDir) {
    $step2['WasmDir'] = $WasmDir
}
Invoke-Step -ScriptName '2-check-wasm.ps1' -Extra $step2

$step3 = @{}
if ($WasmDir) {
    $step3['WasmDir'] = $WasmDir
}
Invoke-Step -ScriptName '3-deploy.ps1' -Extra $step3

$step4 = @{}
if ($KeysDir) {
    $step4['KeysDir'] = $KeysDir
}
Invoke-Step -ScriptName '4-register-keys.ps1' -Extra $step4

Invoke-Step -ScriptName '5-deposit.ps1' -Extra @{}

Write-Host 'All five steps finished.'
