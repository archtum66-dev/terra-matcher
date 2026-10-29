//! Deterministic synthetic order flow for `terra gen` and `terra bench`.
//!
//! The same seed always yields the same messages, so any matcher can be fed the identical flow
//! and its output compared line by line.

use matcher_api::{Cancel, Message, Order, Side, Tif};

/// Number of distinct accounts in the flow. Few enough that self-trades do happen.
pub const ACCOUNTS: u64 = 64;
const MID_TICKS: u64 = 1_000;
const BAND_TICKS: u64 = 30;
/// Cancels target one of the most recent orders, so many hit live orders and many do not.
const CANCEL_WINDOW: u64 = 5_000;

/// xorshift64* generator (same family as the tests use).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform in `0..n` (`n > 0`); the tiny modulo bias is irrelevant here.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

fn owner_of(order_no: u64) -> String {
    let idx = (order_no.wrapping_mul(0x9e37_79b9) >> 3) % ACCOUNTS;
    format!("acct{idx:02}.test")
}

fn order_id(order_no: u64) -> String {
    format!("o{order_no}")
}

/// `messages` messages: about 88 % orders (78 % GTC, 14 % IOC, 8 % post-only), 12 % cancels.
pub fn generate(messages: u64, seed: u64) -> Vec<Message> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(usize::try_from(messages).unwrap_or(0));
    let mut next_order: u64 = 0;
    for _ in 0..messages {
        if next_order > 0 && rng.below(100) < 12 {
            let lowest = next_order.saturating_sub(CANCEL_WINDOW);
            let target = lowest + rng.below(next_order - lowest);
            let account = if rng.below(100) < 85 {
                owner_of(target)
            } else {
                owner_of(rng.below(1_000_000))
            };
            out.push(Message::Cancel(Cancel {
                id: order_id(target),
                account,
            }));
            continue;
        }
        let order_no = next_order;
        next_order += 1;
        let side = if rng.below(2) == 0 {
            Side::Buy
        } else {
            Side::Sell
        };
        let tif = match rng.below(100) {
            0..=77 => Tif::Gtc,
            78..=91 => Tif::Ioc,
            _ => Tif::PostOnly,
        };
        out.push(Message::Order(Order {
            id: order_id(order_no),
            account: owner_of(order_no),
            side,
            price_ticks: MID_TICKS - BAND_TICKS + rng.below(2 * BAND_TICKS + 1),
            qty_lots: 1 + rng.below(50),
            tif,
        }));
    }
    out
}
