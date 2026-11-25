//! Delivery-time verification of a generated deck.
//!
//! The verifier checks structure first (slide count), then anchors by shape
//! name. The four codes are type-independent: missing, duplicate, extra
//! (invalid) and value mismatch. A report is aggregated by issue code; it is a
//! result, not a tool error, so `ok=false` is visible to the caller.

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use flowy_agent_core::ToolCallArgs;
use flowy_agent_core::ToolExecutionResult;
use flowy_agent_core::tools::ToolDefinition;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::delivery::{self, DeliveryStore};
use crate::pptx::resolve_turn_path;
use crate::semantic_verify::{
    SemanticContext, SemanticOutcome, probe_deck_semantics,
};
use crate::verify_contract::{BriefContract, SlideL1Stats, next_action_for};

pub const ANCHOR_MISSING: &str = "ANCHOR_MISSING";
pub const ANCHOR_DUPLICATE: &str = "ANCHOR_DUPLICATE";
pub const ANCHOR_INVALID: &str = "ANCHOR_INVALID";
pub const VALUE_MISMATCH: &str = "VALUE_MISMATCH";
pub const STRUCTURE: &str = "STRUCTURE";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyIssue {
    pub code: &'static str,
    pub field: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub artifact: String,
    pub anchor: &'static str,
    pub ok: bool,
    pub issues: Vec<VerifyIssue>,
}

impl VerifyReport {
    #[must_use]
    pub fn to_value_with_meta(
        &self,
        summary: &str,
        report_id: &str,
        artifact_hash: &str,
        semantic_hint: Option<&str>,
    ) -> Value {
        json!({
            "summary": summary,
            "report_id": report_id,
            "artifact": self.artifact,
            "artifact_hash": artifact_hash,
            "anchor": self.anchor,
            "ok": self.ok,
            "next_action": next_action_for(self),
            "semantic_hint": semantic_hint,
            "issues": self.issues.iter().map(|issue| json!({
                "code": issue.code,
                "field": issue.field,
                "ui_key": ui_key(issue.code),
            })).collect::<Vec<_>>(),
        })
    }
}

