<#
.SYNOPSIS
  Deploy escrow and test-token contracts, then run init calls.
.PARAMETER WasmDir
  Directory with WASM files.
.PARAMETER Force
  Redeploy even if code is already on chain.
.PARAMETER DryRun
  Print near commands without sending transactions.
.EXAMPLE
  .\3-deploy.ps1
#>
[CmdletBinding()]
param(
    [string]$WasmDir,
    [switch]$Force,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')
if ($DryRun -and -not $env:TERRA_STATE_FILE) {
    # a dry run never touches the real state file
    $Script:StateFile = Join-Path $PSScriptRoot 'state.dry-run.json'
}

Start-Log -Name '3-deploy' | Out-Null

$state = Read-State
if ($null -eq $state) {
    throw 'state.json missing; run 1-create-accounts.ps1 first'
}

if (-not $WasmDir) {
    $WasmDir = Get-DefaultWasmDir
}

Invoke-CheckWasm -WasmDir $WasmDir -Build:$false -DryRun:$DryRun

$escrow = $state.escrow_account
$base = $state.base_token
$quote = $state.quote_token

foreach ($id in @($escrow, $base, $quote)) {
    Assert-AccountId -AccountId $id
}

$escrowWasm = Join-Path $WasmDir 'escrow.wasm'
$tokenWasm = Join-Path $WasmDir 'terra_test_token.wasm'
$escrowBytes = (Get-WasmSizeBytes -WasmDir $WasmDir -FileName 'escrow.wasm')
$tokenBytes = (Get-WasmSizeBytes -WasmDir $WasmDir -FileName 'terra_test_token.wasm')
$escrowMin = [math]::Round($escrowBytes * 0.00001 + 1, 4)
$tokenMin = [math]::Round($tokenBytes * 0.00001 + 1, 4)

if (-not $DryRun) {
    $escrowBal = Get-NearBalance -AccountId $escrow
    if ($escrowBal -lt $escrowMin) {
        throw "escrow account needs at least $escrowMin NEAR (has $escrowBal)"
    }
    foreach ($pair in @(@{ Id = $base; Min = $tokenMin }, @{ Id = $quote; Min = $tokenMin })) {
        $bal = Get-NearBalance -AccountId $pair.Id
        if ($bal -lt $pair.Min) {
            throw "$($pair.Id) needs at least $($pair.Min) NEAR (has $bal)"
        }
    }
}

function Deploy-Contract {
    param(
        [string]$Account,
        [string]$WasmPath,
        [string]$InitJson
    )
    if ((-not $Force) -and (Test-ContractDeployed -AccountId $Account)) {
        Write-Host "contract already deployed, skipping: $Account"
        return
    }
    Invoke-Near -NearArgs @(
        'contract', 'deploy', $Account, 'use-file', $WasmPath, 'without-init-call',
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun
    Invoke-Near -NearArgs @(
        'contract', 'call-function', 'as-transaction', $Account, 'new', 'json-args', $InitJson,
        'prepaid-gas', '30 Tgas', 'attached-deposit', '0 NEAR', 'sign-as', $Account,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun
}

Deploy-Contract -Account $escrow -WasmPath $escrowWasm -InitJson '{}'
Deploy-Contract -Account $base -WasmPath $tokenWasm -InitJson '{"symbol":"TBASE"}'
Deploy-Contract -Account $quote -WasmPath $tokenWasm -InitJson '{"symbol":"TQUOTE"}'

if (-not $DryRun) {
    $baseMeta = Invoke-View -Contract $base -Method 'ft_metadata' -ArgsObject @{} -DryRun:$false
    if ($null -eq $baseMeta -or $baseMeta.symbol -ne 'TBASE') {
        throw 'base token ft_metadata symbol check failed'
    }
    $quoteMeta = Invoke-View -Contract $quote -Method 'ft_metadata' -ArgsObject @{} -DryRun:$false
    if ($null -eq $quoteMeta -or $quoteMeta.symbol -ne 'TQUOTE') {
        throw 'quote token ft_metadata symbol check failed'
    }
    # the escrow must answer a view call (the value depends on whether step 4 already ran)
    [void](Invoke-View -Contract $escrow -Method 'get_key' -ArgsObject @{ account_id = $state.alice_account } -DryRun:$false)
}

Write-Host 'NEXT: run 4-register-keys.ps1'
