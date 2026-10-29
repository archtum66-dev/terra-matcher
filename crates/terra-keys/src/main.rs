#![forbid(unsafe_code)]

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
}

fn exit_usage() -> ! {
    usage();
    process::exit(2);
}

fn exit_err(msg: &str) -> ! {
    eprintln!("{msg}");
    process::exit(2);
}

fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 32 {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn read_seed_hex(path: &Path) -> Result<[u8; 32], String> {
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
        _ => exit_usage(),
    }
}
