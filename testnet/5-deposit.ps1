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

function Get-TokenBalance {
    param(
        [string]$Token,
        [string]$Account
    )
    $v = Invoke-View -Contract $Token -Method 'ft_balance_of' -ArgsObject @{ account_id = $Account } -DryRun:$DryRun
    if ($DryRun -or $null -eq $v) {
        return '0'
    }
    return $v.ToString()
}

function Wait-EscrowCredit {
    # Waits until the escrow shows the deposit. If it never does and the tokens are back with the sender, the token
    # refunded the transfer, which means the escrow's ft_on_transfer failed: say so instead of a bare timeout.
    param(
        [string]$Token,
        [string]$Sender,
        [System.Numerics.BigInteger]$WantAtLeast,
        [System.Numerics.BigInteger]$WalletBefore
    )
    try {
        [void](Wait-ViewValue -Contract $escrow -Method 'balance_of' -ArgsObject @{ account_id = $Sender; token_id = $Token } -TimeoutSec 45 -Test {
            param($v)
            $null -ne $v -and [System.Numerics.BigInteger]::Parse([string]$v) -ge $WantAtLeast
        })
    } catch {
        $walletNow = [System.Numerics.BigInteger]::Parse((Get-TokenBalance -Token $Token -Account $Sender))
        if ($walletNow -ge $WalletBefore) {
            throw "DEPOSIT REFUNDED: $Token returned the tokens to $Sender, so the escrow did not accept the transfer (its ft_on_transfer failed). Most likely the escrow on chain is not the current build. Start RUN-1-TO-7.bat again with the same prefix: step 3 replaces changed code and keeps the state. The log has the details."
        }
        throw
    }
}

function Send-Deposit {
    # Brings the escrow balance of $Sender for $Token up to $Amount: mints only what the wallet lacks, then calls ft_transfer_call.
    param(
        [string]$Token,
        [string]$Sender,
        [string]$TokenOwner,
        [string]$Amount
    )
    $want = [System.Numerics.BigInteger]::Parse($Amount)
    $held = [System.Numerics.BigInteger]::Parse((Get-EscrowBalance -Account $Sender -Token $Token))
    $need = $want
    if (-not $Force) {
        if ($held -ge $want) {
            Write-Host "escrow already holds $held of $Token for $Sender; nothing to deposit (use -Force to deposit again)"
            return
        }
        $need = $want - $held
    }
    $wallet = [System.Numerics.BigInteger]::Parse((Get-TokenBalance -Token $Token -Account $Sender))
    if ($wallet -lt $need) {
        $missing = $need - $wallet
        $null = Invoke-Near -NearArgs @(
            'contract', 'call-function', 'as-transaction', $Token, 'mint', 'json-args', "{`"account_id`":`"$Sender`",`"amount`":`"$missing`"}",
            'prepaid-gas', '30 Tgas', 'attached-deposit', '0 NEAR', 'sign-as', $TokenOwner,
            'network-config', 'testnet', 'sign-with-keychain', 'send'
        ) -DryRun:$DryRun
        $wallet = $wallet + $missing
    } else {
        Write-Host "$Sender already holds $wallet of $Token; no mint needed"
    }
    $null = Invoke-Near -NearArgs @(
        'contract', 'call-function', 'as-transaction', $Token, 'ft_transfer_call', 'json-args', "{`"receiver_id`":`"$escrow`",`"amount`":`"$need`",`"msg`":`"`"}",
        'prepaid-gas', '100 Tgas', 'attached-deposit', '1 yoctoNEAR', 'sign-as', $Sender,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun
    if (-not $DryRun) {
        Wait-EscrowCredit -Token $Token -Sender $Sender -WantAtLeast ($held + $need) -WalletBefore $wallet
    }
}

foreach ($token in @($base, $quote)) {
    foreach ($acc in @($escrow, $alice, $bob)) {
        Ensure-Storage -Token $token -Account $acc
    }
}

Send-Deposit -Token $quote -Sender $alice -TokenOwner $quote -Amount $QuoteAmount
Send-Deposit -Token $base -Sender $bob -TokenOwner $base -Amount $BaseAmount

if (-not $DryRun) {
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
