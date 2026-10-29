use matcher_api::{base_amount, quote_amount, Market};

fn test_market(lot: u128, quote_per_tick: u128) -> Market {
    Market {
        market_id: "T".to_string(),
        base_token: "base".to_string(),
        quote_token: "quote".to_string(),
        lot_size_base: lot,
        quote_per_tick_lot: quote_per_tick,
    }
}

#[test]
fn base_amount_simple() {
    let m = test_market(10, 1);
    assert_eq!(base_amount(&m, 3), Some(30));
}

#[test]
fn quote_amount_simple() {
    let m = test_market(1, 5);
    assert_eq!(quote_amount(&m, 2, 100), Some(1000));
}

#[test]
fn base_amount_overflow() {
    let m = test_market(u128::MAX, 1);
    assert_eq!(base_amount(&m, 2), None);
}

#[test]
fn quote_amount_overflow_on_price_multiply() {
    let m = test_market(1, u128::MAX);
    assert_eq!(quote_amount(&m, 2, 2), None);
}

#[test]
fn quote_amount_overflow_on_qty_multiply() {
    let m = test_market(1, u128::MAX / 2 + 1);
    assert_eq!(quote_amount(&m, 2, 1), None);
}
