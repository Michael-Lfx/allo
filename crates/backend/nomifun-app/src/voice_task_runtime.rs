//! Voice tasks use the same Conversation service and runtime registry as desktop tasks.
//! The acoustic process supplies only a goal and delivery identity, never permissions.
use std::{path::Path, sync::Arc};
use async_trait::async_trait;
use nomifun_api_types::{CreateConversationRequest, ResolvedPresetSnapshot, SendMessageRequest, VoiceBoundMessageRequest};
use nomifun_api_types::voice::{VoiceExecutionConfig, VoiceModelRef};
use nomifun_ai_agent::AgentRuntimeRegistry;
use nomifun_conversation::{ConversationService, IdempotentMessageDelivery, PublicTurnDeliveryState};

use crate::voice_tasks::{TaskMutationError, TaskReceipt, VoiceTaskRuntime};
use crate::voice_dialogue::VoiceDialogueCompleter;

pub(crate) struct ConversationVoiceRuntime {
    pub conversation: ConversationService,
    pub registry: Arc<dyn AgentRuntimeRegistry>,
    pub models: Arc<dyn VoiceDialogueCompleter>,
    pub presets: Option<Arc<nomifun_preset::PresetService>>,
}

fn mutation_error(error:nomifun_common::AppError,rejected:&str,uncertain:&str) -> TaskMutationError {
    use nomifun_common::AppError;
    match error {
        AppError::NotFound(_) | AppError::BadRequest(_) | AppError::Unauthorized(_) | AppError::Forbidden(_)
        | AppError::Conflict(_) | AppError::ConversationTurnAdmissionConflict | AppError::ProviderUnavailable(_)
        | AppError::ManagedFreeModelsDisabled(_) | AppError::RateLimited | AppError::UnprocessableEntity(_)
        | AppError::WorkspacePathEdgeWhitespace(_) | AppError::WorkspacePathEdgeWhitespaceRuntimeUnsupported(_) => TaskMutationError::Rejected(rejected.into()),
        _ => TaskMutationError::Uncertain(uncertain.into()),
    }
}

fn receipt(value: IdempotentMessageDelivery) -> TaskReceipt {
    TaskReceipt {
        message_id:value.message_id, runtime_turn_id:value.turn_id, replayed:value.replayed,
        completed:value.completed,result_ok:value.result_ok,result_text:value.result_text,error_code:value.result_error_code,
    }
}

pub(crate) fn validate_workspace(config: &VoiceExecutionConfig) -> Result<(), String> {
    let path=Path::new(&config.workspace);
    if !path.is_absolute() || !path.is_dir() || config.workspace.trim()!=config.workspace { return Err("task_workspace_invalid".into()); }
    if nomifun_common::workspace_path_has_edge_whitespace_segment(path) { return Err("task_workspace_invalid".into()); }
    if config.model.is_none() && config.preset_id.as_deref().is_none_or(|id| id.trim().is_empty()) { return Err("task_execution_model_unconfigured".into()); }
    if config.model.as_ref().is_some_and(|model| model.provider_id.trim().is_empty() || model.model.trim().is_empty()) {return Err("task_execution_model_unconfigured".into());}
    Ok(())
}

#[async_trait]
impl VoiceTaskRuntime for ConversationVoiceRuntime {
    fn progress_events(&self,conversation_id:&str)->Option<tokio::sync::broadcast::Receiver<nomifun_ai_agent::AgentStreamEvent>> {
        self.registry.get_runtime(conversation_id).map(|runtime|runtime.subscribe())
    }
    fn transport_healthy(&self,conversation_id:&str)->Option<bool> {
        self.registry.get_runtime(conversation_id).map(|runtime|runtime.is_transport_healthy())
    }
    async fn confirmations(&self,owner:&str,conversation_id:&str,send_key:&str,message_id:&str)->Result<Vec<nomifun_api_types::voice::VoiceToolConfirmation>,String> {
        self.conversation.list_voice_tool_confirmations(owner,conversation_id,send_key,message_id,&self.registry).await.map_err(|_|"task_approval_changed".into())
    }

    async fn confirm(&self,owner:&str,conversation_id:&str,send_key:&str,message_id:&str,confirmation:&nomifun_api_types::voice::VoiceToolConfirmation,approved:bool)->Result<(),String> {
        self.conversation.confirm_voice_tool_call(owner,conversation_id,send_key,message_id,&confirmation.call_id,nomifun_api_types::ConfirmRequest {
            msg_id:confirmation.msg_id.clone(),always_allow:false,
            data:serde_json::json!({"value":if approved {"proceed_once"}else{"cancel"},"approval_id":confirmation.approval_id}),
        },&self.registry).await.map_err(|_|"task_approval_changed".into())
    }
    async fn resolve_configuration(&self, config:&VoiceExecutionConfig) -> Result<VoiceExecutionConfig,String> {
        self.prepare_configuration(config).await.map(|(config,_)|config)
    }

    async fn prepare_configuration(&self, config:&VoiceExecutionConfig) -> Result<(VoiceExecutionConfig,Option<nomifun_api_types::ResolvedPresetSnapshot>),String> {
        let mut effective = config.clone();
        if effective.model.is_none() && effective.preset_id.as_deref().is_none_or(|id| id.trim().is_empty()) {
            let default_model = self.models.resolve_model(None).await.map_err(|_| "task_execution_model_unconfigured")?;
            effective.model = Some(default_model);
        }
        validate_workspace(&effective)?;
        let mut preset_snapshot=None;
        effective.workspace = nomifun_common::paths::canonicalize_simplified(Path::new(&effective.workspace)).map_err(|_| "task_workspace_invalid")?.to_string_lossy().into_owned();
        if let Some(preset_id) = &effective.preset_id {
            let presets = self.presets.as_ref().ok_or("task_preset_unavailable")?;
            let overrides = nomifun_api_types::PresetOverrides {
                provider_id: effective.model.as_ref().map(|model| model.provider_id.clone()),
                model: effective.model.as_ref().map(|model| model.model.clone()),
                ..Default::default()
            };
            let resolved = presets.resolve(preset_id, nomifun_api_types::PresetTarget::Conversation, None, overrides)
                .await.map_err(|_| "task_preset_unavailable")?;
            if resolved.resolved_agent_type.as_deref().is_some_and(|kind| kind != "nomi") {return Err("task_preset_requires_nomi".into());}
            let model = resolved.resolved_model.clone().ok_or("task_execution_model_unconfigured")?;
            effective.model = Some(nomifun_api_types::voice::VoiceModelRef {
                provider_id: model.provider_id.ok_or("task_execution_model_unconfigured")?, model: model.model,
            });
            preset_snapshot=Some(resolved);
        }
        let model = effective.model.as_ref().ok_or("task_execution_model_unconfigured")?;
        effective.model = Some(self.models.resolve_model(Some(model)).await?);
        Ok((effective,preset_snapshot))
    }

    async fn create(&self, owner:&str, config:&VoiceExecutionConfig, preset:Option<&nomifun_api_types::ResolvedPresetSnapshot>, creation_key:&str) -> Result<String,TaskMutationError> {
        if config.preset_id.is_some()!=preset.is_some() || preset.is_some_and(|snapshot|Some(&snapshot.preset_id)!=config.preset_id.as_ref()) {
            return Err(TaskMutationError::Rejected("task_execution_config_invalid".into()));
        }
        validate_workspace(config).map_err(TaskMutationError::Rejected)?;
        let canonical=nomifun_common::paths::canonicalize_simplified(Path::new(&config.workspace)).map_err(|_| TaskMutationError::Rejected("task_workspace_invalid".into()))?;
        let mode = config.session_mode.as_deref().unwrap_or("default");
        let request:CreateConversationRequest=serde_json::from_value(serde_json::json!({
            "type":"nomi", "name":"语音任务", "model":config.model,
            "delegation_policy":"disabled",
            "extra":{"voice":true,"workspace":canonical.to_string_lossy(),"session_mode":mode}
        })).map_err(|_| TaskMutationError::Rejected("task_execution_config_invalid".into()))?;
        let created=self.conversation.create_voice_idempotent(owner,request,creation_key,&config.workspace,preset.cloned()).await.map_err(|error|mutation_error(error,"task_creation_rejected","task_creation_failed"))?;
        Ok(created.conversation_id)
    }

