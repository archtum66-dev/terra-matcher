#![forbid(unsafe_code)]
//! `terra-1b`: one real settlement of one Terra fill on NEAR mainnet through the
//! NEAR Intents verifier (`intents.near`), with throwaway keys and a few francs.
//!
//! Safety design (the money in play is a few USD, but the rules are strict):
//! - keys are made on the machine that runs this program and never printed;
//! - `execute` is the only command that sends a transaction. It needs the typed
//!   words `JA SENDEN`, so it cannot run unattended or from a script with piped input;
//! - hard caps: at most 1.2 NEAR and 5 USDC move, one fill, deadline at most 120 minutes;
//! - `plan` runs everything except the send, including the verifier's own
//!   `simulate_intents`.

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process;

use ed25519_dalek::{Signer, SigningKey};
use matcher_api::{Market, Matcher, Message};
use matcher_stub::StubMatcher;
use serde_json::{json, Value};
use settlement::{settle_fill, Transport};
use sha2::{Digest, Sha256};

const NEAR_RPC: &str = "https://rpc.mainnet.near.org";
const ONECLICK_QUOTE: &str = "https://1click.chaindefuser.com/v0/quote";
const MAX_BASE_YOCTO: u128 = 1_200_000_000_000_000_000_000_000; // 1.2 NEAR
const MAX_QUOTE_UNITS: u128 = 5_000_000; // 5 USDC (6 decimals)
const MAX_VALID_MINUTES: u64 = 120;
const TX_GAS: u64 = 200_000_000_000_000; // 200 Tgas, unused gas is refunded
const MIN_GAS_BALANCE_YOCTO: u128 = 40_000_000_000_000_000_000_000; // 0.04 NEAR
const CONFIRM_WORDS: &str = "JA SENDEN";

fn usage() {
    eprintln!("terra-1b: one real Terra fill on NEAR Intents (mainnet)");
    eprintln!("  terra-1b keys     --out <keys.json>                 make two throwaway keys (seller, buyer)");
    eprintln!("  terra-1b accounts --keys <keys.json>                public accounts and keys (no secrets)");
    eprintln!("  terra-1b balances --keys <keys.json> --market <market.json>");
    eprintln!("  terra-1b plan     --keys <keys.json> --market <m.json> --orders <o.jsonl> [--reverse]   sign + simulate, sends nothing");
    eprintln!("  terra-1b execute  --keys <keys.json> --market <m.json> --orders <o.jsonl> [--reverse] [--record <dir>]   asks for typed confirmation");
    eprintln!("  --reverse: the two accounts swap roles (the buyer account of the first run now gives NEAR and the seller account gives USDC);");
    eprintln!("             the seller account of the keys file still pays the network fee");
    eprintln!("  terra-1b fund-quote --account <64 hex> --origin <asset> --dest <asset> --amount <units> --refund-to <address> [--dest-chain]");
    eprintln!("  terra-1b selftest                                    transaction format check against the live RPC (random account, no funds)");
}

