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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Market {
    pub market_id: String,
    pub base_token: String,
    pub quote_token: String,
    #[serde(with = "u128_string")]
    pub lot_size_base: u128,
    #[serde(with = "u128_string")]
    pub quote_per_tick_lot: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tif {
    Gtc,
    Ioc,
    PostOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub account: String,
    pub side: Side,
    pub price_ticks: u64,
    pub qty_lots: u64,
    pub tif: Tif,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancel {
    pub id: String,
    pub account: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Order(Order),
    Cancel(Cancel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    InvalidQty,
    InvalidPrice,
    InvalidId,
    DuplicateId,
    PostOnlyWouldCross,
    UnknownOrder,
    NotOwner,
    AmountOverflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {
    pub id: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<RejectReason>,
    pub filled_lots: u64,
    pub resting_lots: u64,
    pub cancelled_lots: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    pub seq: u64,
    pub market_id: String,
    pub taker_id: String,
    pub maker_id: String,
    pub taker_account: String,
    pub maker_account: String,
    pub taker_side: Side,
    pub price_ticks: u64,
    pub qty_lots: u64,
}

pub trait Matcher {
    fn process(&mut self, msg: Message) -> (Ack, Vec<Fill>);
}
