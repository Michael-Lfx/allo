use super::*;

#[derive(Clone, Debug, Default)]
pub struct VoiceWindowContext {
    pub conversation_id: Option<String>,
    pub workspace: Option<String>,
    pub sequence: u64,
}

pub struct ConversationVoiceDialogueRecorder {
    pub conversation_service: Arc<nomifun_conversation::ConversationService>,
    pub companion_service: Option<Arc<CompanionService>>,
    pub target_config: Arc<RwLock<VoiceTargetConfig>>,
    pub active_window_candidate: Arc<RwLock<VoiceWindowContext>>,
    pub user_id: String,
    journal_path: PathBuf,
    journal_lock: Mutex<()>,
    projected_turns: RwLock<std::collections::HashSet<(String, String, String)>>,
    projected_messages: RwLock<std::collections::HashMap<(String, String, String), (Option<String>, String)>>,
    turn_errors: RwLock<std::collections::HashMap<(String, String, String), VoiceSessionError>>,
    execution_runtime:Option<Arc<dyn VoiceTaskRuntime>>,
    pending_call_reservation: RwLock<Option<crate::voice_dialogue::VoiceCallReservation>>,
    projection_fault_gate: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl ConversationVoiceDialogueRecorder {
    fn trigger_voice_autotitle(&self, conversation_id: &str, user_message_id: &str, raw_user_text: &str) {
        let trimmed = raw_user_text.trim();
        if trimmed.is_empty()
            || !trimmed
                .chars()
                .any(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fa5}').contains(&c))
        {
            return;
        }
        let title_svc = self.conversation_service.clone();
        let title_conv_id = conversation_id.to_owned();
        let title_user_msg_id = user_message_id.to_owned();
        let title_billing_turn_id = user_message_id.to_owned();
        let title_content = raw_user_text.to_owned();
        let user_id = self.user_id.clone();
        tokio::spawn(async move {
            let conv = title_svc.get(&user_id, &title_conv_id).await.ok();
            if let Some(ref c) = conv {
                let name = c.name.trim();
                if !name.is_empty() && name != "语音通话" && name != "Voice Call" && name != title_content.trim() {
                    return;
                }
            }
            let title_session_model = conv.and_then(|c| c.model);
            nomifun_ai_agent::with_flowy_chat_session_id(
                title_conv_id.clone(),
                nomifun_ai_agent::with_flowy_billing_turn_id(
                    title_billing_turn_id,
                    title_svc.maybe_autotitle(
                        &title_conv_id,
                        &title_user_msg_id,
                        title_content,
                        title_session_model,
                    ),
                ),
            )
            .await;
        });
    }

    pub fn new(
        conversation_service: Arc<nomifun_conversation::ConversationService>,
        user_id: String,
        data_dir: &Path,
    ) -> Self {
        Self {
            journal_path: data_dir.join("voice").join(format!("dialogue-{user_id}.json")),
            journal_lock: Mutex::new(()),
            projected_turns: RwLock::new(std::collections::HashSet::new()),
            projected_messages: RwLock::new(std::collections::HashMap::new()),
            turn_errors: RwLock::new(std::collections::HashMap::new()),
            execution_runtime:None,
            pending_call_reservation: RwLock::new(None),
            conversation_service,
            companion_service: None,
            target_config: Arc::new(RwLock::new(VoiceTargetConfig::default())),
            active_window_candidate: Arc::new(RwLock::new(VoiceWindowContext::default())),
            user_id,
            projection_fault_gate: None,
        }
    }

    pub(crate) fn with_execution_runtime(mut self,runtime:Arc<dyn VoiceTaskRuntime>)->Self {self.execution_runtime=Some(runtime);self}

    pub fn with_companion_service(mut self, companion_service: Arc<CompanionService>) -> Self {
        self.companion_service = Some(companion_service);
        self
    }

    pub fn with_target_config(mut self, target_config: Arc<RwLock<VoiceTargetConfig>>) -> Self {
        self.target_config = target_config;
        self
    }

    pub fn with_active_window_candidate(mut self, candidate: Arc<RwLock<VoiceWindowContext>>) -> Self {
        self.active_window_candidate = candidate;
        self
    }

    pub fn with_projection_fault_gate(mut self, gate: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.projection_fault_gate = Some(gate);
        self
    }

    fn workspace_sessions_path(&self) -> PathBuf {
        self.journal_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("workspace-sessions-{}.json", self.user_id))
    }

