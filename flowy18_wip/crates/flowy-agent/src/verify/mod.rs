//! Deterministic delivery-time verification for `.pptx`, `.docx`, and `.xlsx`.
//!
//! All three formats share one [`VerifyReport`] contract, one four-code anchor
//! vocabulary (`ANCHOR_MISSING`, `ANCHOR_DUPLICATE`, `ANCHOR_INVALID`,
//! `VALUE_MISMATCH`, plus L1 `STRUCTURE`), host-owned [`BriefContract`] checks,
//! and one persistence/telemetry path into [`DeliveryStore`].

use std::collections::{BTreeMap, HashMap};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flowy_agent_core::tools::ToolDefinition;
use flowy_agent_core::{
    RequiredDeliverable, ToolCallArgs, ToolExecutionResult, ToolRunContext,
    WorkContract,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::delivery::{self, DeliveryStore};
use crate::handle::now_ms;
use crate::semantic_verify::{
    SemanticContext, SemanticOutcome, probe_deck_semantics,
};

pub type VerifyFailureTracker = Arc<Mutex<HashMap<String, u32>>>;

pub const ANCHOR_MISSING: &str = "ANCHOR_MISSING";
pub const ANCHOR_DUPLICATE: &str = "ANCHOR_DUPLICATE";
pub const ANCHOR_INVALID: &str = "ANCHOR_INVALID";
pub const VALUE_MISMATCH: &str = "VALUE_MISMATCH";
pub const STRUCTURE: &str = "STRUCTURE";

/// Safety bound per decompressed XML entry inside an OOXML archive (16 MiB).
const MAX_XML_ENTRY_BYTES: u64 = 16 * 1024 * 1024;

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
        next_action: &str,
        contract_revision: Option<u32>,
        semantic_hint: Option<&str>,
    ) -> Value {
        json!({
            "summary": summary,
            "report_id": report_id,
            "artifact": self.artifact,
            "artifact_hash": artifact_hash,
            "contract_revision": contract_revision,
            "anchor": self.anchor,
            "ok": self.ok,
            "next_action": next_action,
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
        PROVENANCE_INVALID => "verify.provenance_invalid",
        _ => "verify.structure",
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeckAnchor {
    pub anchor: String,
    pub value: String,
}

mod contract;
mod ooxml;
mod provenance;
#[cfg(test)]
mod tests;

pub use self::contract::BriefContract;
use self::ooxml::{
    SlideL1Stats, VerifiedArtifact, push_unique, verify_deck_file_with_shapes,
    verify_docx_file, verify_xlsx_file,
};
pub use self::provenance::PROVENANCE_INVALID;
use self::provenance::ProvenanceClaim;

fn next_action_for(report: &VerifyReport) -> &'static str {
    if report.ok {
        "publish"
    } else if !report.issues.is_empty()
        && report
            .issues
            .iter()
            .all(|issue| issue.code == VALUE_MISMATCH)
    {
        "rerender_once"
    } else {
        "clarify_or_rebuild"
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "shared verification finalizer keeps report, verified bytes, revision, failure budget, and telemetry in one path"
)]
async fn finalize_report(
    path: &Path,
    raw_path: &str,
    report: VerifyReport,
    verified_bytes: &[u8],
    snippets: Option<&[(String, String)]>,
    provenance: &[ProvenanceClaim],
    contract_revision: Option<u32>,
    failures: Option<&VerifyFailureTracker>,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
) -> ToolExecutionResult {
    let provenance_vec = provenance.to_vec();
    let cwd_owned = context.cwd.clone().map(PathBuf::from);
    let signal_clone = context.signal.clone();
    let store_owned = store.cloned();
    let path_owned = path.to_path_buf();
    let verified_vec = verified_bytes.to_vec();
    let mut report_for_blocking = report;
    let (report, provenance_warnings, report_id, artifact_hash) =
        tokio::task::spawn_blocking(move || {
            let provenance_eval = provenance::evaluate_provenance_with_signal(
                &provenance_vec,
                cwd_owned.as_deref(),
                Some(&signal_clone),
            );
            for issue in provenance_eval.issues {
                push_unique(
                    &mut report_for_blocking.issues,
                    issue.code,
                    &issue.field,
                );
            }
            report_for_blocking.ok = report_for_blocking.issues.is_empty();
            let provenance_warnings = provenance_eval.warnings;
            let mut report_id = String::new();
            let mut artifact_hash = delivery::hash_bytes(&verified_vec);
            if let Some(store) = &store_owned {
                let issues = report_for_blocking
                    .issues
                    .iter()
                    .map(|issue| delivery::Issue {
                        code: issue.code.to_owned(),
                        field: issue.field.clone(),
                    })
                    .collect();
                match store.record_report_bytes_with_revision(
                    &path_owned,
                    &verified_vec,
                    issues,
                    contract_revision,
                    now_ms(),
                ) {
                    Ok(persisted) => {
                        report_id = persisted.id;
                        artifact_hash = persisted.artifact_hash;
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "flowy.delivery",
                            %error,
                            path = %path_owned.display(),
                            "could not persist the verification report"
                        );
                    }
                }
            }
            (
                report_for_blocking,
                provenance_warnings,
                report_id,
                artifact_hash,
            )
        })
        .await
        .unwrap_or_else(|_| {
            (
                VerifyReport {
                    artifact: path.display().to_string(),
                    anchor: "shape_name",
                    ok: false,
                    issues: vec![VerifyIssue {
                        code: STRUCTURE,
                        field: "verify_task_failed".to_owned(),
                    }],
                },
                Vec::new(),
                String::new(),
                String::new(),
            )
        });
    // Hard cap on rerender_once: the second failed verification of the same
    // artifact within one turn locks next_action to clarify_or_rebuild.
    let mut next_action = next_action_for(&report);
    if !report.ok
        && let Some(tracker) = failures
    {
        let key = delivery::artifact_key(path);
        let next_count = {
            let mut map = tracker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let next = map.get(&key).copied().unwrap_or(0).saturating_add(1);
            map.insert(key, next);
            next
        };
        if next_count >= 2 {
            next_action = "clarify_or_rebuild";
        }
    }
    let artifact_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(raw_path);
    let (l3_outcome, l3_duration_ms) = if report.ok
        && let (Some(semantic_ctx), Some(extracted)) = (semantic, snippets)
        && !extracted.is_empty()
    {
        probe_deck_semantics(
            semantic_ctx,
            artifact_name,
            extracted,
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
        contract_revision = contract_revision.unwrap_or(0),
        anchor = report.anchor,
        ok = report.ok,
        next_action,
        issue_count = report.issues.len(),
        provenance_warning_count = provenance_warnings.len(),
        l3_status = l3_outcome.status_code(),
        l3_reason = l3_outcome.reason(),
        l3_issue_count = l3_outcome.issue_count(),
        l3_duration_ms,
        "artifact verification completed"
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
    let mut value = report.to_value_with_meta(
        &summary,
        &report_id,
        &artifact_hash,
        next_action,
        contract_revision,
        semantic_hint.as_deref(),
    );
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "provenance_warnings".to_owned(),
            json!(provenance_warnings),
        );
    }
    let content = serde_json::to_string(&value).unwrap_or(summary);
    ToolExecutionResult::success_text(content, value)
}

async fn execute_deck(
    arguments: Value,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
    contract: Option<&BriefContract>,
    failures: Option<&VerifyFailureTracker>,
) -> ToolExecutionResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        path: String,
        slides: usize,
        anchors: Vec<DeckAnchor>,
        #[serde(default)]
        provenance: Vec<ProvenanceClaim>,
    }
    let Ok(args) = serde_json::from_value::<Args>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path, slides and anchors",
        );
    };
    let path = match crate::pptx::resolve_confined_turn_path(
        context.cwd.as_deref(),
        context.output_root.as_deref(),
        &args.path,
    ) {
        Ok(path) => path,
        Err(error) => {
            return ToolExecutionResult::error("INVALID_PATH", error);
        }
    };
    let revision = contract.map(|c| c.revision.max(1));
    let verify_path = path.clone();
    let anchors_owned = args.anchors.clone();
    let contract_owned = contract.cloned();
    let slides = args.slides;
    let verified_result = tokio::task::spawn_blocking(move || {
        verify_deck_file_with_shapes(
            &verify_path,
            slides,
            &anchors_owned,
            contract_owned.as_ref(),
        )
    })
    .await
    .unwrap_or_else(|error| Err(format!("verify task failed: {error}")));
    match verified_result {
        Ok(verified) => {
            finalize_report(
                &path,
                &args.path,
                verified.report,
                &verified.bytes,
                Some(&verified.shapes),
                &args.provenance,
                revision,
                failures,
                context,
                store,
                semantic,
            )
            .await
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

async fn execute_artifact(
    arguments: Value,
    context: &ToolRunContext,
    store: Option<&DeliveryStore>,
    semantic: Option<&SemanticContext>,
    contract: Option<&BriefContract>,
    failures: Option<&VerifyFailureTracker>,
) -> ToolExecutionResult {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        path: String,
        #[serde(default)]
        slides: Option<usize>,
        #[serde(default)]
        anchors: Vec<DeckAnchor>,
        #[serde(default)]
        provenance: Vec<ProvenanceClaim>,
    }
    let Ok(args) = serde_json::from_value::<Args>(arguments) else {
        return ToolExecutionResult::error(
            "INVALID_ARGS",
            "expected path and optional slides/anchors",
        );
    };
    let path = match crate::pptx::resolve_confined_turn_path(
        context.cwd.as_deref(),
        context.output_root.as_deref(),
        &args.path,
    ) {
        Ok(path) => path,
        Err(error) => {
            return ToolExecutionResult::error("INVALID_PATH", error);
        }
    };
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let revision = contract.map(|c| c.revision.max(1));
    if !matches!(ext.as_deref(), Some("pptx" | "docx" | "xlsx")) {
        return ToolExecutionResult::error(
            "UNSUPPORTED_FORMAT",
            format!(
                "no deterministic verifier for {}; supported: .pptx, .docx, .xlsx",
                args.path
            ),
        );
    }
    let verify_path = path.clone();
    let anchors_owned = args.anchors.clone();
    let contract_owned = contract.cloned();
    let slides = args.slides;
    let ext_owned = ext.clone();
    let result = tokio::task::spawn_blocking(move || match ext_owned.as_deref()
    {
        Some("pptx") => match slides {
            Some(slides) => verify_deck_file_with_shapes(
                &verify_path,
                slides,
                &anchors_owned,
                contract_owned.as_ref(),
            )
            .map(|verified| VerifiedArtifact {
                report: verified.report,
                bytes: verified.bytes,
                snippets: verified.shapes,
            }),
            None => Err(
                "pptx verification requires expected slide count (`slides`)"
                    .to_owned(),
            ),
        },
        Some("docx") => {
            verify_docx_file(&verify_path, &anchors_owned, contract_owned.as_ref())
        }
        Some("xlsx") => {
            verify_xlsx_file(&verify_path, &anchors_owned, contract_owned.as_ref())
        }
        _ => Err("unsupported format".to_owned()),
    })
    .await
    .unwrap_or_else(|error| Err(format!("verify task failed: {error}")));
    match result {
        Ok(verified) => {
            finalize_report(
                &path,
                &args.path,
                verified.report,
                &verified.bytes,
                Some(&verified.snippets),
                &args.provenance,
                revision,
                failures,
                context,
                store,
                semantic,
            )
            .await
        }
        Err(error) => ToolExecutionResult::error(
            "VERIFY_FAILED",
            format!("cannot verify {}: {error}", args.path),
        ),
    }
}

fn provenance_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "field": {"type": "string", "minLength": 1},
                "kind": {"type": "string", "enum": ["user_file", "fetched", "browser", "model_recall"]},
                "path": {"type": "string"},
                "sha256": {"type": "string"},
                "note": {"type": "string"}
            },
            "required": ["field", "kind"],
            "additionalProperties": false
        }
    })
}

