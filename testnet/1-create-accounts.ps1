<#
.SYNOPSIS
  Create five testnet accounts for the escrow kit.
.PARAMETER Prefix
  Account name prefix (e.g. terra1).
.PARAMETER DryRun
  Print near commands without executing them.
.EXAMPLE
  .\1-create-accounts.ps1 -Prefix terra1
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Prefix,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')
if ($DryRun -and -not $env:TERRA_STATE_FILE) {
    # a dry run never touches the real state file
    $Script:StateFile = Join-Path $PSScriptRoot 'state.dry-run.json'
}

Start-Log -Name '1-create-accounts' | Out-Null

if ($Prefix -notmatch '^[a-z0-9][a-z0-9-]{2,24}$') {
    throw 'Prefix must match ^[a-z0-9][a-z0-9-]{2,24}$'
}

$accounts = @(
    "$Prefix-escrow.testnet",
    "$Prefix-alice.testnet",
    "$Prefix-bob.testnet",
    "$Prefix-base.testnet",
    "$Prefix-quote.testnet"
)

foreach ($id in $accounts) {
    Assert-AccountId -AccountId $id
}

foreach ($id in $accounts) {
    if (Get-AccountInfo -AccountId $id) {
        Write-Host "account exists, skipping: $id"
        continue
    }
    try {
        Invoke-Near -NearArgs @(
            'account', 'create-account', 'sponsor-by-faucet-service', $id,
            'autogenerate-new-keypair', 'save-to-keychain',
            'network-config', 'testnet', 'create'
        ) -DryRun:$DryRun | Out-Null
    } catch {
        Write-Host "failed to create $id : $($_.Exception.Message)"
        Write-Host 'Re-run this script later; existing accounts are skipped.'
        exit 1
    }
}

$state = [PSCustomObject]@{
    network          = $Script:NearNetwork
    prefix           = $Prefix
    escrow_account   = "$Prefix-escrow.testnet"
    alice_account    = "$Prefix-alice.testnet"
    bob_account      = "$Prefix-bob.testnet"
    base_token       = "$Prefix-base.testnet"
    quote_token      = "$Prefix-quote.testnet"
    keys_dir         = $null
    alice_public_hex = $null
    bob_public_hex   = $null
    updated_at       = (Get-Date).ToUniversalTime().ToString('o')
}
# keep keys already recorded by an earlier run of step 4
$old = Read-State
if ($null -ne $old -and $old.prefix -eq $Prefix) {
    $state.keys_dir = $old.keys_dir
    $state.alice_public_hex = $old.alice_public_hex
    $state.bob_public_hex = $old.bob_public_hex
}
Save-State -State $state

Write-Host ''
Write-Host 'Account balances (NEAR):'
foreach ($id in $accounts) {
    $bal = Get-NearBalance -AccountId $id
    Write-Host "  $id  $bal"
}

Write-Host 'NEXT: run 2-check-wasm.ps1 (optionally with -Build)'