    fn read_workspace_sessions(&self) -> Result<WorkspaceVoiceSessionRegistry, VoiceDialogueRecorderError> {
        let path = self.workspace_sessions_path();
        match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes)
                    .map_err(|e| VoiceDialogueRecorderError::WriteFailed(format!("workspace sessions registry corrupt: {e}")))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(WorkspaceVoiceSessionRegistry::default())
            }
            Err(e) => {
                Err(VoiceDialogueRecorderError::WriteFailed(format!("failed to read workspace sessions registry: {e}")))
            }
        }
    }

    fn save_workspace_sessions(&self, registry: &WorkspaceVoiceSessionRegistry) -> Result<(), VoiceDialogueRecorderError> {
        let bytes = serde_json::to_vec(registry)
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        nomifun_common::dir_config::write_atomic_replace(&self.workspace_sessions_path(), &bytes)
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))
    }

    async fn update_workspace_interaction(&self, conv_id: &str, now: i64) -> Result<(), VoiceDialogueRecorderError> {
        let mut registry = self.read_workspace_sessions()?;
        let mut changed = false;
        let conversation = self.conversation_service.get(&self.user_id, conv_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        // Explicit calls publish their automatic reuse pointer only after real
        // acceptance. The empty key is reserved for the default workspace.
        if conversation.extra["voice_lifecycle"] == "retained"
            && let Some(request_id) = conversation.extra.pointer("/voice_creation_request/request_id").and_then(|v| v.as_str()) {
            let workspace = conversation.extra.get("voice_task_workspace").and_then(|v| v.as_str()).unwrap_or("");
            if registry.sessions.get(workspace).is_none_or(|entry| entry.last_user_interaction_at <= now) {
                registry.sessions.insert(workspace.into(), WorkspaceVoiceSessionEntry {
                    workspace: workspace.into(), conversation_id: conv_id.into(),
                    creation_key: format!("voice:provisional:{}:{request_id}", self.user_id),
                    last_user_interaction_at: now,
                });
                changed = true;
            }
        }
        for entry in registry.sessions.values_mut() {
            if entry.conversation_id == conv_id && entry.last_user_interaction_at < now {
                entry.last_user_interaction_at = now;
                changed = true;
            }
        }
        if changed {
            self.save_workspace_sessions(&registry)?;
        }
        Ok(())
    }

    async fn retained_default_call(&self) -> Result<Option<String>, VoiceDialogueRecorderError> {
        let registry = self.read_workspace_sessions()?;
        let Some(entry) = registry.sessions.get("") else { return Ok(None); };
        let owner = match &self.companion_service {Some(service) => service.default_companion_id().await, None => None};
        let row = self.conversation_service.conversation_repo().get(&entry.conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        Ok(row.filter(|row| row.user_id == self.user_id && row.r#type == "nomi")
            .and_then(|row| serde_json::from_str::<serde_json::Value>(&row.extra).ok())
            .filter(|extra| extra["voice_lifecycle"] == "retained"
                && extra.get("memory_owner_companion_id").and_then(|v| v.as_str()) == owner.as_deref())
            .map(|_| entry.conversation_id.clone()))
    }

    async fn resolve_target_conversation(
        &self,
        candidate: Option<String>,
        _config: VoiceTargetConfig,
        explicit_workspace: Option<&str>,
        session_mode: Option<&str>,
    ) -> Result<(String, Option<String>), VoiceDialogueRecorderError> {
        let now = nomifun_common::now_ms();
        let (candidate_workspace, source_id) = if let Some(ws) = explicit_workspace {
            (Some(ws.to_string()), None)
        } else if let Some(id) = candidate {
            let row = self.conversation_service.conversation_repo().get(&id).await
                .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?
                .filter(|row| row.user_id == self.user_id && row.r#type == "nomi");
            if let Some(row) = row {
                let extra: serde_json::Value = serde_json::from_str(&row.extra).unwrap_or_default();
                let is_temp_workspace = extra.get("temp_workspace_id").is_some();
                let candidate_ws = extra.get("voice_task_workspace").and_then(|v| v.as_str())
                    .or_else(|| {
                        if !is_temp_workspace {
                            extra.get("workspace").and_then(|v| v.as_str())
                        } else {
                            None
                        }
                    })
                    .map(|s| s.to_string());
                (candidate_ws, Some(id))
            } else {
                return Err(VoiceDialogueRecorderError::BindingInvalid("unsupported_source_conversation".into()));
            }
        } else {
            (None, None)
        };

        if let Some(ws) = candidate_workspace {
            let canonical = nomifun_common::paths::canonicalize_simplified(Path::new(&ws))
                .map_err(|e| VoiceDialogueRecorderError::WriteFailed(format!("workspace_directory_invalid: {e}")))?;
            if !canonical.is_dir() {
                return Err(VoiceDialogueRecorderError::WriteFailed(format!("workspace_directory_invalid: {} is not a directory", canonical.display())));
            }
            let canonical_str = canonical.to_string_lossy().to_string();
            let mut registry = self.read_workspace_sessions()?;
            let default_companion = if let Some(comp) = &self.companion_service {
                comp.default_companion_id().await
            } else {
                None
            };
            let title = format!("语音 - {}", canonical.file_name().and_then(|n| n.to_str()).unwrap_or("工作区"));
            let mode = self.conversation_service.resolve_voice_session_mode(session_mode).await
                .map_err(|error| VoiceDialogueRecorderError::BindingInvalid(error.to_string()))?;
            let source = if let Some(id) = source_id.as_deref() {
                Some(self.conversation_service.get(&self.user_id, id).await
                    .map_err(|error| VoiceDialogueRecorderError::WriteFailed(error.to_string()))?)
            } else { None };
            let make_req = || nomifun_api_types::CreateConversationRequest {
                name: Some(title.clone()),
                r#type: nomifun_common::AgentType::Nomi,
                source: Some(nomifun_common::ConversationSource::Nomifun),
                extra: serde_json::json!({
                    "voice": true,
                    "is_default_voice": true,
                    "memory_owner_companion_id": default_companion.clone(),
                    "last_user_interaction_at": now,
                    "session_mode": mode,
                }),
                model: source.as_ref().and_then(|conversation| conversation.model.clone()),
                preset_id: source.as_ref().and_then(|conversation| conversation.preset_id.clone()),
                preset_overrides: None,
                delegation_policy: nomifun_common::DelegationPolicy::Disabled,
                execution_model_pool: None,
                decision_policy: nomifun_common::DecisionPolicy::default(),
                execution_template_id: None,
                channel_chat_id: None,
            };

            let existing_entry = registry.sessions.get(&canonical_str).cloned();
            if let Some(mut entry) = existing_entry {
                // If entry has a non-empty conversation_id, check if it still exists in DB
                if !entry.conversation_id.is_empty() {
                    let existing_row = self.conversation_service.conversation_repo().get(&entry.conversation_id).await.ok().flatten();
                    if let Some(existing_row) = existing_row {
                        let existing_extra:serde_json::Value = serde_json::from_str(&existing_row.extra)
                            .map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
                        let owner_matches = existing_extra.get("memory_owner_companion_id").and_then(|v|v.as_str()) == default_companion.as_deref();
                        let is_voice_busy = self.conversation_service.is_conversation_busy(&self.user_id, &entry.conversation_id).await.unwrap_or(false);
                        let idle_ms = now.saturating_sub(entry.last_user_interaction_at);
                        if existing_extra["voice_lifecycle"] != "discarded" && ((owner_matches && idle_ms <= 30 * 60 * 1000) || is_voice_busy) {
                            entry.last_user_interaction_at = now;
                            registry.sessions.insert(canonical_str.clone(), entry.clone());
                            self.save_workspace_sessions(&registry)?;
                            return Ok((entry.conversation_id, source_id));
                        }
                    }
                } else if !entry.creation_key.is_empty() {
                    // Crash recovery case: entry has a creation_key but conversation_id was not backfilled!
                    if let Ok(Some(existing_conv_id)) = self.conversation_service.voice_conversation_by_creation_key(&self.user_id, &entry.creation_key).await {
                        entry.conversation_id = existing_conv_id.clone();
                        entry.last_user_interaction_at = now;
                        registry.sessions.insert(canonical_str.clone(), entry.clone());
                        self.save_workspace_sessions(&registry)?;
                        return Ok((existing_conv_id, source_id));
                    }
                    // Creation did not happen yet, replay with the EXACT same creation_key
                    let created = self.conversation_service.create_voice_idempotent(&self.user_id, make_req(), &entry.creation_key, &canonical_str, None).await
                        .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
                    entry.conversation_id = created.conversation_id.clone();
                    entry.last_user_interaction_at = now;
                    registry.sessions.insert(canonical_str.clone(), entry);
                    self.save_workspace_sessions(&registry)?;
                    return Ok((created.conversation_id, source_id));
                }

                // Window expired or conversation missing: create new rotation entry
                let new_creation_key = format!("voice-create:workspace:{canonical_str}:{now}");
                entry.creation_key = new_creation_key.clone();
                entry.last_user_interaction_at = now;
                entry.conversation_id.clear();
                registry.sessions.insert(canonical_str.clone(), entry.clone());
                // Hard gate: persist before side effect!
                self.save_workspace_sessions(&registry)?;

                let created = self.conversation_service.create_voice_idempotent(&self.user_id, make_req(), &new_creation_key, &canonical_str, None).await
                    .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
                entry.conversation_id = created.conversation_id.clone();
                registry.sessions.insert(canonical_str.clone(), entry);
                self.save_workspace_sessions(&registry)?;
                return Ok((created.conversation_id, source_id));
            } else {
                let creation_key = format!("voice-create:workspace:{canonical_str}:{now}");
                let mut entry = WorkspaceVoiceSessionEntry {
                    workspace: canonical_str.clone(),
                    conversation_id: String::new(),
                    creation_key: creation_key.clone(),
                    last_user_interaction_at: now,
                };
                registry.sessions.insert(canonical_str.clone(), entry.clone());
                // Hard gate: persist before side effect!
                self.save_workspace_sessions(&registry)?;

                let created = self.conversation_service.create_voice_idempotent(&self.user_id, make_req(), &creation_key, &canonical_str, None).await
                    .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
                entry.conversation_id = created.conversation_id.clone();
                registry.sessions.insert(canonical_str.clone(), entry);
                self.save_workspace_sessions(&registry)?;
                return Ok((created.conversation_id, source_id));
            }
        }

        if let Some(id) = self.retained_default_call().await? { return Ok((id, source_id)); }
        let memory_owner = match &self.companion_service {Some(companions)=>companions.default_companion_id().await,None=>None};
        let home = self.conversation_service.ensure_default_voice_conversation_with_mode(&self.user_id,memory_owner.as_deref(),session_mode).await.map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        Ok((home, source_id))
    }

    pub fn project_snapshot(
        &self,
        mut snapshot: nomifun_api_types::voice::VoiceSessionSnapshot,
        binding: &nomifun_api_types::voice::VoiceBindingSnapshot,
    ) -> nomifun_api_types::voice::VoiceSessionSnapshot {
        snapshot.binding = Some(binding.clone());
        snapshot.projected_turn_ids.clear();
        for turn in &mut snapshot.conversation {
            if self.is_projected(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                snapshot.projected_turn_ids.push(turn.turn_id.clone());
            }
            if let Some((user, assistant)) = self.projected_message_ids(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                turn.user_message_id = user;
                turn.assistant_message_id = Some(assistant);
            }
            if turn.error.is_none() {
                if let Some(error) = self.turn_error(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                    turn.error = Some(error);
                }
            }
        }
        if snapshot.error.is_none() {
            if let Some(curr_turn) = snapshot.turn_id.as_deref() {
                if let Some(error) = self.turn_error(&binding.session_id, &binding.conversation_id, curr_turn) {
                    snapshot.error = Some(error);
                }
            }
        }
        snapshot
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct WorkspaceVoiceSessionRegistry {
    #[serde(default)]
    sessions: std::collections::HashMap<String, WorkspaceVoiceSessionEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WorkspaceVoiceSessionEntry {
    workspace: String,
    conversation_id: String,
    creation_key: String,
    last_user_interaction_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
struct DialogueRecord {
    request: VoiceDialogueRequest,
    conversation_id: String,
    admission_epoch: i64,
    execution: Option<VoiceExecutionConfig>,
    #[serde(default)]
    frozen_preset: Option<nomifun_api_types::ResolvedPresetSnapshot>,
    #[serde(default)]
    source_workspace: Option<String>,
    output: Option<String>,
    projected: bool,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    requested_at: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    assistant_message_id: Option<String>,
    #[serde(default)]
    handoff_content: Option<String>,
    #[serde(default)]
    voice_context_digest: Option<String>,
    #[serde(default)]
    control_error: Option<VoiceSessionError>,
    #[serde(default)]
    preflight_active_operation_id: Option<String>,
    #[serde(default)]
    preflight_epoch: Option<i64>,
    #[serde(default)]
    reconciled_epoch: Option<i64>,
}

fn map_voice_projection_error(error: nomifun_common::AppError) -> VoiceDialogueRecorderError {
    match error {
        nomifun_common::AppError::Conflict(ref message) if message.contains("epoch") || message.contains("revision") => {
            VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into())
        }
        nomifun_common::AppError::Conflict(ref message) if message.contains("busy") => {
            VoiceDialogueRecorderError::BindingBusy(message.clone())
        }
        nomifun_common::AppError::Conflict(_) => VoiceDialogueRecorderError::Conflict,
        other => VoiceDialogueRecorderError::WriteFailed(other.to_string()),
    }
}

impl ConversationVoiceDialogueRecorder {
    fn read_records(&self) -> Result<Vec<DialogueRecord>, VoiceDialogueRecorderError> {
        match std::fs::read(&self.journal_path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string())),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(VoiceDialogueRecorderError::WriteFailed(e.to_string())),
        }
    }
    fn save_records(&self, records: &[DialogueRecord]) -> Result<(), VoiceDialogueRecorderError> {
        let bytes=serde_json::to_vec(records).map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        nomifun_common::dir_config::write_atomic_replace(&self.journal_path,&bytes)
            .map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))
    }
    async fn frozen_execution(&self, conv_id:&str) -> Result<Option<VoiceExecutionConfig>,VoiceDialogueRecorderError> {
        let conv=self.conversation_service.get(&self.user_id,conv_id).await.map_err(|e|match e {
            nomifun_common::AppError::NotFound(_)=>VoiceDialogueRecorderError::BindingInvalid("target_deleted".into()),
            _=>VoiceDialogueRecorderError::WriteFailed(e.to_string()),
        })?;
        let session_mode = conv
            .extra
            .get("session_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("yolo");
        let session_mode = Some(self.conversation_service.resolve_voice_session_mode(Some(session_mode)).await
            .map_err(|error| VoiceDialogueRecorderError::BindingInvalid(error.to_string()))?);
        // New bindings derive execution only from their target Conversation.
        if let Some(workspace) = conv.extra.get("workspace").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let path = Path::new(workspace);
            if path.is_absolute() && path.is_dir() {
                let model = conv.model.as_ref().and_then(|m| serde_json::to_value(m).ok()).and_then(|v| serde_json::from_value::<VoiceModelRef>(v).ok());
                return Ok(Some(VoiceExecutionConfig { workspace: workspace.into(), model, preset_id: conv.preset_id, session_mode }));
            }
        }
        Ok(None)
    }

    async fn reconcile_legitimate_epoch_advance(
        &self,
        records: &mut [DialogueRecord],
        session_id: &str,
        conversation_id: &str,
        frozen_workspace: Option<&str>,
        recorded_epoch: i64,
        current_epoch: i64,
        target_record_id: Option<&str>,
    ) -> Result<i64, VoiceDialogueRecorderError> {
        if current_epoch == recorded_epoch {
            return Ok(current_epoch);
        }
        if current_epoch < recorded_epoch {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }

        // 1. Target conversation and owner verification
        let conv = self.conversation_service.get(&self.user_id, conversation_id).await.map_err(|e| match e {
            nomifun_common::AppError::NotFound(_) => VoiceDialogueRecorderError::BindingInvalid("target_deleted".into()),
            _ => VoiceDialogueRecorderError::WriteFailed(e.to_string()),
        })?;
        if conv.conversation_id != conversation_id {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }
        if conv.extra.get("voice_lifecycle").and_then(|v| v.as_str()) == Some("discarded") {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_deleted".into()));
        }
        if conv.extra.get("_edit_resubmit_fence").is_some() {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }

        // 2. Workspace equivalence verification
        let conv_ws = conv.extra.get("workspace").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
        match (frozen_workspace, conv_ws) {
            (Some(expected), Some(actual)) => {
                let canonical_expected = nomifun_common::paths::canonicalize_simplified(Path::new(expected))
                    .map_err(|_| VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()))?;
                let canonical_actual = nomifun_common::paths::canonicalize_simplified(Path::new(actual))
                    .map_err(|_| VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()))?;
                if !nomifun_common::paths::paths_equivalent(&canonical_expected, &canonical_actual) {
                    return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
                }
            }
            (None, None) => {}
            _ => {
                return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
            }
        }

        // 3. Legitimate task handoff verification
        let current_state = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;

        let target_record = records.iter().find(|r| target_record_id.is_some() && r.request.voice_record_id.as_deref() == target_record_id)
            .or_else(|| records.iter().rfind(|r| r.request.session_id.as_deref() == Some(session_id) && r.request.voice_record_id.is_some()));

        let base_preflight_epoch = target_record
            .and_then(|r| r.preflight_epoch)
            .unwrap_or(recorded_epoch);
        let preflight_active_op = target_record.and_then(|r| r.preflight_active_operation_id.as_deref());
        let already_reconciled = target_record.and_then(|r| r.reconciled_epoch);

        // If this record has already been reconciled, it has already consumed its authorized advance.
        // It cannot absorb another advance without a new preflight!
        if already_reconciled.is_some() {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }

        let Some(expected_op) = preflight_active_op else {
            // No task was running at preflight time:
            // An epoch advance cannot be authorized!
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        };

        // Task was running at preflight time:
        // 1. Legitimate completion must advance epoch from base_preflight_epoch by EXACTLY 1
        if current_epoch != base_preflight_epoch + 1 {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }
        // 2. The task must no longer be active in DB
        if current_state.active_operation_id.as_deref() == Some(expected_op) {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }
        // 3. Receipt for expected_op must exist and be completed (status == "completed")
        let receipt = self.conversation_service.conversation_repo()
            .get_delivery_receipt(&self.user_id, conversation_id, expected_op).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        if !receipt.as_ref().is_some_and(|r| r.status == "completed") {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }

        // 4. Update records and journal atomically
        if let Some(target_id) = target_record_id {
            if let Some(r) = records.iter_mut().find(|r| r.request.voice_record_id.as_deref() == Some(target_id)) {
                r.admission_epoch = current_epoch;
                r.reconciled_epoch = Some(current_epoch);
            }
        } else {
            for r in records.iter_mut().filter(|r| r.request.session_id.as_deref() == Some(session_id) && !r.projected) {
                r.admission_epoch = current_epoch;
                r.reconciled_epoch = Some(current_epoch);
            }
        }
        if let Some(sentinel) = records.iter_mut().find(|r| r.request.session_id.as_deref() == Some(session_id) && r.request.voice_record_id.is_none()) {
            sentinel.admission_epoch = current_epoch;
        }
        self.save_records(records)?;

        Ok(current_epoch)
    }
}

