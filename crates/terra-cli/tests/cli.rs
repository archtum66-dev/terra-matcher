//! Runs the real `terra` binary on the example files (regression test for the argument parser).
use std::path::PathBuf;
use std::process::Command;

fn example(name: &str) -> String {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "..", "examples", name]
        .iter()
        .collect();
    path.to_string_lossy().into_owned()
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_terra"))
        .args(args)
        .output()
        .expect("run terra");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn match_prints_two_acks_and_one_fill() {
    let market = example("market.json");
    let orders = example("orders.jsonl");
    let (ok, stdout, stderr) = run(&["match", "--market", &market, "--orders", &orders]);
    assert!(ok, "stderr: {stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout}");
    assert!(lines[0].starts_with("{\"ack\":"), "{stdout}");
    assert!(lines[2].starts_with("{\"fill\":"), "{stdout}");
    assert!(lines[2].contains("\"price_ticks\":100"), "{stdout}");
}

#[test]
fn settle_prints_one_settlement_pair() {
    let market = example("market.json");
    let orders = example("orders.jsonl");
    let (ok, stdout, stderr) = run(&["settle", "--market", &market, "--orders", &orders]);
    assert!(ok, "stderr: {stderr}");
    let settlements: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("{\"settlement\":"))
        .collect();
    assert_eq!(settlements.len(), 1, "{stdout}");
    assert!(settlements[0].contains("token_diff"), "{stdout}");
}

#[test]
fn match_with_wrong_arguments_fails() {
    let market = example("market.json");
    let (ok, _, stderr) = run(&["match", "--orders", &market, "--market", &market]);
    assert!(!ok);
    assert!(stderr.contains("expected: match"), "{stderr}");
}

#[test]
fn gen_is_deterministic_and_has_the_requested_length() {
    let (ok1, a, e1) = run(&["gen", "--messages", "500", "--seed", "7"]);
    let (ok2, b, _) = run(&["gen", "--messages", "500", "--seed", "7"]);
    let (_, c, _) = run(&["gen", "--messages", "500", "--seed", "8"]);
    assert!(ok1 && ok2, "stderr: {e1}");
    assert_eq!(a, b, "same seed, same flow");
    assert_ne!(a, c, "different seed, different flow");
    assert_eq!(a.lines().count(), 500);
    assert!(a.lines().all(|l| l.starts_with("{\"type\":\"")));
}

#[test]
fn generated_flow_replays_through_match() {
    let (ok, flow, _) = run(&["gen", "--messages", "3000", "--seed", "3"]);
    assert!(ok);
    let path = std::env::temp_dir().join(format!("terra-gen-{}.jsonl", std::process::id()));
    std::fs::write(&path, flow).expect("write flow");
    let market = example("market.json");
    let (ok, out, stderr) = run(&[
        "match",
        "--market",
        &market,
        "--orders",
        &path.to_string_lossy(),
    ]);
    let _ = std::fs::remove_file(&path);
    assert!(ok, "stderr: {stderr}");
    let acks = out.lines().filter(|l| l.starts_with("{\"ack\":")).count();
    let fills = out.lines().filter(|l| l.starts_with("{\"fill\":")).count();
    assert_eq!(acks, 3000, "one ack per message");
    assert!(fills > 100, "the flow must produce trades, got {fills}");
}

#[test]
fn bench_prints_one_json_line_with_rates() {
    let (ok, out, stderr) = run(&["bench", "--messages", "20000", "--repeat", "1"]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(out.lines().count(), 1, "{out}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("json line");
    assert_eq!(v["messages"], 20000);
    assert_eq!(
        v["accepted"].as_u64().unwrap() + v["rejected"].as_u64().unwrap(),
        20000
    );
    assert!(v["fills"].as_u64().unwrap() > 1000, "{v}");
    assert!(v["messages_per_second"].as_f64().unwrap() > 0.0);
    assert!(v["settlement_pairs_per_second"].as_f64().unwrap() > 0.0);
    assert!(v["settlement_pair_json_bytes_avg"].as_f64().unwrap() > 100.0);
}

#[test]
fn gen_and_bench_reject_bad_arguments() {
    for args in [
        vec!["gen"],
        vec!["gen", "--messages", "0"],
        vec!["gen", "--messages", "abc"],
        vec!["bench", "--messages", "10", "--repeat", "0"],
        vec!["bench", "--messages", "10", "--nope", "1"],
    ] {
        let (ok, _, stderr) = run(&args);
        assert!(!ok, "{args:?} must fail");
        assert!(!stderr.is_empty(), "{args:?} needs a message");
    }
}

#[test]
fn demo_writes_examples_and_prints_acks_fill_and_settlement() {
    let dir = std::env::temp_dir().join(format!("terra-demo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = Command::new(env!("CARGO_BIN_EXE_terra"))
        .arg("demo")
        .current_dir(&dir)
        .output()
        .expect("run terra demo");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let market_written = dir.join("examples").join("market.json").exists();
    let orders_written = dir.join("examples").join("orders.jsonl").exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(market_written && orders_written);
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.starts_with("{\"ack\":"))
            .count(),
        2
    );
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.starts_with("{\"fill\":"))
            .count(),
        1
    );
    assert_eq!(
        stdout
            .lines()
            .filter(|l| l.starts_with("{\"settlement\":"))
            .count(),
        1
    );
}