fn fail(msg: &str) -> ! {
    eprintln!("ERROR: {msg}");
    process::exit(2);
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn need(args: &[String], name: &str) -> String {
    flag(args, name).unwrap_or_else(|| fail(&format!("missing {name}")))
}

// ---------------------------------------------------------------- encoding helpers

const B58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn b58_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut bytes: Vec<u8> = Vec::new();
    for c in s.bytes() {
        let mut carry =
            B58.iter()
                .position(|&x| x == c)
                .ok_or_else(|| format!("bad base58 character {}", c as char))? as u32;
        for b in bytes.iter_mut() {
            carry += u32::from(*b) * 58;
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    let zeros = s.bytes().take_while(|&c| c == b'1').count();
    bytes.resize(bytes.len() + zeros, 0u8);
    bytes.reverse();
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex32(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 {
        return Err("seed must be 64 hex characters".to_string());
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn iso_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
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

// ---------------------------------------------------------------- keys file

struct Party {
    account: String,
    key: SigningKey,
}

impl Party {
    fn public_key(&self) -> String {
        format!(
            "ed25519:{}",
            intents_sign::base58(&self.key.verifying_key().to_bytes())
        )
    }
}

fn party_json(key: &SigningKey) -> Value {
    let seed = key.to_bytes();
    let mut full = [0u8; 64];
    full[..32].copy_from_slice(&seed);
    full[32..].copy_from_slice(&key.verifying_key().to_bytes());
    json!({
        "account": intents_sign::implicit_account(key),
        "public_key": format!("ed25519:{}", intents_sign::base58(&key.verifying_key().to_bytes())),
        "seed_hex": hex(&seed),
        "private_key": format!("ed25519:{}", intents_sign::base58(&full)),
    })
}

fn read_party(v: &Value) -> Result<Party, String> {
    let seed = unhex32(
        v["seed_hex"]
            .as_str()
            .ok_or("keys file: seed_hex missing")?,
    )?;
    let key = SigningKey::from_bytes(&seed);
    let account = intents_sign::implicit_account(&key);
    if v["account"].as_str() != Some(account.as_str()) {
        return Err("keys file: account does not match the key".to_string());
    }
    Ok(Party { account, key })
}

fn read_keys(path: &str) -> Result<(Party, Party), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read keys file: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("keys file: {e}"))?;
    Ok((read_party(&v["seller"])?, read_party(&v["buyer"])?))
}

/// Accounts in their roles for one fill: (gives NEAR, gives USDC). `--reverse` swaps the
/// two accounts of the keys file, so a second fill can run the other way round with the
/// balances the first fill left behind. The fee is always paid by the first account of
/// the keys file (`seller`), the only one that holds native NEAR.
fn assign_roles(first: Party, second: Party, reverse: bool) -> (Party, Party) {
    if reverse {
        (second, first)
    } else {
        (first, second)
    }
}

fn is_reverse(args: &[String]) -> bool {
    args.iter().any(|a| a == "--reverse")
}

fn read_roles(args: &[String]) -> Result<(Party, Party), String> {
    let (first, second) = read_keys(&need(args, "--keys"))?;
    Ok(assign_roles(first, second, is_reverse(args)))
}

fn cmd_keys(args: &[String]) -> Result<(), String> {
    let out = need(args, "--out");
    if Path::new(&out).exists() {
        return Err(format!(
            "{out} exists already; it holds the keys of the funded accounts, never overwrite it"
        ));
    }
    if let Some(dir) = Path::new(&out).parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir).map_err(|e| format!("create folder: {e}"))?;
        }
    }
    let seller = intents_sign::random_key();
    let buyer = intents_sign::random_key();
    let doc = json!({
        "version": 1,
        "note": "Throwaway keys of the Terra 1b mainnet check. They control the funds sent to these two accounts. Keep this file private and do not delete it before the tokens are taken out.",
        "created_utc": iso_utc(now_secs()),
        "seller": party_json(&seller),
        "buyer": party_json(&buyer),
    });
    fs::write(&out, serde_json::to_string_pretty(&doc).unwrap_or_default())
        .map_err(|e| format!("write keys file: {e}"))?;
    println!("keys file written: {out}");
    cmd_accounts(&["--keys".to_string(), out])
}

fn cmd_accounts(args: &[String]) -> Result<(), String> {
    let (seller, buyer) = read_keys(&need(args, "--keys"))?;
    println!(
        "{}",
        json!({
            "seller": {"account": seller.account, "public_key": seller.public_key()},
            "buyer": {"account": buyer.account, "public_key": buyer.public_key()},
        })
    );
    Ok(())
}

// ---------------------------------------------------------------- RPC

fn rpc_url() -> String {
    env::var("TERRA_NEAR_RPC").unwrap_or_else(|_| NEAR_RPC.to_string())
}

/// JSON-RPC call; `Ok(Err(text))` is an error object from the node, `Err` a transport failure.
fn rpc(method: &str, params: Value) -> Result<Result<Value, Value>, String> {
    let body = json!({"jsonrpc": "2.0", "id": "terra-1b", "method": method, "params": params});
    let resp = settlement::HttpTransport
        .post_json(&rpc_url(), &[], &body.to_string())
        .map_err(|e| format!("{method}: {e}"))?;
    let v: Value =
        serde_json::from_str(&resp.body).map_err(|e| format!("{method}: answer: {e}"))?;
    if let Some(err) = v.get("error") {
        return Ok(Err(err.clone()));
    }
    v.get("result")
        .cloned()
        .map(Ok)
        .ok_or_else(|| format!("{method}: answer without result"))
}

