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
                    let safe_target = redact_trace_text(&clip_label(
                        &display_path_str(target),
                        240,
                    ));
                    push(
                        TraceLevel::Info,
                        "tool_call",
                        format!(
                            "tool={name} state={state} target={safe_target}"
                        ),
                    );
                }
                ChatLine::ToolResult { text, is_error } if *is_error => {
                    push(
                        TraceLevel::Error,
                        "tool_error",
                        clip_error_tail(text, 120),
                    );
                }
                ChatLine::Verification {
                    report_id,
                    artifact_path,
                    artifact_hash,
                    ok,
                    issues,
                    override_used,
                    provenance_warnings,
                    ..
                } => {
                    let level = if *ok && provenance_warnings.is_empty() {
                        TraceLevel::Info
                    } else {
                        TraceLevel::Warn
                    };
                    let short_hash: String =
                        artifact_hash.chars().take(12).collect();
                    let safe_file = redact_trace_text(&clip_label(
                        &display_path_str(artifact_path),
                        200,
                    ));
                    let prov_suffix = if provenance_warnings.is_empty() {
                        String::new()
                    } else {
                        let safe_warnings =
                            redact_trace_text(&provenance_warnings.join(","));
                        format!(" provenance_warnings={safe_warnings}")
                    };
                    push(
                        level,
                        if *override_used {
                            "verify_override"
                        } else {
                            "verify_result"
                        },
                        format!(
                            "report={report_id} ok={ok} override={override_used} issues={} sha256={short_hash} file={safe_file}{prov_suffix}",
                            issues.len()
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
                    let safe_goal = redact_trace_text(&clip_label(goal, 80));
                    push(
                        TraceLevel::Info,
                        "task_receipt",
                        format!("id={task_id} goal={safe_goal}"),
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
        let safe_text = redact_trace_text(&clip_label(&receipt.text, 80));
        push(
            level,
            event,
            format!(
                "id={} state={state:?} attempts={} text={safe_text}",
                receipt.id, receipt.attempts
            ),
        );
        if let Some(ws) = agent.work_status(&receipt.id)
            && !ws.missing_items.is_empty()
            && seen_work_ids.insert(ws.work_id.clone())
        {
            let safe_missing = redact_trace_text(&ws.missing_items.join(", "));
            push(
                TraceLevel::Warn,
                "work_contract_missing",
                format!(
                    "work_id={} delivered={} missing={safe_missing}",
                    ws.work_id,
                    ws.delivered_artifact_ids.len()
                ),
            );
        }
    }

    if !is_unassigned
        && let Some(ws) = agent.session_work_status(session_id)
        && !ws.missing_items.is_empty()
        && seen_work_ids.insert(ws.work_id.clone())
    {
        let safe_missing = redact_trace_text(&ws.missing_items.join(", "));
        push(
            TraceLevel::Warn,
            "work_contract_missing",
            format!(
                "work_id={} delivered={} missing={safe_missing}",
                ws.work_id,
                ws.delivered_artifact_ids.len()
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
        || key.contains("passwd")
        || key.contains("credential")
        || key == "cookie"
        || key == "set-cookie"
        || key == "authorization"
}

fn is_auth_scheme(raw: &str) -> bool {
    let word = raw.trim_matches(|c: char| matches!(c, '"' | '\'' | '\\'));
    word.eq_ignore_ascii_case("bearer")
        || word.eq_ignore_ascii_case("basic")
        || word.eq_ignore_ascii_case("token")
        || word.eq_ignore_ascii_case("apikey")
        || word.eq_ignore_ascii_case("digest")
        || word.eq_ignore_ascii_case("negotiate")
}

fn looks_like_secret_token(raw: &str) -> bool {
    let core = raw.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | '\\' | ',' | ';' | '{' | '}' | '[' | ']' | '(' | ')'
        )
    });
    if core.len() < 12 {
        return false;
    }
    const PREFIXES: &[&str] = &[
        "sk-",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "AIza",
        "eyJ",
    ];
    PREFIXES.iter().any(|prefix| core.starts_with(prefix))
}

const fn is_trace_delim(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            ',' | '&' | ';' | '?' | '#' | '{' | '}' | '(' | ')' | '[' | ']'
        )
}

const fn resets_pending_redaction(c: char) -> bool {
    matches!(
        c,
        ',' | '&' | ';' | '?' | '#' | '{' | '}' | '(' | ')' | '[' | ']'
    )
}

fn push_bounded(out: &mut String, text: &str) {
    let budget = MAX_TRACE_SUMMARY_BYTES.saturating_add(16);
    if out.len() >= budget {
        return;
    }
    let remaining = budget - out.len();
    if text.len() <= remaining {
        out.push_str(text);
    } else {
        let mut end = remaining;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&text[..end]);
    }
}

