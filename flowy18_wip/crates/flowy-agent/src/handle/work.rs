//! Work-level status reconciliation, UI-decoupled cancellation, card actions, and graceful shutdown.

use std::sync::Arc;
use std::time::{Duration, Instant};

use flowy_agent_core::{
    DispatchRecord, SessionRole, TrackedObjectKind, WorkContract,
};

use super::router::{
    ChildWorkSummary, DispatchWorkResult, RoutedPendingCard, StopWorkResult,
    dispatch_fingerprint, verify_child_ownership,
};
use super::{
    AcceptanceQueue, AcceptedMessage, AgentHandle, AgentUpdate, ArtifactStore,
    BTreeSet, CancelOutcome, CardAction, CardActionOutcome, CardSelection,
    ChatLine, Engine, Harness, MessageSource, Ordering, PendingCard,
    ReceiptState, ResumedSubmission, Role, SessionRecord, SubmissionOutcome,
    TaskTarget, TurnState, TurnStatus, WorkStatus, clear_live_heartbeat, lock,
    log_storage_io_error, now_ms, persist_index,
};
use crate::artifact_store::PublicationState;

type ReapTarget = (Arc<Engine>, String);

enum DenialRollback {
    Unchanged,
    Restore(Option<String>),
}

struct CardResumeSpec<'a> {
    engine: &'a Engine,
    session_id: &'a str,
    card_id: &'a str,
    removed_card: PendingCard,
    previous_contract: Option<WorkContract>,
    visible_prompt: &'a str,
    resumed: ResumedSubmission,
    approval_grant: Option<(String, String)>,
    denial_rollback: DenialRollback,
}

impl AgentHandle {
    /// Unified host view of a work item identified by either its `receipt_id`
    /// or its `submission_id`, combining persisted contract revision, terminal
    /// outcome, missing deliverables, and currently visible committed files.
    #[must_use]
    pub fn work_status(&self, work_id: &str) -> Option<WorkStatus> {
        let receipt = self.receipt_by_id(work_id).or_else(|| {
            AcceptanceQueue::new(self.inner.state.root.clone())
                .all()
                .ok()
                .and_then(|receipts| {
                    receipts
                        .into_iter()
                        .filter(|item| {
                            item.submission_id.as_deref() == Some(work_id)
                        })
                        .max_by_key(|item| item.created_at_ms)
                })
        });
        let mut session_id = receipt
            .as_ref()
            .and_then(|item| item.target_session_id.clone());
        let mut submission_id =
            receipt.as_ref().and_then(|item| item.submission_id.clone());
        let engine = lock(&self.inner.engine).clone();
        let record = engine.as_ref().and_then(|engine| {
            if let Some(sid) = session_id.as_deref() {
                engine.session(sid)
            } else {
                engine.sessions().into_iter().find(|rec| {
                    rec.submission_outcomes.contains_key(work_id)
                        || rec.submission_revisions.contains_key(work_id)
                        || rec
                            .delivery_commits
                            .iter()
                            .any(|c| c.submission_id == work_id)
                        || rec.messages.iter().any(|m| m.id == work_id)
                })
            }
        });
        if receipt.is_none() && record.is_none() {
            return None;
        }
        if let Some(rec) = &record {
            if session_id.is_none() {
                session_id = Some(rec.id.clone());
            }
            if submission_id.is_none() && receipt.is_none() {
                submission_id = Some(work_id.to_owned());
            }
        }
        Some(self.build_work_status(
            work_id,
            receipt,
            record,
            session_id,
            submission_id,
        ))
    }

    /// Unified host view of the latest work item in `session_id` (covering
    /// both queued receipts and direct `submit_chat` / `send` turns), without
    /// requiring callers to construct internal `submission-*` IDs.
    #[must_use]
    pub fn session_work_status(&self, session_id: &str) -> Option<WorkStatus> {
        let record = lock(&self.inner.engine)
            .as_ref()
            .and_then(|engine| engine.session(session_id))?;
        let submission_id = latest_session_submission_id(&record);
        if submission_id.is_none()
            && record.work_contract.is_none()
            && record.delivery_commits.is_empty()
        {
            return None;
        }
        let matched_receipt =
            AcceptanceQueue::new(self.inner.state.root.clone())
                .all()
                .ok()
                .and_then(|receipts| {
                    receipts
                        .into_iter()
                        .filter(|receipt| {
                            receipt.target_session_id.as_deref()
                                == Some(session_id)
                                && match submission_id.as_deref() {
                                    Some(sub) => {
                                        receipt.submission_id.as_deref()
                                            == Some(sub)
                                    }
                                    None => true,
                                }
                        })
                        .max_by_key(|receipt| receipt.created_at_ms)
                });
        let work_id = matched_receipt
            .as_ref()
            .map(|receipt| receipt.id.clone())
            .or_else(|| submission_id.clone())
            .unwrap_or_else(|| session_id.to_owned());
        Some(self.build_work_status(
            &work_id,
            matched_receipt,
            Some(record),
            Some(session_id.to_owned()),
            submission_id,
        ))
    }

