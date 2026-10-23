use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeKind {
    #[serde(rename = "confirmation.pending")]
    ConfirmationPending,
    #[serde(rename = "turn.completed")]
    TurnCompleted,
    #[serde(rename = "turn.failed")]
    TurnFailed,
}

/// Lock-screen wake. Unknown fields are rejected so message text cannot ride along.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakePayload {
    pub v: u32,
    pub kind: WakeKind,
    pub host_fingerprint: String,
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation_id: Option<String>,
    pub sent_at_ms: i64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WakePayloadError {
    #[error("wake payload is not the locked schema")]
    Schema,
    #[error("wake payload version {0} is not supported")]
    Version(u32),
    #[error("wake fingerprint is invalid")]
    Fingerprint,
}

pub fn parse_wake_payload(bytes: &[u8]) -> Result<WakePayload, WakePayloadError> {
    let payload: WakePayload = serde_json::from_slice(bytes).map_err(|_| WakePayloadError::Schema)?;
    if payload.v != 1 {
        return Err(WakePayloadError::Version(payload.v));
    }
    if payload.host_fingerprint.len() != 64
        || !payload.host_fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(WakePayloadError::Fingerprint);
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> &'static str {
        r#"{"v":1,"kind":"turn.completed","hostFingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","conversationId":"0190f5fe-7c00-7a00-8000-000000000001","sentAtMs":1}"#
    }

    #[test]
    fn accepts_the_locked_fields() {
        assert!(parse_wake_payload(sample().as_bytes()).is_ok());
    }

    #[test]
    fn rejects_message_text() {
        let raw = sample().replace(
            "}",
            r#","text":"hello"}"#,
        );
        assert_eq!(parse_wake_payload(raw.as_bytes()), Err(WakePayloadError::Schema));
    }
}
