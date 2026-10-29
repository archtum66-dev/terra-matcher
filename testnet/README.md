# Testnet run kit (steps 1-7)

PowerShell scripts to prepare the escrow contract and test tokens on NEAR **testnet**. For the contract rules, see `docs/ESCROW.md`. **Untested on a live network.** The scripts were run only against a local simulator of near-cli-rs and of the RPC (command forms and JSON arguments checked); the first live run is yours. They send transactions only when you start them.

## Prerequisites

- Windows PowerShell 5.1 or newer
- [near-cli-rs](https://github.com/near/near-cli-rs): `irm https://github.com/near/near-cli-rs/releases/latest/download/near-cli-rs-installer.ps1 | iex`
- WASM files `escrow.wasm` and `terra_test_token.wasm` under `testnet\wasm\` (prebuilt copies with `SHA256SUMS.txt` can be dropped in; or build with `2-check-wasm.ps1 -Build` when Rust and target `wasm32-unknown-unknown` are installed, or copy artifacts in manually)
- `terra-keys` at `testnet\bin\terra-keys.exe`, on `PATH`, or Rust to run `cargo run -q -p terra-keys --`

Optional environment variables: `TERRA_RPC_URL`, `TERRA_STATE_FILE`, `TERRA_NEAR_BIN`.

## Step 1 — Create accounts

Do create five faucet-funded accounts and save public state, see `1-create-accounts.ps1`.

```powershell
cd testnet
.\1-create-accounts.ps1 -Prefix terra1
```

You should see `NEXT: run 2-check-wasm.ps1` and a table of account balances. Keys are stored in the Windows keychain (`save-to-keychain`), not in the repo.

## Step 2 — Check WASM

Do verify (or build) contract WASM, see `2-check-wasm.ps1`.

```powershell
.\2-check-wasm.ps1
# or
.\2-check-wasm.ps1 -Build
```

You should see file sizes, SHA-256 hashes, and estimated storage bind (~0.00001 NEAR per byte of code).

## Step 3 — Deploy

Do deploy escrow and both token contracts, then call `new` in separate transactions, see `3-deploy.ps1`.

```powershell
.\3-deploy.ps1
```

You should see `NEXT: run 4-register-keys.ps1` after the view checks (`ft_metadata` symbols of both tokens, the escrow answers `get_key`). A second run skips contracts that are already deployed (`-Force` redeploys the code and keeps the state).

## Step 4 — Register order keys

Do generate `*.order-key` files outside the repo and call `register_key`, see `4-register-keys.ps1`.

```powershell
.\4-register-keys.ps1
```

Default key directory: `%USERPROFILE%\.terra-testnet\keys`. Output lists account, public key hex, and key **path** (never file contents).

## Step 5 — Deposit

Do register NEP-145 storage, mint test tokens, and `ft_transfer_call` into the escrow, see `5-deposit.ps1`.

```powershell
.\5-deposit.ps1
```

Default amounts: `100000000` base units to Bob (base token), `1000000000` quote units to Alice (6 decimals). A balance table must match before the script succeeds.

## Step 6 — Settle

Do sign matching buy/sell orders with `terra-keys`, submit two fills via `settle`, and expect an overfill attempt to fail, see `6-settle.ps1`.

```powershell
.\6-settle.ps1
```

Anyone may call `settle`; the default submitter is the base token contract account (not Alice or Bob). The script runs two partial fills, then tries one lot more than remains open — the contract should reject that; a failed `near` exit code there is normal.

Example manual tools (placeholders):

```powershell
terra-keys make-order --key-file <PATH>\alice.order-key --contract-id <ESCROW> --account-id <ALICE> --base-token <BASE> --quote-token <QUOTE> --lot-size-base 1000000 --quote-per-tick-lot 1000 --side buy --price-ticks 100 --qty-lots 5 --nonce 1 --expires-in-secs 3600
terra-keys settle-args --buy-file orders\buy.json --sell-file orders\sell.json --fill-lots 3 --price-ticks 95
terra-keys verify-order --signed-order-file orders\buy.json --key-file <PATH>\alice.order-key
```

Signed order JSON is written under `testnet\orders\` (gitignored). Transaction gas and hashes from step 6 appear in the settle table on stdout.

## Step 7 — Withdraw

Do withdraw all non-zero escrow balances for Alice and Bob, see `7-withdraw.ps1`.

```powershell
.\7-withdraw.ps1
```

Writes `testnet\results\summary.md` with a table of all recorded transactions (labels, Tgas, links to testnet.nearblocks.io).

## Run all steps

```powershell
.\run-1-to-5.ps1 -Prefix terra1
.\run-1-to-7.ps1 -Prefix terra1
```

Or double-click `RUN-1-TO-5.bat` or `RUN-1-TO-7.bat` and enter a prefix.

Use `-DryRun` on any script to print `near` commands without sending transactions (RPC view helpers still run where noted in `lib.ps1`).

## Costs

- Faucet-funded account creation (testnet limits apply)
- Contract storage: about **1 NEAR per 100 KB** of WASM code on the account, plus small deposits for NEP-141 / NEP-145 registration (`0.01 NEAR` per registration in step 5)

## Security

- NEAR account keys: Windows keychain via near-cli-rs
- Order keys: `*.order-key` under `%USERPROFILE%\.terra-testnet\keys` by default — back them up; never commit them
- Scripts refuse the word `mainnet` in `near` argument lists
- Logs and console output redact `ed25519:` keys, seed-phrase and secret-key lines, and the paths of `*.order-key` files; no script prints or logs key file contents

Logs: `testnet\logs\`. State (public IDs and key hex only): `testnet\state.json`. Full-run transaction summary: `testnet\results\summary.md`.

## Troubleshooting

- Faucet errors on step 1: wait and re-run; existing accounts are skipped
- Missing WASM: run step 2 with `-Build` or copy files into `testnet\wasm\`
- `-DryRun` to inspect commands; compare logs under `testnet\logs\`

Still **untested on a live network** for steps 6 and 7 until you run them on testnet.