fn view_call(contract: &str, method: &str, args: &Value) -> Result<Result<Value, String>, String> {
    let params = json!({
        "request_type": "call_function", "finality": "final",
        "account_id": contract, "method_name": method,
        "args_base64": intents_sign::base64(args.to_string().as_bytes()),
    });
    match rpc("query", params)? {
        Err(e) => Ok(Err(e.to_string())),
        Ok(result) => {
            if let Some(err) = result.get("error").and_then(|e| e.as_str()) {
                return Ok(Err(err.to_string()));
            }
            let bytes: Vec<u8> = result["result"]
                .as_array()
                .ok_or("view: no result bytes")?
                .iter()
                .map(|b| b.as_u64().unwrap_or(0) as u8)
                .collect();
            serde_json::from_slice(&bytes)
                .map(Ok)
                .map_err(|e| format!("view result: {e}"))
        }
    }
}

/// Native NEAR balance in yoctoNEAR; `None` when the account does not exist yet.
fn native_balance(account: &str) -> Result<Option<u128>, String> {
    let params =
        json!({"request_type": "view_account", "finality": "final", "account_id": account});
    match rpc("query", params)? {
        Ok(v) => v["amount"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .map(Some)
            .ok_or_else(|| "view_account: no amount".to_string()),
        Err(e) => {
            let text = e.to_string();
            if text.contains("UNKNOWN_ACCOUNT") || text.contains("does not exist") {
                Ok(None)
            } else {
                Err(format!("view_account: {text}"))
            }
        }
    }
}

fn read_market(path: &str) -> Result<Market, String> {
    let data = fs::read_to_string(path).map_err(|e| format!("read market: {e}"))?;
    serde_json::from_str(&data).map_err(|e| format!("parse market: {e}"))
}

fn token_balance(market: &Market, account: &str) -> Result<(u128, u128), String> {
    let v = view_call(
        intents_sign::VERIFIER,
        "mt_batch_balance_of",
        &json!({"account_id": account, "token_ids": [market.base_token, market.quote_token]}),
    )?
    .map_err(|e| format!("mt_batch_balance_of: {e}"))?;
    let get = |i: usize| -> u128 { v[i].as_str().and_then(|s| s.parse().ok()).unwrap_or(0) };
    Ok((get(0), get(1)))
}

struct Balances {
    seller_base: u128,
    seller_native: u128,
    buyer_native: u128,
    buyer_quote: u128,
}

fn read_balances(market: &Market, seller: &Party, buyer: &Party) -> Result<Balances, String> {
    let (s_base, s_quote) = token_balance(market, &seller.account)?;
    let (b_base, b_quote) = token_balance(market, &buyer.account)?;
    let s_native = native_balance(&seller.account)?.unwrap_or(0);
    let b_native = native_balance(&buyer.account)?.unwrap_or(0);
    // wNEAR that sits on the NEAR chain itself (not in the verifier): shows whether a
    // payment to the account arrived as real NEAR (native) or as the wrapped token.
    let wnear_on_chain = |acct: &str| -> String {
        view_call("wrap.near", "ft_balance_of", &json!({"account_id": acct}))
            .ok()
            .and_then(Result::ok)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "?".to_string())
    };
    println!(
        "{}",
        json!({"balances": {
            "seller": {"account": seller.account, "base_in_verifier": s_base.to_string(), "quote_in_verifier": s_quote.to_string(), "native_near_yocto": s_native.to_string(), "wnear_token_on_chain_yocto": wnear_on_chain(&seller.account)},
            "buyer": {"account": buyer.account, "base_in_verifier": b_base.to_string(), "quote_in_verifier": b_quote.to_string(), "native_near_yocto": b_native.to_string(), "wnear_token_on_chain_yocto": wnear_on_chain(&buyer.account)},
            "base_token": market.base_token, "quote_token": market.quote_token,
        }})
    );
    Ok(Balances {
        seller_base: s_base,
        seller_native: s_native,
        buyer_native: b_native,
        buyer_quote: b_quote,
    })
}

fn cmd_balances(args: &[String]) -> Result<(), String> {
    let (seller, buyer) = read_keys(&need(args, "--keys"))?;
    let market = read_market(&need(args, "--market"))?;
    read_balances(&market, &seller, &buyer).map(|_| ())
}

