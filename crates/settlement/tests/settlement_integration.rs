use matcher_api::{base_amount, quote_amount, Market, Side};
use settlement::{
    settle_fill, verify_pair, FixtureTransport, QuoteClient, QuoteError, SettlementError, QUOTE_URL,
};

#[test]
fn golden_settlement_simple() {
    let market = Market {
        market_id: "NEAR-USDC".to_string(),
        base_token: "nep141:wrap.near".to_string(),
        quote_token: "nep141:usdc.near".to_string(),
        lot_size_base: 1,
        quote_per_tick_lot: 1,
    };
    let fill = matcher_api::Fill {
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
    let expected: settlement::SettlementPair =
        serde_json::from_str(include_str!("fixtures/settlement_simple.json")).expect("fixture");
    assert_eq!(pair, expected);
}

#[test]
fn random_fills_verify_balanced() {
    let market = Market {
        market_id: "TEST".to_string(),
        base_token: "nep141:base.test".to_string(),
        quote_token: "nep141:quote.test".to_string(),
        lot_size_base: 3,
        quote_per_tick_lot: 7,
    };
    let mut state: u64 = 0xDEAD_BEEF_CAFE_BABE;
    for i in 0..2000 {
        state = xorshift64star(state);
        let qty_lots = state % 1000 + 1;
        state = xorshift64star(state);
        let price_ticks = state % 10_000 + 1;
        state = xorshift64star(state);
        let taker_side = if state & 1 == 0 {
            Side::Buy
        } else {
            Side::Sell
        };
        if base_amount(&market, qty_lots).is_none() {
            continue;
        }
        if quote_amount(&market, qty_lots, price_ticks).is_none() {
            continue;
        }
        let fill = matcher_api::Fill {
            seq: i + 1,
            market_id: "TEST".to_string(),
            taker_id: "t".to_string(),
            maker_id: "m".to_string(),
            taker_account: "alice.test".to_string(),
            maker_account: "bob.test".to_string(),
            taker_side,
            price_ticks,
            qty_lots,
        };
        let pair = settle_fill(&market, &fill, "2030-01-01T00:00:00Z").expect("settle");
        verify_pair(&pair).expect("balanced");
    }
}

fn xorshift64star(mut x: u64) -> u64 {
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

#[test]
fn settle_errors() {
    let market = Market {
        market_id: "M".to_string(),
        base_token: "nep141:a".to_string(),
        quote_token: "nep141:b".to_string(),
        lot_size_base: 1,
        quote_per_tick_lot: 1,
    };
    let base_fill = matcher_api::Fill {
        seq: 1,
        market_id: "M".to_string(),
        taker_id: "t".to_string(),
        maker_id: "m".to_string(),
        taker_account: "alice.test".to_string(),
        maker_account: "bob.test".to_string(),
        taker_side: Side::Buy,
        price_ticks: 1,
        qty_lots: 1,
    };

    let mut wrong_market = base_fill.clone();
    wrong_market.market_id = "OTHER".to_string();
    assert_eq!(
        settle_fill(&market, &wrong_market, "2030-01-01T00:00:00Z"),
        Err(SettlementError::MarketIdMismatch)
    );

    let mut zero_qty = base_fill.clone();
    zero_qty.qty_lots = 0;
    assert_eq!(
        settle_fill(&market, &zero_qty, "2030-01-01T00:00:00Z"),
        Err(SettlementError::ZeroQuantity)
    );

    let overflow_market = Market {
        market_id: "M".to_string(),
        base_token: "nep141:a".to_string(),
        quote_token: "nep141:b".to_string(),
        lot_size_base: u128::MAX,
        quote_per_tick_lot: 2,
    };
    let mut big = base_fill.clone();
    big.qty_lots = 2;
    assert_eq!(
        settle_fill(&overflow_market, &big, "2030-01-01T00:00:00Z"),
        Err(SettlementError::AmountOverflow)
    );

    let huge_market = Market {
        market_id: "M".to_string(),
        base_token: "nep141:a".to_string(),
        quote_token: "nep141:b".to_string(),
        lot_size_base: i128::MAX as u128,
        quote_per_tick_lot: 1,
    };
    let mut one_lot = base_fill.clone();
    one_lot.qty_lots = 2;
    assert_eq!(
        settle_fill(&huge_market, &one_lot, "2030-01-01T00:00:00Z"),
        Err(SettlementError::AmountExceedsI128)
    );
}

#[test]
fn intent_json_field_names() {
    let market = Market {
        market_id: "M".to_string(),
        base_token: "nep141:base".to_string(),
        quote_token: "nep141:quote".to_string(),
        lot_size_base: 10,
        quote_per_tick_lot: 1,
    };
    let fill = matcher_api::Fill {
        seq: 1,
        market_id: "M".to_string(),
        taker_id: "t".to_string(),
        maker_id: "m".to_string(),
        taker_account: "alice.test".to_string(),
        maker_account: "bob.test".to_string(),
        taker_side: Side::Sell,
        price_ticks: 2,
        qty_lots: 3,
    };
    let pair = settle_fill(&market, &fill, "2030-01-01T00:00:00Z").expect("settle");
    let seller_json = serde_json::to_string(&pair.seller_draft.intents[0]).expect("json");
    let v: serde_json::Value = serde_json::from_str(&seller_json).expect("parse");
    assert_eq!(v["intent"], "token_diff");
    assert_eq!(v["diff"]["nep141:base"], "-30");
    assert!(!seller_json.contains('+'));
}

#[test]
fn quote_http_errors() {
    let body_400 = include_str!("fixtures/quote_error_400.json");
    let transport = FixtureTransport::new().with_response(QUOTE_URL, 400, body_400);
    let client = QuoteClient::new(transport);
    let req = settlement::funding_quote_request(
        "nep141:eth",
        "nep141:usdc.near",
        "1",
        "alice.test",
        "0x1",
        "2030-01-01T00:00:00Z",
        50,
    );
    assert!(matches!(
        client.quote(&req),
        Err(QuoteError::InvalidInput(_))
    ));

    let body_401 = include_str!("fixtures/quote_error_401.json");
    let transport = FixtureTransport::new().with_response(QUOTE_URL, 401, body_401);
    let client = QuoteClient::new(transport);
    assert!(matches!(
        client.quote(&req),
        Err(QuoteError::InvalidApiKey(_))
    ));
}
