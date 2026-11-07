#![forbid(unsafe_code)]

mod flow;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process;
use std::time::Instant;

use matcher_api::{quote_amount, Ack, Fill, Market, Matcher, Message};
use matcher_stub::StubMatcher;
use serde::Serialize;
use settlement::{reference_check, settle_fill, QuoteResponse, SettlementPair};
#[cfg(feature = "http")]
use settlement::{reference_quote_request, QuoteClient, QuoteRequest, Transport, QUOTE_URL};

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
    eprintln!("  terra gen --messages <n> [--seed <n>]");
    eprintln!("  terra bench --messages <n> [--seed <n>] [--repeat <n>]");
    #[cfg(feature = "http")]
    eprintln!("  terra quote --dry --file <request.json>");
    #[cfg(feature = "http")]
    eprintln!(
        "  terra live-check --market <market.json> --orders <orders.jsonl> [--tolerance-bps <n>] [--slippage-bps <n>] [--record <dir>]"
    );
    #[cfg(feature = "http")]
    eprintln!(
        "  terra intents-check --market <market.json> --orders <orders.jsonl> [--record <dir>]"
    );
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
    if args.len() != 5 || args[0] != "match" || args[1] != "--market" || args[3] != "--orders" {
        return Err("expected: match --market <path> --orders <path>".to_string());
    }
    Ok((args[2].clone(), args[4].clone()))
}

#[cfg(feature = "http")]
fn run_quote(file_path: &str) -> Result<(), String> {
    let data = fs::read_to_string(file_path).map_err(|e| format!("read request: {e}"))?;
    let req: QuoteRequest =
        serde_json::from_str(&data).map_err(|e| format!("parse request: {e}"))?;
    let client = with_optional_key(QuoteClient::new(settlement::HttpTransport));
    let resp = client
        .quote(&req)
        .map_err(|e| format!("quote failed: {e}"))?;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer_pretty(&mut out, &resp).map_err(|e| format!("write response: {e}"))?;
    writeln!(out).map_err(|e| format!("write newline: {e}"))?;
    Ok(())
}

/// Uses `ONECLICK_API_KEY` when it is set. Without a key the public 1Click API still
/// answers dry quotes (checked live on 2026-11-07); it then adds its default app fee.
#[cfg(feature = "http")]
fn with_optional_key<T: Transport>(client: QuoteClient<T>) -> QuoteClient<T> {
    match env::var("ONECLICK_API_KEY") {
        Ok(key) if !key.trim().is_empty() => client.with_api_key(key),
        _ => client,
    }
}

/// Live transport that also keeps the last request and response body, for `--record`.
#[cfg(feature = "http")]
struct TeeTransport {
    last: std::sync::Arc<std::sync::Mutex<Option<(String, String)>>>,
}

#[cfg(feature = "http")]
impl Transport for TeeTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<settlement::TransportResponse, settlement::TransportError> {
        let resp = settlement::HttpTransport.post_json(url, headers, body)?;
        if let Ok(mut guard) = self.last.lock() {
            *guard = Some((body.to_string(), resp.body.clone()));
        }
        Ok(resp)
    }
}

