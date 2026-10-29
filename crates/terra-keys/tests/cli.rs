//! Integration tests for the `terra-keys` binary.
use ed25519_dalek::{SigningKey, Verifier, VerifyingKey};
use escrow_types::{order_hash, order_hash_hex, Order, Side, SignedOrder};
use std::process::Command;
use std::{fs, path::PathBuf};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_terra-keys"))
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("terra-keys-test-{}-{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

fn write_seed(dir: &PathBuf, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("{}\n", "07".repeat(32))).expect("write seed");
    path
}

fn sample_make_order_args(key_file: &str) -> Vec<String> {
    vec![
        "make-order".to_string(),
        "--key-file".to_string(),
        key_file.to_string(),
        "--contract-id".to_string(),
        "escrow.example.testnet".to_string(),
        "--account-id".to_string(),
        "alice.testnet".to_string(),
        "--base-token".to_string(),
        "base.testnet".to_string(),
        "--quote-token".to_string(),
        "quote.testnet".to_string(),
        "--lot-size-base".to_string(),
        "1000000".to_string(),
        "--quote-per-tick-lot".to_string(),
        "1000".to_string(),
        "--side".to_string(),
        "buy".to_string(),
        "--price-ticks".to_string(),
        "100".to_string(),
        "--qty-lots".to_string(),
        "5".to_string(),
        "--nonce".to_string(),
        "42".to_string(),
        "--expiry-ns".to_string(),
        "1793000000000000000".to_string(),
    ]
}

#[test]
fn keygen_creates_file_and_public_hex() {
    let dir = temp_dir("keygen_creates_file_and_public_hex");
    let out = Command::new(bin())
        .args(["keygen", "--name", "alice", "--out", dir.to_str().unwrap()])
        .output()
        .expect("run keygen");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains('\n') || stdout.lines().count() == 1);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).expect("parse json");
    let pk = json["public_key_hex"].as_str().expect("public_key_hex");
    assert_eq!(pk.len(), 64);
    assert!(pk.chars().all(|c| c.is_ascii_hexdigit()));
    let key_path = dir.join("alice.order-key");
    let contents = fs::read_to_string(&key_path).expect("read key file");
    assert_eq!(contents.trim().len(), 64);
    assert!(contents
        .trim()
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert!(!stdout
        .to_lowercase()
        .contains(&contents.trim().to_lowercase()));
    cleanup(&dir);
}

#[test]
fn pubkey_matches_keygen() {
    let dir = temp_dir("pubkey_matches_keygen");
    let out1 = Command::new(bin())
        .args(["keygen", "--name", "bob", "--out", dir.to_str().unwrap()])
        .output()
        .expect("run keygen");
    assert!(out1.status.success());
    let gen_json: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out1.stdout).trim()).unwrap();
    let pk1 = gen_json["public_key_hex"].as_str().unwrap();
    let key_path = dir.join("bob.order-key");
    let out2 = Command::new(bin())
        .args(["pubkey", "--key-file", key_path.to_str().unwrap()])
        .output()
        .expect("run pubkey");
    assert!(out2.status.success());
    let pub_json: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out2.stdout).trim()).unwrap();
    assert_eq!(pub_json["public_key_hex"].as_str().unwrap(), pk1);
    cleanup(&dir);
}

#[test]
fn pubkey_from_known_seed() {
    let dir = temp_dir("pubkey_from_known_seed");
    let key_path = dir.join("fixed.order-key");
    fs::write(&key_path, format!("{}\n", "07".repeat(32))).expect("write seed");
    let expected = hex::encode(
        SigningKey::from_bytes(&[7u8; 32])
            .verifying_key()
            .to_bytes(),
    );
    let out = Command::new(bin())
        .args(["pubkey", "--key-file", key_path.to_str().unwrap()])
        .output()
        .expect("run pubkey");
    assert!(out.status.success());
    let json: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    assert_eq!(json["public_key_hex"].as_str().unwrap(), expected);
    cleanup(&dir);
}

#[test]
fn keygen_refuses_overwrite() {
    let dir = temp_dir("keygen_refuses_overwrite");
    let dir_str = dir.to_str().unwrap();
    assert!(Command::new(bin())
        .args(["keygen", "--name", "dup", "--out", dir_str])
        .status()
        .unwrap()
        .success());
    let out = Command::new(bin())
        .args(["keygen", "--name", "dup", "--out", dir_str])
        .output()
        .expect("run keygen again");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("refusing to overwrite"));
    cleanup(&dir);
}

#[test]
fn keygen_rejects_invalid_name() {
    let dir = temp_dir("keygen_rejects_invalid_name");
    let out = Command::new(bin())
        .args([
            "keygen",
            "--name",
            "Bad Name!",
            "--out",
            dir.to_str().unwrap(),
        ])
        .output()
        .expect("run keygen");
    assert_eq!(out.status.code(), Some(2));
    cleanup(&dir);
}

