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

pub struct ConversationVoiceDialogueRecorder {
    pub conversation_service: Arc<nomifun_conversation::ConversationService>,
    pub companion_service: Option<Arc<CompanionService>>,
    pub target_config: Arc<RwLock<VoiceTargetConfig>>,
    pub active_window_candidate: Arc<RwLock<Option<String>>>,
    pub user_id: String,
    journal_path: PathBuf,
    journal_lock: Mutex<()>,
    execution_runtime:Option<Arc<dyn VoiceTaskRuntime>>,
}

impl ConversationVoiceDialogueRecorder {
    pub fn new(
        conversation_service: Arc<nomifun_conversation::ConversationService>,
        user_id: String,
        data_dir: &Path,
    ) -> Self {
        Self {
            journal_path: data_dir.join("voice").join(format!("dialogue-{user_id}.json")),
            journal_lock: Mutex::new(()),
            execution_runtime:None,
            conversation_service,
            companion_service: None,
            target_config: Arc::new(RwLock::new(VoiceTargetConfig::default())),
            active_window_candidate: Arc::new(RwLock::new(None)),
            user_id,
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

    pub fn with_active_window_candidate(mut self, candidate: Arc<RwLock<Option<String>>>) -> Self {
        self.active_window_candidate = candidate;
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

    fn update_workspace_interaction(&self, conv_id: &str, now: i64) -> Result<(), VoiceDialogueRecorderError> {
        let mut registry = self.read_workspace_sessions()?;
        let mut changed = false;
        for entry in registry.sessions.values_mut() {
            if entry.conversation_id == conv_id {
                entry.last_user_interaction_at = now;
                changed = true;
            }
        }
        if changed {
            self.save_workspace_sessions(&registry)?;
        }
        Ok(())
    }

    async fn resolve_target_conversation(&self, candidate: Option<String>, config: VoiceTargetConfig)
        -> Result<(String, Option<String>), VoiceDialogueRecorderError> {
        let now = nomifun_common::now_ms();
        let (candidate_workspace, source_id) = if let Some(id) = candidate {
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
                (None, None)
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
            let make_req = || nomifun_api_types::CreateConversationRequest {
                name: Some(title.clone()),
                r#type: nomifun_common::AgentType::Nomi,
                source: Some(nomifun_common::ConversationSource::Nomifun),
                extra: serde_json::json!({
                    "voice": true,
                    "is_default_voice": true,
                    "memory_owner_companion_id": default_companion.clone(),
                    "last_user_interaction_at": now,
                }),
                model: None,
                preset_id: None,
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
                        if (owner_matches && idle_ms <= 30 * 60 * 1000) || is_voice_busy {
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

        // Selection matrix fallthrough for candidates without custom workspace
        if let Some(id) = source_id.as_deref() {
            let is_busy = self.conversation_service.is_conversation_busy(&self.user_id, id).await.unwrap_or(false);
            if !is_busy {
                return Ok((id.to_string(), source_id));
            }
        }

        if config.mode == VoiceTargetMode::Companion && let (Some(id), Some(companions)) = (&config.companion_id, &self.companion_service) {
            if companions.get_companion(id).await.is_ok() {
                let conversation = match companions.companion_active_thread(id).await {
                    Ok(Some(thread)) => Some(thread),
                    Ok(None) => Some(companions.create_companion_thread(id, None).await.map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?.conversation_id),
                    Err(e) => return Err(VoiceDialogueRecorderError::WriteFailed(e.to_string())),
                };
                if let Some(conversation) = conversation {
                    return Ok((conversation.clone(), source_id.or(Some(conversation))));
                }
            }
        }
        let memory_owner = match &self.companion_service {Some(companions)=>companions.default_companion_id().await,None=>None};
        let home = self.conversation_service.ensure_default_voice_conversation_with_owner(&self.user_id,memory_owner.as_deref()).await.map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
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
    handoff_content: Option<String>,
    #[serde(default)]
    voice_context_digest: Option<String>,
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
            .filter(|m| matches!(*m, "default" | "auto_edit" | "yolo"))
            .map(str::to_owned);
        if conv.extra.get("is_default_voice").and_then(|v|v.as_bool())!=Some(true)
            && let Some(workspace)=conv.extra.get("workspace").and_then(|v|v.as_str()).filter(|s|!s.is_empty()) {
            let model=conv.model.as_ref().and_then(|model|serde_json::to_value(model).ok()).and_then(|value|serde_json::from_value::<VoiceModelRef>(value).ok());
            return Ok(Some(VoiceExecutionConfig{workspace:workspace.into(),model,preset_id:conv.preset_id,session_mode}));
        }
        let config_path=self.journal_path.parent().and_then(Path::parent).unwrap().join("config").join("voice_companion.json");
        let explicit = match std::fs::read(config_path) {
            Ok(bytes)=> {
                let root:serde_json::Value=serde_json::from_slice(&bytes).map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))?;
                serde_json::from_value(root.get("execution").cloned().unwrap_or(serde_json::Value::Null))
                    .map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))?
            },
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>None,
            Err(e)=>return Err(VoiceDialogueRecorderError::WriteFailed(e.to_string())),
        };
        if let Some(config) = explicit {
            return Ok(Some(config));
        }
        // Fallback: This conversation's own bound/managed workspace
        if let Some(workspace) = conv.extra.get("workspace").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let path = Path::new(workspace);
            if path.is_absolute() && path.is_dir() {
                let model = conv.model.as_ref().and_then(|m| serde_json::to_value(m).ok()).and_then(|v| serde_json::from_value::<VoiceModelRef>(v).ok());
                return Ok(Some(VoiceExecutionConfig { workspace: workspace.into(), model, preset_id: conv.preset_id, session_mode }));
            }
        }
        Ok(None)
    }
}

#[async_trait::async_trait]
impl VoiceDialogueRecorder for ConversationVoiceDialogueRecorder {
    async fn keyboard_configuration(&self, conversation_id:&str) -> Result<crate::voice_dialogue::FrozenVoiceExecution,String> {
        let conv = self.conversation_service.get(&self.user_id, conversation_id).await.map_err(|_|"binding_invalid")?;
        if conv.extra.get("voice").and_then(|v|v.as_bool()) != Some(true) {return Err("binding_invalid".into());}
        let config = VoiceExecutionConfig {
            workspace: conv.extra.get("workspace").and_then(|v|v.as_str()).ok_or("task_workspace_invalid")?.into(),
            model: conv.model.as_ref().and_then(|m|serde_json::to_value(m).ok()).and_then(|m|serde_json::from_value(m).ok()),
            preset_id: conv.preset_id,
            session_mode: conv.extra.get("session_mode").and_then(|v|v.as_str()).map(str::to_owned),
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
    async fn freeze_session(&self,session:&str)->Result<Option<nomifun_api_types::voice::VoiceBindingSnapshot>,VoiceDialogueRecorderError> {
        let _guard=self.journal_lock.lock().await;
        let mut records=self.read_records()?;
        if !records.iter().any(|r|r.request.session_id.as_deref()==Some(session)) {
            let candidate=self.active_window_candidate.read().unwrap_or_else(|e|e.into_inner()).clone();
            let config=self.target_config.read().unwrap_or_else(|e|e.into_inner()).clone();
            let (id,source)=self.resolve_target_conversation(candidate,config).await?;
            let source=source.as_deref().unwrap_or(&id);
            let execution=match self.frozen_execution(source).await {
                Ok(config)=>config,
                Err(VoiceDialogueRecorderError::BindingInvalid(_))=>self.frozen_execution(&id).await?,
                Err(error)=>return Err(error),
            };
            let source_workspace=execution.as_ref().map(|c|c.workspace.clone());
            let (execution,frozen_preset)=match (execution,&self.execution_runtime) {
                (Some(config),Some(runtime))=>match runtime.prepare_configuration(&config).await {
                    Ok((config,preset))=>(Some(config),preset),Err(_)=>(None,None),
                },
                (config,_) => (config,None),
            };
            let epoch=self.conversation_service.conversation_repo().get_turn_admission_state(&self.user_id,&id).await.map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))?.epoch;
            records.push(DialogueRecord {request:VoiceDialogueRequest {session_id:Some(session.into()),binding_id:Some(format!("voice-binding-{session}")),..Default::default()},conversation_id:id,admission_epoch:epoch,execution,frozen_preset,source_workspace,output:None,projected:false,command_id:None,requested_at:None,message_id:None,handoff_content:None,voice_context_digest:None});
            self.save_records(&records)?;
        }
        let binding=records.iter().find(|r|r.request.session_id.as_deref()==Some(session)).unwrap();
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

    async fn preflight(&self,request:&VoiceDialogueRequest)->Result<VoiceDialoguePreflight,VoiceDialogueRecorderError> {
        let session=request.session_id.as_deref().filter(|s|!s.is_empty()).ok_or_else(||VoiceDialogueRecorderError::BindingInvalid("scope_required".into()))?;
        let turn=request.turn_id.as_deref().filter(|s|!s.is_empty()).ok_or_else(||VoiceDialogueRecorderError::BindingInvalid("scope_required".into()))?;
        if request.voice_record_id.as_deref()!=Some(format!("voice-{turn}").as_str())
            || request.binding_id.as_deref()!=Some(format!("voice-binding-{session}").as_str())
            || request.raw_user_text.as_deref().is_none_or(|s|s.trim().is_empty()) {
            return Err(VoiceDialogueRecorderError::BindingInvalid("scope_invalid".into()));
        }
        self.freeze_session(session).await?;
        let _guard=self.journal_lock.lock().await;
        let mut records=self.read_records()?;
        if let Some(record)=records.iter().find(|r|r.request.voice_record_id==request.voice_record_id) {
            if record.request!=*request {return Err(VoiceDialogueRecorderError::Conflict);}
            return match &record.output {
                Some(content) if record.projected=>Ok(VoiceDialoguePreflight::Cached{content:content.clone()}),
                Some(content)=>Ok(VoiceDialoguePreflight::Generated{conversation_id:record.conversation_id.clone(),content:content.clone()}),
                None=>Err(VoiceDialogueRecorderError::GenerationUnknown),
            };
        }
        let frozen=records.iter().find(|r|r.request.session_id==request.session_id).cloned()
            .ok_or_else(||VoiceDialogueRecorderError::BindingInvalid("binding_missing".into()))?;
        let (conversation_id,execution,frozen_preset,source_workspace)=(frozen.conversation_id,frozen.execution,frozen.frozen_preset,frozen.source_workspace);
        let epoch=self.conversation_service.conversation_repo().get_turn_admission_state(&self.user_id,&conversation_id).await
            .map_err(|e|VoiceDialogueRecorderError::WriteFailed(e.to_string()))?.epoch;
        let has_unsettled = records.iter().any(|r| {
            r.request.session_id == request.session_id
                && r.request.voice_record_id.is_some()
                && !r.projected
        });
        if has_unsettled {
            if records.iter().find(|r| r.request.session_id == request.session_id).is_some_and(|r| r.admission_epoch != epoch) {
                return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
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
            handoff_content: None,
            voice_context_digest: None,
        });
        // The original ASR, frozen target/config and generation claim precede the model side effect.
        self.save_records(&records)?;
        Ok(VoiceDialoguePreflight::Ready{conversation_id})
    }

    async fn record_pair(&self,conversation_id:&str,request:&VoiceDialogueRequest,assistant_content:&str)->Result<(),VoiceDialogueRecorderError> {
        let _guard=self.journal_lock.lock().await;
        let mut records=self.read_records()?;
        let admission_epoch;
        let user_message_id;
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
        }
        self.save_records(&records)?; // Never regenerate an answer when transcript projection fails.

        let decision=nomifun_api_types::voice::parse_voice_decision(assistant_content).map_err(|_|VoiceDialogueRecorderError::DecisionInvalid)?;
        let understanding=decision.understood_request.as_deref();
        let reply=decision.reply_tts.as_deref();
        let display=match (understanding,reply) {
            (Some(understanding),Some(reply))=>format!("{understanding}\n\n{reply}"),
            (Some(text),None)|(None,Some(text))=>text.to_owned(),
            _=>assistant_content.to_owned(),
        };

        let current_epoch = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?.epoch;
        if current_epoch != admission_epoch {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
        }

        let raw_user_text = request.raw_user_text.as_deref().unwrap_or("").trim();
        let op_id = request.voice_record_id.as_deref().unwrap_or("voice-turn");

        let is_busy = self.conversation_service.is_conversation_busy(&self.user_id, conversation_id).await.unwrap_or(false);
        if !is_busy {
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
                Ok(_) => {
                    self.update_workspace_interaction(conversation_id, nomifun_common::now_ms())?;
                    records.iter_mut().find(|r| r.request.voice_record_id == request.voice_record_id).unwrap().projected = true;
                    return self.save_records(&records);
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

        // Busy append fallback:
        let current_epoch = self.conversation_service.conversation_repo()
            .get_turn_admission_state(&self.user_id, conversation_id).await
            .map_err(|e| VoiceDialogueRecorderError::WriteFailed(e.to_string()))?.epoch;
        if current_epoch != admission_epoch {
            return Err(VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into()));
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

        self.conversation_service.project_assistant_message_idempotent_with_epoch(
            &self.user_id,
            conversation_id,
            op_id,
            &display,
            "voice_companion",
            Some(admission_epoch),
        ).await.map_err(|e| match e {
            nomifun_common::AppError::Conflict(ref msg) if msg.contains("epoch") || msg.contains("revision") => {
                VoiceDialogueRecorderError::BindingInvalid("target_revision_changed".into())
            }
            nomifun_common::AppError::Conflict(msg) => VoiceDialogueRecorderError::BindingBusy(msg),
            _ => VoiceDialogueRecorderError::WriteFailed(e.to_string()),
        })?;

        self.update_workspace_interaction(conversation_id, nomifun_common::now_ms())?;
        records.iter_mut().find(|r| r.request.voice_record_id == request.voice_record_id).unwrap().projected = true;
        self.save_records(&records)
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

pub struct VoiceCompanionProcessManager {
    audio_recovery:std::sync::Mutex<AudioRecovery>,
    prompted_approvals:std::sync::Mutex<std::collections::VecDeque<String>>,
    revoked_sessions: RwLock<std::collections::VecDeque<String>>,
    task_feedback: std::sync::Mutex<TaskFeedbackTracker>,
    daemon_origin: String,
    tasks: Option<Arc<VoiceTaskService>>,
    data_dir: PathBuf,
    task_projection_started: AtomicBool,
    dialogue_completer: Option<Arc<dyn VoiceDialogueCompleter>>,
    dialogue_recorder: Option<Arc<dyn VoiceDialogueRecorder>>,
    dialogue_selection: Arc<RwLock<Option<VoiceModelRef>>>,
    target_selection: Arc<RwLock<VoiceTargetConfig>>,
    active_window_candidate: Arc<RwLock<Option<String>>>,
    companion_service: Option<Arc<CompanionService>>,
    dialogue_bridge: Mutex<Option<VoiceDialogueBridge>>,
    config_write_lock: std::sync::Mutex<()>,
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
}

static GLOBAL_COMPANION_MANAGER: std::sync::OnceLock<Arc<VoiceCompanionProcessManager>> =
    std::sync::OnceLock::new();

impl VoiceCompanionProcessManager {
    pub fn register_global(manager: Arc<Self>) {
        let _ = GLOBAL_COMPANION_MANAGER.set(manager);
    }

    pub fn global() -> Option<Arc<Self>> {
        GLOBAL_COMPANION_MANAGER.get().cloned()
    }

    pub fn new(models_manager: Arc<nomifun_voice_models::ModelManager>, data_dir: &Path) -> Self {
        let config_path = data_dir.join("config").join("voice_companion.json");
        let selection = std::fs::read(&config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<VoiceDialogueConfig>(&bytes).ok())
            .and_then(|config| config.dialogue_model);
        let target_config = std::fs::read(&config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|val| val.get("target").cloned())
            .and_then(|target_val| serde_json::from_value::<VoiceTargetConfig>(target_val).ok())
            .unwrap_or_default();
        let (events, _) = broadcast::channel(128);
        Self {
            audio_recovery:std::sync::Mutex::new(AudioRecovery::default()),
            prompted_approvals:std::sync::Mutex::new(std::collections::VecDeque::new()),
            revoked_sessions:RwLock::new(std::collections::VecDeque::new()),
            task_feedback: std::sync::Mutex::new(TaskFeedbackTracker::new()),
            daemon_origin: "http://127.0.0.1:18080".into(),
            tasks: None,
            data_dir: data_dir.to_path_buf(),
            task_projection_started: AtomicBool::new(false),
            dialogue_completer: None,
            dialogue_recorder: None,
            dialogue_selection: Arc::new(RwLock::new(selection)),
            target_selection: Arc::new(RwLock::new(target_config)),
            active_window_candidate: Arc::new(RwLock::new(None)),
            companion_service: None,
            dialogue_bridge: Mutex::new(None),
            config_write_lock: std::sync::Mutex::new(()),
            feedback_update_lock: Mutex::new(()),
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

    pub fn active_window_candidate(&self) -> Arc<RwLock<Option<String>>> {
        self.active_window_candidate.clone()
    }

    pub fn set_candidate_conversation(&self, candidate: Option<String>) {
        *self.active_window_candidate.write().unwrap_or_else(|e| e.into_inner()) = candidate;
    }

    pub fn requested_target(&self) -> VoiceTargetConfig {
        self.target_selection.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub async fn target_settings(&self) -> VoiceTargetSettings {
        let requested = self.requested_target();
        let (effective, effective_conversation_id, reason) = match requested.mode {
            VoiceTargetMode::VoiceHome => {
                (VoiceTargetConfig { mode: VoiceTargetMode::VoiceHome, companion_id: None }, None, None)
            }
            VoiceTargetMode::Companion => {
                if let Some(companion_id) = &requested.companion_id {
                    if let Some(companion_service) = &self.companion_service {
                        if companion_service.get_companion(companion_id).await.is_ok() {
                            let conv_id = companion_service.companion_active_thread(companion_id).await.ok().flatten();
                            (
                                VoiceTargetConfig { mode: VoiceTargetMode::Companion, companion_id: Some(companion_id.clone()) },
                                conv_id,
                                None,
                            )
                        } else {
                            (
                                VoiceTargetConfig { mode: VoiceTargetMode::VoiceHome, companion_id: None },
                                None,
                                Some("companion_not_found".to_string()),
                            )
                        }
                    } else {
                        (
                            VoiceTargetConfig { mode: VoiceTargetMode::VoiceHome, companion_id: None },
                            None,
                            Some("companion_service_unavailable".to_string()),
                        )
                    }
                } else {
                    (
                        VoiceTargetConfig { mode: VoiceTargetMode::VoiceHome, companion_id: None },
                        None,
                        Some("companion_id_required".to_string()),
                    )
                }
            }
        };
        VoiceTargetSettings {
            requested,
            effective,
            effective_conversation_id,
            reason,
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

    fn requested_execution(&self) -> Result<Option<VoiceExecutionConfig>, String> {
        let bytes = match std::fs::read(&self.config_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("configuration_read_failed".into()),
        };
        let root: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "configuration_invalid")?;
        let root = root.as_object().ok_or("configuration_invalid")?;
        match root.get("execution") {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(value) => serde_json::from_value(value.clone()).map(Some).map_err(|_| "execution_configuration_invalid".into()),
        }
    }

    async fn execution_settings(&self) -> VoiceExecutionSettings {
        let (requested, read_error) = match self.requested_execution() {
            Ok(requested) => (requested,None),
            Err(code) => (None,Some(code)),
        };
        let fallback = if requested.is_none() && read_error.is_none() {
            if let Some(recorder) = &self.dialogue_recorder {
                recorder.fallback_execution().await.ok().flatten()
            } else {
                None
            }
        } else {
            None
        };
        let candidate = requested.as_ref().or(fallback.as_ref());
        let resolution = match (read_error, candidate, &self.tasks) {
            (Some(code), _, _) => Err(code),
            (None, None, _) => Err("execution_unconfigured".into()),
            (None, Some(_), None) => Err("runtime_unavailable".into()),
            (None, Some(config), Some(tasks)) => tasks.resolve_configuration(config).await,
        };
        let (effective, reason) = match resolution { Ok(config) => (Some(config),None), Err(code) => (None,Some(code)) };
        let settings = VoiceExecutionSettings {
            effective,
            available: reason.is_none(),
            requested,
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

    async fn update_execution(&self, requested: Option<VoiceExecutionConfig>) -> Result<VoiceExecutionSettings, String> {
        if let Some(config) = &requested {
            crate::voice_task_runtime::validate_workspace(config)?;
            if let Some(tasks) = &self.tasks {
                tasks.resolve_configuration(config).await?;
            } else if let (Some(model), Some(completer)) = (&config.model, &self.dialogue_completer) {
                completer.resolve_model(Some(model)).await?;
            }
        }
        let value = serde_json::to_value(requested).map_err(|_| "execution_configuration_invalid")?;
        self.write_config_entry("execution", value)?;
        Ok(self.execution_settings().await)
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
                        let notification=tasks.notification_for(&task.command_id).ok().flatten();
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
                    if let (Some(task),Some(confirmation))=(active.clone(),approval) {
                        let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.notify_confirmation(&task,&confirmation).await {owner.project_task_error(code);}}));
                    } else if let Ok(Some((task,goal)))=tasks.pending_result() {
                        manager.task_feedback.lock().unwrap_or_else(|e|e.into_inner()).clear_pending();
                        let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.notify_task_result(&task,&goal).await {owner.project_task_error(code);}}));
                    } else if let Some(task)=active.filter(|task|task.status==VoiceTaskStatus::Accepted) {
                        let owner=manager.clone();speech=Some(tokio::spawn(async move {if let Err(code)=owner.maybe_notify_task_progress(&task).await {owner.project_task_error(code);}}));
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
        let tasks = self.tasks.as_ref().ok_or("runtime_unavailable")?;
        if tasks.announcement_claimed(&task.command_id)? {return Ok(());}
        let source = match self.refresh_daemon_snapshot().await {Ok(source) => source,Err(_) => return Ok(())};
        if !matches!(source.phase, VoiceSessionPhase::Idle | VoiceSessionPhase::Listening | VoiceSessionPhase::FollowUp)
            || source.ended_reason == Some(nomifun_api_types::voice::VoiceSessionEndReason::UserStop)
            || source.audio.as_ref().is_some_and(|audio| audio.speech_detected) {return Ok(());}
        let full_text = task.result_text.as_deref().filter(|text|!text.trim().is_empty())
            .unwrap_or(if task.status==VoiceTaskStatus::Completed {"任务已完成，详情请查看会话。"} else {"任务未完成，详情请查看会话。"});
        let summary = extract_spoken_summary(full_text, task.status);
        let spoken_goal: String = goal.chars().take(25).collect();
        let text = if !spoken_goal.trim().is_empty() {
            format!("任务「{spoken_goal}」的结果：{summary}")
        } else {
            summary
        };
        // Claim durably before dispatch: a lost response or host restart must
        // not replay a result as a new announcement. Text remains in the task.
        if !tasks.claim_announcement(&task.command_id,true).await? {return Ok(());}
        let notification=tasks.notification_for(&task.command_id)?.ok_or("task_announcement_failed")?;
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
        self.reconcile_announcements().await?;
        let tasks = self.tasks.as_ref().ok_or("runtime_unavailable")?;
        let task = tasks.status_for(command_id).await?;
        if !matches!(task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) {
            return Err("task_not_terminal".into());
        }
        let goal = tasks.pending_result().ok().flatten()
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
        if self.prompted_approvals.lock().unwrap_or_else(|error|error.into_inner()).contains(&confirmation.approval_id){return Ok(());}
        let source=match self.refresh_daemon_snapshot().await {Ok(source)=>source,Err(_)=>return Ok(())};
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
        let frozen = if let Some(recorder) = &self.dialogue_recorder {
            Some(recorder.task_configuration(&request.command).await?.ok_or("binding_invalid")?)
        } else {
            None
        };
        let config = match &frozen {
            Some(frozen) => frozen.config.clone(),
            None => match self.requested_execution()? {
                Some(config) => config,
                None => match &self.dialogue_recorder {
                    Some(rec) => rec.fallback_execution().await?.ok_or("execution_unconfigured")?,
                    None => return Err("execution_unconfigured".into()),
                },
            },
        };
        // Admission belongs to the host: dropping a voice request cannot cancel
        // an accepted task or abandon its durable delivery record.
        let manager = self.clone();
        let command_id = request.command.command_id.clone();
        let command_id_timeout = command_id.clone();
        let session_id = request.command.session_id.clone();
        let turn_id = request.command.turn_id.clone();
        let tasks_ref = tasks.clone();
        let handoff = frozen.as_ref().and_then(|f| f.handoff_content.clone());
        let display_text = frozen.as_ref().and_then(|f| f.display_text.clone());
        let target_conversation = frozen.as_ref().and_then(|f| f.conversation_id.clone());
        let message_id = frozen.as_ref().and_then(|f| f.message_id.clone());
        let voice_context_digest = frozen.as_ref().and_then(|f| f.voice_context_digest.clone());
        let expected_preset = frozen.map(|f| f.preset);
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
        let choice = self
            .dialogue_selection
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let effective = match &self.dialogue_completer {
            Some(completer) => completer.resolve_model(choice.as_ref()).await.ok(),
            None => None,
        };
        VoiceDialogueSettings {
            config: VoiceDialogueConfig {
                dialogue_model: choice,
            },
            available: effective.is_some(),
            effective_model: effective,
        }
    }

    async fn update_dialogue(
        &self,
        config: VoiceDialogueConfig,
    ) -> Result<VoiceDialogueSettings, String> {
        if let Some(choice) = &config.dialogue_model {
            self.dialogue_completer
                .as_ref()
                .ok_or("model_unavailable")?
                .resolve_model(Some(choice))
                .await?;
        }
        self.write_config_entry(
            "dialogue_model",
            serde_json::to_value(&config.dialogue_model).map_err(|_| "configuration_invalid")?,
        )?;
        *self
            .dialogue_selection
            .write()
            .unwrap_or_else(|e| e.into_inner()) = config.dialogue_model;
        Ok(self.dialogue_settings().await)
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
        self.project(source.clone(), generation);
        Ok(source)
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

    /// One upstream stream per host, independent of any renderer/page lifecycle.
    fn start_projection(self: &Arc<Self>) {
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
                if !manager.status().await.running {
                    manager.disconnected();
                    if manager.read_autostart_config() {
                        manager.auto_start_if_enabled().await;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
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
                    loop {
                        let chunk = tokio::select! {
                            biased;
                            _ = manager.process_changed.notified() => break,
                            _ = recovery_tick.tick() => {
                                if manager.shutdown_requested.load(Ordering::Acquire)
                                    || generation!=manager.generation.load(Ordering::Acquire)
                                    || manager.recover_audio_if_needed().await {break;}
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
                                    manager.project(snapshot.clone(), generation);
                                    if let (Some(recorder),Some(session))=(&manager.dialogue_recorder,&snapshot.session_id) {
                                        if matches!(snapshot.phase,VoiceSessionPhase::Listening|VoiceSessionPhase::Transcribing|VoiceSessionPhase::Deciding)
                                            && manager.snapshot().binding.is_none() {
                                            match recorder.freeze_session(session).await {
                                                Ok(Some(binding))=>{
                                                    let mut current=manager.snapshot.write().unwrap_or_else(|e|e.into_inner());
                                                    if current.session_id.as_ref()==Some(session) {current.binding=Some(binding);current.seq+=1;let _=manager.events.send(current.clone());}
                                                },
                                                Err(_)=>manager.project_task_error("binding_invalid".into()),
                                                _=>{},
                                            }
                                        }
                                    }
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
        self.task_feedback.lock().unwrap_or_else(|e| e.into_inner()).clear_pending();
        if !request.matches(&self.snapshot()) {
            return Err("session_scope_changed".into());
        }
        if let Some(session_id)=&request.session_id {
            let mut revoked=self.revoked_sessions.write().unwrap_or_else(|error|error.into_inner());
            if !revoked.contains(session_id) {revoked.push_back(session_id.clone());}
            if revoked.len()>64 {revoked.pop_front();}
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
        let response = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().map_err(|_|"daemon_unavailable".to_owned())?
            .post(format!("{}/api/voice/stop",self.daemon_origin))
            .header("x-ai-mate-control-token", &self.control_token)
            .json(&request)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|_| "daemon_unavailable".to_string())?;
        if response.status().is_success() {
            self.refresh_daemon_snapshot().await?;
            Ok(())
        } else {
            Err("session_scope_changed".into())
        }
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

        // 3. Known development build target directories
        for exe_name in exe_names {
            let candidates = [
                Path::new("target/debug").join(exe_name),
                Path::new("target/release").join(exe_name),
                Path::new("build.noindex/debug").join(exe_name),
                Path::new("build.noindex/release").join(exe_name),
            ];

            for c in &candidates {
                if c.is_file() {
                    return Some(c.clone());
                }
            }

            // 4. Resolve relative to workspace root if cwd is a sub-crate
            if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
                let root = Path::new(&manifest_dir).ancestors().nth(2);
                if let Some(r) = root {
                    for dir in [
                        "target/debug",
                        "target/release",
                        "build.noindex/debug",
                        "build.noindex/release",
                    ] {
                        let c = r.join(dir).join(exe_name);
                        if c.is_file() {
                            return Some(c);
                        }
                    }
                }
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
    pub async fn status(&self) -> VoiceCompanionStatus {
        let models_ready = self.models_manager.get_status().await.state.is_ready();
        let autostart = self.read_autostart_config();

        let mut lock = self.child.lock().await;
        let mut running = false;
        let mut pid = None;

        if let Some(ref mut child) = *lock {
            match child.try_wait() {
                Ok(Some(exit_status)) => {
                    info!(?exit_status, "Voice companion child process has terminated");
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
        }
    }

    /// Launches the `ai-mate` companion daemon under whole-tree supervision.
    pub async fn start(self: &Arc<Self>) -> Result<(), String> {
        self.shutdown_requested.store(false, Ordering::Release);
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

        let bin_path = Self::resolve_voice_companion_binary().ok_or_else(|| {
            "voice-companion binary not found. Please compile voice-companion with `cargo build -p voice-companion` first.".to_string()
        })?;

        let feedback_config = serde_json::to_string(&self.requested_feedback()?)
            .map_err(|_| "configuration_invalid")?;
        info!(binary = %bin_path.display(), "Spawning Voice Companion daemon...");
        self.starting();

        let mut builder = ChildProcessBuilder::new(&bin_path);
        builder.arg("--bypass-pairing");
        builder.arg("--host").arg("127.0.0.1:18080");
        builder.env("VOICE_COMPANION_MODEL_DIR", self.models_manager.models_dir());
        builder.env("AI_MATE_MODEL_DIR", self.models_manager.models_dir());
        builder.env("VOICE_COMPANION_CONTROL_TOKEN", &self.control_token);
        builder.env("AI_MATE_CONTROL_TOKEN", &self.control_token);
        builder.env("VOICE_COMPANION_FEEDBACK_CONFIG", &feedback_config);
        builder.env("AI_MATE_FEEDBACK_CONFIG", feedback_config);
        if let Some(completer) = &self.dialogue_completer {
            let mut bridge = self.dialogue_bridge.lock().await;
            if bridge.is_none() {
                *bridge = Some(
                    VoiceDialogueBridge::start_with_recorder(
                        completer.clone(),
                        self.dialogue_selection.clone(),
                        self.control_token.clone(),
                        self.private_task_routes(),
                        self.dialogue_recorder.clone(),
                    )
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
    let request_scope=VoiceTaskControlRequest {instance_id:manager.snapshot().instance_id,task:request.task};
    match manager.control_task(request_scope,request.action).await {
        Ok(task) => axum::Json(task).into_response(),Err(code) => task_api_error(code),
    }
}

/// Extracts a concise, high-signal spoken summary (50-80 chars) suitable for TTS playback.
///
/// Preserves:
/// - Explicit conclusion sentences (e.g. "已完成", "成功", "未找到", "失败");
/// - Vital parameters (temperatures, numbers, dates, locations, units);
/// - Negations ("不", "未", "无", "失败", "没有", "无法") - never drops or inverts polarity;
/// - Strips markdown noise, code blocks, and stage directions.
pub fn extract_spoken_summary(raw: &str, status: VoiceTaskStatus) -> String {
    let text = strip_markdown_and_noise(raw);
    let text = text.trim();
    if text.is_empty() {
        return match status {
            VoiceTaskStatus::Completed => "任务已完成，详情请在会话中查看。".into(),
            _ => "任务未完成，详情请在会话中查看。".into(),
        };
    }
    // If it's already concise enough (under 80 chars), speak as is.
    if text.chars().count() <= 80 {
        return text.to_string();
    }

    let has_failure = is_negation_or_failure(text) || status == VoiceTaskStatus::Failed;
    const MAX_TARGET_CHARS: usize = 65;

    let sentences = split_sentences(text);
    if sentences.is_empty() {
        let truncated: String = text.chars().take(MAX_TARGET_CHARS).collect();
        let res = format!("{truncated}，详情请在会话中查看。");
        if has_failure && !is_negation_or_failure(&res) {
            return "任务执行结果包含未完成事项与重要说明，详情请在会话中查看。".to_string();
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
                let res = format!("{trimmed}，详情请在会话中查看。");
                if has_failure && !is_negation_or_failure(&res) {
                    return "任务执行结果包含未完成事项与重要说明，详情请在会话中查看。".to_string();
                }
                return res;
            }

            selected_clause_indices.sort_unstable(); // keep chronological order
            let mut clause_summary = String::new();
            for &c_idx in &selected_clause_indices {
                let c = clauses[c_idx].trim();
                if !c.is_empty() {
                    if !clause_summary.is_empty() && !clause_summary.ends_with(['，', ',']) {
                        clause_summary.push('，');
                    }
                    clause_summary.push_str(c);
                }
            }
            let trimmed = clause_summary.trim_end_matches(['，', ',']);
            let res = format!("{trimmed}，详情请在会话中查看。");
            if has_failure && !is_negation_or_failure(&res) {
                return "任务执行结果包含未完成事项与重要说明，详情请在会话中查看。".to_string();
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
                summary.push('，');
            }
            summary.push_str(s);
        }
    }

    let summary = summary.trim_end_matches(['。', '！', '!', '？', '?', '；', ';', '，', ',']);
    let result = if summary.chars().count() < text.chars().count() {
        format!("{summary}，详情请在会话中查看。")
    } else {
        summary.to_string()
    };
    if has_failure && !is_negation_or_failure(&result) {
        return "任务执行结果包含未完成事项与重要说明，详情请在会话中查看。".to_string();
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
    ) || (text.contains('未') && !text.contains("未来"))
        || (text.contains('无') && !text.contains("无论"))
        || (text.contains('不') && !text.contains("不但") && !text.contains("不仅"))
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|k| text.contains(k))
}

fn split_sentences(text: &str) -> Vec<&str> {
    text.split(|c| matches!(c, '。' | '！' | '!' | '？' | '?' | '\n' | '；' | ';'))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
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

pub fn voice_companion_routes(manager: Arc<VoiceCompanionProcessManager>) -> axum::Router {
    use axum::routing::{get, post};
    manager.start_projection();
    manager.start_task_projection();
    axum::Router::new()
        .route("/api/voice/companion/execution", get(companion_execution_handler).post(companion_update_execution_handler))
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
            get(companion_config_handler).post(companion_update_config_handler),
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

async fn companion_update_execution_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(requested): axum::Json<Option<VoiceExecutionConfig>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.update_execution(requested).await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => task_api_error(code),
    }
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
    match tasks.notification_for(&command_id) {
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

async fn companion_update_config_handler(
    axum::extract::State(manager): axum::extract::State<Arc<VoiceCompanionProcessManager>>,
    axum::Json(config): axum::Json<VoiceDialogueConfig>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    match manager.update_dialogue(config).await {
        Ok(settings) => axum::Json(settings).into_response(),
        Err(code) => (
            axum::http::StatusCode::BAD_REQUEST,
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
            conversation:vec![nomifun_api_types::voice::VoiceConversationTurn {turn_id:"old-turn".into(),user_text:Some("读取文件".into()),assistant_text:Some("文件里的真实结果".into()),..Default::default()}],..Default::default()};
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
        let config_dir = dir.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("voice_companion.json"), serde_json::to_vec(&serde_json::json!({
            "autostart": true,
            "execution": {
                "workspace": dir.path().to_str().unwrap(),
                "model": {"provider_id": "test-p", "model": "test-m"}
            }
        })).unwrap()).unwrap();

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

        let created_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let runtime = Arc::new(SlowRuntime { created: created_flag.clone() });
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path(),
        ).with_tasks("test-owner".into(), runtime));

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
    async fn execution_configuration_preserves_other_preferences_and_rejects_implicit_workspaces() {
        use tower::ServiceExt;
        let dir = tempfile::tempdir().unwrap();
        let manager = Arc::new(VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())), dir.path(),
        ));
        manager.write_config_entry("dialogue_model", serde_json::json!({"provider_id":"kept-provider", "model":"kept-chat"})).unwrap();
        manager.write_config_entry("future", serde_json::json!({"keep":true})).unwrap();
        let app = voice_companion_routes(manager.clone());
        let config = serde_json::json!({"workspace":dir.path(), "model":{"provider_id":"requested-provider", "model":"requested-model"}, "preset_id":null, "api_key":"must-not-be-stored"});
        let request = |body: &serde_json::Value| axum::http::Request::builder().method("POST").uri("/api/voice/companion/execution")
            .header("content-type","application/json").body(axum::body::Body::from(body.to_string())).unwrap();
        let response = app.clone().oneshot(request(&config)).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let settings: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(response.into_body(),16_384).await.unwrap()).unwrap();
        assert_eq!(settings["requested"]["workspace"], config["workspace"]);
        assert_eq!(settings["available"],false);
        assert_eq!(settings["reason_code"],"runtime_unavailable");
        let saved = std::fs::read(&manager.config_path).unwrap();
        let root: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(root["future"]["keep"],true);
        assert_eq!(root["dialogue_model"]["model"],"kept-chat");
        assert!(root["execution"].get("api_key").is_none());
        let mut invalid = config;
        invalid["workspace"] = "relative".into();
        let rejected = app.oneshot(request(&invalid)).await.unwrap();
        assert_eq!(rejected.status(),axum::http::StatusCode::BAD_REQUEST);
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
    async fn dialogue_selection_and_enable_intent_preserve_each_other() {
        struct Models;
        #[async_trait::async_trait]
        impl VoiceDialogueCompleter for Models {
            async fn resolve_model(
                &self,
                choice: Option<&VoiceModelRef>,
            ) -> Result<VoiceModelRef, String> {
                choice.cloned().ok_or_else(|| "model_unavailable".into())
            }
            async fn complete(&self, _: &str, _: Option<&VoiceModelRef>) -> Result<String, String> {
                unreachable!("configuration cannot invoke a model")
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let manager = VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(dir.path())),
            dir.path(),
        )
        .with_dialogue(Arc::new(Models));
        manager.write_autostart_config(true).unwrap();
        let selected = VoiceModelRef {
            provider_id: "catalog-provider".into(),
            model: "catalog-model".into(),
        };
        manager
            .update_dialogue(VoiceDialogueConfig {
                dialogue_model: Some(selected.clone()),
            })
            .await
            .unwrap();
        assert!(manager.read_autostart_config());
        manager.write_autostart_config(false).unwrap();
        let saved: VoiceDialogueConfig =
            serde_json::from_slice(&std::fs::read(&manager.config_path).unwrap()).unwrap();
        assert_eq!(saved.dialogue_model, Some(selected));
        let public = serde_json::to_string(&manager.dialogue_settings().await).unwrap();
        assert!(!public.contains("token"));
        assert!(!public.contains("api_key"));
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
        let name=if cfg!(windows){"ai-mate.exe"}else{"ai-mate"};
        let daemon=bin.join(name);std::fs::write(&daemon,b"test-only executable fixture").unwrap();
        let host=root.path().join("MacOS/Flowy");
        assert_eq!(VoiceCompanionProcessManager::installed_ai_mate_binary(Some(&bin),Some(&host)),Some(daemon));
        assert!(VoiceCompanionProcessManager::installed_ai_mate_binary(None,Some(&host)).is_none());
        let neighboring=root.path().join(name);std::fs::write(&neighboring,b"test-only colocated fixture").unwrap();
        assert_eq!(VoiceCompanionProcessManager::installed_ai_mate_binary(None,Some(&root.path().join("Flowy.exe"))),Some(neighboring));
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
        assert!(settings.reason.is_none());
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
            manager.active_window_candidate().read().unwrap().as_deref(),
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
            manager.active_window_candidate().read().unwrap().as_deref(),
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
        let notif_claimed = tasks.notification_for("cmd-replay-test").unwrap().unwrap();
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
        assert_eq!(tasks.notification_for("cmd-replay-test").unwrap().unwrap().status,"failed","offline replay cannot claim acceptance or change playback history");
    }

    #[test]
    fn test_spoken_summary_concise_text_preserved() {
        let text = "北京今天晴，气温 15 到 25 度，北风 2 级。";
        let summary = extract_spoken_summary(text, VoiceTaskStatus::Completed);
        assert_eq!(summary, text);
        assert!(summary.chars().count() <= 80);
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
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed);
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
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Failed);
        assert!(summary.contains("失败"), "summary must retain negation/failure marker");
        assert!(summary.contains("未能") || summary.contains("未"), "summary must retain negation marker");
        assert!(summary.chars().count() <= 80);
    }

    #[test]
    fn test_spoken_summary_handles_empty_fallback() {
        let summary_done = extract_spoken_summary("", VoiceTaskStatus::Completed);
        assert_eq!(summary_done, "任务已完成，详情请在会话中查看。");
        let summary_failed = extract_spoken_summary("   ", VoiceTaskStatus::Failed);
        assert_eq!(summary_failed, "任务未完成，详情请在会话中查看。");
    }

    #[test]
    fn test_spoken_summary_must_not_drop_actual_backup_failure() {
        let raw = "目录扫描已完成，已检查全部文件并统计文件数量和大小以及当前磁盘空间使用情况并生成了详细目录结构和待处理文件清单用于下一步执行，但是备份失败没有复制任何文件且目标目录仍然为空，请勿删除原始文件以免丢失数据。";
        let summary = extract_spoken_summary(raw, VoiceTaskStatus::Completed);
        assert!(summary.contains("备份失败") || summary.contains("没有复制"), "spoken summary lost the actual failure: {summary}");
        assert!(summary.chars().count() <= 80, "summary must be <= 80 chars, got: {}", summary.chars().count());
    }
}
