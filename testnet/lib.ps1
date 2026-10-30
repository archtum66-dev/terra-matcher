# Shared helpers for testnet PowerShell scripts (Windows PowerShell 5.1 compatible).

$Script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Script:NearNetwork = 'testnet'

if ($env:TERRA_RPC_URL) {
    $Script:RpcUrl = $env:TERRA_RPC_URL
} else {
    $Script:RpcUrl = 'https://test.rpc.fastnear.com'
}

if ($env:TERRA_STATE_FILE) {
    $Script:StateFile = $env:TERRA_STATE_FILE
} else {
    $Script:StateFile = Join-Path $PSScriptRoot 'state.json'
}

$Script:LogDir = Join-Path $PSScriptRoot 'logs'

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if (-not ([System.Management.Automation.PSTypeName]'System.Numerics.BigInteger').Type) {
    Add-Type -AssemblyName System.Numerics
}

function Assert-AccountId {
    param(
        [Parameter(Mandatory = $true)]
        [string]$AccountId
    )
    if ($AccountId -notmatch '^[a-z0-9][a-z0-9_-]{1,60}\.testnet$') {
        throw "invalid testnet account id: $AccountId"
    }
}

function Assert-NoMainnetInArgs {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$ArgList
    )
    foreach ($a in $ArgList) {
        if ($a -eq 'mainnet') {
            throw 'refusing to use mainnet'
        }
    }
}

function Quote-NativeArg {
    # Quote one argument for a Windows command line (CommandLineToArgvW rules).
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Arg
    )
    if ($Arg.Length -eq 0) {
        return '""'
    }
    if ($Arg -notmatch '[\s"]') {
        return $Arg
    }
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.Append('"')
    $bs = 0
    foreach ($ch in $Arg.ToCharArray()) {
        if ($ch -eq [char]'\') {
            $bs++
        } elseif ($ch -eq [char]'"') {
            # backslashes before a quote are doubled, the quote itself is escaped
            [void]$sb.Append('\' * ($bs * 2 + 1))
            [void]$sb.Append('"')
            $bs = 0
        } else {
            if ($bs -gt 0) {
                [void]$sb.Append('\' * $bs)
                $bs = 0
            }
            [void]$sb.Append($ch)
        }
    }
    if ($bs -gt 0) {
        # trailing backslashes are doubled because the closing quote follows
        [void]$sb.Append('\' * ($bs * 2))
    }
    [void]$sb.Append('"')
    return $sb.ToString()
}

function Redact-SensitiveText {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Text
    )
    if ([string]::IsNullOrEmpty($Text)) {
        return $Text
    }
    $lines = $Text -split "`n"
    $out = New-Object System.Collections.Generic.List[string]
    foreach ($line in $lines) {
        $l = $line
        if ($l -match '(?i)(seed phrase|secret key|private key)') {
            $l = '<redacted>'
        } else {
            $l = [regex]::Replace($l, 'ed25519:[123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz]{40,}', '<redacted>')
        }
        $out.Add($l)
    }
    return ($out -join "`n")
}

function Redact-ArgumentList {
    param(
        [string[]]$ArgList
    )
    $result = @()
    foreach ($a in $ArgList) {
        if ($a -like '*.order-key') {
            $result += '<redacted-path>'
        } else {
            $result += $a
        }
    }
    return $result
}

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [Parameter(Mandatory = $true)]
        [string[]]$ArgumentList
    )
    $quoted = @()
    foreach ($a in $ArgumentList) {
        $quoted += (Quote-NativeArg -Arg $a)
    }
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $FilePath
    $psi.Arguments = ($quoted -join ' ')
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    $psi.CreateNoWindow = $true
    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    [void]$proc.Start()
    # no input is ever sent: an unexpected prompt of the tool fails fast instead of waiting for a keyboard
    $proc.StandardInput.Close()
    # read both streams asynchronously so a full pipe cannot block the child
    $outTask = $proc.StandardOutput.ReadToEndAsync()
    $errTask = $proc.StandardError.ReadToEndAsync()
    $proc.WaitForExit()
    return [PSCustomObject]@{
        ExitCode = $proc.ExitCode
        StdOut   = $outTask.Result
        StdErr   = $errTask.Result
    }
}