fn redact_url_userinfo(segment: &str) -> Option<String> {
    let scheme_end = segment.find("://")?;
    let after_scheme = &segment[scheme_end + 3..];
    let authority_end = after_scheme.find('/').unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let at_pos = authority.rfind('@')?;
    let mut masked = String::with_capacity(segment.len());
    masked.push_str(&segment[..scheme_end + 3]);
    masked.push_str("[REDACTED]@");
    masked.push_str(&after_scheme[at_pos + 1..]);
    Some(masked)
}

fn redact_path_embedded_tokens(segment: &str) -> String {
    if !segment.contains('/') && !segment.contains('\\') {
        return segment.to_owned();
    }
    let mut result = String::with_capacity(segment.len());
    let mut part_start = 0;
    for (idx, ch) in segment.char_indices() {
        if ch == '/' || ch == '\\' {
            let part = &segment[part_start..idx];
            if looks_like_secret_token(part) {
                result.push_str("[REDACTED]");
            } else {
                result.push_str(part);
            }
            result.push(ch);
            part_start = idx + ch.len_utf8();
        }
    }
    let tail = &segment[part_start..];
    if looks_like_secret_token(tail) {
        result.push_str("[REDACTED]");
    } else {
        result.push_str(tail);
    }
    result
}

fn push_redacted_segment(
    segment: &str,
    redact_next: &mut bool,
    out: &mut String,
) {
    let stripped =
        segment.trim_matches(|c: char| matches!(c, '"' | '\'' | '\\'));
    if stripped == ":" || stripped == "=" {
        push_bounded(out, segment);
        return;
    }
    if stripped.eq_ignore_ascii_case("bearer")
        || stripped.eq_ignore_ascii_case("basic")
    {
        push_bounded(out, stripped);
        *redact_next = true;
        return;
    }
    if *redact_next {
        if is_auth_scheme(segment) {
            push_bounded(out, stripped);
            // Keep `redact_next = true` so the credential after the scheme is redacted.
            return;
        }
        *redact_next = false;
        out.push_str("[REDACTED]");
        return;
    }
    // Check URL userinfo (`https://user:pass@host/...`) first.
    if let Some(masked_url) = redact_url_userinfo(segment) {
        let masked_path = redact_path_embedded_tokens(&masked_url);
        push_bounded(out, &masked_path);
        return;
    }
    // Handle standalone quoted secret key followed by spaced colon/value
    // (e.g. `{"api_key" : "secret"}`) without redacting normal prose words
    // such as "key takeaway" or "token limit".
    if segment.starts_with('"')
        && segment.ends_with('"')
        && is_secret_key(segment)
        && !segment.contains('=')
        && !segment.contains(':')
    {
        push_bounded(out, segment);
        *redact_next = true;
        return;
    }
    // Scan every `=`/`:` separator so nested forms like `$env:OPENAI_API_KEY=...`
    // or `target=api_key=...` cannot hide an inner secret key behind an outer label.
    let mut prev_sep_end = 0;
    for (sep_idx, sep_char) in segment.char_indices() {
        if sep_char != '=' && sep_char != ':' {
            continue;
        }
        let lhs_full = &segment[..sep_idx];
        let lhs_local = &segment[prev_sep_end..sep_idx];
        let rhs = &segment[sep_idx + 1..];
        let rhs_stripped =
            rhs.trim_matches(|c: char| matches!(c, '"' | '\'' | '\\'));
        if is_secret_key(lhs_local) {
            push_bounded(out, lhs_full);
            out.push(sep_char);
            if rhs.is_empty() {
                *redact_next = true;
            } else if is_auth_scheme(rhs_stripped) {
                push_bounded(out, rhs_stripped);
                *redact_next = true;
            } else {
                out.push_str("[REDACTED]");
            }
            return;
        }
        if looks_like_secret_token(rhs) {
            push_bounded(out, lhs_full);
            out.push(sep_char);
            out.push_str("[REDACTED]");
            return;
        }
        if rhs_stripped.eq_ignore_ascii_case("bearer")
            || rhs_stripped.eq_ignore_ascii_case("basic")
            || rhs_stripped.eq_ignore_ascii_case("apikey")
            || rhs_stripped.eq_ignore_ascii_case("api-key")
        {
            push_bounded(out, segment);
            *redact_next = true;
            return;
        }
        prev_sep_end = sep_idx + 1;
    }
    if looks_like_secret_token(segment) {
        out.push_str("[REDACTED]");
        return;
    }
    let path_safe = redact_path_embedded_tokens(segment);
    push_bounded(out, &path_safe);
}

