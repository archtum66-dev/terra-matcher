#![forbid(unsafe_code)]

mod order;

use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

fn usage() {
    eprintln!("Usage:");
    eprintln!("  terra-keys keygen --name <name> --out <dir>");
    eprintln!("  terra-keys pubkey --key-file <path>");
    eprintln!("  terra-keys make-order --key-file <path> --contract-id <id> --account-id <id> --base-token <id> --quote-token <id> --lot-size-base <u128> --quote-per-tick-lot <u128> --side buy|sell --price-ticks <u64> --qty-lots <u64> --nonce <u64> (--expiry-ns <u64> | --expires-in-secs <u64>)");
    eprintln!("  terra-keys settle-args --buy-file <path> --sell-file <path> --fill-lots <u64> --price-ticks <u64>");
    eprintln!("  terra-keys verify-order --signed-order-file <path> (--public-key-hex <64 hex> | --key-file <path>)");
}

fn exit_usage() -> ! {
    usage();
    process::exit(2);
}

fn exit_err(msg: &str) -> ! {
    eprintln!("{msg}");
    process::exit(2);
}

pub(crate) fn read_seed_hex(path: &Path) -> Result<[u8; 32], String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read key file: {e}"))?;
    let trimmed = raw.trim();
    if trimmed.len() != 64 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("key file must contain exactly 64 hex characters".to_string());
    }
    let bytes = hex::decode(trimmed).map_err(|e| format!("invalid hex: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "seed must be 32 bytes".to_string())?;
    Ok(arr)
}

fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 32 {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn public_key_hex_from_seed(seed: &[u8; 32]) -> String {
    let sk = SigningKey::from_bytes(seed);
    hex::encode(sk.verifying_key().to_bytes())
}

#[cfg(unix)]
fn restrict_key_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict_key_file_permissions(_path: &Path) {}

fn cmd_keygen(name: &str, out_dir: &Path) -> Result<(), String> {
    if !valid_name(name) {
        return Err("invalid name: must match ^[a-z0-9_-]{1,32}$".to_string());
    }
    fs::create_dir_all(out_dir).map_err(|e| format!("create output dir: {e}"))?;
    let key_path = out_dir.join(format!("{name}.order-key"));
    if key_path.exists() {
        return Err("refusing to overwrite".to_string());
    }
    let signing_key = SigningKey::generate(&mut OsRng);
    let seed = signing_key.to_bytes();
    let seed_hex = hex::encode(seed);
    fs::write(&key_path, format!("{seed_hex}\n")).map_err(|e| format!("write key file: {e}"))?;
    restrict_key_file_permissions(&key_path);
    let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    let key_file = key_path.to_string_lossy();
    let line = serde_json::json!({
        "name": name,
        "public_key_hex": public_key_hex,
        "key_file": key_file,
    });
    println!("{line}");
    Ok(())
}

fn cmd_pubkey(key_file: &Path) -> Result<(), String> {
    let seed = read_seed_hex(key_file)?;
    let public_key_hex = public_key_hex_from_seed(&seed);
    let line = serde_json::json!({ "public_key_hex": public_key_hex });
    println!("{line}");
    Ok(())
}

struct FlagMap {
    args: Vec<String>,
}

impl FlagMap {
    fn new(args: Vec<String>) -> Self {
        Self { args }
    }

    fn take(&mut self, flag: &str) -> Result<Option<String>, String> {
        let mut i = 0;
        while i < self.args.len() {
            if self.args[i] == flag {
                if i + 1 >= self.args.len() {
                    return Err(format!("missing value for {flag}"));
                }
                let value = self.args[i + 1].clone();
                self.args.remove(i + 1);
                self.args.remove(i);
                return Ok(Some(value));
            }
            i += 1;
        }
        Ok(None)
    }

    fn rest(&self) -> &[String] {
        &self.args
    }
}

fn run_make_order(mut flags: FlagMap) -> Result<(), String> {
    let key_file = flags
        .take("--key-file")?
        .map(PathBuf::from)
        .ok_or("missing --key-file")?;
    let contract_id = flags
        .take("--contract-id")?
        .ok_or("missing --contract-id")?;
    let account_id = flags.take("--account-id")?.ok_or("missing --account-id")?;
    let base_token = flags.take("--base-token")?.ok_or("missing --base-token")?;
    let quote_token = flags
        .take("--quote-token")?
        .ok_or("missing --quote-token")?;
    let lot_size_base = flags
        .take("--lot-size-base")?
        .ok_or("missing --lot-size-base")?;
    let quote_per_tick_lot = flags
        .take("--quote-per-tick-lot")?
        .ok_or("missing --quote-per-tick-lot")?;
    let side = flags.take("--side")?.ok_or("missing --side")?;
    let price_ticks = flags
        .take("--price-ticks")?
        .ok_or("missing --price-ticks")?;
    let qty_lots = flags.take("--qty-lots")?.ok_or("missing --qty-lots")?;
    let nonce = flags.take("--nonce")?.ok_or("missing --nonce")?;
    let expiry_ns = flags.take("--expiry-ns")?;
    let expires_in_secs = flags.take("--expires-in-secs")?;
    if !flags.rest().is_empty() {
        return Err(format!("unexpected argument: {}", flags.rest()[0]));
    }
    order::cmd_make_order(&order::MakeOrderArgs {
        key_file: &key_file,
        contract_id: &contract_id,
        account_id: &account_id,
        base_token: &base_token,
        quote_token: &quote_token,
        lot_size_base: &lot_size_base,
        quote_per_tick_lot: &quote_per_tick_lot,
        side: &side,
        price_ticks: &price_ticks,
        qty_lots: &qty_lots,
        nonce: &nonce,
        expiry_ns: expiry_ns.as_deref(),
        expires_in_secs: expires_in_secs.as_deref(),
    })
}

fn run_settle_args(mut flags: FlagMap) -> Result<(), String> {
    let buy_file = flags
        .take("--buy-file")?
        .map(PathBuf::from)
        .ok_or("missing --buy-file")?;
    let sell_file = flags
        .take("--sell-file")?
        .map(PathBuf::from)
        .ok_or("missing --sell-file")?;
    let fill_lots = flags.take("--fill-lots")?.ok_or("missing --fill-lots")?;
    let price_ticks = flags
        .take("--price-ticks")?
        .ok_or("missing --price-ticks")?;
    if !flags.rest().is_empty() {
        return Err(format!("unexpected argument: {}", flags.rest()[0]));
    }
    order::cmd_settle_args(&buy_file, &sell_file, &fill_lots, &price_ticks)
}

fn run_verify_order(mut flags: FlagMap) -> Result<bool, String> {
    let signed_order_file = flags
        .take("--signed-order-file")?
        .map(PathBuf::from)
        .ok_or("missing --signed-order-file")?;
    let public_key_hex = flags.take("--public-key-hex")?;
    let key_file = flags.take("--key-file")?.map(PathBuf::from);
    if !flags.rest().is_empty() {
        return Err(format!("unexpected argument: {}", flags.rest()[0]));
    }
    order::cmd_verify_order(
        &signed_order_file,
        public_key_hex.as_deref(),
        key_file.as_deref(),
    )
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        exit_usage();
    }
    match args[1].as_str() {
        "keygen" => {
            let mut name: Option<String> = None;
            let mut out: Option<PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--name" => {
                        i += 1;
                        if i >= args.len() {
                            exit_usage();
                        }
                        name = Some(args[i].clone());
                    }
                    "--out" => {
                        i += 1;
                        if i >= args.len() {
                            exit_usage();
                        }
                        out = Some(PathBuf::from(&args[i]));
                    }
                    _ => exit_usage(),
                }
                i += 1;
            }
            let name = name.unwrap_or_else(|| exit_usage());
            let out = out.unwrap_or_else(|| exit_usage());
            if let Err(e) = cmd_keygen(&name, &out) {
                exit_err(&e);
            }
        }
        "pubkey" => {
            let mut key_file: Option<PathBuf> = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--key-file" => {
                        i += 1;
                        if i >= args.len() {
                            exit_usage();
                        }
                        key_file = Some(PathBuf::from(&args[i]));
                    }
                    _ => exit_usage(),
                }
                i += 1;
            }
            let key_file = key_file.unwrap_or_else(|| exit_usage());
            if let Err(e) = cmd_pubkey(&key_file) {
                exit_err(&e);
            }
        }
        "make-order" => {
            let flags = FlagMap::new(args[2..].to_vec());
            if let Err(e) = run_make_order(flags) {
                exit_err(&e);
            }
        }
        "settle-args" => {
            let flags = FlagMap::new(args[2..].to_vec());
            if let Err(e) = run_settle_args(flags) {
                exit_err(&e);
            }
        }
        "verify-order" => {
            let flags = FlagMap::new(args[2..].to_vec());
            match run_verify_order(flags) {
                Ok(true) => {}
                Ok(false) => process::exit(2),
                Err(e) => exit_err(&e),
            }
        }
        _ => exit_usage(),
    }
}
