use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::model::Platform;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Payload {
    Heartbeat {
        host_id: String,
        host_name: String,
        platform: Platform,
        channel: u8,
        ready: bool,
        keyboard_connected: bool,
        current_channel: Option<u8>,
        sent_at_ms: i64,
    },
    Claim {
        host_id: String,
        channel: u8,
        nonce: String,
        sent_at_ms: i64,
    },
    SwitchDispatched {
        host_id: String,
        claim_nonce: String,
        target_channel: u8,
        accepted: bool,
        detail: String,
        sent_at_ms: i64,
    },
    ArrivalConfirmed {
        host_id: String,
        claim_nonce: String,
        channel: u8,
        sent_at_ms: i64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub payload: Payload,
    pub signature: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("消息序列化失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("消息签名无效")]
    InvalidSignature,
    #[error("配对码不可用")]
    InvalidKey,
}

type HmacSha256 = Hmac<Sha256>;

fn signature(payload: &Payload, pairing_code: &str) -> Result<String, ProtocolError> {
    let bytes = serde_json::to_vec(payload)?;
    let mut mac = HmacSha256::new_from_slice(pairing_code.as_bytes())
        .map_err(|_| ProtocolError::InvalidKey)?;
    mac.update(&bytes);
    Ok(STANDARD_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub fn encode(payload: Payload, pairing_code: &str) -> Result<Vec<u8>, ProtocolError> {
    let envelope = Envelope {
        signature: signature(&payload, pairing_code)?,
        payload,
    };
    Ok(serde_json::to_vec(&envelope)?)
}

pub fn decode(bytes: &[u8], pairing_code: &str) -> Result<Payload, ProtocolError> {
    let envelope: Envelope = serde_json::from_slice(bytes)?;
    let expected = signature(&envelope.payload, pairing_code)?;
    if envelope.signature != expected {
        return Err(ProtocolError::InvalidSignature);
    }
    Ok(envelope.payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Payload {
        Payload::Claim {
            host_id: "mac".into(),
            channel: 3,
            nonce: "n1".into(),
            sent_at_ms: 42,
        }
    }

    #[test]
    fn signed_message_round_trips() {
        let encoded = encode(sample(), "secret").unwrap();
        let decoded = decode(&encoded, "secret").unwrap();
        assert!(matches!(decoded, Payload::Claim { channel: 3, .. }));
    }

    #[test]
    fn wrong_pairing_code_is_rejected() {
        let encoded = encode(sample(), "secret").unwrap();
        assert!(matches!(
            decode(&encoded, "other"),
            Err(ProtocolError::InvalidSignature)
        ));
    }

    #[test]
    fn arrival_confirmation_round_trips() {
        let payload = Payload::ArrivalConfirmed {
            host_id: "mac".into(),
            claim_nonce: "claim-1".into(),
            channel: 3,
            sent_at_ms: 42,
        };
        let encoded = encode(payload, "secret").unwrap();
        assert!(matches!(
            decode(&encoded, "secret").unwrap(),
            Payload::ArrivalConfirmed { channel: 3, .. }
        ));
    }
}
