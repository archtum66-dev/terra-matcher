<#
.SYNOPSIS
  Generate order keys and register public keys on the escrow contract.
.PARAMETER KeysDir
  Directory for *.order-key files (default: %USERPROFILE%\.terra-testnet\keys).
.PARAMETER KeysTool
  Path to terra-keys binary (optional).
.PARAMETER DryRun
  Print commands without executing them.
.EXAMPLE
  .\4-register-keys.ps1
#>
[CmdletBinding()]
param(
    [string]$KeysDir,
    [string]$KeysTool,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'lib.ps1')

Start-Log -Name '4-register-keys' | Out-Null

$state = Read-State
if ($null -eq $state) {
    throw 'state.json missing; run 1-create-accounts.ps1 first'
}

if (-not $KeysDir) {
    $home = $env:USERPROFILE
    if (-not $home) {
        $home = $env:HOME
    }
    $KeysDir = Join-Path $home '.terra-testnet\keys'
}

function Resolve-KeysTool {
    param([string]$Explicit)
    if ($Explicit -and (Test-Path -LiteralPath $Explicit)) {
        return $Explicit
    }
    $localExe = Join-Path $PSScriptRoot 'bin\terra-keys.exe'
    if (Test-Path -LiteralPath $localExe) {
        return $localExe
    }
    $localNoExt = Join-Path $PSScriptRoot 'bin\terra-keys'
    if (Test-Path -LiteralPath $localNoExt) {
        return $localNoExt
    }
    $cmd = Get-Command terra-keys -ErrorAction SilentlyContinue
    if ($null -ne $cmd) {
        return $cmd.Source
    }
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($null -ne $cargo) {
        return 'cargo-run-terra-keys'
    }
    throw 'terra-keys not found: build testnet\bin\terra-keys.exe or install cargo and run from repo root'
}

$tool = Resolve-KeysTool -Explicit $KeysTool

function Invoke-KeysTool {
    param(
        [string[]]$ToolArgs
    )
    if ($tool -eq 'cargo-run-terra-keys') {
        $cargo = (Get-Command cargo).Source
        $all = @('run', '-q', '-p', 'terra-keys', '--') + $ToolArgs
        if ($DryRun) {
            Write-Host "[dry-run] cargo $($all -join ' ')"
            return [PSCustomObject]@{ ExitCode = 0; StdOut = '{"public_key_hex":"00"}'; StdErr = '' }
        }
        return Invoke-Native -FilePath $cargo -ArgumentList $all
    }
    if ($DryRun) {
        Write-Host "[dry-run] terra-keys $($ToolArgs -join ' ')"
        return [PSCustomObject]@{ ExitCode = 0; StdOut = '{"public_key_hex":"00"}'; StdErr = '' }
    }
    return Invoke-Native -FilePath $tool -ArgumentList $ToolArgs
}

if (-not (Test-Path -LiteralPath $KeysDir)) {
    if (-not $DryRun) {
        New-Item -ItemType Directory -Path $KeysDir -Force | Out-Null
    }
}

$traders = @(
    @{ Name = 'alice'; Account = $state.alice_account; Field = 'alice_public_hex' },
    @{ Name = 'bob'; Account = $state.bob_account; Field = 'bob_public_hex' }
)

foreach ($t in $traders) {
    $keyPath = Join-Path $KeysDir "$($t.Name).order-key"
    if (-not (Test-Path -LiteralPath $keyPath)) {
        $gen = Invoke-KeysTool -ToolArgs @('keygen', '--name', $t.Name, '--out', $KeysDir)
        if ($gen.ExitCode -ne 0) {
            throw "keygen failed for $($t.Name): $($gen.StdErr)"
        }
    }
    $pub = Invoke-KeysTool -ToolArgs @('pubkey', '--key-file', $keyPath)
    if ($pub.ExitCode -ne 0) {
        throw "pubkey failed for $($t.Name): $($pub.StdErr)"
    }
    $pubJson = ($pub.StdOut.Trim() | ConvertFrom-Json)
    $hex = $pubJson.public_key_hex
    if ($hex.Length -ne 64) {
        throw 'public_key_hex must be 64 characters'
    }
    if ($env:OS -eq 'Windows_NT' -and -not $DryRun) {
        $user = $env:USERNAME
        $icacls = Join-Path $env:SystemRoot 'System32\icacls.exe'
        & $icacls $keyPath '/inheritance:r' "/grant:r$($user):(R,W)" | Out-Null
    }
    Invoke-Near -NearArgs @(
        'contract', 'call-function', 'as-transaction', $state.escrow_account, 'register_key', 'json-args', "{`"public_key_hex`":`"$hex`"}",
        'prepaid-gas', '30 Tgas', 'attached-deposit', '0 NEAR', 'sign-as', $t.Account,
        'network-config', 'testnet', 'sign-with-keychain', 'send'
    ) -DryRun:$DryRun
    if (-not $DryRun) {
        $onChain = Invoke-View -Contract $state.escrow_account -Method 'get_key' -ArgsObject @{ account_id = $t.Account }
        if ($onChain -ne $hex) {
            throw "get_key mismatch for $($t.Account)"
        }
    }
    $state | Add-Member -NotePropertyName $t.Field -NotePropertyValue $hex -Force
    Write-Host "$($t.Account) public_key_hex=$hex"
    Write-Host "  key file: $keyPath (keep safe, never commit to git)"
}

$state.keys_dir = $KeysDir
$state.updated_at = (Get-Date).ToUniversalTime().ToString('o')
if (-not $DryRun) {
    Save-State -State $state
}

Write-Host 'NEXT: run 5-deposit.ps1'
