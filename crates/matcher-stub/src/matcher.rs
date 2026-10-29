use std::collections::{BTreeMap, BTreeSet, VecDeque};

use matcher_api::{
    Ack, Cancel, Fill, Market, Matcher, Message, Order, RejectReason, Side, Status, Tif,
};

use crate::validate::{validate_order_fields, validate_order_id};

#[derive(Debug, Clone)]
struct RestingOrder {
    id: String,
    account: String,
    qty_lots: u64,
}

pub struct StubMatcher {
    market: Market,
    buys: BTreeMap<u64, VecDeque<RestingOrder>>,
    sells: BTreeMap<u64, VecDeque<RestingOrder>>,
    /// Resting order location for cancel and lookup.
    location: BTreeMap<String, (Side, u64)>,
    used_ids: BTreeSet<String>,
    next_seq: u64,
}

impl StubMatcher {
    pub fn new(market: Market) -> Self {
        Self {
            market,
            buys: BTreeMap::new(),
            sells: BTreeMap::new(),
            location: BTreeMap::new(),
            used_ids: BTreeSet::new(),
            next_seq: 1,
        }
    }

    fn reject_ack(id: String, reason: RejectReason) -> Ack {
        Ack {
            id,
            status: Status::Rejected,
            reason: Some(reason),
            filled_lots: 0,
            resting_lots: 0,
            cancelled_lots: 0,
        }
    }

    /// A post-only order crosses if the first crossing price level is headed by another account.
    /// A level headed by the same account is a self-trade (cancel-newest), not a cross.
    fn would_cross_post_only(&self, order: &Order) -> bool {
        match self.peek_opposite_maker(order) {
            Some((_, maker)) => maker.account != order.account,
            None => false,
        }
    }

    fn best_bid(&self) -> Option<u64> {
        self.buys
            .iter()
            .rev()
            .find_map(|(&p, q)| if q.is_empty() { None } else { Some(p) })
    }

    fn best_ask(&self) -> Option<u64> {
        self.sells
            .iter()
            .find_map(|(&p, q)| if q.is_empty() { None } else { Some(p) })
    }

    fn push_resting(&mut self, order: Order, qty_lots: u64) {
        let resting = RestingOrder {
            id: order.id.clone(),
            account: order.account,
            qty_lots,
        };
        let price = order.price_ticks;
        let book = match order.side {
            Side::Buy => &mut self.buys,
            Side::Sell => &mut self.sells,
        };
        book.entry(price).or_default().push_back(resting);
        self.location.insert(order.id, (order.side, price));
    }

    fn remove_resting(&mut self, id: &str) -> Option<RestingOrder> {
        let (side, price) = self.location.remove(id)?;
        let book = match side {
            Side::Buy => &mut self.buys,
            Side::Sell => &mut self.sells,
        };
        let queue = book.get_mut(&price)?;
        let pos = queue.iter().position(|o| o.id == id)?;
        let removed = queue.remove(pos)?;
        if queue.is_empty() {
            book.remove(&price);
        }
        Some(removed)
    }

    fn peek_opposite_maker(&self, taker: &Order) -> Option<(u64, RestingOrder)> {
        match taker.side {
            Side::Buy => {
                for (&price, queue) in &self.sells {
                    if price > taker.price_ticks {
                        continue;
                    }
                    if let Some(maker) = queue.front() {
                        return Some((price, maker.clone()));
                    }
                }
                None
            }
            Side::Sell => {
                for (&price, queue) in self.buys.iter().rev() {
                    if price < taker.price_ticks {
                        continue;
                    }
                    if let Some(maker) = queue.front() {
                        return Some((price, maker.clone()));
                    }
                }
                None
            }
        }
    }

    fn apply_fill(
        &mut self,
        taker: &Order,
        maker_price: u64,
        maker_id: &str,
        maker_account: &str,
        qty_lots: u64,
        fills: &mut Vec<Fill>,
    ) {
        let seq = self.next_seq;
        self.next_seq += 1;
        fills.push(Fill {
            seq,
            market_id: self.market.market_id.clone(),
            taker_id: taker.id.clone(),
            maker_id: maker_id.to_string(),
            taker_account: taker.account.clone(),
            maker_account: maker_account.to_string(),
            taker_side: taker.side,
            price_ticks: maker_price,
            qty_lots,
        });

        if let Some(mut maker) = self.remove_resting(maker_id) {
            if maker.qty_lots > qty_lots {
                maker.qty_lots -= qty_lots;
                let side = match taker.side {
                    Side::Buy => Side::Sell,
                    Side::Sell => Side::Buy,
                };
                let book = match side {
                    Side::Buy => &mut self.buys,
                    Side::Sell => &mut self.sells,
                };
                book.entry(maker_price)
                    .or_default()
                    .push_front(maker.clone());
                self.location.insert(maker.id.clone(), (side, maker_price));
            }
        }
    }

