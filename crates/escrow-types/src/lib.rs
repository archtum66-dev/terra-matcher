#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

mod u128_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<u128>().map_err(serde::de::Error::custom)
    }
}

mod u64_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<u64>().map_err(serde::de::Error::custom)
    }
}

/// Signed limit order fields in canonical JSON field order (see `docs/ESCROW.md` section 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub v: u8,
    pub contract_id: String,
    pub account_id: String,
    pub base_token: String,
    pub quote_token: String,
    #[serde(with = "u128_string")]
    pub lot_size_base: u128,
    #[serde(with = "u128_string")]
    pub quote_per_tick_lot: u128,
    pub side: Side,
    pub price_ticks: u64,
    pub qty_lots: u64,
    #[serde(with = "u64_string")]
    pub expiry_ns: u64,
    #[serde(with = "u64_string")]
    pub nonce: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedOrder {
    pub order: Order,
    pub signature: String,
}

/// Compact JSON bytes with fields in struct declaration order (no whitespace).
pub fn canonical_bytes(order: &Order) -> Result<Vec<u8>, String> {
    serde_json::to_vec(order).map_err(|e| e.to_string())
}

/// `(base_amount, quote_amount)` for a fill at `price_ticks`, or `None` on overflow.
pub fn fill_amounts(order: &Order, fill_lots: u64, price_ticks: u64) -> Option<(u128, u128)> {
    let qty = u128::from(fill_lots);
    let base = qty.checked_mul(order.lot_size_base)?;
    let price = u128::from(price_ticks);
    let quote = qty.checked_mul(price)?.checked_mul(order.quote_per_tick_lot)?;
    Some((base, quote))
}

#[cfg(feature = "hash")]
pub fn order_hash(order: &Order) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let Ok(bytes) = canonical_bytes(order) else {
        return [0u8; 32];
    };
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[cfg(feature = "hash")]
pub fn order_hash_hex(order: &Order) -> String {
    order_hash(order)
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use matcher_api::{base_amount, quote_amount, Market};

    fn example_order() -> Order {
        Order {
            v: 1,
            contract_id: "escrow.example.testnet".to_string(),
            account_id: "alice.testnet".to_string(),
            base_token: "base.testnet".to_string(),
            quote_token: "quote.testnet".to_string(),
            lot_size_base: 1_000_000,
            quote_per_tick_lot: 10,
            side: Side::Buy,
            price_ticks: 100,
            qty_lots: 5,
            expiry_ns: 1_793_000_000_000_000_000,
            nonce: 1,
        }
    }

    #[test]
    fn canonical_matches_golden_fixture() {
        let order = example_order();
        let bytes = canonical_bytes(&order).expect("serialize");
        let fixture = include_str!("../tests/fixtures/order_canonical.json");
        assert_eq!(String::from_utf8(bytes).expect("utf8"), fixture.trim());
    }

    #[test]
    fn json_round_trip() {
        let order = example_order();
        let bytes = canonical_bytes(&order).expect("serialize");
        let parsed: Order = serde_json::from_slice(&bytes).expect("parse");
        let again = canonical_bytes(&parsed).expect("serialize again");
        assert_eq!(bytes, again);
        assert_eq!(parsed, order);
    }

    #[test]
    fn fill_amounts_overflow() {
        let mut order = example_order();
        order.lot_size_base = u128::MAX;
        assert!(fill_amounts(&order, 2, 1).is_none());
        order.lot_size_base = 1;
        order.quote_per_tick_lot = u128::MAX;
        assert!(fill_amounts(&order, 2, 2).is_none());
    }

    #[test]
    fn fill_amounts_matches_matcher_api() {
        let order = example_order();
        let market = Market {
            market_id: "T".to_string(),
            base_token: order.base_token.clone(),
            quote_token: order.quote_token.clone(),
            lot_size_base: order.lot_size_base,
            quote_per_tick_lot: order.quote_per_tick_lot,
        };
        for (fill_lots, price_ticks) in [(5u64, 100u64), (1, 50), (3, 99)] {
            let got = fill_amounts(&order, fill_lots, price_ticks);
            let base = base_amount(&market, fill_lots);
            let quote = quote_amount(&market, fill_lots, price_ticks);
            assert_eq!(got, base.zip(quote));
        }
    }
}