    /// Single-pass batch projection of work contracts for the Work Board:
    /// avoids `O(S + R)` replays of `acceptance/journal.jsonl`, `committed_ids`,
    /// and full `SessionRecord` clones on the UI thread.
    ///
    /// Returns `(by_session_id, by_work_id)`.
    #[must_use]
    pub fn board_work_contracts(
        &self,
        visible_receipts: &[(AcceptedMessage, ReceiptState)],
    ) -> (
        std::collections::HashMap<String, WorkStatus>,
        std::collections::HashMap<String, WorkStatus>,
    ) {
        use std::collections::HashMap;
        let store = self.inner.state.artifact_store.get();
        let committed_ids = store
            .and_then(|s| s.committed_ids().ok())
            .unwrap_or_default();
        let mut artifact_cache: HashMap<
            String,
            Option<(PublicationState, String, String)>,
        > = HashMap::new();
        let mut lookup_artifact = |id: &str| {
            artifact_cache
                .entry(id.to_owned())
                .or_insert_with(|| {
                    store.and_then(|s| s.get(id).ok()).flatten().map(|item| {
                        (
                            item.manifest.publication_state,
                            item.manifest.receipt.display_name,
                            item.manifest.receipt.delivery_key,
                        )
                    })
                })
                .clone()
        };

        let mut by_session: HashMap<String, WorkStatus> = HashMap::new();
        let mut by_work: HashMap<String, WorkStatus> = HashMap::new();
        let now = now_ms();

        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = &engine {
            engine.with_sessions(|records| {
                for rec in records {
                    let submission_id = latest_session_submission_id(rec);
                    if submission_id.is_none()
                        && rec.work_contract.is_none()
                        && rec.delivery_commits.is_empty()
                    {
                        continue;
                    }
                    let matched_receipt = visible_receipts
                        .iter()
                        .filter(|(receipt, _)| {
                            receipt.target_session_id.as_deref()
                                == Some(rec.id.as_str())
                                && match submission_id.as_deref() {
                                    Some(sub) => {
                                        receipt.submission_id.as_deref()
                                            == Some(sub)
                                    }
                                    None => true,
                                }
                        })
                        .max_by_key(|(receipt, _)| receipt.created_at_ms);
                    let work_id = matched_receipt
                        .map(|(r, _)| r.id.clone())
                        .or_else(|| submission_id.clone())
                        .unwrap_or_else(|| rec.id.clone());
                    let status = build_work_status_borrowed(
                        &work_id,
                        matched_receipt.map(|(_, st)| *st),
                        Some(rec),
                        Some(rec.id.clone()),
                        submission_id,
                        &committed_ids,
                        &mut lookup_artifact,
                    );
                    by_session.insert(rec.id.clone(), status);
                }

                for (receipt, state) in visible_receipts {
                    let session_id = receipt.target_session_id.clone();
                    let submission_id = receipt.submission_id.clone();
                    let rec = session_id
                        .as_deref()
                        .and_then(|sid| records.iter().find(|r| r.id == sid))
                        .or_else(|| {
                            submission_id.as_deref().and_then(|sub| {
                                records.iter().find(|r| {
                                    r.submission_outcomes.contains_key(sub)
                                        || r.submission_revisions
                                            .contains_key(sub)
                                        || r.delivery_commits
                                            .iter()
                                            .any(|c| c.submission_id == sub)
                                })
                            })
                        });
                    let resolved_sid =
                        session_id.or_else(|| rec.map(|r| r.id.clone()));
                    let status = build_work_status_borrowed(
                        &receipt.id,
                        Some(*state),
                        rec,
                        resolved_sid,
                        submission_id,
                        &committed_ids,
                        &mut lookup_artifact,
                    );
                    by_work.insert(receipt.id.clone(), status);
                }
            });
        } else {
            for (receipt, state) in visible_receipts {
                let status = WorkStatus {
                    work_id: receipt.id.clone(),
                    session_id: receipt.target_session_id.clone(),
                    submission_id: receipt.submission_id.clone(),
                    contract_revision: None,
                    receipt_state: Some(*state)
                        .or_else(|| Some(receipt.state_at(now))),
                    outcome: None,
                    required_items: Vec::new(),
                    delivered_artifact_ids: Vec::new(),
                    missing_items: Vec::new(),
                };
                by_work.insert(receipt.id.clone(), status);
            }
        }
        (by_session, by_work)
    }

    fn build_work_status(
        &self,
        work_id: &str,
        receipt: Option<AcceptedMessage>,
        record: Option<SessionRecord>,
        session_id: Option<String>,
        submission_id: Option<String>,
    ) -> WorkStatus {
        let receipt_state =
            receipt.as_ref().map(|item| item.state_at(now_ms()));
        let store = self.inner.state.artifact_store.get();
        let committed_ids = store
            .and_then(|s| s.committed_ids().ok())
            .unwrap_or_default();
        let mut lookup_artifact = |id: &str| {
            store.and_then(|s| s.get(id).ok()).flatten().map(|item| {
                (
                    item.manifest.publication_state,
                    item.manifest.receipt.display_name,
                    item.manifest.receipt.delivery_key,
                )
            })
        };
        build_work_status_borrowed(
            work_id,
            receipt_state,
            record.as_ref(),
            session_id,
            submission_id,
            &committed_ids,
            &mut lookup_artifact,
        )
    }

    /// Authoritative host summaries of all child Workers owned by `root_session_id`.
    #[must_use]
    pub fn child_works(&self, root_session_id: &str) -> Vec<ChildWorkSummary> {
        self.inner.child_works(root_session_id)
    }

    /// All pending interactive cards routed to `root_session_id` (both Router
    /// choice cards and child Worker approval/disambiguation cards).
    #[must_use]
    pub fn root_pending_cards(
        &self,
        root_session_id: &str,
    ) -> Vec<RoutedPendingCard> {
        self.inner.root_pending_cards(root_session_id)
    }

