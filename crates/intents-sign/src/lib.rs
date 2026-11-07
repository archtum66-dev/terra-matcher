#![forbid(unsafe_code)]
//! Sign Terra settlement drafts for the NEAR Intents verifier (`intents.near`) with
//! NEP-413, and adjust a pair for the verifier's protocol fee.
//!
//! Checked against the live verifier on 2026-11-07: `simulate_intents` on mainnet
//! accepted the format and both signatures of a signed pair and stopped only at the
//! balances of the throwaway accounts; a pair with one corrupted signature was
//! rejected with `invalid signature`. Sources: NEP-413, NEP-461, and the verifier
//! source in `github.com/near/intents` (`crates/signatures/nep413`,
//! `contracts/defuse/core/src/intents/token_diff.rs`, `contracts/defuse/README.md`).

use std::collections::BTreeMap;

use ed25519_dalek::Signer;
pub use ed25519_dalek::SigningKey;
use rand_core::{OsRng, RngCore};
use serde_json::{json, Value};
use settlement::{nep413_message, Intent, IntentDraft, SettlementError, SettlementPair};
use sha2::{Digest, Sha256};

/// The verifier contract on NEAR mainnet (there is no testnet deployment).
pub const VERIFIER: &str = "intents.near";
/// NEP-461 prefix tag for NEP-413: 2^31 + 413.
pub const NEP413_TAG: u32 = 2_147_484_061;
/// Fees are expressed in pips: 1_000_000 pips = 100 %.
pub const PIPS_MAX: u128 = 1_000_000;
/// Magic prefix of versioned nonces (first 4 bytes of `sha256("versioned_nonce")`).
pub const NONCE_MAGIC: [u8; 4] = [0x56, 0x28, 0xf6, 0xc6];

/// SHA-256 of `borsh((NEP413_TAG, Nep413Payload { message, nonce, recipient, callback_url: None }))`.
/// The ed25519 signature is made over this hash.
pub fn nep413_prehash(message: &str, nonce: &[u8; 32], recipient: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(NEP413_TAG.to_le_bytes());
    h.update((message.len() as u32).to_le_bytes());
    h.update(message.as_bytes());
    h.update(nonce);
    h.update((recipient.len() as u32).to_le_bytes());
    h.update(recipient.as_bytes());
    h.update([0u8]); // callback_url: None
    h.finalize().into()
}

/// Versioned nonce V1: magic (4) | version 0 (1) | salt (4) | deadline in ns, little endian (8) | random (15).
/// The nonce deadline must not be earlier than the intent deadline.
pub fn nonce_v1(salt: [u8; 4], deadline_ns: u64, random: [u8; 15]) -> [u8; 32] {
    let mut n = [0u8; 32];
    n[..4].copy_from_slice(&NONCE_MAGIC);
    n[4] = 0;
    n[5..9].copy_from_slice(&salt);
    n[9..17].copy_from_slice(&deadline_ns.to_le_bytes());
    n[17..].copy_from_slice(&random);
    n
}