    async fn create_bound(&self,owner:&str,config:&VoiceExecutionConfig,preset:Option<&nomifun_api_types::ResolvedPresetSnapshot>,creation_key:&str,target:Option<&str>)->Result<String,TaskMutationError>{
        let Some(conversation_id)=target else {return self.create(owner,config,preset,creation_key).await;};
        self.conversation.ensure_voice_task_policy(owner,conversation_id,&config.workspace,config.model.as_ref(),preset,config.session_mode.as_deref()).await
            .map_err(|error|mutation_error(error,"task_creation_rejected","task_creation_failed"))?;
        Ok(conversation_id.to_owned())
    }
    async fn find_created(&self,owner:&str,creation_key:&str) -> Result<Option<String>,String> {
        self.conversation.voice_conversation_by_creation_key(owner,creation_key).await.map_err(|_|"task_creation_lookup_failed".into())
    }

    async fn append(&self, owner:&str, conversation_id:&str, message_id:&str, display_text:&str, command_id:&str) -> Result<(),TaskMutationError> {
        self.conversation
            .append_voice_message(owner, conversation_id, message_id, display_text, command_id)
            .await
            .map_err(|error| mutation_error(error, "task_delivery_rejected", "task_delivery_failed"))
    }

    async fn send_bound(
        &self,
        owner: &str,
        conversation_id: &str,
        send_key: &str,
        message_id: &str,
        command_id: &str,
        display_text: &str,
        goal: &str,
        context_digest: Option<&str>,
        frozen_config: Option<&VoiceExecutionConfig>,
        preset: Option<&ResolvedPresetSnapshot>,
    ) -> Result<TaskReceipt, TaskMutationError> {
        let conversation = self
            .conversation
            .get(owner, conversation_id)
            .await
            .map_err(|error| mutation_error(error, "task_delivery_rejected", "task_conversation_unavailable"))?;

        if let Some(frozen) = frozen_config {
            let actual_workspace = conversation
                .extra
                .get("voice_task_workspace")
                .or_else(|| conversation.extra.get("workspace"))
                .and_then(serde_json::Value::as_str);
            let workspace_matches = match actual_workspace {
                Some(ws) => {
                    let ws_path = Path::new(ws);
                    let frozen_path = Path::new(&frozen.workspace);
                    nomifun_common::paths::paths_equivalent(ws_path, frozen_path)
                }
                None => false,
            };
            if !workspace_matches {
                return Err(TaskMutationError::Rejected("task_execution_config_conflict".into()));
            }

            let actual_model = conversation
                .model
                .as_ref()
                .and_then(|m| serde_json::to_value(m).ok())
                .and_then(|v| serde_json::from_value::<VoiceModelRef>(v).ok());
            if frozen.model.is_some() && frozen.model != actual_model {
                return Err(TaskMutationError::Rejected("task_execution_config_conflict".into()));
            }

            let frozen_preset = frozen.preset_id.as_deref().filter(|s| !s.trim().is_empty());
            let actual_preset = conversation.preset_id.as_deref().filter(|s| !s.trim().is_empty());
            if frozen_preset != actual_preset {
                return Err(TaskMutationError::Rejected("task_execution_config_conflict".into()));
            }

            if preset.is_some() && conversation.preset_snapshot.as_ref() != preset {
                return Err(TaskMutationError::Rejected("task_execution_config_conflict".into()));
            }

            let frozen_mode = frozen.session_mode.as_deref().unwrap_or("default");
            let actual_session_mode = conversation
                .extra
                .get("session_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            if frozen_mode != actual_session_mode {
                return Err(TaskMutationError::Rejected("task_execution_config_conflict".into()));
            }
        }

        let workspace = conversation
            .extra
            .get("voice_task_workspace")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TaskMutationError::Rejected("task_policy_missing".into()))?
            .to_owned();
        let epoch = self
            .conversation
            .conversation_repo()
            .get_turn_admission_state(owner, conversation_id)
            .await
            .map_err(|_| TaskMutationError::Uncertain("task_delivery_failed".into()))?
            .epoch;
        let request = VoiceBoundMessageRequest {
            message_id: message_id.to_owned(),
            display_text: display_text.to_owned(),
            command_id: command_id.to_owned(),
            expected_admission_epoch: epoch,
            expected_workspace: workspace,
            voice_context_digest: context_digest.map(str::to_owned),
            content: goal.to_owned(),
            files: Vec::new(),
            inject_skills: Vec::new(),
            hidden: false,
            origin: Some("companion".into()),
            channel_platform: None,
        };
        match self.conversation.send_voice_bound_message(owner, conversation_id, send_key, request, &self.registry).await {
            Ok(value) => Ok(receipt(value)),
            Err(error) => {
                if !matches!(
                    self.conversation.public_turn_delivery_state(owner, conversation_id, send_key).await,
                    Ok(PublicTurnDeliveryState::Missing)
                ) {
                    return Err(TaskMutationError::Uncertain("task_delivery_failed".into()));
                }
                Err(mutation_error(error, "task_delivery_rejected", "task_delivery_failed"))
            }
        }
    }
    async fn send(&self, owner:&str, conversation_id:&str, send_key:&str, goal:&str) -> Result<TaskReceipt,TaskMutationError> {
        let conversation=self.conversation.get(owner,conversation_id).await.map_err(|error|mutation_error(error,"task_delivery_rejected","task_conversation_unavailable"))?;
        if conversation.extra.get("voice_task_workspace").and_then(serde_json::Value::as_str).is_none() {return Err(TaskMutationError::Rejected("task_policy_missing".into()));}
        let request=SendMessageRequest {content:goal.to_owned(),files:Vec::new(),inject_skills:Vec::new(),hidden:false,origin:Some("companion".into()),channel_platform:None};
        match self.conversation.send_message_with_idempotency_key(owner,conversation_id,send_key,request,&self.registry).await {
            Ok(value)=>Ok(receipt(value)),
            Err(error)=>{
                // A response error can follow durable admission. Only a missing
                // receipt permits classification as a definitive rejection.
                if !matches!(self.conversation.public_turn_delivery_state(owner,conversation_id,send_key).await,Ok(PublicTurnDeliveryState::Missing)) {
                    return Err(TaskMutationError::Uncertain("task_delivery_failed".into()));
                }
                Err(mutation_error(error,"task_delivery_rejected","task_delivery_failed"))
            }
        }
    }

    async fn observe(&self, owner:&str, conversation_id:&str, send_key:&str) -> Result<Option<TaskReceipt>,String> {
        match self.conversation.public_turn_delivery_state(owner,conversation_id,send_key).await.map_err(|_| "task_receipt_unavailable")? {
            PublicTurnDeliveryState::Missing => Ok(None),
            PublicTurnDeliveryState::Accepted {message_id} => Ok(Some(TaskReceipt {message_id,runtime_turn_id:None,replayed:true,completed:false,result_ok:None,result_text:None,error_code:None})),
            PublicTurnDeliveryState::Completed(value) => Ok(Some(receipt(value))),
        }
    }

    async fn cancel(&self, owner:&str, conversation_id:&str, send_key:&str,message_id:&str) -> Result<(),String> {
        self.conversation.cancel_voice_turn(owner,conversation_id,send_key,message_id,&self.registry).await.map_err(|error|match error {
            nomifun_common::AppError::Timeout(_) => "task_cancel_pending".into(),
            nomifun_common::AppError::Conflict(_) => "task_scope_changed".into(),
            _ => "task_cancel_failed".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice_tasks::VoiceTaskRuntime;
    use nomifun_api_types::voice::{VoiceModelRef, VoiceTaskCommand, VoiceTaskStatus, VoiceTaskSnapshot, VoiceTaskDelivery, VoiceTaskSubmitRequest, VoiceSessionSnapshot, VoiceSessionPhase, VoiceConnectionState};
    use std::sync::atomic::{AtomicUsize, Ordering};

    // These fixtures share process-level engine resources; each models the product's one active voice task.
    static RUNTIME_REPLAY_LOCK:tokio::sync::Mutex<()>=tokio::sync::Mutex::const_new(());

    #[derive(Debug,Clone,Copy,PartialEq,Eq)]
    enum ReadScenario {Api,Worker,InstalledAcoustics,CancelRunning,VoiceControls,FrozenPreset,ApprovalAccept,ApprovalReject,ApprovalShell,OralApprovalAccept,OralApprovalReject,OralApprovalSilence,ResultFollowUp,QueuedDrain,KeyboardQueuedDrain}

    #[derive(Default)]
    struct RecordedTts(std::sync::Mutex<Vec<String>>);
    #[async_trait]
    impl voice_companion::replay_terminal::tts::TtsProvider for RecordedTts {
        async fn synthesize_stream(&self,text:&str,_voice:&str) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>,voice_companion::replay_terminal::tts::TtsError> {
            self.0.lock().unwrap().push(text.to_owned());
            let pcm:Vec<f32> = (0..2400).map(|sample|0.1*(sample as f32*0.13).sin()).collect();
            let (tx,rx) = tokio::sync::mpsc::channel(1);
            tx.try_send(nomifun_audio::pcm_to_wav(&pcm,24_000)).unwrap();
            Ok(rx)
        }
    }

    struct CountedStt {
        inner:Arc<dyn nomifun_audio::recorder::SttCallback>,
        calls:AtomicUsize,
    }
    struct ScriptedStt(std::sync::Mutex<std::collections::VecDeque<String>>);
    #[async_trait]
    impl nomifun_audio::recorder::SttCallback for ScriptedStt {
        async fn transcribe(&self,_channel:nomifun_audio::frame::AudioChannel,_pcm:Vec<f32>,_rate:u32) -> Option<String> {
            self.0.lock().unwrap().pop_front()
        }
    }

    async fn feed_control_speech(producer:&nomifun_audio::voice_pipeline::SpscProducer) {
        for index in 0..75 {
            let pcm:Vec<f32>=(0..320).map(|sample|if index<25 {0.03*(sample as f32*std::f32::consts::TAU*200.0/16000.0).sin()} else {0.0}).collect();
            assert_eq!(producer.push(&pcm),pcm.len());
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
    #[async_trait]
    impl nomifun_audio::recorder::SttCallback for CountedStt {
        async fn transcribe(&self,channel:nomifun_audio::frame::AudioChannel,pcm:Vec<f32>,rate:u32) -> Option<String> {
            self.calls.fetch_add(1,Ordering::SeqCst);
            self.inner.transcribe(channel,pcm,rate).await
        }
    }

    #[tokio::test]
    async fn task_b_is_queued_during_running_task_a_and_drained_with_same_message_id_and_single_bubble() {
        real_read_scenario(ReadScenario::QueuedDrain).await;
    }

    #[tokio::test]
    async fn keyboard_b_uses_the_voice_fifo_and_can_cancel_c_without_cancelling_a() {
        real_read_scenario(ReadScenario::KeyboardQueuedDrain).await;
    }

    #[tokio::test]
    async fn a_voice_task_uses_the_real_conversation_runtime_to_read_a_fresh_workspace_file() {
        real_read_scenario(ReadScenario::Api).await;
    }

    #[tokio::test]
    async fn a_captured_voice_task_reaches_the_host_real_read_and_spoken_result() {
        real_read_scenario(ReadScenario::Worker).await;
    }

    #[tokio::test]
    async fn a_follow_up_can_explain_the_actual_task_result_without_executing_again() {
        real_read_scenario(ReadScenario::ResultFollowUp).await;
    }

    #[tokio::test]
    #[ignore = "explicit installed KWS/VAD/ASR replay; controlled dialogue and WAV synthesis; real runtime Read; no microphone or speaker"]
    async fn installed_acoustics_reach_a_real_read_and_return_the_result() {
        real_read_scenario(ReadScenario::InstalledAcoustics).await;
    }

    #[tokio::test]
    async fn cancelling_an_actual_running_voice_task_prevents_its_read() {
        real_read_scenario(ReadScenario::CancelRunning).await;
    }

    #[tokio::test]
    async fn stop_voice_keeps_the_real_task_alive_and_chat_query_cancel_share_the_voice_worker() {
        real_read_scenario(ReadScenario::VoiceControls).await;
    }

    #[tokio::test]
    async fn a_prepared_voice_task_keeps_its_preset_revision_when_the_catalog_changes() {
        real_read_scenario(ReadScenario::FrozenPreset).await;
    }

    #[tokio::test]
    async fn actual_voice_delete_waits_for_its_bound_one_time_confirmation() {
        real_read_scenario(ReadScenario::ApprovalAccept).await;
    }

    #[tokio::test]
    async fn refusing_an_actual_voice_delete_preserves_the_local_file() {
        real_read_scenario(ReadScenario::ApprovalReject).await;
    }

    #[tokio::test]
    async fn actual_tool_approval_travels_through_the_voice_prompt_and_capture_worker() {
        for scenario in [ReadScenario::OralApprovalAccept,ReadScenario::OralApprovalReject,ReadScenario::OralApprovalSilence] {
            real_read_scenario(scenario).await;
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn an_actual_destructive_command_cannot_run_before_its_voice_approval() {
        real_read_scenario(ReadScenario::ApprovalShell).await;
    }

    async fn real_read_scenario(scenario:ReadScenario) {
        let _fixture=RUNTIME_REPLAY_LOCK.lock().await;
        let oral_approval=matches!(scenario,ReadScenario::OralApprovalAccept|ReadScenario::OralApprovalReject|ReadScenario::OralApprovalSilence);
        let result_followup=scenario==ReadScenario::ResultFollowUp;
        let keyboard_queue=scenario==ReadScenario::KeyboardQueuedDrain;
        let queued_drain=matches!(scenario,ReadScenario::QueuedDrain|ReadScenario::KeyboardQueuedDrain);
        let from_worker=oral_approval || result_followup || keyboard_queue || matches!(scenario,ReadScenario::Worker|ReadScenario::InstalledAcoustics|ReadScenario::VoiceControls);
        let real_acoustics=scenario==ReadScenario::InstalledAcoustics;
        let voice_controls=scenario==ReadScenario::VoiceControls;
        let cancel_running=matches!(scenario,ReadScenario::CancelRunning|ReadScenario::VoiceControls);
        let approval=oral_approval || matches!(scenario,ReadScenario::ApprovalAccept|ReadScenario::ApprovalReject|ReadScenario::ApprovalShell);
        let approving=matches!(scenario,ReadScenario::ApprovalAccept|ReadScenario::OralApprovalAccept|ReadScenario::ApprovalShell);
        let approval_shell=scenario==ReadScenario::ApprovalShell;
        use axum::{Json, Router, routing::post, response::{Sse, sse::Event}};
        use tower::ServiceExt;
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("work");
        std::fs::create_dir_all(&workspace).unwrap();
        let marker = format!("voice-owned-{}", nomifun_common::generate_id());
        let file = workspace.join("acceptance.txt");
        std::fs::write(&file, &marker).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed_read = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let read_proof = observed_read.clone();
        let expected = marker.clone();
        let input_path = file.clone();
        let goal = if approval {format!("删除 {}，执行前请我确认。",file.display())}else{format!("读取 {} 并返回完整内容。",file.display())};
        let voice_goal = goal.clone();
        let dialogue_calls = Arc::new(AtomicUsize::new(0));
        let dialogue_counter = dialogue_calls.clone();
        let model_entered=Arc::new(tokio::sync::Notify::new());
        let release_model=Arc::new(tokio::sync::Notify::new());
        let entered=model_entered.clone();let released=release_model.clone();
        // Only the external model is controlled. Conversation, registry, engine,
        // tool dispatch, workspace policy, actual Read and durable receipts are real.
        let upstream = Router::new().route("/v1/chat/completions", post(move |Json(body): Json<serde_json::Value>| {
            let counted = counted.clone(); let read_proof = read_proof.clone();
            let expected = expected.clone(); let input_path = input_path.clone();
            let voice_goal = voice_goal.clone(); let dialogue_counter = dialogue_counter.clone();
            let entered=entered.clone();let released=released.clone();
            async move {
                let is_runtime = body["tools"].as_array().is_some_and(|tools| !tools.is_empty());
                let choice = if is_runtime {
                    let index=counted.fetch_add(1, Ordering::SeqCst);
                    if (cancel_running || queued_drain) && index==0 {entered.notify_one();released.notified().await;}
                    let tool_result = body["messages"].as_array().and_then(|msgs| {
                        let last_assistant_idx = msgs.iter().rposition(|m| m["role"] == "assistant")?;
                        if msgs[last_assistant_idx].get("tool_calls").is_some() {
                            msgs[last_assistant_idx..].iter().rev().find(|m| m["role"] == "tool")
                        } else {
                            None
                        }
                    });
                    if approval && approving && !approval_shell && index==3 {
                        assert!(body["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap_or_default().contains("Before finishing, run a verification command"),"the extra model call is the existing office evidence nudge");
                        serde_json::json!({"delta":{"role":"assistant","content":"删除已完成且验证通过。"},"finish_reason":"stop"})
                    } else if approval && index==1 {
                        let actual=tool_result.expect("mutation must follow an actual Read")["content"].as_str().unwrap();
                        assert!(actual.contains(&expected));read_proof.fetch_add(1,Ordering::SeqCst);
                        let (name,arguments)=if approval_shell {("Bash",serde_json::json!({"command":"Remove-Item -LiteralPath 'acceptance.txt'"}))}
                            else {("ApplyPatch",serde_json::json!({"patch":"*** Begin Patch\n*** Delete File: acceptance.txt\n*** End Patch"}))};
                        assert!(body["tools"].as_array().unwrap().iter().any(|tool|tool["function"]["name"]==name));
                        serde_json::json!({"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"mutate-owned","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]},"finish_reason":"tool_calls"})
                    } else if let Some(result) = tool_result {
                        let actual = result["content"].as_str().expect("OpenAI tool result must be text");
                        if approval {eprintln!("actual voice approval tool result: {actual}");}
                        if !approval && !queued_drain {assert!(actual.contains(&expected), "the real Read must return the fresh file marker: {actual}");}
                        if !approval {read_proof.fetch_add(1, Ordering::SeqCst);}
                        serde_json::json!({"delta":{"role":"assistant","content":format!("实际工具结果：{actual}")},"finish_reason":"stop"})
                    } else {
                        let target_file = body["messages"].as_array().and_then(|msgs| {
                            msgs.iter().rev().find_map(|m| {
                                let content = if let Some(s) = m["content"].as_str() {
                                    s.to_string()
                                } else if let Some(arr) = m["content"].as_array() {
                                    arr.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect()
                                } else {
                                    String::new()
                                };
                                if let Some(s) = content.find("读取 ") {
                                    if let Some(e) = content[s..].find(" 并返回") {
                                        let raw = &content[s + "读取 ".len()..s + e];
                                        return Some(raw.trim().to_string());
                                    }
                                }
                                None
                            })
                        }).unwrap_or_else(|| input_path.to_string_lossy().to_string());
                        assert!(body["tools"].as_array().unwrap().iter().any(|tool| tool["function"]["name"] == "Read"));
                        serde_json::json!({"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"read-owned","type":"function","function":{"name":"Read","arguments":serde_json::json!({"file_path":target_file}).to_string()}}]},"finish_reason":"tool_calls"})
                    }
                } else if body["messages"].as_array().unwrap().iter().any(|message|message["content"].as_str().is_some_and(|text|text.contains("语音决策 JSON 契约"))) {
                    let index=dialogue_counter.fetch_add(1,Ordering::SeqCst);
                    let decision=if result_followup && index==1 {
                        assert!(body.to_string().contains(&expected),"the follow-up model must receive the actual runtime result, not just an accepted task label");
                        serde_json::json!({"intent_type":"chat","understood_request":"解释刚才的实际读取结果。","reply_tts":format!("刚才文件内容是 {expected}")})
                    } else if voice_controls && index==1 {
                        serde_json::json!({"intent_type":"chat","understood_request":"用户在任务运行期间打招呼。","reply_tts":"你好，语音聊天可以继续。"})
                    } else if voice_controls && index>=2 {
                        panic!("query/cancel must remain local when the dialogue model is offline");
                    } else {serde_json::json!({
                        "intent_type":"agent_task","task_goal":voice_goal,"understood_request":"读取指定工作区文件并返回内容。",
                        "spoken_ack":"模型不能代替真实接单","requires_confirmation":false
                    })};
                    serde_json::json!({"delta":{"role":"assistant","content":decision.to_string()},"finish_reason":"stop"})
                } else {
                    // The product's existing title completer is also isolated.
                    serde_json::json!({"delta":{"role":"assistant","content":"语音读取验收"},"finish_reason":"stop"})
                };
                let chunk = serde_json::json!({"id":"voice-fixture","object":"chat.completion.chunk","created":1,"model":"voice-runtime-fixture","choices":[{"index":0,"delta":choice["delta"],"finish_reason":choice["finish_reason"]}]});
                Sse::new(futures_util::stream::iter(vec![
                    Ok::<_, std::convert::Infallible>(Event::default().data(chunk.to_string())),
                    Ok(Event::default().data("[DONE]")),
                ]))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
        let database = nomifun_db::init_database_memory().await.unwrap();
        let database_pool = database.pool().clone();
        let services = crate::AppServices::from_config(database, &crate::AppConfig {
            data_dir: root.path().join("data"), work_dir: workspace.clone(), ..Default::default()
        }).await.unwrap();
        let provider_id = nomifun_common::generate_id();
        let encrypted = nomifun_common::encrypt_string("test-only", &services.encryption_key).unwrap();
        services.provider_repo.create(nomifun_db::CreateProviderParams {
            provider_id: Some(&provider_id), platform: "openai", name: "voice fixture",
            base_url: &endpoint, api_key_encrypted: &encrypted,
            models: r#"["voice-runtime-fixture"]"#, enabled: true,
            model_context_limits: None, model_protocols: None, model_descriptions: None,
            model_enabled: None, bedrock_config: None, is_full_url: false, sort_order: None,
        }).await.unwrap();
        services.provider_model_repo.update(&provider_id, "voice-runtime-fixture", &nomifun_db::ProviderModelUpdate {
            tasks: Some(r#"["chat"]"#), ..Default::default()
        }).await.unwrap();
        let state = crate::router::build_conversation_state(&services, None,
            Arc::new(nomifun_file::SnapshotService::with_snapshot_root(root.path().join("snapshots"))));
        let models = Arc::new(crate::voice_dialogue::ProviderVoiceDialogue {
            provider_repo: services.provider_repo.clone(), provider_model_repo: services.provider_model_repo.clone(),
            encryption_key: services.encryption_key, workspace: workspace.clone(),
        });
        if scenario==ReadScenario::FrozenPreset {
            let registry=nomifun_extension::ExtensionRegistry::new(nomifun_extension::ExtensionStateStore::new(root.path().join("extensions.json")),services.event_bus.clone(),services.app_version.clone());
            let presets=crate::build_preset_state(&services,registry).service;
            state.service.with_preset_service(presets.clone());
            let preset=presets.create(serde_json::from_value(serde_json::json!({
                "name":"voice frozen preset", "instructions":"Use the original instruction.","targets":["conversation"]
            })).unwrap()).await.unwrap();
            let runtime=ConversationVoiceRuntime {conversation:state.service,registry:services.agent_runtime_registry.clone(),models,presets:Some(presets.clone())};
            let config=VoiceExecutionConfig {workspace:workspace.to_string_lossy().into(),model:Some(VoiceModelRef {provider_id,model:"voice-runtime-fixture".into()}),preset_id:Some(preset.preset_id.clone()),session_mode:None};
            let (effective,snapshot)=runtime.prepare_configuration(&config).await.unwrap();
            presets.update(&preset.preset_id,nomifun_api_types::UpdatePresetRequest {instructions:Some("A later catalog edit must not change this command.".into()),..Default::default()}).await.unwrap();
            let id=runtime.create(&services.authoritative_user_id,&effective,snapshot.as_ref(),"voice-create:frozen").await.unwrap();
            let conversation=runtime.conversation.get(&services.authoritative_user_id,&id).await.unwrap();
            let frozen=conversation.preset_snapshot.unwrap();
            assert_eq!(frozen.instructions,"Use the original instruction.");
            assert_eq!(frozen.preset_revision,preset.revision);
            assert_eq!(calls.load(Ordering::SeqCst),0);
            server.abort();return;
        }
        let runtime: Arc<dyn VoiceTaskRuntime> = Arc::new(ConversationVoiceRuntime {
            conversation: state.service.clone(), registry: services.agent_runtime_registry.clone(), models:models.clone(), presets:None,
        });
        let command = VoiceTaskCommand {command_id:"read-command".into(),session_id:"acoustic-session".into(),turn_id:"acoustic-turn".into(),goal:goal.clone(),record_id:None,binding_id:None};
        let config = VoiceExecutionConfig {workspace:workspace.to_string_lossy().into(),model:Some(VoiceModelRef {provider_id,model:"voice-runtime-fixture".into()}),preset_id:None,session_mode:None};
        let config_dir = services.data_dir.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("voice_companion.json"), serde_json::to_vec(&serde_json::json!({"execution":config,"autostart":true})).unwrap()).unwrap();
        let daemon_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let daemon_origin = format!("http://{}",daemon_listener.local_addr().unwrap());
        let builder=crate::voice_companion_manager::VoiceCompanionProcessManager::new(
            Arc::new(nomifun_voice_models::ModelManager::new(root.path().join("models"))),&services.data_dir,
        ).with_tasks(services.authoritative_user_id.to_string(),runtime.clone()).with_dialogue(models.clone()).with_replay_daemon(daemon_origin);
        let recorder=Arc::new(crate::voice_companion_manager::ConversationVoiceDialogueRecorder::new(
            Arc::new(state.service.clone()),services.authoritative_user_id.to_string(),&services.data_dir,
        ).with_target_config(builder.target_selection()).with_active_window_candidate(builder.active_window_candidate()).with_execution_runtime(runtime));
        let manager=Arc::new(if from_worker || keyboard_queue {builder.with_recorder(recorder.clone())}else{builder});
        let mut acoustic = None;
        let mut acoustic_worker = None;
        let mut daemon_server = None;
        let mut bridge = None;
        let mut acoustic_audio = None;
        let mut acoustic_tts = None;
        let mut acoustic_stt = None;
        if from_worker {
            use voice_companion::{voice::VoiceCompanionRuntime,oobe::OobeManager};
            use nomifun_audio::{voice_pipeline::{VoiceCapturePipeline,PipelineConfig},stt::FakeSttCallback};
            use voice_companion::replay_terminal::{completion::HostDialogueClient,router::OpenAiCompatibleVoiceRouter,task_client::HostTaskClient,playback::{TestAudioSink,PlaybackPipeline}};
            let hosted = crate::voice_dialogue::VoiceDialogueBridge::start_with_recorder(models,
                Arc::new(std::sync::RwLock::new(config.model.clone())),manager.replay_control_credential(),manager.private_task_routes(),Some(recorder)).await.unwrap();
            let router = Arc::new(OpenAiCompatibleVoiceRouter::with_completion(Arc::new(HostDialogueClient::new(hosted.endpoint.clone(),manager.replay_control_credential()).unwrap())));
            let task_client = Arc::new(HostTaskClient::new(hosted.endpoint.replace("/complete","/tasks"),manager.replay_control_credential()).unwrap());
            let (capture,producer) = VoiceCapturePipeline::new(PipelineConfig::default());
            let model_dir = if real_acoustics {
                std::path::PathBuf::from(std::env::var_os("VOICE_REPLAY_MODEL_DIR").expect("set installed acoustic models"))
            } else {root.path().join("models")};
            let models = Arc::new(nomifun_voice_models::ModelManager::new(model_dir.clone()));
            let oobe = Arc::new(OobeManager::new_bypassed());
            let sink = Arc::new(TestAudioSink::new());
            let transcriber:Arc<dyn nomifun_audio::recorder::SttCallback> = if real_acoustics {
                Arc::new(nomifun_audio::stt::try_load_sherpa(Some(&model_dir)).unwrap().expect("installed real ASR is required"))
            } else if voice_controls {
                Arc::new(ScriptedStt(std::sync::Mutex::new([goal,"你好".into(),"查询任务进度".into(),"取消任务".into()].into_iter().collect())))
            } else if oral_approval {
                Arc::new(ScriptedStt(std::sync::Mutex::new([goal,if approving {"确认".into()}else{"取消".into()}].into_iter().collect())))
            } else if result_followup {
                Arc::new(ScriptedStt(std::sync::Mutex::new([goal,"请解释刚才的结果".into()].into_iter().collect())))
            } else {Arc::new(FakeSttCallback::always(goal))};
            let transcriber = Arc::new(CountedStt {inner:transcriber,calls:AtomicUsize::new(0)});
            let (voice,_unused_audio) = VoiceCompanionRuntime::from_capture(models.clone(),oobe.clone(),Arc::new(tokio::sync::Mutex::new(capture)),transcriber.clone(),router);
            let tts = Arc::new(RecordedTts::default());
            let voice = Arc::new(voice.with_tasks(task_client).with_test_playback(Arc::new(PlaybackPipeline::new(sink.clone())),tts.clone()));
            voice.update_feedback(serde_json::from_value(serde_json::json!({"mode":"off","volume":0,"speak_replies":true})).unwrap()).unwrap();
            let daemon_config = voice_companion::config::Config {control_token:Some(manager.replay_control_credential()),..Default::default()};
            let daemon_routes = voice_companion::server::build_router_full(&daemon_config,models,(*oobe).clone(),Some(voice.event_sender()),Some(voice.clone()));
            daemon_server = Some(tokio::spawn(async move {axum::serve(daemon_listener,daemon_routes).await.unwrap();}));
            if !real_acoustics {voice.handle_wake().await.unwrap();}
            acoustic_worker = Some(voice.start_worker());
            acoustic = Some((voice,producer));
            acoustic_audio = Some(sink);
            acoustic_tts = Some(tts);
            acoustic_stt = Some(transcriber);
            bridge = Some(hosted);
        }
        let source = VoiceSessionSnapshot {
            instance_id:"replayed-daemon".into(), seq:1, enabled:true, connection:VoiceConnectionState::Connected,
            phase:VoiceSessionPhase::Deciding, session_id:Some(command.session_id.clone()), turn_id:Some(command.turn_id.clone()),
            acoustic_ready:true, ..Default::default()
        };
        // Acoustic input is the controlled boundary; the API and execution below
        // use the production handlers and real runtime, without opening a device.
        if !from_worker {manager.replay_acoustic_source(source.clone());}
        let app = crate::voice_companion_manager::voice_companion_routes(manager.clone());
        let settings = app.clone().oneshot(axum::http::Request::builder().uri("/api/voice/companion/execution").body(axum::body::Body::empty()).unwrap()).await.unwrap();
        assert_eq!(settings.status(), axum::http::StatusCode::OK);
        let settings: serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(settings.into_body(), 16_384).await.unwrap()).unwrap();
        assert_eq!(settings["available"], true);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "configuration reads cannot initialize task inference");
        let saved_config = std::fs::read(config_dir.join("voice_companion.json")).unwrap();
        std::fs::write(config_dir.join("voice_companion.json"),b"invalid-json").unwrap();
        let invalid = app.clone().oneshot(axum::http::Request::builder().uri("/api/voice/companion/execution").body(axum::body::Body::empty()).unwrap()).await.unwrap();
        let invalid:serde_json::Value = serde_json::from_slice(&axum::body::to_bytes(invalid.into_body(),16_384).await.unwrap()).unwrap();
        assert_eq!(invalid["available"],false);
        assert_eq!(manager.snapshot().task_ready,false,"unreadable configuration must revoke cached readiness");
        std::fs::write(config_dir.join("voice_companion.json"),saved_config).unwrap();
        let unavailable_preset = serde_json::json!({"workspace":workspace,"model":null,"preset_id":nomifun_common::generate_id()});
        let rejected = app.clone().oneshot(axum::http::Request::builder().method("POST").uri("/api/voice/companion/execution")
            .header("content-type","application/json").body(axum::body::Body::from(unavailable_preset.to_string())).unwrap()).await.unwrap();
        assert_eq!(rejected.status(), axum::http::StatusCode::BAD_REQUEST, "an unresolved preset must not be reported as a ready runtime configuration");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let command = if let Some((voice,producer)) = &acoustic {
            let input = if real_acoustics {
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../docs/architecture/voice-agent/execution/audio-fixtures/wake-and-command-16k.wav");
                let (samples,rate) = nomifun_audio::parse_audio_frame_to_pcm(&std::fs::read(path).unwrap());
                assert_eq!(rate,16_000);
                let mut pcm = vec![0.0;8_000]; pcm.extend(samples); pcm.extend(vec![0.0;24_000]); pcm
            } else {
                (0..75*320).map(|index| if index<25*320 {0.03*((index%320) as f32*std::f32::consts::TAU*200.0/16000.0).sin()} else {0.0}).collect()
            };
            for pcm in input.chunks(320) {
                assert_eq!(producer.push(pcm),pcm.len());
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    if let Some(task) = manager.snapshot().task {
                        break VoiceTaskCommand {command_id:task.command_id,session_id:task.session_id,turn_id:task.acoustic_turn_id,goal:command.goal.clone(),record_id:None,binding_id:None};
                    }
                    assert!(voice.snapshot().error.is_none(),"{:?}",voice.snapshot().error);
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }).await.expect("production worker must submit through its authenticated host transport")
        } else {command};
        let command_id = command.command_id.clone();
        let mut drained_b_record: Option<VoiceTaskSnapshot> = None;
        let submission = VoiceTaskSubmitRequest {instance_id:manager.snapshot().instance_id, command};
        let submit_request = || axum::http::Request::builder().method("POST").uri("/api/voice/companion/tasks")
            .header("content-type", "application/json").body(axum::body::Body::from(serde_json::to_vec(&submission).unwrap())).unwrap();
        if !from_worker || keyboard_queue {
            let accepted: VoiceTaskDelivery = if keyboard_queue {
                VoiceTaskDelivery {task:manager.snapshot().task.expect("A was accepted through the production voice worker"),freshly_accepted:false}
            } else {
                let response = app.clone().oneshot(submit_request()).await.unwrap();
                assert_eq!(response.status(), axum::http::StatusCode::OK);
                serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 32_768).await.unwrap()).unwrap()
            };
            assert!(accepted.task.conversation_id.is_some());
            if approval {
                let uri=format!("/api/voice/companion/tasks/{command_id}/confirmations");
                let pending=tokio::time::timeout(std::time::Duration::from_secs(5),async {
                    loop {
                        let response=app.clone().oneshot(axum::http::Request::builder().uri(&uri).body(axum::body::Body::empty()).unwrap()).await.unwrap();
                        assert_eq!(response.status(),axum::http::StatusCode::OK,"pending approvals must come from the actual runtime");
                        let pending:Vec<nomifun_api_types::voice::VoiceToolConfirmation>=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),65_536).await.unwrap()).unwrap();
                        if let Some(pending)=pending.into_iter().next(){break pending;}
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }).await.expect("the actual deletion must suspend before execution");
                assert!(file.exists());assert_eq!(pending.tool_name,if approval_shell {"Bash"}else{"ApplyPatch"});
                let body=serde_json::json!({"instance_id":manager.snapshot().instance_id,"command_id":command_id,
                    "conversation_id":accepted.task.conversation_id,"message_id":accepted.task.message_id,
                    "msg_id":pending.msg_id,"call_id":pending.call_id,"approved":approving});
                let post=|approval_id:&str,body:&serde_json::Value|axum::http::Request::builder().method("POST").uri(format!("{uri}/{approval_id}"))
                    .header("content-type","application/json").body(axum::body::Body::from(body.to_string())).unwrap();
                let wrong=app.clone().oneshot(post("old-approval",&body)).await.unwrap();assert_eq!(wrong.status(),axum::http::StatusCode::CONFLICT);assert!(file.exists());
                for (field,value) in [("instance_id","old-host"),("conversation_id","another-task"),("msg_id","old-message"),("call_id","old-call")] {
                    let mut stale=body.clone();stale[field]=serde_json::Value::String(value.into());
                    let response=app.clone().oneshot(post(&pending.approval_id,&stale)).await.unwrap();assert_eq!(response.status(),axum::http::StatusCode::CONFLICT,"stale {field} must not execute the real tool");assert!(file.exists());
                }
                let mut permanent=body.clone();permanent["always_allow"]=serde_json::json!(true);
                let response=app.clone().oneshot(post(&pending.approval_id,&permanent)).await.unwrap();assert_eq!(response.status(),axum::http::StatusCode::UNPROCESSABLE_ENTITY);assert!(file.exists());
                let response=app.clone().oneshot(post(&pending.approval_id,&body)).await.unwrap();assert_eq!(response.status(),axum::http::StatusCode::OK);
                let duplicate=app.clone().oneshot(post(&pending.approval_id,&body)).await.unwrap();assert_eq!(duplicate.status(),axum::http::StatusCode::CONFLICT);
            }
            if cancel_running {
                tokio::time::timeout(std::time::Duration::from_secs(5),model_entered.notified()).await.expect("the real runtime must be waiting in its model call");
                let scoped=serde_json::json!({"instance_id":manager.snapshot().instance_id,"command_id":command_id,
                    "conversation_id":accepted.task.conversation_id,"message_id":accepted.task.message_id});
                let request=||axum::http::Request::builder().method("POST").uri(format!("/api/voice/companion/tasks/{command_id}/cancel"))
                    .header("content-type","application/json").body(axum::body::Body::from(scoped.to_string())).unwrap();
                let response=tokio::time::timeout(std::time::Duration::from_secs(10),app.clone().oneshot(request())).await;
                release_model.notify_waiters();
                let response=response.expect("the public cancel must remain bounded while the model request is pending").unwrap();
                let status=response.status();
                let body=axum::body::to_bytes(response.into_body(),32_768).await.unwrap();
                assert!(status.is_success(),"cancel status: {status}, body: {}",String::from_utf8_lossy(&body));
                let task:VoiceTaskSnapshot=serde_json::from_slice(&body).unwrap();
                assert!(matches!(task.status,VoiceTaskStatus::Cancelling|VoiceTaskStatus::Failed),"a cancel acknowledgement is not a successful task outcome");
            }
            if queued_drain {
                tokio::time::timeout(std::time::Duration::from_secs(5), model_entered.notified()).await.expect("task A must be waiting in its model call");
                let marker_b = format!("voice-queued-b-{}", nomifun_common::generate_id());
                let file_b = workspace.join("acceptance_b.txt");
                std::fs::write(&file_b, &marker_b).unwrap();
                let goal_b = format!("读取 {} 并返回完整内容。", file_b.display());
                let command_b = VoiceTaskCommand {
                    command_id: "read-command-b".into(),
                    session_id: "acoustic-session".into(),
                    turn_id: "acoustic-turn-b".into(),
                    goal: goal_b.clone(),
                    record_id: None,
                    binding_id: None,
                };
                let submit_b = VoiceTaskSubmitRequest {
                    instance_id: manager.snapshot().instance_id,
                    command: command_b.clone(),
                };
                if !keyboard_queue {manager.replay_acoustic_source(VoiceSessionSnapshot {
                    instance_id: manager.snapshot().instance_id,
                    seq: 3,
                    enabled: true,
                    connection: VoiceConnectionState::Connected,
                    phase: VoiceSessionPhase::Deciding,
                    session_id: Some("acoustic-session".into()),
                    turn_id: Some("acoustic-turn-b".into()),
                    acoustic_ready: true,
                    ..Default::default()
                });}
                let (submit_path,submit_body)=if keyboard_queue {
                    ("/api/voice/companion/messages",serde_json::json!({"conversation_id":accepted.task.conversation_id,"command_id":"read-command-b","text":goal_b}))
                } else {("/api/voice/companion/tasks",serde_json::to_value(&submit_b).unwrap())};
                let queued_resp = app.clone().oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri(submit_path)
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(serde_json::to_vec(&submit_body).unwrap()))
                        .unwrap(),
                ).await.unwrap();
                assert_eq!(queued_resp.status(), axum::http::StatusCode::OK);
                let delivery_b: VoiceTaskDelivery = serde_json::from_slice(&axum::body::to_bytes(queued_resp.into_body(), 32_768).await.unwrap()).unwrap();
                assert_eq!(delivery_b.task.status, VoiceTaskStatus::Queued, "task B must be queued while task A is running");
                assert_eq!(delivery_b.task.command_id, "read-command-b");
                let b_msg_id = delivery_b.task.message_id.clone().expect("queued task B must retain stable message_id");
                assert_eq!(delivery_b.task.conversation_id, accepted.task.conversation_id, "task B must share the workspace voice conversation");

                if keyboard_queue {
                    let replay=app.clone().oneshot(axum::http::Request::builder().method("POST").uri(submit_path).header("content-type","application/json")
                        .body(axum::body::Body::from(submit_body.to_string())).unwrap()).await.unwrap();
                    assert_eq!(replay.status(),axum::http::StatusCode::OK);
                    let replay:VoiceTaskDelivery=serde_json::from_slice(&axum::body::to_bytes(replay.into_body(),32768).await.unwrap()).unwrap();
                    assert_eq!(replay.task.message_id,delivery_b.task.message_id);
                    let response=app.clone().oneshot(axum::http::Request::builder().method("POST").uri(submit_path).header("content-type","application/json")
                        .body(axum::body::Body::from(serde_json::json!({"conversation_id":accepted.task.conversation_id,"command_id":"cancel-keyboard-c","text":"Do not execute this queued request"}).to_string())).unwrap()).await.unwrap();
                    assert_eq!(response.status(),axum::http::StatusCode::OK);
                    let c:VoiceTaskDelivery=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),32768).await.unwrap()).unwrap();
                    assert_eq!(c.task.status,VoiceTaskStatus::Queued);
                    let projection=manager.snapshot();
                    assert_eq!(projection.queue.len(),3);
                    assert_eq!(projection.queue[1].position,1);
                    assert_eq!(projection.queue[2].position,2);
                    let cancelled=app.clone().oneshot(axum::http::Request::builder().method("POST").uri("/api/voice/companion/tasks/cancel-keyboard-c/cancel").header("content-type","application/json")
                        .body(axum::body::Body::from(serde_json::json!({"instance_id":projection.instance_id,"command_id":c.task.command_id,"conversation_id":c.task.conversation_id,"message_id":c.task.message_id}).to_string())).unwrap()).await.unwrap();
                    assert_eq!(cancelled.status(),axum::http::StatusCode::OK);
                    assert_eq!(manager.snapshot().queue.len(),2);
                    assert_eq!(manager.snapshot().task.unwrap().command_id,accepted.task.command_id);
                }

                // Verify user messages in SQLite: B has exactly 1 user bubble with b_msg_id
                let conv_id = accepted.task.conversation_id.as_deref().unwrap();
                let b_messages: Vec<(String, String)> = sqlx::query_as("SELECT message_id, content FROM messages WHERE conversation_id = ? AND position = 'right'")
                    .bind(conv_id)
                    .fetch_all(&database_pool)
                    .await
                    .unwrap();
                let b_matching: Vec<_> = b_messages.iter().filter(|(mid, _)| mid == &b_msg_id).collect();
                assert_eq!(b_matching.len(), 1, "task B must have exactly one user message bubble in SQLite");

                // Release task A
                release_model.notify_waiters();

                // Wait for task B to be automatically drained after task A reaches terminal state
                let drained_b = tokio::time::timeout(std::time::Duration::from_secs(15), async {
                    loop {
                        let response = app.clone().oneshot(
                            axum::http::Request::builder()
                                .uri("/api/voice/companion/tasks/read-command-b")
                                .body(axum::body::Body::empty())
                                .unwrap(),
                        ).await.unwrap();
                        assert_eq!(response.status(), axum::http::StatusCode::OK);
                        let task: VoiceTaskSnapshot = serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 32_768).await.unwrap()).unwrap();
                        if matches!(task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) {
                            break task;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                }).await.expect("queued task B must drain automatically and reach completion");
                assert_eq!(drained_b.status, VoiceTaskStatus::Completed, "{drained_b:?}");
                assert!(drained_b.result_text.as_deref().unwrap().contains(&marker_b), "task B must return fresh content from acceptance_b.txt: {:?}", drained_b.result_text);
                assert_eq!(drained_b.message_id, Some(b_msg_id), "drained task B must retain its original message_id from when it was queued");
                drained_b_record = Some(drained_b);
            }
        }
        if voice_controls {
            let (voice,producer)=acoustic.as_ref().unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5),model_entered.notified()).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5),async {
                while voice.snapshot().phase!=VoiceSessionPhase::Listening {tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
            }).await.unwrap();
            let snapshot=manager.snapshot();
            manager.stop_session(nomifun_api_types::voice::VoiceSessionControlRequest {instance_id:snapshot.instance_id,session_id:snapshot.session_id,turn_id:snapshot.turn_id}).await.unwrap();
            assert_eq!(manager.snapshot().task.as_ref().unwrap().status,VoiceTaskStatus::Accepted,"stopping voice cannot cancel accepted execution");
            assert_eq!(calls.load(Ordering::SeqCst),1);
            assert_eq!(observed_read.load(Ordering::SeqCst),0);
            voice.handle_wake().await.unwrap();
            feed_control_speech(producer).await;
            tokio::time::timeout(std::time::Duration::from_secs(5),async {
                while voice.snapshot().phase!=VoiceSessionPhase::FollowUp {tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
            }).await.expect("chat must complete while the task's model call remains blocked");
            assert_eq!(voice.snapshot().assistant_text.as_deref(),Some("你好，语音聊天可以继续。"));
            feed_control_speech(producer).await;
            tokio::time::timeout(std::time::Duration::from_secs(5),async {
                while !voice.snapshot().assistant_text.as_deref().is_some_and(|text|text.contains("任务已接受")) {tokio::time::sleep(std::time::Duration::from_millis(10)).await;}
            }).await.expect("spoken query must use the real accepted receipt");
            feed_control_speech(producer).await;
            let controlled=tokio::time::timeout(std::time::Duration::from_secs(10),async {
                loop {
                    let snapshot=voice.snapshot();
                    if snapshot.error.is_some() {panic!("voice control failed: {:?}",snapshot.error);}
                    if matches!(manager.snapshot().task.as_ref().map(|task|task.status),Some(VoiceTaskStatus::Cancelling|VoiceTaskStatus::Failed))
                        && snapshot.phase==VoiceSessionPhase::FollowUp
                        && snapshot.assistant_text.as_deref().is_some_and(|text|text.contains("正在取消任务")||text.contains("任务已结束"))
                    {break;}
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }).await;
            release_model.notify_waiters();
            controlled.expect("spoken cancellation must reach the actual task");
        }
        if oral_approval {
            let (voice,producer)=acoustic.as_ref().unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(10),async {
                loop {
                    if voice.snapshot().phase==VoiceSessionPhase::ListeningConfirm {break;}
                    assert!(file.exists(),"the real file cannot be deleted while the confirmation prompt is pending");
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }).await.unwrap_or_else(|_|panic!("the actual pending tool must trigger spoken listening; host={:?}; daemon={:?}; tts={:?}",manager.snapshot(),voice.snapshot(),acoustic_tts.as_ref().unwrap().0.lock().unwrap()));
            assert!(file.exists());
            if scenario!=ReadScenario::OralApprovalSilence {feed_control_speech(producer).await;}
        }
        let done = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                let response = app.clone().oneshot(axum::http::Request::builder().uri(format!("/api/voice/companion/tasks/{command_id}")).body(axum::body::Body::empty()).unwrap()).await.unwrap();
                assert_eq!(response.status(), axum::http::StatusCode::OK);
                let task: VoiceTaskSnapshot = serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 32_768).await.unwrap()).unwrap();
                if matches!(task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) {break task;}
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }).await.expect("real voice task must reach its persisted terminal receipt");
        if cancel_running {
            assert_eq!(done.status,VoiceTaskStatus::Failed,"{done:?}");
            assert_eq!(done.result_ok,Some(false));
            assert_eq!(observed_read.load(Ordering::SeqCst),0,"the cancelled model response must never dispatch Read");
            assert_eq!(calls.load(Ordering::SeqCst),1);
        } else {
            assert_eq!(done.status, VoiceTaskStatus::Completed, "{done:?}");
            if approval {assert_eq!(file.exists(),!approving,"only the real accepted approval may delete the file: {done:?}");}
            else {assert!(done.result_text.as_deref().unwrap().contains(&marker));}
            assert_eq!(observed_read.load(Ordering::SeqCst), if queued_drain { 2 } else { 1 });
            assert_eq!(calls.load(Ordering::SeqCst), if approval && approving && !approval_shell {4}else if approval {3}else if queued_drain {4}else{2});
        }
        let (creation_count,):(i64,) = sqlx::query_as("SELECT COUNT(*) FROM conversation_creation_keys WHERE user_id = ? AND creation_key LIKE 'voice-create:%'")
            .bind(services.authoritative_user_id.as_ref()).fetch_one(&database_pool).await.unwrap();
        let (delivery_count,):(i64,) = sqlx::query_as("SELECT COUNT(*) FROM conversation_delivery_receipts WHERE conversation_id = ?")
            .bind(done.conversation_id.as_deref().unwrap()).fetch_one(&database_pool).await.unwrap();
        if from_worker {
            assert_eq!(delivery_count, if voice_controls || keyboard_queue { 3 } else { 2 }, "unified conversation must hold dialogue projection receipt(s) and each executed task delivery receipt");
        } else {
            assert_eq!((creation_count,delivery_count),(1,if queued_drain { 2 } else { 1 }),"one voice command must create one conversation identity and one real send receipt");
        }
        eprintln!("voice_acceptance_evidence: {}",serde_json::json!({
            "scenario":format!("{scenario:?}"),"command_id":done.command_id,"session_id":done.session_id,"acoustic_turn_id":done.acoustic_turn_id,
            "conversation_id":done.conversation_id,"message_id":done.message_id,"runtime_turn_id":done.runtime_turn_id,"status":done.status,"result_ok":done.result_ok,
            "creation_records":creation_count,"delivery_receipts":delivery_count,"asr_calls":acoustic_stt.as_ref().map_or(0,|stt|stt.calls.load(Ordering::SeqCst)),
            "dialogue_calls":dialogue_calls.load(Ordering::SeqCst),"execution_model_calls":calls.load(Ordering::SeqCst),"actual_read_results":observed_read.load(Ordering::SeqCst),
            "file_exists":file.exists(),"host_instance_id":manager.snapshot().instance_id,"host_seq":manager.snapshot().seq,
            "real_acoustics":real_acoustics,"physical_devices":false
        }));
        if queued_drain {
            let drained_b = drained_b_record.as_ref().unwrap();
            assert_eq!(manager.snapshot().task.as_ref(), Some(drained_b));
            manager.replay_task_projection(VoiceTaskSnapshot {status:VoiceTaskStatus::Accepted,result_ok:None,result_text:None,..drained_b.clone()});
            assert_eq!(manager.snapshot().task.as_ref(),Some(drained_b),"a delayed accepted projection must not regress the saved completed result");
        } else {
            assert_eq!(manager.snapshot().task.as_ref(), Some(&done));
            manager.replay_task_projection(VoiceTaskSnapshot {status:VoiceTaskStatus::Accepted,result_ok:None,result_text:None,..done.clone()});
            assert_eq!(manager.snapshot().task.as_ref(),Some(&done),"a delayed accepted projection must not regress the saved completed result");
            if !from_worker {
                let cancel_request = |instance_id:&str, conversation_id:Option<&str>| axum::http::Request::builder()
                    .method("POST").uri(format!("/api/voice/companion/tasks/{command_id}/cancel"))
                    .header("content-type","application/json").body(axum::body::Body::from(serde_json::json!({
                        "instance_id":instance_id,"command_id":command_id,"conversation_id":conversation_id,"message_id":done.message_id
                    }).to_string())).unwrap();
                let rejected=app.clone().oneshot(cancel_request("previous-host",done.conversation_id.as_deref())).await.unwrap();
                assert_eq!(rejected.status(),axum::http::StatusCode::CONFLICT,"old host controls must be rejected before touching a runtime");
                let rejected=app.clone().oneshot(cancel_request(&manager.snapshot().instance_id,Some("a-different-conversation"))).await.unwrap();
                assert_eq!(rejected.status(),axum::http::StatusCode::CONFLICT);
                let terminal=app.clone().oneshot(cancel_request(&manager.snapshot().instance_id,done.conversation_id.as_deref())).await.unwrap();
                assert_eq!(terminal.status(),axum::http::StatusCode::OK);
                let terminal:VoiceTaskSnapshot=serde_json::from_slice(&axum::body::to_bytes(terminal.into_body(),32_768).await.unwrap()).unwrap();
                assert_eq!(terminal,done,"cancelling an already completed task cannot invent a cancelled outcome");
            }
        }
        if let Some((voice,_)) = &acoustic {
            if oral_approval {
                assert_eq!(dialogue_calls.load(Ordering::SeqCst),1,"an approval answer must never invoke dialogue inference");
                assert_eq!(acoustic_stt.as_ref().unwrap().calls.load(Ordering::SeqCst),if scenario==ReadScenario::OralApprovalSilence {1}else{2});
                assert!(acoustic_tts.as_ref().unwrap().0.lock().unwrap().iter().any(|text|text.contains("ApplyPatch")));
                assert_eq!(file.exists(),!approving);
                acoustic_worker.take().unwrap().abort();daemon_server.take().unwrap().abort();drop(bridge);server.abort();return;
            }
            if voice_controls {
                assert_eq!(done.status,VoiceTaskStatus::Failed);
                assert_eq!(dialogue_calls.load(Ordering::SeqCst),2,"oral query and cancellation bypass the model");
                assert_eq!(acoustic_stt.as_ref().unwrap().calls.load(Ordering::SeqCst),4);
                assert_eq!(observed_read.load(Ordering::SeqCst),0);
                assert!(!acoustic_tts.as_ref().unwrap().0.lock().unwrap().iter().any(|text|text.contains("模型不能")),"unverified model acknowledgements cannot be spoken");
                acoustic_worker.take().unwrap().abort();daemon_server.take().unwrap().abort();drop(bridge);server.abort();return;
            }
            tokio::time::timeout(std::time::Duration::from_secs(4),async {
                loop {
                    let snapshot = voice.snapshot();
                    if snapshot.assistant_text.as_deref().is_some_and(|text|text.contains(&marker))
                        && snapshot.phase==VoiceSessionPhase::FollowUp
                    {break;}
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            }).await.expect("a real task result must return to daemon text and playback");
            assert_eq!(dialogue_calls.load(Ordering::SeqCst),1,"one utterance uses one dialogue routing call");
            assert_eq!(acoustic_stt.as_ref().unwrap().calls.load(Ordering::SeqCst),1,"one utterance uses one ASR call");
            if real_acoustics {
                let snapshot = voice.snapshot();
                let audio = snapshot.audio.unwrap();
                assert!(audio.kws_ready && audio.vad_ready && audio.asr_ready,"replay must load actual acoustic engines");
                assert!(snapshot.conversation.iter().any(|turn|turn.user_text.as_deref().is_some_and(|text|text.contains("天气"))),"the fixed speech must reach real ASR and remain readable");
            }
            assert!(acoustic_audio.as_ref().unwrap().frame_count()>=2,"acceptance and real result must both reach the playback sink");
            assert!(acoustic_tts.as_ref().unwrap().0.lock().unwrap().iter().any(|text|text.contains(&marker)));
            if result_followup {
                feed_control_speech(&acoustic.as_ref().unwrap().1).await;
                tokio::time::timeout(std::time::Duration::from_secs(8),async {
                    loop {
                        let snapshot=voice.snapshot();assert!(snapshot.error.is_none(),"follow-up failed: {:?}",snapshot.error);
                        if snapshot.phase==VoiceSessionPhase::FollowUp && snapshot.assistant_text.as_deref().is_some_and(|text|text.contains("刚才文件内容是")){break;}
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }).await.unwrap();
                assert_eq!(dialogue_calls.load(Ordering::SeqCst),2);assert_eq!(acoustic_stt.as_ref().unwrap().calls.load(Ordering::SeqCst),2);
                assert_eq!(calls.load(Ordering::SeqCst),2);assert_eq!(observed_read.load(Ordering::SeqCst),1,"a short explanation must not execute the task again");
            }
            for wav in acoustic_audio.as_ref().unwrap().frames() {
                let (pcm,rate) = nomifun_audio::parse_audio_frame_to_pcm(&wav);
                assert_eq!(rate,24_000);
                assert!(pcm.iter().any(|sample|sample.abs()>0.001),"the test synthesis must deliver valid non-silent WAV to the production playback pipeline");
            }
            acoustic_worker.take().unwrap().abort();
            daemon_server.take().unwrap().abort();
            drop(bridge);
            server.abort();
            return;
        }
        if queued_drain {
            server.abort();
            return;
        }
        manager.replay_acoustic_source(VoiceSessionSnapshot {seq:2, ..source});
        assert_eq!(manager.snapshot().task.as_ref(), Some(&done), "later acoustic snapshots cannot erase the real task result");
        let replay = app.clone().oneshot(submit_request()).await.unwrap();
        assert_eq!(replay.status(), axum::http::StatusCode::OK);
        let replay: VoiceTaskDelivery = serde_json::from_slice(&axum::body::to_bytes(replay.into_body(), 32_768).await.unwrap()).unwrap();
        assert_eq!(replay.task, done);
        assert!(!replay.freshly_accepted);
        assert_eq!(calls.load(Ordering::SeqCst),if cancel_running {1}else if approval && approving && !approval_shell {4}else if approval {3}else{2});
        server.abort();
    }
    #[test]
    fn execution_requires_an_explicit_existing_workspace_and_model_or_preset() {
        let dir=tempfile::tempdir().unwrap();
        let mut config=VoiceExecutionConfig {workspace:dir.path().to_string_lossy().into(),model:None,preset_id:None,session_mode:None};
        assert_eq!(validate_workspace(&config).unwrap_err(),"task_execution_model_unconfigured");
        config.preset_id=Some("configured-preset".into());assert!(validate_workspace(&config).is_ok());
        config.workspace="relative-workspace".into();assert_eq!(validate_workspace(&config).unwrap_err(),"task_workspace_invalid");
        config.workspace=dir.path().join("not-created").to_string_lossy().into();assert_eq!(validate_workspace(&config).unwrap_err(),"task_workspace_invalid");
        assert!(!dir.path().join("not-created").exists());
    }
}
