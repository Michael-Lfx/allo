//! Session-level lightweight Router tools, child Work summaries, ownership
//! validation, idempotent dispatch, and bounded read-only status header.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use flowy_agent_core::{
    AcceptanceQueue, BranchHeartbeat, DisambiguationOption, DispatchRecord,
    Engine, FactStatus, ObjectVerifyState, PendingCard, PreToolDecision,
    PreToolHook, ReceiptState, SessionOrigin, SessionRecord, SessionRole,
    SessionWorkspace, StopHandoffManifest, SubmissionOutcome, TrackedObject,
    TrackedObjectKind, TurnState, WorkAccessMode,
    tools::{ToolDefinition, ToolRegistry},
    types::{CancelToken, ToolCallArgs, ToolExecutionResult, ToolRunContext},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    AgentUpdate, ChatProjection, Harness, Live, Ordering, TurnStatus, lock,
    log_storage_io_error, next_stamp, now_ms, persist_index,
};
use crate::artifact_store::PublicationState;

/// Hard character budget for the Router read-only child-Work status header.
const MAX_STATUS_HEADER_CHARS: usize = 900;
/// Maximum recent child Works projected into the Router read-only header.
const MAX_HEADER_WORKS: usize = 8;
const MAX_ROUTER_PROVIDER_HINTS: usize = 8;
const MAX_ROUTER_PROVIDER_HINT_CHARS: usize = 160;
const MAX_ROUTER_PROVIDER_TOTAL_CHARS: usize = 600;

/// Structured result returned by `dispatch_work` and `revise_work`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchWorkResult {
    pub work_id: String,
    pub receipt_id: String,
    pub status: String,
    pub access_mode: WorkAccessMode,
    pub deduplicated: bool,
}

/// Structured result returned by `stop_router_reply` and `cancel_work`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopWorkResult {
    pub work_id: String,
    pub outcome: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cancelled_receipt_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub committed_artifact_ids: Vec<String>,
}

/// Authoritative host summary of one Worker child session owned by a Router.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildWorkSummary {
    pub work_id: String,
    pub root_session_id: String,
    pub title: String,
    pub access_mode: WorkAccessMode,
    pub origin: SessionOrigin,
    pub workspace_root: Option<PathBuf>,
    pub state: String,
    pub turn_status: TurnStatus,
    pub contract_revision: Option<u32>,
    pub heartbeat: Option<BranchHeartbeat>,
    pub pending_cards: Vec<PendingCard>,
    pub delivered_artifact_ids: Vec<String>,
    pub missing_items: Vec<String>,
    pub objects: Vec<TrackedObject>,
    pub last_stop_handoff: Option<StopHandoffManifest>,
    pub updated_at: u64,
}

/// Pending interactive card routed to a root conversation (either owned by the
/// Router itself or surfaced from one of its child Workers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutedPendingCard {
    pub root_session_id: String,
    pub owner_session_id: String,
    pub owner_role: SessionRole,
    pub card: PendingCard,
}

/// Parsed request handed from the `dispatch_work` tool to [`Harness`].
#[derive(Clone, Debug)]
pub(super) struct DispatchWorkRequest {
    pub(super) brief: String,
    pub(super) requested_mode: Option<WorkAccessMode>,
    pub(super) target_work_id: Option<String>,
    pub(super) target_object_ids: Vec<String>,
    pub(super) request_id: Option<String>,
}

/// Verify that `worker_id` exists, has [`SessionRole::Worker`], and belongs to
/// `router_id` (enforcing strict cross-root isolation).
pub(super) fn verify_child_ownership(
    engine: &Engine,
    router_id: &str,
    worker_id: &str,
) -> Result<SessionRecord, &'static str> {
    let Some(worker) = engine.session(worker_id) else {
        return Err("WORK_NOT_FOUND");
    };
    if worker.role != SessionRole::Worker
        || worker.root_session_id.as_deref() != Some(router_id)
    {
        return Err("WORK_OWNERSHIP_DENIED");
    }
    Ok(worker)
}

/// Map every `object_id` in `object_ids` to its owning child Worker under
/// `router_id`, rejecting unknown or cross-root object IDs fail-closed.
pub(super) fn resolve_owned_objects(
    engine: &Engine,
    router_id: &str,
    object_ids: &[String],
) -> Result<BTreeMap<String, String>, &'static str> {
    if object_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let children = engine.child_sessions(router_id);
    let mut owners = BTreeMap::new();
    for oid in object_ids {
        let owner = children.iter().find_map(|child| {
            child
                .work_contract
                .as_ref()
                .and_then(|wc| wc.find_object(oid))
                .map(|_| child.id.clone())
        });
        let Some(worker_id) = owner else {
            return Err("OBJECT_OWNERSHIP_DENIED");
        };
        owners.insert(oid.clone(), worker_id);
    }
    Ok(owners)
}

/// Locate which `(session_id, effective_root_id)` owns `object_id` anywhere in
/// the engine so `ask_user_choice` can reject foreign-root object references.
fn find_object_owner_globally(
    engine: &Engine,
    object_id: &str,
) -> Option<(String, String)> {
    engine.with_sessions(|records| {
        records.iter().find_map(|rec| {
            rec.work_contract
                .as_ref()
                .and_then(|wc| wc.find_object(object_id))
                .map(|_| {
                    (rec.id.clone(), rec.effective_root_session_id().to_owned())
                })
        })
    })
}

/// Compute a canonical SHA-256 fingerprint for idempotent dispatch / revise
/// requests (AWS idempotency-key semantics).
pub(super) fn dispatch_fingerprint(
    action: &str,
    target_work_id: Option<&str>,
    mode: Option<WorkAccessMode>,
    brief: &str,
    target_object_ids: &[String],
) -> String {
    let mut sorted_oids: Vec<&str> =
        target_object_ids.iter().map(String::as_str).collect();
    sorted_oids.sort_unstable();
    sorted_oids.dedup();
    let mode_str = match mode {
        Some(WorkAccessMode::ReadOnly) => "read_only",
        Some(WorkAccessMode::ReadWrite) => "read_write",
        None => "inherit_or_default",
    };
    let payload = json!({
        "action": action,
        "brief": brief.trim(),
        "mode": mode_str,
        "target_object_ids": sorted_oids,
        "target_work_id": target_work_id.unwrap_or(""),
    });
    let encoded = serde_json::to_string(&payload).unwrap_or_default();
    crate::delivery::hash_bytes(encoded.as_bytes())
}

