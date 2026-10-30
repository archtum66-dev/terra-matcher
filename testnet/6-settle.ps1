<#
.SYNOPSIS
  Build signed orders, settle two fills, and verify overfill rejection.
.PARAMETER KeysDir
  Directory for alice.order-key and bob.order-key (default from state or step 4 default).
.PARAMETER LotSizeBase
  Lot size in base token smallest units.
.PARAMETER QuotePerTickLot
  Quote amount per tick per lot.
.PARAMETER BuyPriceTicks
  Alice buy limit price in ticks.
.PARAMETER SellPriceTicks
  Bob sell limit price in ticks.
.PARAMETER QtyLots
  Order quantity in lots (both sides).
.PARAMETER Fill1Lots
  First fill size in lots.
.PARAMETER Fill1PriceTicks
  First fill price in ticks.
.PARAMETER Fill2Lots
  Second fill size in lots.
.PARAMETER Fill2PriceTicks
  Second fill price in ticks.
.PARAMETER Submitter
  NEAR account that submits settle (default: base token contract account).
.PARAMETER SkipNegativeCheck
  Skip the expected overfill rejection test.
.PARAMETER Force
  Run even if settle_done is already true in state.
.PARAMETER DryRun
  Print commands without sending transactions.
.EXAMPLE
  .\6-settle.ps1
#>
[CmdletBinding()]
param(
    [string]$KeysDir,
    [string]$LotSizeBase = '1000000',
    [string]$QuotePerTickLot = '1000',
    [int]$BuyPriceTicks = 100,
    [int]$SellPriceTicks = 90,
    [int]$QtyLots = 5,
    [int]$Fill1Lots = 3,
    [int]$Fill1PriceTicks = 95,
    [int]$Fill2Lots = 2,
    [int]$Fill2PriceTicks = 100,
    [string]$Submitter,
    [switch]$SkipNegativeCheck,
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

Start-Log -Name '6-settle' | Out-Null

$state = Read-State
if ($null -eq $state) {
    throw 'state.json missing; run steps 1-5 first'
}

function Test-StateField {
    param(
        [Parameter(Mandatory = $true)]
        $Obj,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )
    return ($null -ne $Obj.PSObject.Properties[$Name] -and $null -ne $Obj.$Name)
}

$required = @(
    'escrow_account', 'alice_account', 'bob_account',
    'base_token', 'quote_token'
)
if (-not $DryRun) {
    # a dry run may run before step 4 has stored the public keys
    $required += @('alice_public_hex', 'bob_public_hex')
}
foreach ($f in $required) {
    if (-not (Test-StateField -Obj $state -Name $f)) {
        throw "state.json missing field: $f (run steps 1-5 and 4-register-keys)"
    }
}

if (-not $KeysDir) {
    if (Test-StateField -Obj $state -Name 'keys_dir') {
        $KeysDir = $state.keys_dir
    }
}
if (-not $KeysDir) {
    $userHome = $env:USERPROFILE
    if (-not $userHome) {
        $userHome = $env:HOME
    }
    $KeysDir = Join-Path (Join-Path $userHome '.terra-testnet') 'keys'
}

$aliceKey = Join-Path $KeysDir 'alice.order-key'
$bobKey = Join-Path $KeysDir 'bob.order-key'
if (-not $DryRun -and (-not (Test-Path -LiteralPath $aliceKey) -or -not (Test-Path -LiteralPath $bobKey))) {
    throw "missing alice.order-key or bob.order-key under $KeysDir ; run 4-register-keys.ps1"
}

$escrow = $state.escrow_account
$alice = $state.alice_account
$bob = $state.bob_account
$base = $state.base_token
$quote = $state.quote_token

if (-not $Submitter) {
    $Submitter = $base
}

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
        return '0'
    }
    if ($null -eq $v) {
        return '0'
    }
    return $v.ToString()
}

