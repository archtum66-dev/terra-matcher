# Terra matcher (v0.1)

Non-custodial matcher interface and a public in-memory stub for a NEAR Intents exchange. The matcher pairs limit orders off-chain; it never holds funds. Settlement is a separate step (not implemented here).

**Status:** Early scaffold — types, conformance vectors, reference stub, and a dry-run CLI.

## Quick start

1. Run `cargo test` from the repository root — all tests pass (conformance vectors, determinism, invariant fuzz, amount checks).
2. Run `cargo run -p terra-cli -- demo` — writes `examples/market.json` and `examples/orders.jsonl`, then prints one JSON line per ack and fill, for example:

```json
{"ack":{"id":"s1","status":"accepted","filled_lots":0,"resting_lots":5,"cancelled_lots":0}}
{"ack":{"id":"b1","status":"accepted","filled_lots":5,"resting_lots":0,"cancelled_lots":0}}
{"fill":{"seq":1,"market_id":"NEAR-USDC","taker_id":"b1","maker_id":"s1","taker_account":"bob.near","maker_account":"alice.near","taker_side":"buy","price_ticks":100,"qty_lots":5}}
```

3. Read `docs/SPEC.md` for message formats, matching rules, and settlement mapping.

Replay a custom scenario:

```text
cargo run -p terra-cli -- match --market examples/market.json --orders examples/orders.jsonl
```

## Crates

| Crate | Role |
|---|---|
| `matcher-api` | Types, `Matcher` trait, amount helpers, conformance runner |
| `matcher-stub` | Public reference matcher (`StubMatcher`) |
| `terra-cli` | `terra` dry-run binary |

## Not here yet

- Settlement via NEAR Intents
- NEAR contract
- Hosted demo

## AI assistance

Parts of this repository were drafted with Cursor and Claude and reviewed by hand.
