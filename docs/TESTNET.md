# Escrow on NEAR testnet (manual checklist)

The scripted version of this guide (all seven steps, with preflight and dry run) is in [`testnet/README.md`](../testnet/README.md).

This document is a **do X, see Y** guide for deploying and exercising the escrow contract with [near-cli-rs](https://github.com/near/near-cli-rs). **Live status:** the scripted version has run completely on testnet, twice (30 October 2026): accounts, deployment, keys, deposit, two fills, a rejected overfill and four withdrawals. The measured gas and the transaction links are in [`BENCHMARKS.md`](BENCHMARKS.md). The first attempts found two bugs that unit tests cannot see and that are now fixed and checked: the argument of `ft_on_transfer` was named `_msg`, so the generated JSON reader expected a key `_msg` and the token refunded every deposit (test `exported_argument_names_match_nep141`); and a build without `--cfg near` hid all contract error texts (step 2 and the preflight now reject such a file). The orders made by `terra-keys` are also accepted by the contract code in a test (`contract/escrow/tests/cli_interop.rs`).

Replace placeholders: `<ESCROW_ACCOUNT>`, `<ALICE>`, `<BOB>`, `<BASE_TOKEN>`, `<QUOTE_TOKEN>`.

## 1. Create a testnet account

Do create a named account on testnet, see [NEAR testnet wallet / CLI docs](https://docs.near.org/tools/near-cli).

```bash
near account create-account sponsor-by-faucet-service <ESCROW_ACCOUNT> autogenerate-new-keypair save-to-keychain network-config testnet create
```

Keys are stored in the local keychain (`save-to-keychain`), not printed to the terminal.

Repeat for trader accounts `<ALICE>` and `<BOB>` if needed.

## 2. Build the WASM

Do build the contract crate, see `contract/escrow/Cargo.toml` release profile.

```bash
cd contract/escrow
cargo near build non-reproducible-wasm
```

Alternative with plain cargo (the flag `--cfg near` is required, see below):

```bash
cd contract/escrow
RUSTFLAGS="--cfg near" cargo build --target wasm32-unknown-unknown --release
```

In PowerShell: `$env:RUSTFLAGS = "--cfg near"` before `cargo build`.

**Why `--cfg near`:** the NEAR SDK has two modes. With the flag (set by `cargo near build`) the contract uses the chain's host functions: contract errors keep their text (for example `overfill`) and hashing runs on the chain. Without it the file still runs, but every contract error shows only `WebAssembly trap: An unreachable opcode was executed`, and hashing runs in the contract code (larger file, more gas). `testnet/2-check-wasm.ps1` and the preflight reject a file that was built without the flag.

Artifact path depends on the tool (typically `target/near/escrow.wasm` or `target/wasm32-unknown-unknown/release/escrow.wasm`).

The plain `cargo build` variant with `--cfg near` was built in the drafting environment (about 343 KB); `cargo near build` was not run.

## 3. Deploy

Do deploy to `<ESCROW_ACCOUNT>`, see NEAR CLI `deploy` help.

```bash
near contract deploy <ESCROW_ACCOUNT> use-file <PATH_TO_escrow.wasm> without-init-call network-config testnet sign-with-keychain send
```

Then initialize in a **separate** transaction:

```bash
near contract call-function as-transaction <ESCROW_ACCOUNT> new json-args '{}' prepaid-gas '30 Tgas' attached-deposit '0 NEAR' sign-as <ESCROW_ACCOUNT> network-config testnet sign-with-keychain send
```

## 4. Register order keys

Each trader registers a 32-byte ed25519 public key (64 hex characters). Do call `register_key`, see `docs/ESCROW.md` section 2.

```bash
near contract call-function as-transaction <ESCROW_ACCOUNT> register_key json-args '{"public_key_hex":"<64_HEX_CHARS>"}' prepaid-gas '30 Tgas' attached-deposit '0 NEAR' sign-as <ALICE> network-config testnet sign-with-keychain send
```

## 5. Deposit (NEP-141)

The sender account and the escrow account must be registered on the token contract first (`storage_deposit`, NEP-145). The kit script `testnet/5-deposit.ps1` does this for the test tokens before minting and transferring.

Do send tokens with `ft_transfer_call` and an **empty** `msg` so the escrow credits free balance, see `ft_on_transfer` in `docs/ESCROW.md`.

```bash
near contract call-function as-transaction <BASE_TOKEN> ft_transfer_call json-args '{"receiver_id":"<ESCROW_ACCOUNT>","amount":"<AMOUNT>","msg":""}' prepaid-gas '100 Tgas' attached-deposit '1 yoctoNEAR' sign-as <ALICE> network-config testnet sign-with-keychain send
```

Deposit quote token for the buyer and base token for the seller before settling.

## 6. Settle a fill

Do build signed orders with `terra-keys`, assemble `settle` JSON with `terra-keys settle-args`, then submit the transaction, see `docs/ESCROW.md` section 3. Step `testnet/6-settle.ps1` automates two fills and an overfill check.

Create and sign a buy order:

```bash
terra-keys make-order --key-file <ORDER_KEY_FILE> --contract-id <ESCROW_ACCOUNT> --account-id <ALICE> --base-token <BASE_TOKEN> --quote-token <QUOTE_TOKEN> --lot-size-base 1000000 --quote-per-tick-lot 1000 --side buy --price-ticks 100 --qty-lots 5 --nonce 1 --expires-in-secs 3600
```

Build settle arguments from buy and sell order files:

```bash
terra-keys settle-args --buy-file <BUY_JSON> --sell-file <SELL_JSON> --fill-lots 3 --price-ticks 95
```

Submit (use the JSON line from `settle-args` as `json-args`):

```bash
near contract call-function as-transaction <ESCROW_ACCOUNT> settle json-args '<SETTLE_ARGS_JSON>' prepaid-gas '100 Tgas' attached-deposit '0 NEAR' sign-as <ANY_ACCOUNT> network-config testnet sign-with-keychain send
```

Anyone may call `settle`; signatures must verify against each trader's registered key.

## 7. Withdraw free balance

Do attach **1 yoctoNEAR** and call `withdraw`, see `docs/ESCROW.md` section 2. `testnet/7-withdraw.ps1` withdraws all four escrow balances for Alice and Bob and writes `testnet/results/summary.md`. `withdraw` forwards 30 Tgas to the token's `ft_transfer` and 10 Tgas to its own callback, so attach at least 60 Tgas. Measured on testnet: a whole `withdraw` burns about 6.95 Tgas in total.

```bash
near contract call-function as-transaction <ESCROW_ACCOUNT> withdraw json-args '{"token_id":"<QUOTE_TOKEN>","amount":"<AMOUNT>","receiver_id":null}' prepaid-gas '100 Tgas' attached-deposit '1 yoctoNEAR' sign-as <ALICE> network-config testnet sign-with-keychain send
```

## What you will see

On a successful fill, the contract logs a NEP-297 style line (see `docs/ESCROW.md` section 4), for example:

```json
EVENT_JSON:{"standard":"terra_escrow","version":"0.1.0","event":"fill","data":{"buy_hash":"<hex>","sell_hash":"<hex>","buyer":"<ALICE>","seller":"<BOB>","fill_lots":5,"price_ticks":100,"base_amount":"<string>","quote_amount":"<string>"}}
```

Deposits, withdrawals, cancellations, key registration and refunds of failed withdrawals emit `deposit`, `withdraw`, `cancel`, `key_registered` and `withdraw_refund` events with the same `EVENT_JSON:` prefix.

## Views

```bash
near contract call-function as-read-only <ESCROW_ACCOUNT> balance_of json-args '{"account_id":"<ALICE>","token_id":"<QUOTE_TOKEN>"}' network-config testnet now
near contract call-function as-read-only <ESCROW_ACCOUNT> order_state json-args '{"account_id":"<ALICE>","order_hash":"<64_HEX>"}' network-config testnet now
```
