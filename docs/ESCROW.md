# Escrow Contract v0.1 (NEAR)

A small non-custodial settlement contract. **Nobody can freeze or take your funds, not even the matcher operator.**

- Traders deposit NEP-141 tokens. A deposit is a free balance that its owner can withdraw at any time.
- Orders are signed off-chain with a trader's **order key** (ed25519). Nothing is locked while an order is open.
- **Anyone** may submit a fill (`settle`). The contract checks both signatures and both limit prices. The matcher only proposes pairs; it has no special rights and cannot move funds.
- Orders expire (`expiry_ns`) and can be cancelled on chain. Withdrawing the balance also stops an order from filling.

This contract runs on NEAR **testnet**. NEAR Intents has no testnet, so the Intents part of this repo works with dry-run quotes and unsigned `token_diff` drafts (see `docs/SPEC.md`, section 4).

## 1 Signed order

Canonical JSON (fields in exactly this order, no whitespace, strings for 128-bit and nanosecond values):

```json
{"v":1,"contract_id":"escrow.example.testnet","account_id":"alice.testnet","base_token":"base.testnet","quote_token":"quote.testnet","lot_size_base":"1000000","quote_per_tick_lot":"10","side":"buy","price_ticks":100,"qty_lots":5,"expiry_ns":"1793000000000000000","nonce":"1"}
```

| Field | Type | Rule |
|---|---|---|
| `v` | u8 | must be 1 |
| `contract_id` | string | must equal the escrow account (replay protection across deployments) |
| `account_id` | string | the trader |
| `base_token`, `quote_token` | string | NEP-141 contract account ids (not the `nep141:` Intents form) |
| `lot_size_base`, `quote_per_tick_lot` | string (u128) | as in `docs/SPEC.md`, section 1 |
| `side` | `"buy"` or `"sell"` | |
| `price_ticks` | u64 | limit price, greater than 0 |
| `qty_lots` | u64 | greater than 0 |
| `expiry_ns` | string (u64) | block timestamp in nanoseconds; the order is valid while `block_timestamp < expiry_ns` |
| `nonce` | string (u64) | any value; makes otherwise identical orders distinct |

**Order hash** = `sha256(canonical JSON bytes)`. **Signature** = ed25519 signature over the 32 hash bytes, hex encoded (128 characters). The contract re-serialises the order it receives, so whitespace in the call arguments does not matter.

## 2 Methods

| Method | Who | What |
|---|---|---|
| `ft_on_transfer(sender_id, amount, msg)` | token contract | credits the sender's free balance of the calling token; returns `"0"` |
| `register_key(public_key_hex)` | the trader (predecessor) | sets the trader's order key (32 bytes, 64 hex characters); replaces an earlier key |
| `withdraw(token_id, amount, receiver_id?)` | the trader, 1 yoctoNEAR attached | debits first, then `ft_transfer`; if the transfer fails the callback credits the amount back |
| `settle(buy, sell, fill_lots, price_ticks)` | anyone | see section 3 |
| `cancel_order(order_hash)` | the trader | marks that hash cancelled for the caller |
| `balance_of(account_id, token_id)` | view | free balance |
| `order_state(account_id, order_hash)` | view | `{ filled_lots, cancelled }` |
| `get_key(account_id)` | view | registered order key or null |

`buy` and `sell` are `{ "order": <Order>, "signature": "<hex>" }`.

## 3 `settle` rules (all must hold, otherwise the call panics and nothing changes)

1. `v == 1`, both `contract_id` equal this account, both orders not expired.
2. `buy.side == buy`, `sell.side == sell`, accounts differ.
3. Both orders have the same `base_token`, `quote_token`, `lot_size_base`, `quote_per_tick_lot`.
4. Each order's account has a registered key and the signature verifies over the order hash.
5. Neither order hash is cancelled.
6. `sell.price_ticks <= price_ticks <= buy.price_ticks` (the fill price lies inside both limits).
7. `fill_lots > 0` and for each order `filled_lots + fill_lots <= qty_lots` (no overfill).
8. `base_amount = fill_lots * lot_size_base` and `quote_amount = fill_lots * price_ticks * quote_per_tick_lot`, both with checked arithmetic.
9. The buyer's free `quote_token` balance is at least `quote_amount`; the seller's free `base_token` balance is at least `base_amount`.

Effects: buyer `-quote_amount` quote, `+base_amount` base; seller `+quote_amount` quote, `-base_amount` base; both `filled_lots` grow by `fill_lots`; one `fill` event is logged.

## 4 Event (NEP-297)

`EVENT_JSON:{"standard":"terra_escrow","version":"0.1.0","event":"fill","data":{"buy_hash":"..","sell_hash":"..","buyer":"..","seller":"..","fill_lots":5,"price_ticks":100,"base_amount":"..","quote_amount":".."}}`

Further events: `deposit`, `withdraw`, `cancel`, `key_registered`.

## 5 What the operator can and cannot do

| Can | Cannot |
|---|---|
| propose pairs of signed orders (anyone can) | settle outside the signed limits |
| choose the fill price inside both limits | fill an expired, cancelled or overfilled order |
| stay offline: traders withdraw and cancel on their own | withdraw anyone's balance |

**Known limits of v0.1:** storage costs are paid by the contract account (a fee model is not built); a fill price anywhere inside the limit range is allowed (price-time fairness is the matcher's job, not the contract's); no upgrade path, no admin key. Testnet only. Not audited.
