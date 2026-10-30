use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use escrow_types::{order_hash, Order, Side, SignedOrder};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::read_seed_hex;

#[derive(Serialize)]
pub struct MakeOrderLine {
    pub order_hash: String,
    pub public_key_hex: String,
    pub signed_order: SignedOrder,
}

#[derive(Serialize)]
pub struct SettleArgsLine {
    pub buy: SignedOrder,
    pub sell: SignedOrder,
    pub fill_lots: u64,
    pub price_ticks: u64,
}

#[derive(Serialize)]
pub struct VerifyValidLine {
    pub valid: bool,
    pub order_hash: String,
}

#[derive(Serialize)]
pub struct VerifyInvalidLine {
    pub valid: bool,
}

pub fn load_signed_order_file(path: &Path) -> Result<SignedOrder, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read order file: {e}"))?;
    let trimmed = raw.trim();
    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("parse JSON: {e}"))?;
    if let Some(inner) = value.get("signed_order") {
        serde_json::from_value(inner.clone()).map_err(|e| format!("parse signed_order: {e}"))
    } else {
        serde_json::from_value(value).map_err(|e| format!("parse SignedOrder: {e}"))
    }
}

fn parse_side(s: &str) -> Result<Side, String> {
    match s {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        _ => Err("side must be buy or sell".to_string()),
    }
}

fn parse_u128(name: &str, s: &str) -> Result<u128, String> {
    s.parse::<u128>()
        .map_err(|_| format!("invalid {name}: must be a non-negative integer"))
}

fn parse_u64(name: &str, s: &str) -> Result<u64, String> {
    s.parse::<u64>()
        .map_err(|_| format!("invalid {name}: must be a non-negative integer"))
}

fn non_empty_id(name: &str, s: &str) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    Ok(())
}

fn expiry_ns_from_flags(
    expiry_ns: Option<u64>,
    expires_in_secs: Option<u64>,
) -> Result<u64, String> {
    match (expiry_ns, expires_in_secs) {
        (Some(_), Some(_)) => {
            Err("use exactly one of --expiry-ns or --expires-in-secs".to_string())
        }
        (None, None) => Err("missing expiry: use --expiry-ns or --expires-in-secs".to_string()),
        (Some(v), None) => Ok(v),
        (None, Some(secs)) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("system time before UNIX epoch: {e}"))?;
            let now_ns = u64::try_from(now.as_nanos())
                .map_err(|_| "current time in nanoseconds does not fit in u64".to_string())?;
            let delta = secs.checked_mul(1_000_000_000).ok_or_else(|| {
                "expires-in-secs overflow when converting to nanoseconds".to_string()
            })?;
            now_ns
                .checked_add(delta)
                .ok_or_else(|| "expiry_ns overflow".to_string())
        }
    }
}

fn sign_order(seed: &[u8; 32], order: &Order) -> Result<(String, String, SignedOrder), String> {
    let hash = order_hash(order)?;
    let hash_hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    let sk = SigningKey::from_bytes(seed);
    let sig = sk.sign(&hash);
    let sig_hex = hex::encode(sig.to_bytes());
    let public_key_hex = hex::encode(sk.verifying_key().to_bytes());
    let signed_order = SignedOrder {
        order: order.clone(),
        signature: sig_hex,
    };
    Ok((hash_hex, public_key_hex, signed_order))
}

/// Raw command-line values of `make-order` (validated in `cmd_make_order`).
pub struct MakeOrderArgs<'a> {
    pub key_file: &'a Path,
    pub contract_id: &'a str,
    pub account_id: &'a str,
    pub base_token: &'a str,
    pub quote_token: &'a str,
    pub lot_size_base: &'a str,
    pub quote_per_tick_lot: &'a str,
    pub side: &'a str,
    pub price_ticks: &'a str,
    pub qty_lots: &'a str,
    pub nonce: &'a str,
    pub expiry_ns: Option<&'a str>,
    pub expires_in_secs: Option<&'a str>,
}