function Get-NearExecutable {
    if ($env:TERRA_NEAR_BIN) {
        return $env:TERRA_NEAR_BIN
    }
    $local = Join-Path $PSScriptRoot 'bin\near.exe'
    if (Test-Path -LiteralPath $local) {
        return $local
    }
    $cmd = Get-Command near -ErrorAction SilentlyContinue
    if ($null -eq $cmd) {
        throw @"
near-cli-rs is not installed or not on PATH.
Install: irm https://github.com/near/near-cli-rs/releases/latest/download/near-cli-rs-installer.ps1 | iex
"@
    }
    return $cmd.Source
}

function Start-Log {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name
    )
    if (-not (Test-Path -LiteralPath $Script:LogDir)) {
        New-Item -ItemType Directory -Path $Script:LogDir -Force | Out-Null
    }
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $Script:CurrentLogFile = Join-Path $Script:LogDir "$Name-$stamp.log"
    "=== log started $stamp ===" | Set-Content -LiteralPath $Script:CurrentLogFile -Encoding ASCII
    return $Script:CurrentLogFile
}

function Write-LogLine {
    param([string]$Line)
    if ($Script:CurrentLogFile) {
        Add-Content -LiteralPath $Script:CurrentLogFile -Value (Redact-SensitiveText -Text $Line) -Encoding ASCII
    }
}

function Invoke-Near {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$NearArgs,
        [switch]$DryRun,
        [switch]$AllowFailure
    )
    Assert-NoMainnetInArgs -ArgList $NearArgs
    $displayArgs = Redact-ArgumentList -ArgList $NearArgs
    $cmdDisplay = "near $($displayArgs -join ' ')"
    if ($DryRun) {
        Write-Host "[dry-run] $cmdDisplay"
        Write-LogLine "[dry-run] $cmdDisplay"
        return [PSCustomObject]@{ ExitCode = 0; StdOut = ''; StdErr = '' }
    }
    $near = Get-NearExecutable
    Write-Host $cmdDisplay
    Write-LogLine $cmdDisplay
    $result = Invoke-Native -FilePath $near -ArgumentList $NearArgs
    if ($result.StdOut) {
        $safeOut = Redact-SensitiveText -Text $result.StdOut
        Write-Host $safeOut
        Write-LogLine $safeOut
    }
    if ($result.StdErr) {
        $safeErr = Redact-SensitiveText -Text $result.StdErr
        Write-Host $safeErr
        Write-LogLine $safeErr
    }
    if ($AllowFailure) {
        return $result
    }
    if ($result.ExitCode -ne 0) {
        throw "near exited with code $($result.ExitCode)"
    }
    if (Test-NearOutputFailed -Text ($result.StdOut + "`n" + $result.StdErr)) {
        throw 'the transaction was rejected on chain (see the output above)'
    }
    return $result
}

function Remove-AnsiCodes {
    # near-cli-rs may colour its output; strip the escape sequences before parsing
    param([string]$Text)
    if (-not $Text) {
        return ''
    }
    return ([regex]::Replace($Text, "\x1b\[[0-9;]*[A-Za-z]", ''))
}

function Test-NearOutputFailed {
    # True when the output of a near call shows an on-chain failure (a contract panic or an execution error).
    # near-cli-rs may exit with 0 even then, so the text is checked as well as the exit code.
    param([string]$Text)
    $clean = Remove-AnsiCodes -Text $Text
    return ($clean -match 'Smart contract panicked|ExecutionError|FunctionCallError|ActionError|Failure:')
}

function Wait-ViewValue {
    # Calls a view method until the test returns $true (the final block can trail the transaction by a few seconds).
    # Returns the last value; throws after TimeoutSec with that value in the message.
    param(
        [Parameter(Mandatory = $true)]
        [string]$Contract,
        [Parameter(Mandatory = $true)]
        [string]$Method,
        $ArgsObject,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Test,
        [int]$TimeoutSec = 60,
        [switch]$DryRun
    )
    if ($DryRun) {
        return $null
    }
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ($true) {
        $last = Invoke-View -Contract $Contract -Method $Method -ArgsObject $ArgsObject
        $passed = $false
        try {
            $passed = [bool](& $Test $last)
        } catch {
            $passed = $false
        }
        if ($passed) {
            return $last
        }
        if ((Get-Date) -gt $deadline) {
            $shown = ($last | ConvertTo-Json -Compress -Depth 5)
            throw "timeout waiting for $Contract.$Method; last value: $shown"
        }
        Start-Sleep -Seconds 3
    }
}