    /// Stop ONLY the Router's active reply without cancelling any child Workers.
    pub fn stop_router_reply(
        &self,
        router_id: &str,
    ) -> Result<StopWorkResult, &'static str> {
        if self.inner.session_role(router_id) != SessionRole::Router {
            return Err("notice.agent_failed");
        }
        let outcome = self
            .inner
            .cancel_task(TaskTarget::Session(router_id.to_owned()))?;
        let status = self.session_status(router_id).key().to_owned();
        let outcome_str = match outcome {
            CancelOutcome::Cancelled => {
                if self.session_status(router_id) == TurnStatus::Stopping {
                    "stopping"
                } else {
                    "cancelled"
                }
            }
            CancelOutcome::AlreadySettled => "already_settled",
            CancelOutcome::FinalizingNotCancellable => {
                "finalizing_not_cancellable"
            }
        }
        .to_owned();
        Ok(StopWorkResult {
            work_id: router_id.to_owned(),
            outcome: outcome_str,
            status,
            cancelled_receipt_ids: Vec::new(),
            committed_artifact_ids: Vec::new(),
        })
    }

    /// Cancel one owned child Worker across all states (Queued, Running,
    /// `WaitingForUser`, Finalizing, or terminal).
    pub fn cancel_work(
        &self,
        root_session_id: &str,
        worker_id: &str,
    ) -> Result<StopWorkResult, &'static str> {
        self.inner.cancel_work(root_session_id, worker_id)
    }

    /// Idempotently revise an owned child Worker: cancels queued/running work,
    /// waits for old turn settlement, preserves immutable environment/access
    /// mode, and enqueues strictly one new strict receipt.
    pub fn revise_work(
        &self,
        root_session_id: &str,
        worker_id: &str,
        revision_brief: &str,
        request_id: &str,
    ) -> Result<DispatchWorkResult, &'static str> {
        let req_id = request_id.trim();
        if req_id.is_empty() {
            return Err("INVALID_ARGS");
        }
        self.inner.revise_work(
            root_session_id,
            worker_id,
            revision_brief,
            Some(req_id),
        )
    }

    /// Strongly-typed task cancellation decoupled from whichever session the
    /// UI is currently viewing. Never reads `active`.
    pub fn cancel_task(
        &self,
        target: TaskTarget,
    ) -> Result<CancelOutcome, &'static str> {
        self.inner.cancel_task(target)
    }

    /// Apply a structured card-level action inside the host kernel without
    /// coupling presentation widgets to storage or queue internals.
    pub fn apply_card_action(
        &self,
        action: CardAction,
    ) -> Result<CardActionOutcome, &'static str> {
        match action {
            CardAction::OverrideDeliver {
                session_id,
                report_id,
                artifact_path,
            } => {
                let artifact_id = self.override_deliver_artifact(
                    &session_id,
                    &report_id,
                    &artifact_path,
                )?;
                Ok(CardActionOutcome::ArtifactDelivered { artifact_id })
            }
            CardAction::RetryDelivery {
                session_id,
                submission_id,
            } => {
                if let Some(sub) = submission_id.as_deref() {
                    let has_commit = lock(&self.inner.engine)
                        .as_ref()
                        .and_then(|engine| engine.session(&session_id))
                        .is_some_and(|rec| {
                            rec.delivery_commits
                                .iter()
                                .any(|commit| commit.submission_id == sub)
                        });
                    if !has_commit {
                        return Err("chat.status_delivery_failed");
                    }
                }
                self.retry_delivery(Some(&session_id))?;
                Ok(CardActionOutcome::DeliveryRetried)
            }
            CardAction::RetryDeadReceipt { receipt_id } => {
                let queue = AcceptanceQueue::new(self.inner.state.root.clone());
                let Some(requeued) =
                    queue.retry_dead(&receipt_id, now_ms()).map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?
                else {
                    return Err("notice.receipt_gave_up");
                };
                self.inner.state.push_update(AgentUpdate::Receipt {
                    id: receipt_id,
                    state: ReceiptState::Cancelled,
                });
                self.inner.state.push_update(AgentUpdate::Receipt {
                    id: requeued.id.clone(),
                    state: ReceiptState::Queued,
                });
                if self.inner.state.autonomous_drain.load(Ordering::Acquire) {
                    self.inner.kick_autonomous_drain();
                }
                Ok(CardActionOutcome::ReceiptRequeued {
                    receipt_id: requeued.id,
                })
            }
            CardAction::ResolveCard {
                session_id,
                card_id,
                selection,
            } => self.resolve_card_action(&session_id, &card_id, selection),
        }
    }

    fn resolve_card_action(
        &self,
        session_id: &str,
        card_id: &str,
        selection: CardSelection,
    ) -> Result<CardActionOutcome, &'static str> {
        let _dispatch = lock(&self.inner.state.dispatch);
        if self.is_initializing() {
            return Err("library.loading");
        }
        if self.inner.is_session_busy_or_active(session_id) {
            return Err("notice.agent_busy");
        }
        let engine = lock(&self.inner.engine)
            .clone()
            .ok_or("notice.agent_failed")?;
        let record = engine.session(session_id).ok_or("notice.agent_failed")?;
        let Some(card) = record
            .pending_cards
            .iter()
            .find(|item| item.card_id() == card_id)
            .cloned()
        else {
            return Err("notice.card_not_found");
        };
        match (card, selection) {
            (
                PendingCard::Disambiguation {
                    prompt,
                    impact_boundary,
                    options,
                    allow_multi,
                    ..
                },
                CardSelection::Disambiguate { object_ids },
            ) => {
                if object_ids.is_empty() {
                    return Err("notice.card_selection_empty");
                }
                if !allow_multi && object_ids.len() > 1 {
                    return Err("notice.card_selection_invalid");
                }
                let valid = object_ids
                    .iter()
                    .all(|oid| options.iter().any(|opt| opt.object_id == *oid));
                if !valid {
                    return Err("notice.card_selection_invalid");
                }
                if record.role == SessionRole::Router {
                    let removed_card = engine
                        .take_pending_card(
                            session_id,
                            card_id,
                            TurnState::Idle,
                            None,
                        )
                        .map_err(|_| {
                            self.inner.state.report_storage_error();
                            "notice.session_storage_failed"
                        })?
                        .ok_or("notice.card_not_found")?;
                    let children = engine.child_sessions(session_id);
                    let mut details = Vec::with_capacity(object_ids.len());
                    for oid in &object_ids {
                        let label = options
                            .iter()
                            .find(|opt| opt.object_id == *oid)
                            .map_or_else(
                                || oid.clone(),
                                |opt| opt.label.clone(),
                            );
                        if let Some((owner_id, obj, confirmed)) =
                            children.iter().find_map(|child| {
                                let wc = child.work_contract.as_ref()?;
                                let obj = wc.find_object(oid)?;
                                let facts = wc.confirmed_facts_for(oid);
                                Some((child.id.clone(), obj.clone(), facts))
                            })
                        {
                            let facts_str = if confirmed.is_empty() {
                                "none".to_owned()
                            } else {
                                confirmed
                                    .iter()
                                    .map(|(k, v)| format!("{k}={v}"))
                                    .collect::<Vec<_>>()
                                    .join(",")
                            };
                            let kind = match obj.kind {
                                TrackedObjectKind::Deliverable => "deliverable",
                                TrackedObjectKind::Record => "record",
                            };
                            details.push(format!(
                                "object_id={oid} kind={kind} label={label} owner_work_id={owner_id} confirmed_facts=[{facts_str}]"
                            ));
                        } else {
                            details
                                .push(format!("object_id={oid} label={label}"));
                        }
                    }
                    let resume_prompt = format!(
                        "[CardChoiceConfirmed] 原问题：{prompt}\n用户已确认选择：{}\n影响边界：{impact_boundary}\n请严格按已确认目标继续执行，不得修改未选对象。",
                        details.join("；")
                    );
                    return self.resume_after_card_resolution(CardResumeSpec {
                        engine: &engine,
                        session_id,
                        card_id,
                        removed_card,
                        previous_contract: None,
                        visible_prompt: &resume_prompt,
                        resumed: ResumedSubmission {
                            skip_contract_evolve: true,
                            model_prompt_override: Some(resume_prompt.clone()),
                            display_text_override: Some(String::new()),
                            skip_push_user_line: true,
                            prompt_source: MessageSource::CardResume,
                            bypass_busy_queue: true,
                            ..ResumedSubmission::default()
                        },
                        approval_grant: None,
                        denial_rollback: DenialRollback::Unchanged,
                    });
                }
                let prev_contract = record
                    .work_contract
                    .as_ref()
                    .ok_or("notice.card_selection_invalid")?;
                let updated_brief =
                    crate::verify::apply_disambiguation_selection(
                        prev_contract,
                        &prompt,
                        &object_ids,
                        now_ms(),
                    )?;
                let updated_wc = updated_brief.to_work_contract();
                let removed_card = engine
                    .take_pending_card(
                        session_id,
                        card_id,
                        TurnState::Idle,
                        Some(updated_wc),
                    )
                    .map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?
                    .ok_or("notice.card_not_found")?;
                let selected_labels: Vec<String> = options
                    .iter()
                    .filter(|opt| object_ids.contains(&opt.object_id))
                    .map(|opt| opt.label.clone())
                    .collect();
                let suffix = format!(
                    "\n\n[已确认修改目标：{}；影响边界：{impact_boundary}]",
                    selected_labels.join("、")
                );
                self.resume_after_card_resolution(CardResumeSpec {
                    engine: &engine,
                    session_id,
                    card_id,
                    removed_card,
                    previous_contract: record.work_contract.clone(),
                    visible_prompt: &prompt,
                    resumed: ResumedSubmission {
                        contract: Some(updated_brief),
                        model_prompt_suffix: Some(suffix),
                        skip_push_user_line: true,
                        bypass_busy_queue: true,
                        ..ResumedSubmission::default()
                    },
                    approval_grant: None,
                    denial_rollback: DenialRollback::Unchanged,
                })
            }
            (
                PendingCard::ToolApproval {
                    tool_name,
                    args_hash: expected_hash,
                    command_preview,
                    ..
                },
                CardSelection::ApproveTool { args_hash },
            ) => {
                if args_hash != expected_hash {
                    return Err("notice.approval_hash_mismatch");
                }
                let removed_card = engine
                    .take_pending_card(
                        session_id,
                        card_id,
                        TurnState::Idle,
                        None,
                    )
                    .map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?
                    .ok_or("notice.card_not_found")?;
                lock(&self.inner.state.approved_tool_calls)
                    .entry(session_id.to_owned())
                    .or_default()
                    .push((tool_name.clone(), args_hash.clone()));
                let resume_prompt = format!(
                    "[ToolApproved] 用户已批准执行 `{tool_name}`（命令预览：`{command_preview}`）。请使用与上次完全一致的参数继续完成该操作。"
                );
                self.resume_after_card_resolution(CardResumeSpec {
                    engine: &engine,
                    session_id,
                    card_id,
                    removed_card,
                    previous_contract: None,
                    visible_prompt: &resume_prompt,
                    resumed: ResumedSubmission {
                        skip_contract_evolve: true,
                        model_prompt_override: Some(resume_prompt.clone()),
                        display_text_override: Some(String::new()),
                        skip_push_user_line: true,
                        preserve_tool_approval: true,
                        prompt_source: MessageSource::CardResume,
                        bypass_busy_queue: true,
                        ..ResumedSubmission::default()
                    },
                    approval_grant: Some((tool_name, args_hash)),
                    denial_rollback: DenialRollback::Unchanged,
                })
            }
            (
                PendingCard::ToolApproval {
                    tool_name,
                    command_preview,
                    ..
                },
                CardSelection::DenyTool { reason },
            ) => {
                let deny_reason = reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("user denied tool execution")
                    .to_owned();
                let removed_card = engine
                    .take_pending_card(
                        session_id,
                        card_id,
                        TurnState::Idle,
                        None,
                    )
                    .map_err(|_| {
                        self.inner.state.report_storage_error();
                        "notice.session_storage_failed"
                    })?
                    .ok_or("notice.card_not_found")?;
                let previous_denial = lock(&self.inner.state.denied_tool_turns)
                    .insert(session_id.to_owned(), deny_reason.clone());
                let deny_prompt = format!(
                    "[UserDenied] 用户已拒绝执行 `{tool_name}`（`{command_preview}`；原因：{deny_reason}）。严禁使用任何等价写命令或替代工具绕过此拒绝，请直接向用户确认已停止该操作。"
                );
                self.resume_after_card_resolution(CardResumeSpec {
                    engine: &engine,
                    session_id,
                    card_id,
                    removed_card,
                    previous_contract: None,
                    visible_prompt: &deny_prompt,
                    resumed: ResumedSubmission {
                        skip_contract_evolve: true,
                        model_prompt_override: Some(deny_prompt.clone()),
                        display_text_override: Some(String::new()),
                        skip_push_user_line: true,
                        preserve_turn_denial: true,
                        prompt_source: MessageSource::CardResume,
                        bypass_busy_queue: true,
                        ..ResumedSubmission::default()
                    },
                    approval_grant: None,
                    denial_rollback: DenialRollback::Restore(previous_denial),
                })
            }
            _ => Err("notice.card_selection_invalid"),
        }
    }

    fn resume_after_card_resolution(
        &self,
        spec: CardResumeSpec<'_>,
    ) -> Result<CardActionOutcome, &'static str> {
        let CardResumeSpec {
            engine,
            session_id,
            card_id,
            removed_card,
            previous_contract,
            visible_prompt,
            resumed,
            approval_grant,
            denial_rollback,
        } = spec;
        let has_remaining_cards = engine
            .with_session(session_id, |rec| !rec.pending_cards.is_empty())
            .unwrap_or(false);
        if has_remaining_cards {
            if let Some(session) =
                lock(&self.inner.state.live).get_mut(session_id)
            {
                session.status = TurnStatus::WaitingForUser;
                session.updated_at = now_ms();
            }
            if persist_index(&self.inner.state).is_err() {
                self.inner.state.report_storage_error();
            }
            self.inner.state.push_update(AgentUpdate::Session {
                id: session_id.to_owned(),
            });
            self.inner.state.push_update(AgentUpdate::Sessions);
            return Ok(CardActionOutcome::CardResolved {
                card_id: card_id.to_owned(),
                resumed: false,
            });
        }
        {
            let mut live = lock(&self.inner.state.live);
            if let Some(session) = live.get_mut(session_id) {
                session.status = TurnStatus::Idle;
                session.projection.lines.retain(|line| {
                    !matches!(
                        line,
                        ChatLine::Status { text }
                            if text == TurnStatus::WaitingForUser.key()
                    )
                });
            }
        }
        let dispatch_result = (|| -> Result<(), &'static str> {
            let mut bytes = [0_u8; 16];
            getrandom::fill(&mut bytes).map_err(|_| "notice.agent_failed")?;
            let submission = u128::from_le_bytes(bytes);
            self.inner.submit_message_targeted_locked(
                Some(session_id),
                visible_prompt,
                Some(submission),
                None,
                None,
                None,
                Some(resumed),
            )?;
            Ok(())
        })();
        if let Err(err) = dispatch_result {
            let _ = engine.park_pending_card(
                session_id,
                removed_card,
                previous_contract,
            );
            if let Some((tool, hash)) = approval_grant {
                let mut grants = lock(&self.inner.state.approved_tool_calls);
                if let Some(list) = grants.get_mut(session_id) {
                    if let Some(pos) =
                        list.iter().rposition(|(t, h)| *t == tool && *h == hash)
                    {
                        list.remove(pos);
                    }
                    if list.is_empty() {
                        grants.remove(session_id);
                    }
                }
            }
            if let DenialRollback::Restore(prev) = denial_rollback {
                let mut denials = lock(&self.inner.state.denied_tool_turns);
                if let Some(old) = prev {
                    denials.insert(session_id.to_owned(), old);
                } else {
                    denials.remove(session_id);
                }
            }
            {
                let mut live = lock(&self.inner.state.live);
                if let Some(session) = live.get_mut(session_id) {
                    session.status = TurnStatus::WaitingForUser;
                    let already_waiting = matches!(
                        session.projection.lines.last(),
                        Some(ChatLine::Status { text })
                            if text == TurnStatus::WaitingForUser.key()
                    );
                    if !already_waiting {
                        session.projection.lines.push(ChatLine::Status {
                            text: TurnStatus::WaitingForUser.key().into(),
                        });
                    }
                }
            }
            let _ = persist_index(&self.inner.state);
            self.inner.state.push_update(AgentUpdate::Session {
                id: session_id.to_owned(),
            });
            self.inner.state.push_update(AgentUpdate::Sessions);
            return Err(err);
        }
        let root_id = self.inner.root_session_id_for(session_id);
        if root_id != session_id {
            self.inner
                .state
                .push_update(AgentUpdate::Session { id: root_id });
        }
        Ok(CardActionOutcome::CardResolved {
            card_id: card_id.to_owned(),
            resumed: true,
        })
    }

    /// Optional graceful-close hint: pauses autonomous draining, cancels
    /// cancellable busy turns (preserving and waiting briefly for finalizing
    /// deliveries), flushes the UI index before and after reaping, and returns
    /// within the bounded timeout. Crash correctness never depends on calling
    /// this.
    pub fn prepare_shutdown(&self) {
        self.prepare_shutdown_with_timeout(std::time::Duration::from_secs(2));
    }

    /// Bounded variant of [`Self::prepare_shutdown`] so UI frameworks with
    /// strict quit deadlines can align the grace wait without losing the
    /// initial `persist_index` flush.
    pub fn prepare_shutdown_with_timeout(&self, timeout: std::time::Duration) {
        self.inner
            .state
            .shutdown_requested
            .store(true, Ordering::Release);
        self.inner
            .state
            .autonomous_drain
            .store(false, Ordering::Release);
        self.inner.state.drain_cancel.cancel();
        // Persist immediately so index updates survive even if the host exits
        // before child-process reaping completes.
        let _ = persist_index(&self.inner.state);
        let mut to_wait: BTreeSet<String> =
            self.busy_session_ids().into_iter().collect();
        let engine = lock(&self.inner.engine).clone();
        if let Some(engine) = &engine {
            for active_id in engine.active_session_ids() {
                to_wait.insert(active_id);
            }
        }
        let mut wait_list = Vec::new();
        for session_id in to_wait {
            if self.inner.is_session_waiting_for_user(&session_id) {
                continue;
            }
            match self.cancel_task(TaskTarget::Session(session_id.clone())) {
                Ok(
                    CancelOutcome::Cancelled
                    | CancelOutcome::FinalizingNotCancellable,
                ) => {
                    // Wait for both cancelled turns reaping child processes AND
                    // finalizing deliveries finishing their atomic commit.
                    wait_list.push(session_id);
                }
                Ok(CancelOutcome::AlreadySettled) => {
                    // May still be reaping after an earlier cancel.
                    if engine
                        .as_ref()
                        .is_some_and(|eng| eng.is_active(&session_id))
                    {
                        wait_list.push(session_id);
                    }
                }
                Err(_) => {}
            }
        }
        if !wait_list.is_empty()
            && !timeout.is_zero()
            && let Some(engine) = engine
        {
            let deadline = std::time::Instant::now() + timeout;
            for session_id in wait_list {
                let remaining = deadline
                    .saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let _ = engine.wait_until_idle(&session_id, remaining);
            }
        }
        let _ = persist_index(&self.inner.state);
    }

    pub fn stop(&self) {
        let reap_wait = {
            let _dispatch = lock(&self.inner.state.dispatch);
            self.stop_locked()
        };
        self.inner.await_fast_cancel_settle(reap_wait);
    }

    pub fn stop_if_active(&self, expected: Option<&str>) {
        let reap_wait = {
            let _dispatch = lock(&self.inner.state.dispatch);
            if lock(&self.inner.state.active).as_deref() == expected {
                self.stop_locked()
            } else {
                None
            }
        };
        self.inner.await_fast_cancel_settle(reap_wait);
    }

    fn stop_locked(&self) -> Option<ReapTarget> {
        let id = lock(&self.inner.state.active).clone()?;
        self.inner
            .cancel_session_locked(&id)
            .ok()
            .and_then(|(_, wait)| wait)
    }
}