fn parse_work_access_mode(
    raw: Option<&str>,
) -> Result<Option<WorkAccessMode>, &'static str> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some("read_only" | "readonly" | "read") => {
            Ok(Some(WorkAccessMode::ReadOnly))
        }
        Some("read_write" | "readwrite" | "write") => {
            Ok(Some(WorkAccessMode::ReadWrite))
        }
        Some(_) => Err("INVALID_ACCESS_MODE"),
    }
}

fn next_choice_card_id(now: u64) -> String {
    format!("choice-{now}-{}", next_stamp())
}

fn arg_str<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).map_or("", str::trim)
}

fn arg_opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Build the Router's dedicated [`PreToolHook`] intercepting `ask_user_choice`
/// and suspending the turn immediately (`PreToolDecision::SuspendWithCard`).
pub(super) fn build_router_pre_tool_hook(
    harness: Weak<Harness>,
    router_id: String,
) -> PreToolHook {
    Arc::new(move |tool_name: &str, args: &Value| {
        if tool_name != "ask_user_choice" {
            return PreToolDecision::Allow;
        }
        let prompt = arg_str(args, "prompt");
        if prompt.is_empty() {
            return PreToolDecision::Deny {
                reason: "INVALID_ARGS: ask_user_choice requires a non-empty `prompt`".to_owned(),
            };
        }
        let impact_boundary = arg_opt_str(args, "impact_boundary")
            .unwrap_or_else(|| {
                "仅执行你确认的选项，未选对象保持原样".to_owned()
            });
        let allow_multi = args
            .get("allow_multi")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(raw_options) = args.get("options").and_then(Value::as_array)
        else {
            return PreToolDecision::Deny {
                reason: "INVALID_ARGS: ask_user_choice requires a non-empty `options` array".to_owned(),
            };
        };
        if raw_options.is_empty() {
            return PreToolDecision::Deny {
                reason:
                    "INVALID_ARGS: ask_user_choice `options` must not be empty"
                        .to_owned(),
            };
        }
        let engine = harness.upgrade().and_then(|h| lock(&h.engine).clone());
        let mut seen_ids = BTreeSet::new();
        let mut options = Vec::with_capacity(raw_options.len());
        for raw in raw_options {
            let object_id = arg_str(raw, "object_id");
            let label = arg_str(raw, "label");
            if object_id.is_empty() || label.is_empty() {
                return PreToolDecision::Deny {
                    reason: "INVALID_ARGS: each option requires non-empty `object_id` and `label`".to_owned(),
                };
            }
            if !seen_ids.insert(object_id.to_owned()) {
                return PreToolDecision::Deny {
                    reason: format!(
                        "INVALID_ARGS: duplicate option object_id `{object_id}`"
                    ),
                };
            }
            if let Some(eng) = &engine
                && let Some((_, owner_root)) =
                    find_object_owner_globally(eng, object_id)
                && owner_root != router_id
            {
                return PreToolDecision::Deny {
                    reason: format!(
                        "OBJECT_OWNERSHIP_DENIED: option `{object_id}` belongs to another conversation"
                    ),
                };
            }
            let anchor_summary = raw
                .get("anchor_summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let extension = raw
                .get("extension")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            options.push(DisambiguationOption {
                object_id: object_id.to_owned(),
                label: label.to_owned(),
                anchor_summary,
                extension,
            });
        }
        let now = now_ms();
        let card = PendingCard::Disambiguation {
            card_id: next_choice_card_id(now),
            created_at_ms: now,
            prompt: prompt.to_owned(),
            impact_boundary,
            options,
            allow_multi,
            dispatch_receipt_id: None,
        };
        PreToolDecision::SuspendWithCard {
            card,
            reason: "USER_CHOICE_REQUIRED: awaiting explicit user selection"
                .to_owned(),
        }
    })
}

/// Assemble the Router's lightweight tool set.
///
/// - Normal Router turns expose only the 4 control tools (`dispatch_work`,
///   `query_work_status`, `control_work`, `ask_user_choice`).
/// - Automatic `[WorkEventBatch]` announcement turns physically strip
///   `dispatch_work`, `control_work`, and `ask_user_choice`, leaving only
///   read-only `query_work_status` to prevent failure-announcement retry storms.
pub(super) fn router_tool_registry(
    harness: Weak<Harness>,
    router_id: String,
    is_announcement: bool,
) -> ToolRegistry {
    let registry = ToolRegistry::new();
    registry.register(query_work_status_tool(
        Weak::clone(&harness),
        router_id.clone(),
    ));
    if !is_announcement {
        registry.register(dispatch_work_tool(
            Weak::clone(&harness),
            router_id.clone(),
        ));
        registry.register(control_work_tool(Weak::clone(&harness), router_id));
        registry.register(ask_user_choice_tool());
    }
    registry
}

fn dispatch_work_tool(
    harness: Weak<Harness>,
    router_id: String,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "dispatch_work",
        "Delegate a file, shell, verification, or multi-step task to an asynchronous Worker session. Returns immediately with work_id and receipt_id.",
        json!({
            "type": "object",
            "properties": {
                "brief": { "type": "string", "description": "Clear, self-contained task brief for the Worker." },
                "mode": { "type": "string", "enum": ["read_only", "read_write"], "description": "Workspace access mode. `read_only` ONLY exposes `read_file`/`glob`/`grep` (no shell/curl/extensions) and can run concurrently. Tasks needing shell commands, network/curl calls, extension/automation providers, file edits, or artifact creation MUST use `read_write` (even for read-only extension queries)." },
                "target_work_id": { "type": "string", "description": "Existing child Worker ID to continue (keeps original origin, workspace, and access_mode)." },
                "target_object_ids": { "type": "array", "items": { "type": "string" }, "description": "Optional object IDs owned by child Workers to locate and target the exact owning Worker." },
                "request_id": { "type": "string", "description": "Caller-supplied idempotency key preventing duplicate work creation on retries." }
            },
            "required": ["brief"]
        }),
        move |args: ToolCallArgs, _ctx: ToolRunContext| {
            let harness = Weak::clone(&harness);
            let router_id = router_id.clone();
            Box::pin(async move {
                let Some(harness) = harness.upgrade() else {
                    return ToolExecutionResult::error(
                        "HOST_UNAVAILABLE",
                        "agent host is shutting down",
                    );
                };
                let brief = arg_str(&args.arguments, "brief").to_owned();
                if brief.is_empty() {
                    return ToolExecutionResult::error(
                        "INVALID_ARGS",
                        "`brief` must be a non-empty string",
                    );
                }
                let requested_mode = match parse_work_access_mode(
                    args.arguments.get("mode").and_then(Value::as_str),
                ) {
                    Ok(mode) => mode,
                    Err(code) => {
                        return ToolExecutionResult::error(
                            code,
                            "`mode` must be `read_only` or `read_write`",
                        );
                    }
                };
                let target_work_id =
                    arg_opt_str(&args.arguments, "target_work_id");
                let target_object_ids: Vec<String> = args
                    .arguments
                    .get("target_object_ids")
                    .and_then(Value::as_array)
                    .map_or_default(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .collect()
                    });
                let request_id = arg_opt_str(&args.arguments, "request_id");
                let req = DispatchWorkRequest {
                    brief,
                    requested_mode,
                    target_work_id,
                    target_object_ids,
                    request_id,
                };
                let h = Arc::clone(&harness);
                let rid = router_id.clone();
                match tokio::task::spawn_blocking(move || {
                    h.dispatch_work_from_router(&rid, req)
                })
                .await
                {
                    Ok(Ok(result)) => ToolExecutionResult::success_value(
                        serde_json::to_value(result)
                            .unwrap_or_else(|_| json!({})),
                    ),
                    Ok(Err(code)) => ToolExecutionResult::error(
                        code,
                        format!("dispatch_work rejected: {code}"),
                    ),
                    Err(err) => ToolExecutionResult::error(
                        "INTERNAL_ERROR",
                        err.to_string(),
                    ),
                }
            })
        },
    ))
}