function Invoke-RpcRaw {
    # Returns the parsed JSON-RPC response (result or error), throws only on transport errors.
    param(
        [Parameter(Mandatory = $true)]
        [string]$Method,
        [Parameter(Mandatory = $true)]
        $Params
    )
    $body = @{
        jsonrpc = '2.0'
        id      = 'terra-testnet'
        method  = $Method
        params  = $Params
    } | ConvertTo-Json -Depth 10 -Compress
    try {
        return (Invoke-RestMethod -Uri $Script:RpcUrl -Method Post -ContentType 'application/json' -Body $body)
    } catch {
        throw "RPC request failed: $($_.Exception.Message)"
    }
}

function Invoke-Rpc {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Method,
        [Parameter(Mandatory = $true)]
        $Params
    )
    $resp = Invoke-RpcRaw -Method $Method -Params $Params
    if ($resp.PSObject.Properties['error'] -and $null -ne $resp.error) {
        throw "RPC error: $($resp.error.message) $($resp.error.data)"
    }
    return $resp.result
}

function Get-AccountInfo {
    # Returns the account record, or $null when the account does not exist.
    param(
        [Parameter(Mandatory = $true)]
        [string]$AccountId
    )
    Assert-AccountId -AccountId $AccountId
    $resp = Invoke-RpcRaw -Method 'query' -Params @{
        request_type = 'view_account'
        finality     = 'final'
        account_id   = $AccountId
    }
    if ($resp.PSObject.Properties['error'] -and $null -ne $resp.error) {
        $errName = ''
        if ($resp.error.PSObject.Properties['cause'] -and $null -ne $resp.error.cause -and $resp.error.cause.PSObject.Properties['name']) {
            $errName = $resp.error.cause.name
        }
        if ($errName -eq 'UNKNOWN_ACCOUNT') {
            return $null
        }
        throw "RPC error: $($resp.error.message) $($resp.error.data)"
    }
    return $resp.result
}

function Test-ContractDeployed {
    param(
        [Parameter(Mandatory = $true)]
        [string]$AccountId
    )
    $info = Get-AccountInfo -AccountId $AccountId
    if ($null -eq $info) {
        return $false
    }
    return ($info.code_hash -ne '11111111111111111111111111111111')
}

function ConvertTo-Base58 {
    # Bitcoin-style base58, the form in which NEAR shows hashes (account code_hash, transaction IDs).
    param(
        [Parameter(Mandatory = $true)]
        [byte[]]$Bytes
    )
    $alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
    # one extra zero byte keeps the number positive; BigInteger wants little endian
    $len = $Bytes.Length
    $tmp = New-Object byte[] ($len + 1)
    for ($i = 0; $i -lt $len; $i++) {
        $tmp[$i] = $Bytes[$len - 1 - $i]
    }
    $n = New-Object System.Numerics.BigInteger (, $tmp)
    $base = New-Object System.Numerics.BigInteger (58)
    $sb = New-Object System.Text.StringBuilder
    while (-not $n.IsZero) {
        $rem = [System.Numerics.BigInteger]::Remainder($n, $base)
        $n = [System.Numerics.BigInteger]::Divide($n, $base)
        [void]$sb.Insert(0, $alphabet[[int]$rem])
    }
    foreach ($b in $Bytes) {
        if ($b -ne 0) {
            break
        }
        [void]$sb.Insert(0, '1')
    }
    return $sb.ToString()
}

function Get-FileHashBase58 {
    # SHA-256 of a file in the base58 form that the chain reports as code_hash.
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $sha.ComputeHash([System.IO.File]::ReadAllBytes($Path))
    } finally {
        $sha.Dispose()
    }
    return (ConvertTo-Base58 -Bytes $hash)
}

function Test-ContractCodeMatches {
    # True when the code on the account is exactly the given WASM file.
    param(
        [Parameter(Mandatory = $true)]
        [string]$AccountId,
        [Parameter(Mandatory = $true)]
        [string]$WasmPath
    )
    $info = Get-AccountInfo -AccountId $AccountId
    if ($null -eq $info) {
        return $false
    }
    return ($info.code_hash -eq (Get-FileHashBase58 -Path $WasmPath))
}