function Get-FillAmounts {
    param(
        [int]$Lots,
        [int]$PriceTicks
    )
    $lotsBi = [System.Numerics.BigInteger]::Parse($Lots.ToString())
    $lotSize = [System.Numerics.BigInteger]::Parse($LotSizeBase)
    $qpt = [System.Numerics.BigInteger]::Parse($QuotePerTickLot)
    $price = [System.Numerics.BigInteger]::Parse($PriceTicks.ToString())
    $baseAmt = $lotsBi * $lotSize
    $quoteAmt = $lotsBi * $price * $qpt
    return @{
        Base  = $baseAmt.ToString()
        Quote = $quoteAmt.ToString()
    }
}

if ((Test-StateField -Obj $state -Name 'settle_done') -and ($state.settle_done -eq $true) -and (-not $Force)) {
    Write-Host 'settle already done (use -Force to redo)'
    Write-Host 'NEXT: .\7-withdraw.ps1'
    exit 0
}

if (-not $DryRun) {
    $aliceKeyOnChain = Invoke-View -Contract $escrow -Method 'get_key' -ArgsObject @{ account_id = $alice }
    $bobKeyOnChain = Invoke-View -Contract $escrow -Method 'get_key' -ArgsObject @{ account_id = $bob }
    if ($aliceKeyOnChain -ne $state.alice_public_hex) {
        throw 'get_key mismatch for alice'
    }
    if ($bobKeyOnChain -ne $state.bob_public_hex) {
        throw 'get_key mismatch for bob'
    }
}

$needQuote = [System.Numerics.BigInteger]::Zero
$needBase = [System.Numerics.BigInteger]::Zero
foreach ($fill in @(
        @{ Lots = $Fill1Lots; Price = $Fill1PriceTicks },
        @{ Lots = $Fill2Lots; Price = $Fill2PriceTicks }
    )) {
    $am = Get-FillAmounts -Lots $fill.Lots -PriceTicks $fill.Price
    $needQuote = $needQuote + [System.Numerics.BigInteger]::Parse($am.Quote)
    $needBase = $needBase + [System.Numerics.BigInteger]::Parse($am.Base)
}

$aliceQuoteBal = Get-EscrowBalanceStr -Account $alice -Token $quote
$bobBaseBal = Get-EscrowBalanceStr -Account $bob -Token $base
if (-not $DryRun) {
    $aq = [System.Numerics.BigInteger]::Parse($aliceQuoteBal)
    $bb = [System.Numerics.BigInteger]::Parse($bobBaseBal)
    if ($aq -lt $needQuote -or $bb -lt $needBase) {
        throw @"
escrow balances too low for both fills (run 5-deposit.ps1).
  alice quote need $($needQuote.ToString()) have $aliceQuoteBal
  bob base need $($needBase.ToString()) have $bobBaseBal
"@
    }
}

$ordersDir = Join-Path $PSScriptRoot 'orders'
if (-not (Test-Path -LiteralPath $ordersDir)) {
    if (-not $DryRun) {
        New-Item -ItemType Directory -Path $ordersDir -Force | Out-Null
    }
}

