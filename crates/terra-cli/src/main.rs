#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

use matcher_api::{Ack, Fill, Market, Matcher, Message};
use matcher_stub::StubMatcher;
use serde::Serialize;

#[derive(Serialize)]
struct AckLine<'a> {
    ack: &'a Ack,
}

#[derive(Serialize)]
struct FillLine<'a> {
    fill: &'a Fill,
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  terra match --market <market.json> --orders <orders.jsonl>");
    eprintln!("  terra demo");
}

fn parse_match_args(args: &[String]) -> Result<(String, String), String> {
    if args.len() != 5 {
        return Err("expected: match --market <path> --orders <path>".to_string());
    }
    if args[0] != "match" || args[2] != "--market" || args[4] != "--orders" {
        return Err("expected: match --market <path> --orders <path>".to_string());
    }
    Ok((args[1].clone(), args[3].clone()))
}

fn read_market(path: &str) -> Result<Market, String> {
    let data = fs::read_to_string(path).map_err(|e| format!("read market: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse market: {e}"))
}

fn run_match(market_path: &str, orders_path: &str) -> Result<(), String> {
    let market = read_market(market_path)?;
    let mut matcher = StubMatcher::new(market);
    let orders_data = fs::read_to_string(orders_path).map_err(|e| format!("read orders: {e}"))?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for (line_no, line) in orders_data.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Message = serde_json::from_str(trimmed)
            .map_err(|e| format!("parse order line {}: {e}", line_no + 1))?;
        let (ack, fills) = matcher.process(msg);
        serde_json::to_writer(&mut out, &AckLine { ack: &ack })
            .map_err(|e| format!("write ack: {e}"))?;
        writeln!(out).map_err(|e| format!("write newline: {e}"))?;
        for fill in &fills {
            serde_json::to_writer(&mut out, &FillLine { fill })
                .map_err(|e| format!("write fill: {e}"))?;
            writeln!(out).map_err(|e| format!("write newline: {e}"))?;
        }
    }
    Ok(())
}

fn demo() -> Result<(), String> {
    let examples_dir = Path::new("examples");
    fs::create_dir_all(examples_dir).map_err(|e| format!("create examples: {e}"))?;

    let market_json = r#"{
  "market_id": "NEAR-USDC",
  "base_token": "nep141:wrap.near",
  "quote_token": "nep141:usdc.near",
  "lot_size_base": "1",
  "quote_per_tick_lot": "1"
}"#;
    let orders_jsonl = concat!(
        r#"{"type":"order","id":"s1","account":"alice.near","side":"sell","#,
        r#""price_ticks":100,"qty_lots":5,"tif":"gtc"}"#,
        "\n",
        r#"{"type":"order","id":"b1","account":"bob.near","side":"buy","#,
        r#""price_ticks":100,"qty_lots":5,"tif":"gtc"}"#,
        "\n",
    );

    fs::write(examples_dir.join("market.json"), market_json)
        .map_err(|e| format!("write market.json: {e}"))?;
    fs::write(examples_dir.join("orders.jsonl"), orders_jsonl)
        .map_err(|e| format!("write orders.jsonl: {e}"))?;

    run_match("examples/market.json", "examples/orders.jsonl")
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [] => {
            usage();
            Err("missing command".to_string())
        }
        [cmd] if cmd == "demo" => demo(),
        _ => match parse_match_args(&args) {
            Ok((market, orders)) => run_match(&market, &orders),
            Err(e) => {
                usage();
                Err(e)
            }
        },
    };

    if let Err(e) = result {
        eprintln!("{e}");
        process::exit(2);
    }
}