// ---------------------------------------------------------------- signing

struct Prepared {
    signed: Vec<Value>,
    base: u128,
    quote: u128,
    deadline: String,
    salt: String,
    fee_pips: u32,
}

fn prepare(args: &[String], seller: &Party, buyer: &Party) -> Result<Prepared, String> {
    let market = read_market(&need(args, "--market"))?;
    let orders =
        fs::read_to_string(need(args, "--orders")).map_err(|e| format!("read orders: {e}"))?;
    let minutes: u64 = flag(args, "--valid-minutes")
        .map(|v| v.parse().unwrap_or(0))
        .unwrap_or(30);
    if minutes == 0 || minutes > MAX_VALID_MINUTES {
        return Err(format!("--valid-minutes must be 1..={MAX_VALID_MINUTES}"));
    }
    let salt_hex = view_call(intents_sign::VERIFIER, "current_salt", &json!({}))??.to_string();
    let salt =
        intents_sign::parse_salt(&salt_hex).ok_or_else(|| format!("unexpected salt {salt_hex}"))?;
    let fee_pips = view_call(intents_sign::VERIFIER, "fee", &json!({}))??
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
        .ok_or("unexpected fee")?;

    let mut matcher = StubMatcher::new(market.clone());
    let mut fills = Vec::new();
    for (n, line) in orders.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let msg: Message =
            serde_json::from_str(t).map_err(|e| format!("order line {}: {e}", n + 1))?;
        let (_ack, f) = matcher.process(msg);
        fills.extend(f);
    }
    if fills.len() != 1 {
        return Err(format!("expected exactly one fill, got {}", fills.len()));
    }
    let deadline_secs = now_secs() + minutes * 60;
    let deadline = iso_utc(deadline_secs);
    let pair = settle_fill(&market, &fills[0], &deadline).map_err(|e| format!("settle: {e}"))?;
    let mut pair =
        intents_sign::apply_protocol_fee(&pair, fee_pips).map_err(|e| format!("fee: {e}"))?;
    let base: u128 = matcher_api::base_amount(&market, fills[0].qty_lots).ok_or("base overflow")?;
    let quote: u128 = matcher_api::quote_amount(&market, fills[0].qty_lots, fills[0].price_ticks)
        .ok_or("quote overflow")?;
    if base > MAX_BASE_YOCTO || quote > MAX_QUOTE_UNITS {
        return Err(format!("amounts above the hard caps (base {base} > {MAX_BASE_YOCTO} or quote {quote} > {MAX_QUOTE_UNITS})"));
    }
    pair.buyer_draft.signer_id = buyer.account.clone();
    pair.seller_draft.signer_id = seller.account.clone();
    let ns = deadline_secs.saturating_mul(1_000_000_000);
    let signed = vec![
        intents_sign::sign_nep413(
            &buyer.key,
            &pair.buyer_draft,
            intents_sign::nonce_v1(salt, ns, intents_sign::random_nonce_tail()),
        ),
        intents_sign::sign_nep413(
            &seller.key,
            &pair.seller_draft,
            intents_sign::nonce_v1(salt, ns, intents_sign::random_nonce_tail()),
        ),
    ];
    Ok(Prepared {
        signed,
        base,
        quote,
        deadline,
        salt: salt_hex,
        fee_pips,
    })
}

fn money(base: u128, quote: u128) -> String {
    format!(
        "{}.{:06} NEAR  <->  {}.{:06} USDC",
        base / 1_000_000_000_000_000_000_000_000,
        (base % 1_000_000_000_000_000_000_000_000) / 1_000_000_000_000_000_000,
        quote / 1_000_000,
        quote % 1_000_000
    )
}