pub fn cmd_make_order(args: &MakeOrderArgs<'_>) -> Result<(), String> {
    let MakeOrderArgs {
        key_file,
        contract_id,
        account_id,
        base_token,
        quote_token,
        lot_size_base,
        quote_per_tick_lot,
        side,
        price_ticks,
        qty_lots,
        nonce,
        expiry_ns,
        expires_in_secs,
    } = *args;
    non_empty_id("contract_id", contract_id)?;
    non_empty_id("account_id", account_id)?;
    non_empty_id("base_token", base_token)?;
    non_empty_id("quote_token", quote_token)?;

    let lot_size_base = parse_u128("lot_size_base", lot_size_base)?;
    let quote_per_tick_lot = parse_u128("quote_per_tick_lot", quote_per_tick_lot)?;
    if lot_size_base == 0 {
        return Err("lot_size_base must be greater than 0".to_string());
    }
    if quote_per_tick_lot == 0 {
        return Err("quote_per_tick_lot must be greater than 0".to_string());
    }

    let price_ticks = parse_u64("price_ticks", price_ticks)?;
    let qty_lots = parse_u64("qty_lots", qty_lots)?;
    if price_ticks == 0 {
        return Err("price_ticks must be greater than 0".to_string());
    }
    if qty_lots == 0 {
        return Err("qty_lots must be greater than 0".to_string());
    }

    let nonce = parse_u64("nonce", nonce)?;
    let expiry_flag_ns = match expiry_ns {
        Some(s) => Some(parse_u64("expiry_ns", s)?),
        None => None,
    };
    let expiry_flag_secs = match expires_in_secs {
        Some(s) => Some(parse_u64("expires_in_secs", s)?),
        None => None,
    };
    let expiry_ns = expiry_ns_from_flags(expiry_flag_ns, expiry_flag_secs)?;

    let order = Order {
        v: 1,
        contract_id: contract_id.to_string(),
        account_id: account_id.to_string(),
        base_token: base_token.to_string(),
        quote_token: quote_token.to_string(),
        lot_size_base,
        quote_per_tick_lot,
        side: parse_side(side)?,
        price_ticks,
        qty_lots,
        expiry_ns,
        nonce,
    };

    let seed = read_seed_hex(key_file)?;
    let (order_hash_hex, public_key_hex, signed_order) = sign_order(&seed, &order)?;
    let line = MakeOrderLine {
        order_hash: order_hash_hex,
        public_key_hex,
        signed_order,
    };
    let json = serde_json::to_string(&line).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

pub fn validate_settle_pair(
    buy: &SignedOrder,
    sell: &SignedOrder,
    fill_lots: u64,
    price_ticks: u64,
) -> Result<(), String> {
    if buy.order.side != Side::Buy {
        return Err("buy order must have side buy".to_string());
    }
    if sell.order.side != Side::Sell {
        return Err("sell order must have side sell".to_string());
    }
    if buy.order.account_id == sell.order.account_id {
        return Err("buy and sell accounts must differ".to_string());
    }
    if buy.order.contract_id != sell.order.contract_id {
        return Err("contract_id mismatch between buy and sell".to_string());
    }
    if buy.order.base_token != sell.order.base_token
        || buy.order.quote_token != sell.order.quote_token
        || buy.order.lot_size_base != sell.order.lot_size_base
        || buy.order.quote_per_tick_lot != sell.order.quote_per_tick_lot
    {
        return Err("market fields must match between buy and sell".to_string());
    }
    if fill_lots == 0 {
        return Err("fill_lots must be greater than 0".to_string());
    }
    if fill_lots > buy.order.qty_lots {
        return Err("fill_lots exceeds buy order qty_lots".to_string());
    }
    if fill_lots > sell.order.qty_lots {
        return Err("fill_lots exceeds sell order qty_lots".to_string());
    }
    if price_ticks < sell.order.price_ticks {
        return Err("price_ticks below sell limit".to_string());
    }
    if price_ticks > buy.order.price_ticks {
        return Err("price_ticks above buy limit".to_string());
    }
    Ok(())
}

pub fn cmd_settle_args(
    buy_file: &Path,
    sell_file: &Path,
    fill_lots: &str,
    price_ticks: &str,
) -> Result<(), String> {
    let fill_lots = parse_u64("fill_lots", fill_lots)?;
    let price_ticks = parse_u64("price_ticks", price_ticks)?;
    let buy = load_signed_order_file(buy_file)?;
    let sell = load_signed_order_file(sell_file)?;
    validate_settle_pair(&buy, &sell, fill_lots, price_ticks)?;
    let line = SettleArgsLine {
        buy,
        sell,
        fill_lots,
        price_ticks,
    };
    let json = serde_json::to_string(&line).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn verifying_key_from_hex(hex_str: &str) -> Result<VerifyingKey, String> {
    if hex_str.len() != 64 || !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("public_key_hex must be 64 hex characters".to_string());
    }
    let bytes = hex::decode(hex_str).map_err(|e| format!("invalid public_key_hex: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&arr).map_err(|e| format!("invalid public key: {e}"))
}

pub fn verify_signed_order(signed: &SignedOrder, vk: &VerifyingKey) -> Result<[u8; 32], String> {
    let hash = order_hash(&signed.order)?;
    let sig_bytes =
        hex::decode(&signed.signature).map_err(|e| format!("invalid signature hex: {e}"))?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(&hash, &sig)
        .map_err(|_| "signature verification failed".to_string())?;
    Ok(hash)
}

/// Returns `Ok(true)` when valid, `Ok(false)` after printing invalid JSON (exit 2 in main).
pub fn cmd_verify_order(
    signed_order_file: &Path,
    public_key_hex: Option<&str>,
    key_file: Option<&Path>,
) -> Result<bool, String> {
    if public_key_hex.is_some() && key_file.is_some() {
        return Err("use exactly one of --public-key-hex or --key-file".to_string());
    }
    if public_key_hex.is_none() && key_file.is_none() {
        return Err("missing --public-key-hex or --key-file".to_string());
    }
    let signed = load_signed_order_file(signed_order_file)?;
    let vk = if let Some(hex_str) = public_key_hex {
        verifying_key_from_hex(hex_str)?
    } else {
        let seed = read_seed_hex(key_file.ok_or("internal: key_file")?)?;
        SigningKey::from_bytes(&seed).verifying_key()
    };
    match verify_signed_order(&signed, &vk) {
        Ok(hash) => {
            let hash_hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            let line = VerifyValidLine {
                valid: true,
                order_hash: hash_hex,
            };
            let json = serde_json::to_string(&line).map_err(|e| e.to_string())?;
            println!("{json}");
            Ok(true)
        }
        Err(_) => {
            let line = VerifyInvalidLine { valid: false };
            let json = serde_json::to_string(&line).map_err(|e| e.to_string())?;
            println!("{json}");
            Ok(false)
        }
    }
}
