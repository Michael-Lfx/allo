//! Voice Companion Child-Process Supervisor (`ai-mate`)
//!
//! Authoritative baseline: docs/architecture/voice-agent/interaction.zh.md
//!
//! Supervises the independent `ai-mate` daemon process alongside the Flowy
//! desktop application:
//! - Hidden console window, completely headless with zero extraneous tray icons;
//! - Strict whole-tree lifecycle authority via `nomi_process_runtime::ChildProcessBuilder`;
//! - Windows Job Object / Unix process-group anchoring so children die immediately
//!   when Flowy desktop shuts down or terminates unexpectedly;
//! - Auto-start on application boot when models are ready and user has enabled the companion.

use crate::voice_dialogue::{
    VoiceDialogueBridge, VoiceDialogueCompleter, VoiceDialoguePreflight, VoiceDialogueRecorder,
    VoiceDialogueRecorderError,
};
use crate::voice_tasks::{VoiceTaskRuntime, VoiceTaskService};
use nomifun_api_types::voice::{VoiceExecutionConfig, VoiceExecutionSettings};
use nomifun_api_types::voice::{VoiceTaskDelivery, VoiceTaskSnapshot, VoiceTaskStatus, VoiceTaskSubmitRequest};
use nomifun_api_types::voice::{VoiceTaskControlAction,VoiceTaskControlCommand,VoiceTaskControlRequest};
#[cfg(test)]
use nomifun_api_types::voice::VoiceTaskNotification;
use nomifun_api_types::voice::{VoiceToolConfirmation,VoiceToolConfirmationRequest};
use nomi_process_runtime::{ChildProcessBuilder, ManagedChildProcess};
use nomifun_api_types::voice::{VoiceCompanionEvent, VoiceConnectionState, VoiceSessionError};
use nomifun_api_types::voice::{VoiceDialogueConfig, VoiceDialogueRequest, VoiceDialogueSettings, VoiceModelRef};
use nomifun_api_types::voice::{
    VoiceFeedbackConfig, VoiceFeedbackPatch, VoiceFeedbackPreviewRequest, VoiceFeedbackSettings,
    VoiceTargetConfig, VoiceTargetMode, VoiceTargetSettings, VoiceTargetUpdateRequest,
};
pub use nomifun_api_types::voice::{
    VoiceCallStartRequest, VoiceCallStartResponse, VoiceCallStartStatus, VoiceCallTarget,
    VoiceSessionControlRequest, VoiceSessionPhase, VoiceSessionSnapshot,
};
use nomifun_companion::CompanionService;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::Mutex;
use tokio::sync::{Notify, broadcast};
use tracing::{info, warn};

/// Stable error code returned when the packaged voice daemon is absent.
pub const VOICE_COMPANION_MISSING: &str = "voice_companion_missing";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub models_ready: bool,
    pub autostart: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub acoustic_ready: bool,
    #[serde(default)]
    pub dialogue_ready: bool,
    #[serde(default)]
    pub playback_ready: bool,
    #[serde(default)]
    pub task_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<nomifun_api_types::voice::VoiceCapabilityReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionToggleRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCompanionToggleResponse {
    pub success: bool,
    pub running: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VoiceCompanionPersistedConfig {
    pub autostart: bool,
}

#[derive(Default)]
struct AudioRecovery {
    attempts: u8,
    next_attempt_ms: Option<i64>,
}

/// Bounded automatic daemon restart budget: 2s / 5s / 15s, at most three
/// attempts. A stable run of 60 seconds clears the budget.
#[derive(Default)]
struct RestartBudget {
    attempts: u8,
    next_attempt: Option<tokio::time::Instant>,
    running_since: Option<tokio::time::Instant>,
    exhausted_reported: bool,
}

impl RestartBudget {
    const MAX_ATTEMPTS: u8 = 3;

    fn delay(attempt: u8) -> std::time::Duration {
        match attempt {
            0 => std::time::Duration::from_secs(2),
            1 => std::time::Duration::from_secs(5),
            _ => std::time::Duration::from_secs(15),
        }
    }
}

enum RestartDecision {
    Wait(std::time::Duration),
    StartNow,
    Exhausted,
}

impl AudioRecovery {
    fn due(&mut self, snapshot:&VoiceSessionSnapshot, now_ms:i64)->bool {
        if !snapshot.enabled || snapshot.connection!=VoiceConnectionState::Connected
            || !matches!(snapshot.phase,VoiceSessionPhase::Idle|VoiceSessionPhase::Error)
            || snapshot.ended_reason==Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || snapshot.audio.as_ref().is_some_and(|audio|audio.speech_detected)
        {return false;}
        let capture_failed=snapshot.audio.as_ref().is_some_and(|audio|audio.capture_state=="closed"
            || (audio.capture_state=="no_pcm" && audio.last_frame_at_ms.is_none_or(|last|now_ms.saturating_sub(last)>=5_000)));
        let speaker_failed=snapshot.acoustic_ready && !snapshot.playback_ready;
        if !capture_failed && !speaker_failed {
            if snapshot.acoustic_ready && snapshot.playback_ready {*self=Self::default();}
            return false;
        }
        let due=*self.next_attempt_ms.get_or_insert(now_ms.saturating_add(2_000));
        if now_ms<due {return false;}
        self.attempts=self.attempts.saturating_add(1);
        self.next_attempt_ms=Some(now_ms.saturating_add((2_000_i64<<self.attempts.min(4)).min(30_000)));
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskFeedbackDecision {
    None,
    TriggerFeedback {
        command_id: String,
        stage: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct TaskFeedbackTracker {
    pub active_command_id: Option<String>,
    pub accepted_at_ms: i64,
    pub last_feedback_at_ms: i64,
    pub last_stage: Option<String>,
    pub consecutive_same_count: usize,
    pub pending_feedback: Option<String>,
}

impl TaskFeedbackTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate(&mut self,task:&VoiceTaskSnapshot,now_ms:i64)->TaskFeedbackDecision {
        self.evaluate_progress(task,None,now_ms)
    }
    pub fn evaluate_progress(&mut self, task: &VoiceTaskSnapshot, progress:Option<&nomifun_api_types::voice::VoiceTaskProgress>, now_ms: i64) -> TaskFeedbackDecision {
        if task.status != VoiceTaskStatus::Accepted {
            self.active_command_id = None;
            self.pending_feedback = None;
            self.consecutive_same_count = 0;
            return TaskFeedbackDecision::None;
        }

        if self.active_command_id.as_deref() != Some(&task.command_id) {
            self.active_command_id = Some(task.command_id.clone());
            self.accepted_at_ms = now_ms;
            self.last_feedback_at_ms = 0;
            self.last_stage = progress.map(|p|format!("{}:{}",p.stage,p.health));
            self.consecutive_same_count = 0;
            self.pending_feedback = None;
        }

        let current_stage = progress.map(|p|format!("{}:{}",p.stage,p.health));
        if current_stage != self.last_stage {
            self.last_stage = current_stage;
            self.consecutive_same_count = 0;
        }

        if self.last_feedback_at_ms > 0 && self.consecutive_same_count >= 2 {
            return TaskFeedbackDecision::None;
        }

        let due = if self.last_feedback_at_ms == 0 {
            now_ms >= self.accepted_at_ms + 15_000
        } else {
            now_ms >= self.last_feedback_at_ms + 45_000
        };

        if due {
            let stage_name = self.last_stage.clone().unwrap_or_else(|| "processing".to_string());
            self.pending_feedback = Some(stage_name.clone());
            TaskFeedbackDecision::TriggerFeedback {
                command_id: task.command_id.clone(),
                stage: stage_name,
            }
        } else {
            TaskFeedbackDecision::None
        }
    }

    pub fn record_dispatched(&mut self, now_ms: i64) {
        self.last_feedback_at_ms = now_ms;
        self.consecutive_same_count += 1;
        self.pending_feedback = None;
    }

    pub fn clear_pending(&mut self) {
        self.pending_feedback = None;
    }
}

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
                        self.projected_turns.write().unwrap_or_else(|e|e.into_inner()).insert(key);
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

fn wake_rejection_precedes_session(status: axum::http::StatusCode, body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else { return false; };
    if value.get("success").and_then(|value| value.as_bool()) != Some(false) { return false; }
    let reason = value.get("error").and_then(|value| value.as_str());
    matches!(
        (status, reason),
        (axum::http::StatusCode::FORBIDDEN, Some("pairing_required"))
            | (axum::http::StatusCode::SERVICE_UNAVAILABLE, Some("models_unavailable" | "microphone_unavailable"))
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HandledStartOutcome {
    Starting {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        conversation_id: String,
        memory_owner: Option<String>,
        reserved_at_epoch_ms: i64,
    },
    Bound {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        conversation_id: String,
        session_id: Option<String>,
        memory_owner: Option<String>,
        bound_at_epoch_ms: i64,
    },
    Stopped {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        conversation_id: String,
        session_id: Option<String>,
        stopped_at_epoch_ms: i64,
    },
    Rejected {
        target: nomifun_api_types::voice::VoiceCallTarget,
        session_mode: Option<String>,
        status_code_u16: u16,
        error_json: String,
        at_epoch_ms: i64,
    },
}

impl HandledStartOutcome {
    fn session_mode(&self) -> &Option<String> {
        match self {
            Self::Starting { session_mode, .. } | Self::Bound { session_mode, .. }
            | Self::Stopped { session_mode, .. } | Self::Rejected { session_mode, .. } => session_mode,
        }
    }
    pub fn target(&self) -> &nomifun_api_types::voice::VoiceCallTarget {
        match self {
            Self::Starting { target, .. }
            | Self::Bound { target, .. }
            | Self::Stopped { target, .. }
            | Self::Rejected { target, .. } => target,
        }
    }
}

#[derive(Clone)]
struct StoredCapabilities {
    report: nomifun_api_types::voice::VoiceCapabilityReport,
    attempt: u64,
    expires_at_ms: i64,
    signature: String,
}

fn readiness_runtime_scope_valid(
    snapshot: &nomifun_api_types::voice::VoiceSessionSnapshot,
    now_ms: i64,
) -> bool {
    let audio = snapshot.audio.as_ref();
    let recent_pcm = audio.is_some_and(|value| {
        value.frames_received > 0
            && value
                .last_frame_at_ms
                .is_some_and(|at| now_ms.saturating_sub(at) <= 2_000)
            && value.capture_state != "closed"
    });
    snapshot.enabled
        && snapshot.connection == VoiceConnectionState::Connected
        && snapshot.acoustic_ready
        && snapshot.playback_ready
        && recent_pcm
        && audio.is_some_and(|value| value.asr_ready && value.vad_ready)
}
fn tts_self_check_languages_valid(
    response: &nomifun_api_types::voice::VoiceTtsSelfCheckResponse,
) -> bool {
    ["zh-CN", "en-US"].iter().all(|language| {
        response.languages.iter().any(|entry| {
            let expected_ms = (entry.samples as u64).saturating_mul(1_000)
                / u64::from(entry.sample_rate.max(1));
            entry.language == *language
                && entry.ok
                && (8_000..=48_000).contains(&entry.sample_rate)
                && entry.samples > 0
                && (100..=30_000).contains(&entry.duration_ms)
                && entry.duration_ms.abs_diff(expected_ms) <= 100
                && (entry.rms_milli >= 5 || entry.peak_milli >= 20)
        })
    })
}
/// Dropping an unfinished readiness check releases the host admission flag and
/// cancels the daemon-side fixed-sample playback/work for this check id.
struct SelfCheckFlight {
    manager: std::sync::Weak<VoiceCompanionProcessManager>,
    check_id: String,
    finished: bool,
}

impl Drop for SelfCheckFlight {
    fn drop(&mut self) {
        let Some(manager) = self.manager.upgrade() else {
            return;
        };
        manager.self_check_active.store(false, Ordering::Release);
        if self.finished {
            return;
        }
        let check_id = self.check_id.clone();
        let origin = manager.daemon_origin.clone();
        let token = manager.control_token.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let client = match reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(2))
                .build()
            {
                Ok(client) => client,
                Err(_) => return,
            };
            if let Err(error) = client
                .post(format!(
                    "{origin}/api/voice/diagnostics/tts-self-check/cancel"
                ))
                .header("x-ai-mate-control-token", token)
                .json(&serde_json::json!({ "check_id": check_id }))
                .send()
                .await
            {
                tracing::warn!(%error, "voice_self_check_cancel_failed");
            }
        });
    }
}
/// Holds the host-side model-write gate. Dropping it re-opens call admission
/// and restarts the daemon only when the user's enable intent is still valid.
pub struct ModelWriteGuard {
    manager: std::sync::Weak<VoiceCompanionProcessManager>,
    resume_daemon: bool,
    released: bool,
}

impl std::fmt::Debug for ModelWriteGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelWriteGuard")
            .field("resume_daemon", &self.resume_daemon)
            .field("released", &self.released)
            .finish()
    }
}

impl Drop for ModelWriteGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let Some(manager) = self.manager.upgrade() else {
            return;
        };
        manager.resource_write_active.store(false, Ordering::Release);
        if !self.resume_daemon {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    if let Err(error) = manager.resume_after_model_write().await {
                        warn!(%error, "voice_daemon_restart_after_model_write_failed");
                    }
                });
            }
            Err(error) => warn!(%error, "voice_model_write_guard_dropped_without_runtime"),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub struct ChildExitObservation {
    pub pid: u32,
    pub code: Option<i32>,
    pub observed_at_ms: i64,
}

pub struct VoiceCompanionProcessManager {
    audio_recovery:std::sync::Mutex<AudioRecovery>,
    resource_write_active:AtomicBool,
    self_check_lock:tokio::sync::Mutex<()>,
    self_check_active:AtomicBool,
    self_check_cancel_signal:tokio::sync::Notify,
    self_check_attempt:AtomicU64,
    capabilities:std::sync::RwLock<Option<StoredCapabilities>>,
    restart_budget:std::sync::Mutex<RestartBudget>,
    prompted_approvals:std::sync::Mutex<std::collections::VecDeque<String>>,
    revoked_sessions: RwLock<std::collections::VecDeque<String>>,
    task_feedback: std::sync::Mutex<TaskFeedbackTracker>,
    daemon_origin: String,
    #[cfg(test)]
    test_daemon_port: Option<u16>,
    #[cfg(test)]
    last_child_exit: Arc<std::sync::Mutex<Option<ChildExitObservation>>>,
    #[cfg(test)]
    test_child_pid: Arc<std::sync::atomic::AtomicU32>,
    tasks: Option<Arc<VoiceTaskService>>,
    data_dir: PathBuf,
    task_projection_started: AtomicBool,
    dialogue_completer: Option<Arc<dyn VoiceDialogueCompleter>>,
    dialogue_recorder: Option<Arc<dyn VoiceDialogueRecorder>>,
    target_selection: Arc<RwLock<VoiceTargetConfig>>,
    active_window_candidate: Arc<RwLock<VoiceWindowContext>>,
    companion_service: Option<Arc<CompanionService>>,
    dialogue_bridge: Mutex<Option<VoiceDialogueBridge>>,
    config_write_lock: std::sync::Mutex<()>,
    lifecycle_lock: Mutex<()>,
    feedback_update_lock: Mutex<()>,
    child: Arc<Mutex<Option<ManagedChildProcess>>>,
    models_manager: Arc<nomifun_voice_models::ModelManager>,
    config_path: PathBuf,
    control_token: String,
    snapshot: RwLock<VoiceSessionSnapshot>,
    source: RwLock<Option<(String, u64)>>,
    task_sync_seq: AtomicU64,
    events: broadcast::Sender<VoiceSessionSnapshot>,
    projection_started: AtomicBool,
    generation: AtomicU64,
    process_changed: Notify,
    shutdown_requested: AtomicBool,
    call_start_lock: tokio::sync::Mutex<()>,
    handled_start_requests: std::sync::RwLock<std::collections::HashMap<String, HandledStartOutcome>>,
    viewed_conversation: RwLock<(u64, Option<String>)>,
}

static GLOBAL_COMPANION_MANAGER: std::sync::OnceLock<Arc<VoiceCompanionProcessManager>> =
    std::sync::OnceLock::new();

impl VoiceCompanionProcessManager {
    pub fn register_global(manager: Arc<Self>) {
        manager.install_call_delete_guard();
        let _ = GLOBAL_COMPANION_MANAGER.set(manager);
    }

    fn install_call_delete_guard(self: &Arc<Self>) {
        if let Some(recorder) = &self.dialogue_recorder {
            let weak = Arc::downgrade(self);
            recorder.set_call_delete_guard(Arc::new(move |id| weak.upgrade().is_some_and(|manager| manager.conversation_in_call(id))));
        }
    }

    fn conversation_in_call(&self, id: &str) -> bool {
        let snapshot = self.snapshot();
        (snapshot.session_id.is_some() && snapshot.ended_reason.is_none()
            && snapshot.binding.as_ref().is_some_and(|b| b.conversation_id == id))
            || self.dialogue_recorder.as_ref().and_then(|r| r.current_call_reservation()).is_some_and(|r| r.conversation_id == id)
    }

    async fn push_admission(&self, scope: &str, allowed: bool, expires_at_ms: i64) {
        let client = match reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(2))
            .build()
        {
            Ok(client) => client,
            Err(_) => return,
        };
        if let Err(error) = client
            .post(format!("{}/api/voice/admission", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .json(&serde_json::json!({
                "scope": scope,
                "allowed": allowed,
                "expires_at_ms": expires_at_ms,
            }))
            .send()
            .await
        {
            tracing::warn!(%error, "voice_wake_admission_push_failed");
        }
    }
    async fn refresh_wake_admission(&self) {
        if self.self_check_active.load(Ordering::Acquire) {
            self.push_admission("wake", false, 0).await;
            self.push_admission("call", false, 0).await;
            return;
        }
        let capabilities = self.current_capabilities().await;
        let expires_at_ms = capabilities
            .as_ref()
            .and_then(|report| report.checked_at_ms)
            .map(|checked| checked.saturating_add(60_000))
            .unwrap_or(0);
        let wake = capabilities.as_ref().is_some_and(|report| report.can_wake);
        let call = capabilities
            .as_ref()
            .is_some_and(|report| report.can_start_full_call);
        self.push_admission("wake", wake, if wake { expires_at_ms } else { 0 })
            .await;
        self.push_admission("call", call, if call { expires_at_ms } else { 0 })
            .await;
    }
    fn invalidate_capabilities_if(&self, attempt: u64, generation: u64) {
        let mut guard = self
            .capabilities
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if guard
            .as_ref()
            .is_some_and(|stored| stored.attempt == attempt && stored.report.generation == generation)
        {
            *guard = None;
        }
    }
    fn invalidate_capabilities(&self) {
        *self
            .capabilities
            .write()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }

    async fn capability_signature(&self) -> String {
        let status = self.models_manager.get_status().await;
        let snapshot = self.snapshot();
        let dialogue = match &self.dialogue_completer {
            Some(completer) => completer.configuration_revision().await,
            None => "unconfigured".into(),
        };
        let tts = snapshot
            .tts
            .as_ref()
            .map(|value| {
                format!(
                    "{}:{}:{}",
                    value.provider,
                    value.voice.as_deref().unwrap_or(""),
                    value.online
                )
            })
            .unwrap_or_else(|| "unknown".into());
        let audio = snapshot.audio.as_ref();
        let daemon_instance_id = self
            .source
            .read()
            .ok()
            .and_then(|s| s.as_ref().map(|(id, _)| id.clone()))
            .unwrap_or_else(|| "none".into());
        let worker_epoch = snapshot.worker_epoch.unwrap_or(0);
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            snapshot.instance_id,
            daemon_instance_id,
            worker_epoch,
            self.generation.load(Ordering::Acquire),
            status.state.status_name(),
            status.models_dir.display(),
            status.total_size_bytes,
            self.models_manager.identity_revision(),
            snapshot.dialogue_ready,
            dialogue,
            tts,
            snapshot.playback_ready,
            audio.is_some_and(|value| value.asr_ready),
            audio.is_some_and(|value| value.vad_ready),
            audio
                .map(|value| value.capture_state.as_str())
                .unwrap_or("none")
        )
    }

