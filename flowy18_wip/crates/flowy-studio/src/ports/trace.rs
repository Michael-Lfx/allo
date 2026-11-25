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

/// Projects structured, redacted trace events for a single session (or the
/// synthetic `UNASSIGNED_QUEUE_SESSION_ID` bucket).
///
/// May query kernel work-contract status on disk via `AgentHandle`, so callers
/// must invoke this from dirty-cache refresh paths — never inside `Render::render`.
#[must_use]
pub fn collect_session_traces(
    agent: &AgentHandle,
    session_id: &str,
    visible_receipts: &[(AcceptedMessage, ReceiptState)],
) -> Vec<RedactedTraceEntry> {
    let is_unassigned = session_id == super::UNASSIGNED_QUEUE_SESSION_ID;
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

    if !is_unassigned {
        let lines = agent.session_lines(session_id);
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
                    push(
                        TraceLevel::Error,
                        "tool_error",
                        clip_label(text, 180),
                    );
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
    }

    let known_sessions: std::collections::HashSet<String> = if is_unassigned {
        agent.list_sessions().into_iter().map(|s| s.id).collect()
    } else {
        std::collections::HashSet::new()
    };

    for (receipt, state) in visible_receipts.iter().filter(|(r, _)| {
        if is_unassigned {
            let owner =
                r.target_session_id.as_deref().or(r.session_hint.as_deref());
            owner.is_none_or(|id| !known_sessions.contains(id))
        } else {
            r.target_session_id.as_deref() == Some(session_id)
                || r.session_hint.as_deref() == Some(session_id)
        }
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

    if !is_unassigned
        && let Some(ws) = agent.session_work_status(session_id)
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
            '"' | '\'' | '\\' | ',' | ';' | '{' | '}' | '[' | ']' | '(' | ')'
        )
    });
    core.len() >= 12
        && (core.starts_with("sk-")
            || core.starts_with("ghp_")
            || core.starts_with("eyJ"))
}

const fn is_trace_delim(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            ',' | '&' | ';' | '?' | '{' | '}' | '(' | ')' | '[' | ']'
        )
}

const fn resets_pending_redaction(c: char) -> bool {
    matches!(c, ',' | '&' | ';' | '?' | '{' | '}' | '(' | ')' | '[' | ']')
}

fn find_first_kv_sep(segment: &str) -> Option<(usize, char)> {
    segment.char_indices().find(|&(_, c)| c == '=' || c == ':')
}

fn push_redacted_segment(
    segment: &str,
    redact_next: &mut bool,
    out: &mut String,
) {
    let stripped =
        segment.trim_matches(|c: char| matches!(c, '"' | '\'' | '\\'));
    if stripped.eq_ignore_ascii_case("bearer") {
        out.push_str("Bearer");
        *redact_next = true;
        return;
    }
    if *redact_next {
        *redact_next = false;
        out.push_str("[REDACTED]");
        return;
    }
    if let Some((sep_idx, sep_char)) = find_first_kv_sep(segment) {
        let lhs = &segment[..sep_idx];
        let rhs = &segment[sep_idx + 1..];
        let rhs_stripped =
            rhs.trim_matches(|c: char| matches!(c, '"' | '\'' | '\\'));
        if is_secret_key(lhs) {
            out.push_str(lhs);
            out.push(sep_char);
            if rhs.is_empty() {
                *redact_next = true;
            } else if rhs_stripped.eq_ignore_ascii_case("bearer") {
                out.push_str("Bearer");
                *redact_next = true;
            } else {
                out.push_str("[REDACTED]");
            }
            return;
        }
        if looks_like_secret_token(rhs) {
            out.push_str(lhs);
            out.push(sep_char);
            out.push_str("[REDACTED]");
            return;
        }
    }
    if looks_like_secret_token(segment) {
        out.push_str("[REDACTED]");
        return;
    }
    out.push_str(segment);
}

/// Masks secret tokens/keys (including compact JSON key-value pairs, URL query
/// parameters, Bearer headers, and JWTs) and clamps payload length before
/// displaying or exporting trace text.
#[must_use]
pub fn redact_trace_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_TRACE_SUMMARY_BYTES));
    let mut redact_next = false;
    let mut seg_start: Option<usize> = None;

    for (idx, ch) in raw.char_indices() {
        if is_trace_delim(ch) {
            if let Some(start) = seg_start.take() {
                push_redacted_segment(
                    &raw[start..idx],
                    &mut redact_next,
                    &mut out,
                );
            }
            if resets_pending_redaction(ch) {
                redact_next = false;
            }
            out.push(ch);
            if out.len() > MAX_TRACE_SUMMARY_BYTES {
                break;
            }
        } else if seg_start.is_none() {
            seg_start = Some(idx);
        }
    }
    if out.len() <= MAX_TRACE_SUMMARY_BYTES
        && let Some(start) = seg_start
    {
        push_redacted_segment(&raw[start..], &mut redact_next, &mut out);
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

    #[test]
    fn redact_trace_text_masks_compact_json_non_first_keys_and_url_queries() {
        let json = r#"{"user":"alice","api_key":"top-secret-1","token":"opaque-tok-2","Authorization":"Bearer opaque-bearer-3","note":"sk-proj-abcdefghijklmno","ok":true}"#;
        let clean_json = redact_trace_text(json);
        assert!(!clean_json.contains("top-secret-1"), "{clean_json}");
        assert!(!clean_json.contains("opaque-tok-2"), "{clean_json}");
        assert!(!clean_json.contains("opaque-bearer-3"), "{clean_json}");
        assert!(
            !clean_json.contains("sk-proj-abcdefghijklmno"),
            "{clean_json}"
        );
        assert!(clean_json.contains(r#""user":"alice""#), "{clean_json}");
        assert!(clean_json.contains(r#""ok":true"#), "{clean_json}");

        let url = "GET https://server.flowyaipc.com/v1?x=1&api_key=url-secret-1&access_token=url-tok-2&ok=true";
        let clean_url = redact_trace_text(url);
        assert!(!clean_url.contains("url-secret-1"), "{clean_url}");
        assert!(!clean_url.contains("url-tok-2"), "{clean_url}");
        assert!(clean_url.contains("x=1"), "{clean_url}");
        assert!(clean_url.contains("ok=true"), "{clean_url}");
    }

    #[test]
    fn collect_session_traces_surfaces_unassigned_queue_receipts() {
        let root = std::env::temp_dir().join(format!(
            "flowy-studio-trace-unassigned-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let queue = flowy_agent::AcceptanceQueue::new(root.clone());
        let unowned = queue
            .accept(None, "queued with api_key=leak-me-not", 10)
            .unwrap();
        let orphan = queue
            .accept(Some("gone-session"), "dead orphan task", 20)
            .unwrap();
        let handle = AgentHandle::with_config(
            root.clone(),
            flowy_agent::LlmConfig::default(),
        );
        let visible = vec![
            (unowned, ReceiptState::Queued),
            (orphan, ReceiptState::Dead),
        ];
        let entries = collect_session_traces(
            &handle,
            crate::ports::UNASSIGNED_QUEUE_SESSION_ID,
            &visible,
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].event, "receipt_queued");
        assert!(!entries[0].summary.contains("leak-me-not"));
        assert_eq!(entries[1].event, "receipt_dead");
        assert_eq!(entries[1].level, TraceLevel::Error);

        drop(handle);
        let _ = std::fs::remove_dir_all(root);
    }
}
