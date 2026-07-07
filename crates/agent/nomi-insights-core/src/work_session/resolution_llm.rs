//! LLM-based session resolution labeling via the auxiliary client.

use std::time::Duration;

use nomi_auxiliary::{AuxiliaryClient, AuxiliaryRequest, AuxiliaryTask, text_message};
use nomi_types::message::Role;
use crate::types::{ResolutionPayload, validate_signal_codes, ALLOWED_SIGNAL_CODES};
use nomi_poi::{is_poi_synthetic_user_text, message_text_from_value};
use serde::Deserialize;
use tracing::{debug, warn};

use super::resolution::SessionSignals;

const RESOLUTION_LLM_TASK: &str = "resolution";
const MAX_TRANSCRIPT_CHARS: usize = 16_000;
const MAX_MESSAGE_CHARS: usize = 2_000;

const ALLOWED_VERDICTS: &[&str] = &[
    "solved_confirmed",
    "solved_inferred",
    "partial",
    "unresolved",
    "failed",
    "abandoned",
];

const ALLOWED_CONFIDENCE: &[&str] = &["high", "medium", "low"];
const ALLOWED_TIERS: &[&str] = &["A", "B", "C", "D"];
const ALLOWED_FEEDBACK: &[&str] = &[
    "explicit_positive",
    "explicit_negative",
    "neutral",
    "unknown",
];
const ALLOWED_OBJECTIVE: &[&str] = &["pass", "fail", "not_applicable"];

fn resolution_llm_system_prompt() -> &'static str {
    r#"You label whether a multi-turn agent session resolved the user's task.

Output ONLY one JSON object (no markdown fences):
{
  "verdict": string,
  "confidence_band": string,
  "evidence_tier": string,
  "user_feedback_band": string,
  "objective_check_band": string|null,
  "signal_codes": [string],
  "recovery_attempted": boolean
}

Field rules:
- verdict: one of solved_confirmed, solved_inferred, partial, unresolved, failed, abandoned.
- confidence_band: high | medium | low.
- evidence_tier: A (explicit user confirmation) | B (strong objective success) | C (inferred completion) | D (insufficient signal).
- user_feedback_band: explicit_positive | explicit_negative | neutral | unknown.
- objective_check_band: pass | fail | not_applicable | null when no tool/objective signal.
- signal_codes: non-empty array; each item MUST be from this whitelist:
  user_explicit_positive, user_explicit_negative, user_correction_loop,
  closure_without_followup, followup_same_poi_later, objective_test_pass,
  objective_test_fail, objective_not_applicable, skill_created_this_session,
  skill_patched_this_session, insufficient_turns
- recovery_attempted: true if the agent retried or patched skills/tools after failure.

Judgment guidance (semantic, not keyword matching):
- Gratitude or brief closure ("谢谢", "好的", "ok thanks") after substantive help → often solved_inferred tier B/C, not unresolved.
- User correction loops or explicit dissatisfaction → partial/failed with user_correction_loop.
- Successful skill_manage / deliverable completion with no follow-up complaint → solved_inferred tier C minimum.
- Pure chit-chat or session ended mid-task with no outcome → unresolved/abandoned tier D.
- Do NOT invent signal_codes outside the whitelist."#
}

#[derive(Debug, Deserialize)]
struct LlmResolutionRaw {
    verdict: String,
    confidence_band: String,
    evidence_tier: String,
    user_feedback_band: String,
    #[serde(default)]
    objective_check_band: Option<String>,
    #[serde(default)]
    signal_codes: Vec<String>,
    #[serde(default)]
    recovery_attempted: bool,
}

/// Infer resolution payload from session transcript via auxiliary LLM.
pub async fn infer_resolution_from_transcript_llm(
    auxiliary: &AuxiliaryClient,
    transcript: &str,
    signals: &SessionSignals,
) -> Option<ResolutionPayload> {
    let body = if transcript.chars().count() > MAX_TRANSCRIPT_CHARS {
        format!(
            "{}\n…[truncated]",
            transcript
                .chars()
                .take(MAX_TRANSCRIPT_CHARS)
                .collect::<String>()
        )
    } else {
        transcript.to_string()
    };

    let hints = format!(
        "Structured hints from local rules (may be incomplete):\n\
         user_turns={}\n\
         tool_successes={}\n\
         tool_failures={}\n\
         skill_patched={}\n\
         skill_created={}\n\
         explicit_positive={}\n\
         explicit_negative={}\n\
         correction_loops={}\n\
         closure_without_followup={}",
        signals.user_turns,
        signals.tool_successes,
        signals.tool_failures,
        signals.skill_patched,
        signals.skill_created,
        signals.explicit_positive,
        signals.explicit_negative,
        signals.correction_loops,
        signals.closure_without_followup,
    );

    let user =
        format!("Label resolution for this agent session transcript.\n\n{hints}\n\n---\n\n{body}");

    let request = AuxiliaryRequest::new(
        AuxiliaryTask::Custom(RESOLUTION_LLM_TASK.to_string()),
        vec![
            text_message(Role::System, resolution_llm_system_prompt()),
            text_message(Role::User, user),
        ],
    )
    .with_temperature(0.1)
    .with_max_tokens(800)
    .with_timeout(Duration::from_secs(60));

    match auxiliary.call(request).await {
        Ok(resp) => {
            let text = resp.text().unwrap_or_default();
            match parse_llm_resolution_json(&text) {
                Some(payload) => {
                    debug!(
                        verdict = %payload.verdict,
                        evidence_tier = %payload.evidence_tier,
                        "resolution: session-end LLM labeling"
                    );
                    Some(payload)
                }
                None if !text.trim().is_empty() => {
                    warn!(
                        chars = text.chars().count(),
                        "resolution: LLM returned unparseable JSON — falling back to rules"
                    );
                    None
                }
                None => None,
            }
        }
        Err(err) => {
            warn!("resolution: LLM labeling failed: {err}");
            None
        }
    }
}