    /// Returns the stored report only while its attempt, version signature and
    /// TTL are still current. Expiry never runs a background check or hangs up
    /// an established call.
    pub async fn current_capabilities(
        &self,
    ) -> Option<nomifun_api_types::voice::VoiceCapabilityReport> {
        let stored = self
            .capabilities
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()?;
        let observed_attempt = stored.attempt;
        let observed_generation = stored.report.generation;
        if nomifun_common::now_ms() > stored.expires_at_ms
            || observed_attempt != self.self_check_attempt.load(Ordering::Acquire)
            || observed_generation != self.generation.load(Ordering::Acquire)
            || stored.signature != self.capability_signature().await
        {
            self.invalidate_capabilities_if(observed_attempt, observed_generation);
            return None;
        }
        // The cached report must still describe the current runtime: a 60s TTL
        // alone cannot hide a dead microphone, sink or engine between reads.
        if !readiness_runtime_scope_valid(&self.snapshot(), nomifun_common::now_ms()) {
            self.invalidate_capabilities_if(observed_attempt, observed_generation);
            return None;
        }
        // A newer check may have published while this read was validating.
        let still_current = self
            .capabilities
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|current| {
                current.attempt == observed_attempt && current.signature == stored.signature
            });
        if !still_current {
            return None;
        }
        Some(stored.report)
    }

    /// Explicit user-triggered full check. Capture comes from fresh PCM; TTS and
    /// playback come from the production daemon provider; the LLM check is a
    /// fixed, tool-free, context-free request and is opt-in because it may cost.
    /// Cancels the current explicit readiness check (including its fixed-sample
    /// playback) without touching an established voice call.
    pub fn cancel_self_check(&self) {
        self.self_check_cancel_signal.notify_waiters();
    }

    pub async fn run_self_check(
        self: &Arc<Self>,
        include_llm: bool,
    ) -> Result<nomifun_api_types::voice::VoiceCapabilityReport, String> {
        self.run_self_check_with_locale_and_stage(include_llm, None, None).await
    }

    pub async fn run_self_check_with_locale_and_stage(
        self: &Arc<Self>,
        include_llm: bool,
        locale: Option<String>,
        stage: Option<String>,
    ) -> Result<nomifun_api_types::voice::VoiceCapabilityReport, String> {
        tokio::select! {
            biased;
            _ = self.self_check_cancel_signal.notified() => Err("self_check_cancelled".into()),
            result = self.run_self_check_inner(include_llm, locale, stage) => result,
        }
    }

    async fn run_self_check_inner(
        self: &Arc<Self>,
        include_llm: bool,
        locale: Option<String>,
        stage: Option<String>,
    ) -> Result<nomifun_api_types::voice::VoiceCapabilityReport, String> {
        use nomifun_api_types::voice::{
            VoiceCapabilityEvidence, VoiceCapabilityReport, VoiceCapabilityStatus,
            VoiceEngineSelfCheckResponse, VoiceTtsSelfCheckResponse,
        };
        let _guard = self.self_check_lock.try_lock().map_err(|_| "self_check_in_progress")?;
        {
            let _admission = self.call_start_lock.lock().await;
            if self.self_check_active.load(Ordering::Acquire) {
                return Err("self_check_in_progress".into());
            }
            if self.voice_suite_in_use() {
                return Err("voice_suite_in_use".into());
            }
            self.self_check_active.store(true, Ordering::Release);
        }
        let attempt = self.self_check_attempt.fetch_add(1, Ordering::AcqRel) + 1;
        let checked_at_ms = nomifun_common::now_ms();
        let generation = self.generation.load(Ordering::Acquire);
        let snapshot = self.snapshot();
        let check_id = format!("{}-{}", snapshot.instance_id, attempt);
        let mut flight = SelfCheckFlight {
            manager: Arc::downgrade(self),
            check_id,
            finished: false,
        };
        self.push_admission("wake", false, 0).await;
        self.push_admission("call", false, 0).await;
        let signature = self.capability_signature().await;
        let evidence = |status: VoiceCapabilityStatus,
                        reason_code: Option<&str>,
                        evidence: &str| VoiceCapabilityEvidence {
            status,
            reason_code: reason_code.map(str::to_owned),
            evidence: evidence.into(),
            checked_at_ms: Some(checked_at_ms),
        };
        let audio = snapshot.audio.as_ref();
        let recent_pcm = audio.is_some_and(|value| {
            value.frames_received > 0
                && value
                    .last_frame_at_ms
                    .is_some_and(|at| checked_at_ms.saturating_sub(at) <= 2_000)
                && value.capture_state != "closed"
        });
        let capture = if recent_pcm {
            evidence(VoiceCapabilityStatus::Ready, None, "recent_pcm")
        } else {
            evidence(
                VoiceCapabilityStatus::Failed,
                Some("capture_no_recent_pcm"),
                "recent_pcm",
            )
        };
        let daemon_source = self.source.read().ok().and_then(|s| s.clone());
        let daemon_instance_id = daemon_source.as_ref().map(|(id, _)| id.clone());
        let expected_worker_epoch = snapshot.worker_epoch.unwrap_or(0);
        let engine_request_id = format!("engine-{}-{}", snapshot.instance_id, attempt);
        let model_revision = self.models_manager.identity_revision();
        let (engine_response, engine_bind_reason): (Option<VoiceEngineSelfCheckResponse>, Option<&str>) = {
            let client = match reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(90))
                .build()
            {
                Ok(client) => client,
                Err(_) => return Err("engine_check_client_failed".into()),
            };
            let sent = client
                .get(format!(
                    "{}/api/voice/diagnostics/engine-self-check?request_id={}&timeout_ms=45000",
                    self.daemon_origin, engine_request_id
                ))
                .header("x-ai-mate-control-token", &self.control_token)
                .header("x-voice-check-request", &engine_request_id)
                .header("x-voice-check-instance", &snapshot.instance_id)
                .header("x-voice-check-model-revision", &model_revision)
                .send()
                .await;
            match sent {
                Ok(response) if response.status().is_success() => {
                    match response.json::<VoiceEngineSelfCheckResponse>().await {
                        Ok(parsed)
                            if parsed.request_id == engine_request_id
                                && daemon_instance_id.as_deref() == Some(&parsed.instance_id)
                                && parsed.worker_epoch > 0
                                && expected_worker_epoch > 0
                                && parsed.worker_epoch == expected_worker_epoch
                                && parsed.model_revision == model_revision =>
                        {
                            (Some(parsed), None)
                        }
                        Ok(_) => (None, Some("engine_response_stale")),
                        Err(_) => (None, Some("engine_response_invalid")),
                    }
                }
                _ => (None, Some("engine_check_unreachable")),
            }
        };        let (kws, vad, asr) = match engine_response {
            Some(response) if response.fixtures_available => (
                if response.kws_ok {
                    evidence(VoiceCapabilityStatus::Ready, None, "fixed_audio_inference")
                } else {
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some("kws_inference_failed"),
                        "fixed_audio_inference",
                    )
                },
                if response.vad_ok {
                    evidence(VoiceCapabilityStatus::Ready, None, "fixed_audio_inference")
                } else {
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some("vad_inference_failed"),
                        "fixed_audio_inference",
                    )
                },
                if response.asr_ok {
                    evidence(VoiceCapabilityStatus::Ready, None, "fixed_audio_inference")
                } else {
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some("asr_inference_failed"),
                        "fixed_audio_inference",
                    )
                },
            ),
            Some(response) => {
                let reason = match response.reason.as_deref() {
                    Some(s) if s.starts_with("fixture_synthesis_failed") => {
                        "fixture_synthesis_failed".to_string()
                    }
                    Some(s) => s.to_string(),
                    None => "fixed_audio_missing".to_string(),
                };
                (
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(&reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(&reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(&reason),
                        "fixed_audio_inference",
                    ),
                )
            }
            None => {
                let reason = engine_bind_reason.unwrap_or("engine_check_unreachable");
                (
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "fixed_audio_inference",
                    ),
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "fixed_audio_inference",
                    ),
                )
            }        };

        let tts_response: Result<VoiceTtsSelfCheckResponse, String> = {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(40))
                .build()
                .map_err(|_| "tts_check_client_failed".to_string())?;
            match client
                .post(format!(
                    "{}/api/voice/diagnostics/tts-self-check",
                    self.daemon_origin
                ))
                .header("x-ai-mate-control-token", &self.control_token)
                .json(&serde_json::json!({ "check_id": flight.check_id, "locale": locale, "stage": stage }))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => response
                    .json::<VoiceTtsSelfCheckResponse>()
                    .await
                    .map_err(|_| "tts_bad_response".to_string()),
                Ok(response) => {
                    let status_code = response.status().as_u16();
                    tracing::warn!(status = status_code, "voice_tts_self_check_rejected");
                    Err(format!("tts_http_{status_code}"))
                }
                Err(error) => {
                    tracing::warn!(%error, "voice_tts_self_check_unreachable");
                    Err("tts_check_unreachable".into())
                }
            }
        };
        let (tts, playback) = match tts_response {
            Ok(response) => {
                let languages_valid = tts_self_check_languages_valid(&response);
                let tts_valid = response.ok && languages_valid;
                (
                    if tts_valid {
                        evidence(VoiceCapabilityStatus::Ready, None, "tts_decoded")
                    } else {
                        evidence(
                            VoiceCapabilityStatus::Failed,
                            Some(if response.ok {
                                "tts_inconsistent_response"
                            } else {
                                response.reason.as_deref().unwrap_or("tts_failed")
                            }),
                            "tts_decoded",
                        )
                    },
                    if tts_valid && response.playback_ok {
                        evidence(VoiceCapabilityStatus::Ready, None, "sink_consumed")
                    } else {
                        evidence(
                            VoiceCapabilityStatus::Failed,
                            Some(if tts_valid {
                                "playback_not_consumed"
                            } else {
                                "playback_without_valid_synthesis"
                            }),
                            "sink_consumed",
                        )
                    },
                )
            }
            Err(reason) => (
                evidence(
                    VoiceCapabilityStatus::Failed,
                    Some(&reason),
                    "tts_decoded",
                ),
                evidence(
                    VoiceCapabilityStatus::Failed,
                    Some("playback_not_checked"),
                    "sink_consumed",
                ),
            ),
        };

        let llm = if !include_llm {
            evidence(
                VoiceCapabilityStatus::Unknown,
                Some("llm_check_not_run"),
                "live_response",
            )
        } else if let Some(completer) = &self.dialogue_completer {
            match tokio::time::timeout(
                std::time::Duration::from_secs(12),
                completer.complete("Reply with exactly: OK", None),
            )
            .await
            {
                Ok(Ok(reply)) if !reply.trim().is_empty() => {
                    evidence(VoiceCapabilityStatus::Ready, None, "live_response")
                }
                Ok(Ok(_)) => evidence(
                    VoiceCapabilityStatus::Failed,
                    Some("llm_empty_response"),
                    "live_response",
                ),
                Ok(Err(error)) => {
                    let reason = if error.contains("401") || error.contains("403") {
                        "llm_auth_failed"
                    } else if error.contains("429") {
                        "llm_rate_limited"
                    } else if error.contains("timeout") {
                        "llm_timeout"
                    } else {
                        "llm_failed"
                    };
                    evidence(
                        VoiceCapabilityStatus::Failed,
                        Some(reason),
                        "live_response",
                    )
                }
                Err(_) => evidence(
                    VoiceCapabilityStatus::Failed,
                    Some("llm_timeout"),
                    "live_response",
                ),
            }
        } else {
            evidence(
                VoiceCapabilityStatus::Unsupported,
                Some("dialogue_unconfigured"),
                "live_response",
            )
        };

        let ready = |value: &VoiceCapabilityEvidence| {
            value.status == VoiceCapabilityStatus::Ready
        };
        let can_start_full_call = ready(&capture)
            && ready(&vad)
            && ready(&asr)
            && ready(&llm)
            && ready(&tts)
            && ready(&playback);
        let reason_code = if can_start_full_call {
            None
        } else {
            [&capture, &vad, &asr, &llm, &tts, &playback]
                .into_iter()
                .find(|value| !ready(value))
                .and_then(|value| value.reason_code.clone())
                .or_else(|| Some("self_check_incomplete".into()))
        };
        let mut report = VoiceCapabilityReport {
            capture,
            kws,
            vad,
            asr,
            llm,
            tts,
            playback,
            checked_at_ms: Some(checked_at_ms),
            generation,
            can_start_full_call,
            can_wake: false,
            can_transcribe: false,
            reason_code,
            degraded_text_only: false,
        };
        report.can_wake = ready(&report.kws)
            && ready(&report.vad)
            && ready(&report.asr)
            && ready(&report.capture);
        report.can_transcribe =
            ready(&report.vad) && ready(&report.asr) && ready(&report.capture);
        report.degraded_text_only = !report.can_start_full_call && report.can_transcribe;

        {
            let _admission = self.call_start_lock.lock().await;
            let current_signature = self.capability_signature().await;
            let current_daemon_source = self.source.read().ok().and_then(|s| s.clone());
            let current = attempt == self.self_check_attempt.load(Ordering::Acquire)
                && generation == self.generation.load(Ordering::Acquire)
                && signature == current_signature
                && daemon_source.is_some()
                && daemon_source.as_ref().map(|(id, _)| id) == current_daemon_source.as_ref().map(|(id, _)| id)
                && readiness_runtime_scope_valid(&self.snapshot(), nomifun_common::now_ms());
            if current {
                *self
                    .capabilities
                    .write()
                    .unwrap_or_else(|error| error.into_inner()) = Some(StoredCapabilities {
                    report: report.clone(),
                    attempt,
                    expires_at_ms: checked_at_ms.saturating_add(60_000),
                    signature,
                });
            } else {
                report.can_start_full_call = false;
                report.reason_code = Some("self_check_superseded".into());
            }
            self.self_check_active.store(false, Ordering::Release);
            flight.finished = true;
        }
        self.push_admission(
            "wake",
            report.can_wake,
            if report.can_wake {
                checked_at_ms.saturating_add(60_000)
            } else {
                0
            },
        )
        .await;
        self.push_admission(
            "call",
            report.can_start_full_call,
            if report.can_start_full_call {
                checked_at_ms.saturating_add(60_000)
            } else {
                0
            },
        )
        .await;
        Ok(report)
    }

    /// Host authority for model writes: an active call, an unsettled task or a
    /// pending tool approval owns the acoustic suite until it finishes.
    pub fn voice_suite_in_use(&self) -> bool {
        let snapshot = self.snapshot();
        let active_call = snapshot.session_id.is_some()
            && snapshot.ended_reason.is_none()
            && snapshot.phase != VoiceSessionPhase::Idle;
        let unsettled_task = snapshot.task.as_ref().is_some_and(|task| {
            matches!(
                task.status,
                nomifun_api_types::voice::VoiceTaskStatus::Queued
                    | nomifun_api_types::voice::VoiceTaskStatus::Delivering
                    | nomifun_api_types::voice::VoiceTaskStatus::Unknown
                    | nomifun_api_types::voice::VoiceTaskStatus::Accepted
                    | nomifun_api_types::voice::VoiceTaskStatus::Cancelling
            )
        });
        let pending_approval = snapshot
            .confirmation
            .as_ref()
            .is_some_and(|confirmation| !confirmation.approval_id.is_empty());
        let pending_queue = !snapshot.queue.is_empty();
        let pending_start = self
            .dialogue_recorder
            .as_ref()
            .and_then(|recorder| recorder.current_call_reservation())
            .is_some();
        active_call || unsettled_task || pending_approval || pending_queue || pending_start
    }

    /// Atomically closes call/task admission, verifies nothing owns the suite,
    /// then releases the daemon (and its native model handles) before a write.
    pub async fn begin_model_write(self: &Arc<Self>) -> Result<ModelWriteGuard, String> {
        let _call = self.call_start_lock.lock().await;
        if self.resource_write_active.load(Ordering::Acquire) {
            return Err("voice_resource_write_in_progress".into());
        }
        if self.self_check_active.load(Ordering::Acquire) {
            return Err("voice_self_check_in_progress".into());
        }
        if self.voice_suite_in_use() {
            return Err("voice_suite_in_use".into());
        }
        self.resource_write_active.store(true, Ordering::Release);
        self.invalidate_capabilities();
        let resume_daemon =
            !self.shutdown_requested.load(Ordering::Acquire) && self.read_autostart_config();

        let released = {
            let mut child = self.child.lock().await;
            match child.take() {
                Some(mut process) => process.shutdown().await,
                None => Ok(()),
            }
        };
        if let Err(error) = released {
            self.resource_write_active.store(false, Ordering::Release);
            return Err(format!("daemon_release_failed: {error}"));
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.process_changed.notify_one();
        self.disconnected();
        Ok(ModelWriteGuard {
            manager: Arc::downgrade(self),
            resume_daemon,
            released: false,
        })
    }

    async fn resume_after_model_write(self: Arc<Self>) -> Result<(), String> {
        if self.shutdown_requested.load(Ordering::Acquire) || !self.read_autostart_config() {
            return Ok(());
        }
        self.start_child(false).await
    }

    pub fn global() -> Option<Arc<Self>> {
        GLOBAL_COMPANION_MANAGER.get().cloned()
    }

    pub fn new(models_manager: Arc<nomifun_voice_models::ModelManager>, data_dir: &Path) -> Self {
        let config_path = data_dir.join("config").join("voice_companion.json");
        let target_config = std::fs::read(&config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|val| val.get("target").cloned())
            .and_then(|target_val| serde_json::from_value::<VoiceTargetConfig>(target_val).ok())
            .unwrap_or_default();
        let (events, _) = broadcast::channel(128);
        Self {
            audio_recovery:std::sync::Mutex::new(AudioRecovery::default()),
            resource_write_active:AtomicBool::new(false),
            self_check_lock:tokio::sync::Mutex::new(()),
            self_check_active:AtomicBool::new(false),
            self_check_cancel_signal:tokio::sync::Notify::new(),
            self_check_attempt:AtomicU64::new(0),
            capabilities:std::sync::RwLock::new(None),
            restart_budget:std::sync::Mutex::new(RestartBudget::default()),
            prompted_approvals:std::sync::Mutex::new(std::collections::VecDeque::new()),
            revoked_sessions:RwLock::new(std::collections::VecDeque::new()),
            task_feedback: std::sync::Mutex::new(TaskFeedbackTracker::new()),
            daemon_origin: "http://127.0.0.1:18080".into(),
            #[cfg(test)]
            test_daemon_port: None,
            #[cfg(test)]
            last_child_exit: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(test)]
            test_child_pid: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            tasks: None,
            data_dir: data_dir.to_path_buf(),
            task_projection_started: AtomicBool::new(false),
            dialogue_completer: None,
            dialogue_recorder: None,
            target_selection: Arc::new(RwLock::new(target_config)),
            active_window_candidate: Arc::new(RwLock::new(VoiceWindowContext::default())),
            companion_service: None,
            dialogue_bridge: Mutex::new(None),
            config_write_lock: std::sync::Mutex::new(()),
            lifecycle_lock: Mutex::new(()),
            feedback_update_lock: Mutex::new(()),
            call_start_lock: tokio::sync::Mutex::new(()),
            handled_start_requests: std::sync::RwLock::new(std::collections::HashMap::new()),
            viewed_conversation: RwLock::new((0, None)),
            child: Arc::new(Mutex::new(None)),
            models_manager,
            config_path,
            control_token: format!(
                "{}{}",
                nomifun_common::generate_id(),
                nomifun_common::generate_id()
            ),
            snapshot: RwLock::new(VoiceSessionSnapshot {
                instance_id: nomifun_common::generate_id(),
                ..Default::default()
            }),
            source: RwLock::new(None),
            task_sync_seq: AtomicU64::new(0),
            events,
            projection_started: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            process_changed: Notify::new(),
            shutdown_requested: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> VoiceSessionSnapshot {
        self.snapshot
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    pub(crate) fn with_companion_service(mut self, companion_service: Arc<CompanionService>) -> Self {
        self.companion_service = Some(companion_service);
        self
    }

    pub fn target_selection(&self) -> Arc<RwLock<VoiceTargetConfig>> {
        self.target_selection.clone()
    }

    pub fn active_window_candidate(&self) -> Arc<RwLock<VoiceWindowContext>> {
        self.active_window_candidate.clone()
    }

    pub fn set_candidate_conversation(&self, candidate: Option<String>) {
        let mut context = self.active_window_candidate.write().unwrap_or_else(|e| e.into_inner());
        context.conversation_id = candidate;
        context.workspace = None;
    }

    pub fn report_window_context(&self, context: VoiceWindowContext) {
        let mut viewed = self.viewed_conversation.write().unwrap_or_else(|e| e.into_inner());
        // View-only reports participate in the same ordering as full targets.
        if context.sequence <= viewed.0 { return; }
        *viewed = (context.sequence, context.conversation_id.clone());
        let mut current = self.active_window_candidate.write().unwrap_or_else(|e| e.into_inner());
        if context.sequence > current.sequence { *current = context; }
    }

    pub fn report_voice_view(&self, conversation_id: Option<String>, sequence: u64) {
        let mut viewed = self.viewed_conversation.write().unwrap_or_else(|e| e.into_inner());
        if sequence > viewed.0 { *viewed = (sequence, conversation_id); }
    }

    pub fn requested_target(&self) -> VoiceTargetConfig {
        self.target_selection.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn record_handled_start_request(&self, request_id: String, outcome: HandledStartOutcome) {
        let mut map = self.handled_start_requests.write().unwrap_or_else(|e| e.into_inner());
        if map.len() >= 128 {
            let oldest_key = map.iter()
                .min_by_key(|(_, v)| match v {
                    HandledStartOutcome::Starting { reserved_at_epoch_ms, .. } => *reserved_at_epoch_ms,
                    HandledStartOutcome::Bound { bound_at_epoch_ms, .. } => *bound_at_epoch_ms,
                    HandledStartOutcome::Stopped { stopped_at_epoch_ms, .. } => *stopped_at_epoch_ms,
                    HandledStartOutcome::Rejected { at_epoch_ms, .. } => *at_epoch_ms,
                })
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest_key {
                map.remove(&k);
            }
        }
        map.insert(request_id, outcome);
    }

    pub fn mark_request_bound(&self, request_id: &str, session_id: &str, conversation_id: &str, memory_owner: Option<&str>) {
        let mut map = self.handled_start_requests.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = map.get_mut(request_id) {
            if !matches!(existing, HandledStartOutcome::Starting { conversation_id: expected, .. } if expected == conversation_id) {
                return;
            }
            let target = existing.target().clone();
            let session_mode = existing.session_mode().clone();
            *existing = HandledStartOutcome::Bound {
                target,
                session_mode,
                conversation_id: conversation_id.to_string(),
                session_id: Some(session_id.to_string()),
                memory_owner: memory_owner.map(|s| s.to_string()),
                bound_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
            };
        }
    }

    pub fn mark_active_request_stopped(&self, session_id: Option<&str>, call_request_id: Option<&str>) {
        let Some(sid) = session_id else { return; };
        {
            let mut map = self.handled_start_requests.write().unwrap_or_else(|e| e.into_inner());
            for (request_id, outcome) in map.iter_mut() {
                let matches = match outcome {
                    HandledStartOutcome::Starting { .. } => call_request_id == Some(request_id.as_str()),
                    HandledStartOutcome::Bound { session_id: Some(bound), .. } => bound == sid
                        && call_request_id.is_none_or(|id| id == request_id),
                    _ => false,
                };
                if matches {
                    if let HandledStartOutcome::Starting { conversation_id, target, session_mode, .. }
                        | HandledStartOutcome::Bound { conversation_id, target, session_mode, .. } = outcome {
                        *outcome = HandledStartOutcome::Stopped {
                            target: target.clone(), session_mode: session_mode.clone(),
                            conversation_id: conversation_id.clone(), session_id: Some(sid.into()),
                            stopped_at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                        };
                    }
                }
            }
        }
        // Freeze takes the reservation lock before the receipt lock; never invert that order here.
        if let (Some(recorder), Some(request_id)) = (&self.dialogue_recorder, call_request_id) {
            recorder.take_call_reservation(request_id);
        }
    }

    pub async fn target_settings(&self) -> VoiceTargetSettings {
        // Keep the persisted request readable, but do not claim retired pet routing is effective.
        VoiceTargetSettings {
            requested: self.requested_target(),
            effective: VoiceTargetConfig { mode: VoiceTargetMode::VoiceHome, companion_id: None },
            effective_conversation_id: self.snapshot().binding.map(|binding| binding.conversation_id),
            reason: Some("workspace_routing_active".into()),
        }
    }
    pub async fn update_target(&self, update: VoiceTargetUpdateRequest) -> Result<VoiceTargetSettings, String> {
        let new_config = match update.mode {
            VoiceTargetMode::VoiceHome => VoiceTargetConfig {
                mode: VoiceTargetMode::VoiceHome,
                companion_id: None,
            },
            VoiceTargetMode::Companion => {
                let comp_id = update.companion_id.ok_or_else(|| "companion_id_required".to_string())?;
                if let Some(companion_service) = &self.companion_service {
                    if companion_service.get_companion(&comp_id).await.is_err() {
                        return Err("companion_not_found".to_string());
                    }
                }
                VoiceTargetConfig {
                    mode: VoiceTargetMode::Companion,
                    companion_id: Some(comp_id),
                }
            }
        };
        let value = serde_json::to_value(&new_config).map_err(|e| e.to_string())?;
        self.write_config_entry("target", value)?;
        *self.target_selection.write().unwrap_or_else(|e| e.into_inner()) = new_config;
        Ok(self.target_settings().await)
    }

    pub(crate) fn with_dialogue(mut self, completer: Arc<dyn VoiceDialogueCompleter>) -> Self {
        self.dialogue_completer = Some(completer);
        self
    }

    pub(crate) fn with_recorder(mut self, recorder: Arc<dyn VoiceDialogueRecorder>) -> Self {
        self.dialogue_recorder = Some(recorder);
        self
    }

    pub(crate) fn with_tasks(mut self, owner: String, runtime: Arc<dyn VoiceTaskRuntime>) -> Self {
        let path = self.data_dir.join("voice").join(format!("tasks-{owner}.json"));
        self.tasks = Some(Arc::new(VoiceTaskService::new(owner, path, runtime)));
        self
    }

    async fn execution_settings(&self) -> VoiceExecutionSettings {
        let session = self.snapshot().session_id;
        let candidate = match &self.dialogue_recorder {
            Some(recorder) => match session {
                Some(session) => recorder.bound_execution(&session).await,
                None => recorder.fallback_execution().await,
            },
            None => Ok(None),
        };
        let resolution = match (candidate, &self.tasks) {
            (Err(code), _) => Err(code),
            (Ok(None), _) => Err("execution_unconfigured".into()),
            (Ok(Some(_)), None) => Err("runtime_unavailable".into()),
            (Ok(Some(config)), Some(tasks)) => tasks.resolve_configuration(&config).await,
        };
        let (effective, reason) = match resolution { Ok(config) => (Some(config),None), Err(code) => (None,Some(code)) };
        let settings = VoiceExecutionSettings {
            effective,
            available: reason.is_none(),
            requested: None,
            reason_code: reason,
        };
        let mut snapshot = self.snapshot.write().unwrap_or_else(|error| error.into_inner());
        if snapshot.task_ready != settings.available {
            snapshot.task_ready = settings.available;
            snapshot.seq += 1;
            let _ = self.events.send(snapshot.clone());
        }
        settings
    }

    fn project_task(&self, task: VoiceTaskSnapshot) {
        let (task,queue) = match &self.tasks {
            Some(tasks) => match (tasks.current_task(),tasks.queue_projection()) {
                (Ok(Some(current)),Ok(queue)) => (current,queue),
                _ => return,
            },
            None => (task,Vec::new()),
        };
        let sync_seq;
        {
            let mut snapshot = self.snapshot.write().unwrap_or_else(|error| error.into_inner());
            // A poll or HTTP read may finish after a newer persisted receipt or
            // command. Publish only the current durable projection under this lock.
            if self.tasks.as_ref().is_some_and(|tasks|tasks.current_task().ok().flatten().as_ref()!=Some(&task)) {return;}
            if snapshot.task.as_ref() == Some(&task) && snapshot.queue == queue { return; }
            let changed_task = snapshot.task.as_ref() != Some(&task);
            snapshot.task = Some(task.clone());
            snapshot.queue = queue;
            if changed_task {snapshot.confirmation=None;}
            snapshot.task_error_code = None;
            snapshot.seq += 1;
            let _ = self.events.send(snapshot.clone());
            sync_seq = self.task_sync_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        }
        let daemon_origin = self.daemon_origin.clone();
        let token = self.control_token.clone();
        let daemon_instance_id = self.source.read().ok().and_then(|s| s.as_ref().map(|(id, _)| id.clone()));
        tokio::spawn(async move {
            let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build();
            if let Ok(c) = client {
                let payload = serde_json::json!({
                    "task": Some(task),
                    "instance_id": daemon_instance_id,
                    "seq": sync_seq,
                });
                let _ = c.post(format!("{daemon_origin}/api/voice/task-status")).header("x-ai-mate-control-token", token).json(&payload).send().await;
            }
        });
    }

    fn project_task_error(&self, code: String) {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|error| error.into_inner());
        if snapshot.task_error_code.as_ref() == Some(&code) { return; }
        snapshot.task_error_code = Some(code);
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    fn project_confirmation(&self,task:&VoiceTaskSnapshot,confirmation:Option<VoiceToolConfirmation>) {
        let mut snapshot=self.snapshot.write().unwrap_or_else(|error|error.into_inner());
        if snapshot.task.as_ref()!=Some(task) || snapshot.confirmation==confirmation {return;}
        snapshot.confirmation=confirmation;
        snapshot.seq+=1;
        let _=self.events.send(snapshot.clone());
    }

    fn start_task_projection(self: &Arc<Self>) {
        if self.tasks.is_none() || self.task_projection_started.swap(true, Ordering::AcqRel) { return; }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut speech:Option<tokio::task::JoinHandle<()>>=None;
            if let Some(manager) = weak.upgrade() {manager.execution_settings().await;}
            loop {
                let Some(manager)=weak.upgrade() else {return;};
                let tasks=manager.tasks.as_ref().expect("task projection requires its adapter");
                let mut active=None;
                let mut approval=None;
                match tasks.status().await {
                    Ok(Some(task))=>{
                        manager.project_task(task.clone());
                        let progress=if matches!(task.status,VoiceTaskStatus::Accepted|VoiceTaskStatus::Cancelling) {tasks.progress_for(&task).ok()}else{None};
                        let notification=tasks.notification_for(&task.command_id, manager.snapshot().locale.as_deref()).ok().flatten();
                        {
                            let mut snapshot=manager.snapshot.write().unwrap_or_else(|e|e.into_inner());
                            if snapshot.task_progress!=progress || snapshot.task_notification!=notification {
                                snapshot.task_progress=progress;snapshot.task_notification=notification;snapshot.seq+=1;let _=manager.events.send(snapshot.clone());
                            }
                        }
                        match tasks.confirmations(&task.command_id).await {
                            Ok(confirmations)=>{approval=confirmations.into_iter().next();manager.project_confirmation(&task,approval.clone());},
                            Err(code)=>manager.project_task_error(code),
                        }
                        active=Some(task);
                    },
                    Ok(None)=>{}, Err(code)=>manager.project_task_error(code),
                }
                let _=manager.reconcile_announcements().await;
                // Playback and approval listening never hold up deadline/receipt observation.
                // One owned speech job keeps notices bounded and gives terminal/approval work priority.
                if speech.as_ref().is_none_or(|job|job.is_finished()) {
                    // Recover the binding after a missed SSE frame/reconnect before selecting a notice.
                    if manager.snapshot().binding.is_none() && active.is_some() {
                        let _ = manager.refresh_daemon_snapshot().await;
                    }
                    let active_binding = manager.snapshot().binding;
                    if let Some(binding) = active_binding {
                        let conv_id = binding.conversation_id.clone();
                        if let (Some(task),Some(confirmation))=(active.clone().filter(|t| t.conversation_id.as_deref() == Some(conv_id.as_str())),approval) {
                            let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.notify_confirmation(&task,&confirmation).await {owner.project_task_error(code);}}));
                        } else if let Ok(Some((task,goal)))=tasks.pending_result_for_conversation(&conv_id) {
                            manager.task_feedback.lock().unwrap_or_else(|e|e.into_inner()).clear_pending();
                            let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.notify_task_result(&task,&goal).await {owner.project_task_error(code);}}));
                        } else if let Some(task)=active.filter(|task|task.conversation_id.as_deref() == Some(conv_id.as_str()) && task.status==VoiceTaskStatus::Accepted) {
                            let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.maybe_notify_task_progress(&task).await {owner.project_task_error(code);}}));
                        }
                    }
                }
                drop(manager);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }

    async fn maybe_notify_task_progress(&self, task: &VoiceTaskSnapshot) -> Result<(), String> {
        let now_ms = nomifun_common::now_ms();
        let decision = {
            let mut tracker = self.task_feedback.lock().unwrap_or_else(|e| e.into_inner());
            tracker.evaluate_progress(task,self.snapshot().task_progress.as_ref(),now_ms)
        };
        let TaskFeedbackDecision::TriggerFeedback { command_id: _, stage: _ } = decision else {
            return Ok(());
        };

        let snapshot = self.snapshot();
        if !snapshot.enabled || snapshot.connection != VoiceConnectionState::Connected {
            return Ok(());
        }
        let Some(binding) = &snapshot.binding else { return Ok(()); };
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Ok(()); }

        // Priority 1: Check active tool confirmation
        if snapshot.confirmation.is_some() {
            return Ok(());
        }

        // Priority 2: Check acoustic state (no 抢话 during user utterance)
        if snapshot.audio.as_ref().is_some_and(|audio| audio.speech_detected) {
            return Ok(());
        }

        let source = match self.refresh_daemon_snapshot().await {
            Ok(source) => source,
            Err(_) => return Ok(()),
        };
        if source.session_id.as_deref() != Some(&binding.session_id) {
            return Ok(());
        }
        if !matches!(source.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
            || source.ended_reason == Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || source.audio.as_ref().is_some_and(|audio| audio.speech_detected)
        {
            return Ok(());
        }

        let tasks=self.tasks.as_ref().ok_or("runtime_unavailable")?;
        if tasks.status_for(&task.command_id).await?.status!=VoiceTaskStatus::Accepted {return Ok(());}
        let progress=tasks.progress_for(task)?;
        let text=match progress.health.as_str() {
            "unverified"=>"暂时无法核实执行侧状态。任务尚未确认结束，你可以查询、取消或查看任务详情。",
            "no_recent_progress"=>"任务九十秒内没有新的业务进展，尚未确认结束。你可以继续等待、查询或取消。",
            _=>match progress.stage.as_str() {
                "thinking"=>"执行侧已报告正在分析任务，结果尚未返回。",
                "tool"=>"执行侧已报告工具调用进展，最终结果尚未返回。",
                "answering"=>"执行侧正在生成文字结果，尚未确认完成。",
                "planning"=>"执行侧已更新计划，任务尚未确认完成。",
                "approval"=>return Ok(()),
                _=>"任务已经接受，尚未收到新的执行进展。你可以查询或取消任务。",
            },
        };
        let request=nomifun_api_types::voice::VoiceTaskResultAnnouncement {
            scope:VoiceSessionControlRequest{instance_id:source.instance_id,session_id:source.session_id,turn_id:source.turn_id},
            command_id:task.command_id.clone(),notification_id:Some(format!("voice-progress:{}",task.command_id)),
            attempt_id:Some(format!("voice-progress:{}:{now_ms}",task.command_id)),goal:None,text:text.into(),
        };
        let response=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"daemon_unavailable")?
            .post(format!("{}/api/voice/task-progress",self.daemon_origin)).header("x-ai-mate-control-token",&self.control_token)
            .json(&request).timeout(std::time::Duration::from_secs(45)).send().await;
        if let Ok(response)=response && response.status().is_success() {
            self.task_feedback.lock().unwrap_or_else(|e|e.into_inner()).record_dispatched(now_ms);
        }
        Ok(())
    }

    async fn notify_task_result(&self, task: &VoiceTaskSnapshot, goal:&str) -> Result<(),String> {
        let snapshot = self.snapshot();
        if !matches!(task.status,VoiceTaskStatus::Completed|VoiceTaskStatus::Failed)
            || !snapshot.enabled || snapshot.connection!=VoiceConnectionState::Connected
        {return Ok(());}
        let Some(binding) = &snapshot.binding else { return Ok(()); };
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Ok(()); }
        let tasks = self.tasks.as_ref().ok_or("runtime_unavailable")?;
        if tasks.announcement_claimed(&task.command_id)? {return Ok(());}
        let source = match self.refresh_daemon_snapshot().await {Ok(source) => source,Err(_) => return Ok(())};
        if source.session_id.as_deref() != Some(&binding.session_id) { return Ok(()); }
        if !matches!(source.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
            || source.ended_reason == Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || source.audio.as_ref().is_some_and(|audio| audio.speech_detected) {return Ok(());}
        let text = format_task_result_announcement(task, goal, nomifun_api_types::voice::is_english_locale(source.locale.as_deref()));
        // Claim durably before dispatch: a lost response or host restart must
        // not replay a result as a new announcement. Text remains in the task.
        if !tasks.claim_announcement(&task.command_id,true).await? {return Ok(());}
        let notification=tasks.notification_for(&task.command_id, source.locale.as_deref())?.ok_or("task_announcement_failed")?;
        let request = nomifun_api_types::voice::VoiceTaskResultAnnouncement {
            scope:VoiceSessionControlRequest {instance_id:source.instance_id,session_id:source.session_id,turn_id:source.turn_id},
            command_id:task.command_id.clone(),
            notification_id: Some(notification.notification_id),
            attempt_id: Some(notification.attempt_id.clone()),
            goal:Some(goal.to_owned()),
            text,
        };
        let response = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "task_announcement_failed")?
            .post(format!("{}/api/voice/task-result",self.daemon_origin))
            .header("x-ai-mate-control-token",&self.control_token).json(&request)
            .timeout(std::time::Duration::from_secs(45)).send().await;
        match response {
            Ok(response) if response.status().is_success() => {
                let receipt=match response.json::<nomifun_api_types::voice::VoicePlaybackReceipt>().await {
                    Ok(receipt)=>receipt,
                    Err(_)=>{tasks.finish_announcement(&task.command_id,&notification.attempt_id,"unknown").await?;return Err("task_announcement_failed".into());},
                };
                if receipt.attempt_id!=notification.attempt_id {tasks.finish_announcement(&task.command_id,&notification.attempt_id,"unknown").await?;return Err("task_scope_changed".into());}
                tasks.finish_announcement(&task.command_id,&receipt.attempt_id,&receipt.status).await?;
                if receipt.status=="failed" {return Err("task_announcement_failed".into());}
                Ok(())
            },
            Ok(response) if response.status()==reqwest::StatusCode::CONFLICT => {
                let body=response.json::<serde_json::Value>().await.unwrap_or_default();
                if matches!(body["code"].as_str(),Some("voice_busy"|"task_scope_changed")) {
                    tasks.claim_announcement(&task.command_id,false).await?;
                    Ok(())
                } else {
                    tasks.finish_announcement(&task.command_id,&notification.attempt_id,"failed").await?;
                    Err("task_announcement_failed".into())
                }
            },
            _ => {
                tasks.finish_announcement(&task.command_id,&notification.attempt_id,"unknown").await?;
                Err("task_announcement_failed".into())
            },
        }
    }

    async fn reconcile_announcements(&self)->Result<(),String> {
        let Some(tasks)=&self.tasks else {return Ok(());};
        let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"daemon_unavailable")?;
        for (command,attempt) in tasks.unresolved_announcements()? {
            let mut url=reqwest::Url::parse(&format!("{}/api/voice/notifications/",self.daemon_origin)).map_err(|_|"daemon_unavailable")?;
            url.path_segments_mut().map_err(|_|"daemon_unavailable")?.pop_if_empty().push(&attempt);
            let response=client.get(url).header("x-ai-mate-control-token",&self.control_token).timeout(std::time::Duration::from_millis(250)).send().await;
            if let Ok(response)=response && response.status().is_success() {
                let receipt=response.json::<nomifun_api_types::voice::VoicePlaybackReceipt>().await.map_err(|_|"task_announcement_failed")?;
                if receipt.attempt_id==attempt {tasks.finish_announcement(&command,&attempt,&receipt.status).await?;}
            } else {tasks.finish_announcement(&command,&attempt,"unknown").await?;}
        }
        Ok(())
    }

    pub async fn replay_task_result(&self, command_id: &str,request:&nomifun_api_types::voice::VoiceTaskReplayRequest) -> Result<(), String> {
        let scope=self.snapshot();
        if request.instance_id!=scope.instance_id || scope.session_id.as_deref()!=Some(&request.session_id) || scope.turn_id.as_deref()!=Some(&request.turn_id) {return Err("task_scope_changed".into());}
        if !scope.enabled || scope.connection!=VoiceConnectionState::Connected {return Err("daemon_unavailable".into());}
        if scope.ended_reason==Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop) {return Err("task_scope_changed".into());}
        let Some(binding) = &scope.binding else { return Err("binding_invalid".into()); };
        self.reconcile_announcements().await?;
        let tasks = self.tasks.as_ref().ok_or("runtime_unavailable")?;
        let task = tasks.status_for(command_id).await?;
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Err("task_command_conflict".into()); }
        if !matches!(task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) {
            return Err("task_not_terminal".into());
        }
        let goal = tasks.pending_result_for_conversation(&binding.conversation_id).ok().flatten()
            .and_then(|(t, g)| (t.command_id == command_id).then_some(g))
            .unwrap_or_else(|| "先前任务".into());
        // Unclaim so next claim generates a new attempt_id
        if !tasks.request_replay(command_id,&request.replay_id).await? {return Ok(());}
        self.notify_task_result(&task, &goal).await
    }

    async fn notify_confirmation(&self,task:&VoiceTaskSnapshot,confirmation:&VoiceToolConfirmation)->Result<(),String> {
        use nomifun_api_types::voice::{VoiceTaskReference,VoiceToolConfirmationPrompt,VoiceToolConfirmationReply};
        let snapshot=self.snapshot();
        if !snapshot.enabled || snapshot.connection!=VoiceConnectionState::Connected
            || confirmation.expires_at_ms<=nomifun_common::now_ms() {return Ok(());}
        let Some(binding) = &snapshot.binding else { return Ok(()); };
        if task.conversation_id.as_deref() != Some(binding.conversation_id.as_str()) { return Ok(()); }
        if self.prompted_approvals.lock().unwrap_or_else(|error|error.into_inner()).contains(&confirmation.approval_id){return Ok(());}
        let source=match self.refresh_daemon_snapshot().await {Ok(source)=>source,Err(_)=>return Ok(())};
        if source.session_id.as_deref() != Some(&binding.session_id) { return Ok(()); }
        if !matches!(source.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
            || source.ended_reason == Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || source.audio.as_ref().is_some_and(|audio| audio.speech_detected) {return Ok(());}
        let summary=nomifun_ai_agent::voice_confirmation_summary(&confirmation.tool_name,&confirmation.arguments).ok_or("task_approval_detail_required")?;
        let text=format!("准备执行工具 {}，参数为 {}。完整参数可在面板查看。提示结束后，请说确认或取消；只允许本次操作。",confirmation.tool_name,summary);
        let request=VoiceToolConfirmationPrompt {
            scope:VoiceSessionControlRequest {instance_id:source.instance_id,session_id:source.session_id,turn_id:source.turn_id},
            task:VoiceTaskReference::from(task),approval_id:confirmation.approval_id.clone(),msg_id:confirmation.msg_id.clone(),call_id:confirmation.call_id.clone(),
            expires_at_ms:confirmation.expires_at_ms,text,
        };
        {
            let mut prompted=self.prompted_approvals.lock().unwrap_or_else(|error|error.into_inner());
            if prompted.contains(&confirmation.approval_id){return Ok(());}
            prompted.push_back(confirmation.approval_id.clone());if prompted.len()>64{prompted.pop_front();}
        }
        let remaining=std::time::Duration::from_millis(confirmation.expires_at_ms.saturating_sub(nomifun_common::now_ms()).max(1) as u64);
        let response=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"task_approval_failed")?
            .post(format!("{}/api/voice/confirmation",self.daemon_origin)).header("x-ai-mate-control-token",&self.control_token)
            .json(&request).timeout(remaining).send().await;
        let approved=match response {
            Ok(response) if response.status().is_success()=>response.json::<VoiceToolConfirmationReply>().await.ok()
                .is_some_and(|reply|reply.approval_id==confirmation.approval_id && reply.msg_id==confirmation.msg_id && reply.call_id==confirmation.call_id && reply.approved),
            Ok(response) if response.status()==reqwest::StatusCode::CONFLICT=>{
                self.prompted_approvals.lock().unwrap_or_else(|error|error.into_inner()).retain(|id|id!=&confirmation.approval_id);
                return Ok(());
            },
            _=>false,
        };
        let decision=VoiceToolConfirmationRequest {instance_id:snapshot.instance_id,command_id:task.command_id.clone(),conversation_id:task.conversation_id.clone(),message_id:task.message_id.clone(),msg_id:confirmation.msg_id.clone(),call_id:confirmation.call_id.clone(),approved};
        let tasks=self.tasks.as_ref().ok_or("runtime_unavailable")?;
        match tasks.confirm(&confirmation.approval_id,&decision).await {
            Err(code) if code=="task_approval_changed"=>Ok(()),
            result=>result,
        }
    }

    async fn submit_task(self: &Arc<Self>, request: VoiceTaskSubmitRequest) -> Result<VoiceTaskDelivery, String> {
        if self.resource_write_active.load(Ordering::Acquire) {
            return Err("voice_resource_write_in_progress".into());
        }
        let tasks = self.tasks.clone().ok_or("runtime_unavailable")?;
        let snapshot = self.snapshot();
        if request.instance_id != snapshot.instance_id {return Err("task_scope_changed".into());}
        if let Some(delivery)=tasks.replay(&request.command).await? {
            self.project_task(delivery.task.clone());
            return Ok(delivery);
        }
        if snapshot.session_id.as_deref() != Some(request.command.session_id.as_str())
            || snapshot.turn_id.as_deref() != Some(request.command.turn_id.as_str())
            || !matches!(snapshot.phase, VoiceSessionPhase::Deciding | VoiceSessionPhase::Dispatching)
            || !snapshot.enabled || snapshot.connection != VoiceConnectionState::Connected
            || self.revoked_sessions.read().unwrap_or_else(|error|error.into_inner()).contains(&request.command.session_id)
        { return Err("task_scope_changed".into()); }
        let recorder = self.dialogue_recorder.as_ref().ok_or("binding_invalid")?;
        let frozen = recorder.task_configuration(&request.command).await?.ok_or("binding_invalid")?;
        let config = frozen.config;
        // Admission belongs to the host: dropping a voice request cannot cancel
        // an accepted task or abandon its durable delivery record.
        let manager = self.clone();
        let command_id = request.command.command_id.clone();
        let command_id_timeout = command_id.clone();
        let session_id = request.command.session_id.clone();
        let turn_id = request.command.turn_id.clone();
        let tasks_ref = tasks.clone();
        let handoff = frozen.handoff_content;
        let display_text = frozen.display_text;
        let target_conversation = frozen.conversation_id;
        let message_id = frozen.message_id;
        let voice_context_digest = frozen.voice_context_digest;
        let expected_preset = Some(frozen.preset);
        let handle = tokio::spawn(async move {
            match tasks.submit_frozen_with_handoff_queued_with_context(
                request.command,
                config,
                expected_preset,
                handoff,
                display_text,
                target_conversation,
                message_id,
                voice_context_digest,
            ).await {
                Ok(delivery) => { manager.project_task(delivery.task.clone()); Ok(delivery) },
                Err(code) => {
                    if matches!(code.as_str(), "task_creation_unknown" | "task_delivery_unknown") {
                        if let Ok(task) = tasks.status_for(&command_id).await {
                            manager.project_task(task.clone());
                            return Ok(VoiceTaskDelivery { task, freshly_accepted: false });
                        }
                    }
                    if let Ok(task)=tasks.status_for(&command_id).await {manager.project_task(task);}
                    manager.project_task_error(code.clone());
                    Err(code)
                }
            }
        });
        match tokio::time::timeout(std::time::Duration::from_secs(3), handle).await {
            Ok(result) => result.map_err(|_| "task_delivery_unknown".to_string())?,
            Err(_timeout) => {
                let task = tasks_ref.status_for(&command_id_timeout).await.unwrap_or_else(|_| VoiceTaskSnapshot {
                    command_id: command_id_timeout,
                    session_id,
                    acoustic_turn_id: turn_id,
                    conversation_id: None,
                    message_id: None,
                    runtime_turn_id: None,
                    status: VoiceTaskStatus::Unknown,
                    result_ok: None,
                    result_text: None,
                    error_code: Some("task_delivery_pending".into()),
                });
                self.project_task(task.clone());
                Ok(VoiceTaskDelivery { task, freshly_accepted: false })
            }
        }
    }

    async fn submit_keyboard_message(self:&Arc<Self>, request:nomifun_api_types::voice::VoiceKeyboardMessageRequest) -> Result<VoiceTaskDelivery,String> {
        let tasks = self.tasks.clone().ok_or("runtime_unavailable")?;
        let command = nomifun_api_types::voice::VoiceTaskCommand {
            session_id:format!("keyboard-{}",request.conversation_id),turn_id:request.command_id.clone(),
            command_id:request.command_id,goal:request.text,record_id:None,binding_id:None,
        };
        VoiceTaskService::validate_command(&command)?;
        // A retry keeps the original frozen configuration even after a model switch.
        if let Some(delivery) = tasks.replay(&command).await? {return Ok(delivery);}
        let frozen = self.dialogue_recorder.as_ref().ok_or("binding_invalid")?
            .keyboard_configuration(&request.conversation_id).await?;
        let manager = self.clone();
        tokio::spawn(async move {
            let result = tasks.submit_frozen_with_handoff_queued_with_context(command.clone(),frozen.config,Some(frozen.preset),
                Some(command.goal.clone()),Some(command.goal),Some(request.conversation_id),None,None).await;
            if let Ok(Some(task)) = tasks.current_task() {manager.project_task(task);}
            result
        }).await.map_err(|_|"task_delivery_unknown".to_owned())?
    }

    async fn control_task(self:&Arc<Self>,request:VoiceTaskControlRequest,action:VoiceTaskControlAction) -> Result<VoiceTaskSnapshot,String> {
        if action == VoiceTaskControlAction::CancelTask {
            self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        }
        if request.instance_id!=self.snapshot().instance_id {return Err("task_scope_changed".into());}
        let tasks=self.tasks.clone().ok_or("runtime_unavailable")?;
        let current=tasks.task_by_id(&request.task.command_id)?.ok_or("task_not_found")?;
        if !request.task.matches(&current) {return Err("task_scope_changed".into());}
        if action==VoiceTaskControlAction::QueryStatus {
            let task=tasks.status_for(&request.task.command_id).await?;
            self.project_task(task.clone());return Ok(task);
        }
        if !matches!(current.status,VoiceTaskStatus::Completed|VoiceTaskStatus::Failed)
            && (current.conversation_id.is_none() || current.message_id.is_none())
        {return Err("task_delivery_pending".into());}
        let manager=self.clone();
        tokio::spawn(async move {
            let result=tasks.cancel(&request.task.command_id).await;
            if let Ok(task)=tasks.current_task() && let Some(task)=task {manager.project_task(task);}
            if let Err(code)=&result {manager.project_task_error(code.clone());}
            result
        }).await.map_err(|_|"task_control_unknown".to_owned())?
    }

    async fn dialogue_settings(&self) -> VoiceDialogueSettings {
        let effective = match &self.dialogue_completer {
            Some(completer) => completer.resolve_model(None).await.ok(),
            None => None,
        };
        VoiceDialogueSettings {
            config: VoiceDialogueConfig {
                dialogue_model: None,
            },
            available: effective.is_some(),
            effective_model: effective,
        }
    }

    async fn start_dialogue_bridge(self: &Arc<Self>) -> Result<VoiceDialogueBridge, String> {
        VoiceDialogueBridge::start_with_recorder(
            self.dialogue_completer.clone().ok_or("model_unavailable")?,
            Arc::new(RwLock::new(None)),
            self.control_token.clone(),
            self.private_task_routes(),
            self.dialogue_recorder.clone(),
        ).await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<VoiceSessionSnapshot> {
        self.events.subscribe()
    }

    pub(crate) fn private_task_routes(self: &Arc<Self>) -> axum::Router {
        axum::Router::new().route("/tasks", axum::routing::post(private_task_submit).get(private_current_task))
            .route("/task-control",axum::routing::post(private_task_control))
            .with_state(Arc::downgrade(self))
    }

    async fn private_voice_scope(&self, request:&VoiceSessionControlRequest) -> Result<(),String> {
        let source=self.refresh_daemon_snapshot().await?;
        if !request.matches(&source) || !matches!(source.phase,VoiceSessionPhase::Deciding|VoiceSessionPhase::Dispatching)
            || request.session_id.as_ref().is_some_and(|id|self.revoked_sessions.read().unwrap_or_else(|error|error.into_inner()).contains(id))
        {return Err("task_scope_changed".into());}
        Ok(())
    }

    async fn refresh_daemon_snapshot(&self) -> Result<VoiceSessionSnapshot, String> {
        let generation = self.generation.load(Ordering::Acquire);
        let response = reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "daemon_unavailable")?
            .get(format!("{}/api/voice/snapshot", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .timeout(std::time::Duration::from_secs(3)).send().await.map_err(|_| "daemon_unavailable")?;
        if !response.status().is_success() {return Err("daemon_unavailable".into());}
        let source: VoiceSessionSnapshot = response.json().await.map_err(|_| "daemon_unavailable")?;
        if generation != self.generation.load(Ordering::Acquire) { return Err("task_scope_changed".into()); }
        self.project_with_binding(source.clone(), generation).await;
        Ok(source)
    }

    async fn project_with_binding(&self, source: VoiceSessionSnapshot, generation: u64) {
        self.project(source.clone(), generation);
        if generation != self.generation.load(Ordering::Acquire) { return; }
        if let (Some(recorder), Some(session)) = (&self.dialogue_recorder, &source.session_id) {
            if matches!(source.phase, VoiceSessionPhase::Listening | VoiceSessionPhase::Transcribing | VoiceSessionPhase::Deciding)
                && self.snapshot().binding.is_none() {
                match recorder.freeze_session(session, source.call_request_id.as_deref()).await {
                    Ok(Some(binding)) => {
                        let mut current = self.snapshot.write().unwrap_or_else(|e|e.into_inner());
                        if generation == self.generation.load(Ordering::Acquire) && current.session_id.as_ref() == Some(session) {
                            current.binding = Some(binding);
                            current.seq += 1;
                            let _ = self.events.send(current.clone());
                        }
                    },
                    Err(_) => self.project_task_error("binding_invalid".into()),
                    _ => {},
                }
            }
        }
    }

    fn requested_feedback(&self) -> Result<VoiceFeedbackConfig, String> {
        if !self.config_path.exists() {
            return Ok(Default::default());
        }
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&self.config_path).map_err(|_| "configuration_read_failed")?,
        )
        .map_err(|_| "configuration_invalid")?;
        let object = value.as_object().ok_or("configuration_invalid")?;
        let config: VoiceFeedbackConfig = match object.get("feedback") {
            Some(value) => {
                serde_json::from_value(value.clone()).map_err(|_| "configuration_invalid")?
            }
            None => Default::default(),
        };
        config.validate()?;
        Ok(config)
    }

    async fn feedback_rpc(
        &self,
        method: reqwest::Method,
        suffix: &str,
        body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response, String> {
        if self.snapshot().connection != VoiceConnectionState::Connected {
            return Err("daemon_unavailable".into());
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "daemon_unavailable")?;
        let mut request = client
            .request(
                method,
                format!("{}/api/voice/feedback{suffix}",self.daemon_origin),
            )
            .header("x-ai-mate-control-token", &self.control_token)
            .timeout(std::time::Duration::from_secs(3));
        if let Some(body) = body {
            request = request.json(&body);
        }
        request
            .send()
            .await
            .map_err(|_| "daemon_unavailable".into())
    }

    async fn feedback_settings(&self) -> Result<VoiceFeedbackSettings, String> {
        let requested = self.requested_feedback()?;
        let effective = match self.feedback_rpc(reqwest::Method::GET, "", None).await {
            Ok(response) if response.status().is_success() => response.json().await.ok(),
            _ => None,
        };
        Ok(VoiceFeedbackSettings {
            requested,
            effective,
        })
    }

    async fn update_feedback(
        &self,
        patch: VoiceFeedbackPatch,
    ) -> Result<VoiceFeedbackSettings, String> {
        let _guard = self.feedback_update_lock.lock().await;
        let requested = patch.apply(self.requested_feedback()?);
        requested.validate()?;
        let value = serde_json::to_value(requested).map_err(|_| "configuration_invalid")?;
        self.write_config_entry("feedback", value.clone())?;
        let effective = match self
            .feedback_rpc(reqwest::Method::POST, "", Some(value))
            .await
        {
            Ok(response) if response.status().is_success() => response.json().await.ok(),
            _ => None,
        };
        Ok(VoiceFeedbackSettings {
            requested,
            effective,
        })
    }

    #[cfg(test)]
    pub(crate) fn replay_acoustic_source(&self, snapshot: VoiceSessionSnapshot) {
        self.projection_started.store(true, Ordering::Release);
        self.project(snapshot, self.generation.load(Ordering::Acquire));
    }

    #[cfg(test)]
    #[cfg(test)]
    fn with_test_daemon_port(mut self, port: u16) -> Self {
        self.daemon_origin = format!("http://127.0.0.1:{port}");
        self.test_daemon_port = Some(port);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_replay_daemon(mut self, origin: String) -> Self {
        self.daemon_origin = origin;
        self.projection_started.store(true, Ordering::Release);
        self
    }

    #[cfg(test)]
    pub(crate) fn replay_control_credential(&self) -> String { self.control_token.clone() }

    #[cfg(test)]
    pub(crate) fn replay_task_projection(&self, task: VoiceTaskSnapshot) {self.project_task(task);}

    #[doc(hidden)]
    pub fn project_snapshot_for_test(&self, source_snapshot: VoiceSessionSnapshot, generation: u64) {
        self.project(source_snapshot, generation);
    }

    #[doc(hidden)]
    pub fn project_task_for_test(&self, task: VoiceTaskSnapshot) {
        self.project_task(task);
    }

    fn project(&self, mut source_snapshot: VoiceSessionSnapshot, generation: u64) {
        if generation != self.generation.load(Ordering::Acquire) {
            return;
        }
        let mut source = self.source.write().unwrap_or_else(|err| err.into_inner());
        if source.as_ref().is_some_and(|(id, seq)| {
            *id == source_snapshot.instance_id
                && (*seq > source_snapshot.seq
                    || (*seq == source_snapshot.seq
                        && self.snapshot().connection == VoiceConnectionState::Connected))
        }) {
            return;
        }
        *source = Some((source_snapshot.instance_id.clone(), source_snapshot.seq));
        if source_snapshot.ended_reason.is_some() {
            self.mark_active_request_stopped(source_snapshot.session_id.as_deref(), source_snapshot.call_request_id.as_deref());
        }
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        source_snapshot.instance_id = snapshot.instance_id.clone();
        source_snapshot.seq = snapshot.seq + 1;
        source_snapshot.enabled = self.read_autostart_config();
        source_snapshot.connection = VoiceConnectionState::Connected;
        source_snapshot.task = snapshot.task.clone();
        source_snapshot.queue = snapshot.queue.clone();
        source_snapshot.binding = snapshot.binding.clone().filter(|binding|Some(&binding.session_id)==source_snapshot.session_id.as_ref());
        source_snapshot.task_progress = snapshot.task_progress.clone();
        source_snapshot.task_notification = snapshot.task_notification.clone();
        source_snapshot.confirmation=snapshot.confirmation.clone();
        source_snapshot.task_ready = snapshot.task_ready;
        source_snapshot.task_error_code = snapshot.task_error_code.clone();
        source_snapshot.conversation.retain(|turn| turn.session_id.is_some() && turn.session_id == source_snapshot.session_id);
        source_snapshot.projected_turn_ids.clear();
        if let Some(binding) = &source_snapshot.binding {
            for turn in &mut source_snapshot.conversation {
                if let Some(recorder) = &self.dialogue_recorder {
                    if recorder.is_projected(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                        source_snapshot.projected_turn_ids.push(turn.turn_id.clone());
                    }
                    if let Some((user, assistant)) = recorder.projected_message_ids(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                        turn.user_message_id = user;
                        turn.assistant_message_id = Some(assistant);
                    }
                    if turn.error.is_none()
                        && let Some(error) = recorder.turn_error(&binding.session_id, &binding.conversation_id, &turn.turn_id) {
                        turn.error = Some(error);
                    }
                }
            }
        }
        // Fresh daemon audio snapshots must not erase the last readable reply.
        // Session/turn identities still come exclusively from the new daemon.
        if source_snapshot.session_id.is_none() && source_snapshot.conversation.is_empty() {
            source_snapshot.conversation=snapshot.conversation.clone();
            source_snapshot.user_text=snapshot.user_text.clone();
            source_snapshot.understood_request=snapshot.understood_request.clone();
            source_snapshot.assistant_text=snapshot.assistant_text.clone();
        }
        *snapshot = source_snapshot;
        let _ = self.events.send(snapshot.clone());
    }

    fn disconnected(&self) {
        self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        let enabled = self.read_autostart_config();
        let connection = if enabled {
            VoiceConnectionState::Offline
        } else {
            VoiceConnectionState::Stopped
        };
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        if snapshot.enabled == enabled && snapshot.connection == connection {
            return;
        }
        snapshot.enabled = enabled;
        snapshot.connection = connection;
        snapshot.acoustic_ready = false;
        snapshot.dialogue_ready = false;
        snapshot.playback_ready = false;
        snapshot.audio = None;
        snapshot.phase = if enabled {
            VoiceSessionPhase::Error
        } else {
            VoiceSessionPhase::Idle
        };
        snapshot.error = enabled.then(|| VoiceSessionError {
            stage: "connection".into(),
            code: "daemon_unavailable".into(),
        });
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    fn mark_daemon_running(&self, now: tokio::time::Instant) {
        let mut budget = self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(since) = budget.running_since else {
            budget.running_since = Some(now);
            budget.next_attempt = None;
            return;
        };
        if now.duration_since(since) >= std::time::Duration::from_secs(60) {
            *budget = RestartBudget {
                running_since: Some(now),
                ..RestartBudget::default()
            };
        }
    }

    fn restart_decision(&self, now: tokio::time::Instant) -> RestartDecision {
        let mut budget = self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        budget.running_since = None;
        if let Some(at) = budget.next_attempt {
            if now < at {
                return RestartDecision::Wait(at.duration_since(now));
            }
            budget.next_attempt = None;
            return RestartDecision::StartNow;
        }
        if budget.attempts >= RestartBudget::MAX_ATTEMPTS {
            return RestartDecision::Exhausted;
        }
        let delay = RestartBudget::delay(budget.attempts);
        budget.attempts += 1;
        budget.next_attempt = Some(now + delay);
        RestartDecision::Wait(delay)
    }

    fn note_restart_exhausted(&self) -> bool {
        let mut budget = self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if budget.exhausted_reported {
            return false;
        }
        budget.exhausted_reported = true;
        true
    }

    fn reset_restart_budget(&self) {
        *self
            .restart_budget
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = RestartBudget::default();
    }

    fn starting(&self) {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        snapshot.enabled = self.read_autostart_config();
        snapshot.connection = VoiceConnectionState::Starting;
        snapshot.phase = VoiceSessionPhase::Idle;
        snapshot.error = None;
        snapshot.acoustic_ready = false;
        snapshot.dialogue_ready = false;
        snapshot.playback_ready = false;
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    /// Surface a missing packaged component as its own state instead of a
    /// generic daemon connection failure. Repeats are coalesced.
    fn note_missing_voice_binary(&self) {
        let mut snapshot = self.snapshot.write().unwrap_or_else(|err| err.into_inner());
        if snapshot
            .error
            .as_ref()
            .is_some_and(|error| error.code == VOICE_COMPANION_MISSING)
        {
            return;
        }
        snapshot.enabled = self.read_autostart_config();
        snapshot.connection = VoiceConnectionState::Offline;
        snapshot.phase = VoiceSessionPhase::Error;
        snapshot.error = Some(VoiceSessionError {
            stage: "connection".into(),
            code: VOICE_COMPANION_MISSING.into(),
        });
        snapshot.acoustic_ready = false;
        snapshot.dialogue_ready = false;
        snapshot.playback_ready = false;
        snapshot.seq += 1;
        let _ = self.events.send(snapshot.clone());
    }

    /// One upstream stream per host, independent of any renderer/page lifecycle.
    fn start_projection(self: &Arc<Self>) {
        self.install_call_delete_guard();
        if self.projection_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let manager = self.clone();
        tokio::spawn(async move {
            use futures_util::StreamExt;
            let client = reqwest::Client::builder()
                .no_proxy().redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(2))
                .build()
                .expect("fixed loopback client configuration");
            loop {
                if manager.shutdown_requested.load(Ordering::Acquire) {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    continue;
                }
                let status = manager.status().await;
                if !status.running {
                    manager.disconnected();
                    if manager.shutdown_requested.load(Ordering::Acquire)
                        || !manager.read_autostart_config()
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                    // Missing assets are not a crash loop: do not consume the
                    // restart budget while the suite still needs installation.
                    if !status.models_ready {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    // A missing installer component is also not a crash loop.
                    // Report it once with an actionable code instead of letting
                    // reconnects masquerade as a daemon restart failure.
                    if Self::resolve_voice_companion_binary().is_none() {
                        manager.note_missing_voice_binary();
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    match manager.restart_decision(tokio::time::Instant::now()) {
                        RestartDecision::Wait(delay) => {
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                        RestartDecision::Exhausted => {
                            if manager.note_restart_exhausted() {
                                let mut snapshot = manager
                                    .snapshot
                                    .write()
                                    .unwrap_or_else(|err| err.into_inner());
                                snapshot.error = Some(VoiceSessionError {
                                    stage: "connection".into(),
                                    code: "daemon_restart_exhausted".into(),
                                });
                                snapshot.seq += 1;
                                let _ = manager.events.send(snapshot.clone());
                                warn!(
                                    "Voice companion restart budget exhausted; awaiting explicit retry"
                                );
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                        RestartDecision::StartNow => {
                            if let Err(error) = manager.start_child(false).await {
                                warn!(%error, "Voice companion automatic restart failed");
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            continue;
                        }
                    }
                }
                manager.mark_daemon_running(tokio::time::Instant::now());
                let generation = manager.generation.load(Ordering::Acquire);
                let response = client
                    .get(format!("{}/api/voice/events",manager.daemon_origin))
                    .header("x-ai-mate-control-token", &manager.control_token)
                    .send()
                    .await;
                if let Ok(response) = response
                    && response.status().is_success()
                {
                    let mut stream = response.bytes_stream();
                    let mut buffer = Vec::new();
                    let mut recovery_tick=tokio::time::interval(std::time::Duration::from_secs(1));
                    recovery_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    let mut admission_ticks = 0u8;
                    loop {
                        let chunk = tokio::select! {
                            biased;
                            _ = manager.process_changed.notified() => break,
                            _ = recovery_tick.tick() => {
                                if manager.shutdown_requested.load(Ordering::Acquire)
                                    || generation!=manager.generation.load(Ordering::Acquire)
                                    || manager.recover_audio_if_needed().await {break;}
                                if let Err(error) = manager.reconcile_empty_placeholders().await {
                                    tracing::debug!(%error, "voice placeholder cleanup deferred");
                                }
                                admission_ticks = admission_ticks.wrapping_add(1);
                                if admission_ticks % 2 == 0 {
                                    manager.refresh_wake_admission().await;
                                }
                                continue;
                            },
                            next = stream.next() => next,
                        };
                        let Some(Ok(chunk)) = chunk else {
                            break;
                        };
                        buffer.extend_from_slice(&chunk);
                        if buffer.len() > 262_144 {
                            break;
                        }
                        while let Some(end) = buffer.windows(2).position(|bytes| bytes == b"\n\n") {
                            let frame: Vec<_> = buffer.drain(..end + 2).collect();
                            if let Ok(frame) = std::str::from_utf8(&frame) {
                                let data = frame
                                    .lines()
                                    .filter_map(|line| line.strip_prefix("data:"))
                                    .map(str::trim)
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if let Ok(VoiceCompanionEvent::Snapshot { snapshot }) =
                                    serde_json::from_str(&data)
                                {
                                    manager.project_with_binding(snapshot, generation).await;
                                }
                            }
                        }
                    }
                }
                manager.disconnected();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        });
    }

    pub async fn stop_session(&self, request: VoiceSessionControlRequest) -> Result<(), String> {
        let _start_lock = self.call_start_lock.lock().await;
        let snap = self.snapshot();
        if !request.matches(&snap) || request.session_id.is_none()
            || snap.ended_reason.is_some() || snap.phase == VoiceSessionPhase::Idle {
            return Err("session_scope_changed".into());
        }
        if let Some(recorder) = &self.dialogue_recorder {
            if recorder.current_call_reservation().is_some_and(|reservation| {
                Some(reservation.request_id.as_str()) != snap.call_request_id.as_deref()
            }) { return Err("session_scope_changed".into()); }
        }
        self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        if let Some(session_id) = &request.session_id {
            let mut revoked = self.revoked_sessions.write().unwrap_or_else(|error| error.into_inner());
            if !revoked.contains(session_id) {
                revoked.push_back(session_id.clone());
            }
            if revoked.len() > 64 {
                revoked.pop_front();
            }
        }
        let source = self
            .source
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .ok_or_else(|| "daemon_unavailable".to_string())?;
        let request = VoiceSessionControlRequest {
            instance_id: source.0,
            ..request
        };
        let response = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "daemon_unavailable".to_owned())?
            .post(format!("{}/api/voice/stop", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .json(&request)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|_| "daemon_unavailable".to_string())?;
        if response.status().is_success() {
            self.mark_active_request_stopped(request.session_id.as_deref(), snap.call_request_id.as_deref());
            self.refresh_daemon_snapshot().await?;
            Ok(())
        } else {
            Err("session_scope_changed".into())
        }
    }

    async fn reconcile_empty_placeholders(&self) -> Result<(), String> {
        let _guard = self.call_start_lock.lock().await;
        let snapshot = self.snapshot();
        if snapshot.connection != VoiceConnectionState::Connected { return Ok(()); }
        let Some(recorder) = &self.dialogue_recorder else { return Ok(()); };
        let mut protected: Vec<String> = match &self.tasks {
            Some(tasks) => tasks.queue_projection()?.into_iter().filter_map(|item| item.task.conversation_id).collect(),
            None => Vec::new(),
        };
        if let Some(reservation) = recorder.current_call_reservation() { protected.push(reservation.conversation_id); }
        if let Some(binding) = &snapshot.binding {
            if snapshot.ended_reason.is_none() { protected.push(binding.conversation_id.clone()); }
            else if snapshot.session_id.as_deref() == Some(&binding.session_id)
                && let Some(request_id) = snapshot.call_request_id.as_deref() {
                recorder.finish_call_placeholder(&binding.conversation_id, request_id).await.map_err(|e| e.to_string())?;
            }
        }
        let (view_sequence, selected) = self.viewed_conversation.read().unwrap_or_else(|e| e.into_inner()).clone();
        if view_sequence == 0 { return Ok(()); }
        recorder.cleanup_ended_placeholders(selected.as_deref(), &protected).await.map_err(|e| e.to_string())
    }

    pub async fn start_call(
        &self,
        request: nomifun_api_types::voice::VoiceCallStartRequest,
    ) -> Result<nomifun_api_types::voice::VoiceCallStartResponse, (axum::http::StatusCode, String)> {
        let _start_lock = self.call_start_lock.lock().await;
        if self.resource_write_active.load(Ordering::Acquire) {
            return Err((
                axum::http::StatusCode::CONFLICT,
                serde_json::json!({
                    "code": "voice_resource_write_in_progress",
                    "message": "Voice models are being installed or removed; wait for the write to finish"
                })
                .to_string(),
            ));
        }
        if self.self_check_active.load(Ordering::Acquire) {
            return Err((
                axum::http::StatusCode::CONFLICT,
                serde_json::json!({
                    "code": "voice_self_check_in_progress",
                    "reason_code": "self_check_in_progress",
                    "requires_check": false,
                    "message": "A readiness check is using the acoustic suite; wait for it to finish"
                })
                .to_string(),
            ));
        }

        let status = self.status().await;
        if !status.running {
            return Err((
                axum::http::StatusCode::BAD_REQUEST,
                "voice_companion_not_running".to_string(),
            ));
        }

        let recorder = self.dialogue_recorder.as_ref().ok_or_else(|| {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "dialogue_recorder_unavailable".to_string(),
            )
        })?;

        let snap = self.snapshot();
        let is_active = snap.session_id.is_some()
            && snap.ended_reason.is_none()
            && snap.phase != VoiceSessionPhase::Idle;

        // Check idempotency against handled_start_requests cache
        let existing_outcome = {
            self.handled_start_requests
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(&request.request_id)
                .cloned()
        };
        if let Some(existing) = existing_outcome {
            if existing.target() != &request.target || existing.session_mode() != &request.session_mode {
                return Err((
                    axum::http::StatusCode::BAD_REQUEST,
                    serde_json::json!({
                        "code": "request_target_conflict",
                        "message": "Cannot change target for the same request_id"
                    })
                    .to_string(),
                ));
            }
            match existing {
                HandledStartOutcome::Starting {
                    conversation_id,
                    memory_owner,
                    reserved_at_epoch_ms,
                    ..
                } => {
                    let now = chrono::Utc::now().timestamp_millis();
                    if snap.session_id.is_some()
                        && snap.call_request_id.as_deref() == Some(request.request_id.as_str())
                        && is_active
                        && snap.binding.as_ref().map(|b| &b.conversation_id) == Some(&conversation_id)
                    {
                        self.mark_request_bound(
                            &request.request_id,
                            snap.session_id.as_deref().unwrap_or(""),
                            &conversation_id,
                            memory_owner.as_deref(),
                        );
                        return Ok(nomifun_api_types::voice::VoiceCallStartResponse {
                            request_id: request.request_id,
                            status: if snap.acoustic_ready {
                                nomifun_api_types::voice::VoiceCallStartStatus::Ready
                            } else {
                                nomifun_api_types::voice::VoiceCallStartStatus::Active
                            },
                            conversation_id,
                            session_id: snap.session_id,
                            memory_owner_companion_id: memory_owner,
                            binding: snap.binding,
                        });
                    }
                    if now - reserved_at_epoch_ms < 10_000 {
                        return Ok(nomifun_api_types::voice::VoiceCallStartResponse {
                            request_id: request.request_id,
                            status: nomifun_api_types::voice::VoiceCallStartStatus::Starting,
                            conversation_id,
                            session_id: snap.session_id,
                            memory_owner_companion_id: memory_owner,
                            binding: snap.binding,
                        });
                    }
                    return Err((axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        serde_json::json!({"code":"voice_call_start_unknown", "conversation_id":conversation_id}).to_string()));
                }
                HandledStartOutcome::Bound {
                    conversation_id,
                    session_id,
                    memory_owner,
                    ..
                } => {
                    if !is_active || snap.session_id != session_id || snap.call_request_id.as_deref() != Some(request.request_id.as_str()) {
                        return Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, serde_json::json!({"code":"voice_call_start_unknown"}).to_string()));
                    }
                    return Ok(nomifun_api_types::voice::VoiceCallStartResponse {
                        request_id: request.request_id,
                        status: if snap.acoustic_ready
                            && snap.binding.as_ref().map(|b| &b.conversation_id) == Some(&conversation_id)
                        {
                            nomifun_api_types::voice::VoiceCallStartStatus::Ready
                        } else {
                            nomifun_api_types::voice::VoiceCallStartStatus::Active
                        },
                        conversation_id,
                        session_id: snap.session_id.or(session_id),
                        memory_owner_companion_id: memory_owner,
                        binding: snap.binding,
                    });
                }
                HandledStartOutcome::Stopped { conversation_id, .. } => {
                    return Err((
                        axum::http::StatusCode::CONFLICT,
                        serde_json::json!({
                            "code": "voice_call_already_stopped",
                            "active_conversation_id": Some(conversation_id),
                            "message": "This request has already concluded and stopped"
                        })
                        .to_string(),
                    ));
                }
                HandledStartOutcome::Rejected {
                    status_code_u16,
                    error_json,
                    ..
                } => {
                    let status = axum::http::StatusCode::from_u16(status_code_u16)
                        .unwrap_or(axum::http::StatusCode::BAD_REQUEST);
                    return Err((status, error_json));
                }
            }
        }

        // Check if an existing reservation from another request is pending
        if let Some(res) = recorder.current_call_reservation() {
            let elapsed_ms = chrono::Utc::now().timestamp_millis() - res.reserved_at_epoch_ms;
            if elapsed_ms < 10_000 {
                if res.request_id != request.request_id && !is_active {
                    return Err((
                        axum::http::StatusCode::CONFLICT,
                        serde_json::json!({
                            "code": "voice_call_starting",
                            "active_conversation_id": Some(res.conversation_id),
                            "message": "A voice call is currently starting"
                        })
                        .to_string(),
                    ));
                }
            } else {
                recorder.take_call_reservation(&res.request_id);
            }
        }

        // Resolving a target may create a Conversation; arbitrate active calls first.
        if is_active {
            let active_id = snap.binding.as_ref().map(|binding| binding.conversation_id.as_str());
            let same_target = match &request.target {
                VoiceCallTarget::VoiceConversation { conversation_id } => Some(conversation_id.as_str()) == active_id,
                VoiceCallTarget::Workspace { workspace } => snap.binding.as_ref().and_then(|binding| binding.workspace.as_deref())
                    .is_some_and(|active| nomifun_common::paths::paths_equivalent(Path::new(workspace), Path::new(active))),
                VoiceCallTarget::CurrentSelection => active_id.is_some(),
                VoiceCallTarget::DefaultWorkspace => false,
                VoiceCallTarget::NewVoiceConversation { .. } => false,
            };
            if same_target && active_id.is_some() {
                return Ok(VoiceCallStartResponse {
                    request_id: request.request_id,
                    status: VoiceCallStartStatus::Active,
                    conversation_id: active_id.unwrap().to_owned(),
                    session_id: snap.session_id,
                    memory_owner_companion_id: snap.binding.as_ref().and_then(|b| b.memory_owner_companion_id.clone()),
                    binding: snap.binding,
                });
            }
            return Err((axum::http::StatusCode::CONFLICT, serde_json::json!({
                "code":"voice_call_conflict", "active_conversation_id":active_id,
            }).to_string()));
        }

        // A connected daemon authorizes a new full call only with a current,
        // attributed self-check report. Missing/expired/old reports reject
        // before any Conversation placeholder is created.
        if snap.enabled {
            let capabilities = self.current_capabilities().await;
            let can_start = capabilities
                .as_ref()
                .is_some_and(|report| report.can_start_full_call);
            if !can_start {
                let reason_code = capabilities
                    .as_ref()
                    .and_then(|report| report.reason_code.clone())
                    .unwrap_or_else(|| "self_check_required".into());
                let degraded = capabilities
                    .as_ref()
                    .is_some_and(|report| report.degraded_text_only);
                return Err((
                    axum::http::StatusCode::CONFLICT,
                    serde_json::json!({
                        "code": "voice_suite_not_ready",
                        "reason_code": reason_code,
                        "requires_check": capabilities.is_none(),
                        "degraded_text_only": degraded,
                        "capabilities": capabilities,
                        "message": "The voice suite has not passed its current local capability checks for a full call"
                    })
                    .to_string(),
                ));
            }
        }

        let (conversation_id, memory_owner) = recorder
            .resolve_call_target_with_locale(&request.target, request.session_mode.as_deref(), Some(&request.request_id), request.locale.as_deref())
            .await
            .map_err(|e| match e {
                VoiceDialogueRecorderError::WriteFailed(msg) if msg.contains("workspace_directory_invalid") => {
                    (axum::http::StatusCode::BAD_REQUEST, "workspace_directory_invalid".to_string())
                }
                VoiceDialogueRecorderError::BindingInvalid(msg) if matches!(msg.as_str(), "conversation_already_discarded" | "voice_call_already_stopped" | "voice_start_request_conflict") => {
                    (axum::http::StatusCode::CONFLICT, serde_json::json!({
                        "code": msg,
                    }).to_string())
                }
                VoiceDialogueRecorderError::BindingInvalid(msg) => {
                    (axum::http::StatusCode::BAD_REQUEST, msg)
                }
                _ => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "target_resolution_failed".to_string()),
            })?;

        let now_epoch_ms = chrono::Utc::now().timestamp_millis();
        let reservation = crate::voice_dialogue::VoiceCallReservation {
            request_id: request.request_id.clone(),
            target: request.target.clone(),
            conversation_id: conversation_id.clone(),
            memory_owner: memory_owner.clone(),
            reserved_at_epoch_ms: now_epoch_ms,
        };
        recorder.set_call_reservation(Some(reservation));
        self.set_candidate_conversation(Some(conversation_id.clone()));
        self.record_handled_start_request(
            request.request_id.clone(),
            HandledStartOutcome::Starting {
                target: request.target.clone(),
                session_mode: request.session_mode.clone(),
                conversation_id: conversation_id.clone(),
                memory_owner: memory_owner.clone(),
                reserved_at_epoch_ms: now_epoch_ms,
            },
        );

        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                recorder.take_call_reservation(&request.request_id);
                (axum::http::StatusCode::SERVICE_UNAVAILABLE, e.to_string())
            })?;

        let wake_result = client
            .post(format!("{}/api/voice/wake", self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .timeout(std::time::Duration::from_secs(3))
            .json(&{
                let mut body = serde_json::json!({ "request_id": request.request_id });
                if let Some(locale) = &request.locale {
                    body["locale"] = serde_json::Value::String(locale.clone());
                }
                body
            })
            .send()
            .await;

        let resp = match wake_result {
            Ok(r) => r,
            Err(e) => {
                // Unknown transport result: wake may have been delivered to daemon before network drop.
                // Do NOT immediately discard reservation; keep it alive for late binding / idempotent retry.
                return Err((
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    serde_json::json!({
                        "code": "daemon_wake_timeout",
                        "conversation_id": Some(&conversation_id),
                        "message": format!("Failed to receive wake confirmation from voice daemon: {e}. Reservation preserved for retry.")
                    })
                    .to_string(),
                ));
            }
        };

        let resp_status = resp.status();
        if !resp_status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            if !wake_rejection_precedes_session(resp_status, &body_text) {
                return Err((axum::http::StatusCode::SERVICE_UNAVAILABLE, serde_json::json!({
                    "code": "daemon_wake_unknown",
                    "conversation_id": conversation_id,
                    "message": "Wake outcome is unknown; reservation preserved for retry"
                }).to_string()));
            }
            // Only the daemon's explicit pre-wake failures prove there is no session.
            recorder.take_call_reservation(&request.request_id);
            let error_json = serde_json::json!({
                "code": "daemon_wake_rejected",
                "status": resp_status.as_u16(),
                "request_id": request.request_id,
                "terminal": true,
                "message": format!("Daemon rejected wake request: {body_text}"),
            }).to_string();
            self.record_handled_start_request(
                request.request_id.clone(),
                HandledStartOutcome::Rejected {
                    target: request.target.clone(),
                    session_mode: request.session_mode.clone(),
                    status_code_u16: resp_status.as_u16(),
                    error_json: error_json.clone(),
                    at_epoch_ms: chrono::Utc::now().timestamp_millis(),
                },
            );
            return Err((resp_status, error_json));
        }

        Ok(nomifun_api_types::voice::VoiceCallStartResponse {
            request_id: request.request_id,
            status: nomifun_api_types::voice::VoiceCallStartStatus::Starting,
            conversation_id,
            session_id: None,
            memory_owner_companion_id: memory_owner,
            binding: None,
        })
    }

    /// Resolves the compiled `voice-companion` executable path on disk (with fallback to `ai-mate`).
    pub fn resolve_voice_companion_binary() -> Option<PathBuf> {
        let exe_names = if cfg!(windows) {
            ["voice-companion.exe", "ai-mate.exe"]
        } else {
            ["voice-companion", "ai-mate"]
        };

        // 1. Explicit environment variable override
        if let Some(env_path) = std::env::var_os("VOICE_COMPANION_BIN")
            .or_else(|| std::env::var_os("AI_MATE_BIN"))
            .map(PathBuf::from)
        {
            if env_path.is_file() {
                return Some(env_path);
            }
        }

        // 2. Tauri's resource bin is authoritative across Windows/macOS/Linux.
        let bundled_bin=std::env::var_os("NOMIFUN_BUNDLED_BIN_DIR").map(PathBuf::from);
        let current_exe=std::env::current_exe().ok();
        if let Some(binary)=Self::installed_voice_companion_binary(bundled_bin.as_deref(),current_exe.as_deref()) {return Some(binary);}

        // 3. Known development build target directories (prefer newest binary)
        for exe_name in exe_names {
            let mut candidates = vec![
                Path::new("target/debug").join(exe_name),
                Path::new("target/release").join(exe_name),
                Path::new("build.noindex/debug").join(exe_name),
                Path::new("build.noindex/release").join(exe_name),
            ];

            // 4. Resolve relative to workspace root if cwd is a sub-crate
            if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
                if let Some(r) = Path::new(&manifest_dir).ancestors().nth(2) {
                    for dir in [
                        "target/debug",
                        "target/release",
                        "build.noindex/debug",
                        "build.noindex/release",
                    ] {
                        candidates.push(r.join(dir).join(exe_name));
                    }
                }
            }

            let mut existing: Vec<PathBuf> = candidates.into_iter().filter(|c| c.is_file()).collect();
            existing.sort_by_key(|p| {
                std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            });
            if let Some(newest) = existing.pop() {
                let resolved = std::fs::canonicalize(&newest).unwrap_or(newest);
                return Some(resolved);
            }
        }

        None
    }

    /// Backwards-compatible alias for resolving the companion binary.
    #[inline]
    pub fn resolve_ai_mate_binary() -> Option<PathBuf> {
        Self::resolve_voice_companion_binary()
    }

    fn installed_voice_companion_binary(bundled_bin:Option<&Path>,current_exe:Option<&Path>)->Option<PathBuf> {
        let names = if cfg!(windows) {
            ["voice-companion.exe", "ai-mate.exe"]
        } else {
            ["voice-companion", "ai-mate"]
        };
        for name in names {
            if let Some(candidate) = bundled_bin.into_iter().chain(current_exe.and_then(Path::parent))
                .map(|directory| directory.join(name)).find(|candidate| candidate.is_file()) {
                return Some(candidate);
            }
        }
        None
    }

    #[inline]
    #[cfg(test)]
    fn installed_ai_mate_binary(bundled_bin:Option<&Path>,current_exe:Option<&Path>)->Option<PathBuf> {
        Self::installed_voice_companion_binary(bundled_bin, current_exe)
    }

    fn read_autostart_config(&self) -> bool {
        if self.config_path.is_file() {
            if let Ok(raw) = std::fs::read_to_string(&self.config_path) {
                if let Ok(cfg) = serde_json::from_str::<VoiceCompanionPersistedConfig>(&raw) {
                    return cfg.autostart;
                }
            }
        }
        false
    }

    fn write_autostart_config(&self, autostart: bool) -> Result<(), String> {
        self.write_config_entry("autostart", autostart.into())
    }

    fn write_config_entry(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
        let _guard = self
            .config_write_lock
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut cfg = if self.config_path.exists() {
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(&self.config_path)
                    .map_err(|_| "configuration_read_failed".to_string())?,
            )
            .map_err(|_| "configuration_invalid".to_string())?
        } else {
            serde_json::json!({})
        };
        let object = cfg
            .as_object_mut()
            .ok_or_else(|| "configuration_invalid".to_string())?;
        object.insert(key.into(), value);
        let bytes =
            serde_json::to_vec_pretty(&cfg).map_err(|_| "configuration_invalid".to_string())?;
        nomifun_common::dir_config::write_atomic_replace(&self.config_path, &bytes)
            .map_err(|_| "configuration_write_failed".to_string())
    }

    /// Queries the real running status of the background companion daemon.
    #[cfg(test)]
    pub(crate) fn last_child_exit(&self) -> Option<ChildExitObservation> {
        self.last_child_exit
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub async fn status(&self) -> VoiceCompanionStatus {
        let models_ready = self.models_manager.get_status().await.state.is_ready();
        let autostart = self.read_autostart_config();

        let mut lock = self.child.lock().await;
        let mut running = false;
        let mut pid = None;

        if let Some(ref mut child) = *lock {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    #[cfg(test)]
                    {
                        *self.last_child_exit.lock().unwrap_or_else(|error| error.into_inner()) = Some(ChildExitObservation {
                            pid: self.test_child_pid.load(Ordering::Acquire),
                            code: exit_status.code(),
                            observed_at_ms: nomifun_common::now_ms(),
                        });
                    }
                    info!(
                        exit_code = exit_status.code(),
                        ?exit_status,
                        "Voice companion child process has terminated"
                    );
                    *lock = None;
                }
                Ok(None) => {
                    running = true;
                    pid = child.id();
                }
                Err(err) => {
                    warn!(error = %err, "Failed to check status of voice companion process");
                    *lock = None;
                }
            }
        }

        drop(lock);
        let capabilities = self.current_capabilities().await;
        VoiceCompanionStatus {
            running,
            pid,
            models_ready,
            autostart,
            error: self.snapshot().error.map(|error| error.code),
            acoustic_ready: running && self.snapshot().acoustic_ready,
            dialogue_ready: running && self.snapshot().dialogue_ready,
            playback_ready: running && self.snapshot().playback_ready,
            task_ready: self.snapshot().task_ready,
            capabilities,
        }
    }

    /// Launches the `ai-mate` companion daemon under whole-tree supervision.
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        self.shutdown_requested.store(false, Ordering::Release);
        self.reset_restart_budget();
        self.start_child(true).await
    }

    async fn start_child(self:&Arc<Self>, explicit:bool)->Result<(),String> {
        let mut lock = self.child.lock().await;
        if !explicit && (self.shutdown_requested.load(Ordering::Acquire) || !self.read_autostart_config()) {
            return Err("voice_stopped".into());
        }

        // Check if already active
        if let Some(ref mut child) = *lock {
            if let Ok(None) = child.try_wait() {
                info!(
                    pid = child.id(),
                    "Voice companion process is already running"
                );
                return Ok(());
            }
        }

        // Verify models are Ready before attempting launch
        let models_ready = self.models_manager.get_status().await.state.is_ready();
        if !models_ready {
            return Err("Voice acoustic models are not installed yet".to_string());
        }

        let bin_path = match Self::resolve_voice_companion_binary() {
            Some(path) => path,
            None => {
                warn!(
                    bundled_bin = ?std::env::var_os("NOMIFUN_BUNDLED_BIN_DIR"),
                    current_exe = ?std::env::current_exe().ok(),
                    "voice_companion_binary_missing; the installation is missing the voice companion component"
                );
                return Err(VOICE_COMPANION_MISSING.into());
            }
        };

        let feedback_config = serde_json::to_string(&self.requested_feedback()?)
            .map_err(|_| "configuration_invalid")?;
        info!(binary = %bin_path.display(), "Spawning Voice Companion daemon...");
        self.starting();

        let mut builder = ChildProcessBuilder::new(&bin_path);
        builder.arg("--bypass-pairing");
        #[cfg(test)]
        let host = self.test_daemon_port.map(|port| format!("127.0.0.1:{port}")).unwrap_or_else(|| "127.0.0.1:18080".into());
        #[cfg(not(test))]
        let host = "127.0.0.1:18080".to_string();
        builder.arg("--host").arg(host);
        builder.env("VOICE_COMPANION_MODEL_DIR", self.models_manager.models_dir());
        builder.env("AI_MATE_MODEL_DIR", self.models_manager.models_dir());
        builder.env("VOICE_COMPANION_SUPERVISED", "1");
        #[cfg(test)]
        for key in ["NOMIFUN_VOICE_FAULT_BLOCK_ASR", "NOMIFUN_VOICE_FAULT_HEADLESS", "VOICE_COMPANION_NATIVE_STUCK_GRACE_MS", "NOMIFUN_VOICE_NATIVE_LEASE_GRACE_MS", "NOMIFUN_VOICE_SELF_CHECK_AUDIO_DIR"] {
            if let Some(value) = std::env::var_os(key) { builder.env(key, value); }
        }
        builder.env("VOICE_COMPANION_CONTROL_TOKEN", &self.control_token);
        builder.env("AI_MATE_CONTROL_TOKEN", &self.control_token);
        builder.env("VOICE_COMPANION_FEEDBACK_CONFIG", &feedback_config);
        builder.env("AI_MATE_FEEDBACK_CONFIG", feedback_config);
        if self.dialogue_completer.is_some() {
            let mut bridge = self.dialogue_bridge.lock().await;
            if bridge.is_none() {
                *bridge = Some(
                    self.start_dialogue_bridge()
                    .await?,
                );
            }
            builder.env(
                "AI_MATE_DIALOGUE_ENDPOINT",
                &bridge.as_ref().unwrap().endpoint,
            );
            builder.env("AI_MATE_TASK_ENDPOINT", bridge.as_ref().unwrap().endpoint.replace("/complete", "/tasks"));
        }
        // Daemon tracing writes stdout. Keep it in the actual host log root
        // rather than discarding the diagnostics needed for microphone/ASR triage.
        let log_root = std::env::var_os("NOMIFUN_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.config_path
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(Path::new("."))
                    .join("logs")
            });
        let log_dir = log_root.join("voice");
        std::fs::create_dir_all(&log_dir)
            .map_err(|err| format!("Cannot create voice log directory: {err}"))?;
        let log_path = log_dir.join(format!("ai-mate-{}.log", nomifun_common::now_ms()));
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|err| format!("Cannot open voice log: {err}"))?;
        let stderr = log
            .try_clone()
            .map_err(|err| format!("Cannot clone voice log handle: {err}"))?;
        info!(path = %log_path.display(), "Voice companion diagnostics log");
        builder.stdout(std::process::Stdio::from(log));
        builder.stderr(std::process::Stdio::from(stderr));

        if self.shutdown_requested.load(Ordering::Acquire) || (!explicit && !self.read_autostart_config()) {
            return Err("voice_stopped".into());
        }
        let managed = builder
            .spawn_managed()
            .map_err(|e| format!("Failed to spawn ai-mate process: {e}"))?;

        let pid = managed.id();
        #[cfg(test)]
        self.test_child_pid.store(pid.unwrap_or(0), Ordering::Release);
        info!(?pid, "AI Mate voice companion daemon successfully started");

        *lock = Some(managed);
        drop(lock);

        self.generation.fetch_add(1, Ordering::AcqRel);
        *self.source.write().unwrap_or_else(|err| err.into_inner()) = None;
        self.process_changed.notify_one();
        Ok(())
    }

    async fn recover_audio_if_needed(self:&Arc<Self>)->bool {
        if self.shutdown_requested.load(Ordering::Acquire) {return false;}
        let generation=self.generation.load(Ordering::Acquire);
        let snapshot=self.snapshot();
        if !self.audio_recovery.lock().unwrap_or_else(|error|error.into_inner()).due(&snapshot,nomifun_common::now_ms()) {return false;}
        let mut child=self.child.lock().await;
        if generation!=self.generation.load(Ordering::Acquire) || self.shutdown_requested.load(Ordering::Acquire)
            || !self.read_autostart_config() {return false;}
        warn!(capture_state=?snapshot.audio.as_ref().map(|audio|&audio.capture_state),playback_ready=snapshot.playback_ready,"voice_audio_device_recovery");
        self.generation.fetch_add(1,Ordering::AcqRel);
        *self.source.write().unwrap_or_else(|error|error.into_inner())=None;
        if let Some(mut process)=child.take() {
            if let Err(error)=process.shutdown().await {warn!(%error,"voice_audio_recovery_shutdown_failed");}
        }
        drop(child);
        self.disconnected();
        if let Err(error)=self.start_child(false).await {warn!(%error,"voice_audio_recovery_start_failed");}
        true
    }

    /// Gracefully shuts down and reaps the companion child process tree.
    pub async fn stop(&self) -> Result<(), String> {
        self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        self.shutdown_requested.store(true, Ordering::Release);
        self.reset_restart_budget();
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.process_changed.notify_one();

        let mut lock = self.child.lock().await;
        if let Some(mut child) = lock.take() {
            info!(
                pid = child.id(),
                "Stopping AI Mate voice companion daemon..."
            );
            if let Err(e) = child.shutdown().await {
                warn!(error = %e, "Error shutting down ai-mate process tree");
            } else {
                info!("AI Mate voice companion daemon stopped cleanly");
            }
        }
        self.disconnected();
        Ok(())
    }

    /// Toggles the companion daemon on or off.
    pub async fn toggle(self: &Arc<Self>, enabled: bool) -> Result<bool, String> {
        // Serialize the user's enable/disable choice with prepare's
        // check-start-recheck cycle. A prepare can never stop a newer enable.
        let _lifecycle = self.lifecycle_lock.lock().await;
        self.write_autostart_config(enabled)?;
        if enabled {
            self.start().await?;
            Ok(true)
        } else {
            self.stop().await?;
            Ok(false)
        }
    }

    /// Automatically starts the companion daemon on Flowy startup if enabled and models are Ready.
    pub async fn auto_start_if_enabled(self: &Arc<Self>) {
        if !self.shutdown_requested.load(Ordering::Acquire) && self.read_autostart_config() {
            let models_ready = self.models_manager.get_status().await.state.is_ready();
            if models_ready {
                info!("Auto-starting AI Mate voice companion on application boot...");
                if let Err(err) = self.start_child(false).await {
                    warn!(error = %err, "Failed to auto-start AI Mate companion on boot");
                }
            } else {
                info!(
                    "Voice companion autostart is enabled, but acoustic models are not yet ready; standing by"
                );
            }
        }
    }
}