function Get-NearBalance {
    # Balance in NEAR as a decimal (4 places). 0 when the account does not exist.
    param(
        [Parameter(Mandatory = $true)]
        [string]$AccountId
    )
    $info = Get-AccountInfo -AccountId $AccountId
    if ($null -eq $info) {
        return [decimal]0
    }
    $yocto = [System.Numerics.BigInteger]::Parse($info.amount)
    $unit = [System.Numerics.BigInteger]::Pow(10, 24)
    $whole = [System.Numerics.BigInteger]::Divide($yocto, $unit)
    $rest = [System.Numerics.BigInteger]::Remainder($yocto, $unit)
    $value = [decimal]::Parse($whole.ToString()) + ([decimal]::Parse($rest.ToString()) / [decimal]1000000000000000000000000)
    return [math]::Round($value, 4)
}

function Invoke-View {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Contract,
        [Parameter(Mandatory = $true)]
        [string]$Method,
        $ArgsObject,
        [switch]$DryRun
    )
    if ($DryRun) {
        Write-Host "[dry-run] skipped view $Contract.$Method"
        return $null
    }
    Assert-AccountId -AccountId $Contract
    $json = '{}'
    if ($null -ne $ArgsObject) {
        $json = ($ArgsObject | ConvertTo-Json -Compress -Depth 10)
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $b64 = [Convert]::ToBase64String($bytes)
    $params = @{
        request_type = 'call_function'
        finality     = 'final'
        account_id   = $Contract
        method_name  = $Method
        args_base64  = $b64
    }
    $result = Invoke-Rpc -Method 'query' -Params $params
    $byteList = $result.result
    if ($null -eq $byteList) {
        return $null
    }
    $outBytes = New-Object byte[] $byteList.Count
    for ($i = 0; $i -lt $byteList.Count; $i++) {
        $outBytes[$i] = [byte]$byteList[$i]
    }
    $text = [System.Text.Encoding]::UTF8.GetString($outBytes)
    if ($text -eq 'null') {
        return $null
    }
    if ($text -match '^"[0-9]+"$') {
        return ($text.Trim('"'))
    }
    if ($text -match '^"[a-f0-9]+"$') {
        return ($text.Trim('"'))
    }
    try {
        return ($text | ConvertFrom-Json)
    } catch {
        return $text
    }
}

function Read-State {
    if (-not (Test-Path -LiteralPath $Script:StateFile)) {
        return $null
    }
    $raw = Get-Content -LiteralPath $Script:StateFile -Raw -Encoding ASCII
    return ($raw | ConvertFrom-Json)
}

function Save-State {
    param(
        [Parameter(Mandatory = $true)]
        $State
    )
    $json = $State | ConvertTo-Json -Depth 5
    Set-Content -LiteralPath $Script:StateFile -Value $json -Encoding ASCII
}

function Get-DefaultWasmDir {
    return (Join-Path $PSScriptRoot 'wasm')
}

function Test-WasmBuiltForChain {
    # True when the WASM was built with `--cfg near`: only then the SDK imports the host function panic_utf8
    # (contract errors keep their text) and hashes through the chain instead of guest code. Without the flag the
    # file still runs, but every contract error shows only "unreachable" and costs more gas.
    param(
        [Parameter(Mandatory = $true)]
        [byte[]]$Bytes
    )
    $text = [System.Text.Encoding]::ASCII.GetString($Bytes)
    return $text.Contains('panic_utf8')
}