/// Child Work summaries and conversation automations for `query_work_status`,
/// or a tool error code with its message.
type WorkStatusLists = Result<(Vec<Value>, Vec<Value>), (&'static str, String)>;

fn query_work_status_tool(
    harness: Weak<Harness>,
    router_id: String,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "query_work_status",
        "Read authoritative host status of child Works (state, verification, deliverables, records, pending approvals/cards, and stop handoffs). `automations` lists this conversation's scheduled automations as saved by the host, each with `next_run_local` and its `last_run`.",
        json!({
            "type": "object",
            "properties": {
                "work_id": { "type": "string", "description": "Optional specific child Worker ID to inspect." }
            }
        }),
        move |args: ToolCallArgs, _ctx: ToolRunContext| {
            let harness = Weak::clone(&harness);
            let router_id = router_id.clone();
            Box::pin(async move {
                let Some(harness) = harness.upgrade() else {
                    return ToolExecutionResult::error(
                        "HOST_UNAVAILABLE",
                        "agent host is shutting down",
                    );
                };
                let filter_work_id = arg_opt_str(&args.arguments, "work_id");
                let h = Arc::clone(&harness);
                let rid = router_id.clone();
                let res = tokio::task::spawn_blocking(
                    move || -> WorkStatusLists {
                        if let Some(work_id) = filter_work_id.as_deref() {
                            let engine =
                                lock(&h.engine).clone().ok_or_else(|| {
                                    (
                                        "HOST_UNAVAILABLE",
                                        "engine unavailable".to_owned(),
                                    )
                                })?;
                            verify_child_ownership(&engine, &rid, work_id)
                                .map_err(|code| {
                                    (
                                        code,
                                        format!(
                                            "cannot query work `{work_id}`: {code}"
                                        ),
                                    )
                                })?;
                        }
                        let works = h.child_works(&rid);
                        Ok(works
                            .into_iter()
                            .filter(|w| {
                                filter_work_id
                                    .as_deref()
                                    .is_none_or(|id| w.work_id == id)
                            })
                            .map(child_work_to_json)
                            .collect::<Vec<_>>())
                        .map(|works| (works, h.conversation_automations(&rid)))
                    },
                )
                .await;
                match res {
                    Ok(Ok((filtered, automations))) => {
                        ToolExecutionResult::success_value(json!({
                            "root_session_id": router_id,
                            "works": filtered,
                            "automations": automations,
                        }))
                    }
                    Ok(Err((code, msg))) => {
                        ToolExecutionResult::error(code, msg)
                    }
                    Err(err) => ToolExecutionResult::error(
                        "INTERNAL_ERROR",
                        err.to_string(),
                    ),
                }
            })
        },
    ))
}

