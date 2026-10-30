<#
.SYNOPSIS
  Register token storage, mint test tokens, and deposit into the escrow.
.PARAMETER BaseAmount
  Base token amount in smallest units (default 100000000).
.PARAMETER QuoteAmount
  Quote token amount in smallest units (default 1000000000).
.PARAMETER Force
  Run mint and deposit even if escrow balances are already sufficient.
.PARAMETER DryRun
  Print near commands without sending transactions.
.EXAMPLE
  .\5-deposit.ps1
#>
[CmdletBinding()]
param(
    [string]$BaseAmount = '100000000',
    [string]$QuoteAmount = '1000000000',
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

Start-Log -Name '5-deposit' | Out-Null

$state = Read-State
if ($null -eq $state) {
    throw 'state.json missing; run 1-create-accounts.ps1 first'
}

if ($BaseAmount -notmatch '^\d+$' -or $QuoteAmount -notmatch '^\d+$') {
    throw 'BaseAmount and QuoteAmount must contain digits only'
}

$escrow = $state.escrow_account
$alice = $state.alice_account
$bob = $state.bob_account
$base = $state.base_token
$quote = $state.quote_token

function Get-EscrowBalance {
    param(
        [string]$Account,
        [string]$Token
    )
    $v = Invoke-View -Contract $escrow -Method 'balance_of' -ArgsObject @{
        account_id = $Account
        token_id   = $Token
    } -DryRun:$DryRun
    if ($DryRun) {
        return '0'
    }
    if ($null -eq $v) {
        return '0'
    }
    return $v.ToString()
}

function Compare-DecimalStrings {
    param([string]$Left, [string]$Right)
    $l = [System.Numerics.BigInteger]::Parse($Left)
    $r = [System.Numerics.BigInteger]::Parse($Right)
    return ($l.CompareTo($r))
}

$aliceQuoteBal = Get-EscrowBalance -Account $alice -Token $quote
$bobBaseBal = Get-EscrowBalance -Account $bob -Token $base

$skipMintDeposit = $false
if (-not $Force) {
    if ((Compare-DecimalStrings -Left $aliceQuoteBal -Right $QuoteAmount) -ge 0) {
        if ((Compare-DecimalStrings -Left $bobBaseBal -Right $BaseAmount) -ge 0) {
            Write-Host 'escrow balances already sufficient; skipping mint and deposit (use -Force to redo)'
            $skipMintDeposit = $true
        }
    }
}

function Ensure-Storage {
    param(
        [string]$Token,
        [string]$Account
    )
    $bal = Invoke-View -Contract $Token -Method 'storage_balance_of' -ArgsObject @{ account_id = $Account } -DryRun:$DryRun
    if ($DryRun) {
        Write-Host "[dry-run] would storage_deposit $Account on $Token"
        return
    }
    if ($null -eq $bal) {
        $null = Invoke-Near -NearArgs @(
            'contract', 'call-function', 'as-transaction', $Token, 'storage_deposit', 'json-args', "{`"account_id`":`"$Account`",`"registration_only`":true}",
            'prepaid-gas', '30 Tgas', 'attached-deposit', '0.01 NEAR', 'sign-as', $Account,
            'network-config', 'testnet', 'sign-with-keychain', 'send'
        ) -DryRun:$false
    }
}

if (-not $skipMintDeposit) {
    foreach ($token in @($base, $quote)) {
        foreach ($acc in @($escrow, $alice, $bob)) {
            Ensure-Storage -Token $token -Account $acc
        }
    }

    $null = Invoke-Near -NearArgs @(
        'contract', 'call-function', 'as-transaction', $quote, 'mint', 'json-args', "{`"account_id`":`"$alice`",`"amount`":`"$QuoteAmount`"}",
        'prepaid-gas', '30 Tgas', 'attached-deposit', '0 NEAR', 'sign-as', $quote,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun

    $null = Invoke-Near -NearArgs @(
        'contract', 'call-function', 'as-transaction', $base, 'mint', 'json-args', "{`"account_id`":`"$bob`",`"amount`":`"$BaseAmount`"}",
        'prepaid-gas', '30 Tgas', 'attached-deposit', '0 NEAR', 'sign-as', $base,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun

    $null = Invoke-Near -NearArgs @(
        'contract', 'call-function', 'as-transaction', $quote, 'ft_transfer_call', 'json-args', "{`"receiver_id`":`"$escrow`",`"amount`":`"$QuoteAmount`",`"msg`":`"`"}",
        'prepaid-gas', '100 Tgas', 'attached-deposit', '1 yoctoNEAR', 'sign-as', $alice,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun

    $null = Invoke-Near -NearArgs @(
        'contract', 'call-function', 'as-transaction', $base, 'ft_transfer_call', 'json-args', "{`"receiver_id`":`"$escrow`",`"amount`":`"$BaseAmount`",`"msg`":`"`"}",
        'prepaid-gas', '100 Tgas', 'attached-deposit', '1 yoctoNEAR', 'sign-as', $bob,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun
}

if (-not $DryRun) {
    $wantQuote = [System.Numerics.BigInteger]::Parse($QuoteAmount)
    $wantBase = [System.Numerics.BigInteger]::Parse($BaseAmount)
    # waits until the final block shows the deposit; throws with the last value on a timeout
    [void](Wait-ViewValue -Contract $escrow -Method 'balance_of' -ArgsObject @{ account_id = $alice; token_id = $quote } -Test { param($v) $null -ne $v -and [System.Numerics.BigInteger]::Parse([string]$v) -ge $wantQuote })
    [void](Wait-ViewValue -Contract $escrow -Method 'balance_of' -ArgsObject @{ account_id = $bob; token_id = $base } -Test { param($v) $null -ne $v -and [System.Numerics.BigInteger]::Parse([string]$v) -ge $wantBase })
    $aliceQuoteBal = Get-EscrowBalance -Account $alice -Token $quote
    $bobBaseBal = Get-EscrowBalance -Account $bob -Token $base
    Write-Host ''
    Write-Host 'Escrow deposit check:'
    Write-Host "  alice / quote  expected >= $QuoteAmount  actual $aliceQuoteBal"
    Write-Host "  bob / base     expected >= $BaseAmount  actual $bobBaseBal"
    if ((Compare-DecimalStrings -Left $aliceQuoteBal -Right $QuoteAmount) -lt 0) {
        throw 'alice quote escrow balance too low'
    }
    if ((Compare-DecimalStrings -Left $bobBaseBal -Right $BaseAmount) -lt 0) {
        throw 'bob base escrow balance too low'
    }
}

Write-Host 'NEXT: steps 1-5 complete (settle/withdraw are documented in docs/TESTNET.md)'
