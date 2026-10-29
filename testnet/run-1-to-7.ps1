<#
.SYNOPSIS
  Run testnet setup steps 1 through 7 in order.
.PARAMETER Prefix
  Account prefix for step 1.
.PARAMETER DryRun
  Pass -DryRun to each step.
.PARAMETER WasmDir
  Optional WASM directory for steps 2 and 3.
.PARAMETER KeysDir
  Optional keys directory for step 4.
.EXAMPLE
  .\run-1-to-7.ps1 -Prefix terra1
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
    $global:LASTEXITCODE = 0
    & $path @params
    if ($global:LASTEXITCODE -ne 0) {
        exit $global:LASTEXITCODE
    }
}

$run5 = Join-Path $PSScriptRoot 'run-1-to-5.ps1'
$params5 = @{ Prefix = $Prefix }
if ($DryRun) {
    $params5['DryRun'] = $true
}
if ($WasmDir) {
    $params5['WasmDir'] = $WasmDir
}
if ($KeysDir) {
    $params5['KeysDir'] = $KeysDir
}
$global:LASTEXITCODE = 0
& $run5 @params5
if ($global:LASTEXITCODE -ne 0) {
    exit $global:LASTEXITCODE
}

Invoke-Step -ScriptName '6-settle.ps1' -Extra @{}
Invoke-Step -ScriptName '7-withdraw.ps1' -Extra @{}

Write-Host 'All seven steps finished.'