#[test]
fn make_order_hash_and_signature() {
    let dir = temp_dir("make_order_hash_and_signature");
    let key_path = write_seed(&dir, "k.order-key");
    let args: Vec<&str> = sample_make_order_args(key_path.to_str().unwrap())
        .iter()
        .map(String::as_str)
        .collect();
    let out = Command::new(bin()).args(args).output().expect("run make-order");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    let order: Order =
        serde_json::from_value(line["signed_order"]["order"].clone()).unwrap();
    let expected_hash = order_hash_hex(&order).unwrap();
    assert_eq!(line["order_hash"].as_str().unwrap(), expected_hash);
    let hash = order_hash(&order).unwrap();
    let sig_hex = line["signed_order"]["signature"].as_str().unwrap();
    let sig_bytes = hex::decode(sig_hex).unwrap();
    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes.try_into().unwrap());
    let vk = VerifyingKey::from_bytes(
        &hex::decode(line["public_key_hex"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap(),
    )
    .unwrap();
    vk.verify(&hash, &sig).unwrap();
    cleanup(&dir);
}

#[test]
fn make_order_deterministic_signature() {
    let dir = temp_dir("make_order_deterministic_signature");
    let key_path = write_seed(&dir, "k.order-key");
    let args: Vec<&str> = sample_make_order_args(key_path.to_str().unwrap())
        .iter()
        .map(String::as_str)
        .collect();
    let out1 = Command::new(bin()).args(&args).output().unwrap();
    let out2 = Command::new(bin()).args(&args).output().unwrap();
    assert!(out1.status.success() && out2.status.success());
    let j1: serde_json::Value = serde_json::from_str(String::from_utf8_lossy(&out1.stdout).trim()).unwrap();
    let j2: serde_json::Value = serde_json::from_str(String::from_utf8_lossy(&out2.stdout).trim()).unwrap();
    assert_eq!(
        j1["signed_order"]["signature"],
        j2["signed_order"]["signature"]
    );
    cleanup(&dir);
}

#[test]
fn verify_order_valid_and_tampered() {
    let dir = temp_dir("verify_order_valid_and_tampered");
    let key_path = write_seed(&dir, "k.order-key");
    let args: Vec<&str> = sample_make_order_args(key_path.to_str().unwrap())
        .iter()
        .map(String::as_str)
        .collect();
    let out = Command::new(bin()).args(args).output().unwrap();
    assert!(out.status.success());
    let order_path = dir.join("order.json");
    fs::write(&order_path, String::from_utf8_lossy(&out.stdout)).unwrap();
    let verify = Command::new(bin())
        .args([
            "verify-order",
            "--signed-order-file",
            order_path.to_str().unwrap(),
            "--key-file",
            key_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(verify.status.success());
    let vj: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&verify.stdout).trim()).unwrap();
    assert_eq!(vj["valid"], true);

    let mut tampered: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    tampered["signed_order"]["order"]["nonce"] = serde_json::Value::String("99".to_string());
    fs::write(&order_path, tampered.to_string()).unwrap();
    let bad = Command::new(bin())
        .args([
            "verify-order",
            "--signed-order-file",
            order_path.to_str().unwrap(),
            "--key-file",
            key_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let bj: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&bad.stdout).trim()).unwrap();
    assert_eq!(bj["valid"], false);
    cleanup(&dir);
}

fn write_pair(dir: &PathBuf) -> (PathBuf, PathBuf) {
    let alice_key = write_seed(dir, "alice.order-key");
    let bob_key = write_seed(dir, "bob.order-key");
    let buy_args = vec![
        "make-order",
        "--key-file",
        alice_key.to_str().unwrap(),
        "--contract-id",
        "escrow.example.testnet",
        "--account-id",
        "alice.testnet",
        "--base-token",
        "base.testnet",
        "--quote-token",
        "quote.testnet",
        "--lot-size-base",
        "1000000",
        "--quote-per-tick-lot",
        "1000",
        "--side",
        "buy",
        "--price-ticks",
        "100",
        "--qty-lots",
        "5",
        "--nonce",
        "1",
        "--expiry-ns",
        "1793000000000000000",
    ];
    let sell_args = vec![
        "make-order",
        "--key-file",
        bob_key.to_str().unwrap(),
        "--contract-id",
        "escrow.example.testnet",
        "--account-id",
        "bob.testnet",
        "--base-token",
        "base.testnet",
        "--quote-token",
        "quote.testnet",
        "--lot-size-base",
        "1000000",
        "--quote-per-tick-lot",
        "1000",
        "--side",
        "sell",
        "--price-ticks",
        "90",
        "--qty-lots",
        "5",
        "--nonce",
        "2",
        "--expiry-ns",
        "1793000000000000000",
    ];
    let buy_path = dir.join("buy.json");
    let sell_path = dir.join("sell.json");
    let buy_out = Command::new(bin()).args(buy_args).output().unwrap();
    let sell_out = Command::new(bin()).args(sell_args).output().unwrap();
    assert!(buy_out.status.success() && sell_out.status.success());
    fs::write(&buy_path, String::from_utf8_lossy(&buy_out.stdout)).unwrap();
    fs::write(&sell_path, String::from_utf8_lossy(&sell_out.stdout)).unwrap();
    (buy_path, sell_path)
}

#[test]
fn settle_args_happy_path() {
    let dir = temp_dir("settle_args_happy_path");
    let (buy_path, sell_path) = write_pair(&dir);
    let out = Command::new(bin())
        .args([
            "settle-args",
            "--buy-file",
            buy_path.to_str().unwrap(),
            "--sell-file",
            sell_path.to_str().unwrap(),
            "--fill-lots",
            "3",
            "--price-ticks",
            "95",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let line: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    let _buy: SignedOrder = serde_json::from_value(line["buy"].clone()).unwrap();
    let _sell: SignedOrder = serde_json::from_value(line["sell"].clone()).unwrap();
    assert_eq!(line["fill_lots"], 3);
    assert_eq!(line["price_ticks"], 95);
    cleanup(&dir);
}

#[test]
fn settle_args_rejects_invalid_pairs() {
    let dir = temp_dir("settle_args_rejects_invalid_pairs");
    let (buy_path, sell_path) = write_pair(&dir);

    let run = |extra_price: &str| -> i32 {
        Command::new(bin())
            .args([
                "settle-args",
                "--buy-file",
                buy_path.to_str().unwrap(),
                "--sell-file",
                sell_path.to_str().unwrap(),
                "--fill-lots",
                "1",
                "--price-ticks",
                extra_price,
            ])
            .output()
            .unwrap()
            .status
            .code()
            .unwrap_or(-1)
    };
    assert_eq!(run("101"), 2);
    assert_eq!(run("89"), 2);

    let sell_only = dir.join("sell_only.json");
    fs::write(&sell_only, fs::read_to_string(&sell_path).unwrap()).unwrap();
    assert_eq!(
        Command::new(bin())
            .args([
                "settle-args",
                "--buy-file",
                sell_only.to_str().unwrap(),
                "--sell-file",
                sell_only.to_str().unwrap(),
                "--fill-lots",
                "1",
                "--price-ticks",
                "95",
            ])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );

    let buy_raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&buy_path).unwrap()).unwrap();
    let mut bob_buy = buy_raw.clone();
    bob_buy["signed_order"]["order"]["account_id"] =
        serde_json::Value::String("bob.testnet".to_string());
    let bob_buy_path = dir.join("bob_buy.json");
    fs::write(&bob_buy_path, bob_buy.to_string()).unwrap();
    assert_eq!(
        Command::new(bin())
            .args([
                "settle-args",
                "--buy-file",
                bob_buy_path.to_str().unwrap(),
                "--sell-file",
                sell_path.to_str().unwrap(),
                "--fill-lots",
                "1",
                "--price-ticks",
                "95",
            ])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );

    assert_eq!(
        Command::new(bin())
            .args([
                "settle-args",
                "--buy-file",
                buy_path.to_str().unwrap(),
                "--sell-file",
                sell_path.to_str().unwrap(),
                "--fill-lots",
                "6",
                "--price-ticks",
                "95",
            ])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
    cleanup(&dir);
}

#[test]
fn make_order_rejects_zero_price() {
    let dir = temp_dir("make_order_rejects_zero_price");
    let key_path = write_seed(&dir, "k.order-key");
    let mut args = sample_make_order_args(key_path.to_str().unwrap());
    for i in 0..args.len() {
        if args[i] == "--price-ticks" {
            args[i + 1] = "0".to_string();
            break;
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = Command::new(bin()).args(arg_refs).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    cleanup(&dir);
}

#[test]
fn stdout_never_contains_seed_hex() {
    let dir = temp_dir("stdout_never_contains_seed_hex");
    let seed_hex = "07".repeat(32);
    let key_path = write_seed(&dir, "k.order-key");
    let commands: Vec<Vec<String>> = vec![
        vec![
            "pubkey".into(),
            "--key-file".into(),
            key_path.to_str().unwrap().into(),
        ],
        sample_make_order_args(key_path.to_str().unwrap()),
    ];
    for cmd in commands {
        let arg_refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
        let out = Command::new(bin()).args(arg_refs).output().unwrap();
        assert!(out.status.success());
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !combined.to_lowercase().contains(&seed_hex.to_lowercase()),
            "seed leaked in output"
        );
    }
    cleanup(&dir);
}

#[test]
fn make_order_expires_in_secs() {
    let dir = temp_dir("make_order_expires_in_secs");
    let key_path = write_seed(&dir, "k.order-key");
    let mut args = sample_make_order_args(key_path.to_str().unwrap());
    let mut filtered: Vec<String> = Vec::new();
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--expiry-ns" {
            skip_next = true;
            continue;
        }
        filtered.push(a);
    }
    filtered.push("--expires-in-secs".to_string());
    filtered.push("3600".to_string());
    let arg_refs: Vec<&str> = filtered.iter().map(String::as_str).collect();
    let out = Command::new(bin()).args(arg_refs).output().unwrap();
    assert!(out.status.success());
    let line: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    let expiry: u64 = line["signed_order"]["order"]["expiry_ns"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let two_hours = 2_u64 * 3600 * 1_000_000_000;
    assert!(expiry > now_ns);
    assert!(expiry < now_ns + two_hours);
    cleanup(&dir);
}
