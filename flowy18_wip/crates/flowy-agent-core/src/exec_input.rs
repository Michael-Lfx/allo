//! The durable input handed to one execution attempt (V0).
//!
//! `ExecInput` carries the work identity, the attempt, the host-owned token and
//! configuration, and the untrusted context. The acceptance receipt is linked
//! one-way through `receipt_id`; the journal never grows host credentials. The
//! six invariants from the plan's §4 are asserted in the tests below.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::acceptance::AcceptedMessage;

/// One decision from the work's history.
///
/// The scope is the work it was decided for: a decision is never inherited by
/// another work or by a later attempt of the same work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub work_id: String,
    pub text: String,
}

/// Untrusted material for one attempt: the prompt, answers, pending questions
/// and the work-local decisions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecContext {
    pub inputs: Vec<String>,
    pub answers: Vec<String>,
    pub pending: Vec<String>,
    pub decided: Vec<Decision>,
}

/// One execution attempt's complete input.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecInput {
    pub work_id: String,
    pub revision: u64,
    pub attempt_id: String,
    pub token: String,
    pub mode: String,
    pub capabilities: Vec<String>,
    pub ask_first: Vec<String>,
    pub context: ExecContext,
    pub session_hint: Option<String>,
    /// One-way link back to the receipt that was accepted. Absent for inputs
    /// that were not derived from an acceptance (for example a retry created by
    /// the scheduler).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
}

impl fmt::Debug for ExecInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecInput")
            .field("work_id", &self.work_id)
            .field("revision", &self.revision)
            .field("attempt_id", &self.attempt_id)
            .field("token", &"<redacted>")
            .field("mode", &self.mode)
            .field("capabilities", &self.capabilities)
            .field("ask_first", &self.ask_first)
            .field("context", &self.context)
            .field("session_hint", &self.session_hint)
            .field("receipt_id", &self.receipt_id)
            .finish()
    }
}

/// Host-owned values assigned when an attempt is claimed.
///
/// They are deliberately separate from the receipt: the acceptance journal
/// records the user's text, never the token, capabilities or revision.
pub struct ExecAssignment<'a> {
    pub work_id: &'a str,
    pub revision: u64,
    pub attempt_id: &'a str,
    pub token: &'a str,
    pub mode: &'a str,
    pub capabilities: &'a [String],
    pub ask_first: &'a [String],
}

impl fmt::Debug for ExecAssignment<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecAssignment")
            .field("work_id", &self.work_id)
            .field("revision", &self.revision)
            .field("attempt_id", &self.attempt_id)
            .field("token", &"<redacted>")
            .field("mode", &self.mode)
            .field("capabilities", &self.capabilities)
            .field("ask_first", &self.ask_first)
            .finish()
    }
}