/// Builds, signs, checks balances and runs `simulate_intents`. Sends nothing.
fn plan(args: &[String]) -> Result<(Party, Party, Prepared, bool), String> {
    let (seller, buyer) = read_roles(args)?;
    let reverse = is_reverse(args);
    let market = read_market(&need(args, "--market"))?;
    let prep = prepare(args, &seller, &buyer)?;
    println!("Fill: {}", money(prep.base, prep.quote));
    if reverse {
        println!("Reverse run: the two accounts of the keys file swapped roles");
    }
    println!("Seller (gives NEAR): {}", seller.account);
    println!("Buyer  (gives USDC): {}", buyer.account);
    println!(
        "Verifier fee: {} pips, salt {}, valid until {}",
        prep.fee_pips, prep.salt, prep.deadline
    );
    let bal = read_balances(&market, &seller, &buyer)?;
    let funded_base = bal.seller_base >= prep.base;
    let funded_quote = bal.buyer_quote >= prep.quote;
    let payer_native = if reverse {
        bal.buyer_native
    } else {
        bal.seller_native
    };
    let funded_gas = payer_native >= MIN_GAS_BALANCE_YOCTO;
    println!("Seller holds enough NEAR in the verifier: {funded_base}");
    println!("Buyer holds enough USDC in the verifier:  {funded_quote}");
    println!("Account that pays the network fee holds NEAR for it (>= 0.04): {funded_gas}");
    let sim = view_call(
        intents_sign::VERIFIER,
        "simulate_intents",
        &json!({"signed": prep.signed}),
    )?;
    let sim_ok = match &sim {
        Ok(_) => {
            println!("simulate_intents: the verifier would execute both intents");
            true
        }
        Err(msg) => {
            println!("simulate_intents: {msg}");
            false
        }
    };
    let ready = funded_base && funded_quote && funded_gas && sim_ok;
    println!("READY: {ready}");
    Ok((seller, buyer, prep, ready))
}

fn cmd_plan(args: &[String]) -> Result<(), String> {
    plan(args).map(|_| ())
}

// ---------------------------------------------------------------- transaction

fn borsh_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

/// One-action transaction: `FunctionCall`, borsh encoded, signed ed25519 over sha256.
/// Returns (signed bytes, transaction hash in base58).
fn build_signed_tx(
    signer: &Party,
    receiver: &str,
    nonce: u64,
    block_hash: &[u8; 32],
    method: &str,
    args: &[u8],
    gas: u64,
) -> (Vec<u8>, String) {
    let mut tx = Vec::new();
    borsh_string(&mut tx, &signer.account);
    tx.push(0u8); // KeyType::ED25519
    tx.extend_from_slice(&signer.key.verifying_key().to_bytes());
    tx.extend_from_slice(&nonce.to_le_bytes());
    borsh_string(&mut tx, receiver);
    tx.extend_from_slice(block_hash);
    tx.extend_from_slice(&1u32.to_le_bytes()); // one action
    tx.push(2u8); // Action::FunctionCall
    borsh_string(&mut tx, method);
    tx.extend_from_slice(&(args.len() as u32).to_le_bytes());
    tx.extend_from_slice(args);
    tx.extend_from_slice(&gas.to_le_bytes());
    tx.extend_from_slice(&0u128.to_le_bytes()); // deposit
    let digest: [u8; 32] = Sha256::digest(&tx).into();
    let sig = signer.key.sign(&digest);
    let mut signed = tx;
    signed.push(0u8); // KeyType::ED25519
    signed.extend_from_slice(&sig.to_bytes());
    (signed, intents_sign::base58(&digest))
}

fn access_key_state(signer: &Party) -> Result<(u64, [u8; 32]), String> {
    let params = json!({"request_type": "view_access_key", "finality": "final", "account_id": signer.account, "public_key": signer.public_key()});
    let v = rpc("query", params)?.map_err(|e| format!("view_access_key: {e}"))?;
    let nonce = v["nonce"].as_u64().ok_or("access key: no nonce")?;
    let hash = b58_decode(
        v["block_hash"]
            .as_str()
            .ok_or("access key: no block hash")?,
    )?;
    let arr: [u8; 32] = hash
        .try_into()
        .map_err(|_| "block hash must be 32 bytes".to_string())?;
    Ok((nonce, arr))
}

fn outcome_summary(result: &Value) -> (bool, Vec<String>) {
    let mut ok = result["status"].get("SuccessValue").is_some();
    let mut events = Vec::new();
    if let Some(receipts) = result["receipts_outcome"].as_array() {
        for r in receipts {
            if r["outcome"]["status"].get("Failure").is_some() {
                ok = false;
            }
            if let Some(logs) = r["outcome"]["logs"].as_array() {
                for l in logs.iter().filter_map(|l| l.as_str()) {
                    if l.contains("token_diff") || l.contains("intents_executed") {
                        events.push(l.to_string());
                    }
                }
            }
        }
    }
    (ok, events)
}

