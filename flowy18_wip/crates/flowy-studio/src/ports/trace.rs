//! Structured redacted trace projections and JSONL export.

use flowy_agent::{AcceptedMessage, AgentHandle, ChatLine, ReceiptState};
use serde::Serialize;

use super::{clip_label, display_path_str};

/// Maximum UTF-8 byte budget retained per trace summary line.
const MAX_TRACE_SUMMARY_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RedactedTraceEntry {
    pub seq: usize,
    pub level: TraceLevel,
    pub event: &'static str,
    pub summary: String,
}

/// Projects structured, redacted trace events for a single session purely from
/// `AgentHandle` in-memory projections (never reads kernel disk files).
#[must_use]
pub fn collect_session_traces(
    agent: &AgentHandle,
    session_id: &str,
    visible_receipts: &[(AcceptedMessage, ReceiptState)],
) -> Vec<RedactedTraceEntry> {
    let lines = agent.session_lines(session_id);
    let mut entries = Vec::new();
    let mut seen_work_ids = std::collections::HashSet::new();
    let mut push = |level: TraceLevel, event: &'static str, raw: String| {
        let seq = entries.len() + 1;
        entries.push(RedactedTraceEntry {
            seq,
            level,
            event,
            summary: redact_trace_text(&raw),
        });
    };

    for line in &lines {
        match line {
            ChatLine::User { text } => {
                push(TraceLevel::Info, "turn_start", clip_label(text, 120));
            }
            ChatLine::ToolCall {
                name,
                target,
                running,
                ..
            } => {
                let state = if *running { "running" } else { "done" };
                push(
                    TraceLevel::Info,
                    "tool_call",
                    format!(
                        "tool={name} state={state} target={}",
                        display_path_str(target)
                    ),
                );
            }
            ChatLine::ToolResult { text, is_error } if *is_error => {
                push(TraceLevel::Error, "tool_error", clip_label(text, 180));
            }
            ChatLine::Verification {
                report_id,
                artifact_path,
                artifact_hash,
                ok,
                issues,
                override_used,
                ..
            } => {
                let level = if *ok {
                    TraceLevel::Info
                } else {
                    TraceLevel::Warn
                };
                let short_hash: String =
                    artifact_hash.chars().take(12).collect();
                push(
                    level,
                    if *override_used {
                        "verify_override"
                    } else {
                        "verify_result"
                    },
                    format!(
                        "report={report_id} ok={ok} override={override_used} issues={} sha256={short_hash} file={}",
                        issues.len(),
                        display_path_str(artifact_path)
                    ),
                );
            }
            ChatLine::FileChanges { files } => {
                push(
                    TraceLevel::Info,
                    "delivery_files",
                    format!("count={}", files.len()),
                );
            }
            ChatLine::TaskReceipt { task_id, goal } => {
                push(
                    TraceLevel::Info,
                    "task_receipt",
                    format!("id={task_id} goal={}", clip_label(goal, 80)),
                );
            }
            _ => {}
        }
    }

    for (receipt, state) in visible_receipts.iter().filter(|(r, _)| {
        r.target_session_id.as_deref() == Some(session_id)
            || r.session_hint.as_deref() == Some(session_id)
    }) {
        let (level, event) = match *state {
            ReceiptState::Retrying => (TraceLevel::Warn, "receipt_retrying"),
            ReceiptState::Running => (TraceLevel::Info, "receipt_running"),
            ReceiptState::Dead => (TraceLevel::Error, "receipt_dead"),
            ReceiptState::Quarantined => {
                (TraceLevel::Error, "receipt_quarantined")
            }
            _ => (TraceLevel::Warn, "receipt_queued"),
        };
        push(
            level,
            event,
            format!(
                "id={} state={state:?} attempts={} text={}",
                receipt.id,
                receipt.attempts,
                clip_label(&receipt.text, 80)
            ),
        );
        if let Some(ws) = agent.work_status(&receipt.id)
            && !ws.missing_items.is_empty()
            && seen_work_ids.insert(ws.work_id.clone())
        {
            push(
                TraceLevel::Warn,
                "work_contract_missing",
                format!(
                    "work_id={} delivered={} missing={}",
                    ws.work_id,
                    ws.delivered_artifact_ids.len(),
                    ws.missing_items.join(", ")
                ),
            );
        }
    }

    if let Some(ws) = agent.session_work_status(session_id)
        && !ws.missing_items.is_empty()
        && seen_work_ids.insert(ws.work_id.clone())
    {
        push(
            TraceLevel::Warn,
            "work_contract_missing",
            format!(
                "work_id={} delivered={} missing={}",
                ws.work_id,
                ws.delivered_artifact_ids.len(),
                ws.missing_items.join(", ")
            ),
        );
    }

    // Keep ring-bounded tail (most recent 256 events).
    if entries.len() > 256 {
        let drop_count = entries.len() - 256;
        entries.drain(..drop_count);
    }
    entries
}

