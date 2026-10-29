<#
.SYNOPSIS
  Verify or build escrow and test-token WASM artifacts.
.PARAMETER WasmDir
  Directory containing escrow.wasm and terra_test_token.wasm.
.PARAMETER Build
  Run cargo build when WASM files are missing.
.PARAMETER DryRun
  Skip cargo build execution and RPC-heavy steps where applicable.
.EXAMPLE
  .\2-check-wasm.ps1 -Build
#>
[CmdletBinding()]
param(
    [string]$WasmDir,
    [switch]$Build,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')

Start-Log -Name '2-check-wasm' | Out-Null

if (-not $WasmDir) {
    $WasmDir = Get-DefaultWasmDir
}

Invoke-CheckWasm -WasmDir $WasmDir -Build:$Build -DryRun:$DryRun

Write-Host 'NEXT: run 3-deploy.ps1'
