# Terra matcher (v0.1)

Non-custodial matcher interface and a public in-memory stub for a NEAR Intents exchange. The matcher pairs limit orders off-chain; it never holds funds. Settlement builds unsigned `token_diff` intent pairs for the NEAR Intents verifier.

**Status:** Early scaffold — types, conformance vectors, reference stub, settlement mapping, dry quote client, a dry-run CLI, and an escrow contract for NEAR testnet (unit-tested only, not deployed).

## Quick start

1. Run `cargo test --all` from the repository root — all tests pass (conformance vectors, settlement balances, quote fixtures, determinism, invariant fuzz, amount checks).
2. Run `cargo run -p terra-cli -- demo` — writes `examples/market.json` and `examples/orders.jsonl`, prints matcher acks and fills, then one settlement JSON line per fill, for example:

```json
{"ack":{"id":"s1","status":"accepted","filled_lots":0,"resting_lots":5,"cancelled_lots":0}}
{"ack":{"id":"b1","status":"accepted","filled_lots":5,"resting_lots":0,"cancelled_lots":0}}
{"fill":{"seq":1,"market_id":"NEAR-USDC","taker_id":"b1","maker_id":"s1","taker_account":"bob.test","maker_account":"alice.test","taker_side":"buy","price_ticks":100,"qty_lots":5}}
{"settlement":{"fill_seq":1,"buyer":"bob.test","seller":"alice.test","buyer_draft":{"signer_id":"bob.test","deadline":"2030-01-01T00:00:00Z","intents":[{"intent":"token_diff","diff":{"nep141:usdc.near":"-500","nep141:wrap.near":"5"}}]},"seller_draft":{"signer_id":"alice.test","deadline":"2030-01-01T00:00:00Z","intents":[{"intent":"token_diff","diff":{"nep141:usdc.near":"500","nep141:wrap.near":"-5"}}]}}}
```

3. Read `docs/SPEC.md` for message formats, matching rules, and settlement mapping. See `docs/INTENTS_FACTS.md` for verifier and 1Click quote field notes.

Replay a custom scenario:

```text
cargo run -p terra-cli -- match --market examples/market.json --orders examples/orders.jsonl
cargo run -p terra-cli -- settle --market examples/market.json --orders examples/orders.jsonl
```

## Settlement

Each fill produces exactly one **pair** of unsigned `token_diff` intent drafts (buyer and seller). Amounts follow fixed lots from `base_amount` and `quote_amount`. The settlement crate checks that token sums are zero before output. Nothing is signed in this repo. Quote requests built or sent through the 1Click client always use `"dry": true`.

## Escrow contract

The NEAR testnet escrow holds NEP-141 deposits as free balances, verifies ed25519-signed limit orders, and applies fills when anyone calls `settle`. Rules, error strings, and events are defined in `docs/ESCROW.md`. A manual testnet checklist (placeholders only) is in `docs/TESTNET.md`.

**State:** builds to wasm (about 360 KB) and passes 26 unit tests. **Not deployed, not tested on a live network, not audited.**

1. `cargo test --all` (repository root) runs the matcher, settlement and `escrow-types` tests. `cargo test -p escrow-types --features hash` adds the order-hash test.
2. `cd contract/escrow && cargo test` runs the contract unit tests (the contract has its own Cargo workspace).
3. `cd contract/escrow && cargo build --target wasm32-unknown-unknown --release` builds the contract (needs the `wasm32-unknown-unknown` Rust target).

## Crates

| Crate | Role |
|---|---|
| `escrow-types` | Canonical signed-order JSON, fill amount helpers, optional order hash |
| `matcher-api` | Types, `Matcher` trait, amount helpers, conformance runner |
| `matcher-stub` | Public reference matcher (`StubMatcher`) |
| `settlement` | Fill → `token_diff` pair, wallet message helpers, dry quote client |
| `terra-cli` | `terra` dry-run binary (`match`, `settle`, `demo`; optional `quote` with `--features http`) |

## Not here yet

- Hosted demo
- Escrow deployed and exercised on a live testnet
- Live verifier or 1Click integration in default builds (use `settlement` / `terra-cli` `http` feature only when you intentionally call the public quote API with `ONECLICK_API_KEY`)

## Limits

Quote and settlement fixtures are hand-written from public documentation, not recorded from a live system. Wallet message JSON shapes are marked `UNVERIFIED` in code until checked against `intents.near`. The escrow contract has unit tests only; gas values for `withdraw` are estimates.

## AI assistance

Parts of this repository were drafted with Cursor and Claude and reviewed by hand.