fn control_work_tool(
    harness: Weak<Harness>,
    router_id: String,
) -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "control_work",
        "Cancel or revise an owned child Worker. Use `cancel` to stop queued/running/waiting work, or `revise` to safely settle and re-dispatch with updated instructions.",
        json!({
            "type": "object",
            "properties": {
                "work_id": { "type": "string" },
                "action": { "type": "string", "enum": ["cancel", "revise"] },
                "revision_brief": { "type": "string", "description": "Required when action is `revise`." },
                "request_id": { "type": "string", "description": "Idempotency key for `revise`." }
            },
            "required": ["work_id", "action"]
        }),
        move |args: ToolCallArgs, _ctx: ToolRunContext| {
            let harness = Weak::clone(&harness);
            let router_id = router_id.clone();
            Box::pin(async move {
                let Some(harness) = harness.upgrade() else {
                    return ToolExecutionResult::error(
                        "HOST_UNAVAILABLE",
                        "agent host is shutting down",
                    );
                };
                let work_id = arg_str(&args.arguments, "work_id").to_owned();
                let action = arg_str(&args.arguments, "action").to_owned();
                if work_id.is_empty() {
                    return ToolExecutionResult::error(
                        "INVALID_ARGS",
                        "`work_id` is required",
                    );
                }
                match action.as_str() {
                    "cancel" => {
                        let h = Arc::clone(&harness);
                        let rid = router_id.clone();
                        let wid = work_id.clone();
                        let res = tokio::task::spawn_blocking(move || {
                            h.cancel_work(&rid, &wid)
                        })
                        .await;
                        match res {
                            Ok(Ok(stop)) => ToolExecutionResult::success_value(
                                serde_json::to_value(stop)
                                    .unwrap_or_else(|_| json!({})),
                            ),
                            Ok(Err(code)) => ToolExecutionResult::error(
                                code,
                                format!("cancel_work failed: {code}"),
                            ),
                            Err(err) => ToolExecutionResult::error(
                                "INTERNAL_ERROR",
                                err.to_string(),
                            ),
                        }
                    }
                    "revise" => {
                        let brief = arg_str(&args.arguments, "revision_brief")
                            .to_owned();
                        if brief.is_empty() {
                            return ToolExecutionResult::error(
                                "INVALID_ARGS",
                                "`revision_brief` is required for `revise`",
                            );
                        }
                        let request_id =
                            arg_opt_str(&args.arguments, "request_id");
                        let h = Arc::clone(&harness);
                        let rid = router_id.clone();
                        let wid = work_id.clone();
                        let res = tokio::task::spawn_blocking(move || {
                            h.revise_work(
                                &rid,
                                &wid,
                                &brief,
                                request_id.as_deref(),
                            )
                        })
                        .await;
                        match res {
                            Ok(Ok(dispatched)) => {
                                ToolExecutionResult::success_value(
                                    serde_json::to_value(dispatched)
                                        .unwrap_or_else(|_| json!({})),
                                )
                            }
                            Ok(Err(code)) => ToolExecutionResult::error(
                                code,
                                format!("revise_work failed: {code}"),
                            ),
                            Err(err) => ToolExecutionResult::error(
                                "INTERNAL_ERROR",
                                err.to_string(),
                            ),
                        }
                    }
                    _ => ToolExecutionResult::error(
                        "INVALID_ARGS",
                        "`action` must be `cancel` or `revise`",
                    ),
                }
            })
        },
    ))
}

fn ask_user_choice_tool() -> Arc<ToolDefinition> {
    Arc::new(ToolDefinition::new(
        "ask_user_choice",
        "Pause and present a durable disambiguation/choice card to the user before executing an ambiguous modification.",
        json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string" },
                "impact_boundary": { "type": "string" },
                "allow_multi": { "type": "boolean" },
                "options": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "object_id": { "type": "string" },
                            "label": { "type": "string" },
                            "anchor_summary": { "type": "string" },
                            "extension": { "type": "string" }
                        },
                        "required": ["object_id", "label"]
                    }
                }
            },
            "required": ["prompt", "options"]
        }),
        |_args: ToolCallArgs, _ctx: ToolRunContext| {
            Box::pin(async {
                ToolExecutionResult::error(
                    "HOST_HOOK_BYPASSED",
                    "ask_user_choice must be suspended by host pre_tool_hook",
                )
            })
        },
    ))
}

fn child_work_to_json(summary: ChildWorkSummary) -> Value {
    let objects: Vec<Value> = summary
        .objects
        .iter()
        .map(|obj| {
            let confirmed: BTreeMap<&str, &str> = obj
                .overrides
                .iter()
                .filter(|(_, e)| e.status == FactStatus::Confirmed)
                .map(|(k, e)| (k.as_str(), e.value.as_str()))
                .collect();
            let pending_proposals: BTreeMap<&str, &str> = obj
                .overrides
                .iter()
                .filter(|(_, e)| e.status == FactStatus::PendingProposal)
                .map(|(k, e)| (k.as_str(), e.value.as_str()))
                .collect();
            let verify_state = match &obj.verify_state {
                ObjectVerifyState::Unverified => "unverified".to_owned(),
                ObjectVerifyState::Passed {
                    report_id,
                    revision,
                } => format!("passed(rev={revision},report={report_id})"),
                ObjectVerifyState::Failed { report_id } => {
                    format!("failed(report={report_id})")
                }
                ObjectVerifyState::Overridden { report_id } => {
                    format!("overridden(report={report_id})")
                }
            };
            json!({
                "object_id": obj.object_id,
                "kind": match obj.kind {
                    TrackedObjectKind::Deliverable => "deliverable",
                    TrackedObjectKind::Record => "record",
                },
                "label": obj.display_label(),
                "filename": obj.filename,
                "extension": obj.extension,
                "anchor_summary": obj.anchor_summary,
                "verify_state": verify_state,
                "confirmed_facts": confirmed,
                "pending_proposals": pending_proposals,
            })
        })
        .collect();
    let pending_cards: Vec<Value> = summary
        .pending_cards
        .iter()
        .map(|card| match card {
            PendingCard::ToolApproval {
                card_id,
                tool_name,
                target_summary,
                risk_reason,
                ..
            } => json!({
                "kind": "tool_approval",
                "card_id": card_id,
                "tool_name": tool_name,
                "target_summary": target_summary,
                "risk_reason": risk_reason,
            }),
            PendingCard::Disambiguation {
                card_id,
                prompt,
                options,
                ..
            } => json!({
                "kind": "disambiguation",
                "card_id": card_id,
                "prompt": prompt,
                "option_count": options.len(),
            }),
        })
        .collect();
    json!({
        "work_id": summary.work_id,
        "title": summary.title,
        "access_mode": summary.access_mode,
        "state": summary.state,
        "contract_revision": summary.contract_revision,
        "heartbeat": summary.heartbeat,
        "delivered_artifact_ids": summary.delivered_artifact_ids,
        "missing_items": summary.missing_items,
        "pending_cards": pending_cards,
        "objects": objects,
        "last_stop_handoff": summary.last_stop_handoff,
    })
}