impl Harness {
    pub(super) fn cancel_task(
        &self,
        target: TaskTarget,
    ) -> Result<CancelOutcome, &'static str> {
        let (outcome, reap_wait) = {
            let _dispatch = lock(&self.state.dispatch);
            match target {
                TaskTarget::Session(session_id) => {
                    self.cancel_session_locked(&session_id)?
                }
                TaskTarget::Receipt(receipt_id) => {
                    self.cancel_receipt_locked(&receipt_id)?
                }
            }
        };
        self.await_fast_cancel_settle(reap_wait);
        Ok(outcome)
    }

    pub(super) fn cancel_work(
        &self,
        root_session_id: &str,
        worker_id: &str,
    ) -> Result<StopWorkResult, &'static str> {
        let (result, reap_wait) = {
            let _dispatch = lock(&self.state.dispatch);
            let engine =
                lock(&self.engine).clone().ok_or("notice.agent_failed")?;
            let worker =
                verify_child_ownership(&engine, root_session_id, worker_id)?;
            let mut cancelled_receipt_ids =
                super::observer::cancel_receipts_for_sessions(
                    &self.state,
                    std::iter::once(worker_id),
                );
            let (session_outcome, reap_wait) =
                self.cancel_session_locked(worker_id)?;
            if session_outcome == CancelOutcome::FinalizingNotCancellable {
                let committed_artifact_ids: Vec<String> = engine
                    .session(worker_id)
                    .unwrap_or(worker)
                    .delivery_commits
                    .into_iter()
                    .filter(|c| c.completed_successfully)
                    .flat_map(|c| c.artifact_ids)
                    .collect();
                return Ok(StopWorkResult {
                    work_id: worker_id.to_owned(),
                    outcome: "finalizing_not_cancellable".to_owned(),
                    status: TurnStatus::SavingFiles.key().to_owned(),
                    cancelled_receipt_ids,
                    committed_artifact_ids,
                });
            }
            let cleared_waiting = if session_outcome
                == CancelOutcome::AlreadySettled
                && (!worker.pending_cards.is_empty()
                    || worker.status == TurnState::WaitingForUser
                    || lock(&self.state.live).get(worker_id).is_some_and(|s| {
                        s.status == TurnStatus::WaitingForUser
                    })) {
                let _ =
                    engine.clear_pending_cards(worker_id, TurnState::Stopped);
                if let Some(live) = lock(&self.state.live).get_mut(worker_id) {
                    live.status = TurnStatus::Stopped;
                    live.turn_started_at = None;
                    clear_live_heartbeat(live);
                    live.updated_at = now_ms();
                }
                let _ = persist_index(&self.state);
                true
            } else {
                false
            };
            cancelled_receipt_ids.sort();
            cancelled_receipt_ids.dedup();
            let live_status = lock(&self.state.live)
                .get(worker_id)
                .map_or(TurnStatus::Stopped, |s| s.status.clone());
            let outcome = if session_outcome == CancelOutcome::Cancelled {
                if matches!(live_status, TurnStatus::Stopping) {
                    "stopping"
                } else {
                    "cancelled"
                }
            } else if !cancelled_receipt_ids.is_empty() || cleared_waiting {
                "cancelled"
            } else {
                "already_settled"
            };
            self.state.push_update(AgentUpdate::Session {
                id: worker_id.to_owned(),
            });
            self.state.push_update(AgentUpdate::Session {
                id: root_session_id.to_owned(),
            });
            (
                StopWorkResult {
                    work_id: worker_id.to_owned(),
                    outcome: outcome.to_owned(),
                    status: live_status.key().to_owned(),
                    cancelled_receipt_ids,
                    committed_artifact_ids: Vec::new(),
                },
                reap_wait,
            )
        };
        self.await_fast_cancel_settle(reap_wait);
        let mut final_result = result;
        let latest_status = lock(&self.state.live)
            .get(worker_id)
            .map(|s| s.status.clone());
        if let Some(status) = latest_status {
            status.key().clone_into(&mut final_result.status);
            if final_result.outcome == "stopping"
                && !matches!(status, TurnStatus::Stopping)
            {
                "cancelled".clone_into(&mut final_result.outcome);
            }
        }
        Ok(final_result)
    }

    pub(super) fn revise_work(
        self: &Arc<Self>,
        root_session_id: &str,
        worker_id: &str,
        revision_brief: &str,
        request_id: Option<&str>,
    ) -> Result<DispatchWorkResult, &'static str> {
        if self.state.shutdown_requested.load(Ordering::Acquire) {
            return Err("HOST_SHUTTING_DOWN");
        }
        let brief = revision_brief.trim();
        let req_id = request_id.map(str::trim).filter(|s| !s.is_empty());
        if brief.is_empty() {
            return Err("INVALID_ARGS");
        }
        let fingerprint =
            dispatch_fingerprint("revise", Some(worker_id), None, brief, &[]);
        let (engine, worker_mode, reap_wait) = {
            let _dispatch = lock(&self.state.dispatch);
            let engine =
                lock(&self.engine).clone().ok_or("notice.agent_failed")?;
            let worker =
                verify_child_ownership(&engine, root_session_id, worker_id)?;
            if let Some(dedup) = self.lookup_idempotent_dispatch(
                &engine,
                root_session_id,
                req_id,
                &fingerprint,
                Some(worker.access_mode),
            )? {
                return Ok(dedup);
            }
            let _ = super::observer::cancel_receipts_for_sessions(
                &self.state,
                std::iter::once(worker_id),
            );
            // If the old turn is already inside atomic `Finalizing`, let it
            // finish committing cleanly and wait for idle below before
            // enqueuing the revision (only `cancel_work` fails closed with
            // `finalizing_not_cancellable`).
            let (_cancel_outcome, reap_wait) =
                self.cancel_session_locked(worker_id)?;
            (engine, worker.access_mode, reap_wait)
        };

        // Wait outside `_dispatch` for the old Worker turn and its workspace
        // lease to settle before enqueuing strictly one revised receipt.
        self.await_fast_cancel_settle(reap_wait);
        if engine.is_active(worker_id)
            && !engine.wait_until_idle(worker_id, Duration::from_secs(3))
        {
            return Err("notice.agent_busy");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.is_session_busy_or_active(worker_id)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.is_session_busy_or_active(worker_id) {
            return Err("notice.agent_busy");
        }

        let dispatched = {
            let _dispatch = lock(&self.state.dispatch);
            if self.state.shutdown_requested.load(Ordering::Acquire) {
                return Err("HOST_SHUTTING_DOWN");
            }
            let worker =
                verify_child_ownership(&engine, root_session_id, worker_id)?;
            if let Some(dedup) = self.lookup_idempotent_dispatch(
                &engine,
                root_session_id,
                req_id,
                &fingerprint,
                Some(worker.access_mode),
            )? {
                return Ok(dedup);
            }
            let _ = super::observer::cancel_receipts_for_sessions(
                &self.state,
                std::iter::once(worker_id),
            );
            if !worker.pending_cards.is_empty()
                || worker.status == TurnState::WaitingForUser
            {
                let _ = engine.clear_pending_cards(worker_id, TurnState::Idle);
                if let Some(live) = lock(&self.state.live).get_mut(worker_id) {
                    live.status = TurnStatus::Idle;
                    live.updated_at = now_ms();
                }
                let _ = persist_index(&self.state);
            }
            let now = now_ms();
            let queue = AcceptanceQueue::new(self.state.root.clone());
            let receipt = queue
                .accept_strict(worker_id, brief, req_id, now)
                .map_err(|_| {
                    self.state.report_storage_error();
                    "notice.session_storage_failed"
                })?;
            if let Some(explicit_id) = req_id
                && engine
                    .record_dispatch_request(
                        root_session_id,
                        DispatchRecord {
                            request_id: explicit_id.to_owned(),
                            fingerprint,
                            worker_id: worker_id.to_owned(),
                            receipt_id: receipt.id.clone(),
                            created_at_ms: now,
                        },
                    )
                    .is_err()
            {
                let _ = queue.cancel(&receipt.id, now_ms());
                self.state.report_storage_error();
                return Err("notice.session_storage_failed");
            }
            self.state.push_update(AgentUpdate::Receipt {
                id: receipt.id.clone(),
                state: ReceiptState::Queued,
            });
            self.state.push_update(AgentUpdate::Session {
                id: root_session_id.to_owned(),
            });
            DispatchWorkResult {
                work_id: worker_id.to_owned(),
                receipt_id: receipt.id,
                status: "queued".to_owned(),
                access_mode: worker_mode,
                deduplicated: false,
            }
        };
        if self.state.autonomous_drain.load(Ordering::Acquire) {
            self.kick_autonomous_drain();
        }
        Ok(dispatched)
    }

    /// Fast stream aborts unwind within a few milliseconds; wait briefly
    /// OUTSIDE `_dispatch` so other sessions are never blocked while genuinely
    /// slow-reaping child tools remain honestly observable in `Stopping`.
    pub(super) fn await_fast_cancel_settle(&self, target: Option<ReapTarget>) {
        let Some((eng, id)) = target else {
            return;
        };
        if eng.wait_until_idle(&id, std::time::Duration::from_millis(50)) {
            let transitioned = {
                let mut live = lock(&self.state.live);
                if let Some(session) = live.get_mut(&id)
                    && matches!(session.status, TurnStatus::Stopping)
                {
                    session.status = TurnStatus::Stopped;
                    session.turn_started_at = None;
                    clear_live_heartbeat(session);
                    session.updated_at = now_ms();
                    let already_stopped = matches!(
                        session.projection.lines.last(),
                        Some(ChatLine::Status { text })
                            if text == TurnStatus::Stopped.key()
                    );
                    if !already_stopped {
                        session.projection.lines.push(ChatLine::Status {
                            text: TurnStatus::Stopped.key().into(),
                        });
                    }
                    true
                } else {
                    false
                }
            };
            if transitioned {
                if persist_index(&self.state).is_err() {
                    self.state.report_storage_error();
                }
                self.state.push_update(AgentUpdate::Session { id });
                self.state.push_update(AgentUpdate::Sessions);
            }
        }
    }

    pub(super) fn cancel_session_locked(
        &self,
        id: &str,
    ) -> Result<(CancelOutcome, Option<ReapTarget>), &'static str> {
        let engine = lock(&self.engine).clone();
        let (live_busy, live_stopping) =
            lock(&self.state.live)
                .get(id)
                .map_or((false, false), |session| {
                    (
                        session.status.is_busy(),
                        matches!(session.status, TurnStatus::Stopping),
                    )
                });
        let engine_cancelling =
            engine.as_ref().is_some_and(|eng| eng.is_cancelling(id));
        if live_stopping || engine_cancelling {
            return Ok((CancelOutcome::AlreadySettled, None));
        }
        let engine_busy = engine.as_ref().is_some_and(|eng| {
            eng.session(id).is_some_and(|rec| rec.status.is_busy())
                || eng.is_active(id)
        });
        if !live_busy && !engine_busy {
            return Ok((CancelOutcome::AlreadySettled, None));
        }
        let mut storage_err: Option<&'static str> = None;
        if let Some(eng) = &engine {
            match eng.request_cancel(id) {
                Ok(false) => {
                    return Ok((CancelOutcome::FinalizingNotCancellable, None));
                }
                Ok(true) => {}
                Err(error) => {
                    log_storage_io_error(
                        "cancel agent turn",
                        &flowy_agent_core::SessionIndex::path(&self.state.root),
                        &error,
                    );
                    self.state.report_storage_error();
                    storage_err = Some("notice.session_storage_failed");
                }
            }
        }
        let still_reaping =
            engine.as_ref().is_some_and(|eng| eng.is_active(id));
        {
            let mut live = lock(&self.state.live);
            if let Some(session) = live.get_mut(id) {
                session.title_signal.cancel();
                session.projection.flush();
                if session.status.is_busy() {
                    if still_reaping {
                        session.status = TurnStatus::Stopping;
                        let hb = session
                            .heartbeat
                            .get_or_insert_with(Default::default);
                        "stopping".clone_into(&mut hb.current_action);
                        hb.next_check_hint = Some(
                            "reaping active tools and writing stop handoff"
                                .to_owned(),
                        );
                        hb.awaiting_evidence = true;
                    } else {
                        session.status = TurnStatus::Stopped;
                        session.projection.lines.push(ChatLine::Status {
                            text: session.status.key().into(),
                        });
                        session.turn_started_at = None;
                    }
                }
                session.updated_at = now_ms();
            }
        }
        let matching_claims =
            super::take_session_claims(&self.state.active_claims, id);
        for claim in matching_claims {
            claim.heartbeat_cancel.cancel();
            let queue = AcceptanceQueue::new(self.state.root.clone());
            if queue.cancel(&claim.receipt_id, now_ms()).unwrap_or(false) {
                self.state.push_update(AgentUpdate::Receipt {
                    id: claim.receipt_id,
                    state: ReceiptState::Cancelled,
                });
            }
        }
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
        self.state
            .push_update(AgentUpdate::Session { id: id.to_owned() });
        self.state.push_update(AgentUpdate::Sessions);
        if let Some(err) = storage_err {
            return Err(err);
        }
        let reap_wait = if still_reaping {
            engine.map(|eng| (eng, id.to_owned()))
        } else {
            None
        };
        Ok((CancelOutcome::Cancelled, reap_wait))
    }

    fn cancel_receipt_locked(
        &self,
        receipt_id: &str,
    ) -> Result<(CancelOutcome, Option<ReapTarget>), &'static str> {
        let active_session = lock(&self.state.active_claims)
            .get(receipt_id)
            .map(|claim| claim.session_id.clone());
        if let Some(session_id) = active_session {
            return self.cancel_session_locked(&session_id);
        }
        let queue = AcceptanceQueue::new(self.state.root.clone());
        match queue.cancel(receipt_id, now_ms()) {
            Ok(true) => {
                self.state.push_update(AgentUpdate::Receipt {
                    id: receipt_id.to_owned(),
                    state: ReceiptState::Cancelled,
                });
                Ok((CancelOutcome::Cancelled, None))
            }
            Ok(false) => Ok((CancelOutcome::AlreadySettled, None)),
            Err(_) => {
                self.state.report_storage_error();
                Err("notice.session_storage_failed")
            }
        }
    }
}