/// ISO 8601 UTC time `secs` seconds from now (quote deadlines need a real clock).
#[cfg(feature = "http")]
fn iso_utc_after(secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + secs;
    let days = (now / 86_400) as i64;
    let rem = now % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe as i64 + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

#[cfg(feature = "http")]
struct LiveCheckOptions {
    tolerance_bps: u32,
    slippage_bps: u32,
    record_dir: Option<String>,
}

/// Match the orders, then ask the live NEAR Intents 1Click API (dry quote, no deposit,
/// no funds) what the seller of each fill would get for the same base amount, and
/// print how far the matched price is from that market price.
#[cfg(feature = "http")]
fn run_live_check(
    market_path: &str,
    orders_path: &str,
    opts: LiveCheckOptions,
) -> Result<(), String> {
    use matcher_api::{base_amount, Side};
    let last = std::sync::Arc::new(std::sync::Mutex::new(None));
    let client = with_optional_key(QuoteClient::new(TeeTransport {
        last: std::sync::Arc::clone(&last),
    }));
    if let Some(ref dir) = opts.record_dir {
        fs::create_dir_all(dir).map_err(|e| format!("create {dir}: {e}"))?;
    }
    let deadline = iso_utc_after(600);
    run_matcher_lines(market_path, orders_path, |market, fill| {
        let seller = match fill.taker_side {
            Side::Buy => &fill.maker_account,
            Side::Sell => &fill.taker_account,
        };
        let req = reference_quote_request(market, fill, seller, &deadline, opts.slippage_bps)
            .map_err(|e| format!("quote request for fill {}: {e}", fill.seq))?;
        let resp = client
            .quote(&req)
            .map_err(|e| format!("live quote for fill {}: {e}", fill.seq))?;
        if let Some(ref dir) = opts.record_dir {
            if let Ok(guard) = last.lock() {
                if let Some((ref request_body, ref response_body)) = *guard {
                    let base = Path::new(dir);
                    fs::write(
                        base.join(format!("fill-{}.request.json", fill.seq)),
                        request_body,
                    )
                    .map_err(|e| format!("record request: {e}"))?;
                    fs::write(
                        base.join(format!("fill-{}.response.json", fill.seq)),
                        response_body,
                    )
                    .map_err(|e| format!("record response: {e}"))?;
                }
            }
        }
        let fill_quote = quote_amount(market, fill.qty_lots, fill.price_ticks)
            .ok_or_else(|| format!("quote amount overflow in fill {}", fill.seq))?;
        let sold = base_amount(market, fill.qty_lots)
            .ok_or_else(|| format!("base amount overflow in fill {}", fill.seq))?;
        let check = reference_check(fill_quote, &resp, opts.tolerance_bps)
            .map_err(|e| format!("reference check for fill {}: {e}", fill.seq))?;
        let line = serde_json::json!({
            "live_check": {
                "fill_seq": fill.seq,
                "seller": seller,
                "sold_base_amount": sold.to_string(),
                "fill_quote_amount": fill_quote.to_string(),
                "intents_amount_out": resp.quote.as_ref().and_then(|q| q.amount_out.clone()),
                "deviation_bps": check.deviation_bps,
                "within_tolerance": check.within_tolerance,
                "tolerance_bps": opts.tolerance_bps,
                "source": QUOTE_URL,
                "dry": true,
                "correlation_id": resp.correlation_id,
                "quote_timestamp": resp.timestamp,
            }
        });
        println!("{line}");
        Ok(())
    })
}

/// Public NEAR mainnet RPC; override with `TERRA_NEAR_RPC`.
#[cfg(feature = "http")]
const NEAR_RPC: &str = "https://rpc.mainnet.near.org";

/// Read-only `call_function` on `intents.near`. `Ok(Ok(v))`: the method returned `v`.
/// `Ok(Err(msg))`: the contract panicked with `msg`. `Err`: network or RPC failure.
#[cfg(feature = "http")]
fn verifier_view(
    method: &str,
    args: &serde_json::Value,
) -> Result<Result<serde_json::Value, String>, String> {
    let url = env::var("TERRA_NEAR_RPC").unwrap_or_else(|_| NEAR_RPC.to_string());
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "terra",
        "method": "query",
        "params": {
            "request_type": "call_function",
            "finality": "final",
            "account_id": intents_sign::VERIFIER,
            "method_name": method,
            "args_base64": intents_sign::base64(args.to_string().as_bytes()),
        }
    });
    let resp = settlement::HttpTransport
        .post_json(&url, &[], &body.to_string())
        .map_err(|e| format!("{method}: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&resp.body).map_err(|e| format!("{method}: rpc answer: {e}"))?;
    if let Some(err) = v.get("error") {
        let text = err.to_string().replace("\\\"", "\"");
        return match panic_message(&text) {
            Some(msg) => Ok(Err(msg)),
            None => Err(format!("{method}: rpc error: {text}")),
        };
    }
    let result = v
        .get("result")
        .ok_or_else(|| format!("{method}: rpc answer without result"))?;
    if let Some(err) = result.get("error").and_then(|e| e.as_str()) {
        return Ok(Err(panic_message(err).unwrap_or_else(|| err.to_string())));
    }
    let bytes: Vec<u8> = result
        .get("result")
        .and_then(|r| r.as_array())
        .ok_or_else(|| format!("{method}: rpc answer without bytes"))?
        .iter()
        .map(|b| b.as_u64().unwrap_or(0) as u8)
        .collect();
    serde_json::from_slice(&bytes)
        .map(Ok)
        .map_err(|e| format!("{method}: result: {e}"))
}

#[cfg(feature = "http")]
fn panic_message(text: &str) -> Option<String> {
    let start = text.find("panic_msg: \"")? + "panic_msg: \"".len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_string())
}

