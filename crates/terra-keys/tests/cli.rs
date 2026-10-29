//! Integration tests for the `terra-keys` binary.
use ed25519_dalek::SigningKey;
use std::process::Command;
use std::{fs, path::PathBuf};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_terra-keys"))
}

fn temp_dir(tag: &str) -> PathBuf {
    // one directory per test: the tests run in parallel
    let dir = std::env::temp_dir().join(format!("terra-keys-test-{}-{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
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
