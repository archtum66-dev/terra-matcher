# Terra matcher (v0.1)

Non-custodial order matching for a NEAR Intents exchange. Limit orders are paired **off-chain** by a deterministic matcher that never holds funds. Every fill becomes a pair of **unsigned `token_diff` intents** that nets to zero for each token. The parties sign their own half; the NEAR Intents verifier settles. A NEAR testnet escrow contract is the second settlement path.

**Status:** early scaffold, honestly labelled. Working and tested: matcher interface, 22 conformance vectors, reference matcher, settlement mapping, dry quote client, command-line tool, browser demo. Written and unit-tested only: escrow contract (not deployed, not audited).

## Try it in 30 seconds (no install)

Open **`web/index.html`** in any current browser. It is one self-contained file: the reference matcher runs as WebAssembly, offline, with no server, no keys and no network.

- Place orders or pick a scenario (price-time priority, self-trade, post-only, IOC) and watch the book and the fills.
- Each fill shows its unsigned `token_diff` pair and the check that every token nets to zero. Nothing is signed or sent.
- **Run vectors** executes the 22 public conformance vectors inside your browser tab.
- **Download orders.jsonl** gives a file that replays on the command line with identical output.

The page embeds a 267 KB module built from `crates/web-demo`. `python3 web/build.py --check` fails unless the committed page equals a fresh build, see "Rebuild the demo" below.

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

Generate a large deterministic order flow and measure (details and results in `docs/BENCHMARKS.md`):

```text
cargo run --release -p terra-cli -- gen --messages 100000 --seed 1 > flow.jsonl
cargo run --release -p terra-cli -- match --market examples/market.json --orders flow.jsonl
cargo run --release -p terra-cli -- bench --messages 1000000 --seed 1 --repeat 5
```

Any other implementation of the `Matcher` trait can be fed the same `flow.jsonl`. Its output must equal the reference output line by line.

### Rebuild the demo

```text
rustup target add wasm32-unknown-unknown
cargo build -p terra-web --target wasm32-unknown-unknown --profile wasm --locked
python3 web/build.py --check
```

The build is reproducible on the same toolchain (Rust 1.97.0, Linux x86_64): two clean builds gave the same SHA-256.
Use `python3 web/build.py` instead of `--check` to regenerate `web/index.html` after changing the code.

## What is verified

| Claim | How it is checked |
|---|---|
| The matcher follows `docs/SPEC.md` | 22 vectors in `tests/vectors`, run natively and in the browser demo |
| The browser module equals the command line | 3000 random messages: byte-identical output (see `docs/BENCHMARKS.md`) |
| Matching is deterministic and never crosses the book | random-flow invariant tests in `crates/matcher-stub/tests` |
| Every settlement pair nets to zero | checked in `settlement` before output, and again in the demo page |
| Another engine can replace the reference matcher | the same vectors and `terra gen` flows; outputs are compared line by line |
| Commands in this README work | `crates/terra-cli/tests/cli.rs` starts the real binary for `demo`, `match`, `settle`, `gen`, `bench` |

`cargo test --all` runs 43 tests offline. The contract has its own 26 tests (see below).

## Settlement

Each fill produces exactly one **pair** of unsigned `token_diff` intent drafts (buyer and seller). Amounts follow fixed lots from `base_amount` and `quote_amount`. The settlement crate checks that token sums are zero before output. Nothing is signed in this repo. Quote requests built or sent through the 1Click client always use `"dry": true`.

## Escrow contract

The NEAR testnet escrow holds NEP-141 deposits as free balances, verifies ed25519-signed limit orders, and applies fills when anyone calls `settle`. Rules, error strings, and events are defined in `docs/ESCROW.md`. A manual testnet checklist is in `docs/TESTNET.md`. See [`testnet/README.md`](testnet/README.md) (Testnet run kit: scripts for the full run: accounts, deploy, keys, deposit, settle, withdraw).

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
| `terra-cli` | `terra` binary: `match`, `settle`, `demo`, `gen`, `bench`; optional `quote` with `--features http` |
| `web-demo` (`terra-web`) | The reference matcher and settlement drafts as a WebAssembly module for `web/index.html` |

## Folders

| Path | Content |
|---|---|
| `crates/` | Rust crates (table above) |
| `contract/escrow/` | NEAR testnet escrow contract (own Cargo workspace) |
| `web/` | Browser demo: `index.html` (built), `index.template.html`, `build.py` |
| `tests/vectors/` | Public conformance vectors (JSON) |
| `docs/` | `SPEC.md`, `BENCHMARKS.md`, `ESCROW.md`, `TESTNET.md`, `INTENTS_FACTS.md` |
| `examples/` | Example market and orders |

## Not here yet

- A hosted copy of the demo (the page works from a local file)
- Escrow deployed and exercised on a live testnet
- Live verifier or 1Click integration in default builds (use `settlement` / `terra-cli` `http` feature only when you intentionally call the public quote API with `ONECLICK_API_KEY`)

## Limits

Quote and settlement fixtures are hand-written from public documentation, not recorded from a live system. Wallet message JSON shapes are marked `UNVERIFIED` in code until checked against `intents.near`. The escrow contract has unit tests only; gas values for `withdraw` are estimates. The browser demo and the benchmarks use the reference matcher and do not talk to NEAR. On-chain gas and fees are not measured (see `docs/BENCHMARKS.md`).

## AI assistance

Parts of this repository were drafted with Cursor and Claude and reviewed by hand.