/// Parses the verifier's `current_salt` (8 hex characters).
pub fn parse_salt(hex: &str) -> Option<[u8; 4]> {
    let hex = hex.trim().trim_matches('"');
    if hex.len() != 8 {
        return None;
    }
    let mut out = [0u8; 4];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A fresh random key (operating system randomness).
pub fn random_key() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

/// 15 random bytes for a nonce.
pub fn random_nonce_tail() -> [u8; 15] {
    let mut b = [0u8; 15];
    OsRng.fill_bytes(&mut b);
    b
}

/// The implicit NEAR account of an ed25519 key: 64 lowercase hex characters.
/// The verifier knows the key of an implicit account without registration.
pub fn implicit_account(key: &SigningKey) -> String {
    key.verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Signs one draft as a NEP-413 `MultiPayload` for `intents.near`.
pub fn sign_nep413(key: &SigningKey, draft: &IntentDraft, nonce: [u8; 32]) -> Value {
    let message = nep413_message(draft).unwrap_or_default();
    let hash = nep413_prehash(&message, &nonce, VERIFIER);
    let signature = key.sign(&hash);
    json!({
        "standard": "nep413",
        "payload": {
            "recipient": VERIFIER,
            "nonce": base64(&nonce),
            "message": message,
        },
        "public_key": format!("ed25519:{}", base58(&key.verifying_key().to_bytes())),
        "signature": format!("ed25519:{}", base58(&signature.to_bytes())),
    })
}

/// `ceil(amount * fee_pips / 1_000_000)`, the verifier's fee on a token given away.
pub fn fee_ceil(amount: u128, fee_pips: u32) -> Option<u128> {
    let product = amount.checked_mul(u128::from(fee_pips))?;
    Some(product.div_ceil(PIPS_MAX))
}

/// The verifier takes its fee from every negative delta of a `token_diff`. For a batch
/// to balance, each receiver gets the giver's amount minus that fee. This turns a
/// zero-sum pair from `settlement::settle_fill` into the pair the verifier accepts.
pub fn apply_protocol_fee(
    pair: &SettlementPair,
    fee_pips: u32,
) -> Result<SettlementPair, SettlementError> {
    settlement::verify_pair(pair)?;
    let mut out = pair.clone();
    let given = given_amounts(pair)?;
    for draft in [&mut out.buyer_draft, &mut out.seller_draft] {
        for intent in &mut draft.intents {
            let Intent::TokenDiff { diff } = intent;
            for (token, amount) in diff.iter_mut() {
                let value: i128 =
                    amount
                        .parse()
                        .map_err(|_| SettlementError::InvalidIntentAmount {
                            token: token.clone(),
                        })?;
                if value > 0 {
                    let giver = *given.get(token).ok_or(SettlementError::UnbalancedPair {
                        token: token.clone(),
                        sum: value,
                    })?;
                    let fee = fee_ceil(giver, fee_pips).ok_or(SettlementError::AmountOverflow)?;
                    let received = giver
                        .checked_sub(fee)
                        .filter(|r| *r > 0)
                        .ok_or(SettlementError::AmountOverflow)?;
                    *amount = received.to_string();
                }
            }
        }
    }
    verify_pair_with_fee(&out, fee_pips)?;
    Ok(out)
}

/// Checks a pair the way the verifier's invariant does: per token, the positive deltas
/// plus the negative deltas net of the fee must sum to zero.
pub fn verify_pair_with_fee(pair: &SettlementPair, fee_pips: u32) -> Result<(), SettlementError> {
    let mut sums: BTreeMap<String, i128> = BTreeMap::new();
    for draft in [&pair.buyer_draft, &pair.seller_draft] {
        for intent in &draft.intents {
            let Intent::TokenDiff { diff } = intent;
            for (token, amount) in diff {
                let value: i128 =
                    amount
                        .parse()
                        .map_err(|_| SettlementError::InvalidIntentAmount {
                            token: token.clone(),
                        })?;
                let supply = if value < 0 {
                    let given = value.unsigned_abs();
                    let fee = fee_ceil(given, fee_pips).ok_or(SettlementError::AmountOverflow)?;
                    let net = given
                        .checked_sub(fee)
                        .ok_or(SettlementError::AmountOverflow)?;
                    -i128::try_from(net).map_err(|_| SettlementError::AmountExceedsI128)?
                } else {
                    value
                };
                let entry = sums.entry(token.clone()).or_insert(0);
                *entry = entry
                    .checked_add(supply)
                    .ok_or(SettlementError::AmountOverflow)?;
            }
        }
    }
    for (token, sum) in sums {
        if sum != 0 {
            return Err(SettlementError::UnbalancedPair { token, sum });
        }
    }
    Ok(())
}

fn given_amounts(pair: &SettlementPair) -> Result<BTreeMap<String, u128>, SettlementError> {
    let mut given = BTreeMap::new();
    for draft in [&pair.buyer_draft, &pair.seller_draft] {
        for intent in &draft.intents {
            let Intent::TokenDiff { diff } = intent;
            for (token, amount) in diff {
                let value: i128 =
                    amount
                        .parse()
                        .map_err(|_| SettlementError::InvalidIntentAmount {
                            token: token.clone(),
                        })?;
                if value < 0 {
                    given.insert(token.clone(), value.unsigned_abs());
                }
            }
        }
    }
    Ok(given)
}

const B58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Bitcoin-alphabet base58, as NEAR uses for keys and signatures.
pub fn base58(bytes: &[u8]) -> String {
    let zeros = bytes.iter().take_while(|b| **b == 0).count();
    let mut digits: Vec<u8> = Vec::new();
    for &byte in &bytes[zeros..] {
        let mut carry = u32::from(byte);
        for d in digits.iter_mut() {
            carry += u32::from(*d) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut out = String::with_capacity(zeros + digits.len());
    out.push_str(&"1".repeat(zeros));
    out.extend(digits.iter().rev().map(|d| B58[*d as usize] as char));
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    fn arr<const N: usize>(hex_str: &str) -> [u8; N] {
        hex::decode(hex_str).unwrap().try_into().unwrap()
    }

    /// Vector from `crates/signatures/nep413/src/lib.rs` in github.com/near/intents.
    #[test]
    fn nep413_prehash_matches_verifier_vector() {
        let pk = VerifyingKey::from_bytes(&arr::<32>(
            "e2e9cb7ac57cb46d4da1ce1d1cc2c33bdfe17407c517916b522724a8ea2c6c50",
        ))
        .unwrap();
        let sig = Signature::from_bytes(&arr::<64>(
            "e2ff6254871a3fec1853c167b42f0f14248c4cf7fef5452dc24d8dbdc5c4bf183ab707322b4d782d5f5a05571bae476c5f7ee41c473f3002e600865e46b75d0f",
        ));
        let hash = nep413_prehash("Hello, world!", &[0u8; 32], "intents.near");
        assert!(pk.verify(&hash, &sig).is_ok());
    }

    /// Example from `contracts/defuse/README.md` in github.com/near/intents.
    #[test]
    fn nonce_v1_matches_readme_example() {
        let nonce = nonce_v1(
            arr::<4>("252812b3"),
            2_280_047_743 * 1_000_000_000,
            arr::<15>("027015ec13dc11864973138fe6812f"),
        );
        assert_eq!(
            base64(&nonce),
            "Vij2xgAlKBKzADZykFdbpB8CcBXsE9wRhklzE4/mgS8="
        );
        assert_eq!(parse_salt("\"252812b3\""), Some(arr::<4>("252812b3")));
    }

    #[test]
    fn base58_known_values() {
        assert_eq!(base58(b"hello world"), "StV1DL6CwTryKyV");
        assert_eq!(base58(&[0, 0, 1]), "112");
        assert_eq!(base58(&[]), "");
    }

    fn pair() -> SettlementPair {
        let mut buyer = BTreeMap::new();
        buyer.insert(
            "nep141:wrap.near".to_string(),
            "1000000000000000000000000".to_string(),
        );
        buyer.insert("nep141:usdc".to_string(), "-3680000".to_string());
        let mut seller = BTreeMap::new();
        seller.insert(
            "nep141:wrap.near".to_string(),
            "-1000000000000000000000000".to_string(),
        );
        seller.insert("nep141:usdc".to_string(), "3680000".to_string());
        SettlementPair {
            fill_seq: 1,
            buyer: "b".to_string(),
            seller: "s".to_string(),
            buyer_draft: IntentDraft {
                signer_id: "b".to_string(),
                deadline: "2030-01-01T00:00:00Z".to_string(),
                intents: vec![Intent::TokenDiff { diff: buyer }],
            },
            seller_draft: IntentDraft {
                signer_id: "s".to_string(),
                deadline: "2030-01-01T00:00:00Z".to_string(),
                intents: vec![Intent::TokenDiff { diff: seller }],
            },
        }
    }

    #[test]
    fn protocol_fee_reduces_what_each_side_receives() {
        let p = pair();
        assert_eq!(apply_protocol_fee(&p, 0).unwrap(), p);
        assert!(
            verify_pair_with_fee(&p, 1).is_err(),
            "zero-sum pair fails once the fee is 1 pip"
        );
        let f = apply_protocol_fee(&p, 1).unwrap();
        let Intent::TokenDiff { diff: b } = &f.buyer_draft.intents[0];
        let Intent::TokenDiff { diff: s } = &f.seller_draft.intents[0];
        assert_eq!(b["nep141:wrap.near"], "999999000000000000000000");
        assert_eq!(s["nep141:usdc"], "3679996");
        assert_eq!(b["nep141:usdc"], "-3680000");
        assert_eq!(s["nep141:wrap.near"], "-1000000000000000000000000");
        assert_eq!(fee_ceil(3_680_000, 1), Some(4));
    }

    #[test]
    fn signed_payload_verifies_and_has_verifier_shape() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let mut draft = pair().seller_draft;
        draft.signer_id = implicit_account(&key);
        let nonce = nonce_v1([1, 2, 3, 4], 5, [9u8; 15]);
        let v = sign_nep413(&key, &draft, nonce);
        assert_eq!(v["standard"], "nep413");
        assert_eq!(v["payload"]["recipient"], "intents.near");
        let message = v["payload"]["message"].as_str().unwrap();
        let inner: Value = serde_json::from_str(message).unwrap();
        assert_eq!(inner["signer_id"], draft.signer_id);
        assert_eq!(inner["intents"][0]["intent"], "token_diff");
        assert_eq!(implicit_account(&key).len(), 64);
        let sig = key.sign(&nep413_prehash(message, &nonce, VERIFIER));
        assert_eq!(
            v["signature"],
            format!("ed25519:{}", base58(&sig.to_bytes()))
        );
        assert!(key
            .verifying_key()
            .verify(&nep413_prehash(message, &nonce, VERIFIER), &sig)
            .is_ok());
    }
}