/// Build the Router's system prompt plus a bounded (~200 tokens / 900 chars)
/// read-only status header summarizing up to 8 recent child Works.
pub(super) fn build_router_system_prompt(
    harness: &Harness,
    router_id: &str,
    is_announcement: bool,
) -> String {
    let mut out = format!(
        "{} You are Flowy's lightweight conversation Router. Follow strict turn discipline: (1) Before calling tools or `dispatch_work`, emit at most ONE short opening acknowledgement sentence — never write filler narration between tool calls (no \"已派发，我看一下结果\" / \"还在跑，稍等我再查\"). (2) Delegate file editing, document creation, shell/network commands (`curl`), and verification to Worker sessions via `dispatch_work`; note `read_only` Workers only have `read_file`/`glob`/`grep` (no shell), so shell/network queries require `read_write` with read-only constraints in the brief. (3) After dispatching background Workers, stop and yield the turn immediately so Workers run asynchronously — do NOT poll `query_work_status` repeatedly in a loop unless the user explicitly asked for an immediate status check. (4) Never claim a file is verified or ready to send unless `query_work_status` reports verified/delivered evidence. Scheduled automations produce no deliverable: their evidence is `query_work_status.automations`, read by the host. An entry there means it is saved; report its title, schedule and `next_run_local` exactly as given (never convert `*_ms` timestamps yourself), and use its `last_run` for test-run status. Clearly distinguish formal Deliverables from archival Records and Confirmed facts from PendingProposals. Before ambiguous edits across multiple deliverables, call `ask_user_choice` instead of guessing. To create, change, pause, delete or review scheduled automations, dispatch a `read_write` Worker: only it has the automation tools, and automations it creates belong to this conversation, run here on their schedule and report back here.",
        super::BASE_BEHAVIOR_PROMPT
    );
    out.push_str(
        "\nIf the user asks to do work at a future time or on a recurring schedule, dispatch a read_write Worker to save the automation only; it must not perform the task itself. Include the schedule, requested action, and result requirements in its brief. If a time could mean either when to run or which data to look up, clarify before dispatching. If the user already asked to run or test it now, also tell that Worker to call `automation_run_now` right after saving; the test run is separate work in this conversation and is announced here when it ends. Otherwise, once `query_work_status` lists it under `automations`, confirm the schedule with its `next_run_local` and end with one short question asking whether to run it once now as a test. If the user agrees, dispatch a read_write Worker whose brief is only to call `automation_run_now` for that automation (it can find it by title with `automation_list`), not to perform the task itself.",
    );
    let works = harness.child_works(router_id);
    if is_announcement {
        let (active_running, waiting_user) =
            super::work_events::sibling_work_counts(&works);
        if active_running > 0 && waiting_user == 0 {
            out.push_str(
                "\nThis is an automatic `[WorkEventBatch]` progress announcement turn while other sibling Workers are STILL running (`active_running > 0`). Emit ONLY ONE short progress sentence reporting the completed/failed item and remaining background work — do NOT write a multi-section wrap-up, options list, or final conclusion until sibling Workers settle, and do not dispatch or retry work.",
            );
        } else {
            out.push_str(
                "\nThis is an automatic `[WorkEventBatch]` progress announcement turn. Summarize the authoritative Worker state honestly for the user without dispatching or retrying work.",
            );
        }
    }
    append_worker_capabilities_section(harness, &mut out);
    if works.is_empty() {
        return out;
    }
    let mut header = String::from("\n\n[ChildWorksReadOnlySnapshot]\n");
    for work in works.iter().take(MAX_HEADER_WORKS) {
        let mode = match work.access_mode {
            WorkAccessMode::ReadOnly => "ro",
            WorkAccessMode::ReadWrite => "rw",
        };
        let rev = work
            .contract_revision
            .map_or_else(|| "-".to_owned(), |r| format!("v{r}"));
        let cards = work.pending_cards.len();
        let delivered = work.delivered_artifact_ids.len();
        let obj_tags: Vec<String> = work
            .objects
            .iter()
            .take(3)
            .map(|o| {
                let k = match o.kind {
                    TrackedObjectKind::Deliverable => "D",
                    TrackedObjectKind::Record => "R",
                };
                format!("{}:{k}:{}", o.object_id, o.display_label())
            })
            .collect();
        let line = format!(
            "- {}|{}|{}|{}|deliv={}|cards={}|objs=[{}]\n",
            work.work_id,
            mode,
            work.state,
            rev,
            delivered,
            cards,
            obj_tags.join(",")
        );
        if header.chars().count() + line.chars().count()
            > MAX_STATUS_HEADER_CHARS
        {
            header.push_str("...[truncated]\n");
            break;
        }
        header.push_str(&line);
    }
    out.push_str(&header);
    out
}

pub(super) fn normalize_router_provider_hint(
    raw: Option<String>,
) -> Option<String> {
    let raw = raw?;
    let cleaned = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return None;
    }
    Some(flowy_agent_core::log_preview(
        &cleaned,
        MAX_ROUTER_PROVIDER_HINT_CHARS,
    ))
}

