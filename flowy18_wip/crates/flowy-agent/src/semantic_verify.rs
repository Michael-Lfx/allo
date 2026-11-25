//! L3 semantic review hint layer (`最终方案.md` §6.7).
//!
//! Runs independently of the L1/L2 hard gate:
//! - Literal-brief-only prompt rule ("只依据简报的字面要求。简报没要求的，缺了不算问题。").
//! - Strict JSON boolean validation (`"false"` string or non-boolean is never
//!   accepted as `ok = true`).
//! - Fail-closed on timeout, transport error, or malformed JSON (`Inconclusive`).
//! - Attaches an advisory hint to the Studio verification card and structured
//!   telemetry to `flowy.verify` without blocking `DeliveryStore`.

use std::time::{Duration, Instant};

use flowy_agent_core::{CancelToken, ModelRoute, complete_text};
use serde_json::Value;

pub const SEMANTIC_SYSTEM_PROMPT: &str = "\
你是交付物语义复核器（L3）。\
只依据简报的字面要求。简报没要求的，缺了不算问题。\
只复核文件名与幻灯片正文内容是否与简报字面要求一致；忽略流程性指令（如先核验、再交付、发送给我等，这些由程序硬门保证，不写在幻灯片正文里）。\
请直接输出单行紧凑 JSON 对象（不要代码块、不要额外说明）：\
{\"ok\": true|false, \"issues\": [{\"where\": \"位置\", \"what\": \"现象\", \"why\": \"依据\"}]}。\
其中 ok 必须是 JSON 布尔值；若 ok 为 false，issues 至少包含一条带非空 where / what / why 的记录。";

