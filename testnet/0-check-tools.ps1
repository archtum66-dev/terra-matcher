<#
.SYNOPSIS
  Read-only preflight for the testnet run: tools, WASM files, RPC and faucet reachability.
  Creates no account, signs nothing, sends nothing.
.PARAMETER SkipNetwork
  Skip the RPC and faucet checks (offline use).
.EXAMPLE
  .\0-check-tools.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipNetwork
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')
. (Join-Path $PSScriptRoot 'lib-tx.ps1')

Start-Log -Name '0-check-tools' | Out-Null

# SHA-256 of near.exe from the official near-cli-rs v0.30.1 release (x86_64-pc-windows-msvc)
$ExpectedNearSha256 = 'b5afa0c3069c6d217579df801764a286d12a58cbed4f5e99a6d6bbc40cfc4813'
$FaucetUrl = 'https://helper.nearprotocol.com/account'

$Script:Failed = 0
$Script:Warned = 0

function Show-Check {
    param([string]$Level, [string]$Name, [string]$Detail)
    $line = ('[{0}] {1}: {2}' -f $Level, $Name, $Detail)
    Write-Host $line
    Write-LogLine $line
    if ($Level -eq 'FAIL') {
        $Script:Failed++
    }
    if ($Level -eq 'WARN') {
        $Script:Warned++
    }
}

# 1. PowerShell
$psv = $PSVersionTable.PSVersion
if ($psv.Major -ge 5) {
    Show-Check -Level 'ok' -Name 'PowerShell' -Detail $psv.ToString()
} else {
    Show-Check -Level 'FAIL' -Name 'PowerShell' -Detail "version $psv is too old (need 5.1 or newer)"
}

# 2. near-cli-rs
$nearPath = $null
try {
    $nearPath = Get-NearExecutable
} catch {
    Show-Check -Level 'FAIL' -Name 'near-cli-rs' -Detail 'not found (expected testnet\bin\near.exe or near on PATH)'
}
if ($nearPath) {
    $v = Invoke-Native -FilePath $nearPath -ArgumentList @('--version')
    if ($v.ExitCode -eq 0) {
        Show-Check -Level 'ok' -Name 'near-cli-rs' -Detail (($v.StdOut + '').Trim() + "  [$nearPath]")
    } else {
        Show-Check -Level 'FAIL' -Name 'near-cli-rs' -Detail "near --version exited with $($v.ExitCode): $(($v.StdErr + '').Trim())"
    }
    $local = Join-Path $PSScriptRoot 'bin\near.exe'
    if ((Test-Path -LiteralPath $local) -and ($nearPath -eq $local)) {
        $actual = (Get-FileHash -LiteralPath $local -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -eq $ExpectedNearSha256) {
            Show-Check -Level 'ok' -Name 'near.exe checksum' -Detail 'matches the v0.30.1 release'
        } else {
            Show-Check -Level 'WARN' -Name 'near.exe checksum' -Detail "differs from the v0.30.1 release ($actual)"
        }
    }
}

# 3. terra-keys
try {
    $tool = Get-KeysToolCommand
    $u = Invoke-Native -FilePath $tool.File -ArgumentList (@($tool.Prefix) + @('nonexistent-command'))
    $usage = ($u.StdErr + $u.StdOut)
    if ($usage -match 'make-order' -and $usage -match 'settle-args') {
        Show-Check -Level 'ok' -Name 'terra-keys' -Detail "knows make-order and settle-args  [$($tool.File)]"
    } else {
        Show-Check -Level 'FAIL' -Name 'terra-keys' -Detail 'found, but too old (no make-order / settle-args); rebuild it'
    }
} catch {
    Show-Check -Level 'FAIL' -Name 'terra-keys' -Detail $_.Exception.Message
}

