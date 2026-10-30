# Testnet run kit (steps 1-7)

PowerShell scripts that put the escrow contract and two test tokens on NEAR **testnet** and run a full trade: deposit, two partial fills, a rejected overfill, withdrawal. For the contract rules see `docs/ESCROW.md`.

**Status: never run on a live network.** The scripts were tested against a local simulator of near-cli-rs and of the RPC (command forms, JSON arguments, re-runs, stale views, failing exit codes). The first live run is the real test. Testnet only: every script refuses the word `mainnet`. Transactions are sent only when you start a script.

## Quick start

1. Put these files in place (none of them is committed to git):
   - `testnet\bin\near.exe` - near-cli-rs v0.30.1, Windows x86_64. Download `near-cli-rs-x86_64-pc-windows-msvc.tar.gz` from the [release page](https://github.com/near/near-cli-rs/releases/tag/v0.30.1), check it against the release's `sha256.sum`, extract `near.exe`. Its SHA-256 should be `b5afa0c3069c6d217579df801764a286d12a58cbed4f5e99a6d6bbc40cfc4813`. A `near` on `PATH` also works.
   - `testnet\bin\terra-keys.exe` - build with `cargo build --release -p terra-keys`. Without the file the scripts fall back to `cargo run` when Rust is installed.
   - `testnet\wasm\escrow.wasm` and `testnet\wasm\terra_test_token.wasm` - see step 2 below.
2. Double-click `PREFLIGHT.bat`. It checks tools, WASM files, folders, RPC and the faucet host. It creates nothing and sends nothing.
3. Double-click `DRYRUN-1-TO-7.bat`. It prints every `near` command without sending anything.
4. Double-click `RUN-1-TO-7.bat` and enter an account prefix (for example `terra1`). This creates five faucet-funded testnet accounts and sends all transactions.

Result: `testnet\results\summary.md` (every transaction with gas and a testnet.nearblocks.io link) and `testnet\results\results.json`.

## What each step does

| Step | Script | Effect |
|---|---|---|
| 0 | `0-check-tools.ps1` | Read-only preflight |
| 1 | `1-create-accounts.ps1 -Prefix <p>` | Five accounts via the faucet: escrow, alice, bob, base token, quote token. Keys go to the Windows keychain. |
| 2 | `2-check-wasm.ps1` | Sizes and SHA-256 of both WASM files, storage estimate. `-Build` builds them with Rust and the `wasm32-unknown-unknown` target. |
| 3 | `3-deploy.ps1` | Deploys escrow and both tokens, calls `new` in a separate transaction, checks with views. |
| 4 | `4-register-keys.ps1` | Creates alice's and bob's order keys outside the repo and calls `register_key`. |
| 5 | `5-deposit.ps1` | NEP-145 storage deposits, mints test tokens, `ft_transfer_call` into the escrow, checks the balances. |
| 6 | `6-settle.ps1` | Signs a buy and a sell order with `terra-keys`, settles 3 lots at 95 and 2 lots at 100, then sends one lot too many and expects the contract to answer `overfill`. |
| 7 | `7-withdraw.ps1` | Withdraws all non-zero escrow balances of alice and bob, checks that the escrow balances are zero and the tokens arrived, writes `results\summary.md`. |

`run-1-to-5.ps1` and `run-1-to-7.ps1 -Prefix <p>` run the steps in order. `-DryRun` works on every script.

## Re-runs

- Steps 1 and 3 skip what already exists (`3-deploy.ps1 -Force` redeploys the code and keeps the state).
- Step 6 refuses to run twice (`-Force` runs it again with fresh nonces). It stops with a clear message when the escrow balances are too low; run step 5 first.
- Running `run-1-to-7.ps1` again with the same prefix repeats steps 4 to 7 with new orders and appends to `results\results.json`.

## Where things are

- Order keys: `%USERPROFILE%\.terra-testnet\keys\*.order-key`. Never inside the repo, never printed. Back them up.
- Signed orders: `testnet\orders\` (gitignored). Order JSON and signatures only.
- Logs: `testnet\logs\`. State (public IDs and public key hex only): `testnet\state.json`.
- Logs and console output redact `ed25519:` keys, seed-phrase lines and key file paths.

Optional environment variables: `TERRA_RPC_URL`, `TERRA_STATE_FILE`, `TERRA_NEAR_BIN`, `TERRA_RESULTS_FILE`.

## Manual tools

```powershell
terra-keys make-order --key-file <KEY> --contract-id <ESCROW> --account-id <ALICE> --base-token <BASE> --quote-token <QUOTE> --lot-size-base 1000000 --quote-per-tick-lot 1000 --side buy --price-ticks 100 --qty-lots 5 --nonce 1 --expires-in-secs 3600
terra-keys settle-args --buy-file orders\buy.json --sell-file orders\sell.json --fill-lots 3 --price-ticks 95
terra-keys verify-order --signed-order-file orders\buy.json --key-file <KEY>
```

## Costs

Account creation is funded by the testnet faucet. Contract storage is bound at about 1 NEAR per 100 KB of code (about 3.7 NEAR for the escrow, 3.1 NEAR for each token), plus small storage deposits in step 5. Whether the faucet still funds this much is only known after step 1.

## Known risks of the first live run

- The faucet may have changed or be rate limited. Step 1 then fails; wait and re-run, existing accounts are skipped.
- near-cli-rs prompt text and output labels may differ from v0.30.1. The scripts close stdin so a prompt cannot hang them, and they read transaction hashes from the `Transaction ID:` line.
- Gas for `settle` and `withdraw` is estimated (100 Tgas attached). The measured values land in `results\summary.md`.
