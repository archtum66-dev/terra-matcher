use matcher_api::{run_vector, Ack, Fill, Market, Matcher, Message, Vector};
use matcher_stub::{BookView, StubMatcher};
use serde::Serialize;
use settlement::{settle_fill, SettlementPair};

/// Fixed settlement deadline (no wall clock), same value as `terra settle`.
pub const DEADLINE: &str = "2030-01-01T00:00:00Z";

/// The public conformance vectors, embedded at build time.
pub const VECTORS: [(&str, &str); 22] = [
    (
        "amount_overflow",
        include_str!("../../../tests/vectors/amount_overflow.json"),
    ),
    (
        "cancel_not_owner",
        include_str!("../../../tests/vectors/cancel_not_owner.json"),
    ),
    (
        "cancel_ok",
        include_str!("../../../tests/vectors/cancel_ok.json"),
    ),
    (
        "cancel_unknown",
        include_str!("../../../tests/vectors/cancel_unknown.json"),
    ),
    (
        "duplicate_id",
        include_str!("../../../tests/vectors/duplicate_id.json"),
    ),
    (
        "fill_at_maker_price",
        include_str!("../../../tests/vectors/fill_at_maker_price.json"),
    ),
    (
        "invalid_id",
        include_str!("../../../tests/vectors/invalid_id.json"),
    ),
    (
        "ioc_no_liquidity",
        include_str!("../../../tests/vectors/ioc_no_liquidity.json"),
    ),
    (
        "ioc_with_remainder",
        include_str!("../../../tests/vectors/ioc_with_remainder.json"),
    ),
    (
        "multi_price_level",
        include_str!("../../../tests/vectors/multi_price_level.json"),
    ),
    (
        "partial_maker",
        include_str!("../../../tests/vectors/partial_maker.json"),
    ),
    (
        "partial_taker",
        include_str!("../../../tests/vectors/partial_taker.json"),
    ),
    (
        "post_only_rejected",
        include_str!("../../../tests/vectors/post_only_rejected.json"),
    ),
    (
        "post_only_resting",
        include_str!("../../../tests/vectors/post_only_resting.json"),
    ),
    (
        "post_only_vs_own_order",
        include_str!("../../../tests/vectors/post_only_vs_own_order.json"),
    ),
    (
        "price_time_same_level",
        include_str!("../../../tests/vectors/price_time_same_level.json"),
    ),
    (
        "reject_validation",
        include_str!("../../../tests/vectors/reject_validation.json"),
    ),
    (
        "self_trade",
        include_str!("../../../tests/vectors/self_trade.json"),
    ),
    (
        "self_trade_after_partial_fill",
        include_str!("../../../tests/vectors/self_trade_after_partial_fill.json"),
    ),
    (
        "sell_taker_multi_level",
        include_str!("../../../tests/vectors/sell_taker_multi_level.json"),
    ),
    (
        "seq_continues_across_messages",
        include_str!("../../../tests/vectors/seq_continues_across_messages.json"),
    ),
    (
        "simple_match",
        include_str!("../../../tests/vectors/simple_match.json"),
    ),
];

#[derive(Serialize)]
#[serde(untagged)]
enum SettleOut {
    Pair(Box<SettlementPair>),
    Failed { fill_seq: u64, error: String },
}

#[derive(Serialize)]
struct StepOut {
    ack: Ack,
    fills: Vec<Fill>,
    settlements: Vec<SettleOut>,
    book: BookView,
}

#[derive(Serialize)]
struct StartOut<'a> {
    market: &'a Market,
    deadline: &'static str,
    book: BookView,
}

/// One matching session: a market, the reference matcher and every message it has seen.
pub struct Session {
    market: Market,
    matcher: StubMatcher,
    inputs: Vec<Message>,
}

impl Session {
    pub fn new(market_json: &str) -> Result<Self, String> {
        let market: Market =
            serde_json::from_str(market_json).map_err(|e| format!("invalid market: {e}"))?;
        Ok(Self {
            matcher: StubMatcher::new(market.clone()),
            market,
            inputs: Vec::new(),
        })
    }

    /// JSON describing the fresh session (market, deadline, empty book).
    pub fn start_json(&self) -> Result<String, String> {
        serde_json::to_string(&StartOut {
            market: &self.market,
            deadline: DEADLINE,
            book: self.matcher.book_view(),
        })
        .map_err(|e| e.to_string())
    }

    /// Process one message (JSON). Returns ack, fills, unsigned settlement drafts and the book.
    pub fn step(&mut self, message_json: &str) -> Result<String, String> {
        let msg: Message =
            serde_json::from_str(message_json).map_err(|e| format!("invalid message: {e}"))?;
        self.inputs.push(msg.clone());
        let (ack, fills) = self.matcher.process(msg);
        let settlements = fills
            .iter()
            .map(|fill| match settle_fill(&self.market, fill, DEADLINE) {
                Ok(pair) => SettleOut::Pair(Box::new(pair)),
                Err(e) => SettleOut::Failed {
                    fill_seq: fill.seq,
                    error: e.to_string(),
                },
            })
            .collect();
        serde_json::to_string(&StepOut {
            ack,
            fills,
            settlements,
            book: self.matcher.book_view(),
        })
        .map_err(|e| e.to_string())
    }

    /// Every message of this session as JSON lines, ready for `terra match --orders`.
    pub fn export_jsonl(&self) -> Result<String, String> {
        let mut out = String::new();
        for msg in &self.inputs {
            out.push_str(&serde_json::to_string(msg).map_err(|e| e.to_string())?);
            out.push('\n');
        }
        Ok(out)
    }
}

#[derive(Serialize)]
struct VectorResult {
    name: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Runs all embedded conformance vectors on the reference matcher; JSON array of results.
pub fn conformance_report() -> String {
    let results: Vec<VectorResult> = VECTORS
        .iter()
        .map(|(name, data)| {
            let outcome = serde_json::from_str::<Vector>(data)
                .map_err(|e| format!("parse: {e}"))
                .and_then(|v| run_vector(StubMatcher::new, &v));
            VectorResult {
                name: (*name).to_string(),
                ok: outcome.is_ok(),
                error: outcome.err(),
            }
        })
        .collect();
    serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string())
}
