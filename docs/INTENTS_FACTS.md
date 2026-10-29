# NEAR Intents interface facts

Reference notes for settlement and quote code. Written from public documentation on 2026-10-29.

- Verifier contract: `intents.near`, Mainnet only, there is no testnet. Methods `execute_intents` and `simulate_intents`.
- `token_diff` intent: `{"intent":"token_diff","diff":{"<token id>":"<amount>", ...}}`. Token ids look like `nep141:usdc.near`. Amounts are decimal strings. Negative means give, positive means receive (no plus sign). Over all `token_diff` intents of one batch, every token must sum to zero, otherwise the whole batch fails.
- Signed message (NEP-413 standard): `payload.message` is a JSON string with `signer_id`, `deadline` (ISO 8601) and `intents`. Other standards (for example `raw_ed25519`) carry a JSON string with `signer_id`, `verifying_contract`, `deadline`, `nonce`, `intents`.
- Nonce: 32 bytes, base64. First 4 bytes are the contract's current salt, the other 28 bytes are unique data.
- 1Click quote: `POST https://1click.chaindefuser.com/v0/quote`. Required request fields: `dry`, `swapType` (`EXACT_INPUT`, `EXACT_OUTPUT`, `FLEX_INPUT`), `slippageTolerance` (basis points), `originAsset`, `depositType` (`ORIGIN_CHAIN`, `INTENTS`), `destinationAsset`, `amount` (string, smallest unit), `refundTo`, `refundType` (`ORIGIN_CHAIN`, `INTENTS`), `recipient`, `recipientType` (`DESTINATION_CHAIN`, `INTENTS`), `deadline` (ISO 8601). Optional: `depositMode`, `referral`, `quoteWaitingTimeMs`, `appFees`. With `dry: true` the answer has no `depositAddress` and no `deadline` in the quote.
- Quote answer: `correlationId`, `timestamp`, `signature`, `quoteRequest`, `quote` with `amountIn`, `minAmountIn`, `amountOut`, `minAmountOut`, `timeEstimate` (seconds) and optional formatted and USD fields.
- Authentication: header `X-API-Key` (preferred) or `Authorization: Bearer <JWT>`. Errors: 400 invalid input, 401 invalid key.
- Status of this file: written from the public documentation on 2026-10-29. Items not verified against a live system are marked in code comments with `UNVERIFIED`.
