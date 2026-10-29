#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;

use matcher_api::{quote_amount, Ack, Fill, Market, Matcher, Message};
use matcher_stub::StubMatcher;
use serde::Serialize;
use settlement::{reference_check, settle_fill, QuoteResponse, SettlementPair};
#[cfg(feature = "http")]
use settlement::{QuoteClient, QuoteRequest};

/// Fixed example deadline (no wall clock).
pub const DEFAULT_SETTLEMENT_DEADLINE: &str = "2030-01-01T00:00:00Z";

#[derive(Serialize)]
struct AckLine<'a> {
    ack: &'a Ack,
}

#[derive(Serialize)]
struct FillLine<'a> {
    fill: &'a Fill,
}

#[derive(Serialize)]
struct SettlementLine<'a> {
    settlement: SettlementOutput<'a>,
}

#[derive(Serialize)]
struct SettlementOutput<'a> {
    #[serde(flatten)]
    pair: &'a SettlementPair,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_check: Option<settlement::ReferenceCheck>,
}

fn usage() {
    eprintln!("Usage:");
    eprintln!("  terra match --market <market.json> --orders <orders.jsonl>");
    eprintln!(
        "  terra settle --market <market.json> --orders <orders.jsonl> [--deadline <ISO>] [--reference <quote.json>] [--tolerance-bps <n>]"
    );
    eprintln!("  terra demo");
    #[cfg(feature = "http")]
    eprintln!("  terra quote --dry --file <request.json>");
}

fn read_market(path: &str) -> Result<Market, String> {
    let data = fs::read_to_string(path).map_err(|e| format!("read market: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse market: {e}"))
}

fn run_matcher_lines(
    market_path: &str,
    orders_path: &str,
    mut on_fill: impl FnMut(&Market, &Fill) -> Result<(), String>,
) -> Result<(), String> {
    let market = read_market(market_path)?;
    let mut matcher = StubMatcher::new(market.clone());
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
            on_fill(&market, fill)?;
        }
    }
    Ok(())
}

fn run_match(market_path: &str, orders_path: &str) -> Result<(), String> {
    run_matcher_lines(market_path, orders_path, |_market, _fill| Ok(()))
}

struct SettleOptions {
    deadline: String,
    reference_path: Option<String>,
    tolerance_bps: u32,
}

fn run_settle(market_path: &str, orders_path: &str, opts: SettleOptions) -> Result<(), String> {
    let reference: Option<QuoteResponse> = if let Some(ref path) = opts.reference_path {
        let data = fs::read_to_string(path).map_err(|e| format!("read reference: {e}"))?;
        Some(serde_json::from_str(&data).map_err(|e| format!("parse reference: {e}"))?)
    } else {
        None
    };

    run_matcher_lines(market_path, orders_path, |market, fill| {
        let pair = settle_fill(market, fill, &opts.deadline)
            .map_err(|e| format!("settle fill {}: {e}", fill.seq))?;

        let ref_check = if let (Some(q), Some(quote_amt)) = (
            reference.as_ref(),
            quote_amount(market, fill.qty_lots, fill.price_ticks),
        ) {
            Some(
                reference_check(quote_amt, q, opts.tolerance_bps)
                    .map_err(|e| format!("reference check: {e}"))?,
            )
        } else {
            None
        };

        let stdout = io::stdout();
        let mut out = stdout.lock();
        serde_json::to_writer(
            &mut out,
            &SettlementLine {
                settlement: SettlementOutput {
                    pair: &pair,
                    reference_check: ref_check,
                },
            },
        )
        .map_err(|e| format!("write settlement: {e}"))?;
        writeln!(out).map_err(|e| format!("write newline: {e}"))?;
        Ok(())
    })
}