$nonceNow = [int64][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()

$buyPath = Join-Path $ordersDir 'buy.json'
$sellPath = Join-Path $ordersDir 'sell.json'

$buyRaw = Invoke-KeysTool -ToolArgs @(
    'make-order',
    '--key-file', $aliceKey,
    '--contract-id', $escrow,
    '--account-id', $alice,
    '--base-token', $base,
    '--quote-token', $quote,
    '--lot-size-base', $LotSizeBase,
    '--quote-per-tick-lot', $QuotePerTickLot,
    '--side', 'buy',
    '--price-ticks', $BuyPriceTicks.ToString(),
    '--qty-lots', $QtyLots.ToString(),
    '--nonce', $nonceNow.ToString(),
    '--expires-in-secs', '3600'
) -Raw -DryRun:$DryRun

$sellRaw = Invoke-KeysTool -ToolArgs @(
    'make-order',
    '--key-file', $bobKey,
    '--contract-id', $escrow,
    '--account-id', $bob,
    '--base-token', $base,
    '--quote-token', $quote,
    '--lot-size-base', $LotSizeBase,
    '--quote-per-tick-lot', $QuotePerTickLot,
    '--side', 'sell',
    '--price-ticks', $SellPriceTicks.ToString(),
    '--qty-lots', $QtyLots.ToString(),
    '--nonce', ($nonceNow + 1).ToString(),
    '--expires-in-secs', '3600'
) -Raw -DryRun:$DryRun

if (-not $DryRun) {
    Set-Content -LiteralPath $buyPath -Value $buyRaw.Trim() -Encoding ASCII
    Set-Content -LiteralPath $sellPath -Value $sellRaw.Trim() -Encoding ASCII
}

$buyHash = $null
$sellHash = $null
if ($DryRun) {
    $buyHash = ('0' * 64)
    $sellHash = ('1' * 64)
} else {
    $buyParsed = $buyRaw.Trim() | ConvertFrom-Json
    $sellParsed = $sellRaw.Trim() | ConvertFrom-Json
    $buyHash = $buyParsed.order_hash
    $sellHash = $sellParsed.order_hash
}

$script:aliceBaseStart = Get-EscrowBalanceStr -Account $alice -Token $base
$script:bobQuoteStart = Get-EscrowBalanceStr -Account $bob -Token $quote

$cumAliceBaseGain = [System.Numerics.BigInteger]::Zero
$cumBobQuoteGain = [System.Numerics.BigInteger]::Zero

$resultRows = New-Object System.Collections.Generic.List[object]

function Invoke-SettleFill {
    param(
        [string]$Label,
        [int]$FillLots,
        [int]$PriceTicks,
        [string]$BuyHashHex,
        [int]$ExpectedFilledTotal
    )
    $argsJson = Invoke-KeysTool -ToolArgs @(
        'settle-args',
        '--buy-file', $buyPath,
        '--sell-file', $sellPath,
        '--fill-lots', $FillLots.ToString(),
        '--price-ticks', $PriceTicks.ToString()
    ) -Raw -DryRun:$DryRun

    $tx = Invoke-NearTx -NearArgs @(
        'contract', 'call-function', 'as-transaction', $escrow, 'settle', 'json-args', $argsJson,
        'prepaid-gas', '100 Tgas', 'attached-deposit', '0 NEAR', 'sign-as', $Submitter,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -Signer $Submitter -Label $Label -DryRun:$DryRun

    $am = Get-FillAmounts -Lots $FillLots -PriceTicks $PriceTicks
    if (-not $DryRun) {
        $script:cumAliceBaseGain = $script:cumAliceBaseGain + [System.Numerics.BigInteger]::Parse($am.Base)
        $script:cumBobQuoteGain = $script:cumBobQuoteGain + [System.Numerics.BigInteger]::Parse($am.Quote)

        $filled = Wait-ViewValue -Contract $escrow -Method 'order_state' -ArgsObject @{
            account_id = $alice
            order_hash = $BuyHashHex
        } -Test { param($v) $null -ne $v -and $v.filled_lots -eq $ExpectedFilledTotal } -DryRun:$false

        $expectedAliceBase = [System.Numerics.BigInteger]::Parse($script:aliceBaseStart) + $script:cumAliceBaseGain
        $expectedBobQuote = [System.Numerics.BigInteger]::Parse($script:bobQuoteStart) + $script:cumBobQuoteGain
        # waits until the final block shows the new balances; throws with the last value on a timeout
        $null = Wait-ViewValue -Contract $escrow -Method 'balance_of' -ArgsObject @{ account_id = $alice; token_id = $base } -Test { param($v) $null -ne $v -and [System.Numerics.BigInteger]::Parse([string]$v) -eq $expectedAliceBase }
        $null = Wait-ViewValue -Contract $escrow -Method 'balance_of' -ArgsObject @{ account_id = $bob; token_id = $quote } -Test { param($v) $null -ne $v -and [System.Numerics.BigInteger]::Parse([string]$v) -eq $expectedBobQuote }
        if ($null -eq $filled) {
            throw "order_state check failed for $Label"
        }
    }

    $gasTgas = 0.0
    if ($tx.GasBurnt -gt 0) {
        $gasTgas = [math]::Round($tx.GasBurnt / 1000000000000.0, 1)
    }
    $resultRows.Add([PSCustomObject]@{
            Fill      = $Label
            Lots      = $FillLots
            Price     = $PriceTicks
            Base      = $am.Base
            Quote     = $am.Quote
            GasTgas   = $gasTgas
            TxHash    = $tx.TxHash
        })
}

Invoke-SettleFill -Label 'settle-1' -FillLots $Fill1Lots -PriceTicks $Fill1PriceTicks -BuyHashHex $buyHash -ExpectedFilledTotal $Fill1Lots
Invoke-SettleFill -Label 'settle-2' -FillLots $Fill2Lots -PriceTicks $Fill2PriceTicks -BuyHashHex $buyHash -ExpectedFilledTotal ($Fill1Lots + $Fill2Lots)

if (-not $SkipNegativeCheck) {
    $overFillLots = 1
    $argsOver = Invoke-KeysTool -ToolArgs @(
        'settle-args',
        '--buy-file', $buyPath,
        '--sell-file', $sellPath,
        '--fill-lots', $overFillLots.ToString(),
        '--price-ticks', $Fill1PriceTicks.ToString()
    ) -Raw -DryRun:$DryRun

    $over = Invoke-NearTx -NearArgs @(
        'contract', 'call-function', 'as-transaction', $escrow, 'settle', 'json-args', $argsOver,
        'prepaid-gas', '100 Tgas', 'attached-deposit', '0 NEAR', 'sign-as', $Submitter,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -Signer $Submitter -Label 'settle-overfill-rejected' -DryRun:$DryRun -AllowFailure

    if (-not $DryRun) {
        if ($over.Ok) {
            throw 'overfill test: the contract accepted a fill beyond the order quantity'
        }
        if ($over.Output -match 'overfill') {
            Write-Host 'overfill test: the contract rejected the extra fill with "overfill" (expected)'
        } else {
            Write-Host 'overfill test: the transaction was rejected, but the reason text was not recognised; see the log'
        }
        $st = Invoke-View -Contract $escrow -Method 'order_state' -ArgsObject @{
            account_id = $alice
            order_hash = $buyHash
        }
        if ($null -eq $st -or $st.filled_lots -ne $QtyLots) {
            throw 'overfill test: filled_lots should remain at qty_lots'
        }
        $expectedAliceBase = [System.Numerics.BigInteger]::Parse($script:aliceBaseStart) + $script:cumAliceBaseGain
        $expectedBobQuote = [System.Numerics.BigInteger]::Parse($script:bobQuoteStart) + $script:cumBobQuoteGain
        $aliceBaseNow = Get-EscrowBalanceStr -Account $alice -Token $base
        $bobQuoteNow = Get-EscrowBalanceStr -Account $bob -Token $quote
        if ([System.Numerics.BigInteger]::Parse($aliceBaseNow) -ne $expectedAliceBase) {
            throw 'overfill test: alice base changed unexpectedly'
        }
        if ([System.Numerics.BigInteger]::Parse($bobQuoteNow) -ne $expectedBobQuote) {
            throw 'overfill test: bob quote changed unexpectedly'
        }
    }
}

if (-not $DryRun) {
    $state | Add-Member -NotePropertyName 'settle_done' -NotePropertyValue $true -Force
    $state | Add-Member -NotePropertyName 'buy_order_hash' -NotePropertyValue $buyHash -Force
    $state | Add-Member -NotePropertyName 'sell_order_hash' -NotePropertyValue $sellHash -Force
    $state.updated_at = (Get-Date).ToUniversalTime().ToString('o')
    Save-State -State $state
}

Write-Host ''
Write-Host 'Settle results:'
Write-Host ('{0,-22}{1,6}{2,7}{3,14}{4,14}{5,8}  {6}' -f 'Fill', 'Lots', 'Price', 'Base', 'Quote', 'Tgas', 'TxHash')
foreach ($row in $resultRows) {
    Write-Host ('{0,-22}{1,6}{2,7}{3,14}{4,14}{5,8}  {6}' -f $row.Fill, $row.Lots, $row.Price, $row.Base, $row.Quote, $row.GasTgas, $row.TxHash)
}

Write-Host 'NEXT: .\7-withdraw.ps1'
