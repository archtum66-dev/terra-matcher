use crate::types::Market;

/// `qty_lots * lot_size_base`, or `None` on overflow.
pub fn base_amount(market: &Market, qty_lots: u64) -> Option<u128> {
    let qty = u128::from(qty_lots);
    qty.checked_mul(market.lot_size_base)
}

/// `qty_lots * price_ticks * quote_per_tick_lot`, or `None` on overflow.
pub fn quote_amount(market: &Market, qty_lots: u64, price_ticks: u64) -> Option<u128> {
    let qty = u128::from(qty_lots);
    let price = u128::from(price_ticks);
    qty.checked_mul(price)?
        .checked_mul(market.quote_per_tick_lot)
}
