//! Turn submission, tool/skill prompt assembly, and streaming execution dispatch.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use flowy_agent_core::{
    AcceptanceQueue, BranchHeartbeat, Engine, EnqueueOutcome, LoopOutcome,
    PendingCard, PendingRouterFollowup, PromptTurn, ReceiptState, RunError,
    SessionOrigin, SessionRecord, SessionRole, ToolProviderContext, TurnState,
    WorkAccessMode, echo_stream_fn, log_preview,
    types::{CancelToken, MessageSource, Role},
};

use super::guards::{
    PreStagedWorkerProviders, PreToolHookSpec, build_pre_tool_hook,
    merge_staged_providers, mount_tool_providers, normalize_cwd_key,
    read_only_tool_registry, session_working_directory, stage_tool_providers,
    tool_registry,
};
use super::observer::{
    self, capture_turn_artifacts, ensure_engine_session, persist_index,
};
use super::{
    AcceptedClaim, AgentUpdate, BASE_BEHAVIOR_PROMPT, ChatLine, Harness,
    MAX_AUTO_TITLE_ATTEMPTS, RECEIPT_SUPERSEDED_ERR, ResumedSubmission,
    SubmissionSelection, TITLE_MAX_CHARS, TurnStatus, append_skill_listing,
    compose_semantic_brief, engine_config, lock, log_storage_io_error,
    model_route, now_ms, prepare_skill_prompt, router, take_session_claims,
    title, work_events, workspace_has_user_files,
};

impl Harness {
    pub(super) fn resolve_session_cwd(
        &self,
        session_id: &str,
        record: &SessionRecord,
    ) -> PathBuf {
        let workspace = lock(&self.state.live)
            .get(session_id)
            .and_then(|session| session.workspace.clone());
        record.workspace.as_ref().map_or_else(
            || session_working_directory(&self.state.root, workspace.as_ref()),
            |workspace| workspace.root.clone(),
        )
    }

