# Benchmarks

Everything here is measured with commands in this repository. Nothing is estimated.

## What was measured

| Question | Command | Result |
|---|---|---|
| How fast does the reference matcher process a realistic order flow? | `cargo run --release -p terra-cli -- bench --messages 1000000 --seed 1 --repeat 5` | about **0.3 to 0.5 million messages per second** on one core (five runs: 0.32, 0.39, 0.42, 0.46, 0.48) |
| How fast are settlement drafts built? | same command, field `settlement_pairs_per_second` | about **1.1 million pairs per second** (1.10 to 1.14) |
| How big is one settlement? | same command, field `settlement_pair_json_bytes_avg` | **407 bytes** of JSON per fill (buyer and seller draft together) |
| Does the browser demo match the command line? | 3000 random messages through the WebAssembly module and through `terra match` | **byte-identical** output (4157 lines, 1157 fills, 688 rejections) |
| How large is the browser module? | `cargo build -p terra-web --target wasm32-unknown-unknown --profile wasm --locked` | **267 KB** (the whole demo page, module included, is 383 KB) |
| How large is the escrow contract? | `cd contract/escrow && RUSTFLAGS="--cfg near" cargo build --target wasm32-unknown-unknown --release` | **343 KB** wasm |

## The order flow

`terra gen --messages N --seed S` writes a deterministic flow, so any matcher can be timed and compared on the identical input.
With `--messages 1000000 --seed 1` it contains:

- 868,599 accepted and 131,401 rejected messages (post-only that would cross, cancels of unknown or foreign orders, self-trade stops),
- 604,111 fills, 7,899,525 lots matched,
- 133,795 orders still resting at the end.

Mix: about 88 % orders (78 % GTC, 14 % IOC, 8 % post-only) and 12 % cancels, 64 accounts, prices uniform in a band of 61 ticks.
The book grows to a six-figure depth on purpose, so the numbers include the cost of cancelling in deep queues.

## Environment and how to read the numbers

- Shared cloud VM, 2 vCPU Intel Xeon at 2.1 GHz, Linux, Rust 1.97.0, release build, one thread.
- Run-to-run spread on this machine was about ±25 %. The table gives the range, not a single best value.
- The reference matcher is written for clarity, not speed. For example, cancelling scans the queue of its price level.
  Production engines belong behind the same `Matcher` trait and must produce the same output on the same flow (`tests/vectors`, `docs/SPEC.md`).

## What was not measured

- **On-chain gas and fees.** They need a live testnet run with funded accounts and keys, which this repository does not contain.
  `docs/TESTNET.md` lists the calls to make. The gas numbers in that file are estimates and are marked as such.
- **Latency over a network.** The demo and the benchmark run in process.
- **Settlement throughput on NEAR.** The figures above cover building the drafts, not submitting them.