fn cmd_execute(args: &[String]) -> Result<(), String> {
    let (seller, buyer, prep, ready) = plan(args)?;
    if !ready {
        return Err("not ready: nothing was sent".to_string());
    }
    // The network fee is paid by the first account of the keys file, in either direction.
    let reverse = is_reverse(args);
    let payer = if reverse { &buyer } else { &seller };
    println!();
    println!(
        "This sends ONE real transaction on NEAR mainnet: execute_intents on {}",
        intents_sign::VERIFIER
    );
    println!(
        "It moves {} between the two throwaway accounts above.",
        money(prep.base, prep.quote)
    );
    println!(
        "The network fee is paid from account {} (at most 0.04 NEAR, unused gas is refunded).",
        payer.account
    );
    print!("Type exactly {CONFIRM_WORDS} and press Enter to send, anything else cancels: ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    if line.trim() != CONFIRM_WORDS {
        return Err("cancelled: nothing was sent".to_string());
    }
    let (nonce, block_hash) = access_key_state(payer)?;
    let call_args = json!({"signed": prep.signed}).to_string();
    let (signed_tx, tx_hash) = build_signed_tx(
        payer,
        intents_sign::VERIFIER,
        nonce + 1,
        &block_hash,
        "execute_intents",
        call_args.as_bytes(),
        TX_GAS,
    );
    println!("Sending transaction {tx_hash} ...");
    let sent = rpc(
        "send_tx",
        json!({"signed_tx_base64": intents_sign::base64(&signed_tx), "wait_until": "FINAL"}),
    )?;
    let result = match sent {
        Ok(r) => r,
        Err(e) => return Err(format!("the node refused or lost the transaction: {e}\nCheck https://nearblocks.io/txns/{tx_hash} before trying again; never send twice blindly.")),
    };
    let (ok, events) = outcome_summary(&result);
    println!(
        "Transaction {tx_hash}: {}",
        if ok {
            "SUCCESS"
        } else {
            "FAILED (see the node answer in the record)"
        }
    );
    for e in &events {
        println!("  {e}");
    }
    println!("Explorer: https://nearblocks.io/txns/{tx_hash}");
    let market = read_market(&need(args, "--market"))?;
    let after = read_balances(&market, &seller, &buyer)?;
    let record = json!({
        "tx_hash": tx_hash, "ok": ok, "network": "mainnet", "verifier": intents_sign::VERIFIER,
        "seller": seller.account, "buyer": buyer.account, "signer": payer.account, "reverse": reverse,
        "base_yocto": prep.base.to_string(), "quote_units": prep.quote.to_string(),
        "fee_pips": prep.fee_pips, "events": events,
        "gas_burnt_total": result["receipts_outcome"].as_array().map(|a| a.iter().filter_map(|r| r["outcome"]["gas_burnt"].as_u64()).sum::<u64>()).unwrap_or(0) + result["transaction_outcome"]["outcome"]["gas_burnt"].as_u64().unwrap_or(0),
        "payer_native_after_yocto": (if reverse { after.buyer_native } else { after.seller_native }).to_string(),
        "time_utc": iso_utc(now_secs()),
    });
    let dir = flag(args, "--record").unwrap_or_else(|| ".".to_string());
    fs::create_dir_all(&dir).map_err(|e| format!("create record folder: {e}"))?;
    let file = Path::new(&dir).join(format!("terra-1b-result-{tx_hash}.json"));
    fs::write(
        &file,
        serde_json::to_string_pretty(&record).unwrap_or_default(),
    )
    .map_err(|e| format!("write record: {e}"))?;
    println!("Record (public data only): {}", file.display());
    if ok {
        Ok(())
    } else {
        Err("the transaction did not succeed".to_string())
    }
}

