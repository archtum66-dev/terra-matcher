# Testnet run kit (steps 1-7)

PowerShell scripts that put the escrow contract and two test tokens on NEAR **testnet** and run a full trade: deposit, two partial fills, a rejected overfill, withdrawal. For the contract rules see `docs/ESCROW.md`.

**Status:** the complete run (steps 1 to 7) worked on the real testnet on 30 October 2026, and twice more on later starts with the same prefix, using near-cli-rs v0.30.1. The measured gas and the transaction links are in `docs/BENCHMARKS.md`. Two bugs found on the way are fixed (see "Lessons from the live runs" below). The scripts are also tested against a local simulator of near-cli-rs and of the RPC (command forms, JSON arguments, re-runs, stale views, failing exit codes, a refunded deposit). Testnet only: every script refuses the word `mainnet`. Transactions are sent only when you start a script.

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
| 2 | `2-check-wasm.ps1` | Sizes and SHA-256 of both WASM files, storage estimate, and a check that they were built for the chain (`--cfg near`). `-Build` builds them with Rust (`RUSTFLAGS=--cfg near`, target `wasm32-unknown-unknown`). |
| 3 | `3-deploy.ps1` | Deploys escrow and both tokens, calls `new` in a separate transaction, checks with views. |
| 4 | `4-register-keys.ps1` | Creates alice's and bob's order keys outside the repo and calls `register_key`. |
| 5 | `5-deposit.ps1` | NEP-145 storage deposits, mints test tokens, `ft_transfer_call` into the escrow, checks the balances. |
| 6 | `6-settle.ps1` | Signs a buy and a sell order with `terra-keys`, settles 3 lots at 95 and 2 lots at 100, then sends one lot too many and expects the contract to answer `overfill`. |
| 7 | `7-withdraw.ps1` | Withdraws all non-zero escrow balances of alice and bob, checks that the escrow balances are zero and the tokens arrived, writes `results\summary.md`. |

`run-1-to-5.ps1` and `run-1-to-7.ps1 -Prefix <p>` run the steps in order. `-DryRun` works on every script.

## Re-runs

- Steps 1 and 3 skip what already exists. Step 3 compares the code hash on chain with the local WASM file: identical code is skipped, changed code is deployed again and the contract state is kept (`new` is then rejected as already initialised, which is expected and ignored). `3-deploy.ps1 -Force` redeploys in any case.
- Step 5 deposits only what the escrow still lacks and mints only what the wallet still lacks, so it can be repeated. If the token refunds a deposit, it stops at once with the message `DEPOSIT REFUNDED` instead of waiting.
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

Account creation is funded by the testnet faucet. Contract storage is bound at about 1 NEAR per 100 KB of code (about 3.5 NEAR for the escrow, 3.0 NEAR for each token), plus small storage deposits in step 5. Whether the faucet still funds this much is only known after step 1.

## Lessons from the live runs

- **Argument names are the JSON keys.** The NEAR SDK reads the call arguments by the Rust parameter names. A parameter written `_msg` (to silence an unused-variable warning) is read as the key `_msg`, so the token's `ft_on_transfer(sender_id, amount, msg)` call cannot be read, the contract panics and the token refunds the full amount. Unit tests that call the Rust method directly do not see this. Two tests now check the generated call arguments and the source for underscore-prefixed parameters of exported methods.
- **Build the contracts with `RUSTFLAGS=--cfg near`.** A plain `cargo build --target wasm32-unknown-unknown` compiles, deploys and runs, but the SDK then uses guest-side fallbacks: contract errors show only `unreachable` (the second live start of step 3 stopped on exactly that: the already-initialised `new` call gave no text) and hashing costs more gas. Step 2 and the preflight now reject such a file; step 3 also treats a bare `unreachable` from `new` as "already initialised".
- Receipts of other contracts are not shown by near-cli-rs; whether a deposit arrived is checked with the view `balance_of` and, on a timeout, with the token balance of the sender (tokens back = refunded).

## Remaining risks

- Gas is measured: `settle` burns about 4.4 Tgas and `withdraw` about 7 Tgas in total (100 Tgas is attached, which is generous). Every run writes its values to `results\summary.md` and `results\results.json`.
- The faucet may rate limit new accounts. Step 1 then fails; wait and re-run, existing accounts are skipped. (It worked in the first live run.)
- near-cli-rs prompt text and output labels may differ from v0.30.1. The scripts close stdin so a prompt cannot hang them, and they read transaction hashes from the `Transaction ID:` line (as printed by v0.30.1 in the first live run).
