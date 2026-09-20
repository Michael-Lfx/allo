//! Deterministic reproduction bundle exported from observation traces.
//!
//! Provides self-contained JSON fixtures that capture the full turn execution
//! (prompt preview, timeline events, model calls, tools, error causes, gaps)
//! along with sanitized environment fingerprints, enabling offline inspection
//! and replay (`nomicore replay`).

use serde::{Deserialize, Serialize};

use crate::project::ProjectedTurn;

pub const REPRO_SCHEMA_VERSION: u32 = 1;

/// Self-contained reproduction fixture exported from observation trace.
/// Can be shared with developers or fed into replay tools.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReproBundle {
    pub schema_version: u32,
    pub generated_at_ms: u64,
    pub environment: ReproEnvironment,
    pub turn: ProjectedTurn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReproEnvironment {
    pub os: String,
    pub arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

impl ReproBundle {
    pub fn from_projected_turn(turn: &ProjectedTurn) -> Self {
        Self {
            schema_version: REPRO_SCHEMA_VERSION,
            generated_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            environment: ReproEnvironment {
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
                app_version: option_env!("CARGO_PKG_VERSION").map(String::from),
            },
            turn: turn.clone(),
        }
    }

    /// Serializes the repro bundle to formatted JSON with secrets and API keys
    /// sanitized via `nomi-redact`.
    pub fn to_sanitized_json(&self) -> Result<String, serde_json::Error> {
        let raw = serde_json::to_string_pretty(self)?;
        Ok(nomi_redact::redact_secrets_owned(raw))
    }

    /// Parses a reproduction bundle from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{ExecutionStatus, Integrity};

    fn dummy_turn() -> ProjectedTurn {
        ProjectedTurn {
            root_turn_id: "turn-test-123".to_string(),
            conversation_id: Some("conv-abc".to_string()),
            msg_id: Some("msg-xyz".to_string()),
            session_kind: Some("chat".to_string()),
            execution_id: None,
            step_id: None,
            execution_attempt_id: None,
            status: ExecutionStatus::Failed,
            integrity: Integrity::Complete,
            interrupted: false,
            error: Some("Authorization failed: Bearer sk-1234567890abcdef1234567890".to_string()),
            started_at_ms: Some(1000),
            ended_at_ms: Some(2000),
            elapsed_ms: Some(1000),
            prompt_preview: Some("Write code with api_key = 'sk-123456789012345678901234'".to_string()),
            prompt_preview_context_only: false,
            max_event_seq: 10,
            has_turn_start: true,
            has_turn_end: true,
            gap_count: 0,
            timeline: Vec::new(),
            model_calls: Vec::new(),
            gaps: Vec::new(),
        }
    }

    #[test]
    fn test_repro_bundle_creation_and_redaction() {
        let turn = dummy_turn();
        let bundle = ReproBundle::from_projected_turn(&turn);

        assert_eq!(bundle.schema_version, REPRO_SCHEMA_VERSION);
        assert_eq!(bundle.turn.root_turn_id, "turn-test-123");

        let json = bundle.to_sanitized_json().expect("sanitized json serialization");
        // Ensure sensitive secrets are redacted
        assert!(!json.contains("sk-1234567890abcdef1234567890"));
        assert!(json.contains("[REDACTED_SECRET]"));

        // Ensure round-trip deserialization succeeds
        let parsed = ReproBundle::from_json(&json).expect("deserialize repro bundle");
        assert_eq!(parsed.turn.root_turn_id, "turn-test-123");
    }

    #[test]
    fn test_repro_bundle_with_complex_escapes_and_special_characters() {
        let mut turn = dummy_turn();
        turn.error = Some(
            "Error: {\"msg\": \"Auth failed\", \"token\": \"Bearer secret_token_value_here_12345678\", \"path\": \"C:\\\\Users\\\\Admin\\\\file.rs\"}"
                .to_string(),
        );
        turn.prompt_preview = Some(
            "Prompt with \"quotes\", \n newlines, \t tabs and api_key=\"sk-1234567890abcdef1234567890\" inside"
                .to_string(),
        );

        let bundle = ReproBundle::from_projected_turn(&turn);
        let json = bundle
            .to_sanitized_json()
            .expect("sanitized json serialization with escapes");

        assert!(!json.contains("sk-1234567890abcdef1234567890"));
        assert!(!json.contains("secret_token_value_here_12345678"));
        assert!(json.contains("[REDACTED_SECRET]"));

        // Round-trip deserialization must remain 100% valid JSON
        let parsed = ReproBundle::from_json(&json).expect("deserialize complex repro bundle");
        assert!(parsed
            .turn
            .error
            .as_deref()
            .unwrap()
            .contains("[REDACTED_SECRET]"));
        assert!(parsed
            .turn
            .prompt_preview
            .as_deref()
            .unwrap()
            .contains("[REDACTED_SECRET]"));
    }
}