async fn private_task_submit(
    axum::extract::State(weak): axum::extract::State<std::sync::Weak<VoiceCompanionProcessManager>>,
    headers: axum::http::HeaderMap,
    axum::Json(mut request): axum::Json<VoiceTaskSubmitRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(manager) = weak.upgrade() else { return task_api_error("runtime_unavailable".into()); };
    if headers.get("x-ai-mate-control-token").and_then(|value| value.to_str().ok()) != Some(manager.control_token.as_str()) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    // Read the current daemon state before admitting a command. SSE delivery may
    // lag behind this authenticated request; no delayed event replay is needed.
    let scope=VoiceSessionControlRequest {instance_id:request.instance_id.clone(),session_id:Some(request.command.session_id.clone()),turn_id:Some(request.command.turn_id.clone())};
    if let Err(code)=manager.private_voice_scope(&scope).await {return task_api_error(code);}
    request.instance_id = manager.snapshot().instance_id;
    match manager.submit_task(request).await {
        Ok(delivery) => axum::Json(delivery).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn private_current_task(
    axum::extract::State(weak):axum::extract::State<std::sync::Weak<VoiceCompanionProcessManager>>,
    headers:axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(manager)=weak.upgrade() else {return task_api_error("runtime_unavailable".into());};
    if headers.get("x-ai-mate-control-token").and_then(|value|value.to_str().ok())!=Some(manager.control_token.as_str()) {return axum::http::StatusCode::UNAUTHORIZED.into_response();}
    match manager.tasks.as_ref().map(|tasks|tasks.current_task()).transpose() {
        Ok(task) => {
            let task=task.flatten().map(|mut task|{task.result_text=None;task});
            axum::Json(task).into_response()
        },
        Err(code) => task_api_error(code),
    }
}

async fn private_task_control(
    axum::extract::State(weak):axum::extract::State<std::sync::Weak<VoiceCompanionProcessManager>>,
    headers:axum::http::HeaderMap,
    axum::Json(request):axum::Json<VoiceTaskControlCommand>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(manager)=weak.upgrade() else {return task_api_error("runtime_unavailable".into());};
    if headers.get("x-ai-mate-control-token").and_then(|value|value.to_str().ok())!=Some(manager.control_token.as_str()) {return axum::http::StatusCode::UNAUTHORIZED.into_response();}
    if let Err(code)=manager.private_voice_scope(&request.scope).await {return task_api_error(code);}
    let raw_user_text = request.raw_user_text.as_deref().map(str::trim).filter(|value| !value.is_empty());
    // An older daemon may query without the new provenance field. Reads remain
    // compatible; cancellation still requires the explicit affirmative text.
    if request.action == VoiceTaskControlAction::CancelTask && raw_user_text.is_none() {
        return task_api_error("task_control_scope_required".into());
    }
    let Some(recorder)=manager.dialogue_recorder.clone() else {
        return task_api_error("binding_invalid".into());
    };
    let conversation=match recorder.task_scope_conversation(&request.scope).await {
        Ok(Some(conversation))=>conversation,
        Ok(None)=>return task_api_error("binding_invalid".into()),
        Err(error)=>return task_api_error(crate::voice_dialogue::record_error_code(&error).into()),
    };
    if request.task.conversation_id.as_deref()!=Some(conversation.as_str()) {
        return task_api_error("task_scope_changed".into());
    }
    if request.action==VoiceTaskControlAction::CancelTask
        && !nomifun_api_types::voice::is_affirmative_cancel_command(raw_user_text.unwrap_or_default()) {
        return task_api_error("task_control_ambiguous".into());
    }
    let request_scope=VoiceTaskControlRequest {instance_id:manager.snapshot().instance_id,task:request.task.clone()};
    let result=manager.control_task(request_scope,request.action).await;
    let persistence=match raw_user_text {
        Some(raw)=>recorder.record_task_control_result(
            &request.scope,
            &request.task,
            Some(raw),
            request.locale.as_deref(),
            request.action,
            &result,
        ).await,
        None=>{
            // Compatibility read-only query: no provenance means the user
            // question cannot be projected, but the task state is still valid.
            tracing::warn!("voice_task_control_missing_raw_text");
            Ok(())
        }
    };
    match result {
        Ok(task) => match persistence {
            Ok(()) => axum::Json(task).into_response(),
            Err(error) => task_api_error(crate::voice_dialogue::record_error_code(&error).into()),
        },
        Err(code) => {
            if let Err(error)=persistence {
                tracing::warn!(code=crate::voice_dialogue::record_error_code(&error),"voice_task_control_result_not_persisted");
            }
            task_api_error(code)
        }
    }
}

/// Formats spoken/display task status response text across languages and terminal outcomes.
fn format_task_status_text(task: &nomifun_api_types::voice::VoiceTaskSnapshot, is_en: bool, is_cancelled: bool) -> String {
    use nomifun_api_types::voice::VoiceTaskStatus;
    if is_cancelled && task.status == VoiceTaskStatus::Failed {
        if is_en { "Task cancelled." } else { "任务已取消。" }.to_string()
    } else if is_en {
        match task.status {
            VoiceTaskStatus::Queued => "The task is queued, waiting for previous task to finish.".into(),
            VoiceTaskStatus::Delivering => "Submitting the task, receipt verification pending.".into(),
            VoiceTaskStatus::Unknown => "Task status not verified yet, checking receipt.".into(),
            VoiceTaskStatus::Accepted => "Task accepted, waiting for execution results.".into(),
            VoiceTaskStatus::Cancelling => "Cancelling the task, awaiting execution confirmation.".into(),
            VoiceTaskStatus::Completed => format!(
                "Execution finished. {}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("Please check task details.").chars().take(220).collect::<String>()
            ),
            VoiceTaskStatus::Failed => format!(
                "Task ended, execution was unsuccessful. {}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("Please check task details for reasons.").chars().take(220).collect::<String>()
            ),
        }
    } else {
        match task.status {
            VoiceTaskStatus::Queued => "任务已排队，等待上一任务结束。".into(),
            VoiceTaskStatus::Delivering => "任务正在提交，尚未核实接单。".into(),
            VoiceTaskStatus::Unknown => "任务状态尚未核实，正在核对原回执。".into(),
            VoiceTaskStatus::Accepted => "任务已接受，正在等待执行结果。".into(),
            VoiceTaskStatus::Cancelling => "正在取消任务，等待执行侧确认。".into(),
            VoiceTaskStatus::Completed => format!(
                "执行已结束。{}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("请查看任务详情。").chars().take(220).collect::<String>()
            ),
            VoiceTaskStatus::Failed => format!(
                "任务已结束，执行结果未成功。{}",
                task.result_text.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("请查看任务详情了解原因。").chars().take(220).collect::<String>()
            ),
        }
    }
}

fn format_task_result_announcement(task: &VoiceTaskSnapshot, goal: &str, is_en: bool) -> String {
    let summary = extract_spoken_summary(task.result_text.as_deref().unwrap_or(""), task.status, is_en);
    let spoken_goal: String = goal.chars().take(25).collect();
    if spoken_goal.trim().is_empty() { summary }
    else if is_en { format!("Result for ‘{spoken_goal}’: {summary}") }
    else { format!("任务「{spoken_goal}」的结果：{summary}") }
}

/// Extracts a concise, high-signal spoken summary (50-80 chars) suitable for TTS playback.
///
/// Preserves:
/// - Explicit conclusion sentences (e.g. "已完成", "成功", "未找到", "失败");
/// - Vital parameters (temperatures, numbers, dates, locations, units);
/// - Negations ("不", "未", "无", "失败", "没有", "无法") - never drops or inverts polarity;
/// - Strips markdown noise, code blocks, and stage directions.
pub fn extract_spoken_summary(raw: &str, status: VoiceTaskStatus, is_en: bool) -> String {
    let text = strip_markdown_and_noise(raw);
    let text = text.trim();
    if text.is_empty() {
        return match (status, is_en) {
            (VoiceTaskStatus::Completed, true) => "Execution finished. See conversation for details.".into(),
            (VoiceTaskStatus::Completed, false) => "执行已结束，详情请在会话中查看。".into(),
            (_, true) => "Execution was unsuccessful. See conversation for details.".into(),
            (_, false) => "任务未完成，详情请在会话中查看。".into(),
        };
    }
    let details = if is_en { ". See conversation for details." } else { "，详情请在会话中查看。" };
    let restrictions = if is_en { "Do not act on a shortened result. Review the full details and restrictions." }
        else { "任务执行结果包含未完成事项与重要说明，详情请在会话中查看。" };
    // If it's already concise enough (under 80 chars), speak as is.
    if text.chars().count() <= 80 {
        return text.to_string();
    }

    let has_failure = is_negation_or_failure(text) || status == VoiceTaskStatus::Failed;
    const MAX_TARGET_CHARS: usize = 65;

    let sentences = split_sentences(text);
    // English restrictions need their whole sentence: a success prefix is not
    // a safe substitute when the failure/condition cannot fit the speech budget.
    if has_english_failure(text) {
        let critical = sentences
            .iter()
            .copied()
            .filter(|s| is_negation_or_failure(s))
            .collect::<Vec<_>>()
            .join(". ");
        return if critical.is_empty() || critical.chars().count() > 60 {
            restrictions.into()
        } else {
            format!("{critical}{details}")
        };
    }
    if sentences.is_empty() {
        let truncated: String = text.chars().take(MAX_TARGET_CHARS).collect();
        let res = format!("{truncated}{details}");
        if has_failure && !is_negation_or_failure(&res) {
            return restrictions.to_string();
        }
        return res;
    }

    // Score sentences:
    // Conclusive words, parameters, negation words
    let mut scored: Vec<(usize, i32, &str)> = Vec::new();
    for (idx, &s) in sentences.iter().enumerate() {
        let mut score = 0;
        // Prioritize early sentences
        if idx == 0 {
            score += 15;
        } else if idx == 1 {
            score += 8;
        }
        // Conclusive keywords
        if contains_any(s, &["已完成", "成功", "完成", "查询到", "查到", "结果为", "总计", "共计", "天气为", "温度为", "状态为"]) {
            score += 30;
        } else if contains_any(s, &["天气", "温度", "状态", "结果"]) {
            score += 15;
        }
        // Negation keywords - critical! Never drop negation
        if is_negation_or_failure(s) {
            score += 60;
        }
        // Parameter signals: digits and units
        if s.chars().any(|c| c.is_ascii_digit()) {
            score += 15;
        }
        if contains_any(s, &["℃", "度", "元", "个", "条", "次", "点", "分", "%", "米", "日", "号"]) {
            score += 15;
        }
        scored.push((idx, score, s));
    }

    // Sort by score descending to find best candidates
    scored.sort_by(|a, b| b.1.cmp(&a.1));

    let mut selected_indices = Vec::new();
    let mut accumulated_chars = 0;

    for (idx, _, s) in &scored {
        let s_chars = s.chars().count();
        if accumulated_chars + s_chars <= MAX_TARGET_CHARS {
            selected_indices.push(*idx);
            accumulated_chars += s_chars;
        } else if selected_indices.is_empty() {
            // Even the single best sentence is longer than MAX_TARGET_CHARS.
            let clauses = split_clauses(s);
            let mut scored_clauses: Vec<(usize, i32, &str)> = Vec::new();
            for (c_idx, &c) in clauses.iter().enumerate() {
                let mut c_score = 0;
                if c_idx == 0 {
                    c_score += 15;
                }
                if is_negation_or_failure(c) {
                    c_score += 60;
                }
                if contains_any(c, &["已完成", "成功", "完成", "查询到", "查到", "结果为", "但是", "但"]) {
                    c_score += 30;
                }
                if c.chars().any(|ch| ch.is_ascii_digit()) {
                    c_score += 15;
                }
                scored_clauses.push((c_idx, c_score, c));
            }
            scored_clauses.sort_by(|a, b| b.1.cmp(&a.1));

            let mut selected_clause_indices = Vec::new();
            let mut acc_clause_chars = 0;
            for (c_idx, _, c) in &scored_clauses {
                let c_len = c.chars().count();
                if acc_clause_chars + c_len <= MAX_TARGET_CHARS {
                    selected_clause_indices.push(*c_idx);
                    acc_clause_chars += c_len;
                }
            }

            if selected_clause_indices.is_empty() {
                let target_clause = if has_failure {
                    clauses.iter().find(|&&c| is_negation_or_failure(c)).unwrap_or(&clauses[0])
                } else {
                    &clauses[0]
                };
                let first_part: String = target_clause.chars().take(MAX_TARGET_CHARS).collect();
                let trimmed = first_part.trim_end_matches(['，', ',']);
                let res = format!("{trimmed}{details}");
                if has_failure && !is_negation_or_failure(&res) {
                    return restrictions.to_string();
                }
                return res;
            }

            selected_clause_indices.sort_unstable(); // keep chronological order
            let mut clause_summary = String::new();
            for &c_idx in &selected_clause_indices {
                let c = clauses[c_idx].trim();
                if !c.is_empty() {
                    if !clause_summary.is_empty() && !clause_summary.ends_with(['，', ',']) {
                        clause_summary.push_str(if is_en { ", " } else { "，" });
                    }
                    clause_summary.push_str(c);
                }
            }
            let trimmed = clause_summary.trim_end_matches(['，', ',']);
            let res = format!("{trimmed}{details}");
            if has_failure && !is_negation_or_failure(&res) {
                return restrictions.to_string();
            }
            return res;
        }
    }

    selected_indices.sort_unstable(); // keep chronological order
    let mut summary = String::new();
    for &idx in &selected_indices {
        let s = sentences[idx].trim();
        if !s.is_empty() {
            if !summary.is_empty() && !summary.ends_with(['。', '！', '!', '？', '?', '；', ';', '，', ',']) {
                summary.push_str(if is_en { ", " } else { "，" });
            }
            summary.push_str(s);
        }
    }

    let summary = summary.trim_end_matches(['。', '！', '!', '？', '?', '；', ';', '，', ',']);
    let result = if summary.chars().count() < text.chars().count() {
        format!("{summary}{details}")
    } else {
        summary.to_string()
    };
    if has_failure && !is_negation_or_failure(&result) {
        return restrictions.to_string();
    }
    result
}

fn is_negation_or_failure(text: &str) -> bool {
    contains_any(
        text,
        &[
            "未找到", "失败", "无法", "未完成", "未成功", "未能", "没有", "不存在",
            "无数据", "错误", "未查到", "不成功", "不能", "未复制", "仍然为空", "异常",
        ],
    ) || has_english_failure(text)
        || (text.contains('未') && !text.contains("未来"))
        || (text.contains('无') && !text.contains("无论"))
        || (text.contains('不') && !text.contains("不但") && !text.contains("不仅"))
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| text.contains(k))
}

fn has_english_failure(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '\'' && c != '’')
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "no" | "not"
                    | "never"
                    | "cannot"
                    | "can't"
                    | "can’t"
                    | "couldn't"
                    | "couldn’t"
                    | "don't"
                    | "don’t"
                    | "doesn't"
                    | "doesn’t"
                    | "didn't"
                    | "didn’t"
                    | "wasn't"
                    | "wasn’t"
                    | "weren't"
                    | "weren’t"
                    | "won't"
                    | "won’t"
                    | "failed"
                    | "failure"
                    | "failures"
                    | "unable"
                    | "unsuccessful"
                    | "denied"
                    | "error"
                    | "errors"
            )
        })
}