pub(super) fn latest_session_submission_id(
    record: &SessionRecord,
) -> Option<String> {
    record
        .messages
        .iter()
        .rev()
        .find(|msg| {
            msg.role == Role::User
                && !matches!(msg.source, MessageSource::SystemEvent { .. })
                && (msg.id.starts_with("submission-")
                    || record.submission_outcomes.contains_key(&msg.id)
                    || record.submission_revisions.contains_key(&msg.id))
        })
        .map(|msg| msg.id.clone())
        .or_else(|| {
            record
                .delivery_commits
                .iter()
                .rev()
                .find(|commit| {
                    !commit.submission_id.starts_with("submission-override-")
                })
                .or_else(|| record.delivery_commits.last())
                .map(|commit| commit.submission_id.clone())
        })
        .or_else(|| {
            record
                .submission_revisions
                .iter()
                .max_by_key(|(_, rev)| **rev)
                .map(|(sub, _)| sub.clone())
        })
        .or_else(|| record.submission_outcomes.keys().next_back().cloned())
}

/// Returns `true` when `submission_id` has crossed its durable LLM commit point.
///
/// Matches `SubmissionOutcome::Completed | Partial` or a successful
/// `DeliveryCommit` in `history.json`. Even if the Library marker still needs
/// `retry_delivery`, the LLM turn must never be re-executed on restart. Never
/// mistakes an `Error`, `Aborted`, or pre-run `User`-only prompt for committed.
pub(super) fn submission_committed_in_session(
    record: &SessionRecord,
    submission_id: &str,
) -> bool {
    if let Some(outcome) = record.submission_outcomes.get(submission_id) {
        return matches!(
            outcome,
            SubmissionOutcome::Completed | SubmissionOutcome::Partial { .. }
        );
    }
    if let Some(commit) = record
        .delivery_commits
        .iter()
        .find(|commit| commit.submission_id == submission_id)
    {
        return commit.completed_successfully;
    }
    // Legacy fallback for sessions written before `submission_outcomes`
    // existed: only an explicitly Completed session with post-prompt output
    // qualifies; Error / Stopped / Running never reconcile as Completed.
    if record.status != TurnState::Completed {
        return false;
    }
    let Some(user_idx) = record
        .messages
        .iter()
        .position(|msg| msg.id == submission_id && msg.role == Role::User)
    else {
        return false;
    };
    record.messages[user_idx + 1..]
        .iter()
        .take_while(|msg| {
            !matches!(
                msg.source,
                MessageSource::User | MessageSource::CardResume
            )
        })
        .any(|msg| {
            msg.role == Role::Assistant
                || matches!(msg.source, MessageSource::Tool { .. })
        })
}

