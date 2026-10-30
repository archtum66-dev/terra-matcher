# lib-tx.ps1 - helpers for steps 6 and 7 (settle, withdraw). Dot-source it after lib.ps1.
# ASCII only, Windows PowerShell 5.1 compatible.

# Where the transaction records go. Public data only (labels, accounts, tx hashes, gas).
function Get-ResultsPath {
    if ($env:TERRA_RESULTS_FILE) {
        return $env:TERRA_RESULTS_FILE
    }
    return (Join-Path (Join-Path $PSScriptRoot 'results') 'results.json')
}

function Get-ResultRecords {
    $path = Get-ResultsPath
    if (-not (Test-Path -LiteralPath $path)) {
        return @()
    }
    $raw = Get-Content -LiteralPath $path -Raw -Encoding ASCII
    if (-not $raw -or -not $raw.Trim()) {
        return @()
    }
    $parsed = $raw | ConvertFrom-Json
    return @($parsed)
}

function Add-ResultRecord {
    param([Parameter(Mandatory = $true)]$Record)
    $path = Get-ResultsPath
    $dir = Split-Path -Parent $path
    if (-not (Test-Path -LiteralPath $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    $all = @(Get-ResultRecords)
    $all += $Record
    $json = ConvertTo-Json -InputObject $all -Depth 5
    Set-Content -LiteralPath $path -Value $json -Encoding ASCII
}

function Get-GasFromOutput {
    # Fallback: read "Gas burned: 4.1 Tgas" from the near-cli-rs text. Returns 0 when not found.
    param([string]$Text)
    $clean = Remove-AnsiCodes -Text $Text
    $m = [regex]::Match($clean, 'Gas burned:\s*([0-9][0-9.,]*)\s*([A-Za-z]*)')
    if (-not $m.Success) {
        return [long]0
    }
    $number = [decimal]::Parse(($m.Groups[1].Value -replace ',', ''), [System.Globalization.CultureInfo]::InvariantCulture)
    $unit = $m.Groups[2].Value.ToLowerInvariant()
    $factor = [decimal]1
    if ($unit -eq 'tgas') {
        $factor = [decimal]1000000000000
    } elseif ($unit -eq 'ggas') {
        $factor = [decimal]1000000000
    } elseif ($unit -eq 'mgas') {
        $factor = [decimal]1000000
    } elseif ($unit -eq 'kgas') {
        $factor = [decimal]1000
    }
    return [long][math]::Round($number * $factor)
}

function Get-TxGasFromRpc {
    # Sum of gas_burnt over the transaction and all its receipts (the whole call chain). Returns 0 when the RPC has no answer.
    param(
        [Parameter(Mandatory = $true)][string]$TxHash,
        [Parameter(Mandatory = $true)][string]$Signer
    )
    for ($attempt = 1; $attempt -le 5; $attempt++) {
        $resp = $null
        try {
            $resp = Invoke-RpcRaw -Method 'tx' -Params @{ tx_hash = $TxHash; sender_account_id = $Signer; wait_until = 'FINAL' }
        } catch {
            $resp = $null
        }
        if ($null -ne $resp -and $resp.PSObject.Properties['result'] -and $null -ne $resp.result) {
            $res = $resp.result
            $total = [long]0
            $tokens = [System.Numerics.BigInteger]::Zero
            if ($res.PSObject.Properties['transaction_outcome']) {
                $total += [long]$res.transaction_outcome.outcome.gas_burnt
                $tokens += [System.Numerics.BigInteger]::Parse([string]$res.transaction_outcome.outcome.tokens_burnt)
            }
            if ($res.PSObject.Properties['receipts_outcome']) {
                foreach ($r in @($res.receipts_outcome)) {
                    $total += [long]$r.outcome.gas_burnt
                    $tokens += [System.Numerics.BigInteger]::Parse([string]$r.outcome.tokens_burnt)
                }
            }
            return [PSCustomObject]@{ Gas = $total; Tokens = $tokens.ToString() }
        }
        Start-Sleep -Seconds 3
    }
    return [PSCustomObject]@{ Gas = [long]0; Tokens = '0' }
}

function Invoke-NearTx {
    # Runs one near transaction and records tx hash and gas burnt.
    # With -AllowFailure a rejected transaction does not throw (Ok = false); used for the negative check.
    param(
        [Parameter(Mandatory = $true)][string[]]$NearArgs,
        [Parameter(Mandatory = $true)][string]$Signer,
        [Parameter(Mandatory = $true)][string]$Label,
        [switch]$DryRun,
        [switch]$AllowFailure
    )
    if ($DryRun) {
        [void](Invoke-Near -NearArgs $NearArgs -DryRun)
        return [PSCustomObject]@{ Ok = $true; TxHash = ''; GasBurnt = [long]0; Output = '' }
    }
    $result = Invoke-Near -NearArgs $NearArgs -AllowFailure:$AllowFailure
    $text = Remove-AnsiCodes -Text ($result.StdOut + "`n" + $result.StdErr)
    $ok = ($result.ExitCode -eq 0) -and (-not (Test-NearOutputFailed -Text $text))
    $hash = ''
    $m = [regex]::Match($text, 'Transaction ID:\s*(\S+)')
    if ($m.Success) {
        $hash = $m.Groups[1].Value
    }
    $gas = [long]0
    $tokens = '0'
    if ($hash) {
        $g = Get-TxGasFromRpc -TxHash $hash -Signer $Signer
        $gas = $g.Gas
        $tokens = $g.Tokens
    }
    if ($gas -eq 0) {
        $gas = Get-GasFromOutput -Text $text
    }
    $method = ''
    $idx = [array]::IndexOf($NearArgs, 'as-transaction')
    if ($idx -ge 0 -and ($idx + 2) -lt $NearArgs.Count) {
        $method = $NearArgs[$idx + 2]
    }
    $record = [PSCustomObject]@{
        label       = $Label
        signer      = $Signer
        method      = $method
        tx_hash     = $hash
        gas_burnt   = $gas
        tokens_burnt = $tokens
        ok          = $ok
        time_utc    = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    }
    Add-ResultRecord -Record $record
    if (-not $ok -and -not $AllowFailure) {
        throw "transaction '$Label' failed"
    }
    return [PSCustomObject]@{ Ok = $ok; TxHash = $hash; GasBurnt = $gas; Output = $text }
}

function Get-KeysToolCommand {
    # Finds terra-keys: bin\terra-keys.exe next to the scripts, then PATH, then cargo from the repo.
    $bin = Join-Path $PSScriptRoot 'bin\terra-keys.exe'
    if (Test-Path -LiteralPath $bin) {
        return [PSCustomObject]@{ File = $bin; Prefix = @() }
    }
    $bin2 = Join-Path $PSScriptRoot 'bin\terra-keys'
    if (Test-Path -LiteralPath $bin2) {
        return [PSCustomObject]@{ File = $bin2; Prefix = @() }
    }
    $cmd = Get-Command terra-keys -ErrorAction SilentlyContinue
    if ($null -ne $cmd) {
        return [PSCustomObject]@{ File = $cmd.Source; Prefix = @() }
    }
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    $toml = Join-Path (Split-Path -Parent $PSScriptRoot) 'Cargo.toml'
    if ($null -ne $cargo -and (Test-Path -LiteralPath $toml)) {
        return [PSCustomObject]@{ File = $cargo.Source; Prefix = @('run', '-q', '--manifest-path', $toml, '-p', 'terra-keys', '--') }
    }
    throw 'terra-keys not found: copy terra-keys.exe to testnet\bin or install Rust (cargo)'
}

function Invoke-KeysTool {
    # Runs terra-keys; returns the parsed JSON of its stdout line (or the raw line with -Raw).
    param(
        [Parameter(Mandatory = $true)][string[]]$ToolArgs,
        [switch]$Raw,
        [switch]$DryRun
    )
    if ($DryRun) {
        # a dry run needs no keys: show the call and return a placeholder
        Write-Host "[dry-run] terra-keys $($ToolArgs[0]) ..."
        if ($Raw) {
            return '{}'
        }
        return [PSCustomObject]@{}
    }
    $tool = Get-KeysToolCommand
    $all = @()
    foreach ($p in $tool.Prefix) {
        $all += $p
    }
    foreach ($a in $ToolArgs) {
        $all += $a
    }
    $r = Invoke-Native -FilePath $tool.File -ArgumentList $all
    if ($r.ExitCode -ne 0) {
        $msg = ($r.StdErr + '').Trim()
        throw "terra-keys $($ToolArgs[0]) failed: $msg"
    }
    $lines = @(($r.StdOut -split "`r?`n") | Where-Object { $_.Trim() })
    if ($lines.Count -eq 0) {
        throw "terra-keys $($ToolArgs[0]) printed nothing"
    }
    $line = $lines[$lines.Count - 1].Trim()
    if ($Raw) {
        return $line
    }
    return ($line | ConvertFrom-Json)
}
