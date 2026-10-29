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