impl ExecInput {
    /// Derive the attempt input from an accepted receipt.
    ///
    /// The receipt contributes its id, text and optional session hint. The host
    /// contributes the work/attempt identity, token and configuration.
    /// `decided` is filtered to the receipt's work; a decision from another work
    /// is never inherited.
    #[must_use]
    pub fn from_receipt(
        receipt: &AcceptedMessage,
        assignment: ExecAssignment<'_>,
        decided: Vec<Decision>,
    ) -> Self {
        let decided = decided
            .into_iter()
            .filter(|decision| decision.work_id == assignment.work_id)
            .collect();
        Self {
            work_id: assignment.work_id.to_owned(),
            revision: assignment.revision,
            attempt_id: assignment.attempt_id.to_owned(),
            token: assignment.token.to_owned(),
            mode: assignment.mode.to_owned(),
            capabilities: assignment.capabilities.to_vec(),
            ask_first: assignment.ask_first.to_vec(),
            context: ExecContext {
                inputs: vec![receipt.text.clone()],
                answers: Vec::new(),
                pending: Vec::new(),
                decided,
            },
            session_hint: receipt.session_hint.clone(),
            receipt_id: Some(receipt.id.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(session_hint: Option<&str>) -> AcceptedMessage {
        AcceptedMessage {
            id: "accepted-1".to_owned(),
            session_hint: session_hint.map(str::to_owned),
            text: "ship the release notes".to_owned(),
            created_at_ms: 1_000,
            taken_at_ms: None,
            lease: None,
            completed_at_ms: None,
        }
    }

    fn assignment<'a>(
        token: &'a str,
        capabilities: &'a [String],
        ask_first: &'a [String],
    ) -> ExecAssignment<'a> {
        ExecAssignment {
            work_id: "work-1",
            revision: 3,
            attempt_id: "attempt-1",
            token,
            mode: "ask",
            capabilities,
            ask_first,
        }
    }

    fn input(token: &str) -> ExecInput {
        let capabilities = vec!["read".to_owned()];
        let ask_first = vec!["write".to_owned()];
        ExecInput::from_receipt(
            &receipt(Some("session-1")),
            assignment(token, &capabilities, &ask_first),
            Vec::new(),
        )
    }

    #[test]
    fn token_is_scoped_to_one_attempt_and_never_enters_the_receipt() {
        let first = input("token-secret");
        let second = input("other-token");
        assert_ne!(first.token, second.token);
        let debug = format!("{first:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("token-secret"));
        let receipt_json = serde_json::to_value(receipt(Some("session-1")))
            .expect("receipt json");
        assert!(receipt_json.get("token").is_none());
        assert!(receipt_json.get("capabilities").is_none());
        assert!(receipt_json.get("revision").is_none());
    }

    #[test]
    fn session_hint_remains_optional() {
        let capabilities = vec!["read".to_owned()];
        let ask_first = Vec::new();
        let with_hint = ExecInput::from_receipt(
            &receipt(Some("session-1")),
            assignment("token-secret", &capabilities, &ask_first),
            Vec::new(),
        );
        let without_hint = ExecInput::from_receipt(
            &receipt(None),
            assignment("token-secret", &capabilities, &ask_first),
            Vec::new(),
        );
        assert_eq!(with_hint.session_hint.as_deref(), Some("session-1"));
        assert!(without_hint.session_hint.is_none());
    }

    #[test]
    fn context_carries_the_untrusted_prompt_not_host_config() {
        let input = input("token-secret");
        assert_eq!(
            input.context.inputs,
            vec!["ship the release notes".to_owned()]
        );
        assert_eq!(input.context.answers, Vec::<String>::new());
        assert_eq!(input.context.pending, Vec::<String>::new());
        let context_json =
            serde_json::to_string(&input.context).expect("context json");
        assert!(!context_json.contains("token-secret"));
        assert!(!context_json.contains("read"));
    }

    #[test]
    fn same_token_and_content_round_trips_byte_for_byte() {
        let input = input("token-secret");
        let first = serde_json::to_vec(&input).expect("serialize");
        let decoded: ExecInput =
            serde_json::from_slice(&first).expect("deserialize");
        let second = serde_json::to_vec(&decoded).expect("serialize again");
        assert_eq!(first, second);
        assert_eq!(decoded, input);
    }

    #[test]
    fn attempt_id_keeps_retries_distinct() {
        let input = input("token-secret");
        let retry = ExecInput::from_receipt(
            &receipt(Some("session-1")),
            ExecAssignment {
                work_id: "work-1",
                revision: 3,
                attempt_id: "attempt-2",
                token: "token-secret-2",
                mode: "ask",
                capabilities: input.capabilities.as_slice(),
                ask_first: input.ask_first.as_slice(),
            },
            Vec::new(),
        );
        assert_ne!(input.attempt_id, retry.attempt_id);
        assert_ne!(input.token, retry.token);
        assert_eq!(input.work_id, retry.work_id);
        assert_eq!(input.revision, retry.revision);
    }

    #[test]
    fn decided_is_scoped_to_its_work() {
        let capabilities = vec!["read".to_owned()];
        let ask_first = Vec::new();
        let input = ExecInput::from_receipt(
            &receipt(Some("session-1")),
            assignment("token-secret", &capabilities, &ask_first),
            vec![
                Decision {
                    work_id: "work-1".to_owned(),
                    text: "use the short title".to_owned(),
                },
                Decision {
                    work_id: "work-2".to_owned(),
                    text: "must not leak".to_owned(),
                },
            ],
        );
        assert_eq!(input.context.decided.len(), 1);
        assert_eq!(input.context.decided[0].work_id, "work-1");
        assert_eq!(input.receipt_id.as_deref(), Some("accepted-1"));
    }
}