fn split_sentences(text: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        let after = index + ch.len_utf8();
        let boundary = matches!(ch, '。' | '！' | '!' | '？' | '?' | '\n' | '；' | ';')
            || (ch == '.' && text[after..].chars().next().is_none_or(char::is_whitespace));
        if boundary {
            let sentence = text[start..index].trim();
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            start = after;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

fn split_clauses(text: &str) -> Vec<&str> {
    text.split(|c| matches!(c, '，' | ',' | '、'))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

fn strip_markdown_and_noise(raw: &str) -> String {
    let stage_cleared = nomifun_common::stage_direction::strip_stage_directions(raw);
    let mut result = String::with_capacity(stage_cleared.len());
    let mut in_code_block = false;

    for line in stage_cleared.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        if trimmed.len() >= 3 && trimmed.chars().all(|c| c == '-' || c == '*' || c == '=') {
            continue;
        }
        let mut line_str = trimmed;
        while line_str.starts_with('#') {
            line_str = line_str.trim_start_matches('#').trim_start();
        }
        if line_str.starts_with('>') {
            line_str = line_str.trim_start_matches('>').trim_start();
        }
        if line_str.starts_with("- ") || line_str.starts_with("* ") {
            line_str = line_str[2..].trim_start();
        } else if let Some(rest) = line_str.strip_prefix(|c: char| c.is_ascii_digit()) {
            if let Some(rest2) = rest.strip_prefix(". ") {
                line_str = rest2.trim_start();
            }
        }
        if line_str.is_empty() {
            continue;
        }

        let cleaned_line = clean_inline_markdown(line_str);
        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&cleaned_line);
    }
    result
}

fn clean_inline_markdown(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '`' || c == '*' || c == '_' || c == '~' {
            i += 1;
            continue;
        }
        if c == '[' {
            if let Some(close_bracket) = chars[i..].iter().position(|&x| x == ']') {
                let text_end = i + close_bracket;
                if text_end + 1 < chars.len() && chars[text_end + 1] == '(' {
                    if let Some(close_paren) = chars[text_end + 1..].iter().position(|&x| x == ')') {
                        let link_text: String = chars[i + 1..text_end].iter().collect();
                        out.push_str(&link_text);
                        i = text_end + 1 + close_paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Host-side gate for the model install/delete routes. It runs before the
/// models crate so the daemon's native file handles are released before a
/// writer touches the canonical directory.
pub async fn model_write_guard_middleware(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let method = request.method().clone();
    let path = request.uri().path();
    let is_model_write = (method == axum::http::Method::POST
        && path == "/api/voice/models/install")
        || (method == axum::http::Method::DELETE
            && (path == "/api/voice/models" || path == "/api/voice/models/"));
    if !is_model_write {
        return next.run(request).await;
    }
    match manager.begin_model_write().await {
        Ok(guard) => {
            let response = next.run(request).await;
            drop(guard);
            response
        }
        Err(code) => {
            let message = match code.as_str() {
                "voice_suite_in_use" => "A voice call, task or approval is using the models",
                "voice_resource_write_in_progress" => "Another voice model write is in progress",
                _ => "The voice models are busy",
            };
            (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({
                    "error": format!("{code}: {message}"),
                    "code": code,
                })),
            )
                .into_response()
        }
    }
}

async fn companion_capabilities_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<Option<nomifun_api_types::voice::VoiceCapabilityReport>> {
    axum::Json(manager.current_capabilities().await)
}

async fn companion_prepare_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<nomifun_api_types::voice::VoiceSelfCheckRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let mut started = false;
    {
        // Serialize with toggle(): a disable that lands after this check cannot
        // be overwritten, and a newer enable cannot be stopped by this prepare.
        let _lifecycle = manager.lifecycle_lock.lock().await;
        // A prepare is never an enable action. The user's explicit switch in
        // Settings owns that transition; an unknown/disabled choice must not be
        // repaired into autostart here.
        if !manager.read_autostart_config() {
            return (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
            )
                .into_response();
        }
        if !manager.status().await.running {
            if !manager.models_manager.get_status().await.state.is_ready() {
                return (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({
                        "code": "models_not_installed",
                        "error": "models_not_installed"
                    })),
                )
                    .into_response();
            }
            if let Err(error) = manager.start_child(true).await {
                return (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({ "code": "daemon_start_failed", "error": error })),
                )
                    .into_response();
            }
            started = true;
            // The user may have disabled voice while the child was starting.
            if !manager.read_autostart_config() {
                let _ = manager.stop().await;
                return (
                    axum::http::StatusCode::CONFLICT,
                    axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
                )
                    .into_response();
            }
        }
    }
    if !manager.read_autostart_config() {
        return (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
        )
            .into_response();
    }
    if started {
        for _ in 0..60 {
            let snapshot = manager.snapshot();
            let audio = snapshot.audio.as_ref();
            let ready = snapshot.connection == VoiceConnectionState::Connected
                && audio.is_some_and(|value| {
                    value.frames_received > 0
                        && value
                            .last_frame_at_ms
                            .is_some_and(|at| nomifun_common::now_ms().saturating_sub(at) <= 2_000)
                });
            if ready {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        if !manager.read_autostart_config() {
            return (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({ "code": "voice_disabled", "error": "voice_disabled" })),
            )
                .into_response();
        }
    }
    match tokio::time::timeout(
        std::time::Duration::from_secs(120),
        manager.run_self_check_with_locale_and_stage(
            request.include_llm,
            request.locale,
            request.stage.or_else(|| Some("preparing".into())),
        ),
    )
    .await
    {
        Ok(Ok(report)) => axum::Json(report).into_response(),
        Ok(Err(code)) => (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "code": code, "error": code })),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            axum::Json(serde_json::json!({
                "code": "self_check_budget_exhausted",
                "error": "self_check_budget_exhausted"
            })),
        )
            .into_response(),
    }
}
async fn companion_self_check_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<nomifun_api_types::voice::VoiceSelfCheckRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match tokio::time::timeout(
        std::time::Duration::from_secs(120),
        manager.run_self_check_with_locale_and_stage(
            request.include_llm,
            request.locale,
            request.stage,
        ),
    )
    .await
    {
        Ok(Ok(report)) => axum::Json(report).into_response(),
        Ok(Err(code)) => (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "code": code, "error": code })),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::GATEWAY_TIMEOUT,
            axum::Json(serde_json::json!({
                "code": "self_check_budget_exhausted",
                "error": "self_check_budget_exhausted"
            })),
        )
            .into_response(),
    }
}

pub fn voice_companion_routes(manager: Arc<VoiceCompanionProcessManager>) -> axum::Router {
    use axum::routing::{get, post};
    manager.start_projection();
    manager.start_task_projection();
    axum::Router::new()
        .route("/api/voice/companion/execution", get(companion_execution_handler))
        .route("/api/voice/companion/tasks", post(companion_submit_task_handler))
        .route("/api/voice/companion/messages", post(companion_keyboard_message_handler))
        .route("/api/voice/companion/tasks/{command_id}", get(companion_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/cancel",post(companion_cancel_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/confirmations",get(companion_task_confirmations_handler))
        .route("/api/voice/companion/tasks/{command_id}/confirmations/{approval_id}",post(companion_confirm_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/replay", post(companion_replay_task_handler))
        .route("/api/voice/companion/tasks/{command_id}/notification", get(companion_task_notification_handler))
        .route(
            "/api/voice/companion/config",
            get(companion_config_handler),
        )
        .route(
            "/api/voice/companion/feedback",
            get(companion_feedback_handler).post(companion_update_feedback_handler),
        )
        .route(
            "/api/voice/companion/feedback/preview",
            post(companion_preview_feedback_handler),
        )
        .route("/api/voice/companion/status", get(companion_status_handler))
        .route(
            "/api/voice/companion/capabilities",
            get(companion_capabilities_handler),
        )
        .route(
            "/api/voice/companion/self-check",
            post(companion_self_check_handler),
        )
        .route(
            "/api/voice/companion/prepare",
            post(companion_prepare_handler),
        )
        .route(
            "/api/voice/companion/toggle",
            post(companion_toggle_handler),
        )
        .route("/api/voice/companion/events", get(companion_events_handler))
        .route("/api/voice/companion/wake", post(companion_wake_handler))
        .route(
            "/api/voice/companion/session",
            get(companion_snapshot_handler),
        )
        .route(
            "/api/voice/companion/target",
            get(companion_target_handler).post(companion_update_target_handler),
        )
        .route(
            "/api/voice/companion/candidate",
            post(companion_candidate_handler),
        )
        .route(
            "/api/voice/companion/session/stop",
            post(companion_stop_session_handler),
        )
        .route(
            "/api/voice/companion/session/start",
            post(companion_start_call_handler),
        )
        .with_state(manager)
}

async fn companion_target_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceTargetSettings> {
    axum::Json(manager.target_settings().await)
}

async fn companion_update_target_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(requested): axum::Json<VoiceTargetUpdateRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.update_target(requested).await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => {
            let status = match code.as_str() {
                "companion_not_found" => axum::http::StatusCode::NOT_FOUND,
                "companion_id_required" => axum::http::StatusCode::BAD_REQUEST,
                _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, axum::Json(serde_json::json!({ "code": code }))).into_response()
        }
    }
}

async fn companion_candidate_handler() -> axum::response::Response {
    use axum::response::IntoResponse;
    // A browser request cannot attest native window focus or current navigation.
    (axum::http::StatusCode::FORBIDDEN, axum::Json(serde_json::json!({ "code": "native_context_required" }))).into_response()
}

async fn companion_execution_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceExecutionSettings> {
    axum::Json(manager.execution_settings().await)
}

fn task_api_error(code: String) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse};
    let status = match code.as_str() {
        "runtime_unavailable" | "daemon_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
        "task_scope_changed" | "task_command_conflict" | "task_already_active" | "task_queue_full" | "task_delivery_pending" | "task_approval_changed" | "voice_busy" | "binding_busy" | "voice_record_conflict" | "voice_generation_unknown" | "task_execution_config_conflict" => StatusCode::CONFLICT,
        "task_creation_rejected" | "task_delivery_rejected" => StatusCode::UNPROCESSABLE_ENTITY,
        "task_not_found" => StatusCode::NOT_FOUND,
        "binding_invalid" | "execution_unconfigured" | "task_scope_invalid" | "task_goal_invalid" | "task_workspace_invalid" | "task_execution_model_unconfigured" | "model_unavailable" | "task_preset_unavailable" | "task_preset_requires_nomi" => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, axum::Json(serde_json::json!({"code":code}))).into_response()
}

