use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum Intent {
    TokenDiff { diff: BTreeMap<String, String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentDraft {
    pub signer_id: String,
    pub deadline: String,
    pub intents: Vec<Intent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementPair {
    pub fill_seq: u64,
    pub buyer: String,
    pub seller: String,
    pub buyer_draft: IntentDraft,
    pub seller_draft: IntentDraft,
}