fn parse_llm_resolution_json(text: &str) -> Option<ResolutionPayload> {
    let trimmed = strip_json_fence(text.trim());
    let raw: LlmResolutionRaw = serde_json::from_str(&trimmed).ok()?;
    let verdict = normalize_enum(&raw.verdict, ALLOWED_VERDICTS)?;
    let confidence_band = normalize_enum(&raw.confidence_band, ALLOWED_CONFIDENCE)?;
    let evidence_tier = normalize_enum(&raw.evidence_tier, ALLOWED_TIERS)?;
    let user_feedback_band = normalize_enum(&raw.user_feedback_band, ALLOWED_FEEDBACK)?;
    let objective_check_band = raw
        .objective_check_band
        .as_ref()
        .and_then(|v| normalize_enum(v, ALLOWED_OBJECTIVE).map(str::to_string));

    let mut signal_codes: Vec<String> = raw
        .signal_codes
        .into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    if !validate_signal_codes(&signal_codes) {
        signal_codes.retain(|c| ALLOWED_SIGNAL_CODES.contains(&c.as_str()));
    }
    if signal_codes.is_empty() {
        signal_codes.push("closure_without_followup".to_string());
    }

    Some(ResolutionPayload {
        verdict: verdict.to_string(),
        confidence_band: confidence_band.to_string(),
        evidence_tier: evidence_tier.to_string(),
        user_feedback_band: user_feedback_band.to_string(),
        objective_check_band,
        signal_codes,
        recovery_attempted: raw.recovery_attempted,
    })
}

fn strip_json_fence(text: &str) -> String {
    if text.starts_with("```") {
        text.lines()
            .skip(1)
            .take_while(|l| !l.starts_with("```"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        text.to_string()
    }
}

fn normalize_enum<'a>(value: &str, allowed: &[&'a str]) -> Option<&'a str> {
    let lower = value.trim().to_ascii_lowercase();
    allowed
        .iter()
        .find(|v| v.eq_ignore_ascii_case(&lower))
        .copied()
}

fn truncate_message(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    format!(
        "{}…[truncated]",
        text.chars().take(max_chars).collect::<String>()
    )
}

/// Full session transcript for resolution LLM (user + assistant + tool outcomes).
pub fn format_session_transcript_for_resolution(messages: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = message_text_from_value(msg);
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        if role.eq_ignore_ascii_case("user") && is_poi_synthetic_user_text(trimmed) {
            continue;
        }
        let label = match role.to_ascii_lowercase().as_str() {
            "user" => "USER",
            "assistant" => "ASSISTANT",
            "tool" => "TOOL",
            _ => continue,
        };
        out.push_str(label);
        out.push_str(": ");
        out.push_str(&truncate_message(trimmed, MAX_MESSAGE_CHARS));
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ALLOWED_SIGNAL_CODES;

    #[test]
    fn parse_valid_llm_resolution() {
        let json = r#"{"verdict":"solved_inferred","confidence_band":"medium","evidence_tier":"C","user_feedback_band":"neutral","objective_check_band":"not_applicable","signal_codes":["skill_patched_this_session","closure_without_followup"],"recovery_attempted":false}"#;
        let parsed = parse_llm_resolution_json(json).expect("parse");
        assert_eq!(parsed.verdict, "solved_inferred");
        assert_eq!(parsed.evidence_tier, "C");
        assert!(validate_signal_codes(&parsed.signal_codes));
        assert!(
            parsed
                .signal_codes
                .iter()
                .all(|c| ALLOWED_SIGNAL_CODES.contains(&c.as_str()))
        );
    }

    #[test]
    fn strips_markdown_fence() {
        let json = "```json\n{\"verdict\":\"partial\",\"confidence_band\":\"low\",\"evidence_tier\":\"C\",\"user_feedback_band\":\"neutral\",\"signal_codes\":[\"user_correction_loop\"]}\n```";
        let parsed = parse_llm_resolution_json(json).expect("parse");
        assert_eq!(parsed.verdict, "partial");
    }

    #[test]
    fn format_transcript_includes_tool_role() {
        let messages = vec![
            serde_json::json!({"role":"user","content":"create skill"}),
            serde_json::json!({"role":"tool","content":"{\"success\": true}"}),
        ];
        let t = format_session_transcript_for_resolution(&messages);
        assert!(t.contains("USER:"));
        assert!(t.contains("TOOL:"));
    }
}
