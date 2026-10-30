<#
.SYNOPSIS
  Withdraw all escrow balances for Alice and Bob back to their wallets.
.PARAMETER Force
  Reserved for future use (withdraw runs whenever balances are non-zero).
.PARAMETER DryRun
  Print near commands without sending transactions.
.EXAMPLE
  .\7-withdraw.ps1
#>
[CmdletBinding()]
param(
    [switch]$Force,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')
. (Join-Path $PSScriptRoot 'lib-tx.ps1')
if ($DryRun -and -not $env:TERRA_STATE_FILE) {
    $Script:StateFile = Join-Path $PSScriptRoot 'state.dry-run.json'
}

Start-Log -Name '7-withdraw' | Out-Null

$state = Read-State
if ($null -eq $state) {
    throw 'state.json missing; run steps 1-6 first'
}

$escrow = $state.escrow_account
$alice = $state.alice_account
$bob = $state.bob_account
$base = $state.base_token
$quote = $state.quote_token

function Get-EscrowBalanceStr {
    param(
        [string]$Account,
        [string]$Token
    )
    $v = Invoke-View -Contract $escrow -Method 'balance_of' -ArgsObject @{
        account_id = $Account
        token_id   = $Token
    } -DryRun:$DryRun
    if ($DryRun) {
        # a dry run shows the planned commands with a placeholder amount
        return '1'
    }
    if ($null -eq $v) {
        return '0'
    }
    return $v.ToString()
}

function Get-FtBalanceStr {
    param(
        [string]$Token,
        [string]$Account
    )
    $v = Invoke-View -Contract $Token -Method 'ft_balance_of' -ArgsObject @{ account_id = $Account } -DryRun:$DryRun
    if ($DryRun) {
        return '0'
    }
    if ($null -eq $v) {
        return '0'
    }
    return $v.ToString()
}

function Compare-Positive {
    param([string]$Amount)
    $z = [System.Numerics.BigInteger]::Zero
    $a = [System.Numerics.BigInteger]::Parse($Amount)
    return ($a.CompareTo($z) -gt 0)
}

$withdrawPlan = @(
    @{ Account = $alice; Token = $base; Short = 'alice-base' },
    @{ Account = $alice; Token = $quote; Short = 'alice-quote' },
    @{ Account = $bob; Token = $base; Short = 'bob-base' },
    @{ Account = $bob; Token = $quote; Short = 'bob-quote' }
)

$anyLeft = $false
foreach ($item in $withdrawPlan) {
    $bal = Get-EscrowBalanceStr -Account $item.Account -Token $item.Token
    if (Compare-Positive -Amount $bal) {
        $anyLeft = $true
        break
    }
}

if (-not $anyLeft) {
    Write-Host 'all four escrow balances are zero; nothing to withdraw'
} else {
    foreach ($item in $withdrawPlan) {
        $amount = Get-EscrowBalanceStr -Account $item.Account -Token $item.Token
        if (-not (Compare-Positive -Amount $amount)) {
            continue
        }
        $ftBefore = Get-FtBalanceStr -Token $item.Token -Account $item.Account
        $jsonArgs = "{`"token_id`":`"$($item.Token)`",`"amount`":`"$amount`",`"receiver_id`":null}"
        $label = 'withdraw-' + $item.Short
        Invoke-NearTx -NearArgs @(
            'contract', 'call-function', 'as-transaction', $escrow, 'withdraw', 'json-args', $jsonArgs,
            'prepaid-gas', '100 Tgas', 'attached-deposit', '1 yoctoNEAR', 'sign-as', $item.Account,
            'network-config', 'testnet', 'sign-with-keychain', 'send'
        ) -Signer $item.Account -Label $label -DryRun:$DryRun | Out-Null

        if (-not $DryRun) {
            $acct = $item.Account
            $tok = $item.Token
            $amt = $amount
            $before = $ftBefore
            $null = Wait-ViewValue -Contract $escrow -Method 'balance_of' -ArgsObject @{
                account_id = $acct
                token_id   = $tok
            } -Test { param($v) $null -ne $v -and $v.ToString() -eq '0' } -DryRun:$false

            $expectedAfter = [System.Numerics.BigInteger]::Parse($before) + [System.Numerics.BigInteger]::Parse($amt)
            $null = Wait-ViewValue -Contract $tok -Method 'ft_balance_of' -ArgsObject @{ account_id = $acct } -Test {
                param($v)
                if ($null -eq $v) {
                    return $false
                }
                $now = [System.Numerics.BigInteger]::Parse($v.ToString())
                return ($now -eq $expectedAfter)
            } -DryRun:$false
        }
    }
}

$records = Get-ResultRecords
Write-Host ''
Write-Host 'Transaction results:'
Write-Host ('{0,-28}{1,8}  {2}' -f 'Label', 'Tgas', 'TxHash')
foreach ($rec in $records) {
    $gasTgas = 0.0
    if ($rec.gas_burnt -gt 0) {
        $gasTgas = [math]::Round($rec.gas_burnt / 1000000000000.0, 1)
    }
    $hash = $rec.tx_hash
    if (-not $hash) {
        $hash = ''
    }
    Write-Host ('{0,-28}{1,8}  {2}' -f $rec.label, $gasTgas, $hash)
}

if ($DryRun) {
    Write-Host 'dry run: results\summary.md is not written'
    exit 0
}

$resultsDir = Join-Path $PSScriptRoot 'results'
if (-not (Test-Path -LiteralPath $resultsDir)) {
    New-Item -ItemType Directory -Path $resultsDir -Force | Out-Null
}
$summaryPath = Join-Path $resultsDir 'summary.md'
$lines = New-Object System.Collections.Generic.List[string]
$lines.Add('# Testnet transaction summary')
$lines.Add('')
$lines.Add('| Label | Tgas | TxHash |')
$lines.Add('| --- | ---: | --- |')
foreach ($rec in $records) {
    $gasTgas = 0.0
    if ($rec.gas_burnt -gt 0) {
        $gasTgas = [math]::Round($rec.gas_burnt / 1000000000000.0, 1)
    }
    $hash = $rec.tx_hash
    if (-not $hash) {
        $hash = ''
    }
    $link = $hash
    if ($hash.Length -gt 0) {
        $link = "https://testnet.nearblocks.io/txns/$hash"
    }
    $lines.Add("| $($rec.label) | $gasTgas | [$hash]($link) |")
}
Set-Content -LiteralPath $summaryPath -Value ($lines -join "`n") -Encoding ASCII

Write-Host 'NEXT: results are in testnet\results\summary.md'