/// Retains the final non-empty trimmed line of an error payload, keeping its
/// trailing `max_chars` Unicode scalar values prefixed with `…` when longer.
#[expect(
    clippy::redundant_pub_crate,
    reason = "explicit crate-visible helper inside private trace module"
)]
pub(crate) fn clip_error_tail(raw: &str, max_chars: usize) -> String {
    let last = raw
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if last.is_empty() || max_chars == 0 {
        return String::new();
    }
    let count = last.chars().count();
    if count <= max_chars {
        return last.to_owned();
    }
    let tail: String = last.chars().skip(count - max_chars).collect();
    format!("\u{2026}{tail}")
}

/// Masks secret tokens and clamps payload length before displaying or exporting trace text.
///
/// Handles compact/spaced JSON key-value pairs, URL credentials/query/fragment
/// parameters, PowerShell `$env:` assignments, `Authorization` schemes,
/// path-embedded tokens, and JWTs.
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
            push_bounded(&mut out, ch.encode_utf8(&mut [0; 4]));
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

    #[test]
    fn redact_trace_text_blocks_nested_powershell_basic_url_fragment_and_blob_bypasses()
     {
        // Outer prefix label (`text=`/`target=`) must not hide inner secret keys or Bearer tokens.
        let prefixed = "text=api_key=sk-live-1234567890abcdef target=API_KEY=plain-secret text=Bearer opaque-tok-99";
        let clean_prefixed = redact_trace_text(prefixed);
        assert!(
            !clean_prefixed.contains("sk-live-1234567890abcdef"),
            "{clean_prefixed}"
        );
        assert!(!clean_prefixed.contains("plain-secret"), "{clean_prefixed}");
        assert!(
            !clean_prefixed.contains("opaque-tok-99"),
            "{clean_prefixed}"
        );

        // PowerShell `$env:OPENAI_API_KEY=...`, Basic auth, URL userinfo, `#` fragment, path-embedded token, spaced JSON.
        let complex = concat!(
            r#"$env:OPENAI_API_KEY="sk-live-abcdef123456" "#,
            r#"Authorization: Basic dXNlcjpwYXNz "#,
            r#"{"Authorization":"Basic c2VjcmV0OnBhc3M="} "#,
            r#"{"api_key" : "spaced-secret-val"} "#,
            r#"https://alice:superpass@api.example.com/v1#access_token=frag-secret-42 "#,
            r#"C:\tokens\ghp_1234567890abcdef "#,
            r#"Remember the key takeaway is 42"#
        );
        let clean = redact_trace_text(complex);
        for leaked in [
            "sk-live-abcdef123456",
            "dXNlcjpwYXNz",
            "c2VjcmV0OnBhc3M=",
            "spaced-secret-val",
            "alice:superpass",
            "frag-secret-42",
            "ghp_1234567890abcdef",
        ] {
            assert!(!clean.contains(leaked), "leaked {leaked} in: {clean}");
        }
        assert!(
            clean.contains("Remember the key takeaway is 42"),
            "prose must not be falsely redacted: {clean}"
        );

        // Multi-MB delimiter-free blob is bounded without unbounded allocation.
        let giant_blob = "x".repeat(128 * 1024);
        let clamped = redact_trace_text(&giant_blob);
        assert!(clamped.len() <= MAX_TRACE_SUMMARY_BYTES + 4);
    }

    #[test]
    fn clip_error_tail_extracts_root_cause_line_and_retains_utf8_tail() {
        let python_tb = "Traceback (most recent call last):\n  File \"main.py\", line 12, in <module>\n    run()\nValueError: invalid port number   \n\n  ";
        assert_eq!(
            clip_error_tail(python_tb, 120),
            "ValueError: invalid port number"
        );

        let rust_tb = "thread 'main' panicked at src/lib.rs:42:9:\nassertion `left == right` failed\n  left: 1\n right: 2\n";
        assert_eq!(clip_error_tail(rust_tb, 120), "right: 2");

        assert_eq!(clip_error_tail("", 120), "");
        assert_eq!(clip_error_tail("   \n\t\r\n  ", 120), "");
        assert_eq!(clip_error_tail("Error: boom", 0), "");

        let long_ascii = format!("prefix: {}_TAIL_END", "a".repeat(150));
        let clipped_ascii = clip_error_tail(&long_ascii, 120);
        assert!(clipped_ascii.starts_with('\u{2026}'));
        assert!(clipped_ascii.ends_with("_TAIL_END"));
        assert_eq!(clipped_ascii.chars().count(), 121);

        let long_cjk = format!("错误详情：{}核心失败原因", "字".repeat(130));
        let clipped_cjk = clip_error_tail(&long_cjk, 120);
        assert!(clipped_cjk.starts_with('\u{2026}'));
        assert!(clipped_cjk.ends_with("核心失败原因"));
        assert_eq!(clipped_cjk.chars().count(), 121);
    }
}
