use std::fs;
use std::path::{Path, PathBuf};

use matcher_api::{run_vector, Market, Matcher, Message, Vector};
use matcher_stub::StubMatcher;

fn vectors_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("vectors")
}

fn load_vectors() -> Vec<Vector> {
    let dir = vectors_dir();
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read vectors dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let data =
                fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()));
            serde_json::from_str(&data).unwrap_or_else(|e| panic!("parse {}: {e}", p.display()))
        })
        .collect()
}

#[test]
fn conformance_vectors() {
    let vectors = load_vectors();
    assert!(
        vectors.len() >= 16,
        "expected at least 16 vectors, found {}",
        vectors.len()
    );
    for vector in vectors {
        run_vector(StubMatcher::new, &vector).unwrap_or_else(|e| panic!("{e}"));
    }
}

fn collect_output(market: Market, input: &[Message]) -> Vec<serde_json::Value> {
    let mut matcher = StubMatcher::new(market);
    let mut lines = Vec::new();
    for msg in input {
        let (ack, fills) = matcher.process(msg.clone());
        lines.push(serde_json::json!({ "ack": ack }));
        for fill in fills {
            lines.push(serde_json::json!({ "fill": fill }));
        }
    }
    lines
}

#[test]
fn determinism_same_input_same_output() {
    let vectors = load_vectors();
    assert!(!vectors.is_empty(), "no vectors found");
    for vector in &vectors {
        let a = collect_output(vector.market.clone(), &vector.input);
        let b = collect_output(vector.market.clone(), &vector.input);
        assert_eq!(a, b, "determinism failed for {}", vector.name);
    }
}

struct XorShift64Star {
    state: u64,
}

impl XorShift64Star {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn next_range(&mut self, upper: u64) -> u64 {
        if upper == 0 {
            return 0;
        }
        self.next_u64() % upper
    }
}

fn check_invariants(
    matcher: &StubMatcher,
    fill_qty_by_order: &std::collections::BTreeMap<String, u64>,
    order_qty: &std::collections::BTreeMap<String, u64>,
) {
    let (bid, ask) = matcher.best_bid_ask();
    if let (Some(b), Some(a)) = (bid, ask) {
        assert!(b < a, "crossed book: bid {b} ask {a}");
    }

    for (id, filled) in fill_qty_by_order {
        if let Some(&qty) = order_qty.get(id) {
            assert!(filled <= &qty, "overfill on {id}");
        }
    }
}

#[test]
fn random_messages_invariants() {
    use matcher_api::{Order, Side, Tif};

    let market = Market {
        market_id: "NEAR-USDC".to_string(),
        base_token: "nep141:wrap.near".to_string(),
        quote_token: "nep141:usdc.near".to_string(),
        lot_size_base: 1,
        quote_per_tick_lot: 1,
    };

    let mut rng = XorShift64Star::new(0xDEAD_BEEF_CAFE_BABE);
    let accounts = ["alice.near", "bob.near", "carl.near", "dave.near"];
    let mut matcher = StubMatcher::new(market.clone());
    let mut fill_qty_by_order = std::collections::BTreeMap::new();
    let mut order_qty = std::collections::BTreeMap::new();
    let mut next_seq_expected = 1u64;
    let mut id_counter = 0u64;

    for _ in 0..10_000 {
        id_counter += 1;
        let roll = rng.next_range(100);
        let msg = if roll < 85 {
            let side = if rng.next_u64() & 1 == 0 {
                Side::Buy
            } else {
                Side::Sell
            };
            let price = 90 + rng.next_range(21);
            let qty = 1 + rng.next_range(20);
            let tif_roll = rng.next_range(10);
            let tif = match tif_roll {
                0..=6 => Tif::Gtc,
                7..=8 => Tif::Ioc,
                _ => Tif::PostOnly,
            };
            let account = accounts[rng.next_range(accounts.len() as u64) as usize].to_string();
            let id = format!("o{id_counter}");
            order_qty.insert(id.clone(), qty);
            Message::Order(Order {
                id,
                account,
                side,
                price_ticks: price,
                qty_lots: qty,
                tif,
            })
        } else if roll < 95 && id_counter > 10 {
            let target = 1 + rng.next_range(id_counter.min(50));
            let account = accounts[rng.next_range(accounts.len() as u64) as usize].to_string();
            Message::Cancel(matcher_api::Cancel {
                id: format!("o{target}"),
                account,
            })
        } else {
            continue;
        };

        let (ack, fills) = matcher.process(msg);
        let _ = ack;
        for fill in &fills {
            assert!(fill.qty_lots > 0);
            assert_eq!(fill.seq, next_seq_expected);
            next_seq_expected += 1;
            *fill_qty_by_order.entry(fill.taker_id.clone()).or_insert(0) += fill.qty_lots;
            *fill_qty_by_order.entry(fill.maker_id.clone()).or_insert(0) += fill.qty_lots;
        }

        check_invariants(&matcher, &fill_qty_by_order, &order_qty);
    }
}

#[test]
fn book_view_lists_levels_best_first_in_time_priority() {
    let market: Market = serde_json::from_str(
        r#"{"market_id":"NEAR-USDC","base_token":"nep141:wrap.near","quote_token":"nep141:usdc.near","lot_size_base":"1","quote_per_tick_lot":"1"}"#,
    )
    .expect("market");
    let mut matcher = StubMatcher::new(market);
    let orders = [
        r#"{"type":"order","id":"s1","account":"alice.test","side":"sell","price_ticks":102,"qty_lots":4,"tif":"gtc"}"#,
        r#"{"type":"order","id":"s2","account":"carol.test","side":"sell","price_ticks":101,"qty_lots":3,"tif":"gtc"}"#,
        r#"{"type":"order","id":"s3","account":"alice.test","side":"sell","price_ticks":101,"qty_lots":2,"tif":"gtc"}"#,
        r#"{"type":"order","id":"b1","account":"bob.test","side":"buy","price_ticks":99,"qty_lots":5,"tif":"gtc"}"#,
        r#"{"type":"order","id":"b2","account":"bob.test","side":"buy","price_ticks":98,"qty_lots":1,"tif":"gtc"}"#,
    ];
    for line in orders {
        let msg: Message = serde_json::from_str(line).expect("message");
        matcher.process(msg);
    }
    let view = matcher.book_view();
    let ask_prices: Vec<u64> = view.asks.iter().map(|l| l.price_ticks).collect();
    let bid_prices: Vec<u64> = view.bids.iter().map(|l| l.price_ticks).collect();
    assert_eq!(ask_prices, vec![101, 102]);
    assert_eq!(bid_prices, vec![99, 98]);
    assert_eq!(view.asks[0].total_lots, 5);
    let ids: Vec<&str> = view.asks[0].orders.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, vec!["s2", "s3"], "oldest first within a level");

    // A cancel removes the order and an emptied level disappears.
    let cancel: Message =
        serde_json::from_str(r#"{"type":"cancel","id":"s1","account":"alice.test"}"#).expect("c");
    matcher.process(cancel);
    let view = matcher.book_view();
    assert_eq!(view.asks.len(), 1);
    let (bid, ask) = matcher.best_bid_ask();
    assert_eq!(Some(view.bids[0].price_ticks), bid);
    assert_eq!(Some(view.asks[0].price_ticks), ask);
}
