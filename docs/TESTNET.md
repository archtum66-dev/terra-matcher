# Escrow on NEAR testnet (manual checklist)

This document is a **do X, see Y** guide for deploying and exercising the escrow contract with [near-cli-rs](https://github.com/near/near-cli-rs). **Untested on a live network** — steps are written from the contract spec, not from a recorded testnet run.

Replace placeholders: `<ESCROW_ACCOUNT>`, `<ALICE>`, `<BOB>`, `<BASE_TOKEN>`, `<QUOTE_TOKEN>`.

## 1. Create a testnet account

Do create a named account on testnet, see [NEAR testnet wallet / CLI docs](https://docs.near.org/tools/near-cli).

```bash
near account create-account sponsor-by-faucet-service <ESCROW_ACCOUNT> autogenerate-new-keypair save-to-keychain network-config testnet create
```

Repeat for trader accounts `<ALICE>` and `<BOB>` if needed.

## 2. Build the WASM

Do build the contract crate, see `contract/escrow/Cargo.toml` release profile.

```bash
cd contract/escrow
cargo near build non-reproducible-wasm
```

Alternative:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Artifact path depends on the tool (typically `target/near/escrow.wasm` or `target/wasm32-unknown-unknown/release/escrow.wasm`).

The plain `cargo build` variant was built in the drafting environment (about 360 KB); `cargo near build` was not run.

## 3. Deploy

Do deploy to `<ESCROW_ACCOUNT>`, see NEAR CLI `deploy` help.

```bash
near contract deploy <ESCROW_ACCOUNT> use-file <PATH_TO_escrow.wasm> without-init-call network-config testnet sign-with-keychain send
```

If your flow requires an init call:

```bash
near contract deploy <ESCROW_ACCOUNT> use-file <PATH_TO_escrow.wasm> with-init-call new json-args {} network-config testnet sign-with-keychain send
```

## 4. Register order keys

Each trader registers a 32-byte ed25519 public key (64 hex characters). Do call `register_key`, see `docs/ESCROW.md` section 2.

```bash
near contract call-function as-access-key <ALICE> register_key json-args '{"public_key_hex":"<64_HEX_CHARS>"}' prepaid-gas '30 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-keychain send
```

## 5. Deposit (NEP-141)

Do send tokens with `ft_transfer_call` and an **empty** `msg` so the escrow credits free balance, see `ft_on_transfer` in `docs/ESCROW.md`.

```bash
near contract call-function as-transaction <BASE_TOKEN> ft_transfer_call json-args '{"receiver_id":"<ESCROW_ACCOUNT>","amount":"<AMOUNT>","msg":""}' prepaid-gas '100 Tgas' attached-deposit '1 yoctoNEAR' sign-as <ALICE> network-config testnet sign-with-keychain send
```

Deposit quote token for the buyer and base token for the seller before settling.

## 6. Settle a fill

Do submit matching signed orders (`buy`, `sell`, `fill_lots`, `price_ticks`), see `docs/ESCROW.md` section 3. JSON shapes come from `docs/ESCROW.md` section 1.

```bash
near contract call-function as-transaction <ESCROW_ACCOUNT> settle json-args '{"buy":{"order":{...},"signature":"<128_HEX>"},"sell":{"order":{...},"signature":"<128_HEX>"},"fill_lots":5,"price_ticks":100}' prepaid-gas '100 Tgas' attached-deposit '0 NEAR' sign-as <ANY_ACCOUNT> network-config testnet sign-with-keychain send
```

Anyone may call `settle`; signatures must verify against each trader's registered key.

## 7. Withdraw free balance

Do attach **1 yoctoNEAR** and call `withdraw`, see `docs/ESCROW.md` section 2. `withdraw` forwards 30 Tgas to the token's `ft_transfer` and 10 Tgas to its own callback, so attach at least 60 Tgas. These values are estimates; adjust them after the first live run.

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