    /// Collect Worker `ToolProvider::register_tools()` and prompt hints
    /// BEFORE acquiring `self.state.dispatch` so slow extension providers
    /// cannot stall concurrent `Router` submissions.
    pub(super) fn pre_stage_worker_providers(
        &self,
        explicit_session_id: Option<&str>,
    ) -> Option<PreStagedWorkerProviders> {
        let target_id = match explicit_session_id {
            Some(id) => id.to_owned(),
            None => lock(&self.state.active).clone()?,
        };
        let engine = lock(&self.engine).clone()?;
        let core_session = engine.session(&target_id)?;
        if core_session.role == SessionRole::Router
            || core_session.access_mode == WorkAccessMode::ReadOnly
        {
            return None;
        }
        let cwd = self.resolve_session_cwd(&target_id, &core_session);
        let cwd_key = normalize_cwd_key(&cwd);
        let providers = self.tool_providers();
        let provider_signatures: Vec<(String, u64)> = providers
            .iter()
            .map(|entry| {
                (entry.provider.id().trim().to_owned(), entry.registration_id)
            })
            .collect();
        let ctx = ToolProviderContext {
            session_id: &target_id,
            cwd: &cwd,
            origin: &core_session.origin,
            access_mode: core_session.access_mode,
        };
        let staged = stage_tool_providers(&providers, &ctx);
        Some(PreStagedWorkerProviders {
            session_id: target_id,
            cwd_key,
            origin: core_session.origin,
            access_mode: core_session.access_mode,
            provider_signatures,
            staged,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "thin lock wrapper forwarding all dispatch arguments to submit_message_targeted_with_staged"
    )]
    pub(super) fn submit_message_targeted(
        self: &Arc<Self>,
        explicit_session_id: Option<&str>,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
        pending_claim: Option<AcceptedClaim>,
        reply_to_object_id: Option<&str>,
        resumed: Option<ResumedSubmission>,
    ) -> Result<String, &'static str> {
        let pre_staged = self.pre_stage_worker_providers(explicit_session_id);
        let _dispatch = lock(&self.state.dispatch);
        self.submit_message_targeted_with_staged(
            explicit_session_id,
            text,
            submission,
            selection,
            pending_claim,
            reply_to_object_id,
            resumed,
            pre_staged,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal dispatch entrypoint shares one lock-validated path across direct chat, receipts, and card resume"
    )]
    pub(super) fn submit_message_targeted_locked(
        self: &Arc<Self>,
        explicit_session_id: Option<&str>,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
        pending_claim: Option<AcceptedClaim>,
        reply_to_object_id: Option<&str>,
        mut resumed: Option<ResumedSubmission>,
    ) -> Result<String, &'static str> {
        let pre_staged =
            resumed.as_mut().and_then(|item| item.pre_staged.take());
        self.submit_message_targeted_with_staged(
            explicit_session_id,
            text,
            submission,
            selection,
            pending_claim,
            reply_to_object_id,
            resumed,
            pre_staged,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal dispatch entrypoint shares one lock-validated path across direct chat, receipts, and card resume"
    )]
    fn submit_message_targeted_with_staged(
        self: &Arc<Self>,
        explicit_session_id: Option<&str>,
        text: &str,
        submission: Option<u128>,
        selection: Option<SubmissionSelection<'_>>,
        pending_claim: Option<AcceptedClaim>,
        reply_to_object_id: Option<&str>,
        mut resumed: Option<ResumedSubmission>,
        pre_staged: Option<PreStagedWorkerProviders>,
    ) -> Result<String, &'static str> {
        let text = text.trim();
        tracing::debug!(
            target: "flowy.prompt",
            stage = "AgentHandle::submit_message",
            chars = text.chars().count(),
            retry = submission.is_some(),
            "enter"
        );
        if text.is_empty() {
            return Err("notice.prompt_empty");
        }
        if self.is_initializing() {
            return Err("library.loading");
        }
        let state = &self.state;
        if *lock(&state.load_failed) {
            return Err("notice.session_storage_failed");
        }
        // Re-verify receipt liveness inside `dispatch` so a concurrent
        // `cancel_task(TaskTarget::Receipt)` between `bind` and submit never
        // dispatches a cancelled prompt (closes Window A TOCTOU).
        if let Some(claim) = &pending_claim {
            let queue = AcceptanceQueue::new(state.root.clone());
            let still_owned = queue
                .get(&claim.receipt_id)
                .map_err(|_| "notice.session_storage_failed")?
                .is_some_and(|receipt| {
                    receipt.completed_at_ms.is_none()
                        && !receipt.dead
                        && !receipt.cancelled
                        && !receipt.quarantined
                        && receipt.lease.as_ref().is_some_and(|lease| {
                            lease.attempt_id == claim.attempt_id
                        })
                });
            if !still_owned {
                return Err(RECEIPT_SUPERSEDED_ERR);
            }
        }
        let engine = lock(&self.engine).clone().ok_or("notice.agent_failed")?;
        let bypass_busy_queue =
            resumed.as_ref().is_some_and(|item| item.bypass_busy_queue);
        let is_router_announcement = resumed
            .as_ref()
            .is_some_and(|item| item.is_router_announcement);
        if let Some(sub) = submission {
            if bypass_busy_queue {
                let message_id = format!("submission-{sub:032x}");
                let committed = engine.with_sessions(|records| {
                    records.iter().find_map(|rec| {
                        rec.messages
                            .iter()
                            .any(|m| m.role == Role::User && m.id == message_id)
                            .then(|| rec.id.clone())
                    })
                });
                if let Some(session) = committed {
                    return Ok(session);
                }
            } else if let Some(session) = engine.submitted_session(sub) {
                return Ok(session);
            }
        }
        if let Some(sub) = submission {
            let expected_card_id = format!("disambig-submission-{sub:032x}");
            if let Some(existing_sid) =
                engine.sessions().into_iter().find_map(|rec| {
                    rec.pending_cards
                        .iter()
                        .any(|card| card.card_id() == expected_card_id)
                        .then_some(rec.id)
                })
            {
                return Ok(existing_sid);
            }
        }
        if selection.is_some_and(|selection| {
            self.state.context_generation.load(Ordering::Acquire)
                != selection.generation
                || lock(&state.active).as_deref() != selection.active
        }) {
            return Err("notice.agent_busy");
        }
        let id = match explicit_session_id {
            Some(target_id) => {
                if !lock(&state.live).contains_key(target_id) {
                    return Err("notice.agent_failed");
                }
                target_id.to_owned()
            }
            None => self.ensure_active_session_locked()?,
        };
        if let Err(error) = ensure_engine_session(&engine, &id) {
            log_storage_io_error(
                "ensure structured session",
                &flowy_agent_core::SessionIndex::path(&state.root),
                &error,
            );
            state.report_storage_error();
        }
        let core_session = engine.session(&id).ok_or("notice.agent_failed")?;
        let is_resumed = resumed.is_some();
        let is_card_resume = resumed.as_ref().is_some_and(|item| {
            matches!(item.prompt_source, MessageSource::CardResume)
                || item.contract.is_some()
        });
        if !is_card_resume
            && (!core_session.pending_cards.is_empty()
                || core_session.status == TurnState::WaitingForUser
                || lock(&state.live).get(&id).is_some_and(|session| {
                    session.status == TurnStatus::WaitingForUser
                }))
        {
            return Err("notice.card_pending");
        }
        self.settle_finishing_fence(&engine, &id);
        let target_busy = lock(&state.live)
            .get(&id)
            .is_some_and(|session| session.status.is_busy())
            || engine.is_active(&id);
        if core_session.role == SessionRole::Router
            && target_busy
            && !bypass_busy_queue
            && pending_claim.is_none()
        {
            let sub_u128 = if let Some(sub) = submission {
                sub
            } else {
                let mut bytes = [0_u8; 16];
                getrandom::fill(&mut bytes)
                    .map_err(|_| "notice.agent_failed")?;
                u128::from_le_bytes(bytes)
            };
            let sub_id = format!("submission-{sub_u128:032x}");
            let outcome = engine
                .enqueue_router_followup(
                    &id,
                    PendingRouterFollowup {
                        submission_id: sub_id,
                        text: text.to_owned(),
                        reply_to_object_id: reply_to_object_id
                            .map(str::to_owned),
                        created_at_ms: now_ms(),
                    },
                    work_events::MAX_ROUTER_FOLLOWUPS,
                )
                .map_err(|error| match error {
                    RunError::Storage(io_err) => {
                        log_storage_io_error(
                            "persist router follow-up",
                            &flowy_agent_core::SessionIndex::path(&state.root),
                            &io_err,
                        );
                        state.report_storage_error();
                        "notice.session_storage_failed"
                    }
                    _ => "notice.agent_failed",
                })?;
            match outcome {
                EnqueueOutcome::Duplicate => return Ok(id),
                EnqueueOutcome::QueueFull => {
                    return Err("notice.router_queue_full");
                }
                EnqueueOutcome::Enqueued => {
                    {
                        let mut live = lock(&state.live);
                        if let Some(session) = live.get_mut(&id) {
                            session.projection.push_user(text);
                            session.updated_at = now_ms();
                        }
                    }
                    if persist_index(state).is_err() {
                        state.report_storage_error();
                    }
                    state.push_update(AgentUpdate::Session { id: id.clone() });
                    return Ok(id);
                }
            }
        }
        if target_busy
            || (pending_claim.is_some()
                && core_session.role != SessionRole::Router
                && self.in_flight_work_count() >= self.max_concurrent_works())
        {
            return Err("notice.agent_busy");
        }
        let config = lock(&self.config).clone();
        let skills = self.skill_catalog();
        let (mut prompt, active_skills) = prepare_skill_prompt(text, &skills)?;
        let prompt_source = resumed
            .as_ref()
            .map_or(MessageSource::User, |item| item.prompt_source.clone());
        let skip_push_user_line = resumed
            .as_ref()
            .is_some_and(|item| item.skip_push_user_line);
        let skip_contract_evolve = resumed
            .as_ref()
            .is_some_and(|item| item.skip_contract_evolve);
        let preserve_turn_denial = resumed
            .as_ref()
            .is_some_and(|item| item.preserve_turn_denial);
        let preserve_tool_approval = resumed
            .as_ref()
            .is_some_and(|item| item.preserve_tool_approval);
        let force_reconfirm_tools = resumed.as_mut().map_or_default(|item| {
            std::mem::take(&mut item.force_reconfirm_tools)
        });
        if let Some(override_model) = resumed
            .as_ref()
            .and_then(|item| item.model_prompt_override.clone())
        {
            let display = resumed
                .as_ref()
                .and_then(|item| item.display_text_override.clone())
                .or_else(|| Some(text.to_owned()));
            prompt = match &prompt_source {
                MessageSource::SystemEvent { event_kind } => {
                    PromptTurn::system_event(event_kind.clone(), override_model)
                }
                MessageSource::CardResume => {
                    PromptTurn::card_resume(override_model, display)
                }
                _ => PromptTurn {
                    model: override_model,
                    display,
                    source: prompt_source.clone(),
                    dispatch_receipt_id: None,
                },
            };
        } else {
            if let Some(suffix) = resumed
                .as_ref()
                .and_then(|item| item.model_prompt_suffix.as_deref())
            {
                prompt.model.push_str(suffix);
            }
            prompt.source = prompt_source;
        }
        let turn_dispatch_receipt_id = pending_claim
            .as_ref()
            .map(|claim| claim.receipt_id.clone())
            .or_else(|| {
                resumed
                    .as_ref()
                    .and_then(|item| item.dispatch_receipt_id.clone())
            })
            .filter(|rid| !rid.trim().is_empty());
        prompt
            .dispatch_receipt_id
            .clone_from(&turn_dispatch_receipt_id);
        let is_background_announcement = is_router_announcement
            || matches!(prompt.source, MessageSource::SystemEvent { .. });
        let is_internal_event_turn = is_background_announcement
            || matches!(prompt.source, MessageSource::CardResume);
        let skip_auto_title =
            core_session.role == SessionRole::Worker || is_internal_event_turn;
        let pre_resolved_contract = resumed.and_then(|item| item.contract);
        let title_prompt =
            if is_resumed || observer::is_internal_control_line_text(text) {
                lock(&state.live)
                    .get(&id)
                    .and_then(|session| {
                        session.projection.lines.iter().find_map(|line| {
                            match line {
                            ChatLine::User { text }
                                if !text.trim().is_empty()
                                    && !observer::is_internal_control_line_text(
                                        text,
                                    ) =>
                            {
                                Some(text.clone())
                            }
                            _ => None,
                        }
                        })
                    })
                    .unwrap_or_else(|| text.to_owned())
            } else {
                text.to_owned()
            };
        let stream = config
            .api_key
            .is_none()
            .then(|| echo_stream_fn(text.to_owned()));
        let _ = is_resumed;
        let cwd = self.resolve_session_cwd(&id, &core_session);
        let cwd_key = normalize_cwd_key(&cwd);
        if !preserve_turn_denial {
            lock(&state.denied_tool_turns).remove(&id);
        }
        if !preserve_tool_approval {
            lock(&state.approved_tool_calls).remove(&id);
            if !is_card_resume && core_session.pending_cards.is_empty() {
                lock(&state.pending_provider_approvals)
                    .retain(|(sid, _), _| sid != &id);
            }
        }
        let mut run_config = engine_config(&config, &cwd, &state.root);
        let mut staged_intents_for_turn: Option<
            crate::publish_tool::StagedIntentLog,
        > = None;
        if core_session.role == SessionRole::Router {
            run_config.registry = router::router_tool_registry(
                Arc::downgrade(self),
                id.clone(),
                is_router_announcement,
            );
            run_config.pre_tool_hook = (!is_router_announcement).then(|| {
                router::build_router_pre_tool_hook(
                    Arc::downgrade(self),
                    id.clone(),
                )
            });
            run_config.system = router::build_router_system_prompt(
                self,
                &id,
                is_router_announcement,
            );
        } else if core_session.access_mode == WorkAccessMode::ReadOnly {
            run_config.registry =
                read_only_tool_registry(&cwd, &skills, active_skills);
            run_config.pre_tool_hook = None;
            run_config.system = append_skill_listing(
                &format!(
                    "{BASE_BEHAVIOR_PROMPT} You are running in read-only inspection mode. Inspect files using only read_file, glob, and grep; do not attempt to modify files or execute shell commands."
                ),
                &skills,
            );
        } else {
            run_config.registry =
                tool_registry(&cwd, &state.root, &skills, active_skills);
            if core_session.origin == SessionOrigin::Home
                && let Some(submission) = submission
                && let Some(session_workspace) = core_session.workspace.as_ref()
                && let Some(store) = state.artifact_store.get().cloned()
            {
                let contract = if let Some(resolved) = pre_resolved_contract {
                    resolved
                } else if skip_contract_evolve {
                    core_session.work_contract.as_ref().map_or_default(
                        crate::verify::BriefContract::from_work_contract,
                    )
                } else {
                    match crate::verify::resolve_contract_sync(
                        core_session.work_contract.as_ref(),
                        text,
                        reply_to_object_id,
                        now_ms(),
                    ) {
                        crate::verify::ResolveOutcome::Ambiguous {
                            mut card,
                            ..
                        } => {
                            if let PendingCard::Disambiguation {
                                card_id,
                                dispatch_receipt_id,
                                ..
                            } = &mut card
                            {
                                *card_id = format!(
                                    "disambig-submission-{submission:032x}"
                                );
                                dispatch_receipt_id
                                    .clone_from(&turn_dispatch_receipt_id);
                            }
                            return self.park_disambiguation_locked(
                                &engine,
                                &id,
                                card,
                                text,
                                skip_push_user_line,
                                pending_claim,
                            );
                        }
                        crate::verify::ResolveOutcome::Resolved {
                            contract,
                            ..
                        } => contract,
                    }
                };
                let submission_id = format!("submission-{submission:032x}");
                let output_root =
                    session_workspace.root.join("outputs").join(&submission_id);
                std::fs::create_dir_all(&output_root).map_err(|error| {
                    log_storage_io_error(
                        "create turn output directory",
                        &output_root,
                        &error,
                    );
                    "notice.session_storage_failed"
                })?;
                let delivery = crate::delivery::DeliveryStore::new(
                    state.root.join("delivery"),
                );
                if core_session.work_contract.is_some() || !contract.is_empty()
                {
                    run_config.work_contract =
                        Some(contract.to_work_contract());
                }
                let expected_revision = run_config
                    .work_contract
                    .as_ref()
                    .map(|wc| wc.revision.max(1));
                let semantic_brief = compose_semantic_brief(
                    &core_session.messages,
                    if skip_contract_evolve { "" } else { text },
                );
                let semantic = state
                    .semantic_verify_enabled
                    .load(Ordering::Acquire)
                    .then(|| {
                        config.api_key.as_ref().map(|api_key| {
                            crate::semantic_verify::SemanticContext {
                                brief: semantic_brief.clone(),
                                route: model_route(&config),
                                api_key: api_key.clone(),
                            }
                        })
                    })
                    .flatten();
                let failures: crate::verify::VerifyFailureTracker =
                    Arc::new(Mutex::new(HashMap::new()));
                run_config.registry.register(
                    crate::verify::definition_with_options(
                        Some(delivery.clone()),
                        semantic.clone(),
                        Some(contract.clone()),
                        Some(Arc::clone(&failures)),
                    ),
                );
                run_config.registry.register(
                    crate::verify::artifact_definition_with_options(
                        Some(delivery.clone()),
                        semantic,
                        Some(contract),
                        Some(failures),
                    ),
                );
                let staged_log: crate::publish_tool::StagedIntentLog =
                    Arc::new(Mutex::new(Vec::new()));
                staged_intents_for_turn = Some(Arc::clone(&staged_log));
                run_config.registry.register(
                    crate::publish_tool::definition_with_revision(
                        store.clone(),
                        Some(delivery),
                        expected_revision,
                        Some(staged_log),
                    ),
                );
                run_config.delivery_committer = Some(Arc::new(store));
                let empty_hint = if !core_session.messages.is_empty()
                    || workspace_has_user_files(&cwd, &submission_id)
                {
                    ""
                } else {
                    " The session workspace is currently empty (only the turn output directory exists). Do not search parent directories or run redundant glob/shell discovery looking for existing project files; build deliverables directly from scratch in the output directory above."
                };
                run_config.system = format!(
                    "{BASE_BEHAVIOR_PROMPT} Files to deliver to this user must be created in the current turn output directory: {}. Use write_file (or create_deck for .pptx presentations) with a path inside that directory. Before calling publish_file, verify every .pptx file with verify_deck and every .docx/.xlsx file with verify_artifact. If verification returns next_action=\"rerender_once\", fix and re-verify at most once; if verification still fails or returns next_action=\"clarify_or_rebuild\", stop and explain the issues instead of calling publish_file. publish_file only stages files; delivery occurs after the whole turn completes successfully. Do not treat ordinary text links or intermediate files as delivered artifacts. Maximum 8 files, 128 MiB each, 256 MiB per turn.{empty_hint}",
                    output_root.display()
                );
            } else if run_config.system.trim().is_empty() {
                BASE_BEHAVIOR_PROMPT.clone_into(&mut run_config.system);
            }
            run_config.system =
                append_skill_listing(&run_config.system, &skills);
            let providers = self.tool_providers();
            let mounted = if let Some(pre) = pre_staged.as_ref().filter(|pre| {
                pre.matches(
                    &id,
                    &cwd_key,
                    &core_session.origin,
                    core_session.access_mode,
                    &providers,
                )
            }) {
                merge_staged_providers(&run_config.registry, &pre.staged)
            } else {
                let provider_ctx = ToolProviderContext {
                    session_id: &id,
                    cwd: &cwd,
                    origin: &core_session.origin,
                    access_mode: core_session.access_mode,
                };
                mount_tool_providers(
                    &run_config.registry,
                    &providers,
                    &provider_ctx,
                )
            };
            if !mounted.hints.is_empty() {
                run_config.system.push_str("\n\n[ProviderCapabilities]");
                for hint in &mounted.hints {
                    run_config.system.push_str("\n- ");
                    run_config.system.push_str(hint);
                }
            }
            if preserve_tool_approval {
                let mut grants_guard = lock(&state.approved_tool_calls);
                if let Some(grants) = grants_guard.get_mut(&id) {
                    grants.retain(|grant| match &grant.provider_binding {
                        None => true,
                        Some(binding) => {
                            binding.cwd_key == cwd_key
                                && mounted
                                    .provenance
                                    .get(&grant.tool_name)
                                    .is_some_and(|prov| {
                                        prov.provider_id == binding.provider_id
                                            && prov.registration_id
                                                == binding.registration_id
                                            && prov.tool_fingerprint
                                                == binding.tool_fingerprint
                                    })
                        }
                    });
                    let pre_activated: BTreeSet<String> = grants
                        .iter()
                        .map(|grant| grant.tool_name.clone())
                        .collect();
                    run_config.pre_activated_tools =
                        pre_activated.into_iter().collect();
                    if grants.is_empty() {
                        grants_guard.remove(&id);
                    }
                }
            }
            run_config.pre_tool_hook =
                Some(build_pre_tool_hook(PreToolHookSpec {
                    session_id: id.clone(),
                    cwd: cwd.clone(),
                    origin: core_session.origin.clone(),
                    state: Arc::clone(state),
                    registry: run_config.registry.clone(),
                    providers,
                    mounted_provenance: mounted.provenance,
                    process_nonce: self.process_nonce.clone(),
                    force_reconfirm_tools,
                }));
        }
        if let Some((service, conversation)) =
            self.automation_tools_for(&core_session)
        {
            crate::automation_tools::register(
                &run_config.registry,
                &service,
                &conversation,
            );
        }
        let mut workspace_lease = if let Some(access_mode) =
            self.session_access_mode(&id)
        {
            self.settle_finishing_workspace_lease(&engine, &cwd, access_mode);
            Some(
                state
                    .workspace_locks
                    .try_acquire(&cwd, access_mode, &id)
                    .ok_or("notice.agent_busy")?,
            )
        } else {
            None
        };
        tracing::debug!(
            target: "flowy.prompt",
            stage = "AgentHandle::submit_message",
            session = %id,
            live = config.api_key.is_some(),
            model = %config.model,
            base_url = %config.base_url,
            "→ Engine::start_with_config"
        );
        let turn = engine
            .start_with_config(
                id.clone(),
                prompt.clone(),
                run_config,
                stream,
                submission,
            )
            .map_err(|error| {
                if let RunError::Storage(storage_error) = &error {
                    log_storage_io_error(
                        "start agent turn",
                        &flowy_agent_core::SessionIndex::path(&state.root),
                        storage_error,
                    );
                }
                match error {
                    RunError::Storage(_) => "notice.session_storage_failed",
                    RunError::Busy => "notice.agent_busy",
                    _ => "notice.agent_failed",
                }
            })?;
        let (fallback_title, title_signal) = {
            let mut live = lock(&state.live);
            let session = live.get_mut(&id).expect("active session exists");
            let now = now_ms();
            let prev_status =
                std::mem::replace(&mut session.status, TurnStatus::Running);
            session.active_turn_is_internal = is_background_announcement;
            session.active_turn_prev_status =
                is_background_announcement.then(|| {
                    if prev_status.is_busy() {
                        TurnStatus::Completed
                    } else {
                        prev_status
                    }
                });
            session.turn_started_at = Some(now);
            session.updated_at = now;
            if let Some(rid) = turn_dispatch_receipt_id {
                session.active_dispatch_receipt_id = Some(rid);
            } else if !is_internal_event_turn
                && prompt.display.as_deref() != Some("")
            {
                session.active_dispatch_receipt_id = None;
            }
            session.heartbeat = Some(BranchHeartbeat {
                last_completed: None,
                last_completed_at_ms: None,
                current_action: "thinking".to_owned(),
                next_check_hint: Some(
                    "awaiting first tool or model step".to_owned(),
                ),
                awaiting_evidence: true,
            });
            if !skip_push_user_line {
                session.projection.push_user(text);
            }
            let title = if skip_auto_title {
                (None, None)
            } else if session.titled {
                let signal = if session.auto_title_pending
                    && !session.auto_title_in_flight
                    && session.auto_title_attempts < MAX_AUTO_TITLE_ATTEMPTS
                    && config.api_key.is_some()
                {
                    session.title_signal.cancel();
                    let signal = CancelToken::new();
                    session.title_signal = signal.clone();
                    Some(signal)
                } else {
                    None
                };
                (None, signal)
            } else {
                session.title = title::clamp_title(
                    &title::fallback_title(text, TITLE_MAX_CHARS),
                    TITLE_MAX_CHARS,
                );
                session.titled = true;
                session.auto_title_pending = true;
                let signal = config.api_key.is_some().then(CancelToken::new);
                if let Some(signal) = &signal {
                    session.title_signal.cancel();
                    session.title_signal = signal.clone();
                }
                (Some(session.title.clone()), signal)
            };
            drop(live);
            title
        };
        if let Some(title) = fallback_title
            && engine.set_session_title(&id, title, true).is_err()
        {
            state.report_storage_error();
        }
        if persist_index(state).is_err() {
            state.report_storage_error();
        }
        let turn_done_signal = if let Some(claim) = pending_claim {
            let hb = claim.heartbeat_cancel.clone();
            let receipt_id = claim.receipt_id.clone();
            lock(&state.active_claims).insert(receipt_id.clone(), claim);
            state.push_update(AgentUpdate::Receipt {
                id: receipt_id,
                state: ReceiptState::Running,
            });
            Some(hb)
        } else {
            None
        };
        state.push_update(AgentUpdate::Session { id: id.clone() });
        state.push_update(AgentUpdate::Sessions);
        let state = Arc::clone(state);
        let weak_harness = Arc::downgrade(self);
        let receipt = id.clone();
        let completed_submission = submission;
        let session_role = core_session.role;
        let session_root_id = core_session.root_session_id;
        self.runtime.spawn(async move {
            let outcome = turn.await;
            if let Some(signal) = turn_done_signal {
                signal.cancel();
            }
            match &outcome {
                Ok(LoopOutcome::Completed) => {
                    tracing::debug!(
                        target: "flowy.prompt",
                        stage = "AgentHandle::submit_message",
                        session = %id,
                        ok = true,
                        "turn finished"
                    );
                }
                Ok(loop_outcome) => {
                    tracing::warn!(
                        target: "flowy.prompt",
                        stage = "AgentHandle::submit_message",
                        session = %id,
                        ok = false,
                        ?loop_outcome,
                        prompt_preview = %log_preview(&prompt.model, 200),
                        "turn finished"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        target: "flowy.prompt",
                        stage = "AgentHandle::submit_message",
                        session = %id,
                        ok = false,
                        error = %error,
                        prompt_preview = %log_preview(&prompt.model, 200),
                        "turn finished"
                    );
                }
            }
            if matches!(outcome, Ok(LoopOutcome::Completed)) {
                capture_turn_artifacts(
                    &engine,
                    &state,
                    &id,
                    completed_submission,
                );
            } else if matches!(
                outcome,
                Ok(LoopOutcome::Aborted | LoopOutcome::WaitingForUser)
            ) && let Some(staged_log) = staged_intents_for_turn
            {
                let pending = std::mem::take(
                    &mut *staged_log
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
                if !pending.is_empty() {
                    let expected_sub = completed_submission
                        .map(|s| format!("submission-{s:032x}"));
                    let live_delivery_failed =
                        lock(&state.live).get(&id).is_some_and(|session| {
                            matches!(session.status, TurnStatus::DeliveryFailed)
                        });
                    let has_durable_commit =
                        expected_sub.as_deref().is_some_and(|sub| {
                            engine.session(&id).is_some_and(|rec| {
                                rec.delivery_commits
                                    .iter()
                                    .any(|commit| commit.submission_id == sub)
                            })
                        });
                    if !live_delivery_failed && !has_durable_commit {
                        let gate = crate::delivery::DeliveryStore::new(
                            state.root.join("delivery"),
                        );
                        let reason = match &outcome {
                            Ok(LoopOutcome::WaitingForUser) => {
                                "turn_waiting_for_user"
                            }
                            _ => "turn_aborted",
                        };
                        let now = now_ms();
                        for (delivery_id, artifact_path, sha256) in pending {
                            let _ = gate.abort_intent(
                                &delivery_id,
                                &artifact_path,
                                &sha256,
                                reason,
                                now,
                            );
                        }
                    }
                }
            }
            // Release the directory lease strictly AFTER `turn.await` and
            // artifact capture / staged-intent cleanup, and strictly BEFORE
            // acquiring `accept_drain -> dispatch` (`finish_accepted_if_owned`),
            // `queue.complete`, `kick_autonomous_drain`, or `generate_title`.
            if let Some(lease) = workspace_lease.as_mut() {
                lease.release();
            }
            let expected_sub =
                completed_submission.map(|s| format!("submission-{s:032x}"));
            if matches!(outcome, Ok(LoopOutcome::WaitingForUser)) {
                // PendingCard::ToolApproval is durably persisted in
                // `history.json`. Complete the AcceptanceQueue receipt (matching
                // the Disambiguation handoff) so 30s lease expiry or a later
                // `DenyTool` never causes autonomous_drain to re-run this prompt.
                let matching_claims =
                    take_session_claims(&state.active_claims, &id);
                for claim in matching_claims {
                    claim.heartbeat_cancel.cancel();
                    let queue = AcceptanceQueue::new(state.root.clone());
                    if queue
                        .complete(
                            &claim.receipt_id,
                            &claim.attempt_id,
                            now_ms(),
                        )
                        .unwrap_or(false)
                    {
                        state.push_update(AgentUpdate::Receipt {
                            id: claim.receipt_id,
                            state: ReceiptState::Completed,
                        });
                    }
                }
                if state.autonomous_drain.load(Ordering::Acquire)
                    && let Some(harness) = weak_harness.upgrade()
                {
                    harness.kick_autonomous_drain();
                }
            } else if state.autonomous_drain.load(Ordering::Acquire)
                && let Some(harness) = weak_harness.upgrade()
            {
                if harness.has_owned_claim(&id, expected_sub.as_deref()) {
                    let live_delivery_failed =
                        lock(&state.live).get(&id).is_some_and(|session| {
                            matches!(session.status, TurnStatus::DeliveryFailed)
                        });
                    let has_durable_commit =
                        expected_sub.as_deref().is_some_and(|sub| {
                            engine.session(&id).is_some_and(|rec| {
                                rec.delivery_commits
                                    .iter()
                                    .any(|commit| commit.submission_id == sub)
                            })
                        });
                    let completed =
                        matches!(outcome, Ok(LoopOutcome::Completed))
                            || live_delivery_failed
                            || (matches!(outcome, Err(RunError::Storage(_)))
                                && has_durable_commit);
                    let h = Arc::clone(&harness);
                    let claim_sid = id.clone();
                    let claim_sub = expected_sub.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = h.finish_accepted_if_owned(
                            &claim_sid,
                            claim_sub.as_deref(),
                            completed,
                        );
                    })
                    .await;
                }
                harness.kick_autonomous_drain();
                drop(harness);
            }
            if session_role == SessionRole::Worker
                && let Some(root_id) = session_root_id
                && root_id != id
            {
                let kind = match &outcome {
                    Ok(LoopOutcome::Completed) => {
                        work_events::WorkEventKind::Completed
                    }
                    Ok(LoopOutcome::WaitingForUser) => {
                        work_events::WorkEventKind::WaitingForUser
                    }
                    Ok(LoopOutcome::Aborted) => {
                        work_events::WorkEventKind::Cancelled
                    }
                    Ok(LoopOutcome::Failed) | Err(_) => {
                        work_events::WorkEventKind::Failed
                    }
                };
                let contract_revision = engine
                    .with_session(&id, |rec| {
                        expected_sub
                            .as_deref()
                            .and_then(|sub| {
                                rec.submission_revisions.get(sub).copied()
                            })
                            .or_else(|| {
                                rec.work_contract.as_ref().map(|c| c.revision)
                            })
                    })
                    .flatten();
                work_events::push_work_event(
                    &state,
                    &root_id,
                    work_events::WorkEvent {
                        worker_id: id.clone(),
                        submission_id: expected_sub,
                        contract_revision,
                        kind,
                        observed_at_ms: now_ms(),
                    },
                );
                if let Some(harness) = weak_harness.upgrade() {
                    work_events::schedule_router_inbox_flush(
                        &harness, &root_id,
                    );
                }
            } else if session_role == SessionRole::Router {
                let harness = weak_harness.upgrade();
                if is_router_announcement {
                    // A failed announcement keeps its batch for a retry, so a
                    // finished Worker's result is never dropped silently.
                    let failed =
                        matches!(&outcome, Ok(LoopOutcome::Failed) | Err(_));
                    match harness.as_ref() {
                        Some(harness) => work_events::settle_announcement(
                            harness, &id, failed,
                        ),
                        None => work_events::clear_announcement_in_flight(
                            &state, &id,
                        ),
                    }
                }
                if let Some(harness) = harness {
                    work_events::schedule_router_inbox_flush(&harness, &id);
                }
            }
            if matches!(outcome, Ok(LoopOutcome::Completed))
                && let Some(signal) = title_signal
            {
                tracing::debug!(
                    target: "flowy.prompt",
                    stage = "AgentHandle::generate_title",
                    session = %id,
                    "sidecar LLM"
                );
                observer::generate_title(
                    engine,
                    state,
                    config,
                    signal,
                    id,
                    title_prompt,
                )
                .await;
            }
        });
        Ok(receipt)
    }

    fn park_disambiguation_locked(
        &self,
        engine: &Engine,
        id: &str,
        card: PendingCard,
        text: &str,
        skip_push_user_line: bool,
        pending_claim: Option<AcceptedClaim>,
    ) -> Result<String, &'static str> {
        let state = &self.state;
        let now = now_ms();
        let dispatch_receipt_id = match &card {
            PendingCard::Disambiguation {
                dispatch_receipt_id,
                ..
            } => dispatch_receipt_id.clone(),
            PendingCard::ToolApproval { .. } => None,
        };
        engine.park_pending_card(id, card, None).map_err(|error| {
            log_storage_io_error(
                "park disambiguation card",
                &flowy_agent_core::SessionIndex::path(&state.root),
                &error,
            );
            state.report_storage_error();
            "notice.session_storage_failed"
        })?;
        let fallback_title = {
            let mut live = lock(&state.live);
            let session = live.get_mut(id).expect("active session exists");
            session.status = TurnStatus::WaitingForUser;
            session.turn_started_at = None;
            session.updated_at = now;
            session.active_dispatch_receipt_id = dispatch_receipt_id;
            session.heartbeat = Some(BranchHeartbeat {
                last_completed: session
                    .heartbeat
                    .as_ref()
                    .and_then(|hb| hb.last_completed.clone()),
                last_completed_at_ms: session
                    .heartbeat
                    .as_ref()
                    .and_then(|hb| hb.last_completed_at_ms),
                current_action: "waiting_for_user".to_owned(),
                next_check_hint: Some(
                    "awaiting disambiguation card selection".to_owned(),
                ),
                awaiting_evidence: true,
            });
            if !skip_push_user_line {
                session.projection.push_user(text);
            }
            session.projection.lines.push(ChatLine::Status {
                text: TurnStatus::WaitingForUser.key().into(),
            });
            let title = if session.titled
                || session.role == SessionRole::Worker
                || observer::is_internal_control_line_text(text)
            {
                None
            } else {
                session.title = title::clamp_title(
                    &title::fallback_title(text, TITLE_MAX_CHARS),
                    TITLE_MAX_CHARS,
                );
                session.titled = true;
                session.auto_title_pending = true;
                Some(session.title.clone())
            };
            drop(live);
            title
        };
        if let Some(title) = fallback_title
            && engine.set_session_title(id, title, true).is_err()
        {
            state.report_storage_error();
        }
        if let Some(claim) = pending_claim {
            claim.heartbeat_cancel.cancel();
            let queue = AcceptanceQueue::new(state.root.clone());
            if queue
                .complete(&claim.receipt_id, &claim.attempt_id, now)
                .unwrap_or(false)
            {
                state.push_update(AgentUpdate::Receipt {
                    id: claim.receipt_id,
                    state: ReceiptState::Completed,
                });
            }
        }
        if persist_index(state).is_err() {
            state.report_storage_error();
        }
        state.push_update(AgentUpdate::Session { id: id.to_owned() });
        state.push_update(AgentUpdate::Sessions);
        Ok(id.to_owned())
    }

    pub(super) fn ensure_active_session_locked(
        &self,
    ) -> Result<String, &'static str> {
        let active = lock(&self.state.active).clone();
        if let Some(id) = active
            && lock(&self.state.live).contains_key(&id)
        {
            return Ok(id);
        }
        let workspace = lock(&self.state.workspace_for_new_sessions).clone();
        let origin = lock(&self.state.origin_for_new_sessions).clone();
        self.create_session_inner(origin, workspace, true)
    }
}