/// Turns every fill into its `token_diff` pair, adjusted for the verifier's live fee,
/// signs both halves with NEP-413 using two throwaway keys (implicit accounts, no
/// funds), and asks `intents.near` to run them with `simulate_intents` (read-only, no
/// transaction, nothing is spent). A second call with one corrupted signature is the
/// control: it must be rejected for the signature.
#[cfg(feature = "http")]
fn run_intents_check(
    market_path: &str,
    orders_path: &str,
    record_dir: Option<String>,
) -> Result<(), String> {
    let salt_hex = verifier_view("current_salt", &serde_json::json!({}))?
        .map_err(|e| format!("current_salt: {e}"))?;
    let salt_hex = salt_hex.as_str().unwrap_or_default().to_string();
    let salt =
        intents_sign::parse_salt(&salt_hex).ok_or_else(|| format!("unexpected salt {salt_hex}"))?;
    let fee_pips = verifier_view("fee", &serde_json::json!({}))?
        .map_err(|e| format!("fee: {e}"))?
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
        .ok_or("unexpected fee")?;
    if let Some(ref dir) = record_dir {
        fs::create_dir_all(dir).map_err(|e| format!("create {dir}: {e}"))?;
    }
    let deadline_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        + 600;
    let deadline = iso_utc_after(600);
    let mut all_accepted = true;
    run_matcher_lines(market_path, orders_path, |market, fill| {
        let pair = settle_fill(market, fill, &deadline)
            .map_err(|e| format!("settle fill {}: {e}", fill.seq))?;
        let mut pair = intents_sign::apply_protocol_fee(&pair, fee_pips)
            .map_err(|e| format!("fee for fill {}: {e}", fill.seq))?;
        let buyer_key = intents_sign::random_key();
        let seller_key = intents_sign::random_key();
        pair.buyer_draft.signer_id = intents_sign::implicit_account(&buyer_key);
        pair.seller_draft.signer_id = intents_sign::implicit_account(&seller_key);
        let nonce_deadline_ns = deadline_secs.saturating_mul(1_000_000_000);
        let signed = vec![
            intents_sign::sign_nep413(
                &buyer_key,
                &pair.buyer_draft,
                intents_sign::nonce_v1(salt, nonce_deadline_ns, intents_sign::random_nonce_tail()),
            ),
            intents_sign::sign_nep413(
                &seller_key,
                &pair.seller_draft,
                intents_sign::nonce_v1(salt, nonce_deadline_ns, intents_sign::random_nonce_tail()),
            ),
        ];
        let simulated =
            verifier_view("simulate_intents", &serde_json::json!({ "signed": signed }))?;
        let mut corrupted = signed.clone();
        if let Some(sig) = corrupted[0]["signature"].as_str() {
            let mut chars: Vec<char> = sig.chars().collect();
            let last = chars.len() - 2;
            chars[last] = if chars[last] == '2' { '3' } else { '2' };
            corrupted[0]["signature"] = serde_json::Value::String(chars.into_iter().collect());
        }
        let control = verifier_view(
            "simulate_intents",
            &serde_json::json!({ "signed": corrupted }),
        )?;
        let outcome = |r: &Result<serde_json::Value, String>| match r {
            Ok(_) => "executed".to_string(),
            Err(msg) => msg.clone(),
        };
        let sim_text = outcome(&simulated);
        let control_text = outcome(&control);
        let accepted = (simulated.is_ok() || sim_text.contains("insufficient balance"))
            && control_text.contains("invalid signature");
        all_accepted &= accepted;
        let receives = |draft: &settlement::IntentDraft| {
            draft
                .intents
                .iter()
                .map(|i| serde_json::to_value(i).unwrap_or_default())
                .collect::<Vec<_>>()
        };
        let line = serde_json::json!({
            "intents_check": {
                "fill_seq": fill.seq,
                "verifier": intents_sign::VERIFIER,
                "salt": salt_hex,
                "fee_pips": fee_pips,
                "buyer": pair.buyer_draft.signer_id,
                "seller": pair.seller_draft.signer_id,
                "buyer_intents": receives(&pair.buyer_draft),
                "seller_intents": receives(&pair.seller_draft),
                "simulate_intents": sim_text,
                "control_corrupted_signature": control_text,
                "accepted": accepted,
                "meaning": if accepted {
                    "intents.near accepted the format and both NEP-413 signatures; execution stops at the balances because the throwaway accounts hold no tokens"
                } else {
                    "unexpected verifier answer, see simulate_intents"
                },
            }
        });
        println!("{line}");
        if let Some(ref dir) = record_dir {
            let base = Path::new(dir);
            let signed_text = serde_json::to_string_pretty(&signed).unwrap_or_default();
            fs::write(
                base.join(format!("fill-{}.signed.json", fill.seq)),
                signed_text,
            )
            .map_err(|e| format!("record signed: {e}"))?;
            fs::write(
                base.join(format!("fill-{}.check.json", fill.seq)),
                line.to_string(),
            )
            .map_err(|e| format!("record check: {e}"))?;
        }
        Ok(())
    })?;
    if all_accepted {
        Ok(())
    } else {
        Err("intents.near did not answer as expected".to_string())
    }
}

