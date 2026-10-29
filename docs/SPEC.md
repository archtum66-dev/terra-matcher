# Matcher Interface v0.1

An off-chain matcher pairs signed limit orders. **It never holds funds and cannot move them.**
Settlement is a separate step: every fill becomes one exact `token_diff` pair on the NEAR Intents verifier (`intents.near`), signed by both traders.
Anything that speaks this interface can be plugged in: the public stub in this repo, or a private high-performance engine.

## 1 Market

| Field | Type | Meaning |
|---|---|---|
| `market_id` | string | e.g. `NEAR-USDC` |
| `base_token` | string | Intents asset id, e.g. `nep141:wrap.near` |
| `quote_token` | string | Intents asset id, e.g. `nep141:usdc.near` |
| `lot_size_base` | string (u128) | base units per lot |
| `quote_per_tick_lot` | string (u128) | quote units per (tick x lot) |

**Amounts (integer only, checked arithmetic, no floats):**

- `base_amount  = qty_lots * lot_size_base`
- `quote_amount = qty_lots * price_ticks * quote_per_tick_lot`

An overflow rejects the order (`amount_overflow`).

## 2 Messages (JSON, `snake_case`)

**Order** (`type = "order"`)

| Field | Type | Rule |
|---|---|---|
| `id` | string | unique per matcher run, 1 to 64 chars |
| `account` | string | NEAR account id of the trader |
| `side` | `"buy"` or `"sell"` | |
| `price_ticks` | u64 | must be greater than 0 |
| `qty_lots` | u64 | must be greater than 0 |
| `tif` | `"gtc"`, `"ioc"`, `"post_only"` | see section 3 |

**Cancel** (`type = "cancel"`): `id` (string), `account` (string). Only the owner can cancel.

**Ack** (one per input message)

| Field | Type | Meaning |
|---|---|---|
| `id` | string | order or cancel id |
| `status` | `"accepted"` or `"rejected"` | |
| `reason` | string, only if rejected | `invalid_qty`, `invalid_price`, `invalid_id`, `duplicate_id`, `post_only_would_cross`, `unknown_order`, `not_owner`, `amount_overflow` |
| `filled_lots` | u64 | filled by this message |
| `resting_lots` | u64 | left in the book after this message |
| `cancelled_lots` | u64 | removed by this message (ioc remainder, self-trade, cancel) |

**Fill**

| Field | Type | Meaning |
|---|---|---|
| `seq` | u64 | 1, 2, 3, ... per matcher run, gap-free |
| `market_id` | string | |
| `taker_id`, `maker_id` | string | order ids |
| `taker_account`, `maker_account` | string | |
| `taker_side` | `"buy"` or `"sell"` | |
| `price_ticks` | u64 | **the maker's price** |
| `qty_lots` | u64 | greater than 0 |

## 3 Matching rules

1. **Price-time priority.** Best price first, within one price level the oldest order first.
2. **Fills trade at the maker's price.** A buyer never pays more than their limit, a seller never receives less.
3. **`gtc`:** the remainder rests in the book. **`ioc`:** the remainder is cancelled. **`post_only`:** rejected with `post_only_would_cross` if it would match at once, otherwise it rests.
4. **Self-trade:** if the incoming order would match a resting order of the **same account**, matching stops and the incoming remainder is cancelled (cancel-newest). No fill is produced, the resting order stays. This also applies to `post_only`: if the first crossing price level is headed by the same account, the order is accepted and its remainder is cancelled (not rejected with `post_only_would_cross`).
5. **Determinism:** the same input sequence always produces the same acks and fills, byte for byte. No clocks, no randomness.
6. **Cancel** removes the resting remainder. Cancelling an unknown id gives `unknown_order`, someone else's id gives `not_owner`.
7. **Output order:** for every input message the matcher emits the ack first, then the fills of that message in `seq` order.
8. **Invariants** (checked by tests): no fill with zero quantity; for every order the sum of its fills never exceeds its quantity; after every message the best bid is lower than the best ask; `seq` has no gaps.

## 4 Settlement mapping (one fill, one pair)

A fill is settled in **fixed lots**: one fill gives exactly one pair of `token_diff` intents with exact amounts. A partial fill of an order simply gives several fills and therefore several pairs. A quote is never split.

For a fill with `base_amount` and `quote_amount` (section 1), buyer `B` and seller `S`:

```json
{ "intent": "token_diff", "diff": { "<base_token>": "<base_amount>", "<quote_token>": "-<quote_amount>" } }   // signer B
{ "intent": "token_diff", "diff": { "<base_token>": "-<base_amount>", "<quote_token>": "<quote_amount>" } }   // signer S
```

Amounts are decimal strings, negative means "give", positive means "receive" (no plus sign). Per the verifier's rule, the sum over the pair must be zero for every token. The settlement crate checks this before anything is shown to a wallet.
Signing is done by the traders' wallets. **This repo never holds a key.**

## 5 Safety rules for every implementation

1. No private keys, seeds, tokens or API keys in code, tests, fixtures or logs.
2. Quote requests to the 1Click API are sent with `"dry": true` only. A test enforces it.
3. No Mainnet transaction is ever sent from this repo.
4. If the matcher is down, funds stay with their owners: orders live as signed intents with a deadline and can be cancelled or simply expire.

## 6 Conformance

`tests/vectors/*.json` hold input sequences with the expected acks and fills. Any implementation must reproduce them exactly.
The public stub passes them in CI. A private engine can be checked against the same files.