const SEMANTIC_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct SemanticContext {
    pub brief: String,
    pub route: ModelRoute,
    pub api_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticIssue {
    pub location: String,
    pub what: String,
    pub why: String,
}

impl SemanticIssue {
    #[must_use]
    pub fn format_line(&self) -> String {
        format!("{}: {}（{}）", self.location, self.what, self.why)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticOutcome {
    Skipped,
    Passed,
    Flagged { issues: Vec<SemanticIssue> },
    Inconclusive { reason: String },
}

impl SemanticOutcome {
    #[must_use]
    pub const fn status_code(&self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Passed => "passed",
            Self::Flagged { .. } => "flagged",
            Self::Inconclusive { .. } => "inconclusive",
        }
    }

    #[must_use]
    pub const fn issue_count(&self) -> usize {
        match self {
            Self::Flagged { issues } => issues.len(),
            Self::Skipped | Self::Passed | Self::Inconclusive { .. } => 0,
        }
    }

    #[must_use]
    pub const fn reason(&self) -> &str {
        match self {
            Self::Inconclusive { reason } => reason.as_str(),
            Self::Skipped | Self::Passed | Self::Flagged { .. } => "",
        }
    }

    #[must_use]
    pub fn hint_text(&self) -> Option<String> {
        match self {
            Self::Flagged { issues } if !issues.is_empty() => Some(
                issues
                    .iter()
                    .map(SemanticIssue::format_line)
                    .collect::<Vec<_>>()
                    .join("；"),
            ),
            Self::Skipped
            | Self::Passed
            | Self::Flagged { .. }
            | Self::Inconclusive { .. } => None,
        }
    }
}

fn strip_code_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    let rest = rest
        .strip_prefix("json")
        .or_else(|| rest.strip_prefix("JSON"))
        .unwrap_or(rest)
        .trim_start();
    rest.strip_suffix("```").map_or(trimmed, str::trim)
}

/// Strictly parse and validate the model's L3 JSON verdict.
///
/// Any non-boolean `"ok"` (such as `"false"` or `1`), missing required fields,
/// or empty issue list on `ok: false` resolves to [`SemanticOutcome::Inconclusive`]
/// so malformed outputs never masquerade as a clean pass.
#[must_use]
pub fn parse_semantic_response(raw: &str) -> SemanticOutcome {
    let cleaned = strip_code_fence(raw);
    let Ok(value) = serde_json::from_str::<Value>(cleaned) else {
        return SemanticOutcome::Inconclusive {
            reason: "invalid_json".to_owned(),
        };
    };
    let Some(Value::Bool(ok)) = value.get("ok") else {
        return SemanticOutcome::Inconclusive {
            reason: "non_boolean_ok".to_owned(),
        };
    };
    let mut parsed_issues = Vec::new();
    if let Some(items) = value.get("issues") {
        let Some(array) = items.as_array() else {
            return SemanticOutcome::Inconclusive {
                reason: "invalid_issues_array".to_owned(),
            };
        };
        for item in array {
            let location = item
                .get("where")
                .and_then(Value::as_str)
                .map_or("", str::trim);
            let what = item
                .get("what")
                .and_then(Value::as_str)
                .map_or("", str::trim);
            let why = item
                .get("why")
                .and_then(Value::as_str)
                .map_or("", str::trim);
            if location.is_empty() || what.is_empty() || why.is_empty() {
                return SemanticOutcome::Inconclusive {
                    reason: "incomplete_issue_fields".to_owned(),
                };
            }
            parsed_issues.push(SemanticIssue {
                location: location.to_owned(),
                what: what.to_owned(),
                why: why.to_owned(),
            });
        }
    }
    if !parsed_issues.is_empty() {
        return SemanticOutcome::Flagged {
            issues: parsed_issues,
        };
    }
    if *ok {
        SemanticOutcome::Passed
    } else {
        SemanticOutcome::Inconclusive {
            reason: "failed_without_issues".to_owned(),
        }
    }
}

pub async fn probe_deck_semantics(
    ctx: &SemanticContext,
    artifact_name: &str,
    deck_shapes: &[(String, String)],
    signal: &CancelToken,
) -> (SemanticOutcome, u64) {
    if ctx.brief.trim().is_empty()
        || ctx.api_key.trim().is_empty()
        || deck_shapes.is_empty()
    {
        return (SemanticOutcome::Skipped, 0);
    }
    let started = Instant::now();
    let mut body = String::new();
    body.push_str("【简报字面要求】\n");
    let brief: String = ctx.brief.chars().take(2_000).collect();
    body.push_str(&brief);
    body.push_str("\n\n【产物信息与幻灯片文本】\n");
    if !artifact_name.trim().is_empty() {
        body.push_str("- 文件名: ");
        body.push_str(artifact_name.trim());
        body.push('\n');
    }
    for (name, text) in deck_shapes.iter().take(64) {
        let snippet: String = text.chars().take(400).collect();
        body.push_str("- ");
        body.push_str(name);
        body.push_str(": ");
        body.push_str(&snippet);
        body.push('\n');
    }
    let call = complete_text(
        Some("l3-semantic-verify".to_owned()),
        SEMANTIC_SYSTEM_PROMPT,
        &body,
        ctx.route.clone(),
        Some(ctx.api_key.clone()),
        signal.clone(),
    );
    let outcome = match tokio::time::timeout(SEMANTIC_TIMEOUT, call).await {
        Ok(Ok(raw)) => parse_semantic_response(&raw),
        Ok(Err(_)) => SemanticOutcome::Inconclusive {
            reason: "provider_error".to_owned(),
        },
        Err(_) => SemanticOutcome::Inconclusive {
            reason: "timeout".to_owned(),
        },
    };
    let elapsed_ms =
        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    (outcome, elapsed_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_preserves_literal_brief_only_rule() {
        assert!(
            SEMANTIC_SYSTEM_PROMPT
                .contains("只依据简报的字面要求。简报没要求的，缺了不算问题。")
        );
        assert!(SEMANTIC_SYSTEM_PROMPT.contains("忽略流程性指令"));
        assert_eq!(SemanticOutcome::Passed.reason(), "");
        assert_eq!(
            SemanticOutcome::Inconclusive {
                reason: "timeout".to_owned(),
            }
            .reason(),
            "timeout"
        );
    }

    #[test]
    fn string_boolean_false_and_malformed_outputs_fail_closed() {
        for sample in [
            r#"{"ok": "false", "issues": []}"#,
            r#"{"ok": "true", "issues": []}"#,
            r#"{"ok": 1, "issues": []}"#,
            r#"{"ok": false, "issues": []}"#,
            r#"{"ok": false, "issues": [{"where": "S0", "what": "bad"}]}"#,
            "not json at all",
        ] {
            let outcome = parse_semantic_response(sample);
            assert!(
                matches!(outcome, SemanticOutcome::Inconclusive { .. }),
                "sample {sample:?} must fail closed as Inconclusive, got {outcome:?}"
            );
            assert_ne!(outcome.status_code(), "passed");
        }
    }

    #[test]
    fn valid_pass_and_flagged_verdicts_parse_cleanly() {
        assert_eq!(
            parse_semantic_response(
                "```json\n{\"ok\": true, \"issues\": []}\n```"
            ),
            SemanticOutcome::Passed
        );
        let flagged = parse_semantic_response(
            r#"{"ok": false, "issues": [{"where": "第1页 S0-body", "what": "写成华南区", "why": "简报字面要求华东区"}]}"#,
        );
        assert_eq!(flagged.status_code(), "flagged");
        assert_eq!(flagged.issue_count(), 1);
        assert_eq!(
            flagged.hint_text().as_deref(),
            Some("第1页 S0-body: 写成华南区（简报字面要求华东区）")
        );
    }

    #[test]
    fn semantic_flag_never_blocks_the_hard_delivery_gate() {
        let root = std::env::temp_dir()
            .join(format!("flowy-l3-nonblocking-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let deck = root.join("deck.pptx");
        std::fs::write(&deck, b"PK\x03\x04verified-l1-l2").unwrap();
        let store = crate::delivery::DeliveryStore::new(root.join("delivery"));
        // L1/L2 report passed; L3 flagged an advisory semantic note.
        store.record_report(&deck, Vec::new(), 10).unwrap();
        let flagged = parse_semantic_response(
            r#"{"ok": false, "issues": [{"where": "S1", "what": "缺少时间节点", "why": "简报要求下两个季度"}]}"#,
        );
        assert!(flagged.hint_text().is_some());
        let outcome = store
            .deliver_with(&deck, false, 20, |_| Ok(()))
            .unwrap()
            .outcome;
        assert!(outcome.delivered);
        assert_eq!(outcome.blocked, None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
