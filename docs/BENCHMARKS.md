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
| How large is the escrow contract? | `cd contract/escrow && RUSTFLAGS="--cfg near" cargo build --target wasm32-unknown-unknown --release` | **343 KB** wasm (351,397 bytes) |
| What does one `settle` cost on testnet? | `testnet/RUN-1-TO-7.bat`, two complete runs | **4.4 to 4.5 Tgas** burnt in total (4.36, 4.45, 4.42, 4.46); fee about **0.0004 NEAR**. One fill, the signatures of both orders are verified |
| What does one `withdraw` cost on testnet? | same | **6.95 Tgas** burnt in total (6.947 to 6.957 in eight calls), including the token's `ft_transfer` and the callback; fee about **0.00063 NEAR** |
| Does the whole trade work on a real network? | same | **Yes, twice.** Deposit of both tokens, two fills (3 lots at 95, 2 lots at 100), an overfill rejected with `overfill`, four withdrawals; final balances match the fills |

## Testnet run (30 October 2026)

The run kit in `testnet/` created five accounts (`ag2610-escrow`, `-alice`, `-bob`, `-base`, `-quote`, all `.testnet`), deployed the escrow and two test tokens, registered both order keys, deposited, settled twice, tried an overfill and withdrew everything. It was run twice on the same accounts; the second run deposited again because the first had withdrawn all balances. near-cli-rs 0.30.1, escrow built with `--cfg near` (351,368 bytes in these two runs), test token 300,274 bytes.

Gas is the sum of `gas_burnt` over the transaction and all its receipts, read from the RPC. Fees are the burnt tokens of the same calls. Every row is a real testnet transaction; the links open the explorer.

| Call | Tgas run 1 | Tgas run 2 | Fee run 1 (NEAR) | Transaction run 1 | Transaction run 2 |
|---|---:|---:|---:|---|---|
| settle-1 | 4.36 | 4.42 | 0.00041 | [BnHFaRnw…](https://testnet.nearblocks.io/txns/BnHFaRnwScUr1iLjgkcvyPbAvkVH4krpgkMSkNQe4Z6o) | [82WxXjhV…](https://testnet.nearblocks.io/txns/82WxXjhV2xQAWJrACmJnUMPnqLbpfdtTKk1nVk2ywvB6) |
| settle-2 | 4.45 | 4.46 | 0.00042 | [DcXcJWNf…](https://testnet.nearblocks.io/txns/DcXcJWNf9GGojhV7r9oqHwnaXp7CY6Xpz59KocoQbz7P) | [8vqFdfBY…](https://testnet.nearblocks.io/txns/8vqFdfBYzJhPh26D1sSpEsz1WoYJfwT5iqgXj1LJy8fx) |
| settle-overfill-rejected | - | - | - | rejected with `overfill`, as intended | rejected with `overfill`, as intended |
| withdraw-alice-base | 6.96 | 6.96 | 0.00063 | [82yCZ3Tk…](https://testnet.nearblocks.io/txns/82yCZ3TkNSqnrwqoWdbhFUxS9b2cWrtc6RBFwjejkaH) | [3g9qKKPN…](https://testnet.nearblocks.io/txns/3g9qKKPNd11HptE2SFmxKj4ENWcfMAHMZJ6HTrUMug8Y) |
| withdraw-alice-quote | 6.95 | 6.95 | 0.00063 | [FgbpYqp7…](https://testnet.nearblocks.io/txns/FgbpYqp7BEBux8AXNdnrFYrb6PoibJxF27BdPjZHMUvy) | [13ySDfvK…](https://testnet.nearblocks.io/txns/13ySDfvKx6zbJidXWdmPLLjWcPSgVgTK5yzjFCvAGur3) |
| withdraw-bob-base | 6.95 | 6.95 | 0.00063 | [HFTCLHho…](https://testnet.nearblocks.io/txns/HFTCLHho5NzYVcPpTWRL76hNTn86EUJGGhiZ8Rbb8mLf) | [B7zpCrpV…](https://testnet.nearblocks.io/txns/B7zpCrpVkB1QU6Kx3JXvyHZqRiEsFANYiQcC9XbuPgUi) |
| withdraw-bob-quote | 6.95 | 6.95 | 0.00063 | [FNaZ56BM…](https://testnet.nearblocks.io/txns/FNaZ56BMBb4Nsaugsj4KpmAm81LF1gBmu8dLJQHm85LA) | [9PoVpHTg…](https://testnet.nearblocks.io/txns/9PoVpHTgvKCamNYK9fHbMrYih3QdjoH2he34SNeb32X) |

**Settled in each run:** 3 lots at price 95 (285,000 quote units) and 2 lots at price 100 (200,000 quote units), 5,000,000 base units in total.
**Final balances after the withdrawals, both runs:** Alice 5,000,000 base and 999,515,000 quote; Bob 95,000,000 base and 485,000 quote. All four balances were withdrawn; the amounts match the fills (for example Alice: 1,000,000,000 deposited minus 485,000 paid = 999,515,000).
**Reading the numbers:** a transaction may use at most 300 Tgas, so one `settle` takes about 1.5 % of that. The 100 Tgas attached by the kit is far more than needed; the contract itself reserves 30 + 10 Tgas for the token transfer and the callback of `withdraw`, so at least 60 Tgas must stay attached to `withdraw`.

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

- **Gas of the token deposit** (`ft_transfer_call` on the token contract) and of `register_key`. The kit's summary lists only `settle` and `withdraw`.
- **Mainnet.** Everything on chain was measured on testnet only.
- **Latency over a network.** The demo and the benchmark run in process.
- **Settlement throughput on NEAR.** The figures above cover building the drafts, not submitting them.