#[cfg(feature = "http")]
fn parse_intents_check_args(args: &[String]) -> Result<(String, String, Option<String>), String> {
    if args.is_empty() || args[0] != "intents-check" {
        return Err("expected intents-check subcommand".to_string());
    }
    let (mut market, mut orders, mut record) = (None, None, None);
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = next_arg(args, i + 1, flag)?;
        match flag {
            "--market" => market = Some(value),
            "--orders" => orders = Some(value),
            "--record" => record = Some(value),
            other => return Err(format!("unknown intents-check argument: {other}")),
        }
        i += 2;
    }
    Ok((
        market.ok_or("missing --market")?,
        orders.ok_or("missing --orders")?,
        record,
    ))
}

#[cfg(feature = "http")]
fn parse_live_check_args(args: &[String]) -> Result<(String, String, LiveCheckOptions), String> {
    if args.is_empty() || args[0] != "live-check" {
        return Err("expected live-check subcommand".to_string());
    }
    let mut market = None;
    let mut orders = None;
    let mut opts = LiveCheckOptions {
        tolerance_bps: 100,
        slippage_bps: 100,
        record_dir: None,
    };
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = next_arg(args, i + 1, flag)?;
        match flag {
            "--market" => market = Some(value),
            "--orders" => orders = Some(value),
            "--record" => opts.record_dir = Some(value),
            "--tolerance-bps" => {
                opts.tolerance_bps = value
                    .parse()
                    .map_err(|_| format!("invalid --tolerance-bps: {value}"))?
            }
            "--slippage-bps" => {
                opts.slippage_bps = value
                    .parse()
                    .map_err(|_| format!("invalid --slippage-bps: {value}"))?
            }
            other => return Err(format!("unknown live-check argument: {other}")),
        }
        i += 2;
    }
    Ok((
        market.ok_or("missing --market")?,
        orders.ok_or("missing --orders")?,
        opts,
    ))
}

#[cfg(feature = "http")]
fn parse_quote_args(args: &[String]) -> Result<String, String> {
    if args.len() != 4 || args[0] != "quote" || args[1] != "--dry" || args[2] != "--file" {
        return Err("expected: quote --dry --file <request.json>".to_string());
    }
    Ok(args[3].clone())
}

/// `--messages <n> [--seed <n>] [--repeat <n>]` for `gen` and `bench`.
fn parse_flow_args(args: &[String], command: &str) -> Result<(u64, u64, u32), String> {
    if args.is_empty() || args[0] != command {
        return Err(format!("expected {command} subcommand"));
    }
    let mut messages = None;
    let mut seed = 1u64;
    let mut repeat = 3u32;
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let value = next_arg(args, i + 1, flag)?;
        match flag {
            "--messages" => {
                messages = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| format!("invalid --messages: {value}"))?,
                );
            }
            "--seed" => {
                seed = value
                    .parse::<u64>()
                    .map_err(|_| format!("invalid --seed: {value}"))?;
            }
            "--repeat" if command == "bench" => {
                repeat = value
                    .parse::<u32>()
                    .ok()
                    .filter(|r| *r >= 1)
                    .ok_or_else(|| format!("invalid --repeat: {value}"))?;
            }
            other => return Err(format!("unknown {command} argument: {other}")),
        }
        i += 2;
    }
    let messages = messages.ok_or("missing --messages")?;
    if messages == 0 || messages > 50_000_000 {
        return Err("--messages must be between 1 and 50000000".to_string());
    }
    Ok((messages, seed, repeat))
}

fn run_gen(messages: u64, seed: u64) -> Result<(), String> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for msg in flow::generate(messages, seed) {
        serde_json::to_writer(&mut out, &msg).map_err(|e| format!("write message: {e}"))?;
        writeln!(out).map_err(|e| format!("write newline: {e}"))?;
    }
    out.flush().map_err(|e| format!("flush: {e}"))
}

fn default_market() -> Market {
    Market {
        market_id: "NEAR-USDC".to_string(),
        base_token: "nep141:wrap.near".to_string(),
        quote_token: "nep141:usdc.near".to_string(),
        lot_size_base: 1,
        quote_per_tick_lot: 1,
    }
}