# 4. WASM files
try {
    $wasmDir = Get-DefaultWasmDir
    foreach ($f in @('escrow.wasm', 'terra_test_token.wasm')) {
        $p = Join-Path $wasmDir $f
        if (-not (Test-Path -LiteralPath $p)) {
            throw "$f missing in $wasmDir"
        }
        $bytes = [System.IO.File]::ReadAllBytes($p)
        if ($bytes.Length -lt 8 -or $bytes[0] -ne 0 -or $bytes[1] -ne 0x61 -or $bytes[2] -ne 0x73 -or $bytes[3] -ne 0x6d) {
            throw "$f is not a WASM file"
        }
        if (-not (Test-WasmBuiltForChain -Bytes $bytes)) {
            throw "$f was built without --cfg near: contract errors would show only 'unreachable'. Build with RUSTFLAGS=--cfg near (see README)"
        }
        Show-Check -Level 'ok' -Name $f -Detail ("{0} bytes, built for the chain" -f $bytes.Length)
    }
} catch {
    Show-Check -Level 'FAIL' -Name 'WASM files' -Detail $_.Exception.Message
}

# 5. directories
foreach ($d in @($Script:LogDir, (Join-Path $PSScriptRoot 'results'))) {
    try {
        if (-not (Test-Path -LiteralPath $d)) {
            New-Item -ItemType Directory -Path $d -Force | Out-Null
        }
        $probe = Join-Path $d '.write-test'
        Set-Content -LiteralPath $probe -Value 'x' -Encoding ASCII
        Remove-Item -LiteralPath $probe -Force
        Show-Check -Level 'ok' -Name 'writable' -Detail $d
    } catch {
        Show-Check -Level 'FAIL' -Name 'writable' -Detail "$d : $($_.Exception.Message)"
    }
}

if (-not $SkipNetwork) {
    # 6. RPC
    try {
        $st = Invoke-RpcRaw -Method 'status' -Params @()
        if ($st.PSObject.Properties['result'] -and $null -ne $st.result) {
            $chain = [string]$st.result.chain_id
            $blockTime = [datetime]::Parse([string]$st.result.sync_info.latest_block_time, [System.Globalization.CultureInfo]::InvariantCulture, [System.Globalization.DateTimeStyles]::AdjustToUniversal)
            $skew = [math]::Abs(((Get-Date).ToUniversalTime() - $blockTime).TotalSeconds)
            if ($chain -ne 'testnet') {
                Show-Check -Level 'FAIL' -Name 'RPC' -Detail "chain_id is '$chain', expected testnet ($($Script:RpcUrl))"
            } elseif ($skew -gt 120) {
                Show-Check -Level 'WARN' -Name 'RPC' -Detail ("testnet reachable, but the clock differs from the latest block by {0:N0} s (orders use the local clock)" -f $skew)
            } else {
                Show-Check -Level 'ok' -Name 'RPC' -Detail "testnet reachable ($($Script:RpcUrl)), clock difference $([math]::Round($skew)) s"
            }
        } else {
            Show-Check -Level 'FAIL' -Name 'RPC' -Detail 'no result from the status call'
        }
    } catch {
        Show-Check -Level 'FAIL' -Name 'RPC' -Detail $_.Exception.Message
    }

    # 7. faucet endpoint (any HTTP answer, even an error status, proves the host is alive)
    try {
        $code = 0
        try {
            $r = Invoke-WebRequest -Uri $FaucetUrl -Method Get -UseBasicParsing -TimeoutSec 20
            $code = [int]$r.StatusCode
        } catch {
            $resp = $null
            if ($_.Exception.PSObject.Properties['Response']) {
                $resp = $_.Exception.Response
            }
            if ($null -ne $resp) {
                $code = [int]$resp.StatusCode
            } else {
                throw
            }
        }
        Show-Check -Level 'ok' -Name 'faucet host' -Detail "$FaucetUrl answers (HTTP $code); whether it still hands out accounts is only known after step 1"
    } catch {
        Show-Check -Level 'WARN' -Name 'faucet host' -Detail "$FaucetUrl not reachable: $($_.Exception.Message)"
    }
}

Write-Host ''
if ($Script:Failed -gt 0) {
    Write-Host "Preflight: $($Script:Failed) check(s) failed, $($Script:Warned) warning(s)."
    exit 1
}
Write-Host "Preflight passed ($($Script:Warned) warning(s))."
Write-Host 'NEXT: run RUN-1-TO-7.bat (or .\run-1-to-7.ps1 -Prefix <name>)'
exit 0
