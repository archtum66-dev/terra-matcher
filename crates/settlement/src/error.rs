use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettlementError {
    MarketIdMismatch,
    ZeroQuantity,
    AmountOverflow,
    AmountExceedsI128,
    UnbalancedPair { token: String, sum: i128 },
    InvalidIntentAmount { token: String },
}

impl fmt::Display for SettlementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MarketIdMismatch => write!(f, "fill market_id does not match market"),
            Self::ZeroQuantity => write!(f, "fill qty_lots is zero"),
            Self::AmountOverflow => write!(f, "amount overflow computing base or quote"),
            Self::AmountExceedsI128 => write!(f, "amount exceeds i128::MAX"),
            Self::UnbalancedPair { token, sum } => {
                write!(f, "token {token} sums to {sum}, expected 0")
            }
            Self::InvalidIntentAmount { token } => {
                write!(f, "invalid decimal amount for token {token}")
            }
        }
    }
}

impl std::error::Error for SettlementError {}