function Invoke-CheckWasm {
    param(
        [string]$WasmDir,
        [switch]$Build,
        [switch]$DryRun
    )
    if (-not $WasmDir) {
        $WasmDir = Get-DefaultWasmDir
    }
    $escrowPath = Join-Path $WasmDir 'escrow.wasm'
    $tokenPath = Join-Path $WasmDir 'terra_test_token.wasm'
    $needBuild = (-not (Test-Path -LiteralPath $escrowPath)) -or (-not (Test-Path -LiteralPath $tokenPath))
    if ($needBuild -and $Build) {
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if ($null -eq $cargo) {
            throw 'cargo not found; build WASM manually or copy files to testnet\wasm\'
        }
        if (-not $DryRun) {
            if (-not (Test-Path -LiteralPath $WasmDir)) {
                New-Item -ItemType Directory -Path $WasmDir -Force | Out-Null
            }
            # freshly built files will not match an older checksum list
            $oldSums = Join-Path $WasmDir 'SHA256SUMS.txt'
            if (Test-Path -LiteralPath $oldSums) {
                Remove-Item -LiteralPath $oldSums -Force
            }
        }
        $builds = @(
            @{
                Manifest = Join-Path $Script:RepoRoot 'contract\escrow\Cargo.toml'
                Artifact = Join-Path $Script:RepoRoot 'contract\escrow\target\wasm32-unknown-unknown\release\escrow.wasm'
                Out      = 'escrow.wasm'
            },
            @{
                Manifest = Join-Path $Script:RepoRoot 'contract\test-token\Cargo.toml'
                Artifact = Join-Path $Script:RepoRoot 'contract\test-token\target\wasm32-unknown-unknown\release\terra_test_token.wasm'
                Out      = 'terra_test_token.wasm'
            }
        )
        foreach ($t in $builds) {
            $buildArgs = @(
                'build',
                '--manifest-path', $t.Manifest,
                '--target', 'wasm32-unknown-unknown',
                '--release'
            )
            if ($DryRun) {
                Write-Host "[dry-run] RUSTFLAGS=--cfg near cargo $($buildArgs -join ' ')"
            } else {
                # `--cfg near` makes the SDK use the chain's host functions (see Test-WasmBuiltForChain)
                $oldFlags = $env:RUSTFLAGS
                $env:RUSTFLAGS = '--cfg near'
                try {
                    $br = Invoke-Native -FilePath $cargo.Source -ArgumentList $buildArgs
                } finally {
                    $env:RUSTFLAGS = $oldFlags
                }
                if ($br.ExitCode -ne 0) {
                    throw "cargo build failed for $($t.Out)"
                }
                Copy-Item -LiteralPath $t.Artifact -Destination (Join-Path $WasmDir $t.Out) -Force
            }
        }
    }
    if (-not (Test-Path -LiteralPath $escrowPath) -or -not (Test-Path -LiteralPath $tokenPath)) {
        throw @"
Missing WASM files in $WasmDir (escrow.wasm and terra_test_token.wasm).
Run 2-check-wasm.ps1 -Build with Rust installed, or copy built artifacts into testnet\wasm\.
"@
    }
    foreach ($pair in @(
            @{ Path = $escrowPath; Label = 'escrow.wasm' },
            @{ Path = $tokenPath; Label = 'terra_test_token.wasm' }
        )) {
        $bytes = [System.IO.File]::ReadAllBytes($pair.Path)
        if ($bytes.Length -lt 4 -or $bytes[0] -ne 0 -or $bytes[1] -ne 97 -or $bytes[2] -ne 115 -or $bytes[3] -ne 109) {
            throw "$($pair.Label): invalid WASM magic bytes"
        }
        if (-not (Test-WasmBuiltForChain -Bytes $bytes)) {
            throw "$($pair.Label): built without --cfg near (contract errors would show only 'unreachable'). Build with RUSTFLAGS=--cfg near, see testnet\README.md"
        }
        $sha = [System.Security.Cryptography.SHA256]::Create()
        $hashBytes = $sha.ComputeHash($bytes)
        $hash = ([BitConverter]::ToString($hashBytes) -replace '-', '').ToLowerInvariant()
        Write-Host "$($pair.Label): $($bytes.Length) bytes, sha256=$hash"
        $sumsFile = Join-Path $WasmDir 'SHA256SUMS.txt'
        if (Test-Path -LiteralPath $sumsFile) {
            $expected = $null
            Get-Content -LiteralPath $sumsFile -Encoding ASCII | ForEach-Object {
                if ($_ -match '^([0-9a-f]{64})\s+(\S+)') {
                    if ($Matches[2] -eq $pair.Label) {
                        $expected = $Matches[1]
                    }
                }
            }
            if ($null -ne $expected -and $expected -ne $hash) {
                throw "$($pair.Label): SHA256 mismatch (expected $expected, got $hash)"
            }
        }
        $bindNear = [math]::Round($bytes.Length * 0.00001, 4)
        Write-Host "  estimated storage bind: $bindNear NEAR"
    }
}

function Get-WasmSizeBytes {
    param([string]$WasmDir, [string]$FileName)
    $path = Join-Path $WasmDir $FileName
    return ([System.IO.File]::ReadAllBytes($path)).Length
}