    fn process_order(&mut self, order: Order) -> (Ack, Vec<Fill>) {
        if let Some(reason) = validate_order_fields(&self.market, &order, &self.used_ids) {
            return (Self::reject_ack(order.id, reason), Vec::new());
        }

        if order.tif == Tif::PostOnly && self.would_cross_post_only(&order) {
            return (
                Self::reject_ack(order.id, RejectReason::PostOnlyWouldCross),
                Vec::new(),
            );
        }

        self.used_ids.insert(order.id.clone());

        let mut remaining = order.qty_lots;
        let mut filled_lots = 0u64;
        let mut fills = Vec::new();

        while let Some((maker_price, maker)) = self.peek_opposite_maker(&order) {
            if maker.account == order.account {
                break;
            }
            let trade_qty = remaining.min(maker.qty_lots);
            self.apply_fill(
                &order,
                maker_price,
                &maker.id,
                &maker.account,
                trade_qty,
                &mut fills,
            );
            remaining -= trade_qty;
            filled_lots += trade_qty;
            if remaining == 0 {
                break;
            }
        }

        let mut cancelled_lots = 0u64;
        let mut resting_lots = 0u64;

        if remaining > 0 {
            let hit_self = self
                .peek_opposite_maker(&Order {
                    qty_lots: remaining,
                    ..order.clone()
                })
                .is_some_and(|(_, m)| m.account == order.account);

            if hit_self {
                cancelled_lots = remaining;
                remaining = 0;
            } else {
                match order.tif {
                    Tif::Gtc | Tif::PostOnly => {
                        self.push_resting(order.clone(), remaining);
                        resting_lots = remaining;
                        remaining = 0;
                    }
                    Tif::Ioc => {
                        cancelled_lots = remaining;
                        remaining = 0;
                    }
                }
            }
        }

        let _ = remaining;

        let ack = Ack {
            id: order.id,
            status: Status::Accepted,
            reason: None,
            filled_lots,
            resting_lots,
            cancelled_lots,
        };
        (ack, fills)
    }

    fn process_cancel(&mut self, cancel: Cancel) -> (Ack, Vec<Fill>) {
        if validate_order_id(&cancel.id).is_some() {
            return (
                Self::reject_ack(cancel.id, RejectReason::InvalidId),
                Vec::new(),
            );
        }

        let Some((side, price)) = self.location.get(&cancel.id).copied() else {
            return (
                Self::reject_ack(cancel.id, RejectReason::UnknownOrder),
                Vec::new(),
            );
        };

        let owner = {
            let book = match side {
                Side::Buy => &self.buys,
                Side::Sell => &self.sells,
            };
            let Some(queue) = book.get(&price) else {
                return (
                    Self::reject_ack(cancel.id, RejectReason::UnknownOrder),
                    Vec::new(),
                );
            };
            let Some(order) = queue.iter().find(|o| o.id == cancel.id) else {
                return (
                    Self::reject_ack(cancel.id, RejectReason::UnknownOrder),
                    Vec::new(),
                );
            };
            order.account.clone()
        };

        if owner != cancel.account {
            return (
                Self::reject_ack(cancel.id, RejectReason::NotOwner),
                Vec::new(),
            );
        }

        let Some(resting) = self.remove_resting(&cancel.id) else {
            return (
                Self::reject_ack(cancel.id, RejectReason::UnknownOrder),
                Vec::new(),
            );
        };

        let ack = Ack {
            id: cancel.id,
            status: Status::Accepted,
            reason: None,
            filled_lots: 0,
            resting_lots: 0,
            cancelled_lots: resting.qty_lots,
        };
        (ack, Vec::new())
    }

    /// Exposed for invariant tests.
    pub fn best_bid_ask(&self) -> (Option<u64>, Option<u64>) {
        (self.best_bid(), self.best_ask())
    }
}

impl Matcher for StubMatcher {
    fn process(&mut self, msg: Message) -> (Ack, Vec<Fill>) {
        match msg {
            Message::Order(order) => self.process_order(order),
            Message::Cancel(cancel) => self.process_cancel(cancel),
        }
    }
}