/// Re-evaluate `Partial` submissions for `target_revision` after an override.
///
/// `extra_delivered` passes a single reused snapshot's `(display_name,
/// delivery_key)` without mutating a prior turn's `submission_revisions`.
pub(super) fn reconcile_partial_work_after_override(
    engine: &Engine,
    store: &ArtifactStore,
    session_id: &str,
    target_revision: Option<u32>,
    extra_delivered: Option<(&str, &str)>,
) {
    reconcile_partial_work_inner(
        engine,
        store,
        session_id,
        target_revision,
        extra_delivered,
    );
}

pub(super) fn reconcile_partial_work_for_revision(
    engine: &Engine,
    store: &ArtifactStore,
    session_id: &str,
    target_revision: Option<u32>,
) {
    reconcile_partial_work_inner(
        engine,
        store,
        session_id,
        target_revision,
        None,
    );
}

fn missing_item_satisfied(
    missing_label: &str,
    delivered: &[(&str, &str)],
) -> bool {
    let label = missing_label.trim();
    if label.is_empty() {
        return false;
    }
    let lower = label.to_ascii_lowercase();
    delivered.iter().any(|(name, key)| {
        let name_lower = name.to_ascii_lowercase();
        let key_lower = key.to_ascii_lowercase();
        if let Some(ext) = lower.strip_prefix("*.") {
            let suffix = format!(".{ext}");
            name_lower.ends_with(&suffix) || key_lower.ends_with(&suffix)
        } else if lower.contains('.') {
            name_lower == lower || key_lower == lower
        } else {
            let suffix = format!(".{lower}");
            name_lower.ends_with(&suffix) || key_lower.ends_with(&suffix)
        }
    })
}