async fn companion_keyboard_message_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<nomifun_api_types::voice::VoiceKeyboardMessageRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.submit_keyboard_message(request).await {
        Ok(delivery) => axum::Json(delivery).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn companion_submit_task_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<VoiceTaskSubmitRequest>,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse};
    match manager.submit_task(request).await {
        Ok(delivery) => {
            let status = if matches!(delivery.task.status, VoiceTaskStatus::Delivering | VoiceTaskStatus::Unknown) {StatusCode::ACCEPTED} else {StatusCode::OK};
            (status, axum::Json(delivery)).into_response()
        }
        Err(code) => task_api_error(code),
    }
}

async fn companion_task_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(tasks) = &manager.tasks else {return task_api_error("runtime_unavailable".into());};
    match tasks.status_for(&command_id).await {
        Ok(task) => {manager.project_task(task.clone()); axum::Json(task).into_response()},
        Err(code) => task_api_error(code),
    }
}

async fn companion_cancel_task_handler(
    axum::extract::State(manager):axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id):axum::extract::Path<String>,
    axum::Json(request):axum::Json<VoiceTaskControlRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if command_id!=request.task.command_id {return task_api_error("task_scope_changed".into());}
    match manager.control_task(request,VoiceTaskControlAction::CancelTask).await {
        Ok(task) => axum::Json(task).into_response(),Err(code) => task_api_error(code),
    }
}

async fn companion_task_confirmations_handler(
    axum::extract::State(manager):axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id):axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(tasks)=&manager.tasks else{return task_api_error("runtime_unavailable".into());};
    match tasks.confirmations(&command_id).await {Ok(pending)=>axum::Json(pending).into_response(),Err(code)=>task_api_error(code)}
}

async fn companion_confirm_task_handler(
    axum::extract::State(manager):axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path((command_id,approval_id)):axum::extract::Path<(String,String)>,
    axum::Json(request):axum::Json<VoiceToolConfirmationRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if request.instance_id!=manager.snapshot().instance_id || request.command_id!=command_id {return task_api_error("task_scope_changed".into());}
    let Some(tasks)=&manager.tasks else{return task_api_error("runtime_unavailable".into());};
    match tasks.confirm(&approval_id,&request).await {
        Ok(())=>axum::Json(serde_json::json!({"success":true})).into_response(),Err(code)=>task_api_error(code),
    }
}

async fn companion_replay_task_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
    axum::Json(request):axum::Json<nomifun_api_types::voice::VoiceTaskReplayRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.replay_task_result(&command_id,&request).await {
        Ok(()) => axum::Json(serde_json::json!({"success": true})).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn companion_task_notification_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::extract::Path(command_id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let Some(tasks) = &manager.tasks else {return task_api_error("runtime_unavailable".into());};
    match tasks.notification_for(&command_id, manager.snapshot().locale.as_deref()) {
        Ok(Some(notif)) => axum::Json(notif).into_response(),
        Ok(None) => (axum::http::StatusCode::NOT_FOUND, axum::Json(serde_json::json!({"code": "notification_not_found"}))).into_response(),
        Err(code) => task_api_error(code),
    }
}

async fn companion_config_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceDialogueSettings> {
    axum::Json(manager.dialogue_settings().await)
}

async fn companion_feedback_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.feedback_settings().await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"code":code})),
        )
            .into_response(),
    }
}

async fn companion_update_feedback_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(patch): axum::Json<VoiceFeedbackPatch>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.update_feedback(patch).await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"code":code})),
        )
            .into_response(),
    }
}

async fn companion_preview_feedback_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<VoiceFeedbackPreviewRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager
        .feedback_rpc(
            reqwest::Method::POST,
            "/preview",
            serde_json::to_value(request).ok(),
        )
        .await
    {
        Ok(response) if response.status().is_success() => {
            axum::Json(serde_json::json!({"success":true})).into_response()
        }
        Ok(response) => {
            let code = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|value| value["code"].as_str().map(str::to_owned))
                .filter(|code| {
                    matches!(
                        code.as_str(),
                        "voice_busy" | "feedback_muted" | "cue_output_failed"
                    )
                })
                .unwrap_or_else(|| "daemon_unavailable".into());
            (
                axum::http::StatusCode::CONFLICT,
                axum::Json(serde_json::json!({"code":code})),
            )
                .into_response()
        }
        Err(code) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({"code":code})),
        )
            .into_response(),
    }
}

async fn companion_status_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceCompanionStatus> {
    axum::Json(manager.status().await)
}

async fn companion_toggle_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(payload): axum::Json<VoiceCompanionToggleRequest>,
) -> axum::Json<VoiceCompanionToggleResponse> {
    match manager.toggle(payload.enabled).await {
        Ok(running) => axum::Json(VoiceCompanionToggleResponse {
            success: true,
            running,
            error: None,
        }),
        Err(err) => axum::Json(VoiceCompanionToggleResponse {
            success: false,
            running: false,
            error: Some(err),
        }),
    }
}