fn parse_settle_args(args: &[String]) -> Result<(String, String, SettleOptions), String> {
    if args.is_empty() || args[0] != "settle" {
        return Err("expected settle subcommand".to_string());
    }
    let mut market = None;
    let mut orders = None;
    let mut deadline = DEFAULT_SETTLEMENT_DEADLINE.to_string();
    let mut reference_path = None;
    let mut tolerance_bps = 100u32;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--market" => {
                i += 1;
                market = Some(next_arg(args, i, "--market")?);
                i += 1;
            }
            "--orders" => {
                i += 1;
                orders = Some(next_arg(args, i, "--orders")?);
                i += 1;
            }
            "--deadline" => {
                i += 1;
                deadline = next_arg(args, i, "--deadline")?;
                i += 1;
            }
            "--reference" => {
                i += 1;
                reference_path = Some(next_arg(args, i, "--reference")?);
                i += 1;
            }
            "--tolerance-bps" => {
                i += 1;
                let s = next_arg(args, i, "--tolerance-bps")?;
                tolerance_bps = s
                    .parse()
                    .map_err(|_| format!("invalid --tolerance-bps: {s}"))?;
                i += 1;
            }
            other => return Err(format!("unknown settle argument: {other}")),
        }
    }
    Ok((
        market.ok_or("missing --market")?,
        orders.ok_or("missing --orders")?,
        SettleOptions {
            deadline,
            reference_path,
            tolerance_bps,
        },
    ))
}

fn next_arg(args: &[String], i: usize, flag: &str) -> Result<String, String> {
    args.get(i)
        .cloned()
        .ok_or_else(|| format!("missing value after {flag}"))
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

#[cfg(feature = "http")]
fn run_quote(file_path: &str) -> Result<(), String> {
    let data = fs::read_to_string(file_path).map_err(|e| format!("read request: {e}"))?;
    let req: QuoteRequest =
        serde_json::from_str(&data).map_err(|e| format!("parse request: {e}"))?;
    let api_key = env::var("ONECLICK_API_KEY")
        .map_err(|_| "ONECLICK_API_KEY environment variable not set".to_string())?;
    let client = QuoteClient::new(settlement::HttpTransport).with_api_key(api_key);
    let resp = client
        .quote(&req)
        .map_err(|e| format!("quote failed: {e}"))?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer_pretty(&mut out, &resp).map_err(|e| format!("write response: {e}"))?;
    writeln!(out).map_err(|e| format!("write newline: {e}"))?;
    Ok(())
}

#[cfg(feature = "http")]
fn parse_quote_args(args: &[String]) -> Result<String, String> {
    if args.len() != 4 || args[0] != "quote" || args[1] != "--dry" || args[2] != "--file" {
        return Err("expected: quote --dry --file <request.json>".to_string());
    }
    Ok(args[3].clone())
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
        r#"{"type":"order","id":"s1","account":"alice.test","side":"sell","#,
        r#""price_ticks":100,"qty_lots":5,"tif":"gtc"}"#,
        "\n",
        r#"{"type":"order","id":"b1","account":"bob.test","side":"buy","#,
        r#""price_ticks":100,"qty_lots":5,"tif":"gtc"}"#,
        "\n",
    );

    fs::write(examples_dir.join("market.json"), market_json)
        .map_err(|e| format!("write market.json: {e}"))?;
    fs::write(examples_dir.join("orders.jsonl"), orders_jsonl)
        .map_err(|e| format!("write orders.jsonl: {e}"))?;

    run_match("examples/market.json", "examples/orders.jsonl")?;
    run_settle(
        "examples/market.json",
        "examples/orders.jsonl",
        SettleOptions {
            deadline: DEFAULT_SETTLEMENT_DEADLINE.to_string(),
            reference_path: None,
            tolerance_bps: 100,
        },
    )
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.as_slice() {
        [] => {
            usage();
            Err("missing command".to_string())
        }
        [cmd] if cmd == "demo" => demo(),
        [cmd, ..] if cmd == "settle" => match parse_settle_args(&args) {
            Ok((market, orders, opts)) => run_settle(&market, &orders, opts),
            Err(e) => {
                usage();
                Err(e)
            }
        },
        #[cfg(feature = "http")]
        [cmd, ..] if cmd == "quote" => match parse_quote_args(&args) {
            Ok(path) => run_quote(&path),
            Err(e) => {
                usage();
                Err(e)
            }
        },
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