fn reconcile_partial_work_inner(
    engine: &Engine,
    store: &ArtifactStore,
    session_id: &str,
    target_revision: Option<u32>,
    extra_delivered: Option<(&str, &str)>,
) {
    let Some(record) = engine.session(session_id) else {
        return;
    };
    let Some(contract) = record.work_contract.as_ref() else {
        return;
    };
    if contract.required_items.is_empty() {
        return;
    }
    let active_rev = target_revision.unwrap_or(contract.revision);
    let Ok(committed_ids) = store.committed_ids() else {
        return;
    };
    let mut delivered: Vec<(String, String)> = Vec::new();
    for commit in &record.delivery_commits {
        let commit_rev = record
            .submission_revisions
            .get(&commit.submission_id)
            .copied();
        if commit_rev != Some(active_rev) {
            continue;
        }
        for artifact_id in &commit.artifact_ids {
            if committed_ids.contains(artifact_id)
                && let Ok(Some(item)) = store.get(artifact_id)
                && !matches!(
                    item.manifest.publication_state,
                    PublicationState::Deleted | PublicationState::DeletePending
                )
            {
                delivered.push((
                    item.manifest.receipt.display_name,
                    item.manifest.receipt.delivery_key,
                ));
            }
        }
    }
    if let Some((name, key)) = extra_delivered {
        delivered.push((name.to_owned(), key.to_owned()));
    }
    let pairs: Vec<(&str, &str)> = delivered
        .iter()
        .map(|(n, k)| (n.as_str(), k.as_str()))
        .collect();
    let legacy_untagged = record.submission_revisions.is_empty()
        && active_rev == contract.revision;
    let partial_subs: Vec<(String, Vec<String>)> = record
        .submission_outcomes
        .iter()
        .filter_map(|(sub_id, outcome)| {
            let SubmissionOutcome::Partial { missing } = outcome else {
                return None;
            };
            let matches_rev = match record.submission_revisions.get(sub_id) {
                Some(&rev) => rev == active_rev,
                None => legacy_untagged,
            };
            matches_rev.then(|| (sub_id.clone(), missing.clone()))
        })
        .collect();
    if partial_subs.is_empty() {
        return;
    }
    let current_rev_missing = (active_rev == contract.revision)
        .then(|| contract.missing_deliverables(&pairs));
    for (sub_id, old_missing) in partial_subs {
        let missing = if let Some(computed) = &current_rev_missing {
            computed.clone()
        } else {
            // Older revision after `record.work_contract` evolved: narrow the
            // recorded `Partial { missing }` list against `active_rev`'s
            // delivered items rather than injecting newer-revision items.
            old_missing
                .into_iter()
                .filter(|item| !missing_item_satisfied(item, &pairs))
                .collect()
        };
        let next = if missing.is_empty() {
            SubmissionOutcome::Completed
        } else {
            SubmissionOutcome::Partial { missing }
        };
        let _ = engine.set_submission_outcome(session_id, &sub_id, next);
    }
}

