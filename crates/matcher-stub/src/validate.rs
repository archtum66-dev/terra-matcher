use matcher_api::{base_amount, quote_amount, Market, Order, RejectReason};

pub fn validate_order_id(id: &str) -> Option<RejectReason> {
    if id.is_empty() || id.len() > 64 {
        Some(RejectReason::InvalidId)
    } else {
        None
    }
}

pub fn validate_order_fields(
    market: &Market,
    order: &Order,
    used_ids: &std::collections::BTreeSet<String>,
) -> Option<RejectReason> {
    if validate_order_id(&order.id).is_some() {
        return Some(RejectReason::InvalidId);
    }
    if used_ids.contains(&order.id) {
        return Some(RejectReason::DuplicateId);
    }
    if order.qty_lots == 0 {
        return Some(RejectReason::InvalidQty);
    }
    if order.price_ticks == 0 {
        return Some(RejectReason::InvalidPrice);
    }
    if base_amount(market, order.qty_lots).is_none()
        || quote_amount(market, order.qty_lots, order.price_ticks).is_none()
    {
        return Some(RejectReason::AmountOverflow);
    }
    None
}