#[async_trait::async_trait]
impl VoiceDialogueRecorder for ConversationVoiceDialogueRecorder {
    fn set_call_reservation(&self, reservation: Option<crate::voice_dialogue::VoiceCallReservation>) {
        let mut g = self.pending_call_reservation.write().unwrap_or_else(|e| e.into_inner());
        *g = reservation;
    }

    fn take_call_reservation(&self, request_id: &str) -> Option<crate::voice_dialogue::VoiceCallReservation> {
        let mut g = self.pending_call_reservation.write().unwrap_or_else(|e| e.into_inner());
        if g.as_ref().is_some_and(|reservation| reservation.request_id == request_id) { g.take() } else { None }
    }

    fn current_call_reservation(&self) -> Option<crate::voice_dialogue::VoiceCallReservation> {
        let g = self.pending_call_reservation.read().unwrap_or_else(|e| e.into_inner());
        g.clone()
    }

    fn is_projected(&self, session_id: &str, conversation_id: &str, turn_id: &str) -> bool {
        self.projected_turns.read().unwrap_or_else(|e|e.into_inner()).contains(&(session_id.into(), conversation_id.into(), turn_id.into()))
    }
    fn projected_message_ids(&self, session_id: &str, conversation_id: &str, turn_id: &str) -> Option<(Option<String>, String)> {
        self.projected_messages.read().unwrap_or_else(|e|e.into_inner()).get(&(session_id.into(), conversation_id.into(), turn_id.into())).cloned()
    }
    fn turn_error(&self, session_id: &str, conversation_id: &str, turn_id: &str) -> Option<VoiceSessionError> {
        self.turn_errors.read().unwrap_or_else(|e| e.into_inner())
            .get(&(session_id.into(), conversation_id.into(), turn_id.into())).cloned()
    }
    async fn keyboard_configuration(&self, conversation_id:&str) -> Result<crate::voice_dialogue::FrozenVoiceExecution,String> {
        let conv = self.conversation_service.get(&self.user_id, conversation_id).await.map_err(|_|"binding_invalid")?;
        if conv.extra.get("voice").and_then(|v|v.as_bool()) != Some(true) {return Err("binding_invalid".into());}
        let config = VoiceExecutionConfig {
            workspace: conv.extra.get("workspace").and_then(|v|v.as_str()).ok_or("task_workspace_invalid")?.into(),
            model: conv.model.as_ref().and_then(|m|serde_json::to_value(m).ok()).and_then(|m|serde_json::from_value(m).ok()),
            preset_id: conv.preset_id,
            session_mode: Some(conv.extra.get("session_mode").and_then(|v|v.as_str()).unwrap_or("yolo").to_owned()),
        };
        let runtime = self.execution_runtime.as_ref().ok_or("runtime_unavailable")?;
        let (config,preset) = runtime.prepare_configuration(&config).await?;
        Ok(crate::voice_dialogue::FrozenVoiceExecution {config,preset,conversation_id:Some(conversation_id.into()),
            message_id:None,handoff_content:None,display_text:None,voice_context_digest:None})
    }
    async fn fallback_execution(&self) -> Result<Option<nomifun_api_types::voice::VoiceExecutionConfig>, String> {
        let home = self.conversation_service.ensure_default_voice_conversation(&self.user_id).await
            .map_err(|e| e.to_string())?;
        self.frozen_execution(&home).await.map_err(|e| match e {
            VoiceDialogueRecorderError::BindingInvalid(msg) | VoiceDialogueRecorderError::WriteFailed(msg) => msg,
            _ => "execution_unconfigured".into(),
        })
    }
    async fn bound_execution(&self, session: &str) -> Result<Option<VoiceExecutionConfig>, String> {
        let record = {
            let _guard = self.journal_lock.lock().await;
            self.read_records().map_err(|_| "configuration_read_failed")?.into_iter()
                .find(|record| record.request.session_id.as_deref() == Some(session))
        };
        let Some(record) = record else { return Ok(None); };
        let admission = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, &record.conversation_id).await.map_err(|_| "binding_invalid")?;
        if admission.epoch != record.admission_epoch { return Err("binding_invalid".into()); }
        Ok(record.execution)
    }
    async fn freeze_session(&self,session:&str,call_request_id:Option<&str>)->Result<Option<nomifun_api_types::voice::VoiceBindingSnapshot>,VoiceDialogueRecorderError> {
        let _guard=self.journal_lock.lock().await;
        let mut records=self.read_records()?;
        if !records.iter().any(|r|r.request.session_id.as_deref()==Some(session)) {
            let reservation = self.current_call_reservation();
            let (id, source, reservation_opt) = if let Some(request_id) = call_request_id {
                let res = reservation.filter(|res| res.request_id == request_id)
                    .ok_or_else(||VoiceDialogueRecorderError::BindingInvalid("start_request_changed".into()))?;
                // A delayed receipt may fulfill its original target, never the newly selected project.
                (res.conversation_id.clone(), None, Some(res))
            } else if reservation.is_some() {
                return Err(VoiceDialogueRecorderError::BindingInvalid("start_request_missing".into()));
            } else {
                let candidate=self.active_window_candidate.read().unwrap_or_else(|e|e.into_inner()).clone();
                let config=self.target_config.read().unwrap_or_else(|e|e.into_inner()).clone();
                let (id, source) = self.resolve_target_conversation(candidate.conversation_id,config,candidate.workspace.as_deref(),None).await?;
                (id, source, None)
            };
            let _source = source;
            // Project settings are copied only at creation; reuse always freezes the actual voice target.
            let execution = self.frozen_execution(&id).await?;
            let source_workspace=execution.as_ref().map(|c|c.workspace.clone());
            let (execution,frozen_preset)=match (execution,&self.execution_runtime) {
                (Some(config),Some(runtime))=>match runtime.prepare_configuration(&config).await {
                    Ok((config,preset))=>(Some(config),preset),Err(_)=>(None,None),
                },
                (config,_) => (config,None),
            };
            let epoch=self.conversation_service.conversation_repo().get_turn_admission_state(&self.user_id,&id).await.map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))?.epoch;
            records.push(DialogueRecord {request:VoiceDialogueRequest {call_request_id:call_request_id.map(str::to_owned),session_id:Some(session.into()),binding_id:Some(format!("voice-binding-{session}")),..Default::default()},conversation_id:id.clone(),admission_epoch:epoch,execution,frozen_preset,source_workspace,output:None,projected:false,command_id:None,requested_at:None,message_id:None,assistant_message_id:None,handoff_content:None,voice_context_digest:None,control_error:None,preflight_active_operation_id:None,preflight_epoch:Some(epoch),reconciled_epoch:None});
            // Persist and consume under the same identity lock; a newer start cannot be taken here.
            let mut pending = self.pending_call_reservation.write().unwrap_or_else(|e|e.into_inner());
            if pending.as_ref().map(|res|res.request_id.as_str()) != reservation_opt.as_ref().map(|res|res.request_id.as_str()) {
                return Err(VoiceDialogueRecorderError::BindingInvalid("start_request_changed".into()));
            }
            self.save_records(&records)?;
            if let Some(res) = reservation_opt {
                pending.take();
                if let Some(mgr) = VoiceCompanionProcessManager::global() {
                    mgr.mark_request_bound(&res.request_id, session, &id, res.memory_owner.as_deref());
                }
            }
        }
        let binding=records.iter().find(|r|r.request.session_id.as_deref()==Some(session)).unwrap();
        if binding.request.call_request_id.as_deref() != call_request_id {
            return Err(VoiceDialogueRecorderError::BindingInvalid("start_request_changed".into()));
        }
        // Rehydrate the projection fence once at binding/preflight, never on audio events.
        for record in records.iter().filter(|record| record.projected && record.request.session_id.as_deref() == Some(session) && record.conversation_id == binding.conversation_id) {
            if let (Some(turn), Some(assistant)) = (&record.request.turn_id, &record.assistant_message_id) {
                self.projected_messages.write().unwrap_or_else(|e|e.into_inner()).insert((session.into(), binding.conversation_id.clone(), turn.clone()), (record.message_id.clone(), assistant.clone()));
            }
        }
        // A host restart must not show an already persisted turn as a live duplicate.
        self.projected_turns.write().unwrap_or_else(|e|e.into_inner()).extend(
            records.iter().filter(|record| record.projected && record.request.session_id.as_deref()==Some(session) && record.conversation_id == binding.conversation_id)
                .filter_map(|record| record.request.turn_id.clone().map(|turn| (session.into(), binding.conversation_id.clone(), turn)))
        );
        // A task-control failure is a durable, retryable turn state, not live
        // assistant text. Rehydrate it after a host restart.
        for record in records.iter().filter(|record| record.request.session_id.as_deref() == Some(session) && record.conversation_id == binding.conversation_id) {
            if let (Some(turn), Some(error)) = (&record.request.turn_id, &record.control_error) {
                self.turn_errors.write().unwrap_or_else(|e| e.into_inner())
                    .insert((session.into(), binding.conversation_id.clone(), turn.clone()), error.clone());
            }
        }
        let conversation = self.conversation_service.get(&self.user_id,&binding.conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        Ok(Some(nomifun_api_types::voice::VoiceBindingSnapshot {
            session_id:session.into(),binding_id:format!("voice-binding-{session}"),conversation_id:binding.conversation_id.clone(),workspace:binding.execution.as_ref().map(|e|e.workspace.clone()),
            memory_owner_companion_id:conversation.extra.get("memory_owner_companion_id").filter(|v|!v.is_null()).or_else(|| {
                (conversation.extra.get("companion_session").and_then(|v|v.as_bool())==Some(true))
                    .then(||conversation.extra.get("companion_id")).flatten()
            }).and_then(|v|v.as_str()).map(str::to_owned),
        }))
    }

    async fn resolve_call_target_with_locale(
        &self,
        target: &nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<&str>,
        request_id: Option<&str>,
        locale: Option<&str>,
    ) -> Result<(String, Option<String>), VoiceDialogueRecorderError> {
        // Share the journal lock with late projections publishing reuse pointers:
        // registry read/modify/write must not lose another workspace's update.
        let _guard = self.journal_lock.lock().await;
        match target {
            nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation { workspace, model, preset_id } => {
                let memory_owner = match &self.companion_service {
                    Some(companions) => companions.default_companion_id().await,
                    None => None,
                };
                let canonical_workspace = if let Some(ws) = workspace {
                    let canonical = nomifun_common::paths::canonicalize_simplified(Path::new(ws))
                        .map_err(|e| VoiceDialogueRecorderError::WriteFailed(format!("workspace_directory_invalid: {e}")))?;
                    if !canonical.is_dir() {
                        return Err(VoiceDialogueRecorderError::WriteFailed(format!(
                            "workspace_directory_invalid: {} is not a directory",
                            canonical.display()
                        )));
                    }
                    Some(canonical.to_string_lossy().to_string())
                } else {
                    None
                };
                let req_id = request_id.filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| VoiceDialogueRecorderError::BindingInvalid("voice_start_request_invalid".into()))?;
                let created = self
                    .conversation_service
                    .create_provisional_voice_conversation_with_locale(
                        &self.user_id,
                        memory_owner.as_deref(),
                        session_mode,
                        canonical_workspace.as_deref(),
                        req_id,
                        model.as_ref(),
                        preset_id.as_deref(),
                        locale,
                    )
                    .await
                    .map_err(|e| match e {
                        nomifun_common::AppError::Conflict(msg) => {
                            VoiceDialogueRecorderError::BindingInvalid(msg)
                        }
                        _ => VoiceDialogueRecorderError::WriteFailed(e.to_string()),
                    })?;
                let frozen_owner = created.extra.get("memory_owner_companion_id").and_then(|v| v.as_str()).map(str::to_owned);
                Ok((created.conversation_id, frozen_owner))
            }
            nomifun_api_types::voice::VoiceCallTarget::CurrentSelection => {
                let context = self.active_window_candidate.read().unwrap_or_else(|e| e.into_inner()).clone();
                let (id, _) = self.resolve_target_conversation(context.conversation_id, VoiceTargetConfig::default(), context.workspace.as_deref(), session_mode).await?;
                let conversation = self.conversation_service.get(&self.user_id, &id).await
                    .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
                let owner = conversation.extra.get("memory_owner_companion_id").and_then(|v| v.as_str()).map(str::to_owned);
                Ok((id, owner))
            }
            nomifun_api_types::voice::VoiceCallTarget::Workspace { workspace } => {
                let canonical = nomifun_common::paths::canonicalize_simplified(Path::new(workspace))
                    .map_err(|e| VoiceDialogueRecorderError::WriteFailed(format!("workspace_directory_invalid: {e}")))?;
                if !canonical.is_dir() {
                    return Err(VoiceDialogueRecorderError::WriteFailed(format!(
                        "workspace_directory_invalid: {} is not a directory",
                        canonical.display()
                    )));
                }
                let canonical_str = canonical.to_string_lossy().to_string();
                let (conv_id, _) = self
                    .resolve_target_conversation(None, VoiceTargetConfig::default(), Some(&canonical_str), session_mode)
                    .await?;
                let memory_owner = self
                    .conversation_service
                    .conversation_repo()
                    .get(&conv_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|row| {
                        let extra: serde_json::Value = serde_json::from_str(&row.extra).ok()?;
                        extra.get("memory_owner_companion_id").and_then(|v| v.as_str()).map(str::to_owned)
                    });
                Ok((conv_id, memory_owner))
            }
            nomifun_api_types::voice::VoiceCallTarget::VoiceConversation { conversation_id } => {
                let row = self
                    .conversation_service
                    .conversation_repo()
                    .get(conversation_id)
                    .await
                    .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?
                    .filter(|row| row.user_id == self.user_id && row.r#type == "nomi");
                if let Some(row) = row {
                    let extra: serde_json::Value = serde_json::from_str(&row.extra).unwrap_or_default();
                    if extra.get("voice_lifecycle").and_then(|v| v.as_str()) == Some("discarded") {
                        return Err(VoiceDialogueRecorderError::BindingInvalid("conversation_already_discarded".into()));
                    }
                    if extra.get("voice").and_then(|v| v.as_bool()) == Some(true) {
                        let memory_owner = extra.get("memory_owner_companion_id").and_then(|v| v.as_str()).map(str::to_owned);
                        return Ok((conversation_id.clone(), memory_owner));
                    }
                }
                Err(VoiceDialogueRecorderError::BindingInvalid("conversation_invalid".into()))
            }
            nomifun_api_types::voice::VoiceCallTarget::DefaultWorkspace => {
                let memory_owner = match &self.companion_service {
                    Some(companions) => companions.default_companion_id().await,
                    None => None,
                };
                if let Some(id) = self.retained_default_call().await? { return Ok((id, memory_owner)); }
                let home = self
                    .conversation_service
                    .ensure_default_voice_conversation_with_mode(&self.user_id, memory_owner.as_deref(), session_mode)
                    .await
                    .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
                Ok((home, memory_owner))
            }
        }
    }

    fn set_call_delete_guard(&self, guard: Arc<dyn Fn(&str) -> bool + Send + Sync>) {
        self.conversation_service.set_voice_call_delete_guard(guard);
    }

    async fn finish_call_placeholder(&self, conversation_id: &str, request_id: &str) -> Result<bool, VoiceDialogueRecorderError> {
        let _guard = self.journal_lock.lock().await;
        let records = self.read_records()?;
        if !records.iter().any(|r| r.conversation_id == conversation_id && r.request.call_request_id.as_deref() == Some(request_id)) {
            return Ok(false);
        }
        let conversation = self.conversation_service.get(&self.user_id, conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        if conversation.extra.pointer("/voice_creation_request/request_id").and_then(|v| v.as_str()) != Some(request_id) { return Ok(false); }
        if conversation.extra["voice_lifecycle"] == "retained" {
            self.update_workspace_interaction(conversation_id, conversation.modified_at).await?;
        }
        self.conversation_service.finish_provisional_voice_conversation(&self.user_id, conversation_id, false).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))
    }

    async fn cleanup_ended_placeholders(&self, selected: Option<&str>, protected: &[String]) -> Result<(), VoiceDialogueRecorderError> {
        let _guard = self.journal_lock.lock().await;
        let records = self.read_records()?;
        let ids = self.conversation_service.ended_voice_placeholders(&self.user_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        for id in ids {
            if selected == Some(id.as_str()) || protected.contains(&id)
                || records.iter().any(|r| r.conversation_id == id && r.request.turn_id.is_some() && !r.projected) { continue; }
            self.conversation_service.discard_provisional_voice_conversation(&self.user_id, &id).await
                .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        }
        Ok(())
    }

    async fn preflight(&self,request:&VoiceDialogueRequest)->Result<VoiceDialoguePreflight,VoiceDialogueRecorderError> {
        let session=request.session_id.as_deref().filter(|s|!s.is_empty()).ok_or_else(||VoiceDialogueRecorderError::BindingInvalid("scope_required".into()))?;
        let turn=request.turn_id.as_deref().filter(|s|!s.is_empty()).ok_or_else(||VoiceDialogueRecorderError::BindingInvalid("scope_required".into()))?;
        if request.voice_record_id.as_deref()!=Some(format!("voice-{turn}").as_str())
            || request.binding_id.as_deref()!=Some(format!("voice-binding-{session}").as_str())
            || request.raw_user_text.as_deref().is_none_or(|s|s.trim().is_empty()) {
            return Err(VoiceDialogueRecorderError::BindingInvalid("scope_invalid".into()));
        }
        self.freeze_session(session, request.call_request_id.as_deref()).await?;
        let _guard=self.journal_lock.lock().await;
        let mut records=self.read_records()?;
        if let Some(record)=records.iter().find(|r|r.request.voice_record_id==request.voice_record_id) {
            if record.request!=*request {return Err(VoiceDialogueRecorderError::Conflict);}
            if record.projected {
                if let Some(turn) = &request.turn_id { self.projected_turns.write().unwrap_or_else(|e|e.into_inner()).insert((session.into(), record.conversation_id.clone(), turn.clone())); }
            }
            return match &record.output {
                Some(content) if record.projected=>Ok(VoiceDialoguePreflight::Cached{content:content.clone()}),
                Some(content)=>Ok(VoiceDialoguePreflight::Generated{conversation_id:record.conversation_id.clone(),content:content.clone()}),
                None=>Err(VoiceDialogueRecorderError::GenerationUnknown),
            };
        }
        let frozen=records.iter().find(|r|r.request.session_id==request.session_id).cloned()
            .ok_or_else(||VoiceDialogueRecorderError::BindingInvalid("binding_missing".into()))?;
        let (conversation_id,execution,frozen_preset,source_workspace)=(frozen.conversation_id,frozen.execution,frozen.frozen_preset,frozen.source_workspace);
        let admission_state = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, &conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        let epoch = admission_state.epoch;
        let preflight_active_op = admission_state.active_operation_id;
        let has_unsettled = records.iter().any(|r| {
            r.request.session_id == request.session_id
                && r.request.voice_record_id.is_some()
                && !r.projected
        });
        if has_unsettled {
            let sentinel_epoch = records.iter()
                .find(|r| r.request.session_id == request.session_id)
                .map(|r| r.admission_epoch);
            if let Some(recorded_epoch) = sentinel_epoch {
                if recorded_epoch != epoch {
                    self.reconcile_legitimate_epoch_advance(
                        &mut records,
                        session,
                        &conversation_id,
                        source_workspace.as_deref(),
                        recorded_epoch,
                        epoch,
                        None,
                    ).await?;
                }
            }
        } else if let Some(sentinel) = records.iter_mut().find(|r| r.request.session_id == request.session_id && r.request.voice_record_id.is_none()) {
            sentinel.admission_epoch = epoch;
        }
        let requested_at = Some(chrono::Local::now().to_rfc3339());
        let user_message_id = nomifun_common::generate_id();
        records.push(DialogueRecord {
            request: request.clone(),
            conversation_id: conversation_id.clone(),
            admission_epoch: epoch,
            execution,
            frozen_preset,
            source_workspace,
            output: None,
            projected: false,
            command_id: None,
            requested_at,
            message_id: Some(user_message_id),
            assistant_message_id: None,
            handoff_content: None,
            voice_context_digest: None,
            control_error: None,
            preflight_active_operation_id: preflight_active_op,
            preflight_epoch: Some(epoch),
            reconciled_epoch: None,
        });
        // The original ASR, frozen target/config and generation claim precede the model side effect.
        self.save_records(&records)?;
        Ok(VoiceDialoguePreflight::Ready{conversation_id})
    }

    async fn billing_attribution(
        &self,
        conversation_id: &str,
        request: &VoiceDialogueRequest,
    ) -> Result<nomifun_ai_agent::FlowyProxyAttribution, VoiceDialogueRecorderError> {
        let _guard = self.journal_lock.lock().await;
        let records = self.read_records()?;
        let record = records.iter()
            .find(|record| request.voice_record_id.is_some() && record.request.voice_record_id == request.voice_record_id)
            .filter(|record| record.request == *request && record.conversation_id == conversation_id)
            .ok_or(VoiceDialogueRecorderError::Conflict)?;
        let turn_id = record.message_id.as_deref()
            .and_then(|id| nomifun_common::MessageId::parse(id).ok())
            .ok_or_else(|| VoiceDialogueRecorderError::BindingInvalid("billing_turn_missing".into()))?;
        Ok(nomifun_ai_agent::FlowyProxyAttribution {
            session_id: Some(conversation_id.into()),
            turn_id: Some(turn_id.into_string()),
        })
    }

    async fn record_pair(&self,conversation_id:&str,request:&VoiceDialogueRequest,assistant_content:&str)->Result<(),VoiceDialogueRecorderError> {
        let _guard=self.journal_lock.lock().await;
        let mut records=self.read_records()?;
        let admission_epoch;
        let user_message_id;
        let source_workspace;
        let session_id;
        {
            let record=records.iter_mut().find(|r|r.request.voice_record_id==request.voice_record_id)
                .ok_or(VoiceDialogueRecorderError::Conflict)?;
            if record.request!=*request || record.conversation_id!=conversation_id || record.output.as_deref().is_some_and(|text|text!=assistant_content) {
                return Err(VoiceDialogueRecorderError::Conflict);
            }
            record.output=Some(assistant_content.into());
            let mid = record.message_id.clone().unwrap_or_else(nomifun_common::generate_id);
            record.message_id = Some(mid.clone());
            user_message_id = mid;
            admission_epoch = record.admission_epoch;
            source_workspace = record.source_workspace.clone();
            session_id = record.request.session_id.clone().unwrap_or_default();
        }
        let mut admission_epoch = admission_epoch;
        self.save_records(&records)?; // Never regenerate an answer when transcript projection fails.

        let current_state = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        let current_epoch = current_state.epoch;
        if current_epoch != admission_epoch {
            admission_epoch = self.reconcile_legitimate_epoch_advance(
                &mut records,
                &session_id,
                conversation_id,
                source_workspace.as_deref(),
                admission_epoch,
                current_epoch,
                request.voice_record_id.as_deref(),
            ).await?;
        }

        let decision=nomifun_api_types::voice::parse_voice_decision(assistant_content).map_err(|_|VoiceDialogueRecorderError::DecisionInvalid)?;
        if decision.intent_type == nomifun_api_types::voice::VoiceIntentType::Ignore {
            records.iter_mut().find(|r| r.request.voice_record_id == request.voice_record_id).unwrap().projected = true;
            return self.save_records(&records);
        }
        if decision.intent_type == nomifun_api_types::voice::VoiceIntentType::TaskControl {
            let current_state = self.conversation_service.conversation_repo()
                .get_turn_admission_state(&self.user_id, conversation_id).await
                .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
            let current_epoch = current_state.epoch;
            if current_epoch != admission_epoch {
                admission_epoch = self.reconcile_legitimate_epoch_advance(
                    &mut records,
                    &session_id,
                    conversation_id,
                    source_workspace.as_deref(),
                    admission_epoch,
                    current_epoch,
                    request.voice_record_id.as_deref(),
                ).await?;
            }
            let raw_user_text = request.raw_user_text.as_deref().unwrap_or("").trim();
            if !raw_user_text.is_empty() {
                let op_id = request.voice_record_id.as_deref().unwrap_or("voice-turn");
                self.conversation_service.append_voice_message_with_epoch(
                    &self.user_id, conversation_id, &user_message_id, raw_user_text, op_id, Some(admission_epoch),
                ).await.map_err(|error| match error {
                    nomifun_common::AppError::Conflict(ref msg) if msg.contains("epoch") || msg.contains("revision") => {
                        VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into())
                    }
                    nomifun_common::AppError::Conflict(msg) => VoiceDialogueRecorderError::BindingBusy(msg),
                    other => VoiceDialogueRecorderError::WriteFailed(other.to_string()),
                })?;
            }
            // The real status/cancel result is written by the control boundary.
            // Never substitute global current-task state or a fixed placeholder.
            self.save_records(&records)?;
            return Ok(());
        }
        let display = decision.reply_tts.as_deref()
            .or(decision.spoken_ack.as_deref())
            .or(decision.understood_request.as_deref())
            .unwrap_or_default().to_owned();

        let current_state = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        let current_epoch = current_state.epoch;
        if current_epoch != admission_epoch {
            admission_epoch = self.reconcile_legitimate_epoch_advance(
                &mut records,
                &session_id,
                conversation_id,
                source_workspace.as_deref(),
                admission_epoch,
                current_epoch,
                request.voice_record_id.as_deref(),
            ).await?;
        }

        let raw_user_text = request.raw_user_text.as_deref().unwrap_or("").trim();
        let op_id = request.voice_record_id.as_deref().unwrap_or("voice-turn");

        let is_busy = self.conversation_service.is_conversation_busy(&self.user_id, conversation_id).await.unwrap_or(false);
        let has_display = !display.trim().is_empty();
        if let Some(gate) = &self.projection_fault_gate {
            if gate.swap(false, std::sync::atomic::Ordering::SeqCst) {
                if let (Some(session), Some(turn)) = (&request.session_id, &request.turn_id) {
                    let key = (session.clone(), conversation_id.to_owned(), turn.clone());
                    self.turn_errors.write().unwrap_or_else(|e| e.into_inner()).insert(
                        key,
                        VoiceSessionError {
                            stage: "history".into(),
                            code: "voice_projection_write_failed".into(),
                        },
                    );
                }
                return Err(VoiceDialogueRecorderError::WriteFailed("simulated_storage_failure".into()));
            }
        }
        if !is_busy && has_display {
            let res = self.conversation_service.project_voice_dialogue_pair_with_user_msg_id(
                &self.user_id,
                conversation_id,
                op_id,
                if raw_user_text.is_empty() { None } else { Some(raw_user_text) },
                &display,
                "voice_companion",
                admission_epoch,
                Some(&user_message_id),
            ).await;
            match res {
                Ok(projection) => {
                    self.update_workspace_interaction(conversation_id, nomifun_common::now_ms()).await?;
                    let record = records.iter_mut().find(|r| r.request.voice_record_id == request.voice_record_id).unwrap();
                    record.message_id = projection.user_message.as_ref().map(|row| row.message_id.clone());
                    record.assistant_message_id = Some(projection.assistant_message.message_id.clone());
                    records.iter_mut().find(|r| r.request.voice_record_id == request.voice_record_id).unwrap().projected = true;
                    self.save_records(&records)?;
                    if let (Some(session), Some(turn)) = (&request.session_id, &request.turn_id) {
                        let key = (session.clone(), conversation_id.to_owned(), turn.clone());
                        self.projected_messages.write().unwrap_or_else(|e|e.into_inner()).insert(key.clone(), (projection.user_message.map(|row|row.message_id), projection.assistant_message.message_id));
                        self.projected_turns.write().unwrap_or_else(|e|e.into_inner()).insert(key.clone());
                        self.turn_errors.write().unwrap_or_else(|e|e.into_inner()).remove(&key);
                    }
                    self.trigger_voice_autotitle(conversation_id, &user_message_id, raw_user_text);
                    return Ok(());
                }
                Err(nomifun_common::AppError::Conflict(ref msg)) if msg.contains("epoch") || msg.contains("revision") => {
                    return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
                }
                Err(nomifun_common::AppError::Conflict(_)) => {
                    // Conversation became busy between check and acquire, fallback to busy append below
                }
                Err(e) => return Err(VoiceDialogueRecorderError::WriteFailed(e.to_string())),
            }
        }

        // Busy append fallback (or omitted optional display fields):
        let current_state = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
        let current_epoch = current_state.epoch;
        if current_epoch != admission_epoch {
            admission_epoch = self.reconcile_legitimate_epoch_advance(
                &mut records,
                &session_id,
                conversation_id,
                source_workspace.as_deref(),
                admission_epoch,
                current_epoch,
                request.voice_record_id.as_deref(),
            ).await?;
        }

        if !raw_user_text.is_empty() {
            let command_id = request.voice_record_id.as_deref().unwrap_or("voice-turn");
            self.conversation_service.append_voice_message_with_epoch(
                &self.user_id,
                conversation_id,
                &user_message_id,
                raw_user_text,
                command_id,
                Some(admission_epoch),
            ).await.map_err(|e| match e {
                nomifun_common::AppError::Conflict(ref msg) if msg.contains("epoch") || msg.contains("revision") => {
                    VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into())
                }
                nomifun_common::AppError::Conflict(msg) => VoiceDialogueRecorderError::BindingBusy(msg),
                _ => VoiceDialogueRecorderError::WriteFailed(e.to_string()),
            })?;
        }

        let assistant_message_id = if has_display {
            let mid = self.conversation_service.project_assistant_message_idempotent_with_epoch(
                &self.user_id,
                conversation_id,
                op_id,
                &display,
                "voice_companion",
                Some(admission_epoch),
                Some(&user_message_id),
            ).await.map_err(|e| match e {
                nomifun_common::AppError::Conflict(ref msg) if msg.contains("epoch") || msg.contains("revision") => {
                    VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into())
                }
                nomifun_common::AppError::Conflict(msg) => VoiceDialogueRecorderError::BindingBusy(msg),
                _ => VoiceDialogueRecorderError::WriteFailed(e.to_string()),
            })?;
            Some(mid)
        } else {
            None
        };

        self.update_workspace_interaction(conversation_id, nomifun_common::now_ms()).await?;
        records.iter_mut().find(|r|r.request.voice_record_id==request.voice_record_id).unwrap().assistant_message_id = assistant_message_id.clone();
        records.iter_mut().find(|r| r.request.voice_record_id == request.voice_record_id).unwrap().projected = true;
        self.save_records(&records)?;
        if let (Some(session), Some(turn)) = (&request.session_id, &request.turn_id) {
            let key = (session.clone(), conversation_id.to_owned(), turn.clone());
            self.projected_messages.write().unwrap_or_else(|e|e.into_inner()).insert(key.clone(), (Some(user_message_id.clone()), assistant_message_id.unwrap_or_default()));
            self.projected_turns.write().unwrap_or_else(|e|e.into_inner()).insert(key);
        }
        self.trigger_voice_autotitle(conversation_id, &user_message_id, raw_user_text);
        Ok(())
    }

    async fn task_scope_conversation(
        &self,
        scope: &nomifun_api_types::voice::VoiceSessionControlRequest,
    ) -> Result<Option<String>, VoiceDialogueRecorderError> {
        let session = scope.session_id.as_deref().filter(|value| !value.is_empty())
            .ok_or_else(|| VoiceDialogueRecorderError::BindingInvalid("scope_required".into()))?;
        let turn = scope.turn_id.as_deref().filter(|value| !value.is_empty());
        let _guard = self.journal_lock.lock().await;
        let records = self.read_records()?;
        if let Some(turn) = turn
            && let Some(record) = records.iter().find(|record| {
                record.request.session_id.as_deref() == Some(session)
                    && record.request.turn_id.as_deref() == Some(turn)
            }) {
            return Ok(Some(record.conversation_id.clone()));
        }
        Ok(records.iter()
            .find(|record| record.request.session_id.as_deref() == Some(session) && record.request.turn_id.is_none())
            .map(|record| record.conversation_id.clone()))
    }

    async fn record_task_control_result(
        &self,
        scope: &nomifun_api_types::voice::VoiceSessionControlRequest,
        task_reference: &nomifun_api_types::voice::VoiceTaskReference,
        raw_user_text: Option<&str>,
        locale: Option<&str>,
        action: nomifun_api_types::voice::VoiceTaskControlAction,
        result: &Result<nomifun_api_types::voice::VoiceTaskSnapshot, String>,
    ) -> Result<(), VoiceDialogueRecorderError> {
        let session = scope.session_id.as_deref().filter(|value| !value.is_empty())
            .ok_or_else(|| VoiceDialogueRecorderError::BindingInvalid("scope_required".into()))?;
        let turn = scope.turn_id.as_deref().filter(|value| !value.is_empty())
            .ok_or_else(|| VoiceDialogueRecorderError::BindingInvalid("scope_required".into()))?;
        let supplied_raw = raw_user_text.map(str::trim).filter(|value| !value.is_empty());
        let _guard = self.journal_lock.lock().await;
        let mut records = self.read_records()?;

        let record_index = if let Some(index) = records.iter().position(|record| {
            record.request.session_id.as_deref() == Some(session)
                && record.request.turn_id.as_deref() == Some(turn)
        }) {
            index
        } else {
            let sentinel = records.iter().find(|record| {
                record.request.session_id.as_deref() == Some(session) && record.request.turn_id.is_none()
            }).ok_or_else(|| VoiceDialogueRecorderError::BindingInvalid("binding_missing".into()))?;
            let conversation_id = sentinel.conversation_id.clone();
            let admission_epoch = self.conversation_service.conversation_repo()
                .get_turn_admission_state(&self.user_id, &conversation_id).await
                .map_err(|error| VoiceDialogueRecorderError::WriteFailed(error.to_string()))?.epoch;
            records.push(DialogueRecord {
                request: VoiceDialogueRequest {
                    call_request_id: sentinel.request.call_request_id.clone(),
                    locale: locale.map(str::to_owned),
                    voice_record_id: Some(format!("voice-{turn}")),
                    binding_id: Some(format!("voice-binding-{session}")),
                    session_id: Some(session.into()),
                    turn_id: Some(turn.into()),
                    raw_user_text: supplied_raw.map(str::to_owned),
                    ..Default::default()
                },
                conversation_id,
                admission_epoch,
                execution: sentinel.execution.clone(),
                frozen_preset: sentinel.frozen_preset.clone(),
                source_workspace: sentinel.source_workspace.clone(),
                output: None,
                projected: false,
                command_id: None,
                requested_at: Some(chrono::Local::now().to_rfc3339()),
                message_id: Some(nomifun_common::generate_id()),
                assistant_message_id: None,
                handoff_content: None,
                voice_context_digest: None,
                control_error: None,
                preflight_active_operation_id: None,
                preflight_epoch: Some(admission_epoch),
                reconciled_epoch: None,
            });
            records.len() - 1
        };

        let conversation_id = records[record_index].conversation_id.clone();
        let admission_epoch = records[record_index].admission_epoch;
        if let Some(existing_raw) = records[record_index].request.raw_user_text.as_deref().map(str::trim).filter(|value| !value.is_empty())
            && let Some(supplied) = supplied_raw
            && existing_raw != supplied {
            return Err(VoiceDialogueRecorderError::Conflict);
        }
        let raw_user_text = supplied_raw
            .map(str::to_owned)
            .or_else(|| records[record_index].request.raw_user_text.clone())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| VoiceDialogueRecorderError::BindingInvalid("raw_user_text_missing".into()))?;
        let current_epoch = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, &conversation_id).await
            .map_err(|error| VoiceDialogueRecorderError::WriteFailed(error.to_string()))?.epoch;
        if current_epoch != admission_epoch {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }

        // A replay of an already persisted result must not rewrite it with a
        // newer status snapshot. The first authoritative result owns the turn.
        if records[record_index].projected && records[record_index].assistant_message_id.is_some() {
            records[record_index].control_error = None;
            self.turn_errors.write().unwrap_or_else(|error| error.into_inner())
                .remove(&(session.to_owned(), conversation_id.clone(), turn.to_owned()));
            return Ok(());
        }
        let user_message_id = records[record_index].message_id.clone()
            .unwrap_or_else(nomifun_common::generate_id);
        records[record_index].message_id = Some(user_message_id.clone());
        let operation_id = records[record_index].request.voice_record_id.clone()
            .unwrap_or_else(|| format!("voice-{turn}"));
        self.conversation_service.append_voice_message_with_epoch(
            &self.user_id, &conversation_id, &user_message_id, &raw_user_text, &operation_id, Some(admission_epoch),
        ).await.map_err(map_voice_projection_error)?;

        match result {
            Ok(task) => {
                if !task_reference.matches(task) {
                    return Err(VoiceDialogueRecorderError::BindingInvalid("task_receipt_scope_changed".into()));
                }
                if task.conversation_id.as_deref() != Some(conversation_id.as_str()) {
                    return Err(VoiceDialogueRecorderError::BindingInvalid("task_scope_changed".into()));
                }
                let is_en = nomifun_api_types::voice::is_english_locale(
                    records[record_index].request.locale.as_deref().or(locale),
                );
                let is_cancelled = action == nomifun_api_types::voice::VoiceTaskControlAction::CancelTask
                    && task.error_code.as_deref() == Some("task_cancelled");
                let display = format_task_status_text(&task, is_en, is_cancelled);
                let assistant_message_id = self.conversation_service
                    .project_assistant_message_idempotent_with_epoch(
                        &self.user_id, &conversation_id, &operation_id, &display,
                        "voice_companion", Some(admission_epoch), Some(&user_message_id),
                    ).await.map_err(map_voice_projection_error)?;
                records[record_index].output = Some(serde_json::json!({
                    "intent_type": "task_control",
                    "task_control_action": action,
                    "understood_request": raw_user_text,
                    "reply_tts": display,
                }).to_string());
                records[record_index].assistant_message_id = Some(assistant_message_id.clone());
                records[record_index].projected = true;
                records[record_index].control_error = None;
                self.save_records(&records)?;
                let key = (session.to_owned(), conversation_id.clone(), turn.to_owned());
                self.projected_messages.write().unwrap_or_else(|error| error.into_inner())
                    .insert(key.clone(), (Some(user_message_id), assistant_message_id));
                self.projected_turns.write().unwrap_or_else(|error| error.into_inner()).insert(key.clone());
                self.turn_errors.write().unwrap_or_else(|error| error.into_inner()).remove(&key);
                if let Err(error) = self.update_workspace_interaction(&conversation_id, nomifun_common::now_ms()).await {
                    // The turn is already durable; a workspace-index refresh
                    // must not turn a successful result into a failed control.
                    tracing::warn!(error=%error, "voice_task_control_workspace_interaction_deferred");
                }
                Ok(())
            }
            Err(code) => {
                // Keep the user question, the original identity and an honest,
                // retryable error for this turn. Never write fake assistant text.
                records[record_index].projected = false;
                records[record_index].control_error = Some(VoiceSessionError {
                    stage: "task".into(),
                    code: code.clone(),
                });
                self.save_records(&records)?;
                self.turn_errors.write().unwrap_or_else(|error| error.into_inner()).insert(
                    (session.to_owned(), conversation_id.clone(), turn.to_owned()),
                    VoiceSessionError { stage: "task".into(), code: code.clone() },
                );
                Ok(())
            }
        }
    }

    async fn task_configuration(&self,command:&nomifun_api_types::voice::VoiceTaskCommand)->Result<Option<crate::voice_dialogue::FrozenVoiceExecution>,String> {
        let _guard=self.journal_lock.lock().await;
        VoiceTaskService::validate_command(command)?;
        let mut records=self.read_records().map_err(|_|"voice_projection_write_failed")?;
        let expected_record_id = command.record_id.clone().unwrap_or_else(|| format!("voice-{}", command.turn_id));
        let expected_binding_id = command.binding_id.clone().unwrap_or_else(|| format!("voice-binding-{}", command.session_id));
        let record = records.iter().find(|r| {
            r.request.voice_record_id.as_ref() == Some(&expected_record_id)
            && r.request.binding_id.as_ref() == Some(&expected_binding_id)
            && r.request.session_id.as_deref() == Some(&command.session_id)
            && r.request.turn_id.as_deref() == Some(&command.turn_id)
        }).cloned().ok_or("binding_invalid")?;
        if record.command_id.as_ref().is_some_and(|id|id!=&command.command_id) {return Err("task_command_conflict".into());}
        let decision=nomifun_api_types::voice::parse_voice_decision(record.output.as_deref().ok_or("voice_generation_unknown")?)?;
        if decision.intent_type!=nomifun_api_types::voice::VoiceIntentType::AgentTask || decision.task_goal.as_deref()!=Some(command.goal.as_str()) || !record.projected {
            return Err("task_command_conflict".into());
        }
        let epoch=self.conversation_service.conversation_repo().get_turn_admission_state(&self.user_id,&record.conversation_id).await.map_err(|_|"binding_invalid")?;
        if epoch.epoch!=record.admission_epoch {return Err("binding_invalid".into());}
        let config=record.execution.as_ref().ok_or("binding_invalid")?;
        if let Some(source)=&record.source_workspace {
            let canonical=nomifun_common::paths::canonicalize_simplified(Path::new(source)).map_err(|_|"binding_invalid")?;
            if !nomifun_common::paths::paths_equivalent(&canonical, Path::new(&config.workspace)) {return Err("binding_invalid".into());}
        }
        let raw_user_text = record.request.raw_user_text.as_deref().unwrap_or("");
        if let (Some(handoff), Some(digest)) = (&record.handoff_content, &record.voice_context_digest) {
            return Ok(Some(crate::voice_dialogue::FrozenVoiceExecution {
                config: config.clone(),
                preset: record.frozen_preset.clone(),
                handoff_content: Some(handoff.clone()),
                display_text: Some(raw_user_text.to_owned()).filter(|text| !text.trim().is_empty()),
                conversation_id: Some(record.conversation_id.clone()),
                message_id: record.message_id.clone(),
                voice_context_digest: Some(digest.clone()),
            }));
        }

        let target_idx = records.iter().position(|r| r.request.voice_record_id.as_ref() == Some(&expected_record_id)).unwrap();

        // Collect prior task results from completed tasks
        let tasks_journal_path = self.journal_path.parent().map(|p| p.join(format!("tasks-{}.json", self.user_id)));
        let mut prior_task_results: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(tpath) = tasks_journal_path {
            if let Ok(bytes) = std::fs::read(&tpath) {
                if let Ok(ledger) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if let Some(recs) = ledger.get("records").and_then(|r| r.as_array()) {
                        for rec in recs {
                            let cmd_id = rec.get("command").and_then(|c| c.get("command_id")).and_then(|s| s.as_str());
                            let status = rec.get("task").and_then(|t| t.get("status")).and_then(|s| s.as_str());
                            let res_ok = rec.get("task").and_then(|t| t.get("result_ok")).and_then(|b| b.as_bool()).unwrap_or(false);
                            let res_text = rec.get("task").and_then(|t| t.get("result_text")).and_then(|s| s.as_str());
                            if let (Some(cid), Some(rt)) = (cmd_id, res_text) {
                                if !rt.trim().is_empty() && status == Some("completed") && res_ok {
                                    prior_task_results.insert(cid.to_string(), rt.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Bounded strictly before target_idx in the same conversation (F06, C08)
        let prior_records: Vec<_> = records[..target_idx].iter().filter(|r| {
            r.conversation_id == record.conversation_id
                && r.projected
                && r.output.as_deref().and_then(|output| nomifun_api_types::voice::parse_voice_decision(output).ok())
                    .is_some_and(|decision| decision.intent_type != nomifun_api_types::voice::VoiceIntentType::Ignore)
                && (record.source_workspace.is_none() || r.source_workspace == record.source_workspace)
        }).collect();

        let mut total_chars = 0;
        let mut older_omitted = prior_records.len() > 3;
        let start_idx = prior_records.len().saturating_sub(3);

        // Iterate backwards: prioritize the most recent corrections and constraints (C08)
        let recent_candidates = &prior_records[start_idx..];
        let mut turns_to_include = Vec::new();

        for r in recent_candidates.iter().rev() {
            let mut turn_summary = Vec::new();
            let mut details = Vec::new();
            if let Some(turn_id) = r.request.turn_id.as_deref().filter(|s| !s.trim().is_empty()) {
                turn_summary.push(format!("[轮次 {turn_id}]"));
            }
            if let Some(text) = r.request.raw_user_text.as_deref().filter(|s| !s.trim().is_empty()) {
                turn_summary.push(format!("用户曾说“{}”", text));
            }
            if let Some(out) = &r.output {
                if let Ok(dec) = nomifun_api_types::voice::parse_voice_decision(out) {
                    if let Some(und) = dec.understood_request.filter(|s| !s.trim().is_empty()) {
                        details.push(format!("需求“{}”", und));
                    }
                    if let Some(reply) = dec.reply_tts.filter(|s| !s.trim().is_empty()) {
                        details.push(format!("曾回答“{}”", reply));
                    }
                }
            }
            if let Some(cid) = r.command_id.as_deref() {
                if let Some(res) = prior_task_results.get(cid) {
                    details.push(format!("前序结果“{}”", res));
                }
            }
            if !turn_summary.is_empty() {
                let summary_str = turn_summary.join("，");
                let count = summary_str.chars().count();
                if total_chars + count <= 1000 {
                    total_chars += count;
                    turns_to_include.push((turn_summary, details));
                } else {
                    if turns_to_include.is_empty() {
                        return Err("voice_context_too_large".into());
                    }
                    older_omitted = true;
                    break;
                }
            }
        }

        // Reserve the budget for user constraints before considering assistant prose.
        for (summary, details) in &mut turns_to_include {
            for detail in details {
                let count = detail.chars().count() + 1;
                if total_chars + count <= 1000 {
                    total_chars += count;
                    summary.push(detail.clone());
                }
            }
        }
        turns_to_include.reverse();
        let mut bounded_context: Vec<String> = turns_to_include.into_iter().map(|(summary, _)| summary.join("，")).collect();
        if older_omitted {
            bounded_context.insert(0, "（更早轮次已按预算略过）".to_string());
        }

        let understanding = decision.understood_request.as_deref().unwrap_or("");
        let goal = &command.goal;
        let request_time = record.requested_at.as_deref().unwrap_or("");
        let context_section = if !bounded_context.is_empty() {
            format!("前轮上下文：{}\n", bounded_context.join("；"))
        } else {
            String::new()
        };
        let handoff_content = if !raw_user_text.trim().is_empty() {
            Some(format!(
                "来源：语音委派\n用户原文：{raw_user_text}\n理解需求：{understanding}\n{context_section}任务目标：{goal}\n请求时点：{request_time}\n执行要求：使用当前会话可用工具如实完成任务，不要编造未经验证的数据；遵守确认流程；完成或需要补充时先给出简短结论。"
            ))
        } else {
            None
        };
        let voice_context_digest = handoff_content.as_ref().map(|content| {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(content.as_bytes()))
        });

        if let Some(target) = records.iter_mut().find(|r| r.request.voice_record_id.as_ref() == Some(&expected_record_id)) {
            target.command_id = Some(command.command_id.clone());
            target.handoff_content = handoff_content.clone();
            target.voice_context_digest = voice_context_digest.clone();
            self.save_records(&records).map_err(|_| "voice_projection_write_failed")?;
        }

        Ok(Some(crate::voice_dialogue::FrozenVoiceExecution {
            config: config.clone(),
            preset: record.frozen_preset.clone(),
            handoff_content,
            display_text: Some(raw_user_text.to_owned()).filter(|text| !text.trim().is_empty()),
            conversation_id: Some(record.conversation_id.clone()),
            message_id: record.message_id.clone(),
            voice_context_digest,
        }))
    }
}