fn build_work_status_borrowed<F>(
    work_id: &str,
    receipt_state: Option<ReceiptState>,
    record: Option<&SessionRecord>,
    session_id: Option<String>,
    submission_id: Option<String>,
    committed_ids: &BTreeSet<String>,
    lookup_artifact: &mut F,
) -> WorkStatus
where
    F: FnMut(&str) -> Option<(PublicationState, String, String)>,
{
    let mut delivered_artifact_ids = Vec::new();
    let mut contract_revision = None;
    let mut required_items = Vec::new();
    let mut outcome = None;
    let mut missing_items = Vec::new();
    if let Some(rec) = record {
        contract_revision = submission_id
            .as_deref()
            .and_then(|sub| rec.submission_revisions.get(sub).copied())
            .or_else(|| rec.work_contract.as_ref().map(|c| c.revision));
        required_items = rec
            .work_contract
            .as_ref()
            .map_or_default(|c| c.required_items.clone());
        outcome = submission_id
            .as_deref()
            .and_then(|sub| rec.submission_outcomes.get(sub).cloned());
        for commit in &rec.delivery_commits {
            let belongs = match submission_id.as_deref() {
                Some(sub) => {
                    commit.submission_id == sub
                        || contract_revision.is_some_and(|rev| {
                            rec.submission_revisions
                                .get(&commit.submission_id)
                                .copied()
                                == Some(rev)
                        })
                }
                None => true,
            };
            if belongs {
                for artifact_id in &commit.artifact_ids {
                    if committed_ids.contains(artifact_id)
                        && !delivered_artifact_ids.contains(artifact_id)
                        && lookup_artifact(artifact_id).is_none_or(
                            |(pub_state, _, _)| {
                                !matches!(
                                    pub_state,
                                    PublicationState::Deleted
                                        | PublicationState::DeletePending
                                )
                            },
                        )
                    {
                        delivered_artifact_ids.push(artifact_id.clone());
                    }
                }
            }
        }
        if let Some(SubmissionOutcome::Partial { missing }) = &outcome {
            missing_items.clone_from(missing);
        } else if !matches!(outcome, Some(SubmissionOutcome::Completed))
            && let Some(contract) = &rec.work_contract
            && !contract.required_items.is_empty()
        {
            let delivered_meta: Vec<(String, String)> = delivered_artifact_ids
                .iter()
                .filter_map(|id| {
                    lookup_artifact(id).map(|(_, name, key)| (name, key))
                })
                .collect();
            let pairs: Vec<(&str, &str)> = delivered_meta
                .iter()
                .map(|(n, k)| (n.as_str(), k.as_str()))
                .collect();
            missing_items = contract.missing_deliverables(&pairs);
        }
    }
    WorkStatus {
        work_id: work_id.to_owned(),
        session_id,
        submission_id,
        contract_revision,
        receipt_state,
        outcome,
        required_items,
        delivered_artifact_ids,
        missing_items,
    }
}
