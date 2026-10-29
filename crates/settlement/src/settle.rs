use matcher_api::{base_amount, quote_amount, Market, Side};
use std::collections::BTreeMap;

use crate::error::SettlementError;
use crate::intent::{Intent, IntentDraft, SettlementPair};

const I128_MAX_U128: u128 = i128::MAX as u128;

pub fn settle_fill(
    market: &Market,
    fill: &matcher_api::Fill,
    deadline: &str,
) -> Result<SettlementPair, SettlementError> {
    if fill.market_id != market.market_id {
        return Err(SettlementError::MarketIdMismatch);
    }
    if fill.qty_lots == 0 {
        return Err(SettlementError::ZeroQuantity);
    }

    let base = base_amount(market, fill.qty_lots).ok_or(SettlementError::AmountOverflow)?;
    let quote =
        quote_amount(market, fill.qty_lots, fill.price_ticks).ok_or(SettlementError::AmountOverflow)?;

    if base > I128_MAX_U128 || quote > I128_MAX_U128 {
        return Err(SettlementError::AmountExceedsI128);
    }

    let (buyer_account, seller_account) = match fill.taker_side {
        Side::Buy => (&fill.taker_account, &fill.maker_account),
        Side::Sell => (&fill.maker_account, &fill.taker_account),
    };

    let base_str = base.to_string();
    let quote_str = quote.to_string();
    let neg_base = format_amount_neg(base);
    let neg_quote = format_amount_neg(quote);

    let mut buyer_diff = BTreeMap::new();
    buyer_diff.insert(market.base_token.clone(), base_str.clone());
    buyer_diff.insert(market.quote_token.clone(), neg_quote);

    let mut seller_diff = BTreeMap::new();
    seller_diff.insert(market.base_token.clone(), neg_base);
    seller_diff.insert(market.quote_token.clone(), quote_str);

    let buyer_draft = IntentDraft {
        signer_id: buyer_account.clone(),
        deadline: deadline.to_string(),
        intents: vec![Intent::TokenDiff { diff: buyer_diff }],
    };
    let seller_draft = IntentDraft {
        signer_id: seller_account.clone(),
        deadline: deadline.to_string(),
        intents: vec![Intent::TokenDiff { diff: seller_diff }],
    };

    let pair = SettlementPair {
        fill_seq: fill.seq,
        buyer: buyer_account.clone(),
        seller: seller_account.clone(),
        buyer_draft,
        seller_draft,
    };

    verify_pair(&pair)?;
    Ok(pair)
}

pub fn verify_pair(pair: &SettlementPair) -> Result<(), SettlementError> {
    let mut sums: BTreeMap<String, i128> = BTreeMap::new();

    for draft in [&pair.buyer_draft, &pair.seller_draft] {
        for intent in &draft.intents {
            if let Intent::TokenDiff { diff } = intent {
                for (token, amount_str) in diff {
                    let parsed = parse_intent_amount(amount_str)
                        .ok_or_else(|| SettlementError::InvalidIntentAmount {
                            token: token.clone(),
                        })?;
                    let entry = sums.entry(token.clone()).or_insert(0);
                    *entry = entry
                        .checked_add(parsed)
                        .ok_or(SettlementError::AmountOverflow)?;
                }
            }
        }
    }

    for (token, sum) in sums {
        if sum != 0 {
            return Err(SettlementError::UnbalancedPair { token, sum });
        }
    }
    Ok(())
}

fn format_amount_neg(amount: u128) -> String {
    format!("-{}", amount)
}

fn parse_intent_amount(s: &str) -> Option<i128> {
    s.parse::<i128>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use matcher_api::Fill;

    fn sample_market() -> Market {
        Market {
            market_id: "NEAR-USDC".to_string(),
            base_token: "nep141:wrap.near".to_string(),
            quote_token: "nep141:usdc.near".to_string(),
            lot_size_base: 1,
            quote_per_tick_lot: 1,
        }
    }

    #[test]
    fn settle_simple_fill() {
        let market = sample_market();
        let fill = Fill {
            seq: 1,
            market_id: "NEAR-USDC".to_string(),
            taker_id: "b1".to_string(),
            maker_id: "s1".to_string(),
            taker_account: "bob.test".to_string(),
            maker_account: "alice.test".to_string(),
            taker_side: Side::Buy,
            price_ticks: 100,
            qty_lots: 5,
        };
        let pair = settle_fill(&market, &fill, "2030-01-01T00:00:00Z").expect("settle");
        assert_eq!(pair.buyer, "bob.test");
        assert_eq!(pair.seller, "alice.test");
        verify_pair(&pair).expect("balanced");
    }
}
