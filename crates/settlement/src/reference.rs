use crate::quote::QuoteResponse;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ReferenceCheck {
    pub deviation_bps: i128,
    pub within_tolerance: bool,
}

pub fn reference_check(
    fill_quote_amount: u128,
    quote: &QuoteResponse,
    tolerance_bps: u32,
) -> Result<ReferenceCheck, ReferenceCheckError> {
    let q = quote
        .quote
        .as_ref()
        .ok_or(ReferenceCheckError::MissingQuote)?;
    let amount_out_str = q
        .amount_out
        .as_ref()
        .ok_or(ReferenceCheckError::MissingAmountOut)?;
    let ref_amount: u128 = amount_out_str
        .parse()
        .map_err(|_| ReferenceCheckError::InvalidAmount)?;

    if ref_amount == 0 {
        return Err(ReferenceCheckError::ZeroReference);
    }

    let fill_i = i128::try_from(fill_quote_amount).map_err(|_| ReferenceCheckError::Overflow)?;
    let ref_i = i128::try_from(ref_amount).map_err(|_| ReferenceCheckError::Overflow)?;

    let deviation_bps = fill_i
        .checked_sub(ref_i)
        .and_then(|d| d.checked_mul(10_000))
        .and_then(|n| n.checked_div(ref_i))
        .ok_or(ReferenceCheckError::Overflow)?;

    let tol = i128::from(tolerance_bps);
    let within_tolerance = if deviation_bps >= 0 {
        deviation_bps <= tol
    } else {
        -deviation_bps <= tol
    };

    Ok(ReferenceCheck {
        deviation_bps,
        within_tolerance,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceCheckError {
    MissingQuote,
    MissingAmountOut,
    InvalidAmount,
    ZeroReference,
    Overflow,
}

impl std::fmt::Display for ReferenceCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingQuote => write!(f, "quote response missing quote"),
            Self::MissingAmountOut => write!(f, "quote missing amountOut"),
            Self::InvalidAmount => write!(f, "invalid amountOut"),
            Self::ZeroReference => write!(f, "reference amount is zero"),
            Self::Overflow => write!(f, "arithmetic overflow"),
        }
    }
}

impl std::error::Error for ReferenceCheckError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quote::Quote;

    fn sample_response(amount_out: &str) -> QuoteResponse {
        QuoteResponse {
            correlation_id: None,
            timestamp: None,
            signature: None,
            quote_request: None,
            quote: Some(Quote {
                amount_in: Some("1000000".to_string()),
                min_amount_in: Some("995000".to_string()),
                amount_out: Some(amount_out.to_string()),
                min_amount_out: Some("9900000".to_string()),
                time_estimate: Some(120),
            }),
        }
    }

    #[test]
    fn reference_check_same_price() {
        let q = sample_response("500");
        let r = reference_check(500, &q, 10).expect("ok");
        assert_eq!(r.deviation_bps, 0);
        assert!(r.within_tolerance);
    }

    #[test]
    fn reference_check_worse_for_seller() {
        let q = sample_response("500");
        let r = reference_check(480, &q, 50).expect("ok");
        assert_eq!(r.deviation_bps, -400);
        assert!(!r.within_tolerance);
    }

    #[test]
    fn reference_check_better_for_seller() {
        let q = sample_response("500");
        let r = reference_check(520, &q, 50).expect("ok");
        assert_eq!(r.deviation_bps, 400);
        assert!(!r.within_tolerance);
    }
}