/// Format check of the transaction encoding against the live node: a transaction from a
/// random account that does not exist must be rejected for the account/key, which the
/// node only says after it parsed the bytes and checked the signature. Nothing is spent.
fn cmd_selftest() -> Result<(), String> {
    let key = intents_sign::random_key();
    let ghost = Party {
        account: intents_sign::implicit_account(&key),
        key,
    };
    let head = rpc("block", json!({"finality": "final"}))?.map_err(|e| format!("block: {e}"))?;
    let hash = b58_decode(
        head["header"]["hash"]
            .as_str()
            .ok_or("block hash missing")?,
    )?;
    let block_hash: [u8; 32] = hash
        .try_into()
        .map_err(|_| "block hash length".to_string())?;
    let (signed, tx_hash) = build_signed_tx(
        &ghost,
        intents_sign::VERIFIER,
        1,
        &block_hash,
        "execute_intents",
        br#"{"signed":[]}"#,
        TX_GAS,
    );
    let answer = rpc(
        "send_tx",
        json!({"signed_tx_base64": intents_sign::base64(&signed), "wait_until": "EXECUTED_OPTIMISTIC"}),
    )?;
    let text = match answer {
        Ok(v) => format!("ACCEPTED?? {v}"),
        Err(e) => e.to_string(),
    };
    println!("tx {tx_hash}");
    println!("node answer: {text}");
    let parsed = text.contains("SignerDoesNotExist")
        || text.contains("AccessKeyNotFound")
        || text.contains("DoesNotExist")
        || text.contains("does not exist")
        || text.contains("not found");
    let bad_format = text.contains("Invalid")
        && (text.contains("Borsh") || text.contains("deserial") || text.contains("signature"));
    if parsed && !bad_format {
        println!("OK: the node parsed the transaction and rejected it only because the random account does not exist");
        Ok(())
    } else {
        Err("unexpected node answer, the transaction encoding is not verified".to_string())
    }
}

// ---------------------------------------------------------------- 1Click deposit address

