use serde_json::json;

use crate::intent::IntentDraft;

/// Builds the NEP-413 wallet message JSON string.
///
/// UNVERIFIED against a live verifier.
pub fn nep413_message(draft: &IntentDraft) -> Result<String, serde_json::Error> {
    let value = json!({
        "signer_id": draft.signer_id,
        "deadline": draft.deadline,
        "intents": draft.intents,
    });
    serde_json::to_string(&value)
}

/// Builds the raw_ed25519 payload JSON string.
///
/// UNVERIFIED against a live verifier.
pub fn raw_payload(
    draft: &IntentDraft,
    verifying_contract: &str,
    nonce_b64: &str,
) -> Result<String, serde_json::Error> {
    let value = json!({
        "signer_id": draft.signer_id,
        "verifying_contract": verifying_contract,
        "deadline": draft.deadline,
        "nonce": nonce_b64,
        "intents": draft.intents,
    });
    serde_json::to_string(&value)
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes 32 bytes (4-byte salt + 28-byte unique) as standard Base64 with padding.
pub fn nonce_b64(salt: [u8; 4], unique: [u8; 28]) -> String {
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&salt);
    bytes[4..].copy_from_slice(&unique);
    encode_base64(&bytes)
}

fn encode_base64(input: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i];
        let b1 = if i + 1 < input.len() { input[i + 1] } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] } else { 0 };

        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);

        out.push(BASE64_ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(BASE64_ALPHABET[((n >> 12) & 63) as usize] as char);
        if i + 1 < input.len() {
            out.push(BASE64_ALPHABET[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(BASE64_ALPHABET[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeMap;

    use crate::intent::{Intent, IntentDraft};

    #[test]
    fn nonce_b64_matches_known_vectors() {
        let salt = [0u8; 4];
        let unique = [0u8; 28];
        assert_eq!(
            nonce_b64(salt, unique),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );

        let salt2 = [0, 0, 0, 1];
        let mut unique2 = [0u8; 28];
        for (i, b) in unique2.iter_mut().enumerate() {
            *b = (i + 1) as u8;
        }
        assert_eq!(
            nonce_b64(salt2, unique2),
            "AAAAAQECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxw="
        );
    }

    #[test]
    fn nonce_b64_length_and_padding() {
        let encoded = nonce_b64([0u8; 4], [0u8; 28]);
        assert_eq!(encoded.len(), 44);
        assert!(encoded.ends_with('='));
    }

    #[test]
    fn nep413_message_contains_fields() {
        let draft = IntentDraft {
            signer_id: "alice.test".to_string(),
            deadline: "2030-01-01T00:00:00Z".to_string(),
            intents: vec![Intent::TokenDiff {
                diff: BTreeMap::new(),
            }],
        };
        let msg = nep413_message(&draft).expect("message");
        let v: Value = serde_json::from_str(&msg).expect("json");
        assert_eq!(v["signer_id"], "alice.test");
        assert!(v.get("intents").is_some());
    }

    #[test]
    fn raw_payload_contains_contract_and_nonce() {
        let draft = IntentDraft {
            signer_id: "alice.test".to_string(),
            deadline: "2030-01-01T00:00:00Z".to_string(),
            intents: vec![],
        };
        let payload = raw_payload(&draft, "intents.near", "bm9uY2U=").expect("payload");
        let v: Value = serde_json::from_str(&payload).expect("json");
        assert_eq!(v["verifying_contract"], "intents.near");
        assert_eq!(v["nonce"], "bm9uY2U=");
        assert_eq!(v["signer_id"], "alice.test");
    }
}