#[cfg(test)]
pub fn definition(store: Option<DeliveryStore>) -> Arc<ToolDefinition> {
    definition_with_options(store, None, None, None)
}

#[cfg(test)]
pub fn definition_with_semantic_and_contract(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
) -> Arc<ToolDefinition> {
    definition_with_options(store, semantic, contract, None)
}

pub fn definition_with_options(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
    failures: Option<VerifyFailureTracker>,
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
                },
                "provenance": provenance_schema()
            },
            "required": ["path", "slides", "anchors"],
            "additionalProperties": false
        }),
        move |args: ToolCallArgs, context: ToolRunContext| {
            let store = store.clone();
            let semantic = semantic.clone();
            let contract = contract.clone();
            let failures = failures.clone();
            Box::pin(async move {
                execute_deck(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    semantic.as_ref(),
                    contract.as_ref(),
                    failures.as_ref(),
                )
                .await
            })
        },
    ))
}

#[cfg(test)]
pub fn artifact_definition(
    store: Option<DeliveryStore>,
    contract: Option<BriefContract>,
) -> Arc<ToolDefinition> {
    artifact_definition_with_options(store, None, contract, None)
}

pub fn artifact_definition_with_options(
    store: Option<DeliveryStore>,
    semantic: Option<SemanticContext>,
    contract: Option<BriefContract>,
    failures: Option<VerifyFailureTracker>,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "verify_artifact",
        "Verify a generated .docx, .xlsx, or .pptx artifact before calling publish_file. Checks OOXML structure, non-empty content, host brief constraints, and required non-empty anchors (Word bookmark names for .docx; row_key, 'row_key:ColumnHeader', 'Sheet1!B2', or cell ref like 'B2' for .xlsx; shape names + slides for .pptx). Persists a delivery gate report; a failed report is not a tool error.",
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
                },
                "provenance": provenance_schema()
            },
            "required": ["path", "anchors"],
            "additionalProperties": false
        }),
        move |args: ToolCallArgs, context: ToolRunContext| {
            let store = store.clone();
            let semantic = semantic.clone();
            let contract = contract.clone();
            let failures = failures.clone();
            Box::pin(async move {
                execute_artifact(
                    args.arguments,
                    &context,
                    store.as_ref(),
                    semantic.as_ref(),
                    contract.as_ref(),
                    failures.as_ref(),
                )
                .await
            })
        },
    ))
}