fn cmd_fund_quote(args: &[String]) -> Result<(), String> {
    let account = need(args, "--account");
    let dest_chain = args.iter().any(|a| a == "--dest-chain");
    let body = json!({
        "dry": false, "swapType": "EXACT_INPUT", "slippageTolerance": 100,
        "originAsset": need(args, "--origin"), "depositType": "ORIGIN_CHAIN",
        "destinationAsset": need(args, "--dest"), "amount": need(args, "--amount"),
        "refundTo": need(args, "--refund-to"), "refundType": "ORIGIN_CHAIN",
        "recipient": account,
        "recipientType": if dest_chain { "DESTINATION_CHAIN" } else { "INTENTS" },
        "deadline": iso_utc(now_secs() + 3 * 3600),
    });
    let mut headers = Vec::new();
    if let Ok(k) = env::var("ONECLICK_API_KEY") {
        if !k.trim().is_empty() {
            headers.push(("Authorization".to_string(), format!("Bearer {}", k.trim())));
        }
    }
    let resp = settlement::HttpTransport
        .post_json(ONECLICK_QUOTE, &headers, &body.to_string())
        .map_err(|e| format!("1Click: {e}"))?;
    let v: Value = serde_json::from_str(&resp.body)
        .map_err(|e| format!("1Click answer: {e}: {}", resp.body))?;
    let q = &v["quote"];
    println!(
        "{}",
        json!({"status": resp.status, "deposit_address": q["depositAddress"], "deposit_deadline": q["deadline"],
               "amount_in": q["amountInFormatted"], "amount_in_usd": q["amountInUsd"],
               "amount_out": q["amountOutFormatted"], "amount_out_usd": q["amountOutUsd"],
               "min_amount_out": q["minAmountOut"], "time_estimate_s": q["timeEstimate"],
               "correlation_id": v["correlationId"], "message": v["message"]})
    );
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        usage();
        process::exit(2);
    };
    let rest = &args[1..];
    let result = match cmd {
        "keys" => cmd_keys(rest),
        "accounts" => cmd_accounts(rest),
        "balances" => cmd_balances(rest),
        "plan" => cmd_plan(rest),
        "execute" => cmd_execute(rest),
        "fund-quote" => cmd_fund_quote(rest),
        "selftest" => cmd_selftest(),
        _ => {
            usage();
            process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("ERROR: {e}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base58_round_trips_with_leading_zeros() {
        for bytes in [
            vec![0u8, 0, 1, 2, 3],
            vec![255u8; 32],
            vec![0u8; 5],
            (0u8..64).collect::<Vec<_>>(),
        ] {
            let enc = intents_sign::base58(&bytes);
            assert_eq!(b58_decode(&enc).unwrap(), bytes);
        }
        assert!(b58_decode("0OIl").is_err());
    }

    #[test]
    fn keys_file_round_trips_and_reverse_swaps_roles() {
        let k = intents_sign::random_key();
        let v = party_json(&k);
        let p = read_party(&v).unwrap();
        assert_eq!(p.account, intents_sign::implicit_account(&k));
        assert!(p.public_key().starts_with("ed25519:"));
        // the near-cli style private key is seed || public key, 64 bytes
        let full = b58_decode(
            v["private_key"]
                .as_str()
                .unwrap()
                .trim_start_matches("ed25519:"),
        )
        .unwrap();
        assert_eq!(full.len(), 64);
        assert_eq!(&full[..32], &k.to_bytes());
        // a tampered account is refused
        let mut bad = v.clone();
        bad["account"] = Value::String("00".repeat(32));
        assert!(read_party(&bad).is_err());
        // `--reverse` swaps the roles of the two accounts and nothing else
        let a = intents_sign::random_key();
        let b = intents_sign::random_key();
        let (a_acc, b_acc) = (
            intents_sign::implicit_account(&a),
            intents_sign::implicit_account(&b),
        );
        let mk = |k: SigningKey| Party {
            account: intents_sign::implicit_account(&k),
            key: k,
        };
        let (s, y) = assign_roles(mk(a.clone()), mk(b.clone()), false);
        assert_eq!(
            (s.account.as_str(), y.account.as_str()),
            (a_acc.as_str(), b_acc.as_str())
        );
        let (s, y) = assign_roles(mk(a), mk(b), true);
        assert_eq!(
            (s.account.as_str(), y.account.as_str()),
            (b_acc.as_str(), a_acc.as_str())
        );
        assert!(is_reverse(&["plan".to_string(), "--reverse".to_string()]));
        assert!(!is_reverse(&["plan".to_string()]));
    }

    #[test]
    fn transaction_layout_is_borsh() {
        let key = intents_sign::random_key();
        let p = Party {
            account: intents_sign::implicit_account(&key),
            key,
        };
        let (signed, hash) = build_signed_tx(
            &p,
            "intents.near",
            7,
            &[9u8; 32],
            "execute_intents",
            b"{}",
            1000,
        );
        // 4+64 signer, 1+32 key, 8 nonce, 4+12 receiver, 32 hash, 4 actions, 1 tag,
        // 4+15 method, 4+2 args, 8 gas, 16 deposit, then 1+64 signature
        let expected = 68 + 33 + 8 + 16 + 32 + 4 + 1 + 19 + 6 + 8 + 16 + 65;
        assert_eq!(signed.len(), expected);
        assert_eq!(&signed[..4], &64u32.to_le_bytes());
        assert_eq!(b58_decode(&hash).unwrap().len(), 32);
        // the signature verifies against the hash of the unsigned part
        use ed25519_dalek::Verifier;
        let body = &signed[..signed.len() - 65];
        let digest: [u8; 32] = Sha256::digest(body).into();
        let sig = ed25519_dalek::Signature::from_slice(&signed[signed.len() - 64..]).unwrap();
        assert!(p.key.verifying_key().verify(&digest, &sig).is_ok());
    }

    #[test]
    fn money_text_and_iso_time() {
        assert_eq!(
            money(1_000_000_000_000_000_000_000_000, 3_680_000),
            "1.000000 NEAR  <->  3.680000 USDC"
        );
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_794_074_568), "2026-11-07T18:02:48Z");
    }

    #[test]
    fn outcome_needs_success_and_no_failed_receipt() {
        let ok = json!({"status": {"SuccessValue": ""}, "receipts_outcome": [{"outcome": {"status": {"SuccessValue": ""}, "logs": ["EVENT_JSON:{\"event\":\"token_diff\"}"]}}]});
        let (good, events) = outcome_summary(&ok);
        assert!(good);
        assert_eq!(events.len(), 1);
        let bad = json!({"status": {"SuccessValue": ""}, "receipts_outcome": [{"outcome": {"status": {"Failure": {}}, "logs": []}}]});
        assert!(!outcome_summary(&bad).0);
    }
}