async fn companion_events_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::response::Response {
    use axum::response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    };
    let rx = manager.subscribe();
    let first = Some(manager.snapshot());
    let stream = futures_util::stream::unfold(
        (rx, first, manager),
        |(mut rx, first, manager)| async move {
            let snapshot = if let Some(first) = first {
                first
            } else {
                match rx.recv().await {
                    Ok(snapshot) => snapshot,
                    Err(broadcast::error::RecvError::Lagged(_)) => manager.snapshot(),
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            };
            let event = VoiceCompanionEvent::Snapshot { snapshot };
            let data = serde_json::to_string(&event).expect("voice DTO serializes");
            Some((
                Ok::<_, std::convert::Infallible>(Event::default().data(data)),
                (rx, None, manager),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn companion_snapshot_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
) -> axum::Json<VoiceSessionSnapshot> {
    axum::Json(manager.snapshot())
}

async fn companion_stop_session_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(request): axum::Json<VoiceSessionControlRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.stop_session(request).await {
        Ok(()) => axum::Json(serde_json::json!({"success": true})).into_response(),
        Err(code) => (
            axum::http::StatusCode::CONFLICT,
            axum::Json(serde_json::json!({"success": false, "error": code})),
        )
            .into_response(),
    }
}

async fn companion_start_call_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(payload): axum::Json<nomifun_api_types::voice::VoiceCallStartRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.start_call(payload).await {
        Ok(res) => axum::Json(res).into_response(),
        Err((status, err_str)) => {
            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&err_str) {
                (status, axum::Json(json_val)).into_response()
            } else {
                (status, axum::Json(serde_json::json!({ "code": err_str, "message": err_str }))).into_response()
            }
        }
    }
}

async fn companion_wake_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    let status = manager.status().await;
    if !status.running {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "success": false,
                "error": "Voice companion is not running"
            })),
        )
            .into_response();
    }

    let client = match reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build() {
        Ok(client)=>client,
        Err(_)=>return (StatusCode::SERVICE_UNAVAILABLE,axum::Json(serde_json::json!({"success":false,"error":"companion_control_unavailable"}))).into_response(),
    };
    match client
        .post(format!("{}/api/voice/wake",manager.daemon_origin))
        .header("x-ai-mate-control-token", &manager.control_token)
        .timeout(std::time::Duration::from_secs(3))
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            let json = resp.json::<serde_json::Value>().await.unwrap_or_default();
            (status, axum::Json(json)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            axum::Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to proxy wake to companion: {e}")
            })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify_feature_headless_binary() {
        let binary = std::env::var_os("VOICE_COMPANION_BIN")
            .map(std::path::PathBuf::from)
            .expect("feature headless binary");
        assert_eq!(
            binary.file_name().and_then(|name| name.to_str()),
            Some("voice-companion-headless.exe"),
            "refuse non-feature-headless artifact path before spawn"
        );
        let expected = std::env::var("VOICE_COMPANION_R13_HEADLESS_SHA")
            .expect("expected feature headless SHA");
        let actual = nomifun_voice_models::verifier::compute_sha256(
            &std::fs::read(&binary).expect("read feature binary"),
        );
        assert_eq!(actual, expected, "refuse unexpected feature binary SHA before spawn");
    }

    fn test_port_closed(port: u16) -> bool {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_millis(300),
        )
        .is_err()
    }

    async fn run_headless_case<F, Fut>(
        manager: Arc<VoiceCompanionProcessManager>,
        port: u16,
        health_timeout: std::time::Duration,
        case_timeout: std::time::Duration,
        body: F,
    ) where
        F: FnOnce(Arc<VoiceCompanionProcessManager>, String, String) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        use futures_util::FutureExt as _;
        let origin = format!("http://127.0.0.1:{port}");
        let token = manager.control_token.clone();
        let owned_pid = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let run_manager = manager.clone();
        let run_pid = owned_pid.clone();
        let run = async move {
            run_manager.write_autostart_config(true).unwrap();
            run_manager.start_projection();
            run_manager.start().await.unwrap();
            let started_pid = run_manager.status().await.pid.unwrap_or(0);
            run_pid.store(started_pid, Ordering::Release);
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap();
            let healthy = tokio::time::timeout(health_timeout, async {
                loop {
                    if client
                        .get(format!("{origin}/healthz"))
                        .send()
                        .await
                        .is_ok_and(|response| response.status().is_success())
                    {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            })
            .await
            .is_ok();
            assert!(healthy, "daemon health stage did not become ready");
            body(run_manager, origin, token).await;
        };
        let outcome = tokio::time::timeout(
            case_timeout,
            std::panic::AssertUnwindSafe(run).catch_unwind(),
        )
        .await;

        let mut cleanup_errors = Vec::new();
        match tokio::time::timeout(std::time::Duration::from_secs(10), manager.stop()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => cleanup_errors.push(format!("stop_error:{error}")),
            Err(_) => cleanup_errors.push("stop_timeout".into()),
        }
        let mut closed = false;
        for _ in 0..20 {
            if test_port_closed(port) {
                closed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if !closed {
            cleanup_errors.push(format!("port_still_open:{port}"));
        }
        // Longer than the projection loop's 1s recovery tick: a delayed restart
        // must not appear after stop completed.
        tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
        let after = manager.status().await;
        if after.running || after.pid.is_some() || !test_port_closed(port) {
            cleanup_errors.push("delayed_restart_or_port_reopen".into());
        }
        if !cleanup_errors.is_empty() {
            panic!("cleanup_failed:{cleanup_errors:?}; owned_pid={}", owned_pid.load(Ordering::Acquire));
        }
        eprintln!("cleanup_case_ok owned_pid={} port={port} port_closed=true", owned_pid.load(Ordering::Acquire));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(payload)) => std::panic::resume_unwind(payload),
            Err(_) => panic!("case_timeout_after_cleanup:owned_pid={}", owned_pid.load(Ordering::Acquire)),
        }
    }
    struct FixtureCompleter {
        result: Result<String, String>,
    }
    #[async_trait::async_trait]
    impl VoiceDialogueCompleter for FixtureCompleter {
        async fn resolve_model(
            &self,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<nomifun_api_types::voice::VoiceModelRef, String> {
            Ok(nomifun_api_types::voice::VoiceModelRef {
                provider_id: "fixture".into(),
                model: "fixture".into(),
            })
        }
        async fn complete(
            &self,
            _prompt: &str,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<String, String> {
            self.result.clone()
        }
    }

    struct ConfigurableCompleter {
        revision: Arc<std::sync::Mutex<String>>,
    }
    #[async_trait::async_trait]
    impl VoiceDialogueCompleter for ConfigurableCompleter {
        async fn resolve_model(
            &self,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<nomifun_api_types::voice::VoiceModelRef, String> {
            Ok(nomifun_api_types::voice::VoiceModelRef {
                provider_id: "fixture".into(),
                model: "fixture".into(),
            })
        }
        async fn complete(
            &self,
            _prompt: &str,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<String, String> {
            Ok("OK".into())
        }
        async fn configuration_revision(&self) -> String {
            self.revision
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }
    struct BarrierCompleter {
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        revision: String,
    }
    #[async_trait::async_trait]
    impl VoiceDialogueCompleter for BarrierCompleter {
        async fn resolve_model(
            &self,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<nomifun_api_types::voice::VoiceModelRef, String> {
            Ok(nomifun_api_types::voice::VoiceModelRef {
                provider_id: "fixture".into(),
                model: "fixture".into(),
            })
        }
        async fn complete(
            &self,
            _prompt: &str,
            _choice: Option<&nomifun_api_types::voice::VoiceModelRef>,
        ) -> Result<String, String> {
            Ok("OK".into())
        }
        async fn configuration_revision(&self) -> String {
            self.entered.notify_one();
            self.release.notified().await;
            self.revision.clone()
        }
    }
    fn populate_checked_suite(models_dir: &std::path::Path) -> std::collections::HashMap<String, String> {
        let mut manifest = std::collections::HashMap::new();
        for &relative in nomifun_voice_models::EXPECTED_FILES {
            let path = models_dir.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let content = format!("capability-suite-{relative}").into_bytes();
            std::fs::write(&path, &content).unwrap();
            if nomifun_voice_models::verifier::DOWNLOAD_FILES.contains(&relative) {
                manifest.insert(
                    relative.to_owned(),
                    nomifun_voice_models::verifier::compute_sha256(&content),
                );
            }
        }
        manifest
    }

    fn engine_payload(
        headers: &axum::http::HeaderMap,
        kws_ok: bool,
        vad_ok: bool,
        asr_ok: bool,
    ) -> axum::Json<serde_json::Value> {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
        };
        axum::Json(serde_json::json!({
            "kws_ok": kws_ok,
            "vad_ok": vad_ok,
            "asr_ok": asr_ok,
            "fixtures_available": true,
            "kws_hits": 4,
            "asr_text": "zh=今天天气怎么样？;en=Hello world, Hey friend.",
            "request_id": header("x-voice-check-request"),
            "instance_id": "fixture-daemon-instance-1",
            "worker_epoch": 7,
            "model_revision": header("x-voice-check-model-revision"),
        }))
    }
    async fn tts_fixture(status: u16, body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/engine-self-check",
                axum::routing::get(
                    |headers: axum::http::HeaderMap| async move {
                        engine_payload(&headers, true, true, true)
                    },
                ),
            )
            .route(
            "/api/voice/diagnostics/tts-self-check",
            axum::routing::post(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/json")],
                    body,
                )
            }),
        );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }

    async fn tts_fixture_with_failed_engine(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/engine-self-check",
                axum::routing::get(
                    |headers: axum::http::HeaderMap| async move {
                        engine_payload(&headers, true, true, false)
                    },
                ),
            )
            .route(
                "/api/voice/diagnostics/tts-self-check",
                axum::routing::post(move || async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("content-type", "application/json")],
                        body,
                    )
                }),
            );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }

    async fn tts_fixture_with_stale_engine(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/engine-self-check",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "kws_ok": true,
                        "vad_ok": true,
                        "asr_ok": true,
                        "fixtures_available": true,
                        "kws_hits": 4,
                        "request_id": "another-check",
                        "instance_id": "another-instance",
                        "worker_epoch": 9,
                        "model_revision": "another-revision",
                    }))
                }),
            )
            .route(
                "/api/voice/diagnostics/tts-self-check",
                axum::routing::post(move || async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        [("content-type", "application/json")],
                        body,
                    )
                }),
            );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }

    async fn tts_fixture_without_engine(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route(
            "/api/voice/diagnostics/tts-self-check",
            axum::routing::post(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [("content-type", "application/json")],
                    body,
                )
            }),
        );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (origin, handle)
    }
    fn capability_snapshot(manager: &VoiceCompanionProcessManager, last_frame_at_ms: i64) {
        let mut snapshot = manager.snapshot.write().unwrap();
        snapshot.enabled = true;
        snapshot.connection = VoiceConnectionState::Connected;
        snapshot.acoustic_ready = true;
        snapshot.playback_ready = true;
        snapshot.worker_epoch = Some(7);
        snapshot.audio = Some(nomifun_api_types::voice::VoiceAudioDiagnostics {
            frames_received: 5,
            last_frame_at_ms: Some(last_frame_at_ms),
            capture_state: "running".into(),
            kws_ready: true,
            vad_ready: true,
            asr_ready: true,
            ..Default::default()
        });
        *manager.source.write().unwrap() = Some(("fixture-daemon-instance-1".into(), 1));
    }

    #[test]
    fn voice_projection_rejects_missing_and_foreign_turn_sessions_without_dropping_valid_speech() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        manager.snapshot.write().unwrap().binding = Some(nomifun_api_types::voice::VoiceBindingSnapshot {
            session_id: "session-b".into(), binding_id: "binding-b".into(), conversation_id: "conversation-b".into(),
            workspace: None, memory_owner_companion_id: None,
        });
        let turn = |session: Option<&str>, id: &str| nomifun_api_types::voice::VoiceConversationTurn {
            session_id: session.map(str::to_owned), turn_id: id.into(), user_text: Some(id.into()), ..Default::default()
        };
        manager.project(VoiceSessionSnapshot {
            instance_id: "daemon".into(), seq: 1, session_id: Some("session-b".into()), turn_id: Some("valid".into()),
            conversation: vec![turn(None, "missing"), turn(Some("session-a"), "foreign"), turn(Some("session-b"), "valid")],
            ..Default::default()
        }, 0);
        let snapshot = manager.snapshot();
        assert_eq!(snapshot.conversation.len(), 1);
        assert_eq!(snapshot.conversation[0].turn_id, "valid");
        assert_eq!(snapshot.conversation[0].session_id.as_deref(), Some("session-b"));
    }

    #[test]
    fn automatic_daemon_restart_is_bounded_and_resets_after_stable_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        let base = tokio::time::Instant::now();
        let seconds = |value: u64| base + std::time::Duration::from_secs(value);

        assert!(matches!(
            manager.restart_decision(seconds(0)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(2)
        ));
        assert!(matches!(
            manager.restart_decision(seconds(2)),
            RestartDecision::StartNow
        ));
        assert!(matches!(
            manager.restart_decision(seconds(3)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(5)
        ));
        assert!(matches!(
            manager.restart_decision(seconds(8)),
            RestartDecision::StartNow
        ));
        assert!(matches!(
            manager.restart_decision(seconds(9)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(15)
        ));
        assert!(matches!(
            manager.restart_decision(seconds(24)),
            RestartDecision::StartNow
        ));
        assert!(matches!(
            manager.restart_decision(seconds(25)),
            RestartDecision::Exhausted
        ));

        manager.mark_daemon_running(seconds(30));
        manager.mark_daemon_running(seconds(91));
        assert!(matches!(
            manager.restart_decision(seconds(92)),
            RestartDecision::Wait(delay) if delay == std::time::Duration::from_secs(2)
        ));
    }

    #[tokio::test]
    async fn self_check_attributes_capture_tts_playback_and_llm_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());

        let report = manager.run_self_check(true).await.unwrap();
        assert_eq!(
            report.capture.evidence, "recent_pcm",
            "capture must be attributed to fresh PCM"
        );
        assert_eq!(report.tts.evidence, "tts_decoded");
        assert_eq!(report.playback.evidence, "sink_consumed");
        assert_eq!(report.llm.evidence, "live_response");
        assert!(report.can_start_full_call, "all attributed capabilities are ready");
        assert!(manager.current_capabilities().await.is_some());
        server.abort();
    }

    #[tokio::test]
    async fn self_check_rejects_engine_evidence_from_another_request_instance_or_revision() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let body = r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#;
        let (origin, server) = tts_fixture_with_stale_engine(200, body).await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter { result: Ok("OK".into()) })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(false).await.unwrap();
        assert_eq!(report.kws.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(report.vad.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(report.asr.reason_code.as_deref(), Some("engine_response_stale"));
        assert!(!report.can_wake);
        assert!(!report.can_start_full_call);
        server.abort();
    }

    #[tokio::test]
    async fn self_check_accepts_distinct_daemon_id_and_invalidates_cache_on_daemon_source_restart() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let body = r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#;
        let (origin, server) = tts_fixture(200, body).await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());

        // 1. Host projection ID is distinct from daemon source ID ("fixture-daemon-instance-1")
        assert_ne!(manager.snapshot().instance_id, "fixture-daemon-instance-1");
        let report = manager.run_self_check(true).await.unwrap();
        assert!(report.can_start_full_call, "distinct daemon instance is accepted");
        assert!(manager.current_capabilities().await.is_some());

        // 2. Daemon restarts and acquires a new instance ID: previous capabilities cache must be invalidated
        *manager.source.write().unwrap() = Some(("restarted-daemon-instance-2".into(), 1));
        assert!(manager.current_capabilities().await.is_none(), "cache must invalidate when daemon source changes");
        server.abort();
    }

    #[tokio::test]
    async fn self_check_rejects_stale_worker_epoch_even_if_daemon_instance_id_matches() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let body = r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#;
        let (origin, server) = tts_fixture(200, body).await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        manager.snapshot.write().unwrap().worker_epoch = Some(7);

        // 1. Initially matches epoch 7: report succeeds and is cached.
        let report = manager.run_self_check(true).await.unwrap();
        assert!(report.can_start_full_call, "epoch 7 is accepted");
        assert!(manager.current_capabilities().await.is_some(), "capabilities cached");

        // 2. Underlying worker is replaced/reloaded -> worker_epoch bumps to 8 on the host side,
        // while daemon ID remains "fixture-daemon-instance-1".
        manager.snapshot.write().unwrap().worker_epoch = Some(8);

        // 3. Cache signature must immediately invalidate because worker epoch changed.
        assert!(manager.current_capabilities().await.is_none(), "cache must invalidate when worker_epoch changes");

        // 4. If daemon responds with stale worker_epoch (7 < 8), self check must reject it as engine_response_stale.
        let stale_report = manager.run_self_check(false).await.unwrap();
        assert_eq!(stale_report.kws.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(stale_report.vad.reason_code.as_deref(), Some("engine_response_stale"));
        assert_eq!(stale_report.asr.reason_code.as_deref(), Some("engine_response_stale"));
        assert!(!stale_report.can_start_full_call);
        assert!(!stale_report.can_wake);

        server.abort();
    }

    #[tokio::test]
    async fn prepare_without_models_fails_before_starting_or_creating_a_session() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.write_autostart_config(true).unwrap();
        let app = voice_companion_routes(manager.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/companion/prepare")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"include_llm":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "models_not_installed");
        assert!(manager.child.lock().await.is_none());
        assert!(manager.snapshot().session_id.is_none());
    }

    #[tokio::test]
    async fn prepare_never_enables_a_disabled_companion() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        assert!(!manager.read_autostart_config());
        let response = voice_companion_routes(manager.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/companion/prepare")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"include_llm":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "voice_disabled");
        assert!(!manager.read_autostart_config(), "prepare must not write autostart");
        assert!(manager.child.lock().await.is_none(), "prepare must not start a disabled companion");
    }

    #[tokio::test]
    async fn self_check_routes_share_the_stored_report_with_status() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let app = voice_companion_routes(manager.clone());

        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/companion/self-check")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"include_llm":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 65_536).await.unwrap();
        let report: nomifun_api_types::voice::VoiceCapabilityReport =
            serde_json::from_slice(&body).unwrap();
        assert!(report.can_start_full_call);

        let capabilities = app
            .oneshot(
                axum::http::Request::builder()
                    .method("GET")
                    .uri("/api/voice/companion/capabilities")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(capabilities.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(capabilities.into_body(), 65_536).await.unwrap();
        let stored: Option<nomifun_api_types::voice::VoiceCapabilityReport> =
            serde_json::from_slice(&body).unwrap();
        assert!(stored.is_some_and(|value| value.can_start_full_call));
        // The check must not open a session, task, memory or wake projection.
        assert!(manager.snapshot().session_id.is_none());
        assert!(manager.snapshot().task.is_none());
        assert!(manager.snapshot().queue.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn self_check_reports_stale_capture_and_classified_failures() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(200, "not-json").await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Err("401 unauthorized".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms() - 5_000);

        let report = manager.run_self_check(true).await.unwrap();
        assert_eq!(report.capture.reason_code.as_deref(), Some("capture_no_recent_pcm"));
        assert!(!report.can_start_full_call);
        assert_eq!(report.tts.reason_code.as_deref(), Some("tts_bad_response"));
        assert_eq!(report.llm.reason_code.as_deref(), Some("llm_auth_failed"));
        assert!(report.degraded_text_only == false);
        server.abort();

        let (origin, server) = tts_fixture(429, "{}").await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::with_config(
                    dir.path(),
                    Vec::new(),
                    std::collections::HashMap::new(),
                )),
                dir.path(),
            )
            .with_replay_daemon(origin)
            .with_dialogue(Arc::new(FixtureCompleter {
                result: Err("429 too many requests".into()),
            })),
        );
        let report = manager.run_self_check(true).await.unwrap();
        assert_eq!(report.llm.reason_code.as_deref(), Some("llm_rate_limited"));
        assert_eq!(report.tts.reason_code.as_deref(), Some("tts_http_429"));
        server.abort();
    }

    #[tokio::test]
    async fn failed_engine_inference_blocks_full_call_and_wake() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture_with_failed_engine(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(false).await.unwrap();
        assert!(!report.can_start_full_call);
        // Wake requires the same verified local chain; engine_loaded alone is
        // not enough.
        assert!(!report.can_wake);
        assert!(!report.can_transcribe);
        assert_eq!(report.asr.reason_code.as_deref(), Some("asr_inference_failed"));
        server.abort();
    }

    #[tokio::test]
    async fn loaded_engines_without_inference_evidence_cannot_authorize_call_or_wake() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture_without_engine(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(false).await.unwrap();
        assert!(!report.can_start_full_call);
        assert!(!report.can_wake);
        assert!(!report.can_transcribe);
        assert_eq!(
            report.asr.reason_code.as_deref(),
            Some("engine_check_unreachable")
        );
        server.abort();
    }

    #[tokio::test]
    async fn host_rejects_a_top_level_ok_with_silent_or_incomplete_tts_proof() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":0,"peak_milli":0}]}"#,
        )
        .await;
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(FixtureCompleter {
                    result: Ok("OK".into()),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let report = manager.run_self_check(true).await.unwrap();
        assert!(!report.can_start_full_call);
        assert_eq!(
            report.tts.reason_code.as_deref(),
            Some("tts_inconsistent_response")
        );
        assert_eq!(
            report.playback.reason_code.as_deref(),
            Some("playback_without_valid_synthesis")
        );
        server.abort();
    }

    #[tokio::test]
    async fn stale_reader_cannot_clear_a_newer_published_report() {
        let dir = tempfile::tempdir().unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
                dir.path(),
            )
            .with_dialogue(Arc::new(BarrierCompleter {
                entered: entered.clone(),
                release: release.clone(),
                revision: "old".into(),
            })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        manager.self_check_attempt.store(1, Ordering::Release);
        let old_report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report: old_report,
            attempt: 1,
            expires_at_ms: nomifun_common::now_ms() + 60_000,
            signature: "stale-signature".into(),
        });

        let reader = manager.clone();
        let read = tokio::spawn(async move { reader.current_capabilities().await });
        tokio::time::timeout(std::time::Duration::from_secs(1), entered.notified())
            .await
            .expect("old read must enter configuration_revision");

        let new_report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report: new_report,
            attempt: 2,
            expires_at_ms: nomifun_common::now_ms() + 60_000,
            signature: "new-signature".into(),
        });
        release.notify_waiters();

        let result = read.await.unwrap();
        assert!(result.is_none(), "the stale read must not return its old report");
        let cached = manager.capabilities.read().unwrap().clone();
        assert_eq!(
            cached.as_ref().map(|stored| stored.attempt),
            Some(2),
            "an old read must not clear a newer report"
        );
    }

    #[tokio::test]
    async fn capability_report_is_invalidated_by_fresh_pcm_and_config_revision() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let (origin, server) = tts_fixture(
            200,
            r#"{"ok":true,"provider":"fixture","online":true,"playback_ok":true,"languages":[{"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},{"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}]}"#,
        )
        .await;
        let revision = Arc::new(std::sync::Mutex::new("rev-1".to_string()));
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(ConfigurableCompleter {
                    revision: revision.clone(),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        assert!(manager.run_self_check(true).await.unwrap().can_start_full_call);
        assert!(manager.current_capabilities().await.is_some());

        manager
            .snapshot
            .write()
            .unwrap()
            .audio
            .as_mut()
            .unwrap()
            .last_frame_at_ms = Some(nomifun_common::now_ms() - 5_000);
        assert!(manager.current_capabilities().await.is_none());

        capability_snapshot(&manager, nomifun_common::now_ms());
        assert!(manager.run_self_check(true).await.unwrap().can_start_full_call);
        *revision.lock().unwrap() = "rev-2".to_string();
        assert!(manager.current_capabilities().await.is_none());
        server.abort();
    }

    #[tokio::test]
    async fn readiness_check_atomically_blocks_new_calls_and_model_writes() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.self_check_active.store(true, Ordering::Release);
        let call = manager
            .start_call(VoiceCallStartRequest { locale: None,
                request_id: "during-check".into(),
                target: VoiceCallTarget::DefaultWorkspace,
                session_mode: None,
            })
            .await
            .expect_err("a new call must be rejected during a readiness check");
        assert_eq!(call.0, axum::http::StatusCode::CONFLICT);
        assert!(call.1.contains("voice_self_check_in_progress"));
        assert_eq!(
            manager.begin_model_write().await.unwrap_err(),
            "voice_self_check_in_progress"
        );
        manager.self_check_active.store(false, Ordering::Release);
    }

    #[tokio::test]
    async fn cancelled_readiness_check_releases_admission_and_cancels_daemon_work() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = populate_checked_suite(dir.path());
        let models = Arc::new(nomifun_voice_models::ModelManager::with_config(
            dir.path(),
            Vec::new(),
            manifest,
        ));
        let cancels = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cancels_for_route = cancels.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route(
                "/api/voice/diagnostics/tts-self-check",
                axum::routing::post(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    axum::Json(serde_json::json!({
                        "ok": true, "provider": "fixture", "online": true, "playback_ok": true,
                        "languages": [
                            {"language":"zh-CN","voice":"zh","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50},
                            {"language":"en-US","voice":"en","ok":true,"sample_rate":24000,"samples":2400,"duration_ms":100,"rms_milli":30,"peak_milli":50}
                        ]
                    }))
                }),
            )
            .route(
                "/api/voice/diagnostics/tts-self-check/cancel",
                axum::routing::post(move || {
                    let cancels = cancels_for_route.clone();
                    async move {
                        cancels.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        axum::Json(serde_json::json!({ "cancelled": true }))
                    }
                }),
            );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(models, dir.path())
                .with_replay_daemon(origin)
                .with_dialogue(Arc::new(ConfigurableCompleter {
                    revision: Arc::new(std::sync::Mutex::new("rev".into())),
                })),
        );
        capability_snapshot(&manager, nomifun_common::now_ms());
        let check_manager = manager.clone();
        let task = tokio::spawn(async move { check_manager.run_self_check(false).await });
        for _ in 0..50 {
            if manager.self_check_active.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        manager.cancel_self_check();
        let _ = task.await;
        for _ in 0..50 {
            if !manager.self_check_active.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!manager.self_check_active.load(Ordering::Acquire));
        for _ in 0..50 {
            if cancels.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(cancels.load(std::sync::atomic::Ordering::SeqCst) > 0);
        server.abort();
    }

    #[tokio::test]
    async fn concurrent_self_check_does_not_queue_another_check() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let guard = manager.self_check_lock.lock().await;
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            manager.run_self_check(false),
        ).await;
        drop(guard);
        assert_eq!(result.expect("overlapping check must be rejected promptly").unwrap_err(), "self_check_in_progress");
    }

    #[test]
    fn wake_rejection_is_terminal_only_for_known_pre_session_failures() {
        use axum::http::StatusCode;
        assert!(wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, r#"{"success":false,"error":"models_unavailable"}"#));
        assert!(wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, r#"{"success":false,"error":"microphone_unavailable"}"#));
        assert!(wake_rejection_precedes_session(StatusCode::FORBIDDEN, r#"{"success":false,"error":"pairing_required"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::CONFLICT, r#"{"success":false,"error":"session_busy"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::INTERNAL_SERVER_ERROR, r#"{"success":false,"error":"models_unavailable"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, r#"{"error":"models_unavailable"}"#));
        assert!(!wake_rejection_precedes_session(StatusCode::SERVICE_UNAVAILABLE, "unavailable"));
    }

    #[tokio::test]
    async fn capabilities_expire_with_generation_and_ttl_without_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report,
            attempt: 1,
            expires_at_ms: nomifun_common::now_ms() - 1,
            signature: String::new(),
        });
        assert!(
            manager.current_capabilities().await.is_none(),
            "an expired report must not authorize a call"
        );

        let report = nomifun_api_types::voice::VoiceCapabilityReport {
            generation: manager.generation.load(Ordering::Acquire),
            can_start_full_call: true,
            ..Default::default()
        };
        *manager.capabilities.write().unwrap() = Some(StoredCapabilities {
            report,
            attempt: 2,
            expires_at_ms: nomifun_common::now_ms() + 60_000,
            signature: manager.capability_signature().await,
        });
        manager.generation.fetch_add(1, Ordering::AcqRel);
        assert!(
            manager.current_capabilities().await.is_none(),
            "a daemon generation change must invalidate the previous evidence"
        );
    }

    #[tokio::test]
    async fn a_connected_but_incomplete_suite_rejects_before_any_placeholder() {
        struct GateRecorder {
            resolutions: std::sync::atomic::AtomicUsize,
        }
        #[async_trait::async_trait]
        impl VoiceDialogueRecorder for GateRecorder {
            async fn preflight(&self, _: &VoiceDialogueRequest) -> Result<VoiceDialoguePreflight, VoiceDialogueRecorderError> {
                unreachable!("the admission gate must run before preflight")
            }
            async fn record_pair(&self, _: &str, _: &VoiceDialogueRequest, _: &str) -> Result<(), VoiceDialogueRecorderError> {
                Ok(())
            }
            async fn resolve_call_target_with_locale(&self, _: &VoiceCallTarget, _: Option<&str>, _: Option<&str>, _: Option<&str>) -> Result<(String, Option<String>), VoiceDialogueRecorderError> {
                self.resolutions.fetch_add(1, Ordering::SeqCst);
                Ok(("conversation".into(), None))
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let recorder = Arc::new(GateRecorder { resolutions: std::sync::atomic::AtomicUsize::new(0) });
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        )
        .with_recorder(recorder.clone());
        let builder = if cfg!(windows) {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
            builder.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
            builder
        } else {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("sleep");
            builder.arg("10");
            builder
        };
        if let Some(child) = builder.spawn_managed().ok() {
            *manager.child.lock().await = Some(child);
        }
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.acoustic_ready = true;
            snapshot.dialogue_ready = true;
            snapshot.playback_ready = false;
        }
        let error = manager
            .start_call(VoiceCallStartRequest { locale: None,
                request_id: "gate-1".into(),
                target: VoiceCallTarget::DefaultWorkspace,
                session_mode: None,
            })
            .await
            .expect_err("a connected suite without playback must reject a new full call");
        assert_eq!(error.0, axum::http::StatusCode::CONFLICT);
        assert!(error.1.contains("voice_suite_not_ready"));
        assert_eq!(
            recorder.resolutions.load(Ordering::SeqCst),
            0,
            "no Conversation placeholder may be created before the gate passes"
        );
        assert!(recorder.current_call_reservation().is_none());
    }

    #[tokio::test]
    async fn model_write_rejects_an_active_call_without_touching_the_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.session_id = Some("active-session".into());
            snapshot.phase = VoiceSessionPhase::Listening;
        }
        assert_eq!(
            manager.begin_model_write().await.unwrap_err(),
            "voice_suite_in_use"
        );
        assert!(!manager.resource_write_active.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn http_model_write_is_rejected_while_a_call_is_active() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.session_id = Some("active-session".into());
            snapshot.phase = VoiceSessionPhase::Listening;
        }
        let app = nomifun_voice_models::voice_model_routes(manager.models_manager.clone()).layer(
            axum::middleware::from_fn_with_state(
                manager.clone(),
                model_write_guard_middleware,
            ),
        );
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/api/voice/models/install")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "voice_suite_in_use");
        assert!(json["error"].as_str().unwrap().contains("voice_suite_in_use"));
    }

    #[tokio::test]
    async fn model_write_stops_an_idle_daemon_and_blocks_new_calls() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let builder = if cfg!(windows) {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
            builder.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
            builder
        } else {
            let mut builder = nomi_process_runtime::ChildProcessBuilder::new("sleep");
            builder.arg("10");
            builder
        };
        *manager.child.lock().await = builder.spawn_managed().ok();
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.acoustic_ready = true;
            snapshot.dialogue_ready = true;
            snapshot.playback_ready = true;
        snapshot.worker_epoch = Some(7);
        }

        let guard = manager.begin_model_write().await.unwrap();
        assert!(manager.resource_write_active.load(Ordering::Acquire));
        assert!(
            manager.child.lock().await.is_none(),
            "the idle daemon must release its native model handles before the write"
        );
        let error = manager
            .start_call(VoiceCallStartRequest { locale: None,
                request_id: "during-write".into(),
                target: VoiceCallTarget::DefaultWorkspace,
                session_mode: None,
            })
            .await
            .expect_err("a new call must be rejected while models are being written");
        assert_eq!(error.0, axum::http::StatusCode::CONFLICT);
        assert!(error.1.contains("voice_resource_write_in_progress"));
        drop(guard);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!manager.resource_write_active.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn model_write_does_not_restart_after_the_user_disables_voice() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.write_autostart_config(true).unwrap();
        let guard = manager.begin_model_write().await.unwrap();
        // The user turns the switch off while the write is still running.
        manager.write_autostart_config(false).unwrap();
        drop(guard);
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!manager.resource_write_active.load(Ordering::Acquire));
        assert!(
            manager.child.lock().await.is_none(),
            "a disabled preference must not be undone by the write completion path"
        );
    }

    #[tokio::test]
    async fn voice_foreign_result_and_approval_are_rejected_before_notification_side_effects() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        {
            let mut snapshot = manager.snapshot.write().unwrap();
            snapshot.enabled = true;
            snapshot.connection = VoiceConnectionState::Connected;
            snapshot.session_id = Some("session-b".into());
            snapshot.binding = Some(nomifun_api_types::voice::VoiceBindingSnapshot {
                session_id: "session-b".into(), binding_id: "binding-b".into(), conversation_id: "conversation-b".into(),
                workspace: None, memory_owner_companion_id: None,
            });
        }
        let task = VoiceTaskSnapshot {
            command_id: "command-a".into(), session_id: "session-a".into(), acoustic_turn_id: "turn-a".into(),
            conversation_id: Some("conversation-a".into()), message_id: Some("message-a".into()), runtime_turn_id: None,
            status: VoiceTaskStatus::Completed, result_ok: Some(true), result_text: Some("A private result".into()), error_code: None,
        };
        assert!(manager.notify_task_result(&task, "A private goal").await.is_ok());
        let approval = VoiceToolConfirmation {
            approval_id: "approval-a".into(), msg_id: "message-a".into(), call_id: "call-a".into(), tool_name: "Write".into(),
            arguments: serde_json::json!({"path":"a.txt"}), expires_at_ms: i64::MAX,
        };
        assert!(manager.notify_confirmation(&task, &approval).await.is_ok());
        assert!(manager.prompted_approvals.lock().unwrap().is_empty());
        assert_eq!(manager.snapshot().session_id.as_deref(), Some("session-b"));
        assert!(manager.snapshot().assistant_text.is_none());
    }

    #[test]
    fn audio_recovery_retries_devices_with_backoff_without_restarting_model_failures_or_speech() {
        use nomifun_api_types::voice::{VoiceAudioDiagnostics,VoiceSessionEndReason};
        let mut retry=AudioRecovery::default();
        let mut state=VoiceSessionSnapshot {enabled:true,connection:VoiceConnectionState::Connected,
            phase:VoiceSessionPhase::Idle,acoustic_ready:false,playback_ready:true,
            audio:Some(VoiceAudioDiagnostics {capture_state:"closed".into(),..Default::default()}),..Default::default()};
        assert!(!retry.due(&state,1_000));
        assert!(!retry.due(&state,2_999));
        assert!(retry.due(&state,3_000));
        assert!(!retry.due(&state,6_999));
        assert!(retry.due(&state,7_000));
        state.phase=VoiceSessionPhase::Speaking;
        assert!(!retry.due(&state,20_000),"do not interrupt foreground speech");
        state.phase=VoiceSessionPhase::Idle;state.ended_reason=Some(VoiceSessionEndReason::UserStop);
        assert!(!retry.due(&state,20_000),"explicit stop must not be undone by recovery");
        state.ended_reason=None;state.enabled=false;
        assert!(!retry.due(&state,20_000));
        state.enabled=true;state.acoustic_ready=true;state.audio.as_mut().unwrap().capture_state="receiving".into();
        state.error=Some(VoiceSessionError {stage:"dialogue".into(),code:"model_network_failed".into()});
        assert!(!retry.due(&state,20_000),"model/TTS network failures are not device failures");
        state.playback_ready=false;
        state.audio.as_mut().unwrap().speech_detected=true;
        assert!(!retry.due(&state,21_000),"idle input speech must also be respected");
        state.audio.as_mut().unwrap().speech_detected=false;
        assert!(!retry.due(&state,21_000));
        assert!(retry.due(&state,23_000),"a disconnected speaker must also be reopened");
        state.playback_ready=true;state.audio.as_mut().unwrap().capture_state="no_pcm".into();
        state.audio.as_mut().unwrap().last_frame_at_ms=Some(24_000);
        assert!(!retry.due(&state,25_000),"brief capture gaps must not cause process churn");
        assert!(!retry.due(&state,30_000));
        assert!(retry.due(&state,32_000));
    }

    #[tokio::test]
    async fn automatic_audio_restart_cannot_revoke_host_shutdown_or_disable_intent() {
        let dir=tempfile::tempdir().unwrap();
        let manager=Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path()));
        manager.write_autostart_config(true).unwrap();
        manager.stop().await.unwrap();
        assert_eq!(manager.start_child(false).await.unwrap_err(),"voice_stopped");
        assert!(manager.shutdown_requested.load(Ordering::Acquire));
        manager.shutdown_requested.store(false,Ordering::Release);
        manager.write_autostart_config(false).unwrap();
        assert_eq!(manager.start_child(false).await.unwrap_err(),"voice_stopped");
        assert!(!manager.status().await.running);
    }

    #[test]
    fn audio_recovery_keeps_readable_results_but_rejects_old_session_controls() {
        let dir=tempfile::tempdir().unwrap();
        let manager=VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path());
        manager.write_autostart_config(true).unwrap();
        let old=VoiceSessionSnapshot {instance_id:"old-daemon".into(),seq:1,session_id:Some("old-session".into()),turn_id:Some("old-turn".into()),
            user_text:Some("读取文件".into()),assistant_text:Some("文件里的真实结果".into()),
            conversation:vec![nomifun_api_types::voice::VoiceConversationTurn {session_id:Some("old-session".into()),turn_id:"old-turn".into(),user_text:Some("读取文件".into()),assistant_text:Some("文件里的真实结果".into()),..Default::default()}],..Default::default()};
        manager.project(old,0);
        let old_scope=VoiceSessionControlRequest {instance_id:manager.snapshot().instance_id,session_id:Some("old-session".into()),turn_id:Some("old-turn".into())};
        manager.generation.store(1,Ordering::Release);
        for seq in [1,2] {
            manager.project(VoiceSessionSnapshot {instance_id:"recovered-daemon".into(),seq,acoustic_ready:true,playback_ready:true,..Default::default()},1);
            let fresh=manager.snapshot();
            assert_eq!(fresh.assistant_text.as_deref(),Some("文件里的真实结果"),"recovery must preserve readable text through subsequent audio snapshots");
            assert_eq!(fresh.conversation.len(),1);
            assert!(!old_scope.matches(&fresh),"preserved text cannot restore old control authority");
            assert!(fresh.session_id.is_none() && fresh.turn_id.is_none());
        }
        manager.project(VoiceSessionSnapshot {instance_id:"recovered-daemon".into(),seq:3,session_id:Some("new-session".into()),phase:VoiceSessionPhase::Listening,..Default::default()},1);
        assert!(manager.snapshot().conversation.is_empty());
        assert!(manager.snapshot().assistant_text.is_none());
    }

    #[tokio::test]
    async fn execution_settings_report_unconfigured_without_initializing_runtime_or_audio() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let response = voice_companion_routes(manager.clone())
            .oneshot(axum::http::Request::builder().uri("/api/voice/companion/execution").body(axum::body::Body::empty()).unwrap())
            .await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let settings: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(settings["requested"], serde_json::Value::Null);
        assert_eq!(settings["effective"], serde_json::Value::Null);
        assert_eq!(settings["available"], false);
        assert_eq!(settings["reason_code"], "execution_unconfigured");
        assert!(!manager.status().await.running);
        assert!(!dir.path().join("config/voice_companion.json").exists());
    }

    #[tokio::test]
    async fn submitting_a_voice_task_without_a_runtime_reports_unavailable_instead_of_acceptance() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path(),
        ));
        let request = serde_json::json!({
            "instance_id": manager.snapshot().instance_id,
            "command_id": "not-submitted", "session_id": "session", "turn_id": "turn", "goal": "读取文件"
        });
        let response = voice_companion_routes(manager.clone()).oneshot(axum::http::Request::builder()
            .method("POST").uri("/api/voice/companion/tasks").header("content-type", "application/json")
            .body(axum::body::Body::from(request.to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "runtime_unavailable");
        assert!(body.get("task").is_none());
        assert!(!manager.status().await.running);
    }

    #[tokio::test]
    async fn bv_09_foreground_3s_timeout_returns_unknown_without_cancelling_host_admission() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        struct SlowRuntime {
            created: Arc<std::sync::atomic::AtomicBool>,
        }
        #[async_trait::async_trait]
        impl VoiceTaskRuntime for SlowRuntime {
            async fn resolve_configuration(&self, config: &VoiceExecutionConfig) -> Result<VoiceExecutionConfig, String> { Ok(config.clone()) }
            async fn prepare_configuration(&self, config: &VoiceExecutionConfig) -> Result<(VoiceExecutionConfig, Option<nomifun_api_types::ResolvedPresetSnapshot>), String> { Ok((config.clone(), None)) }
            async fn create(&self, _: &str, _: &VoiceExecutionConfig, _: Option<&nomifun_api_types::ResolvedPresetSnapshot>, _: &str) -> Result<String, crate::voice_tasks::TaskMutationError> {
                tokio::time::sleep(std::time::Duration::from_millis(3500)).await;
                self.created.store(true, Ordering::SeqCst);
                Ok("conv-slow".into())
            }
            async fn create_bound(&self, owner: &str, config: &VoiceExecutionConfig, preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>, creation_key: &str, target: Option<&str>) -> Result<String, crate::voice_tasks::TaskMutationError> {
                if let Some(target_id) = target {
                    tokio::time::sleep(std::time::Duration::from_millis(3500)).await;
                    self.created.store(true, Ordering::SeqCst);
                    return Ok(target_id.into());
                }
                self.create(owner, config, preset, creation_key).await
            }
            async fn find_created(&self, _: &str, _: &str) -> Result<Option<String>, String> { Ok(Some("conv-slow".into())) }
            async fn append(&self, _: &str, _: &str, _: &str, _: &str, _: &str) -> Result<(), crate::voice_tasks::TaskMutationError> { Ok(()) }
            async fn send_bound(
                &self,
                owner: &str,
                conversation_id: &str,
                send_key: &str,
                message_id: &str,
                _command_id: &str,
                _display_text: &str,
                goal: &str,
                _context_digest: Option<&str>,
                _frozen_config: Option<&VoiceExecutionConfig>,
                _preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>,
            ) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                let mut receipt = self.send(owner, conversation_id, send_key, goal).await?;
                receipt.message_id = message_id.to_string();
                Ok(receipt)
            }
            async fn send(&self, _: &str, _: &str, _: &str, _: &str) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                Ok(crate::voice_tasks::TaskReceipt {
                    message_id: "msg-slow".into(),
                    runtime_turn_id: None,
                    replayed: false,
                    completed: false,
                    result_ok: None,
                    result_text: None,
                    error_code: None,
                })
            }
            async fn observe(&self, _: &str, _: &str, _: &str) -> Result<Option<crate::voice_tasks::TaskReceipt>, String> { Ok(None) }
            async fn cancel(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> { Ok(()) }
            async fn confirmations(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Vec<VoiceToolConfirmation>, String> { Ok(Vec::new()) }
            async fn confirm(&self, _: &str, _: &str, _: &str, _: &str, _: &VoiceToolConfirmation, _: bool) -> Result<(), String> { Ok(()) }
        }

        struct BoundRecorder(VoiceExecutionConfig);
        #[async_trait::async_trait]
        impl VoiceDialogueRecorder for BoundRecorder {
            async fn task_configuration(&self, command: &nomifun_api_types::voice::VoiceTaskCommand) -> Result<Option<crate::voice_dialogue::FrozenVoiceExecution>, String> {
                assert_eq!(command.session_id, "session-slow");
                Ok(Some(crate::voice_dialogue::FrozenVoiceExecution {config:self.0.clone(),preset:None,conversation_id:Some("conv-slow".into()),message_id:Some("msg-slow".into()),handoff_content:None,display_text:Some(command.goal.clone()),voice_context_digest:None}))
            }
            async fn preflight(&self, _: &VoiceDialogueRequest) -> Result<crate::voice_dialogue::VoiceDialoguePreflight, VoiceDialogueRecorderError> { unreachable!() }
            async fn record_pair(&self, _: &str, _: &VoiceDialogueRequest, _: &str) -> Result<(), VoiceDialogueRecorderError> { unreachable!() }
        }
        let recorder = Arc::new(BoundRecorder(VoiceExecutionConfig {
            workspace:dir.path().to_string_lossy().into_owned(),model:Some(VoiceModelRef {provider_id:"test-p".into(),model:"test-m".into()}),preset_id:None,session_mode:None,
        }));
        let created_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let runtime = Arc::new(SlowRuntime { created: created_flag.clone() });
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path(),
        ).with_tasks("test-owner".into(), runtime).with_recorder(recorder));
        manager.write_autostart_config(true).unwrap();

        let generation = manager.generation.load(Ordering::Acquire);
        let instance_id = manager.snapshot().instance_id;
        manager.project(VoiceSessionSnapshot {
            instance_id: "daemon-slow".into(),
            seq: 1,
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Deciding,
            session_id: Some("session-slow".into()),
            turn_id: Some("turn-slow".into()),
            ..Default::default()
        }, generation);

        let request = serde_json::json!({
            "instance_id": instance_id,
            "command_id": "command-slow-1",
            "session_id": "session-slow",
            "turn_id": "turn-slow",
            "goal": "执行耗时任务"
        });

        let start = std::time::Instant::now();
        let response = voice_companion_routes(manager.clone()).oneshot(axum::http::Request::builder()
            .method("POST").uri("/api/voice/companion/tasks").header("content-type", "application/json")
            .body(axum::body::Body::from(request.to_string())).unwrap()).await.unwrap();

        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() >= 2800 && elapsed.as_millis() < 4200, "elapsed: {:?}", elapsed);
        assert_eq!(response.status(), axum::http::StatusCode::ACCEPTED);

        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let delivery: VoiceTaskDelivery = serde_json::from_slice(&body).unwrap();
        assert!(!delivery.freshly_accepted, "timed-out foreground response must not claim freshly accepted");
        assert_eq!(delivery.task.command_id, "command-slow-1");
        assert!(matches!(delivery.task.status, VoiceTaskStatus::Unknown | VoiceTaskStatus::Delivering));

        // The background task continues without cancellation
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        assert!(created_flag.load(Ordering::SeqCst), "host background admission must complete even after foreground timeout");
    }

    #[tokio::test]
    async fn retired_configuration_writes_are_rejected_and_legacy_values_are_ignored() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path()));
        manager.write_config_entry("execution", serde_json::json!({"workspace":dir.path(),"model":{"provider_id":"old","model":"old"}})).unwrap();
        manager.write_config_entry("future", serde_json::json!({"keep":true})).unwrap();
        let saved = std::fs::read(&manager.config_path).unwrap();
        let app = voice_companion_routes(manager.clone());
        for path in ["/api/voice/companion/execution", "/api/voice/companion/config"] {
            let response = app.clone().oneshot(axum::http::Request::builder().method("POST").uri(path).header("content-type","application/json")
                .body(axum::body::Body::from(r#"{"workspace":"relative","dialogue_model":null}"#)).unwrap()).await.unwrap();
            assert_eq!(response.status(),axum::http::StatusCode::METHOD_NOT_ALLOWED);
        }
        let settings = manager.execution_settings().await;
        assert!(settings.requested.is_none() && settings.effective.is_none() && !settings.available);
        assert_eq!(std::fs::read(&manager.config_path).unwrap(),saved);
        assert!(!manager.status().await.running);
    }

    #[tokio::test]
    async fn dialogue_model_configuration_is_available_without_starting_audio() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let response = voice_companion_routes(manager)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/voice/companion/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::OK,
            "voice must expose a provider/model reference, not require a second API key"
        );
    }

    #[tokio::test]
    async fn retired_dialogue_override_is_ignored_after_restart_and_preferences_are_preserved() {
        struct Models;
        #[async_trait::async_trait]
        impl VoiceDialogueCompleter for Models {
            async fn resolve_model(&self, choice: Option<&VoiceModelRef>) -> Result<VoiceModelRef,String> {
                assert!(choice.is_none(), "production model resolution must be automatic");
                Ok(VoiceModelRef {provider_id:"catalog".into(),model:"flash".into()})
            }
            async fn complete(&self, _: &str, choice: Option<&VoiceModelRef>) -> Result<String,String> {
                assert!(choice.is_none(), "actual bridge must not send the retired override");
                Ok(r#"{"intent_type":"chat","reply_tts":"hello"}"#.into())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/voice_companion.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = serde_json::json!({"provider_id":"deleted","model":"deleted"});
        std::fs::write(&path,serde_json::json!({"dialogue_model":old,"future":true,"autostart":true}).to_string()).unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path()).with_dialogue(Arc::new(Models)));
        let settings = manager.dialogue_settings().await;
        assert!(settings.config.dialogue_model.is_none());
        assert_eq!(settings.effective_model.unwrap().model,"flash");
        let bridge = manager.start_dialogue_bridge().await.unwrap();
        let response = reqwest::Client::builder().no_proxy().build().unwrap().post(&bridge.endpoint).header("x-ai-mate-control-token", &manager.control_token)
            .json(&VoiceDialogueRequest::new("hello")).send().await.unwrap();
        assert!(response.status().is_success(), "actual private completion route must use automatic selection");
        assert!(response.text().await.unwrap().contains("hello"));
        manager.write_autostart_config(false).unwrap();
        let saved:serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["dialogue_model"],old);
        assert_eq!(saved["future"],true);
        assert_eq!(saved["autostart"],false);
    }

    #[test]
    fn test_voice_companion_status_serialization() {
        let status = VoiceCompanionStatus {
            running: true,
            pid: Some(12345),
            models_ready: true,
            autostart: true,
            error: None,
            acoustic_ready: false,
            dialogue_ready: false,
            playback_ready: false,
            task_ready: false,
            capabilities: None,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"pid\":12345"));
        assert!(json.contains("\"models_ready\":true"));

        let deserialized: VoiceCompanionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.running, true);
        assert_eq!(deserialized.pid, Some(12345));
    }

    #[tokio::test]
    async fn feedback_patch_preserves_model_autostart_unknown_fields_and_rejects_invalid_volume() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        manager.write_autostart_config(true).unwrap();
        let selected = serde_json::json!({"provider_id":"existing","model":"configured"});
        manager
            .write_config_entry("dialogue_model", selected.clone())
            .unwrap();
        manager
            .write_config_entry("future_field", serde_json::json!({"keep":true}))
            .unwrap();
        let first = manager
            .update_feedback(
                serde_json::from_value(serde_json::json!({"volume":23,"speak_replies":false}))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            first.effective.is_none(),
            "saving while stopped cannot claim live application"
        );
        let updated = manager
            .update_feedback(
                serde_json::from_value(serde_json::json!({"mode":"essential"})).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated.requested.volume, 23);
        assert!(!updated.requested.speak_replies);
        assert_eq!(
            updated.requested.mode,
            nomifun_api_types::voice::VoiceFeedbackMode::Essential
        );
        let before = std::fs::read(&manager.config_path).unwrap();
        assert!(
            manager
                .update_feedback(serde_json::from_value(serde_json::json!({"volume":101})).unwrap())
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&manager.config_path).unwrap(), before);
        let saved: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(saved["dialogue_model"], selected);
        assert_eq!(saved["future_field"]["keep"], true);
        assert_eq!(saved["autostart"], true);
        let reloaded = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        assert_eq!(reloaded.requested_feedback().unwrap(), updated.requested);
    }

    #[tokio::test]
    async fn toggle_waits_for_the_prepare_lifecycle_lock() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let guard = manager.lifecycle_lock.lock().await;
        let toggle = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.toggle(false).await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!toggle.is_finished(), "toggle must wait for prepare's lifecycle lock");
        drop(guard);
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), toggle)
            .await
            .expect("toggle must resume after prepare releases the lock")
            .expect("toggle task must not panic");
        assert!(result.is_ok());
        assert!(!manager.read_autostart_config());
    }

    #[test]
    fn test_autostart_config_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let models_mgr = Arc::new(nomifun_voice_models::ModelManager::new(
            temp_dir.path().to_path_buf(),
        ));
        let manager = VoiceCompanionProcessManager::new(models_mgr, temp_dir.path());

        assert_eq!(manager.read_autostart_config(), false);
        manager.write_autostart_config(true).unwrap();
        assert_eq!(manager.read_autostart_config(), true);
        manager.write_autostart_config(false).unwrap();
        assert_eq!(manager.read_autostart_config(), false);
    }

    #[tokio::test]
    async fn invalid_existing_feedback_configuration_is_not_replaced_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        );
        manager.write_autostart_config(true).unwrap();
        for raw in [
            "[]",
            r#"{"feedback":{"volume":101},"autostart":true}"#,
            "invalid-json",
        ] {
            std::fs::write(&manager.config_path, raw).unwrap();
            assert!(manager.feedback_settings().await.is_err());
            assert!(
                manager
                    .update_feedback(VoiceFeedbackPatch::default())
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read_to_string(&manager.config_path).unwrap(), raw);
        }
    }

    #[test]
    fn test_resolve_ai_mate_binary_candidate_search() {
        let resolved = VoiceCompanionProcessManager::resolve_voice_companion_binary();
        if let Some(path) = resolved {
            assert!(path.is_file());
            assert!(path.to_string_lossy().contains("voice-companion") || path.to_string_lossy().contains("ai-mate"));
        }
    }

    #[test]
    fn installed_voice_binary_uses_the_desktop_resource_directory_without_a_checkout() {
        let root=tempfile::tempdir().unwrap();
        let bin=root.path().join("Resources/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let old_name=if cfg!(windows){"ai-mate.exe"}else{"ai-mate"};
        let daemon=bin.join(old_name);std::fs::write(&daemon,b"test-only executable fixture").unwrap();
        let host=root.path().join("MacOS/Flowy");
        assert_eq!(VoiceCompanionProcessManager::installed_ai_mate_binary(Some(&bin),Some(&host)),Some(daemon.clone()));
        assert!(VoiceCompanionProcessManager::installed_ai_mate_binary(None,Some(&host)).is_none());
        let neighboring=root.path().join(old_name);std::fs::write(&neighboring,b"test-only colocated fixture").unwrap();
        assert_eq!(VoiceCompanionProcessManager::installed_ai_mate_binary(None,Some(&root.path().join("Flowy.exe"))),Some(neighboring));
        // The packaged name wins over the legacy ai-mate name.
        let current=bin.join(if cfg!(windows){"voice-companion.exe"}else{"voice-companion"});
        std::fs::write(&current,b"test-only current fixture").unwrap();
        assert_eq!(VoiceCompanionProcessManager::installed_voice_companion_binary(Some(&bin),Some(&host)),Some(current));
    }

    #[tokio::test]
    async fn missing_voice_binary_reports_an_actionable_error_without_restart_noise() {
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        manager.write_autostart_config(true).unwrap();
        let mut events = manager.subscribe();
        manager.note_missing_voice_binary();
        let snapshot = events.try_recv().unwrap();
        assert_eq!(snapshot.phase, VoiceSessionPhase::Error);
        assert_eq!(snapshot.connection, VoiceConnectionState::Offline);
        assert_eq!(snapshot.error.unwrap().code, VOICE_COMPANION_MISSING);
        assert!(manager.snapshot().enabled);
        manager.note_missing_voice_binary();
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn host_stream_recovers_unchanged_daemon_snapshot_and_fences_previous_process() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(
            dir.path().to_path_buf(),
        ));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();
        let mut events = manager.subscribe();
        let daemon = VoiceSessionSnapshot {
            instance_id: "daemon-a".into(),
            seq: 5,
            phase: VoiceSessionPhase::Listening,
            session_id: Some("session-a".into()),
            acoustic_ready: true,
            ..Default::default()
        };
        manager.project(daemon.clone(), 0);
        let first = events.try_recv().unwrap();
        assert_ne!(first.instance_id, daemon.instance_id);
        assert!(first.enabled && first.acoustic_ready);
        manager.project(daemon.clone(), 0);
        assert!(events.try_recv().is_err());
        manager.disconnected();
        assert!(!events.try_recv().unwrap().acoustic_ready);
        manager.project(daemon.clone(), 0);
        let restored = events.try_recv().unwrap();
        assert_eq!(restored.connection, VoiceConnectionState::Connected);
        assert!(restored.seq > first.seq);
        manager.generation.store(1, Ordering::Release);
        manager.project(VoiceSessionSnapshot { seq: 99, ..daemon }, 0);
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn host_shutdown_preserves_enabled_intent() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(
            dir.path().to_path_buf(),
        ));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();
        manager.stop().await.unwrap();
        assert!(manager.read_autostart_config());
        assert!(manager.shutdown_requested.load(Ordering::Acquire));
        manager.toggle(false).await.unwrap();
        assert!(!manager.read_autostart_config());
    }

    #[tokio::test]
    async fn bv_01_target_settings_defaults_to_voice_home_without_starting_audio() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let app = voice_companion_routes(manager.clone());
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/voice/companion/target")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 16_384).await.unwrap();
        let settings: VoiceTargetSettings = serde_json::from_slice(&body).unwrap();
        assert_eq!(settings.requested.mode, VoiceTargetMode::VoiceHome);
        assert_eq!(settings.effective.mode, VoiceTargetMode::VoiceHome);
        assert!(settings.effective_conversation_id.is_none());
        assert_eq!(settings.reason.as_deref(), Some("workspace_routing_active"));
        assert!(!manager.status().await.running);
    }

    #[tokio::test]
    async fn target_update_and_candidate_endpoints_work() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let app = voice_companion_routes(manager.clone());

        // POST /target with missing companion_id when companion mode requested returns 400
        let req_missing = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/target")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "mode": "companion" }).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req_missing).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);

        // POST /target with voice_home returns 200
        let req_home = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/target")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "mode": "voice_home" }).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req_home).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);

        // HTTP cannot attest native window focus
        let req_candidate = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/candidate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "conversation_id": "candidate-c1" }).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req_candidate).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(
            manager.active_window_candidate().read().unwrap().conversation_id.as_deref(),
            None
        );

        // POST /candidate with null clears candidate conversation
        let req_clear = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/candidate")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::json!({ "conversation_id": null }).to_string()))
            .unwrap();
        let resp = app.oneshot(req_clear).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::FORBIDDEN);
        assert_eq!(
            manager.active_window_candidate().read().unwrap().conversation_id.as_deref(),
            None
        );
    }

    #[test]
    fn test_task_feedback_tracker_throttling_and_stage_reset() {
        let mut tracker = TaskFeedbackTracker::new();
        let mut task = VoiceTaskSnapshot {
            command_id: "cmd-1".into(),
            session_id: "session-1".into(),
            acoustic_turn_id: "turn-1".into(),
            conversation_id: Some("conv-1".into()),
            message_id: Some("msg-1".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };

        let t0 = 100_000_i64;
        // At t=0, task just accepted, no feedback due yet
        assert_eq!(tracker.evaluate(&task, t0), TaskFeedbackDecision::None);

        // Before 15s (t0 + 14_999), still no feedback
        assert_eq!(tracker.evaluate(&task, t0 + 14_999), TaskFeedbackDecision::None);

        // At 15s (t0 + 15_000), first waiting feedback is due
        let decision = tracker.evaluate(&task, t0 + 15_000);
        assert_eq!(
            decision,
            TaskFeedbackDecision::TriggerFeedback {
                command_id: "cmd-1".into(),
                stage: "processing".into(),
            }
        );
        // Record dispatched
        tracker.record_dispatched(t0 + 15_000);

        // 15s after first dispatch (t0 + 30_000), throttled (needs 45s)
        assert_eq!(tracker.evaluate(&task, t0 + 30_000), TaskFeedbackDecision::None);

        // 44.999s after first dispatch (t0 + 59_999), still throttled
        assert_eq!(tracker.evaluate(&task, t0 + 59_999), TaskFeedbackDecision::None);

        // 45s after first dispatch (t0 + 60_000), second feedback is due
        assert_eq!(
            tracker.evaluate(&task, t0 + 60_000),
            TaskFeedbackDecision::TriggerFeedback {
                command_id: "cmd-1".into(),
                stage: "processing".into(),
            }
        );
        tracker.record_dispatched(t0 + 60_000);

        // 45s after second dispatch (t0 + 105_000), max 2 consecutive reached, suppressed
        assert_eq!(tracker.evaluate(&task, t0 + 105_000), TaskFeedbackDecision::None);

        // Even after longer time (t0 + 200_000), same stage remains capped at 2
        assert_eq!(tracker.evaluate(&task, t0 + 200_000), TaskFeedbackDecision::None);

        // When stage changes (e.g. tool execution turn starts), consecutive count resets
        task.runtime_turn_id = Some("tool_exec_read".into());
        assert_eq!(tracker.evaluate(&task,t0+201_000),TaskFeedbackDecision::None,"a turn id is not business progress");
        let progress=nomifun_api_types::voice::VoiceTaskProgress {command_id:task.command_id.clone(),stage:"tool".into(),health:"fresh".into(),last_event_at_ms:Some(t0+201_000),last_progress_at_ms:Some(t0+201_000)};
        let stage_decision = tracker.evaluate_progress(&task,Some(&progress),t0+201_000);
        assert_eq!(
            stage_decision,
            TaskFeedbackDecision::TriggerFeedback {
                command_id: "cmd-1".into(),
                stage: "tool:fresh".into(),
            }
        );
        tracker.record_dispatched(t0 + 201_000);

        // Terminal state clears feedback tracking
        task.status = VoiceTaskStatus::Completed;
        assert_eq!(tracker.evaluate(&task, t0 + 202_000), TaskFeedbackDecision::None);
        assert!(tracker.pending_feedback.is_none());
        assert!(tracker.active_command_id.is_none());
    }

    #[tokio::test]
    async fn bv_12_high_frequency_pcm_and_instance_switch_preserve_task_and_monotonic_seq() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(dir.path().to_path_buf()));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();

        // 1. Establish initial active task and confirmation
        let task = VoiceTaskSnapshot {
            command_id: "cmd-bv12".into(),
            session_id: "session-bv12".into(),
            acoustic_turn_id: "turn-bv12".into(),
            conversation_id: Some("conv-bv12".into()),
            message_id: Some("msg-bv12".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };
        manager.replay_task_projection(task.clone());

        let confirmation = VoiceToolConfirmation {
            approval_id: "app-bv12".into(),
            msg_id: "msg-bv12".into(),
            call_id: "call-bv12".into(),
            tool_name: "WorkspaceFileRead".into(),
            arguments: serde_json::json!({"path": "test.txt"}),
            expires_at_ms: nomifun_common::now_ms() + 30_000,
        };
        manager.project_confirmation(&task, Some(confirmation.clone()));

        // Verify baseline snapshot
        let snap = manager.snapshot();
        assert_eq!(snap.task.as_ref(), Some(&task));
        assert_eq!(snap.confirmation.as_ref(), Some(&confirmation));
        let initial_seq = snap.seq;

        // 2. Blast 100 high-frequency PCM / audio diagnostics snapshots from daemon
        let mut last_seq = initial_seq;
        let daemon_id = "daemon-instance-1";
        for i in 1..=100 {
            let daemon_snap = VoiceSessionSnapshot {
                instance_id: daemon_id.into(),
                seq: i as u64,
                enabled: true,
                connection: VoiceConnectionState::Connected,
                phase: VoiceSessionPhase::Listening,
                session_id: Some("session-bv12".into()),
                turn_id: Some("turn-bv12".into()),
                acoustic_ready: true,
                audio: Some(nomifun_api_types::voice::VoiceAudioDiagnostics {
                    rms_milli: (i * 10) as u16,
                    peak_milli: (i * 20) as u16,
                    frames_received: i as u64,
                    last_frame_at_ms: Some(nomifun_common::now_ms()),
                    capture_state: "active".into(),
                    speech_detected: i % 2 == 0,
                    asr_ready: true,
                    kws_ready: true,
                    vad_ready: true,
                    utterance_samples: i as u32 * 160,
                    utterance_peak_milli: (i * 15) as u16,
                    ..Default::default()
                }),
                ..Default::default()
            };
            manager.project(daemon_snap, 0);

            let current = manager.snapshot();
            // Task and confirmation must NEVER be erased by acoustic updates
            assert_eq!(current.task.as_ref(), Some(&task), "audio update {} cleared task", i);
            assert_eq!(current.confirmation.as_ref(), Some(&confirmation), "audio update {} cleared confirmation", i);
            // Sequence must be strictly monotonic
            assert!(current.seq > last_seq, "seq must strictly increase: {} vs {}", current.seq, last_seq);
            last_seq = current.seq;
        }

        // 3. Stale / out-of-order snapshots with lower seq must be ignored
        let stale = VoiceSessionSnapshot {
            instance_id: daemon_id.into(),
            seq: 50, // lower than 100
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Idle,
            ..Default::default()
        };
        manager.project(stale, 0);
        assert_eq!(manager.snapshot().seq, last_seq, "stale seq must be ignored");

        // 4. Daemon instance switch invalidates previous controls
        let old_instance_id = manager.snapshot().instance_id;
        // Host gets refreshed daemon instance
        manager.project(VoiceSessionSnapshot {
            instance_id: "daemon-instance-2".into(),
            seq: 1,
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Idle,
            ..Default::default()
        }, 0);

        // Control request with old instance ID is rejected
        let stale_control = VoiceSessionControlRequest {
            instance_id: old_instance_id,
            session_id: Some("session-bv12".into()),
            turn_id: Some("turn-bv12".into()),
        };
        let stop_err = manager.stop_session(stale_control).await;
        assert_eq!(stop_err, Err("session_scope_changed".to_string()));
    }

    #[tokio::test]
    async fn bv_13_user_speech_suppresses_progress_and_completion_replaces_old_cues() {
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(dir.path().to_path_buf()));
        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()));
        manager.write_autostart_config(true).unwrap();

        let mut tracker = TaskFeedbackTracker::new();
        let task = VoiceTaskSnapshot {
            command_id: "cmd-bv13".into(),
            session_id: "session-bv13".into(),
            acoustic_turn_id: "turn-bv13".into(),
            conversation_id: Some("conv-bv13".into()),
            message_id: Some("msg-bv13".into()),
            runtime_turn_id: None,
            status: VoiceTaskStatus::Accepted,
            result_ok: None,
            result_text: None,
            error_code: None,
        };

        let t0 = 200_000_i64;
        assert_eq!(tracker.evaluate(&task, t0), TaskFeedbackDecision::None);
        assert_eq!(tracker.evaluate(&task, t0 + 15_000), TaskFeedbackDecision::TriggerFeedback {
            command_id: "cmd-bv13".into(),
            stage: "processing".into(),
        });
        assert_eq!(tracker.pending_feedback.as_deref(), Some("processing"));

        // When task completes while feedback is pending, terminal state immediately clears pending feedback
        let mut completed_task = task.clone();
        completed_task.status = VoiceTaskStatus::Completed;
        completed_task.result_ok = Some(true);
        completed_task.result_text = Some("真实读取结果成功".into());

        assert_eq!(tracker.evaluate(&completed_task, t0 + 16_000), TaskFeedbackDecision::None);
        assert!(tracker.pending_feedback.is_none(), "terminal state must replace and discard old pending cues");

        // Speech suppression: when daemon has speech_detected = true, maybe_notify_task_progress does not dispatch
        manager.replay_acoustic_source(VoiceSessionSnapshot {
            instance_id: manager.snapshot().instance_id,
            seq: 1,
            enabled: true,
            connection: VoiceConnectionState::Connected,
            phase: VoiceSessionPhase::Listening,
            session_id: Some("session-bv13".into()),
            turn_id: Some("turn-bv13".into()),
            audio: Some(nomifun_api_types::voice::VoiceAudioDiagnostics {
                speech_detected: true,
                ..Default::default()
            }),
            ..Default::default()
        });

        // Initialize manager's task_feedback tracker so it would be due if not suppressed
        {
            let mut tracker_guard = manager.task_feedback.lock().unwrap();
            tracker_guard.evaluate(&task, t0);
            tracker_guard.accepted_at_ms = nomifun_common::now_ms() - 20_000;
        }

        // Calling maybe_notify_task_progress while user is speaking returns Ok(()) without dispatching
        let res = manager.maybe_notify_task_progress(&task).await;
        assert!(res.is_ok());
        // Verify tracker was not marked dispatched because speech was active
        let tracker_guard = manager.task_feedback.lock().unwrap();
        assert_eq!(tracker_guard.last_feedback_at_ms, 0, "must not dispatch during user speech");
    }

    #[tokio::test]
    async fn test_task_replay_and_notification_endpoints() {
        use tower::ServiceExt;
        use nomifun_api_types::voice::VoiceTaskCommand;
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(nomifun_voice_models::ModelManager::new(dir.path().to_path_buf()));

        struct StubRuntime;
        #[async_trait::async_trait]
        impl VoiceTaskRuntime for StubRuntime {
            async fn resolve_configuration(&self, config: &VoiceExecutionConfig) -> Result<VoiceExecutionConfig, String> { Ok(config.clone()) }
            async fn prepare_configuration(&self, config: &VoiceExecutionConfig) -> Result<(VoiceExecutionConfig, Option<nomifun_api_types::ResolvedPresetSnapshot>), String> { Ok((config.clone(), None)) }
            async fn create(&self, _: &str, _: &VoiceExecutionConfig, _: Option<&nomifun_api_types::ResolvedPresetSnapshot>, _: &str) -> Result<String, crate::voice_tasks::TaskMutationError> { Ok("conv-stub".into()) }
            async fn create_bound(&self, owner: &str, config: &VoiceExecutionConfig, preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>, creation_key: &str, target: Option<&str>) -> Result<String, crate::voice_tasks::TaskMutationError> {
                if let Some(target_id) = target {
                    return Ok(target_id.into());
                }
                self.create(owner, config, preset, creation_key).await
            }
            async fn find_created(&self, _: &str, _: &str) -> Result<Option<String>, String> { Ok(Some("conv-stub".into())) }
            async fn append(&self, _: &str, _: &str, _: &str, _: &str, _: &str) -> Result<(), crate::voice_tasks::TaskMutationError> { Ok(()) }
            async fn send_bound(
                &self,
                owner: &str,
                conversation_id: &str,
                send_key: &str,
                message_id: &str,
                _command_id: &str,
                _display_text: &str,
                goal: &str,
                _context_digest: Option<&str>,
                _frozen_config: Option<&VoiceExecutionConfig>,
                _preset: Option<&nomifun_api_types::ResolvedPresetSnapshot>,
            ) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                let mut receipt = self.send(owner, conversation_id, send_key, goal).await?;
                receipt.message_id = message_id.to_string();
                Ok(receipt)
            }
            async fn send(&self, _: &str, _: &str, _: &str, _: &str) -> Result<crate::voice_tasks::TaskReceipt, crate::voice_tasks::TaskMutationError> {
                Ok(crate::voice_tasks::TaskReceipt {
                    message_id: "msg-stub".into(),
                    runtime_turn_id: None,
                    replayed: false,
                    completed: true,
                    result_ok: Some(true),
                    result_text: Some("已完成真实读取".into()),
                    error_code: None,
                })
            }
            async fn observe(&self, _: &str, _: &str, _: &str) -> Result<Option<crate::voice_tasks::TaskReceipt>, String> { Ok(None) }
            async fn cancel(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> { Ok(()) }
            async fn confirmations(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Vec<VoiceToolConfirmation>, String> { Ok(Vec::new()) }
            async fn confirm(&self, _: &str, _: &str, _: &str, _: &str, _: &VoiceToolConfirmation, _: bool) -> Result<(), String> { Ok(()) }
        }

        let manager = Arc::new(VoiceCompanionProcessManager::new(models, dir.path()).with_tasks("owner".into(), Arc::new(StubRuntime)));
        let app = voice_companion_routes(manager.clone());

        // Submit task which completes immediately via StubRuntime
        let cmd = VoiceTaskCommand {
            command_id: "cmd-replay-test".into(),
            session_id: "session-replay".into(),
            turn_id: "turn-replay".into(),
            goal: "测试重听".into(),
            binding_id: None,
            record_id: None,
        };
        let cfg = VoiceExecutionConfig {
            workspace: dir.path().to_str().unwrap().into(),
            model: Some(VoiceModelRef { provider_id: "test".into(), model: "test".into() }),
            preset_id: None,
            session_mode: None,
        };
        let tasks = manager.tasks.as_ref().unwrap();
        tasks.submit(cmd, cfg).await.unwrap();

        // 1. Query notification before claim
        let req_notif = axum::http::Request::builder()
            .uri("/api/voice/companion/tasks/cmd-replay-test/notification")
            .body(axum::body::Body::empty()).unwrap();
        let resp = app.clone().oneshot(req_notif).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let notif: VoiceTaskNotification = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 16_384).await.unwrap()).unwrap();
        assert_eq!(notif.command_id, "cmd-replay-test");
        assert_eq!(notif.result_summary, "已完成真实读取");

        // 2. Claim announcement, which generates attempt:1
        tasks.claim_announcement("cmd-replay-test", true).await.unwrap();
        let notif_claimed = tasks.notification_for("cmd-replay-test", None).unwrap().unwrap();
        assert_eq!(notif_claimed.attempt_id, "voice-attempt:cmd-replay-test:1");

        // A playback still marked dispatched cannot be replayed under a new attempt.
        {
            let mut snapshot=manager.snapshot.write().unwrap();snapshot.session_id=Some("session-replay".into());snapshot.turn_id=Some("turn-replay".into());
        }
        let request=|instance:&str,replay:&str|axum::http::Request::builder().method("POST")
            .uri("/api/voice/companion/tasks/cmd-replay-test/replay").header("content-type","application/json")
            .body(axum::body::Body::from(serde_json::json!({"instance_id":instance,"session_id":"session-replay","turn_id":"turn-replay","replay_id":replay}).to_string())).unwrap();
        let old=app.clone().oneshot(request("old-host","replay-one")).await.unwrap();assert_eq!(old.status(),axum::http::StatusCode::CONFLICT);
        let pending=app.clone().oneshot(request(&manager.snapshot().instance_id,"replay-one")).await.unwrap();assert_eq!(pending.status(),axum::http::StatusCode::SERVICE_UNAVAILABLE);
        tasks.finish_announcement("cmd-replay-test",&notif_claimed.attempt_id,"failed").await.unwrap();
        let resp=app.clone().oneshot(request(&manager.snapshot().instance_id,"replay-one")).await.unwrap();assert_eq!(resp.status(),axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let again=app.oneshot(request(&manager.snapshot().instance_id,"replay-one")).await.unwrap();assert_eq!(again.status(),axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(tasks.notification_for("cmd-replay-test", None).unwrap().unwrap().status,"failed","offline replay cannot claim acceptance or change playback history");
    }

    #[test]
    fn test_spoken_summary_concise_text_preserved() {
        let text = "北京今天晴，气温 15 到 25 度，北风 2 级。";
        let summary = extract_spoken_summary(text, VoiceTaskStatus::Completed, false);
        assert_eq!(summary, text);
        assert!(summary.chars().count() <= 80);
    }

    #[test]
    fn result_announcement_uses_call_locale_for_wrappers_and_empty_results() {
        let mut task: VoiceTaskSnapshot = serde_json::from_value(serde_json::json!({
            "command_id":"command", "session_id":"session", "acoustic_turn_id":"turn",
            "conversation_id":"conversation", "status":"completed", "result_ok":true
        })).unwrap();
        assert_eq!(format_task_result_announcement(&task, "", true), "Execution finished. See conversation for details.");
        assert_eq!(format_task_result_announcement(&task, "", false), "执行已结束，详情请在会话中查看。");
        task.result_text = Some("Sorry, could not find any records.".into());
        assert_eq!(format_task_result_announcement(&task, "find records", true), "Result for ‘find records’: Sorry, could not find any records.");
        assert_eq!(format_task_result_announcement(&task, "查询记录", false), "任务「查询记录」的结果：Sorry, could not find any records.");
        task.result_text = Some("The query returned 125 records and the report is available in the requested output directory, with all records sorted by date and ready for review.".into());
        let text = format_task_result_announcement(&task, "report", true);
        assert!(!text.contains("详情") && !text.contains("任务"), "{text}");
        task.status = VoiceTaskStatus::Failed;
        task.result_text = None;
        assert_eq!(format_task_result_announcement(&task, "", true), "Execution was unsuccessful. See conversation for details.");
    }

    #[test]
    fn test_spoken_summary_preserves_english_failure_and_restrictions() {
        let raw = "The deployment validation and health checks completed successfully for all 120 configured records. However the database backup was NOT created and you must NOT delete the previous backup.";
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed, true);
        assert!(!summary.contains("completed success"), "must not speak only the successful preamble: {summary}");
        assert!(summary.to_ascii_lowercase().contains("not") || summary.contains("conditions"), "must preserve the restriction or defer to full details: {summary}");
        assert!(summary.chars().count() <= 80);
        for negative in ["cannot", "can't", "couldn’t", "FAILED", "unable", "must not"] {
            assert!(is_negation_or_failure(&format!("Backup {negative} be completed")));
        }
        assert!(!is_negation_or_failure("A notable success notification"));
        assert_eq!(split_sentences("Backup failed. Keep version 1.25 unchanged."), vec!["Backup failed", "Keep version 1.25 unchanged"]);
        let with_number = "All deployment preparation and validation checks completed successfully across the workspace. Backup failed at 1.25 GB.";
        let summary = extract_spoken_summary(with_number, VoiceTaskStatus::Completed, true);
        assert!(summary.contains("Backup failed at 1.25 GB"), "must keep the failure and decimal: {summary}");
        let no_error = "All deployment preparation and validation checks completed successfully across the workspace. No errors occurred.";
        let summary = extract_spoken_summary(no_error, VoiceTaskStatus::Completed, true);
        assert!(summary.contains("No errors occurred"), "a negation is not proof of a failed task: {summary}");
    }

    #[test]
    fn test_spoken_summary_extracts_conclusion_and_parameters_from_long_markdown() {
        let raw = r#"### 任务执行报告
本次任务已经执行完成。
经过全网检索，查询到北京今天天气为晴天，气温 15℃ 至 26℃，空气质量优良。
```json
{"city": "Beijing", "temp": "15-26C", "air": "good"}
```
详细的天气趋势和未来一周预测已保存在会话上下文，用户可随时在界面滚动查看。"#;
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed, false);
        assert!(summary.contains("完成") || summary.contains("查询到"));
        assert!(summary.contains("北京") || summary.contains("天气"));
        assert!(summary.contains("15") && summary.contains("26"));
        assert!(!summary.contains("```"));
        assert!(!summary.contains("###"));
        assert!(summary.chars().count() <= 80, "spoken summary must be <= 80 chars, got: {}", summary.chars().count());
        assert!(summary.ends_with("，详情请在会话中查看。") || summary.ends_with("。"));
    }

    #[test]
    fn test_spoken_summary_strictly_preserves_negation() {
        let raw = "任务执行失败：未能找到您指定的备份目录，请检查路径是否正确，或者重新指定要备份的文件夹。详细错误日志已经写入系统控制台。";
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Failed, false);
        assert!(summary.contains("失败"), "summary must retain negation/failure marker");
        assert!(summary.contains("未能") || summary.contains("未"), "summary must retain negation marker");
        assert!(summary.chars().count() <= 80);
    }

    #[test]
    fn test_spoken_summary_handles_empty_fallback() {
        let summary_done = extract_spoken_summary("", VoiceTaskStatus::Completed, false);
        assert_eq!(summary_done, "执行已结束，详情请在会话中查看。");
        let summary_failed = extract_spoken_summary("   ", VoiceTaskStatus::Failed, false);
        assert_eq!(summary_failed, "任务未完成，详情请在会话中查看。");
    }

    #[test]
    fn test_format_task_status_text_neutral_presentation() {
        let mut task = nomifun_api_types::voice::VoiceTaskSnapshot {
            command_id: "cmd-1".into(),
            session_id: "sess-1".into(),
            acoustic_turn_id: "turn-1".into(),
            conversation_id: Some("conv-1".into()),
            message_id: Some("msg-1".into()),
            runtime_turn_id: Some("rt-1".into()),
            status: VoiceTaskStatus::Completed,
            result_ok: Some(true),
            result_text: Some("抱歉，未能获取天气信息。".into()),
            error_code: None,
        };

        // 1. Chinese completed with unachieved text: neutral prefix + raw text preserved
        let zh_text = format_task_status_text(&task, false, false);
        assert_eq!(zh_text, "执行已结束。抱歉，未能获取天气信息。");

        // 2. English completed with unachieved text: neutral prefix + raw text preserved
        task.result_text = Some("Sorry, could not find any records.".into());
        let en_text = format_task_status_text(&task, true, false);
        assert_eq!(en_text, "Execution finished. Sorry, could not find any records.");

        // 3. Completed with empty/None text
        task.result_text = None;
        assert_eq!(format_task_status_text(&task, false, false), "执行已结束。请查看任务详情。");
        assert_eq!(format_task_status_text(&task, true, false), "Execution finished. Please check task details.");

        // 4. Cancelled
        task.status = VoiceTaskStatus::Failed;
        task.error_code = Some("task_cancelled".into());
        assert_eq!(format_task_status_text(&task, false, true), "任务已取消。");
        assert_eq!(format_task_status_text(&task, true, true), "Task cancelled.");

        // 5. Failed
        task.result_text = Some("Tool execution failed".into());
        assert_eq!(format_task_status_text(&task, false, false), "任务已结束，执行结果未成功。Tool execution failed");
        assert_eq!(format_task_status_text(&task, true, false), "Task ended, execution was unsuccessful. Tool execution failed");

        // 6. Normal success text
        task.status = VoiceTaskStatus::Completed;
        task.result_text = Some("已为您创建日程。".into());
        assert_eq!(format_task_status_text(&task, false, false), "执行已结束。已为您创建日程。");
        assert_eq!(format_task_status_text(&task, true, false), "Execution finished. 已为您创建日程。");
    }

    #[test]
    fn test_spoken_summary_must_not_drop_actual_backup_failure() {
        let raw = "目录扫描已完成，已检查全部文件并统计文件数量和大小以及当前磁盘空间使用情况并生成了详细目录结构和待处理文件清单用于下一步执行，但是备份失败没有复制任何文件且目标目录仍然为空，请勿删除原始文件以免丢失数据。";
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed, false);
        assert!(summary.contains("备份失败") || summary.contains("没有复制"), "spoken summary lost the actual failure: {summary}");
        assert!(summary.chars().count() <= 80, "summary must be <= 80 chars, got: {}", summary.chars().count());
    }

    #[tokio::test]
    async fn test_session_start_call_lifecycle_and_conflict() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        ));
        let app = voice_companion_routes(manager.clone());

        // 1. Calling when manager not running returns 400
        let req_stopped = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-1".into(),
                target: VoiceCallTarget::DefaultWorkspace,
            }).unwrap()))
            .unwrap();
        let resp = app.clone().oneshot(req_stopped).await.unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);

        // 2. Set mock child and mock recorder
        struct TestCallRecorder {
            conversation_id: String,
            memory_owner: Option<String>,
            reservation: std::sync::RwLock<Option<crate::voice_dialogue::VoiceCallReservation>>,
            resolutions: Arc<std::sync::atomic::AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl VoiceDialogueRecorder for TestCallRecorder {
            fn set_call_reservation(&self, res: Option<crate::voice_dialogue::VoiceCallReservation>) {
                *self.reservation.write().unwrap() = res;
            }
            fn take_call_reservation(&self, request_id: &str) -> Option<crate::voice_dialogue::VoiceCallReservation> {
                let mut reservation=self.reservation.write().unwrap();
                if reservation.as_ref().is_some_and(|r|r.request_id==request_id) {reservation.take()} else {None}
            }
            fn current_call_reservation(&self) -> Option<crate::voice_dialogue::VoiceCallReservation> {
                self.reservation.read().unwrap().clone()
            }
            async fn preflight(&self, _: &VoiceDialogueRequest) -> Result<VoiceDialoguePreflight, VoiceDialogueRecorderError> {
                Err(VoiceDialogueRecorderError::BindingInvalid("test".into()))
            }
            async fn record_pair(&self, _: &str, _: &VoiceDialogueRequest, _: &str) -> Result<(), VoiceDialogueRecorderError> {
                Ok(())
            }
            async fn resolve_call_target_with_locale(&self, target: &VoiceCallTarget, _session_mode: Option<&str>, _request_id: Option<&str>, _locale: Option<&str>) -> Result<(String, Option<String>), VoiceDialogueRecorderError> {
                self.resolutions.fetch_add(1, Ordering::SeqCst);
                match target {
                    VoiceCallTarget::Workspace { workspace } if workspace == "invalid_dir" => {
                        Err(VoiceDialogueRecorderError::WriteFailed("workspace_directory_invalid: not a directory".into()))
                    }
                    VoiceCallTarget::Workspace { .. } => Ok((self.conversation_id.clone(), self.memory_owner.clone())),
                    VoiceCallTarget::DefaultWorkspace | VoiceCallTarget::CurrentSelection => Ok((self.conversation_id.clone(), self.memory_owner.clone())),
                    VoiceCallTarget::VoiceConversation { conversation_id } => Ok((conversation_id.clone(), self.memory_owner.clone())),
                    VoiceCallTarget::NewVoiceConversation { .. } => Ok((self.conversation_id.clone(), self.memory_owner.clone())),
                }
            }
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let reject_wake = Arc::new(AtomicBool::new(true));
        let wake_rejection = reject_wake.clone();
        let unknown_wake = Arc::new(AtomicBool::new(false));
        let wake_unknown = unknown_wake.clone();
        let reject_stop = Arc::new(AtomicBool::new(true));
        let stop_rejection = reject_stop.clone();
        let daemon_snapshot = Arc::new(RwLock::new(VoiceSessionSnapshot::default()));
        let stop_snapshot = daemon_snapshot.clone();
        let read_snapshot = daemon_snapshot.clone();
        let mock_daemon = axum::Router::new().route(
            "/api/voice/wake",
            axum::routing::post(move || {let rejected=wake_rejection.load(Ordering::SeqCst);let unknown=wake_unknown.load(Ordering::SeqCst);async move {
                if unknown {
                    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, axum::Json(serde_json::json!({"success":false,"error":"unexpected_failure"})))
                } else if rejected {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({"success":false,"error":"models_unavailable"})))
                } else {
                    (axum::http::StatusCode::OK, axum::Json(serde_json::json!({ "success": true })))
                }
            }}),
        ).route("/api/voice/stop",axum::routing::post(move || {
            let rejected=stop_rejection.load(Ordering::SeqCst);let snapshot=stop_snapshot.clone();
            async move {
                if rejected {return axum::http::StatusCode::CONFLICT;}
                let mut snapshot=snapshot.write().unwrap();
                snapshot.seq+=1;snapshot.phase=VoiceSessionPhase::Idle;
                snapshot.ended_reason=Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop);
                axum::http::StatusCode::OK
            }
        })).route("/api/voice/snapshot",axum::routing::get(move || {
            let snapshot=read_snapshot.clone();async move {axum::Json(snapshot.read().unwrap().clone())}
        }));
        let daemon_server = tokio::spawn(async move {
            let _ = axum::serve(listener, mock_daemon).await;
        });

        let recorder = Arc::new(TestCallRecorder {
            resolutions: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            conversation_id: "conv-voice-test".into(),
            memory_owner: Some("flowy-bolt".into()),
            reservation: std::sync::RwLock::new(None),
        });
        let manager_with_rec = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
                dir.path(),
            )
            .with_replay_daemon(format!("http://127.0.0.1:{port}"))
            .with_recorder(recorder.clone()),
        );
        // Safe synthetic child (R08: non-recursive dummy child)
        let builder = if cfg!(windows) {
            let mut b = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
            b.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
            b
        } else {
            let mut b = nomi_process_runtime::ChildProcessBuilder::new("sleep");
            b.arg("10");
            b
        };
        let managed = builder.spawn_managed().ok();
        if let Some(m) = managed {
            *manager_with_rec.child.lock().await = Some(m);
        }

        let app_running = voice_companion_routes(manager_with_rec.clone());

        // 3. Invalid directory returns 400
        let req_invalid_ws = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-invalid".into(),
                target: VoiceCallTarget::Workspace { workspace: "invalid_dir".into() },
            }).unwrap()))
            .unwrap();
        let resp_invalid = app_running.clone().oneshot(req_invalid_ws).await.unwrap();
        assert_eq!(resp_invalid.status(), axum::http::StatusCode::BAD_REQUEST);

        // A definite daemon rejection is replayed in exactly the same structured form.
        let rejected_request=VoiceCallStartRequest { locale: None,request_id:"req-rejected".into(),target:VoiceCallTarget::CurrentSelection,session_mode:None};
        let first_rejection=manager_with_rec.start_call(rejected_request.clone()).await.unwrap_err();
        assert_eq!(first_rejection.0,axum::http::StatusCode::SERVICE_UNAVAILABLE);
        let rejection: serde_json::Value = serde_json::from_str(&first_rejection.1).unwrap();
        assert_eq!(rejection["code"],"daemon_wake_rejected");
        assert_eq!(rejection["request_id"],"req-rejected");
        assert_eq!(rejection["terminal"],true);
        assert!(recorder.current_call_reservation().is_none());
        reject_wake.store(false,Ordering::SeqCst);
        let replay_rejection=manager_with_rec.start_call(rejected_request).await.unwrap_err();
        assert_eq!(replay_rejection,first_rejection);

        // An unclassified daemon failure must retain the reservation and replay
        // the same request rather than resolving a second conversation.
        unknown_wake.store(true, Ordering::SeqCst);
        let unknown_request=VoiceCallStartRequest { locale: None,request_id:"req-unknown".into(),target:VoiceCallTarget::CurrentSelection,session_mode:None};
        let unknown=manager_with_rec.start_call(unknown_request.clone()).await.unwrap_err();
        assert_eq!(unknown.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&unknown.1).unwrap()["code"], "daemon_wake_unknown");
        assert_eq!(recorder.current_call_reservation().unwrap().request_id, "req-unknown");
        let resolutions = recorder.resolutions.load(Ordering::SeqCst);
        let replay_unknown = manager_with_rec.start_call(unknown_request).await.unwrap();
        assert_eq!(replay_unknown.request_id, "req-unknown");
        assert_eq!(replay_unknown.status, VoiceCallStartStatus::Starting);
        assert_eq!(recorder.resolutions.load(Ordering::SeqCst), resolutions);
        recorder.take_call_reservation("req-unknown");
        manager_with_rec.handled_start_requests.write().unwrap().remove("req-unknown");
        manager_with_rec.set_candidate_conversation(None);
        unknown_wake.store(false, Ordering::SeqCst);

        // 4. Valid start returns starting status and sets active candidate
        let req_valid = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_valid = app_running.clone().oneshot(req_valid).await.unwrap();
        assert_eq!(resp_valid.status(), axum::http::StatusCode::OK);
        let body = axum::body::to_bytes(resp_valid.into_body(), 16_384).await.unwrap();
        let start_resp: VoiceCallStartResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(start_resp.request_id, "req-valid");
        assert_eq!(start_resp.conversation_id, "conv-voice-test");
        assert_eq!(start_resp.status, VoiceCallStartStatus::Starting);
        assert_eq!(start_resp.memory_owner_companion_id.as_deref(), Some("flowy-bolt"));
        assert_eq!(
            manager_with_rec.active_window_candidate.read().unwrap().conversation_id.as_deref(),
            Some("conv-voice-test")
        );

        // 4a-1. F3: An old stop request with mismatched session must NOT clear new pending reservation
        let stop_old = manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id: "old-instance".into(),
            session_id: Some("old-session-id".into()),
            turn_id: None,
        }).await;
        assert!(stop_old.is_err(), "old stop request with wrong instance/session must be rejected");
        assert!(recorder.current_call_reservation().is_some(), "F3: rejected old stop request must NOT destroy new reservation");

        // The old idle snapshot still matches all three control fields while the new wake is pending.
        let idle=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id:idle.instance_id,session_id:idle.session_id,turn_id:idle.turn_id,
        }).await.is_err());
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-valid");

        // 4b. Idempotency test (R02): same request_id and target returns Starting
        let req_retry = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_retry = app_running.clone().oneshot(req_retry).await.unwrap();
        assert_eq!(resp_retry.status(), axum::http::StatusCode::OK);

        // 4c. Request target conflict (R02): same request_id with different target returns 400
        let req_target_conflict = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::DefaultWorkspace,
            }).unwrap()))
            .unwrap();
        let resp_target_conflict = app_running.clone().oneshot(req_target_conflict).await.unwrap();
        assert_eq!(resp_target_conflict.status(), axum::http::StatusCode::BAD_REQUEST);

        let changed_mode = manager_with_rec.start_call(VoiceCallStartRequest { locale: None,
            request_id:"req-valid".into(), target:VoiceCallTarget::Workspace { workspace:"D:\\valid_ws".into() },
            session_mode:Some("default".into()),
        }).await.unwrap_err();
        assert_eq!(changed_mode.0, axum::http::StatusCode::BAD_REQUEST);

        // 4d. Concurrent start (R02): different request_id while pending returns 409
        let req_concurrent = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-different".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_concurrent = app_running.clone().oneshot(req_concurrent).await.unwrap();
        assert_eq!(resp_concurrent.status(), axum::http::StatusCode::CONFLICT);

        let resolutions_before_unknown = recorder.resolutions.load(std::sync::atomic::Ordering::SeqCst);
        if let Some(HandledStartOutcome::Starting {reserved_at_epoch_ms, ..}) = manager_with_rec.handled_start_requests.write().unwrap().get_mut("req-valid") {
            *reserved_at_epoch_ms -= 11_000;
        }
        let unknown = manager_with_rec.start_call(VoiceCallStartRequest { locale: None,
            request_id:"req-valid".into(), target:VoiceCallTarget::Workspace {workspace:"D:\\valid_ws".into()}, session_mode:None,
        }).await.unwrap_err();
        assert!(unknown.1.contains("voice_call_start_unknown"));
        assert_eq!(recorder.resolutions.load(std::sync::atomic::Ordering::SeqCst),resolutions_before_unknown);

        // 5. If already active on same conversation -> returns Active
        {
            let mut snap = manager_with_rec.snapshot.write().unwrap();
            snap.session_id = Some("active-session-1".into());
            snap.call_request_id = Some("req-valid".into());
            snap.phase = VoiceSessionPhase::Listening;
            snap.binding = Some(nomifun_api_types::voice::VoiceBindingSnapshot {
                session_id: "active-session-1".into(),
                binding_id: "binding-1".into(),
                conversation_id: "conv-voice-test".into(),
                workspace: Some("D:\\valid_ws".into()),
                memory_owner_companion_id: Some("flowy-bolt".into()),
            });
        }
        let req_same = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-same".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_same = app_running.clone().oneshot(req_same).await.unwrap();
        assert_eq!(resp_same.status(), axum::http::StatusCode::OK);
        let body_same = axum::body::to_bytes(resp_same.into_body(), 16_384).await.unwrap();
        let start_same: VoiceCallStartResponse = serde_json::from_slice(&body_same).unwrap();
        assert_eq!(start_same.status, VoiceCallStartStatus::Active);
        assert_eq!(start_same.conversation_id, "conv-voice-test");
        assert_eq!(start_same.session_id.as_deref(), Some("active-session-1"));

        // F1: Retrying req-valid while call is bound/active returns OK idempotently
        let req_valid_bound = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::Workspace { workspace: "D:\\valid_ws".into() },
            }).unwrap()))
            .unwrap();
        let resp_bound = app_running.clone().oneshot(req_valid_bound).await.unwrap();
        assert_eq!(resp_bound.status(), axum::http::StatusCode::OK);
        let body_bound = axum::body::to_bytes(resp_bound.into_body(), 16_384).await.unwrap();
        let bound_resp: VoiceCallStartResponse = serde_json::from_slice(&body_bound).unwrap();
        assert_eq!(bound_resp.request_id, "req-valid");
        assert_eq!(bound_resp.status, VoiceCallStartStatus::Active);

        // F1: Same req-valid with changed target after bound STILL returns 400 target conflict
        let req_bound_diff_target = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-valid".into(),
                target: VoiceCallTarget::DefaultWorkspace,
            }).unwrap()))
            .unwrap();
        let resp_bound_diff = app_running.clone().oneshot(req_bound_diff_target).await.unwrap();
        assert_eq!(resp_bound_diff.status(), axum::http::StatusCode::BAD_REQUEST);

        // 6. If already active on different conversation -> 409 CONFLICT
        let resolutions_before_conflict = recorder.resolutions.load(Ordering::SeqCst);
        let req_conflict = axum::http::Request::builder()
            .method("POST")
            .uri("/api/voice/companion/session/start")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_string(&VoiceCallStartRequest { locale: None,
                session_mode: None,
                request_id: "req-diff".into(),
                target: VoiceCallTarget::VoiceConversation { conversation_id: "other-conversation".into() },
            }).unwrap()))
            .unwrap();
        let resp_conflict = app_running.clone().oneshot(req_conflict).await.unwrap();
        assert_eq!(resp_conflict.status(), axum::http::StatusCode::CONFLICT);
        let body_conflict = axum::body::to_bytes(resp_conflict.into_body(), 16_384).await.unwrap();
        let conflict_json: serde_json::Value = serde_json::from_slice(&body_conflict).unwrap();
        assert_eq!(conflict_json["code"], "voice_call_conflict");
        assert_eq!(conflict_json["active_conversation_id"], "conv-voice-test");
        assert_eq!(recorder.resolutions.load(Ordering::SeqCst), resolutions_before_conflict,
            "an active call must reject a different target before creating any Conversation");

        // An ended old session cannot destroy a newer reservation even when the old control matches.
        let unbound_target=VoiceCallTarget::VoiceConversation {conversation_id:"conv-voice-test".into()};
        let reserved_at=chrono::Utc::now().timestamp_millis();
        recorder.set_call_reservation(Some(crate::voice_dialogue::VoiceCallReservation {
            request_id:"req-unbound".into(),target:unbound_target.clone(),conversation_id:"conv-voice-test".into(),
            memory_owner:None,reserved_at_epoch_ms:reserved_at,
        }));
        manager_with_rec.record_handled_start_request("req-unbound".into(),HandledStartOutcome::Starting {
            target:unbound_target,session_mode:None,conversation_id:"conv-voice-test".into(),memory_owner:None,reserved_at_epoch_ms:reserved_at,
        });
        let old_active=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id:old_active.instance_id,session_id:old_active.session_id,turn_id:old_active.turn_id,
        }).await.is_err(),"an old active projection must not stop a different pending request");
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-unbound");
        {
            let mut snapshot=manager_with_rec.snapshot.write().unwrap();
            snapshot.phase=VoiceSessionPhase::Idle;
            snapshot.ended_reason=Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence);
        }
        let old=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {
            instance_id:old.instance_id,session_id:old.session_id,turn_id:old.turn_id,
        }).await.is_err());
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-unbound");

        // An accepted, still-unbound call retains its reservation on a rejected stop.
        let source=VoiceSessionSnapshot {instance_id:"daemon".into(),seq:40,session_id:Some("unbound-session".into()),
            call_request_id:Some("req-unbound".into()),phase:VoiceSessionPhase::Listening,..Default::default()};
        *daemon_snapshot.write().unwrap()=source.clone();
        manager_with_rec.replay_acoustic_source(source);
        manager_with_rec.snapshot.write().unwrap().binding=None;
        let unbound=manager_with_rec.snapshot();
        assert!(manager_with_rec.stop_session(VoiceSessionControlRequest {instance_id:unbound.instance_id.clone(),session_id:unbound.session_id.clone(),turn_id:unbound.turn_id.clone()}).await.is_err());
        assert_eq!(recorder.current_call_reservation().unwrap().request_id,"req-unbound");
        reject_stop.store(false,Ordering::SeqCst);
        manager_with_rec.stop_session(VoiceSessionControlRequest {instance_id:unbound.instance_id,session_id:unbound.session_id,turn_id:unbound.turn_id}).await.unwrap();
        assert!(recorder.current_call_reservation().is_none());
        assert!(matches!(manager_with_rec.handled_start_requests.read().unwrap().get("req-unbound"),Some(HandledStartOutcome::Stopped {..})));
        manager_with_rec.mark_request_bound("req-unbound","late-session","conv-voice-test",None);
        assert!(matches!(manager_with_rec.handled_start_requests.read().unwrap().get("req-unbound"),Some(HandledStartOutcome::Stopped {..})),"late binding must not resurrect a stopped request");
        if let Some(mut child) = manager_with_rec.child.lock().await.take() {
            tokio::time::timeout(std::time::Duration::from_secs(5), child.shutdown()).await.unwrap().unwrap();
            assert!(child.try_wait().unwrap().is_some());
        }
        daemon_server.abort();
        let _ = daemon_server.await;
        let rebound = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
        drop(rebound);
    }

    #[test]
    fn window_context_ignores_late_clear_and_preserves_explicit_project() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        manager.report_window_context(VoiceWindowContext { workspace: Some("project-q".into()), sequence: 12, ..Default::default() });
        manager.report_window_context(VoiceWindowContext { sequence: 11, ..Default::default() });
        assert_eq!(manager.active_window_candidate().read().unwrap().workspace.as_deref(), Some("project-q"));
        manager.report_window_context(VoiceWindowContext { sequence: 13, ..Default::default() });
        assert!(manager.active_window_candidate().read().unwrap().workspace.is_none());
    }

    #[tokio::test]
    async fn durable_voice_identity_survives_start_cache_eviction_and_file_reopen() {
        // Real parts under test: the manager's start-call arbitration and
        // bounded handled-start cache, the production
        // ConversationVoiceDialogueRecorder, file-backed SQLite, the on-disk
        // dialogue journal/workspace registry, and durable provisional
        // creation identity. Simulated boundary only: the daemon wake IPC
        // endpoint and the model call (no microphone or acoustic models are
        // started). No route, arbitration, or storage layer is mocked.
        use async_trait::async_trait;
        use nomifun_ai_agent::{
            AgentRuntimeRegistry, runtime_handle::AgentRuntimeHandle,
            types::AgentRuntimeBuildOptions,
        };
        use nomifun_common::{AgentKillReason, AppError, MessageId, TimestampMs};
        use nomifun_conversation::ConversationService;
        use nomifun_conversation::skill_resolver::{ResolvedAgentSkill, SkillResolver};
        use nomifun_db::{
            SqliteAcpSessionRepository, SqliteAgentMetadataRepository,
            SqliteConversationRepository,
        };
        use nomifun_realtime::UserEventSink;

        struct Sink;
        impl UserEventSink for Sink {
            fn send_to_user(
                &self,
                _: &str,
                _: nomifun_api_types::WebSocketMessage<serde_json::Value>,
            ) {
            }
        }
        struct Skills;
        #[async_trait]
        impl SkillResolver for Skills {
            async fn auto_inject_names(&self) -> Vec<String> {
                vec![]
            }
            async fn resolve_skills(&self, _: &[String]) -> Vec<ResolvedAgentSkill> {
                vec![]
            }
            async fn link_workspace_skills(
                &self,
                _: &std::path::Path,
                _: &[&str],
                _: &[ResolvedAgentSkill],
            ) -> usize {
                0
            }
        }
        struct Runtime;
        #[async_trait]
        impl AgentRuntimeRegistry for Runtime {
            fn get_runtime(&self, _: &str) -> Option<AgentRuntimeHandle> {
                None
            }
            async fn get_or_create_runtime(
                &self,
                _: &str,
                _: AgentRuntimeBuildOptions,
            ) -> Result<AgentRuntimeHandle, AppError> {
                Err(AppError::Internal(
                    "no model execution in this persistence drill".into(),
                ))
            }
            fn terminate(&self, _: &str, _: Option<AgentKillReason>) -> Result<(), AppError> {
                Ok(())
            }
            fn terminate_all(&self) {}
            fn active_runtime_count(&self) -> usize {
                0
            }
            fn collect_idle_runtimes(&self, _: TimestampMs) -> Vec<String> {
                vec![]
            }
        }
        fn service_on_pool(
            pool: nomifun_db::sqlx::SqlitePool,
            owner: &str,
        ) -> Arc<ConversationService> {
            Arc::new(ConversationService::new(
                Arc::<str>::from(owner),
                std::env::temp_dir(),
                Arc::new(Sink),
                Arc::new(Skills),
                Arc::new(Runtime),
                Arc::new(SqliteConversationRepository::new(pool.clone())),
                Arc::new(SqliteAgentMetadataRepository::new(pool.clone())),
                Arc::new(SqliteAcpSessionRepository::new(pool)),
                Arc::new(nomifun_conversation::NoExecutionConversationBoundary),
            ))
        }
        async fn start_mock_daemon() -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap();
            let port = listener.local_addr().unwrap().port();
            let app = axum::Router::new().route(
                "/api/voice/wake",
                axum::routing::post(|| async {
                    (
                        axum::http::StatusCode::OK,
                        axum::Json(serde_json::json!({ "success": true })),
                    )
                }),
            );
            tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            format!("http://127.0.0.1:{port}")
        }
        async fn boot_manager(
            _service: &Arc<ConversationService>,
            recorder: &Arc<ConversationVoiceDialogueRecorder>,
            data_dir: &std::path::Path,
            daemon_origin: String,
        ) -> Arc<VoiceCompanionProcessManager> {
            let manager = Arc::new(
                VoiceCompanionProcessManager::new(
                    Arc::new(nomifun_voice_models::ModelManager::new(data_dir)),
                    data_dir,
                )
                .with_replay_daemon(daemon_origin)
                .with_recorder(recorder.clone()),
            );
            // Safe synthetic child (non-recursive dummy process): start_call
            // requires the companion daemon to report running, while the real
            // IPC boundary is the mocked wake endpoint above.
            let builder = if cfg!(windows) {
                let mut b = nomi_process_runtime::ChildProcessBuilder::new("cmd.exe");
                b.args(["/c", "ping", "-n", "10", "127.0.0.1"]);
                b
            } else {
                let mut b = nomi_process_runtime::ChildProcessBuilder::new("sleep");
                b.arg("10");
                b
            };
            if let Some(child) = builder.spawn_managed().ok() {
                *manager.child.lock().await = Some(child);
            }
            manager
        }

        let dir = tempfile::tempdir().unwrap();
        let database_path = dir.path().join("voice-cache.sqlite");
        let database = nomifun_db::init_database(&database_path).await.unwrap();
        let owner: String = nomifun_db::sqlx::query_scalar(
            "SELECT owner_user_id FROM installation_identity",
        )
        .fetch_one(database.pool())
        .await
        .unwrap();
        let service = service_on_pool(database.pool().clone(), &owner);
        let workspace = tempfile::tempdir().unwrap();
        let frozen_workspace = nomifun_common::paths::canonicalize_simplified(workspace.path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let daemon_origin = start_mock_daemon().await;

        // 1. Production start_call creates the durable identity and records a
        //    Starting receipt in the bounded in-memory cache.
        let recorder =
            Arc::new(ConversationVoiceDialogueRecorder::new(service.clone(), owner.clone(), dir.path()));
        let manager = boot_manager(&service, &recorder, dir.path(), daemon_origin.clone()).await;
        let recover_target = nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
            workspace: Some(frozen_workspace.clone()),
            model: None,
            preset_id: None,
        };
        let recover_request = nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
            request_id: "req-cache-recover".into(),
            target: recover_target.clone(),
            session_mode: None,
        };
        let first_start = manager.start_call(recover_request.clone()).await.unwrap();
        assert_eq!(
            first_start.status,
            nomifun_api_types::voice::VoiceCallStartStatus::Starting
        );
        let retained_conversation_id = first_start.conversation_id.clone();
        service
            .append_voice_message(
                &owner,
                &retained_conversation_id,
                &MessageId::new().into_string(),
                "accepted input must survive",
                "cache-command",
            )
            .await
            .unwrap();

        // A discarded request must never be resurrected by a cache miss.
        // Simulate the 10s starting window elapsing so a different request may
        // proceed without waiting in real time.
        recorder.take_call_reservation("req-cache-recover");
        let closed_start = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-closed".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: None,
            })
            .await
            .unwrap();
        let closed_conversation_id = closed_start.conversation_id.clone();
        assert!(
            service
                .discard_provisional_voice_conversation(&owner, &closed_conversation_id)
                .await
                .unwrap()
        );

        // A changed session mode for the same request_id must conflict.
        recorder.take_call_reservation("req-cache-closed");
        let mode_start = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-mode".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: Some("default".into()),
            })
            .await
            .unwrap();
        let mode_conversation_id = mode_start.conversation_id.clone();
        recorder.take_call_reservation("req-cache-mode");

        // Real journal: one completed dialogue and one claim that never got a
        // model answer (the model boundary itself is simulated/absent).
        let completed_request =
            VoiceDialogueRequest::scoped("cache-session", "cache-turn", "remember this");
        let recorded_conversation = match recorder.preflight(&completed_request).await.unwrap() {
            VoiceDialoguePreflight::Ready { conversation_id } => conversation_id,
            other => panic!("expected a fresh voice binding, got {other:?}"),
        };
        recorder
            .record_pair(
                &recorded_conversation,
                &completed_request,
                r#"{"intent_type":"chat","understood_request":"remember","reply_tts":"remembered"}"#,
            )
            .await
            .unwrap();
        let unanswered_request =
            VoiceDialogueRequest::scoped("cache-session", "cache-turn-unknown", "unfinished");
        recorder.preflight(&unanswered_request).await.unwrap();

        // 2. Force the 128-entry start-receipt cache past its bound. The durable
        //    identity below must not depend on these in-memory receipts.
        let filler_reserved_at = chrono::Utc::now().timestamp_millis() + 10_000;
        for index in 0..135 {
            manager.record_handled_start_request(
                format!("req-filler-{index}"),
                HandledStartOutcome::Starting {
                    target: nomifun_api_types::voice::VoiceCallTarget::CurrentSelection,
                    session_mode: None,
                    conversation_id: format!("conv-filler-{index}"),
                    memory_owner: None,
                    // Newer than the three real receipts, so the bounded cache
                    // evicts the real ones first.
                    reserved_at_epoch_ms: filler_reserved_at + index,
                },
            );
        }
        {
            let receipts = manager.handled_start_requests.read().unwrap();
            assert_eq!(receipts.len(), 128, "start receipts must stay bounded");
            assert!(
                !receipts.contains_key("req-cache-recover"),
                "the real start receipt must be evicted"
            );
            assert!(!receipts.contains_key("req-cache-closed"));
            assert!(!receipts.contains_key("req-cache-mode"));
        }

        // 3. Real restart: drop manager/recorder/service, close SQLite, reopen
        //    the file database and journal, and rebuild all three objects.
        drop(manager);
        drop(recorder);
        drop(service);
        database.close().await;

        let reopened = nomifun_db::init_database(&database_path).await.unwrap();
        let service = service_on_pool(reopened.pool().clone(), &owner);
        let recorder =
            Arc::new(ConversationVoiceDialogueRecorder::new(service.clone(), owner.clone(), dir.path()));
        let manager = boot_manager(&service, &recorder, dir.path(), daemon_origin).await;
        assert!(
            manager.handled_start_requests.read().unwrap().is_empty(),
            "a restarted manager starts with no in-memory receipts"
        );

        // 4. Retry through the same production entrypoint with the same
        //    request_id and frozen target.
        let replay = manager.start_call(recover_request).await.unwrap();
        assert_eq!(
            replay.conversation_id, retained_conversation_id,
            "the durable creation identity must win without the evicted cache"
        );
        assert_eq!(
            replay.status,
            nomifun_api_types::voice::VoiceCallStartStatus::Starting
        );
        assert_eq!(
            recorder
                .current_call_reservation()
                .expect("the replayed start must reserve the exact conversation")
                .conversation_id,
            retained_conversation_id
        );

        let replayed_row = service
            .get(&owner, &retained_conversation_id)
            .await
            .unwrap();
        assert_eq!(
            replayed_row.extra["voice_task_workspace"], frozen_workspace,
            "replaying the frozen target must not switch workspaces"
        );
        assert_eq!(replayed_row.extra["voice_lifecycle"], "retained");
        let duplicate_conversations: i64 = nomifun_db::sqlx::query_scalar(
            "SELECT COUNT(*) FROM conversations \
             WHERE user_id = ? AND json_extract(extra, '$.voice_creation_request.request_id') = ?",
        )
        .bind(&owner)
        .bind("req-cache-recover")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
        assert_eq!(
            duplicate_conversations, 1,
            "the replayed start must not create a second Conversation"
        );
        let retained_messages = service
            .list_messages(&owner, &retained_conversation_id, Default::default())
            .await
            .unwrap();
        assert_eq!(
            retained_messages
                .items
                .iter()
                .filter(|message| message.content["content"] == "accepted input must survive")
                .count(),
            1,
            "an accepted message must not be lost or duplicated"
        );

        // 5. A cache-miss replay of a discarded or reconfigured request fails
        //    closed instead of resurrecting or re-creating the Conversation.
        // Simulate the replay's starting window elapsing.
        recorder.take_call_reservation("req-cache-recover");
        let closed_error = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-closed".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: None,
            })
            .await
            .expect_err("a discarded start request must not be resurrected");
        assert_eq!(closed_error.0, axum::http::StatusCode::CONFLICT);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&closed_error.1).unwrap()["code"],
            "conversation_already_discarded"
        );
        assert_eq!(
            service
                .get(&owner, &closed_conversation_id)
                .await
                .unwrap()
                .extra["voice_lifecycle"],
            "discarded"
        );

        let mode_error = manager
            .start_call(nomifun_api_types::voice::VoiceCallStartRequest { locale: None,
                request_id: "req-cache-mode".into(),
                target: nomifun_api_types::voice::VoiceCallTarget::NewVoiceConversation {
                    workspace: None,
                    model: None,
                    preset_id: None,
                },
                session_mode: Some("yolo".into()),
            })
            .await
            .expect_err("a changed configuration must conflict after restart");
        assert_eq!(mode_error.0, axum::http::StatusCode::CONFLICT);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&mode_error.1).unwrap()["code"],
            "voice_start_request_conflict"
        );
        assert_eq!(
            service
                .get(&owner, &mode_conversation_id)
                .await
                .unwrap()
                .extra["session_mode"],
            "default"
        );

        // 6. Journal reopen: the completed turn is served from disk without a
        //    new generation, and the unanswered claim stays unknown instead of
        //    being silently converted into a fresh model request.
        assert!(matches!(
            recorder.preflight(&completed_request).await.unwrap(),
            VoiceDialoguePreflight::Cached { content } if content.contains("remembered")
        ));
        assert_eq!(
            recorder.preflight(&unanswered_request).await.unwrap_err(),
            VoiceDialogueRecorderError::GenerationUnknown
        );
        let recorded_messages = service
            .list_messages(&owner, &recorded_conversation, Default::default())
            .await
            .unwrap();
        assert_eq!(
            recorded_messages
                .items
                .iter()
                .filter(|message| message.content["content"] == "remember this")
                .count(),
            1,
            "journal replay must not duplicate the accepted user message"
        );

        reopened.close().await;
    }

    #[test]
    fn handled_start_requests_cache_bounds_and_evicts_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        for i in 0..135 {
            manager.record_handled_start_request(
                format!("req-{i}"),
                HandledStartOutcome::Starting {
                    target: VoiceCallTarget::CurrentSelection,
                    session_mode: None,
                    conversation_id: format!("conv-{i}"),
                    memory_owner: None,
                    reserved_at_epoch_ms: 1000 + i as i64,
                },
            );
        }
        let map = manager.handled_start_requests.read().unwrap();
        assert_eq!(map.len(), 128);
        assert!(!map.contains_key("req-0"));
        assert!(!map.contains_key("req-1"));
        assert!(!map.contains_key("req-6"));
        assert!(map.contains_key("req-7"));
        assert!(map.contains_key("req-134"));
    }

    #[test]
    fn voice_view_departure_preserves_target_and_fences_delayed_full_context() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        manager.report_window_context(VoiceWindowContext {sequence: 10, conversation_id: Some("project-a".into()), workspace: Some("workspace-a".into())});
        manager.report_voice_view(None, 12);
        assert_eq!(manager.viewed_conversation.read().unwrap().clone(), (12, None));
        assert_eq!(manager.active_window_candidate.read().unwrap().conversation_id.as_deref(), Some("project-a"));
        manager.report_window_context(VoiceWindowContext {sequence: 11, conversation_id: Some("stale-project".into()), workspace: Some("stale-workspace".into())});
        assert_eq!(manager.viewed_conversation.read().unwrap().clone(), (12, None));
        assert_eq!(manager.active_window_candidate.read().unwrap().conversation_id.as_deref(), Some("project-a"), "an older full report must not cross the newer view-only sequence fence");
        manager.report_window_context(VoiceWindowContext {sequence: 13, conversation_id: Some("project-b".into()), workspace: Some("workspace-b".into())});
        assert_eq!(manager.viewed_conversation.read().unwrap().clone(), (13, Some("project-b".into())));
        assert_eq!(manager.active_window_candidate.read().unwrap().workspace.as_deref(), Some("workspace-b"));
    }

    #[test]
    fn voice_natural_end_retires_unbound_request_without_resurrecting_or_retiring_a_newer_request() {
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path());
        for request in ["unbound", "newer"] {
            manager.record_handled_start_request(request.into(), HandledStartOutcome::Starting {
                target: VoiceCallTarget::CurrentSelection, session_mode: None, conversation_id: "voice".into(),
                memory_owner: None, reserved_at_epoch_ms: 1,
            });
        }
        manager.replay_acoustic_source(VoiceSessionSnapshot {
            instance_id: "daemon".into(), seq: 1, session_id: Some("session-unbound".into()),
            call_request_id: Some("unbound".into()), phase: VoiceSessionPhase::Idle,
            ended_reason: Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence), ..Default::default()
        });
        manager.mark_request_bound("unbound", "late-session", "voice", None);
        let receipts = manager.handled_start_requests.read().unwrap();
        assert!(matches!(receipts.get("unbound"), Some(HandledStartOutcome::Stopped { session_id: Some(session), .. }) if session == "session-unbound"));
        assert!(matches!(receipts.get("newer"), Some(HandledStartOutcome::Starting { .. })));
    }

    #[test]
    fn voice_natural_end_retires_only_its_bound_start_receipt() {
        let dir=tempfile::tempdir().unwrap();
        let manager=VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),dir.path());
        for (request,session) in [("old-request","old-session"),("new-request","new-session")] {
            manager.record_handled_start_request(request.into(),HandledStartOutcome::Bound {
                target:VoiceCallTarget::CurrentSelection,session_mode:None,conversation_id:"voice".into(),
                session_id:Some(session.into()),memory_owner:None,bound_at_epoch_ms:1,
            });
        }
        manager.replay_acoustic_source(VoiceSessionSnapshot {instance_id:"daemon".into(),seq:2,session_id:Some("new-session".into()),
            call_request_id:Some("new-request".into()),phase:VoiceSessionPhase::Listening,..Default::default()});
        manager.replay_acoustic_source(VoiceSessionSnapshot {instance_id:"daemon".into(),seq:1,session_id:Some("old-session".into()),
            ended_reason:Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence),..Default::default()});
        assert!(matches!(manager.handled_start_requests.read().unwrap().get("new-request"),Some(HandledStartOutcome::Bound {..})));
        manager.replay_acoustic_source(VoiceSessionSnapshot {instance_id:"daemon".into(),seq:3,session_id:Some("new-session".into()),
            ended_reason:Some(nomifun_api_types::voice::VoiceSessionEndReason::FollowUpSilence),..Default::default()});
        let receipts=manager.handled_start_requests.read().unwrap();
        assert!(matches!(receipts.get("new-request"),Some(HandledStartOutcome::Stopped {..})));
        assert!(matches!(receipts.get("old-request"),Some(HandledStartOutcome::Bound {..})));
    }
    #[tokio::test]
    #[ignore = "requires feature-enabled daemon binary, read-only models, isolated port and env-set fault injection"]
    async fn supervised_native_stuck_exits_and_exhausts_restart_budget() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file(), "real read-only model suite required");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
                dir.path(),
            )
            .with_test_daemon_port(port),
        );
        run_headless_case(
            manager.clone(),
            port,
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(180),
            |manager, origins, token| async move {
                let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
                let mut exhausted = false;
                for round in 0..5u32 {
                    let mut running = false;
                    let mut current_pid = 0u32;
                    for _ in 0..100 {
                        let status = manager.status().await;
                        if status.running { running = true; current_pid = status.pid.unwrap_or(0); break; }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    if !running {
                        if manager.snapshot().error.as_ref().is_some_and(|error| error.code == "daemon_restart_exhausted") { exhausted = true; break; }
                        panic!("round {round}: daemon did not become running");
                    }
                    let mut healthy = false;
                    for _ in 0..80 {
                        if client.get(format!("{origins}/healthz")).send().await.is_ok_and(|r| r.status().is_success()) { healthy = true; break; }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    assert!(healthy, "round {round}: daemon did not become healthy");
                    let hardware: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    assert_eq!(hardware["mic_attempts"], serde_json::json!(0));
                    assert_eq!(hardware["speaker_attempts"], serde_json::json!(0));
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let response = client.get(format!("{origins}/api/voice/diagnostics/engine-self-check?request_id=r16-{round}&timeout_ms=500")).header("x-ai-mate-control-token", &token).send().await;
                    let status = response.as_ref().ok().map(|response| response.status());
                    let body: serde_json::Value = match response { Ok(response) => response.json().await.unwrap_or_default(), Err(_) => serde_json::Value::Null };
                    assert_eq!(status, Some(reqwest::StatusCode::OK), "round {round}: bounded engine check must return a failure body");
                    let reason = body.get("reason").and_then(|value| value.as_str());
                    if reason != Some("engine_check_timeout") {
                        for _ in 0..20 {
                            if !manager.status().await.running { break; }
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        }
                        let observed = manager.last_child_exit().map(|exit| (exit.pid, exit.code));
                        eprintln!("r16 unexpected_native_outcome round={round} reason={reason:?} observed_exit={observed:?}");
                    }
                    assert_eq!(reason, Some("engine_check_timeout"));
                    assert_ne!(body.get("asr_ok").and_then(|value| value.as_bool()), Some(true));
                    let after: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    assert!(after["fault_asr_entered"].as_u64().unwrap_or(0) > 0, "round {round}: blocking ASR must actually be entered");
                    let observation = &after["fault_observation"];
                    assert_eq!(observation["native_busy"], serde_json::json!(true), "round {round}: blocking job must still be owned");
                    let job = &observation["job"];
                    assert_eq!(job["request_id"], serde_json::json!(format!("r16-{round}")), "round {round}: job request attribution");
                    assert_eq!(job["worker_epoch"], body["worker_epoch"], "round {round}: job worker attribution");
                    assert_eq!(job["instance_id"], body["instance_id"], "round {round}: job instance attribution");
                    assert_eq!(job["pid"], serde_json::json!(current_pid), "round {round}: job PID attribution");
                    assert!(job["job_epoch"].as_u64().unwrap_or(0) > 0, "round {round}: job epoch missing");
                    assert_eq!(job["still_blocking"], serde_json::json!(true), "round {round}: job must remain owned until watchdog");
                    let mut stopped = false;
                    for _ in 0..20 {
                        if !manager.status().await.running { stopped = true; break; }
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    }
                    assert!(stopped, "round {round}: supervised daemon did not exit");
                    let exit = manager.last_child_exit().expect("observed child exit");
                    assert_eq!(exit.code, Some(75), "round {round}: exit must be watchdog 75, got {:?}", exit.code);
                    assert_eq!(exit.pid, current_pid, "round {round}: exit observation must bind the running PID");
                    eprintln!("r16 round={round} pid={} exit={:?} instance={} worker_epoch={} request={} job_epoch={} busy={} still_blocking={}", current_pid, exit.code, body.get("instance_id").and_then(|v| v.as_str()).unwrap_or(""), body.get("worker_epoch").and_then(|v| v.as_u64()).unwrap_or(0), job["request_id"].as_str().unwrap_or(""), job["job_epoch"].as_u64().unwrap_or(0), observation["native_busy"], job["still_blocking"]);
                    if manager.snapshot().error.as_ref().is_some_and(|error| error.code == "daemon_restart_exhausted") { exhausted = true; break; }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                assert!(exhausted, "restart budget must exhaust");
            },
        )
        .await;
    }
    #[tokio::test]
    #[ignore = "requires feature headless daemon; ordinary cancel must retain native lease"]
    async fn ordinary_cancel_does_not_release_native_lease() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file(), "real read-only model suite required");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(
            VoiceCompanionProcessManager::new(
                Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
                dir.path(),
            )
            .with_test_daemon_port(port),
        );
        run_headless_case(
            manager.clone(),
            port,
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(120),
            |manager, origins, token| async move {
                let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
                let first_pid = manager.status().await.pid.expect("first daemon pid");
                let first_instance = manager.snapshot().instance_id.clone();
                let first = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=ordinary-a")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(first.status(), reqwest::StatusCode::OK);
                let mut observation = serde_json::Value::Null;
                for _ in 0..40 {
                    observation = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    if observation["fault_observation"]["job"]["request_id"] == serde_json::json!("ordinary-a")
                        && observation["fault_observation"]["job"]["still_blocking"] == serde_json::json!(true)
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                assert_eq!(observation["fault_observation"]["native_busy"], serde_json::json!(true));
                assert_eq!(observation["fault_observation"]["job"]["request_id"], serde_json::json!("ordinary-a"));
                assert_eq!(observation["fault_observation"]["job"]["pid"], serde_json::json!(first_pid));
                assert_eq!(observation["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));

                let second = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=ordinary-b")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(second.status(), reqwest::StatusCode::OK);
                let after_cancel: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                assert_eq!(after_cancel["fault_observation"]["native_busy"], serde_json::json!(true));
                assert_eq!(after_cancel["fault_observation"]["job"]["request_id"], serde_json::json!("ordinary-a"));
                assert_eq!(after_cancel["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));

                let mut exited = None;
                for _ in 0..240 {
                    if let Some(observed) = manager.last_child_exit()
                        && observed.pid == first_pid
                    {
                        exited = Some(observed);
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                let exit = exited.expect("ordinary stuck daemon must exit");
                assert_eq!(exit.pid, first_pid);
                assert_eq!(exit.code, Some(75));
                let mut restarted = false;
                for _ in 0..80 {
                    let status = manager.status().await;
                    if status.running && status.pid != Some(first_pid) {
                        restarted = true;
                        let mut new_instance = String::new();
                        for _ in 0..40 {
                            if let Ok(response) = client.get(format!("{origins}/api/voice/snapshot")).header("x-ai-mate-control-token", &token).send().await
                                && let Ok(snapshot) = response.json::<serde_json::Value>().await
                                && let Some(instance) = snapshot["instance_id"].as_str()
                                && instance != first_instance
                            {
                                new_instance = instance.to_owned();
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                        assert!(!new_instance.is_empty(), "new daemon instance must be non-empty");
                        assert_ne!(new_instance, first_instance, "new daemon instance must differ");
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                assert!(restarted, "manager must replace the stuck instance");
                eprintln!("r17 ordinary_request={} pid={} exit={:?} new_instance={} still_blocking_after_cancel={}", observation["fault_observation"]["job"]["request_id"], first_pid, exit.code, manager.snapshot().instance_id.as_str(), after_cancel["fault_observation"]["job"]["still_blocking"]);
            },
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires feature headless daemon; healthy A then healthy B must not be killed"]
    async fn healthy_ordinary_a_then_b_completes() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
            dir.path(),
        ).with_test_daemon_port(port));
        run_headless_case(manager.clone(), port, std::time::Duration::from_secs(20), std::time::Duration::from_secs(60), |manager, origins, token| async move {
            let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
            for request_id in ["health-a", "health-b"] {
                let response = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id={request_id}")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::OK);
                let payload: serde_json::Value = response.json().await.unwrap();
                assert_eq!(payload["started"], serde_json::json!(true));
                for _ in 0..40 {
                    let observation: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    if observation["fault_observation"]["native_busy"] == serde_json::json!(false) { break; }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            assert!(manager.status().await.running, "healthy A/B must keep daemon alive");
            assert!(manager.last_child_exit().is_none(), "healthy A/B must not be watchdog-exited");
            eprintln!("r20 healthy_a_b still_running=true exit=None");
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires feature headless daemon; cancelled A without B must retain lease"]
    async fn ordinary_cancel_without_b_retains_lease() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
            dir.path(),
        ).with_test_daemon_port(port));
        run_headless_case(manager.clone(), port, std::time::Duration::from_secs(20), std::time::Duration::from_secs(120), |manager, origins, token| async move {
            let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
            let first_pid = manager.status().await.pid.expect("cancel child pid");
            let response = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=cancel-a")).header("x-ai-mate-control-token", &token).send().await.unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let mut observation = serde_json::Value::Null;
            for _ in 0..40 {
                observation = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                if observation["fault_observation"]["job"]["request_id"] == serde_json::json!("cancel-a") && observation["fault_observation"]["job"]["still_blocking"] == serde_json::json!(true) { break; }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            assert_eq!(observation["fault_observation"]["native_busy"], serde_json::json!(true));
            let cancelled = client.post(format!("{origins}/api/voice/diagnostics/test-cancel")).header("x-ai-mate-control-token", &token).send().await.unwrap();
            assert_eq!(cancelled.status(), reqwest::StatusCode::OK);
            let after: serde_json::Value = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
            assert_eq!(after["fault_observation"]["native_busy"], serde_json::json!(true));
            assert_eq!(after["fault_observation"]["job"]["request_id"], serde_json::json!("cancel-a"));
            assert_eq!(after["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));
            let mut exited = None;
            for _ in 0..240 {
                if let Some(observed) = manager.last_child_exit() && observed.pid == first_pid { exited = Some(observed); break; }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            let exit = exited.expect("cancelled A without B must self-watchdog");
            assert_eq!(exit.code, Some(75));
            eprintln!("r20 cancel_without_b pid={} exit={:?} still_after_cancel={}", first_pid, exit.code, after["fault_observation"]["job"]["still_blocking"]);
        }).await;
    }

    #[tokio::test]
    #[ignore = "requires feature headless daemon; a single ordinary stuck job must self-watchdog"]
    async fn single_ordinary_stuck_exits_without_second_request() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        assert!(models_dir.join("asr/model.int8.onnx").is_file());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)),
            dir.path(),
        ).with_test_daemon_port(port));
        run_headless_case(
            manager.clone(),
            port,
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(120),
            |manager, origins, token| async move {
                let client = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build().unwrap();
                let first_pid = manager.status().await.pid.expect("single child pid");
                assert!(!manager.snapshot().instance_id.is_empty());
                let first_instance = manager.snapshot().instance_id.clone();
                let response = client.post(format!("{origins}/api/voice/diagnostics/test-utterance?request_id=single-a")).header("x-ai-mate-control-token", &token).send().await.unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::OK);
                let mut observation = serde_json::Value::Null;
                for _ in 0..40 {
                    observation = client.get(format!("{origins}/api/voice/diagnostics/test-hardware-calls")).header("x-ai-mate-control-token", &token).send().await.unwrap().json().await.unwrap();
                    if observation["fault_observation"]["job"]["request_id"] == serde_json::json!("single-a")
                        && observation["fault_observation"]["job"]["still_blocking"] == serde_json::json!(true)
                    { break; }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                assert_eq!(observation["fault_observation"]["job"]["request_id"], serde_json::json!("single-a"));
                assert_eq!(observation["fault_observation"]["job"]["pid"], serde_json::json!(first_pid));
                assert_eq!(observation["fault_observation"]["job"]["still_blocking"], serde_json::json!(true));
                assert_eq!(observation["fault_observation"]["native_busy"], serde_json::json!(true));
                let mut exited = None;
                for _ in 0..240 {
                    if let Some(observed) = manager.last_child_exit()
                        && observed.pid == first_pid
                    { exited = Some(observed); break; }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                let exit = exited.expect("single A must self-watchdog without B");
                assert_eq!(exit.pid, first_pid);
                assert_eq!(exit.code, Some(75));
                let mut new_instance = String::new();
                let mut new_pid = None;
                for _ in 0..80 {
                    let status = manager.status().await;
                    if status.running && status.pid != Some(first_pid) {
                        new_pid = status.pid;
                        if let Ok(response) = client.get(format!("{origins}/api/voice/snapshot")).header("x-ai-mate-control-token", &token).send().await
                            && let Ok(snapshot) = response.json::<serde_json::Value>().await
                            && let Some(instance) = snapshot["instance_id"].as_str()
                            && !instance.is_empty()
                            && instance != first_instance
                        { new_instance = instance.to_owned(); break; }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                assert!(!new_instance.is_empty(), "new instance must be non-empty");
                assert_ne!(new_instance, first_instance);
                assert_eq!(new_pid, manager.status().await.pid);
                assert_ne!(new_pid, Some(first_pid));
                eprintln!("r18 single request={} pid={} exit={:?} new_pid={:?} new_instance={}", observation["fault_observation"]["job"]["request_id"], first_pid, exit.code, new_pid, new_instance);
            },
        ).await;
    }

    #[tokio::test]
    #[ignore = "intentional nonzero verifier: requires feature headless daemon; run through R14 cleanup runner"]
    async fn forced_cleanup_failure_must_exit_nonzero() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)), dir.path()).with_test_daemon_port(port));
        let mode = std::env::var("R13_CLEANUP_FAILURE_MODE").unwrap_or_else(|_| "assertion".into());
        let timeout = if mode == "timeout" { std::time::Duration::from_secs(20) } else { std::time::Duration::from_secs(30) };
        run_headless_case(manager, port, std::time::Duration::from_secs(20), timeout, |_, _, _| async move {
            if mode == "timeout" {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            } else {
                panic!("forced_main_assertion");
            }
        })
        .await;
        panic!("forced_failure_must_not_return_normally");
    }

    #[tokio::test]
    #[ignore = "intentional nonzero verifier: requires feature headless daemon; health stage must fail and clean"]
    async fn forced_health_stage_failure_must_exit_nonzero() {
        verify_feature_headless_binary();
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(Arc::new(nomifun_voice_models::ModelManager::new(&models_dir)), dir.path()).with_test_daemon_port(port));
        run_headless_case(manager, port, std::time::Duration::from_millis(1), std::time::Duration::from_secs(30), |_, _, _| async move {
            panic!("health_body_must_not_run");
        })
        .await;
        panic!("health_failure_must_not_return_normally");
    }
    #[test]
    #[ignore = "requires feature-enabled daemon binary and read-only models"]
    fn feature_daemon_without_headless_refuses_before_hardware() {
        verify_feature_headless_binary();
        let bin = std::env::var_os("VOICE_COMPANION_BIN").map(std::path::PathBuf::from).expect("feature daemon binary");
        let models_dir = std::env::var_os("FLOWY_VOICE_R11_MODELS").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("D:/tmp/flowy-dev/models"));
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, format!("[models]\nmodels_dir = '{}'\nauto_download = false\n", models_dir.display())).unwrap();
        let mut child = std::process::Command::new(bin)
            .arg("--config").arg(&config)
            .arg("--host").arg("127.0.0.1:0")
            .arg("--bypass-pairing")
            .env("NOMIFUN_VOICE_FAULT_BLOCK_ASR", "0")
            .env_remove("NOMIFUN_VOICE_FAULT_HEADLESS")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn feature negative control");
        let mut status = None;
        for _ in 0..50 {
            if let Some(observed) = child.try_wait().expect("wait feature negative control") {
                status = Some(observed);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let output = match status {
            Some(_) => child.wait_with_output().expect("collect feature negative output"),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("feature negative control exceeded 5s and was reaped");
            }
        };
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("native_fault_feature_requires_valid_headless"));
    }
}