fn append_worker_capabilities_section(harness: &Harness, out: &mut String) {
    let mut lines = Vec::new();
    let mut total_chars = 0_usize;
    for entry in harness.tool_providers() {
        if lines.len() >= MAX_ROUTER_PROVIDER_HINTS {
            break;
        }
        let Some(hint) = entry.cached_router_hint.as_deref() else {
            continue;
        };
        let cleaned = hint.trim();
        if cleaned.is_empty() {
            continue;
        }
        let remaining =
            MAX_ROUTER_PROVIDER_TOTAL_CHARS.saturating_sub(total_chars);
        if remaining == 0 {
            break;
        }
        let clamped = flowy_agent_core::log_preview(
            cleaned,
            MAX_ROUTER_PROVIDER_HINT_CHARS.min(remaining),
        );
        total_chars = total_chars.saturating_add(clamped.chars().count());
        lines.push(clamped);
    }
    if lines.is_empty() {
        return;
    }
    out.push_str(
        "\n\n[WorkerCapabilities]\nAvailable inside `mode=\"read_write\"` Workers only:\n",
    );
    for line in lines {
        out.push_str("- ");
        out.push_str(&line);
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
}

impl Harness {
    /// Check whether an explicit `request_id` already exists on `router_id`.
    /// Returns `Ok(Some(result))` for an idempotent replay, `Err("IDEMPOTENCY_CONFLICT")`
    /// on payload fingerprint mismatch, or `Ok(None)` when absent.
    pub(super) fn lookup_idempotent_dispatch(
        &self,
        engine: &Engine,
        router_id: &str,
        request_id: Option<&str>,
        fingerprint: &str,
        fallback_access_mode: Option<WorkAccessMode>,
    ) -> Result<Option<DispatchWorkResult>, &'static str> {
        let Some(req_id) = request_id else {
            return Ok(None);
        };
        let Some(existing) = engine.lookup_dispatch_request(router_id, req_id)
        else {
            return Ok(None);
        };
        if existing.fingerprint != fingerprint {
            return Err("IDEMPOTENCY_CONFLICT");
        }
        let access_mode = fallback_access_mode.unwrap_or_else(|| {
            engine
                .with_session(&existing.worker_id, |r| r.access_mode)
                .unwrap_or(WorkAccessMode::ReadWrite)
        });
        let status = self
            .child_works(router_id)
            .into_iter()
            .find(|w| w.work_id == existing.worker_id)
            .map_or_else(|| "queued".to_owned(), |w| w.state);
        Ok(Some(DispatchWorkResult {
            work_id: existing.worker_id,
            receipt_id: existing.receipt_id,
            status,
            access_mode,
            deduplicated: true,
        }))
    }

    pub(super) fn dispatch_work_from_router(
        self: &Arc<Self>,
        router_id: &str,
        req: DispatchWorkRequest,
    ) -> Result<DispatchWorkResult, &'static str> {
        if self.state.shutdown_requested.load(Ordering::Acquire) {
            return Err("HOST_SHUTTING_DOWN");
        }
        let brief = req.brief.trim();
        if brief.is_empty() {
            return Err("INVALID_ARGS");
        }
        let fingerprint = dispatch_fingerprint(
            "dispatch",
            req.target_work_id.as_deref(),
            req.requested_mode,
            brief,
            &req.target_object_ids,
        );
        let result = {
            let _dispatch = lock(&self.state.dispatch);
            if self.state.shutdown_requested.load(Ordering::Acquire) {
                return Err("HOST_SHUTTING_DOWN");
            }
            let engine =
                lock(&self.engine).clone().ok_or("HOST_UNAVAILABLE")?;
            let parent = engine.session(router_id).ok_or("ROUTER_NOT_FOUND")?;
            if parent.role != SessionRole::Router
                || !lock(&self.state.live).contains_key(router_id)
            {
                return Err("ROUTER_NOT_FOUND");
            }
            if self
                .state
                .artifact_store
                .get()
                .is_some_and(|store| store.has_active_deletion_plan(router_id))
            {
                return Err("ROUTER_DELETING");
            }
            if let Some(dedup) = self.lookup_idempotent_dispatch(
                &engine,
                router_id,
                req.request_id.as_deref(),
                &fingerprint,
                None,
            )? {
                return Ok(dedup);
            }

            // Resolve optional `target_object_ids` strictly within owned child Workers.
            let object_owners = resolve_owned_objects(
                &engine,
                router_id,
                &req.target_object_ids,
            )?;
            let mut effective_target = req.target_work_id.clone();
            if !object_owners.is_empty() {
                let unique_workers: BTreeSet<&str> =
                    object_owners.values().map(String::as_str).collect();
                if unique_workers.len() > 1 {
                    return Err("MULTIPLE_TARGET_WORKERS");
                }
                if let Some(&owner_ref) = unique_workers.iter().next() {
                    let owner = owner_ref.to_owned();
                    if let Some(explicit) = effective_target.as_deref()
                        && explicit != owner
                    {
                        return Err("TARGET_WORK_OBJECT_MISMATCH");
                    }
                    effective_target = Some(owner);
                }
            }

            let now = now_ms();
            let (worker_id, access_mode, created_new, created_home_workspace) =
                if let Some(target_id) = effective_target {
                    let worker =
                        verify_child_ownership(&engine, router_id, &target_id)?;
                    if !lock(&self.state.live).contains_key(&target_id) {
                        return Err("WORK_NOT_FOUND");
                    }
                    if worker.access_mode == WorkAccessMode::ReadOnly
                        && req.requested_mode == Some(WorkAccessMode::ReadWrite)
                    {
                        return Err("ACCESS_MODE_ESCALATION_DENIED");
                    }
                    (target_id, worker.access_mode, false, None)
                } else {
                    let mode =
                        req.requested_mode.unwrap_or(WorkAccessMode::ReadWrite);
                    let worker_id = format!("w-{}", next_stamp());
                    let parent_workspace = lock(&self.state.live)
                        .get(router_id)
                        .and_then(|s| s.workspace.clone());
                    let origin = parent.origin.clone();
                    let (session_workspace, home_dir) = match &origin {
                        SessionOrigin::Home => {
                            let root = self
                                .state
                                .root
                                .join("sessions")
                                .join("workspaces")
                                .join(&worker_id);
                            std::fs::create_dir_all(&root).map_err(
                                |error| {
                                    log_storage_io_error(
                                        "create worker workspace",
                                        &root,
                                        &error,
                                    );
                                    "STORAGE_ERROR"
                                },
                            )?;
                            (
                                Some(SessionWorkspace { root: root.clone() }),
                                Some(root),
                            )
                        }
                        SessionOrigin::Project { .. } => (
                            parent.workspace.or_else(|| {
                                parent_workspace.as_ref().map(|ws| {
                                    SessionWorkspace {
                                        root: ws.root.clone(),
                                    }
                                })
                            }),
                            None,
                        ),
                        SessionOrigin::Unknown => (None, None),
                    };
                    if engine
                        .create_session_with_role(
                            worker_id.clone(),
                            now,
                            origin.clone(),
                            session_workspace.clone(),
                            SessionRole::Worker,
                            Some(router_id.to_owned()),
                            mode,
                        )
                        .is_err()
                    {
                        if let Some(dir) = home_dir.as_deref() {
                            let _ = std::fs::remove_dir_all(dir);
                        }
                        return Err("STORAGE_ERROR");
                    }
                    let worker_title = crate::title::clamp_title(
                        &crate::title::fallback_title(
                            brief,
                            crate::title::TITLE_MAX_CHARS,
                        ),
                        crate::title::TITLE_MAX_CHARS,
                    );
                    lock(&self.state.live).insert(
                        worker_id.clone(),
                        Live {
                            projection: ChatProjection::new(),
                            title: worker_title.clone(),
                            titled: true,
                            pinned: false,
                            archived: false,
                            auto_title_pending: false,
                            auto_title_in_flight: false,
                            auto_title_attempts: 0,
                            title_signal: CancelToken::new(),
                            status: TurnStatus::Idle,
                            updated_at: now,
                            turn_started_at: None,
                            workspace: parent_workspace,
                            origin,
                            session_workspace,
                            heartbeat: None,
                            role: SessionRole::Worker,
                            root_session_id: Some(router_id.to_owned()),
                            access_mode: mode,
                            active_turn_is_internal: false,
                            active_turn_prev_status: None,
                            card_receipts: BTreeMap::new(),
                            active_dispatch_receipt_id: None,
                        },
                    );
                    let _ = engine.set_session_title(
                        &worker_id,
                        worker_title,
                        true,
                    );
                    if persist_index(&self.state).is_err() {
                        self.state.report_storage_error();
                    }
                    (worker_id, mode, true, home_dir)
                };

            let rollback_new_worker = || {
                if created_new {
                    let _ = engine.remove_session(&worker_id);
                    lock(&self.state.live).remove(&worker_id);
                    if let Some(dir) = created_home_workspace.as_deref() {
                        let _ = std::fs::remove_dir_all(dir);
                    }
                    let _ = persist_index(&self.state);
                }
            };

            #[cfg(test)]
            {
                let hook =
                    lock(&self.state.before_worker_strict_enqueue_hook).clone();
                if let Some(hook) = hook
                    && let Err(injected) = hook(router_id, &worker_id)
                {
                    rollback_new_worker();
                    return Err(injected);
                }
            }

            let explicit_request_id = req.request_id.as_deref();
            let queue = AcceptanceQueue::new(self.state.root.clone());
            let Ok(receipt) = queue.accept_strict(
                &worker_id,
                brief,
                explicit_request_id,
                now,
            ) else {
                rollback_new_worker();
                self.state.report_storage_error();
                return Err("STORAGE_ERROR");
            };
            if let Some(req_id) = explicit_request_id
                && engine
                    .record_dispatch_request(
                        router_id,
                        DispatchRecord {
                            request_id: req_id.to_owned(),
                            fingerprint,
                            worker_id: worker_id.clone(),
                            receipt_id: receipt.id.clone(),
                            created_at_ms: now,
                        },
                    )
                    .is_err()
            {
                let _ = queue.cancel(&receipt.id, now_ms());
                rollback_new_worker();
                self.state.report_storage_error();
                return Err("STORAGE_ERROR");
            }
            self.state.push_update(AgentUpdate::Receipt {
                id: receipt.id.clone(),
                state: ReceiptState::Queued,
            });
            self.state.push_update(AgentUpdate::Session {
                id: router_id.to_owned(),
            });
            DispatchWorkResult {
                work_id: worker_id,
                receipt_id: receipt.id,
                status: "queued".to_owned(),
                access_mode,
                deduplicated: false,
            }
        };
        if self.state.autonomous_drain.load(Ordering::Acquire) {
            self.kick_autonomous_drain();
        }
        Ok(result)
    }

    pub(super) fn child_works(
        &self,
        root_session_id: &str,
    ) -> Vec<ChildWorkSummary> {
        let Some(engine) = lock(&self.engine).clone() else {
            return Vec::new();
        };
        let children = engine.child_sessions(root_session_id);
        if children.is_empty() {
            return Vec::new();
        }
        let all_receipts = AcceptanceQueue::new(self.state.root.clone())
            .all()
            .unwrap_or_default();
        let store = self.state.artifact_store.get();
        let committed_ids = store
            .and_then(|s| s.committed_ids().ok())
            .unwrap_or_default();
        let now = now_ms();
        let live_snapshot: BTreeMap<
            String,
            (TurnStatus, Option<BranchHeartbeat>, String, u64),
        > = {
            let live = lock(&self.state.live);
            children
                .iter()
                .filter_map(|rec| {
                    live.get(&rec.id).map(|l| {
                        (
                            rec.id.clone(),
                            (
                                l.status.clone(),
                                l.heartbeat.clone(),
                                l.title.clone(),
                                l.updated_at,
                            ),
                        )
                    })
                })
                .collect()
        };
        let mut out = Vec::with_capacity(children.len());
        for rec in children {
            let (live_status, heartbeat, live_title, live_updated) =
                live_snapshot.get(&rec.id).cloned().unwrap_or_else(|| {
                    (
                        super::observer::live_status(rec.status.clone()),
                        None,
                        rec.title.clone(),
                        rec.updated_at,
                    )
                });
            let latest_sub = super::work::latest_session_submission_id(&rec);
            let latest_outcome = latest_sub
                .as_deref()
                .and_then(|sub| rec.submission_outcomes.get(sub));
            let has_queued_receipt = all_receipts.iter().any(|r| {
                r.target_session_id.as_deref().or(r.session_hint.as_deref())
                    == Some(rec.id.as_str())
                    && matches!(
                        r.state_at(now),
                        ReceiptState::Queued | ReceiptState::Retrying
                    )
            });
            let contract_revision = latest_sub
                .as_deref()
                .and_then(|sub| rec.submission_revisions.get(sub).copied())
                .or_else(|| rec.work_contract.as_ref().map(|c| c.revision));
            let mut delivered_artifact_ids = Vec::new();
            for commit in &rec.delivery_commits {
                if !commit.completed_successfully {
                    continue;
                }
                for aid in &commit.artifact_ids {
                    if committed_ids.contains(aid)
                        && !delivered_artifact_ids.contains(aid)
                        && store
                            .and_then(|s| s.get(aid).ok())
                            .flatten()
                            .is_none_or(|item| {
                                !matches!(
                                    item.manifest.publication_state,
                                    PublicationState::Deleted
                                        | PublicationState::DeletePending
                                )
                            })
                    {
                        delivered_artifact_ids.push(aid.clone());
                    }
                }
            }
            let missing_items = match latest_outcome {
                Some(SubmissionOutcome::Partial { missing }) => missing.clone(),
                _ => Vec::new(),
            };
            let state = if !rec.pending_cards.is_empty()
                || live_status == TurnStatus::WaitingForUser
                || rec.status == TurnState::WaitingForUser
            {
                "awaiting_approval"
            } else if matches!(live_status, TurnStatus::Stopping)
                || rec.status == TurnState::Stopping
            {
                "stopping"
            } else if matches!(live_status, TurnStatus::SavingFiles) {
                "verifying"
            } else if matches!(live_status, TurnStatus::Running)
                || engine.is_active(&rec.id)
            {
                "running"
            } else if has_queued_receipt {
                "queued"
            } else if matches!(
                live_status,
                TurnStatus::Error { .. } | TurnStatus::DeliveryFailed
            ) || matches!(
                latest_outcome,
                Some(SubmissionOutcome::Failed { .. })
            ) {
                "failed"
            } else if matches!(
                latest_outcome,
                Some(SubmissionOutcome::Partial { .. })
            ) || !missing_items.is_empty()
            {
                "partial"
            } else if matches!(live_status, TurnStatus::Stopped)
                || matches!(latest_outcome, Some(SubmissionOutcome::Aborted))
            {
                "stopped"
            } else if !delivered_artifact_ids.is_empty() {
                "delivered"
            } else if matches!(live_status, TurnStatus::Completed)
                || matches!(latest_outcome, Some(SubmissionOutcome::Completed))
            {
                "completed"
            } else {
                "idle"
            }
            .to_owned();
            let title = if live_title.is_empty() {
                rec.title.clone()
            } else {
                live_title
            };
            let objects = rec
                .work_contract
                .as_ref()
                .map_or_default(|wc| wc.objects.clone());
            out.push(ChildWorkSummary {
                work_id: rec.id,
                root_session_id: root_session_id.to_owned(),
                title,
                access_mode: rec.access_mode,
                origin: rec.origin,
                workspace_root: rec.workspace.map(|ws| ws.root),
                state,
                turn_status: live_status,
                contract_revision,
                heartbeat,
                pending_cards: rec.pending_cards,
                delivered_artifact_ids,
                missing_items,
                objects,
                last_stop_handoff: rec.last_stop_handoff,
                updated_at: live_updated.max(rec.updated_at),
            });
        }
        out.sort_by_key(|w| std::cmp::Reverse(w.updated_at));
        out
    }

    pub(super) fn root_pending_cards(
        &self,
        root_session_id: &str,
    ) -> Vec<RoutedPendingCard> {
        let Some(engine) = lock(&self.engine).clone() else {
            return Vec::new();
        };
        let mut cards = Vec::new();
        if let Some(root) = engine.session(root_session_id) {
            for card in root.pending_cards {
                cards.push(RoutedPendingCard {
                    root_session_id: root_session_id.to_owned(),
                    owner_session_id: root.id.clone(),
                    owner_role: root.role,
                    card,
                });
            }
        }
        for child in engine.child_sessions(root_session_id) {
            for card in child.pending_cards {
                cards.push(RoutedPendingCard {
                    root_session_id: root_session_id.to_owned(),
                    owner_session_id: child.id.clone(),
                    owner_role: child.role,
                    card,
                });
            }
        }
        cards.sort_by_key(|item| match &item.card {
            PendingCard::Disambiguation { created_at_ms, .. }
            | PendingCard::ToolApproval { created_at_ms, .. } => *created_at_ms,
        });
        cards
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentHandle, LlmConfig};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn router_prompt_and_dispatch_schema_enforce_turn_discipline_and_ro_limits()
    {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "flowy-router-prompt-{}-{nanos}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let handle =
            AgentHandle::with_config(dir.clone(), LlmConfig::default());
        let router_id = handle.create_router_session().unwrap();

        let prompt =
            build_router_system_prompt(&handle.inner, &router_id, false);
        assert!(prompt.contains("ONE short opening acknowledgement sentence"));
        assert!(prompt.contains("do NOT poll `query_work_status` repeatedly"));
        assert!(prompt.contains(
            "`read_only` Workers only have `read_file`/`glob`/`grep`"
        ));

        let tool = dispatch_work_tool(Arc::downgrade(&handle.inner), router_id);
        let mode_desc = tool.parameters["properties"]["mode"]["description"]
            .as_str()
            .expect("mode description must exist");
        assert!(mode_desc.contains("read_file"));
        assert!(mode_desc.contains("no shell/curl"));
        assert!(mode_desc.contains("read_write"));

        drop(handle);
        let _ = std::fs::remove_dir_all(dir);
    }
}