/// Formats redacted trace rows into JSONL for clipboard copy or bug reports.
#[must_use]
pub fn format_traces_jsonl(entries: &[RedactedTraceEntry]) -> String {
    entries
        .iter()
        .filter_map(|entry| serde_json::to_string(entry).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_secret_key(raw: &str) -> bool {
    let key = raw
        .trim_matches(|c: char| {
            !c.is_ascii_alphanumeric() && c != '_' && c != '-'
        })
        .to_ascii_lowercase();
    key.contains("key")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key == "authorization"
}

fn looks_like_secret_token(raw: &str) -> bool {
    let core = raw.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ',' | ';' | '{' | '}' | '[' | ']' | '(' | ')'
        )
    });
    core.len() >= 12
        && (core.starts_with("sk-")
            || core.starts_with("ghp_")
            || core.starts_with("eyJ"))
}

/// Masks secret tokens/keys (including JSON key-value pairs and JWTs) and
/// clamps payload length before displaying or exporting trace text.
#[must_use]
pub fn redact_trace_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_TRACE_SUMMARY_BYTES));
    let mut words = raw.split_whitespace();
    let mut redact_next = false;
    while let Some(word) = words.next() {
        if !out.is_empty() {
            out.push(' ');
        }
        if redact_next {
            redact_next = false;
            out.push_str("[REDACTED]");
            continue;
        }
        let stripped = word.trim_matches(|c: char| {
            matches!(c, '"' | '\'' | ',' | ';' | '{' | '}' | '[' | ']')
        });
        let lower = stripped.to_ascii_lowercase();
        if lower == "bearer" {
            out.push_str("Bearer [REDACTED]");
            let _ = words.next();
            continue;
        }
        if let Some((lhs, rhs)) = word.split_once('=')
            && is_secret_key(lhs)
        {
            out.push_str(lhs);
            out.push_str("=[REDACTED]");
            if rhs.is_empty() {
                redact_next = true;
            }
            continue;
        }
        if let Some((lhs, rhs)) = word.split_once(':')
            && is_secret_key(lhs)
        {
            out.push_str(lhs);
            if rhs
                .trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | '}'))
                .is_empty()
            {
                out.push(':');
                redact_next = true;
            } else {
                out.push_str(":[REDACTED]");
            }
            continue;
        }
        if looks_like_secret_token(word) {
            out.push_str("[REDACTED]");
            continue;
        }
        out.push_str(word);
        if out.len() >= MAX_TRACE_SUMMARY_BYTES {
            break;
        }
    }
    if out.len() > MAX_TRACE_SUMMARY_BYTES {
        let mut end = MAX_TRACE_SUMMARY_BYTES;
        while end > 0 && !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
        out.push('\u{2026}');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_trace_text_masks_secrets_and_clamps_long_payloads() {
        let raw = r#"Authorization: Bearer eyJhbGciOiJIUzI1NiJ9 api_key=my-secret-value {"api_key":"sk-live-1234567890abcdef","token": "eyJhbGciOiJIUzI1NiJ9.payload"} ok=true"#;
        let clean = redact_trace_text(raw);
        assert!(!clean.contains("eyJhbGciOiJIUzI1NiJ9"), "{clean}");
        assert!(!clean.contains("my-secret-value"), "{clean}");
        assert!(!clean.contains("sk-live-1234567890abcdef"), "{clean}");
        assert!(clean.contains("Bearer [REDACTED]"), "{clean}");
        assert!(clean.contains("api_key=[REDACTED]"), "{clean}");
        assert!(clean.contains("ok=true"), "{clean}");

        let huge = "a ".repeat(3000);
        let clamped = redact_trace_text(&huge);
        assert!(clamped.len() <= MAX_TRACE_SUMMARY_BYTES + 4);
    }
}
