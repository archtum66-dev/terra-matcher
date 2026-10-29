#![forbid(unsafe_code)]

mod error;
mod intent;
mod quote;
mod reference;
mod settle;
mod wallet;

pub use error::SettlementError;
pub use intent::{Intent, IntentDraft, SettlementPair};
pub use quote::{
    funding_quote_request, reference_quote_request, DepositType, FixtureTransport, Quote,
    QuoteClient, QuoteError, QuoteRequest, QuoteResponse, RecipientType, RecordingTransport,
    RefundType, SwapType, Transport, TransportError, TransportResponse, QUOTE_URL,
};
pub use reference::{reference_check, ReferenceCheck, ReferenceCheckError};
pub use settle::{settle_fill, verify_pair};
pub use wallet::{nep413_message, nonce_b64, raw_payload};

#[cfg(feature = "http")]
pub use quote::HttpTransport;
