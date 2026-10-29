use std::fs;
use std::path::PathBuf;

use matcher_api::{Matcher, Message};
use matcher_stub::StubMatcher;
use terra_web::{conformance_report, Session, VECTORS};

fn repo_file(parts: &[&str]) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("..");
    for part in parts {
        p.push(part);
    }
    p
}

fn market_json() -> String {
    fs::read_to_string(repo_file(&["examples", "market.json"])).expect("market.json")
}

fn orders() -> Vec<String> {
    fs::read_to_string(repo_file(&["examples", "orders.jsonl"]))
        .expect("orders.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn embedded_vectors_are_exactly_the_files_in_tests_vectors() {
    let mut on_disk: Vec<String> = fs::read_dir(repo_file(&["tests", "vectors"]))
        .expect("vectors dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".json"))
        .map(|n| n.trim_end_matches(".json").to_string())
        .collect();
    on_disk.sort();
    let embedded: Vec<String> = VECTORS.iter().map(|(n, _)| (*n).to_string()).collect();
    assert_eq!(
        embedded, on_disk,
        "update VECTORS in crates/web-demo/src/session.rs"
    );
}

#[test]
fn conformance_report_passes_every_vector() {
    let report: Vec<serde_json::Value> =
        serde_json::from_str(&conformance_report()).expect("report json");
    assert_eq!(report.len(), VECTORS.len());
    for entry in &report {
        assert_eq!(entry["ok"], true, "failed: {entry}");
    }
}

#[test]
fn session_matches_the_reference_matcher_message_by_message() {
    let market: matcher_api::Market = serde_json::from_str(&market_json()).expect("market");
    let mut direct = StubMatcher::new(market);
    let mut session = Session::new(&market_json()).expect("session");
    for line in orders() {
        let out: serde_json::Value =
            serde_json::from_str(&session.step(&line).expect("step")).expect("json");
        let msg: Message = serde_json::from_str(&line).expect("msg");
        let (ack, fills) = direct.process(msg);
        assert_eq!(out["ack"], serde_json::to_value(&ack).expect("ack"));
        assert_eq!(out["fills"], serde_json::to_value(&fills).expect("fills"));
        assert_eq!(
            out["settlements"].as_array().map(Vec::len),
            Some(fills.len()),
            "one settlement draft per fill"
        );
    }
}

#[test]
fn settlement_drafts_balance_to_zero_for_every_token() {
    let mut session = Session::new(&market_json()).expect("session");
    let mut drafts = 0;
    for line in orders() {
        let out: serde_json::Value =
            serde_json::from_str(&session.step(&line).expect("step")).expect("json");
        for s in out["settlements"].as_array().expect("array") {
            drafts += 1;
            let mut sums = std::collections::BTreeMap::<String, i128>::new();
            for who in ["buyer_draft", "seller_draft"] {
                let diff = &s[who]["intents"][0]["diff"];
                for (token, amount) in diff.as_object().expect("diff") {
                    let n: i128 = amount.as_str().expect("string").parse().expect("number");
                    *sums.entry(token.clone()).or_default() += n;
                }
            }
            assert!(sums.values().all(|v| *v == 0), "unbalanced: {sums:?}");
        }
    }
    assert!(drafts >= 1, "example flow must produce at least one fill");
}

#[test]
fn export_replays_to_identical_output() {
    let mut a = Session::new(&market_json()).expect("session");
    let first: Vec<String> = orders().iter().map(|l| a.step(l).expect("step")).collect();
    let exported = a.export_jsonl().expect("export");
    let mut b = Session::new(&market_json()).expect("session");
    let second: Vec<String> = exported.lines().map(|l| b.step(l).expect("step")).collect();
    assert_eq!(first, second);
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(Session::new("{").is_err());
    let mut s = Session::new(&market_json()).expect("session");
    assert!(s.step("not json").is_err());
    assert!(s.step("{\"type\":\"order\"}").is_err());
}