#[must_use]
pub fn ui_key(code: &str) -> &'static str {
    match code {
        ANCHOR_MISSING => "verify.anchor_missing",
        ANCHOR_DUPLICATE => "verify.anchor_duplicate",
        ANCHOR_INVALID => "verify.anchor_invalid",
        VALUE_MISMATCH => "verify.value_mismatch",
        _ => "verify.structure",
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeckAnchor {
    pub anchor: String,
    pub value: String,
}

fn push_unique(issues: &mut Vec<VerifyIssue>, code: &'static str, field: &str) {
    if issues
        .iter()
        .any(|issue| issue.code == code && issue.field == field)
    {
        return;
    }
    issues.push(VerifyIssue {
        code,
        field: field.to_owned(),
    });
}

/// Evaluate a flattened deck (`(shape_name, text)` pairs) against expectations.
fn evaluate(
    artifact: &str,
    slide_count: usize,
    expected_slides: usize,
    deck: &[(String, String)],
    expected: &[DeckAnchor],
) -> VerifyReport {
    let mut issues = Vec::new();
    if slide_count != expected_slides {
        push_unique(&mut issues, STRUCTURE, "slides");
    }
    let mut seen: Vec<&str> = Vec::new();
    for anchor in expected {
        let matches: Vec<&(String, String)> = deck
            .iter()
            .filter(|(name, _)| name == &anchor.anchor)
            .collect();
        if matches.is_empty() {
            push_unique(&mut issues, ANCHOR_MISSING, &anchor.anchor);
        } else if matches.len() > 1 {
            push_unique(&mut issues, ANCHOR_DUPLICATE, &anchor.anchor);
        } else if matches[0].1 != anchor.value {
            push_unique(&mut issues, VALUE_MISMATCH, &anchor.anchor);
        }
        seen.push(&anchor.anchor);
    }
    for (name, _) in deck {
        if !seen.contains(&name.as_str()) {
            push_unique(&mut issues, ANCHOR_INVALID, name);
        }
    }
    VerifyReport {
        artifact: artifact.to_owned(),
        anchor: "shape_name",
        ok: issues.is_empty(),
        issues,
    }
}

/// Read the deck and verify its anchors by shape name.
#[cfg(test)]
fn verify_deck_file(
    path: &Path,
    expected_slides: usize,
    expected: &[DeckAnchor],
) -> Result<VerifyReport, String> {
    verify_deck_file_with_shapes(path, expected_slides, expected, None)
        .map(|(report, _)| report)
}

#[allow(clippy::redundant_pub_crate)]
pub(crate) fn verify_deck_file_with_shapes(
    path: &Path,
    expected_slides: usize,
    expected: &[DeckAnchor],
    contract: Option<&BriefContract>,
) -> Result<(VerifyReport, Vec<(String, String)>), String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let presentation = rpptx::Presentation::from_bytes(&bytes)
        .map_err(|error| error.to_string())?;
    let slides: Vec<rpptx::SlideRef<'_>> = presentation.slides().collect();
    let mut deck = Vec::new();
    let mut per_slide = Vec::with_capacity(slides.len());
    for (index, slide) in slides.iter().enumerate() {
        let mut has_title = false;
        let mut has_body = false;
        let mut non_empty_shapes = 0_usize;
        for shape in slide.shapes() {
            if let Some(name) = shape.non_visual_name() {
                let text = shape.text().unwrap_or_default();
                // Empty placeholders inherited from the layout are not
                // anchors; only shapes that carry content count.
                if !text.trim().is_empty() {
                    non_empty_shapes += 1;
                    let lower = name.to_ascii_lowercase();
                    if lower.contains("title") {
                        has_title = true;
                    } else {
                        has_body = true;
                    }
                    deck.push((name, text));
                }
            }
        }
        per_slide.push(SlideL1Stats {
            index,
            has_title,
            has_body,
            non_empty_shapes,
        });
    }
    let mut report = evaluate(
        &path.display().to_string(),
        slides.len(),
        expected_slides,
        &deck,
        expected,
    );
    if let Some(contract) = contract {
        contract.check_deck(
            path,
            slides.len(),
            expected_slides,
            &per_slide,
            expected.len(),
            &mut report.issues,
        );
    } else {
        BriefContract::default().check_deck(
            path,
            slides.len(),
            expected_slides,
            &per_slide,
            expected.len(),
            &mut report.issues,
        );
    }
    report.ok = report.issues.is_empty();
    Ok((report, deck))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

async fn execute(
    arguments: Value,
    context: &flowy_agent_core::ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
    contract: Option<&BriefContract>,
) -> ToolExecutionResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        path: String,
        slides: usize,
        anchors: Vec<DeckAnchor>,
    }
    let Ok(args) = serde_json::from_value::<Args>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path, slides and anchors",
        );
    };
    let path = resolve_turn_path(
        context.cwd.as_deref(),
        context.output_root.as_deref(),
        &args.path,
    );
    match verify_deck_file_with_shapes(
        &path,
        args.slides,
        &args.anchors,
        contract,
    ) {
        Ok((report, shapes)) => {
            let mut report_id = String::new();
            let mut artifact_hash = std::fs::read(&path).map_or_else(
                |_| String::new(),
                |bytes| {
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    format!("{:x}", hasher.finalize())
                },
            );
            if let Some(store) = store {
                let issues = report
                    .issues
                    .iter()
                    .map(|issue| delivery::Issue {
                        code: issue.code.to_owned(),
                        field: issue.field.clone(),
                    })
                    .collect();
                match store.record_report(&path, issues, now_ms()) {
                    Ok(persisted) => {
                        report_id = persisted.id;
                        artifact_hash = persisted.artifact_hash;
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "flowy.delivery",
                            %error,
                            path = %path.display(),
                            "could not persist the verification report"
                        );
                    }
                }
            }
            let artifact_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&args.path);
            let (l3_outcome, l3_duration_ms) = if report.ok
                && let Some(semantic_ctx) = semantic
            {
                probe_deck_semantics(
                    semantic_ctx,
                    artifact_name,
                    &shapes,
                    &context.signal,
                )
                .await
            } else {
                (SemanticOutcome::Skipped, 0)
            };
            let semantic_hint = l3_outcome.hint_text();
            let session_id = context.agent_id.as_deref().unwrap_or("");
            let submission_id = context.submission_id.as_deref().unwrap_or("");
            tracing::info!(
                target: "flowy.verify",
                session_id,
                root_session_id = session_id,
                submission_id,
                report_id = %report_id,
                artifact_hash = %artifact_hash,
                ok = report.ok,
                issue_count = report.issues.len(),
                l3_status = l3_outcome.status_code(),
                l3_reason = l3_outcome.reason(),
                l3_issue_count = l3_outcome.issue_count(),
                l3_duration_ms,
                "deck verification completed"
            );
            let summary = if report.ok {
                "verification passed".to_owned()
            } else {
                format!(
                    "verification failed: {}",
                    report
                        .issues
                        .iter()
                        .map(|issue| issue.code)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let value = report.to_value_with_meta(
                &summary,
                &report_id,
                &artifact_hash,
                semantic_hint.as_deref(),
            );
            let content = serde_json::to_string(&value).unwrap_or(summary);
            ToolExecutionResult::success_text(content, value)
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

pub fn definition(store: Option<DeliveryStore>) -> Arc<ToolDefinition> {
    definition_with_semantic_and_contract(store, None, None)
}

pub fn definition_with_semantic_and_contract(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "verify_deck",
        "Verify a generated deck before delivery. Checks host brief constraints, L1 per-slide title/body completeness, slide count, and each named shape against the expected text. Each anchor must be the slide shape name (for create_deck outputs: S0-title, S0-body, S1-title, S1-body, where slide 0 title is title and slide i>0 title is '{title} · {i+1}'). Returns ok, next_action ('publish' | 'rerender_once' | 'clarify_or_rebuild'), and ANCHOR_MISSING / ANCHOR_DUPLICATE / ANCHOR_INVALID / VALUE_MISMATCH / STRUCTURE issues; a failed report is not a tool error.",
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1},
                "slides": {"type": "integer", "minimum": 1},
                "anchors": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "anchor": {"type": "string", "minLength": 1},
                            "value": {"type": "string"}
                        },
                        "required": ["anchor", "value"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "slides", "anchors"],
            "additionalProperties": false
        }),
        move |args: ToolCallArgs, context: flowy_agent_core::ToolRunContext| {
            let store = store.clone();
            let semantic = semantic.clone();
            let contract = contract.clone();
            Box::pin(async move {
                execute(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    semantic.as_ref(),
                    contract.as_ref(),
                )
                .await
            })
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(name: &str, value: &str) -> DeckAnchor {
        DeckAnchor {
            anchor: name.to_owned(),
            value: value.to_owned(),
        }
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn a_complete_deck_passes() {
        let deck = vec![pair("S0-title", "T"), pair("S0-body", "one")];
        let expected = vec![anchor("S0-title", "T"), anchor("S0-body", "one")];
        let report = evaluate("deck.pptx", 1, 1, &deck, &expected);
        assert!(report.ok);
        assert_eq!(report.issues.as_slice(), []);
    }

    #[test]
    fn every_code_is_reported_distinctly() {
        let deck = vec![
            pair("S0-title", "T"),
            pair("S0-title", "T"),
            pair("S0-extra", "x"),
        ];
        let expected = vec![
            anchor("S0-title", "T"),
            anchor("S0-body", "one"),
            anchor("S0-value", "wrong"),
        ];
        let report = evaluate("deck.pptx", 2, 1, &deck, &expected);
        assert!(!report.ok);
        let codes: Vec<&str> =
            report.issues.iter().map(|issue| issue.code).collect();
        assert!(codes.contains(&STRUCTURE), "slide count mismatch");
        assert!(codes.contains(&ANCHOR_DUPLICATE), "two S0-title shapes");
        assert!(codes.contains(&ANCHOR_MISSING), "S0-value is absent");
        assert!(codes.contains(&ANCHOR_INVALID), "S0-extra is unexpected");
    }

    #[test]
    fn a_matching_anchor_with_another_value_is_a_mismatch() {
        let deck = vec![pair("S0-title", "Actual")];
        let report = evaluate(
            "deck.pptx",
            1,
            1,
            &deck,
            &[anchor("S0-title", "Expected")],
        );
        assert_eq!(report.issues.len(), 1);
        assert_eq!(report.issues[0].code, VALUE_MISMATCH);
        assert_eq!(report.issues[0].field, "S0-title");
    }

    #[test]
    fn a_written_deck_verifies_by_shape_name() {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("deck.pptx");
        crate::pptx::write_deck(
            &path,
            "T",
            &["one".to_owned(), "two".to_owned()],
        )
        .unwrap();
        let expected = vec![
            anchor("S0-title", "T"),
            anchor("S0-body", "one"),
            anchor("S1-title", "T · 2"),
            anchor("S1-body", "two"),
        ];
        let report = verify_deck_file(&path, 2, &expected).unwrap();
        assert!(report.ok, "unexpected issues: {:?}", report.issues);

        let wrong = vec![
            anchor("S0-title", "WRONG"),
            anchor("S0-body", "one"),
            anchor("S1-title", "T · 2"),
            anchor("S1-body", "two"),
        ];
        let report = verify_deck_file(&path, 2, &wrong).unwrap();
        assert!(!report.ok);
        assert_eq!(report.issues[0].code, VALUE_MISMATCH);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_verified_deck_is_recorded_for_the_delivery_gate() {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let deck = root.join("deck.pptx");
        crate::pptx::write_deck(&deck, "T", &["one".to_owned()]).unwrap();
        let store = DeliveryStore::new(root.join("delivery"));
        let tool = definition(Some(store.clone()));
        let context = flowy_agent_core::ToolRunContext {
            signal: flowy_agent_core::CancelToken::new(),
            agent_id: None,
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: None,
            session_origin: flowy_agent_core::SessionOrigin::Home,
            output_root: None,
            artifact_delivery_available: false,
        };
        let args = flowy_agent_core::ToolCallArgs {
            call_id: "call-1".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "deck.pptx",
                "slides": 1,
                "anchors": [
                    {"anchor": "S0-title", "value": "T"},
                    {"anchor": "S0-body", "value": "one"},
                ],
            }),
        };
        let result = (tool.execute)(args, context).await;
        assert!(
            matches!(result, ToolExecutionResult::Success { .. }),
            "verification must succeed: {result:?}"
        );
        assert!(store.check(&deck).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn shrinking_deck_below_brief_min_slides_fails_even_when_worker_lowers_declared_slides()
     {
        let root = std::env::temp_dir()
            .join(format!("flowy-verify-anticheat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let deck = root.join("q3-east.pptx");
        // Worker shrank a requested 3-slide deck down to 2 slides.
        crate::pptx::write_deck(
            &deck,
            "华东区Q3汇报",
            &["上海6.1亿".to_owned(), "江苏4.2亿".to_owned()],
        )
        .unwrap();
        let store = DeliveryStore::new(root.join("delivery"));
        let contract = BriefContract::from_brief(
            "请生成一份3页华东区汇报 q3-east.pptx 并核验后交付",
        );
        let tool = definition_with_semantic_and_contract(
            Some(store.clone()),
            None,
            Some(contract),
        );
        let context = flowy_agent_core::ToolRunContext {
            signal: flowy_agent_core::CancelToken::new(),
            agent_id: Some("s-cheat".to_owned()),
            cwd: Some(root.display().to_string()),
            todos: None,
            submission_id: Some("sub-cheat".to_owned()),
            session_origin: flowy_agent_core::SessionOrigin::Home,
            output_root: None,
            artifact_delivery_available: true,
        };
        // Worker also self-declares slides=2 to try to pass L1/L2 self-check.
        let args = flowy_agent_core::ToolCallArgs {
            call_id: "call-cheat".to_owned(),
            name: "verify_deck".to_owned(),
            arguments: json!({
                "path": "q3-east.pptx",
                "slides": 2,
                "anchors": [
                    {"anchor": "S0-title", "value": "华东区Q3汇报"},
                    {"anchor": "S0-body", "value": "上海6.1亿"},
                    {"anchor": "S1-title", "value": "华东区Q3汇报 · 2"},
                    {"anchor": "S1-body", "value": "江苏4.2亿"},
                ],
            }),
        };
        let ToolExecutionResult::Success { value, .. } =
            (tool.execute)(args, context).await
        else {
            panic!("failed verification must return a structured report");
        };
        assert_eq!(value["ok"], false);
        assert_eq!(value["next_action"], "clarify_or_rebuild");
        let issues = value["issues"].as_array().unwrap();
        assert!(
            issues.iter().any(|item| {
                item["code"] == STRUCTURE && item["field"] == "brief_min_slides"
            }),
            "host brief contract must block shrunken slide count: {issues:?}"
        );
        // Hard delivery gate must refuse the deck based on the persisted report.
        assert_eq!(
            store.check(&deck).unwrap(),
            Some(delivery::BlockReason::VerificationFailed)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