/// Times the reference matcher and the settlement builder on a synthetic flow; one JSON line.
fn run_bench(messages: u64, seed: u64, repeat: u32) -> Result<(), String> {
    let market = default_market();
    let flow = flow::generate(messages, seed);

    let mut best_match = f64::MAX;
    let mut fills: Vec<Fill> = Vec::new();
    let mut accepted = 0u64;
    let mut rejected = 0u64;
    let mut resting_end = 0u64;
    for _ in 0..repeat {
        let mut matcher = StubMatcher::new(market.clone());
        let mut run_fills: Vec<Fill> = Vec::new();
        let (mut ok, mut bad) = (0u64, 0u64);
        let start = Instant::now();
        for msg in &flow {
            let (ack, f) = matcher.process(msg.clone());
            if ack.status == matcher_api::Status::Accepted {
                ok += 1;
            } else {
                bad += 1;
            }
            run_fills.extend(f);
        }
        let elapsed = start.elapsed().as_secs_f64();
        best_match = best_match.min(elapsed);
        let view = matcher.book_view();
        resting_end = view
            .bids
            .iter()
            .chain(view.asks.iter())
            .map(|l| l.orders.len() as u64)
            .sum();
        fills = run_fills;
        accepted = ok;
        rejected = bad;
    }

    let mut best_settle = f64::MAX;
    let mut settled = 0u64;
    for _ in 0..repeat {
        let start = Instant::now();
        let mut n = 0u64;
        for fill in &fills {
            settle_fill(&market, fill, DEFAULT_SETTLEMENT_DEADLINE)
                .map_err(|e| format!("settle fill {}: {e}", fill.seq))?;
            n += 1;
        }
        best_settle = best_settle.min(start.elapsed().as_secs_f64());
        settled = n;
    }

    let sample: Vec<&Fill> = fills.iter().take(10_000).collect();
    let mut bytes = 0usize;
    for fill in &sample {
        let pair = settle_fill(&market, fill, DEFAULT_SETTLEMENT_DEADLINE)
            .map_err(|e| format!("settle fill {}: {e}", fill.seq))?;
        bytes += serde_json::to_string(&pair)
            .map_err(|e| format!("serialize: {e}"))?
            .len();
    }
    let avg_bytes = if sample.is_empty() {
        0.0
    } else {
        bytes as f64 / sample.len() as f64
    };
    let matched_lots: u128 = fills.iter().map(|f| u128::from(f.qty_lots)).sum();
    let per_second = |n: u64, secs: f64| if secs > 0.0 { n as f64 / secs } else { 0.0 };

    let line = serde_json::json!({
        "messages": messages,
        "seed": seed,
        "repeat": repeat,
        "accepted": accepted,
        "rejected": rejected,
        "fills": fills.len(),
        "matched_lots": matched_lots.to_string(),
        "resting_orders_at_end": resting_end,
        "match_seconds_best": best_match,
        "messages_per_second": per_second(messages, best_match),
        "settle_seconds_best": best_settle,
        "settlement_pairs_per_second": per_second(settled, best_settle),
        "settlement_pair_json_bytes_avg": avg_bytes,
    });
    println!("{line}");
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

    // `settle` prints the acks and fills as well, so the demo shows each line once.
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
        [cmd, ..] if cmd == "gen" => match parse_flow_args(&args, "gen") {
            Ok((messages, seed, _)) => run_gen(messages, seed),
            Err(e) => {
                usage();
                Err(e)
            }
        },
        [cmd, ..] if cmd == "bench" => match parse_flow_args(&args, "bench") {
            Ok((messages, seed, repeat)) => run_bench(messages, seed, repeat),
            Err(e) => {
                usage();
                Err(e)
            }
        },
        [cmd, ..] if cmd == "settle" => match parse_settle_args(&args) {
            Ok((market, orders, opts)) => run_settle(&market, &orders, opts),
            Err(e) => {
                usage();
                Err(e)
            }
        },
        #[cfg(feature = "http")]
        [cmd, ..] if cmd == "intents-check" => match parse_intents_check_args(&args) {
            Ok((market, orders, record)) => run_intents_check(&market, &orders, record),
            Err(e) => {
                usage();
                Err(e)
            }
        },
        #[cfg(feature = "http")]
        [cmd, ..] if cmd == "live-check" => match parse_live_check_args(&args) {
            Ok((market, orders, opts)) => run_live_check(&market, &orders, opts),
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
