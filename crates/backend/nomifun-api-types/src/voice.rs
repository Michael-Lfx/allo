//! Voice Companion & Local Voice Model DTO contracts.
//!
//! Authoritative references:
//! - `docs/architecture/voice-agent/interaction.zh.md` §3.4.4
//! - `docs/architecture/voice-agent/model-management.zh.md` §4

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceFeedbackMode {
    #[default]
    Light,
    Essential,
    Off,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct VoiceFeedbackConfig {
    pub mode: VoiceFeedbackMode,
    pub volume: u8,
    pub speak_replies: bool,
}

impl Default for VoiceFeedbackConfig {
    fn default() -> Self {
        Self {
            mode: VoiceFeedbackMode::Light,
            volume: 50,
            speak_replies: true,
        }
    }
}

impl VoiceFeedbackConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.volume > 100 {
            Err("feedback_volume_invalid")
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceFeedbackSettings {
    pub requested: VoiceFeedbackConfig,
    /// None means the saved preference has not been verified on a live daemon.
    pub effective: Option<VoiceFeedbackConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceFeedbackPatch {
    pub mode: Option<VoiceFeedbackMode>,
    pub volume: Option<u8>,
    pub speak_replies: Option<bool>,
}

impl VoiceFeedbackPatch {
    pub fn apply(self, current: VoiceFeedbackConfig) -> VoiceFeedbackConfig {
        VoiceFeedbackConfig {
            mode: self.mode.unwrap_or(current.mode),
            volume: self.volume.unwrap_or(current.volume),
            speak_replies: self.speak_replies.unwrap_or(current.speak_replies),
        }
    }
}

impl From<VoiceFeedbackConfig> for VoiceFeedbackPatch {
    fn from(c: VoiceFeedbackConfig) -> Self {
        Self {
            mode: Some(c.mode),
            volume: Some(c.volume),
            speak_replies: Some(c.speak_replies),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceFeedbackPreviewRequest {
    pub kind: VoiceCueKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelRef {
    pub provider_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceExecutionConfig {
    pub workspace: String,
    pub model: Option<VoiceModelRef>,
    pub preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceExecutionSettings {
    pub requested: Option<VoiceExecutionConfig>,
    pub effective: Option<VoiceExecutionConfig>,
    pub available: bool,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTaskCommand {
    pub command_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub goal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTaskSubmitRequest {
    pub instance_id: String,
    #[serde(flatten)]
    pub command: VoiceTaskCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceKeyboardMessageRequest {
    pub conversation_id: String,
    pub command_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceQueueItem {
    pub task: VoiceTaskSnapshot,
    pub display_text: String,
    pub workspace: String,
    /// Zero is the running/unresolved item; waiting positions are one based.
    pub position: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTaskStatus {
    Queued,
    Delivering,
    Unknown,
    Accepted,
    Cancelling,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTaskSnapshot {
    pub command_id: String,
    pub session_id: String,
    pub acoustic_turn_id: String,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub runtime_turn_id: Option<String>,
    pub status: VoiceTaskStatus,
    pub result_ok: Option<bool>,
    pub result_text: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTaskDelivery {
    pub task: VoiceTaskSnapshot,
    pub freshly_accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTaskReference {
    pub command_id:String,
    pub conversation_id:Option<String>,
    pub message_id:Option<String>,
}
impl From<&VoiceTaskSnapshot> for VoiceTaskReference {
    fn from(task:&VoiceTaskSnapshot) -> Self {
        Self {command_id:task.command_id.clone(),conversation_id:task.conversation_id.clone(),message_id:task.message_id.clone()}
    }
}
impl VoiceTaskReference {
    pub fn matches(&self, task:&VoiceTaskSnapshot) -> bool {self==&Self::from(task)}
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all="snake_case")]
pub enum VoiceTaskControlAction {QueryStatus,CancelTask}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTaskControlRequest {
    pub instance_id:String,
    #[serde(flatten)]
    pub task:VoiceTaskReference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTaskControlCommand {
    #[serde(flatten)]
    pub scope:VoiceSessionControlRequest,
    pub task:VoiceTaskReference,
    pub action:VoiceTaskControlAction,
    /// The user's original utterance for this acoustic turn. The host uses it to
    /// persist the question and to re-check an explicit cancel before acting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_user_text: Option<String>,
    /// The locale frozen for this call, so the recorded result matches what was
    /// spoken even if the UI language changes later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTaskResultAnnouncement {
    #[serde(flatten)]
    pub scope: VoiceSessionControlRequest,
    pub command_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceToolConfirmation {
    pub approval_id: String,
    pub msg_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceToolConfirmationRequest {
    pub instance_id:String,
    pub command_id:String,
    pub conversation_id:Option<String>,
    pub message_id:Option<String>,
    pub msg_id:String,
    pub call_id:String,
    pub approved:bool,
}

impl VoiceToolConfirmationRequest {
    pub fn task_reference(&self)->VoiceTaskReference {
        VoiceTaskReference {command_id:self.command_id.clone(),conversation_id:self.conversation_id.clone(),message_id:self.message_id.clone()}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceToolConfirmationPrompt {
    #[serde(flatten)]
    pub scope:VoiceSessionControlRequest,
    pub task:VoiceTaskReference,
    pub approval_id:String,
    pub msg_id:String,
    pub call_id:String,
    pub expires_at_ms:i64,
    pub text:String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceToolConfirmationReply {
    pub approval_id:String,
    pub msg_id:String,
    pub call_id:String,
    pub approved:bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceDialogueConfig {
    pub dialogue_model: Option<VoiceModelRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceDialogueSettings {
    #[serde(flatten)]
    pub config: VoiceDialogueConfig,
    pub effective_model: Option<VoiceModelRef>,
    pub available: bool,
}

/// Shared wire contract used by the daemon and the host's durable decision authority.
#[derive(Debug,Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Serialize,Deserialize)]
#[serde(rename_all="snake_case")]
pub enum VoiceIntentType {Ignore,Chat,AgentTask,TaskControl}

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
pub struct VoiceDecision {
    pub intent_type:VoiceIntentType,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub understood_request:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub task_control_action:Option<VoiceTaskControlAction>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub reply_tts:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub spoken_ack:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub task_goal:Option<String>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub requires_confirmation:Option<bool>,
}

pub fn parse_voice_decision(raw:&str)->Result<VoiceDecision,String> {
    let trimmed=raw.trim();
    let body=trimmed.strip_prefix("```json").or_else(||trimmed.strip_prefix("```"));
    let cleaned=body.map(|body|body.trim().strip_suffix("```").unwrap_or(body).trim()).unwrap_or(trimmed);
    let value:serde_json::Value=serde_json::from_str(cleaned).map_err(|_|"model_response_invalid")?;
    for key in ["reply_tts","understood_request","spoken_ack","task_goal","task_control_action"] {
        if value.get(key).is_some_and(|v|!v.is_string()) {return Err("model_response_invalid".into());}
    }
    if value.get("requires_confirmation").is_some_and(|v|!v.is_boolean()) {return Err("model_response_invalid".into());}
    let decision:VoiceDecision=serde_json::from_value(value).map_err(|_|"model_response_invalid")?;
    if decision.understood_request.as_deref()==Some("")
        || (decision.intent_type==VoiceIntentType::Chat && decision.reply_tts.as_deref().is_none_or(|s|s.trim().is_empty())) {
        return Err("model_response_invalid".into());
    }
    Ok(decision)
}

/// Validated speech content only; task acknowledgements never enter this channel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceDialogueSentence {
    pub text: String,
    pub understood_request: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag="type", rename_all="snake_case")]
pub enum VoiceDialogueFrame {
    Sentence { sentence: VoiceDialogueSentence },
    Complete { response: VoiceDialogueResponse },
    Error { code: String },
}

/// The first complete JSON object freezes routing; later objects may only extend chat.
#[derive(Default)]
pub struct VoiceDecisionStream {
    buffer: String,
    decision: Option<VoiceDecision>,
    sentences: usize,
}
impl VoiceDecisionStream {
    pub fn push(&mut self, delta:&str)->Result<Vec<VoiceDialogueSentence>, &'static str> {
        if self.buffer.len()+delta.len()>32_768 {return Err("model_response_invalid");}
        self.buffer.push_str(delta);
        let mut ready=Vec::new();
        loop {
            let mut values=serde_json::Deserializer::from_str(&self.buffer).into_iter::<serde_json::Value>();
            let value=match values.next() {
                Some(Ok(value))=>value,
                Some(Err(error)) if error.is_eof()=>break,
                Some(Err(_))=>return Err("model_response_invalid"),
                None=>{self.buffer.clear();break;},
            };
            let consumed=values.byte_offset();
            self.buffer.drain(..consumed);
            if self.decision.is_none() {
                let decision=parse_voice_decision(&value.to_string()).map_err(|_|"model_response_invalid")?;
                if decision.intent_type==VoiceIntentType::Chat {
                    let text=decision.reply_tts.clone().ok_or("model_response_invalid")?;
                    if text.chars().count()>60 {return Err("model_response_invalid");}
                    ready.push(VoiceDialogueSentence{text,understood_request:decision.understood_request.clone()});
                    self.sentences=1;
                }
                self.decision=Some(decision);
            } else {
                #[derive(Deserialize)]
                struct Continuation {reply_delta:String}
                let continuation:Continuation=serde_json::from_value(value).map_err(|_|"model_response_invalid")?;
                let decision=self.decision.as_mut().unwrap();
                if decision.intent_type!=VoiceIntentType::Chat || continuation.reply_delta.trim().is_empty() || self.sentences>=4 {return Err("model_response_invalid");}
                let reply=decision.reply_tts.as_mut().ok_or("model_response_invalid")?;
                if reply.chars().count()+continuation.reply_delta.chars().count()>60 {return Err("model_response_invalid");}
                reply.push_str(&continuation.reply_delta);
                self.sentences+=1;
                ready.push(VoiceDialogueSentence{text:continuation.reply_delta,understood_request:None});
            }
        }
        Ok(ready)
    }
    pub fn finish(self)->Result<VoiceDecision, &'static str> {
        let remainder = self.buffer.trim();
        let stripped = remainder.strip_prefix("```json").or_else(|| remainder.strip_prefix("```")).map(|s| s.strip_suffix("```").unwrap_or(s).trim()).unwrap_or(remainder);
        if !stripped.is_empty() {return Err("model_response_invalid");}
        self.decision.ok_or("model_response_invalid")
    }
}

fn voice_stream_disabled(value:&bool)->bool {!value}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceDialogueRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default,skip_serializing_if="voice_stream_disabled")]
    pub stream: bool,

    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_user_text: Option<String>,
}

impl VoiceDialogueRequest {
    pub fn scoped(session_id: &str, turn_id: &str, raw_user_text: &str) -> Self {
        Self {
            voice_record_id: Some(format!("voice-{turn_id}")),
            binding_id: Some(format!("voice-binding-{session_id}")),
            session_id: Some(session_id.into()), turn_id: Some(turn_id.into()),
            raw_user_text: Some(raw_user_text.into()), ..Default::default()
        }
    }
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceDialogueResponse {
    pub content: String,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub record_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTargetMode {
    #[default]
    VoiceHome,
    Companion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTargetConfig {
    pub mode: VoiceTargetMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion_id: Option<String>,
}

impl Default for VoiceTargetConfig {
    fn default() -> Self {
        Self {
            mode: VoiceTargetMode::VoiceHome,
            companion_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTargetSettings {
    pub requested: VoiceTargetConfig,
    pub effective: VoiceTargetConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTargetUpdateRequest {
    pub mode: VoiceTargetMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTaskNotification {
    pub command_id: String,
    pub notification_id: String,
    pub attempt_id: String,
    pub status: String,
    pub result_summary: String,
    pub created_at: i64,
}

/// A playback attempt is independent of the execution outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoicePlaybackReceipt {
    pub attempt_id: String,
    pub status: String,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTaskReplayRequest {
    pub instance_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub replay_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceBindingSnapshot {
    pub session_id: String,
    pub binding_id: String,
    pub conversation_id: String,
    pub workspace: Option<String>,
    #[serde(default)]
    pub memory_owner_companion_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTaskProgress {
    pub command_id: String,
    pub stage: String,
    pub health: String,
    pub last_event_at_ms: Option<i64>,
    pub last_progress_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceCallTarget {
    VoiceConversation { conversation_id: String },
    Workspace { workspace: String },
    DefaultWorkspace,
    CurrentSelection,
    NewVoiceConversation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<VoiceModelRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset_id: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceCallStartRequest {
    pub request_id: String,
    pub target: VoiceCallTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceCallStartStatus {
    Starting,
    Ready,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceCallStartResponse {
    pub request_id: String,
    pub status: VoiceCallStartStatus,
    pub conversation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_owner_companion_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<VoiceBindingSnapshot>,
}
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSessionPhase {
    #[default]
    Idle,
    Listening,
    Transcribing,
    Deciding,
    Synthesizing,
    Speaking,
    FollowUp,
    TextFollowUp,
    AwaitingConfirmation,
    ListeningConfirm,
    Dispatching,
    Error,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceConnectionState {
    #[default]
    Stopped,
    Starting,
    Connected,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceSessionError {
    pub stage: String,
    pub code: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceCueKind {
    Wake,
    Recognized,
    FollowUp,
    Sleep,
    Stopped,
    ListeningReminder,
    Processing,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceCueStatus {
    Pending,
    Started,
    Completed,
    Cancelled,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceFeedbackDiagnostic {
    pub request_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub kind: VoiceCueKind,
    pub status: VoiceCueStatus,
    pub reason_code: Option<String>,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSessionEndReason {
    InitialSilence,
    FollowUpSilence,
    UserStop,
    NoIntent,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSessionControlRequest {
    pub instance_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
}

impl VoiceSessionControlRequest {
    pub fn matches(&self, snapshot: &VoiceSessionSnapshot) -> bool {
        self.instance_id == snapshot.instance_id
            && self.session_id == snapshot.session_id
            && self.turn_id == snapshot.turn_id
    }
}

/// Latest session projection. It contains no audio, credentials, or execution authority.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceAudioDiagnostics {
    pub rms_milli: u16,
    pub peak_milli: u16,
    pub frames_received: u64,
    pub last_frame_at_ms: Option<i64>,
    pub capture_state: String,
    pub speech_detected: bool,
    pub asr_ready: bool,
    pub kws_ready: bool,
    #[serde(default)]
    pub vad_ready: bool,
    /// Automatic interruption is disabled unless echo-free input is verified.
    #[serde(default)]
    pub barge_in_ready: bool,
    #[serde(default)]
    pub barge_in_reason: Option<String>,
    #[serde(default)]
    pub utterance_samples: u32,
    #[serde(default)]
    pub utterance_peak_milli: u16,
}

/// Synthesis provider selected for the latest scoped attempt; this is not a playback receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTtsDiagnostic {
    pub provider: String,
    pub voice: Option<String>,
    pub online: bool,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceConversationTurn {
    pub turn_id: String,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub user_message_id: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub assistant_message_id: Option<String>,
    pub user_text: Option<String>,
    #[serde(default)]
    pub understood_request: Option<String>,
    pub assistant_text: Option<String>,
    pub model_text: Option<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub reply_state: Option<VoiceReplyState>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub error: Option<VoiceSessionError>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum VoiceReplyState {Streaming,Complete,Interrupted}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceSessionSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub tts: Option<VoiceTtsDiagnostic>,
    /// Host-authoritative completed projections, used to retire live transcript placeholders.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projected_turn_ids: Vec<String>,
    #[serde(default)]
    pub queue: Vec<VoiceQueueItem>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub binding: Option<VoiceBindingSnapshot>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub task_progress: Option<VoiceTaskProgress>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub task_notification: Option<VoiceTaskNotification>,
    #[serde(default)]
    pub confirmation:Option<VoiceToolConfirmation>,
    #[serde(default)]
    pub task: Option<VoiceTaskSnapshot>,
    #[serde(default)]
    pub task_ready: bool,
    #[serde(default)]
    pub task_error_code: Option<String>,
    #[serde(default)]
    pub ended_reason: Option<VoiceSessionEndReason>,
    #[serde(default)]
    pub feedback: Option<VoiceFeedbackDiagnostic>,
    #[serde(default)]
    pub conversation: Vec<VoiceConversationTurn>,
    pub enabled: bool,
    pub connection: VoiceConnectionState,
    pub instance_id: String,
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_epoch: Option<u64>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub phase: VoiceSessionPhase,
    pub user_text: Option<String>,
    #[serde(default)]
    pub understood_request: Option<String>,
    pub assistant_text: Option<String>,
    pub follow_up_deadline_ms: Option<i64>,
    #[serde(default)]
    pub listening_deadline_ms: Option<i64>,
    pub error: Option<VoiceSessionError>,
    pub acoustic_ready: bool,
    pub dialogue_ready: bool,
    pub playback_ready: bool,
    #[serde(default)]
    pub audio: Option<VoiceAudioDiagnostics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum VoiceCompanionEvent {
    TtsSource { session_id: Option<String>, turn_id: Option<String>, diagnostic: VoiceTtsDiagnostic },
    SessionEnded {
        reason: VoiceSessionEndReason,
    },
    Feedback {
        diagnostic: VoiceFeedbackDiagnostic,
    },
    ModelResponse {
        text: String,
    },
    AudioDiagnostics {
        diagnostics: VoiceAudioDiagnostics,
    },
    Snapshot {
        snapshot: VoiceSessionSnapshot,
    },
    Availability {
        acoustic_ready: bool,
        dialogue_ready: bool,
        playback_ready: bool,
    },
    Error {
        stage: String,
        code: String,
    },
    WakeHit {
        keyword: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locale: Option<String>,
    },
    Listening,
    ListeningReady {
        timeout_secs: u32,
    },
    ListeningDeadlineChanged {
        remaining_ms: u64,
    },
    SpeechStarted,
    Transcribing,
    SpeechRecognized {
        text: String,
    },
    IntentDecided {
        intent: String,
        reply: Option<String>,
        #[serde(default)]
        understood_request: Option<String>,
    },
    TaskDispatching,
    TaskResultReady {
        command_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notification_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attempt_id: Option<String>,
        #[serde(default)]
        goal: Option<String>,
        text: String,
    },
    ReplySentence {text:String,understood_request:Option<String>},
    ResponseReady {text:String},
    ConfirmationPrompted {approval_id:String,text:String,expires_at_ms:i64},
    ConfirmationListening {deadline_ms:i64},
    ConfirmationHeard {text:String},
    TtsSynthesizing {
        text: String,
    },
    TtsSpeaking {
        text: String,
    },
    FollowUpStarted {
        text_only: bool,
    },
    FollowUpWindow {
        timeout_secs: u32,
    },
    TextFollowUpWindow {
        timeout_secs: u32,
    },
    Idle,
}

/// 7-state lifecycle machine for local voice model assets.
///
/// Baseline: `interaction.zh.md` §3.4.4 and `model-management.zh.md` §4.
/// Wire format: tagged with `"status"`, variant struct payloads in `"detail"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "detail")]
pub enum ModelInstallState {
    /// 未安装：本地模型文件缺失或不完整
    NotInstalled,
    /// 探针选源中：正在并发探测 ModelScope / GitHub 等镜像源
    ProbingSource,
    /// 下载中：包含正在下载文件、字节数进度、百分比、实时下载速度
    Downloading {
        current_file: String,
        file_index: usize,
        total_files: usize,
        downloaded_bytes: u64,
        total_bytes: u64,
        progress_percent: f32,
        speed_bytes_per_sec: u64,
    },
    /// 密码学哈希自检中：7 个下载分发文件逐个 SHA256 校验
    VerifyingSha256 {
        current_file: String,
        verified_files: usize,
        total_files: usize,
    },
    /// 完全就绪：校验 100% 匹配，可供声学引擎 mmap 安全加载
    Ready {
        models_dir: PathBuf,
        total_size_bytes: u64,
    },
    /// 安装或校验失败：包含错误原因与可重试标志
    Failed { reason: String, recoverable: bool },
    /// 正在卸载释放磁盘空间 (桌面端按需触发)
    Removing,
}

impl Default for ModelInstallState {
    fn default() -> Self {
        Self::NotInstalled
    }
}

impl ModelInstallState {
    /// Returns the wire status name of this variant.
    pub fn status_name(&self) -> &'static str {
        match self {
            Self::NotInstalled => "NotInstalled",
            Self::ProbingSource => "ProbingSource",
            Self::Downloading { .. } => "Downloading",
            Self::VerifyingSha256 { .. } => "VerifyingSha256",
            Self::Ready { .. } => "Ready",
            Self::Failed { .. } => "Failed",
            Self::Removing => "Removing",
        }
    }

    /// Whether the models are fully ready for acoustic inference.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// Whether the model manager is currently executing an asynchronous transition.
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::ProbingSource
                | Self::Downloading { .. }
                | Self::VerifyingSha256 { .. }
                | Self::Removing
        )
    }

    /// Whether the model manager is in a failed state.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Detailed status for an individual model asset file on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelFileDetail {
    /// Relative subpath in the model topology (e.g. "kws/encoder.int8.onnx")
    pub path: String,
    /// File size in bytes (0 if missing)
    pub size_bytes: u64,
    /// Expected SHA256 checksum from the authoritative manifest
    pub expected_sha256: Option<String>,
    /// Actual SHA256 checksum if computed
    pub actual_sha256: Option<String>,
    /// Whether the file is present on disk and non-empty
    pub exists: bool,
}

/// Response DTO for `GET /api/voice/models/status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelStatusResponse {
    /// Current lifecycle state of the model manager
    pub state: ModelInstallState,
    /// Root path to models directory
    pub models_dir: PathBuf,
    /// Total disk space consumed by installed models in bytes
    pub total_size_bytes: u64,
    /// Per-file breakdown of model assets
    pub files: Vec<VoiceModelFileDetail>,
}

impl VoiceModelStatusResponse {
    /// Creates a status response indicating models are not installed.
    pub fn not_installed(models_dir: PathBuf) -> Self {
        Self {
            state: ModelInstallState::NotInstalled,
            models_dir,
            total_size_bytes: 0,
            files: Vec::new(),
        }
    }
}

/// Request DTO for `POST /api/voice/models/install`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelInstallRequest {
    /// Preferred mirror source ("auto", "modelscope", "github")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefer_mirror: Option<String>,
    /// Optional custom models directory override
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_dir: Option<PathBuf>,
    /// Force re-download and re-verification even if files or state appear ready
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

/// Response DTO for `POST /api/voice/models/install`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelInstallResponse {
    /// Whether the installation task was accepted/started
    pub accepted: bool,
    /// Current or newly transitioned lifecycle state
    pub state: ModelInstallState,
    /// Optional status or informational message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Server-Sent Event wrapper for `GET /api/voice/models/progress`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelProgressEvent {
    /// Event name/type (e.g. "progress")
    pub event: String,
    /// The current model installation state payload
    pub data: ModelInstallState,
}

impl VoiceModelProgressEvent {
    /// Create a new progress event.
    pub fn new(data: ModelInstallState) -> Self {
        Self {
            event: "progress".to_string(),
            data,
        }
    }
}

/// Request DTO for `DELETE /api/voice/models`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceModelDeleteRequest {
    /// Optional force flag to remove even if in unexpected state
    #[serde(default)]
    pub force: bool,
}

/// Response DTO for `DELETE /api/voice/models`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceModelDeleteResponse {
    /// Whether the deletion completed successfully
    pub success: bool,
    /// Number of bytes freed from disk
    pub freed_bytes: u64,
    /// State after deletion (resets to `NotInstalled`)
    pub state: ModelInstallState,
    /// Optional status message
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Response DTO for `GET /api/voice/diagnostics/device`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceDeviceStatusResponse {
    /// Whether a default system microphone is detected and accessible
    pub mic_available: bool,
    /// Device name if available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// Default hardware sample rate in Hz
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    /// Channel count (e.g. 1 mono, 2 stereo)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u16>,
    /// Error message if probe failed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn default_asr_sample_rate() -> u32 {
    16_000
}

/// Request DTO for `POST /api/voice/diagnostics/asr-test`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceAsrTestRequest {
    /// Optional base64-encoded audio (WAV container or raw 16-bit PCM)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_base64: Option<String>,
    /// Sample rate in Hz (default 16000)
    #[serde(default = "default_asr_sample_rate")]
    pub sample_rate: u32,
}

/// Response DTO for `POST /api/voice/diagnostics/asr-test`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceAsrTestResponse {
    /// Whether recognition completed successfully
    pub success: bool,
    /// Transcribed text from SenseVoice model
    pub text: String,
    /// Duration of audio in milliseconds
    pub duration_ms: u64,
    /// Whether transcribed text matched a known wake phrase
    #[serde(default)]
    pub wake_matched: bool,
    /// Clean command text with wake phrase prefix stripped
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean_text: Option<String>,
    /// Error message if ASR failed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceCapabilityStatus {
    #[default]
    Unknown,
    Checking,
    Ready,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VoiceCapabilityEvidence {
    pub status: VoiceCapabilityStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// Stable evidence kind, e.g. `recent_pcm`, `tts_decoded`, `live_response`,
    /// `engine_loaded`. It is not a substitute for the evidence itself.
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at_ms: Option<i64>,
}

impl VoiceCapabilityEvidence {
    pub fn unknown(evidence: &str) -> Self {
        Self {
            status: VoiceCapabilityStatus::Unknown,
            reason_code: None,
            evidence: evidence.into(),
            checked_at_ms: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceCapabilityReport {
    pub capture: VoiceCapabilityEvidence,
    pub kws: VoiceCapabilityEvidence,
    pub vad: VoiceCapabilityEvidence,
    pub asr: VoiceCapabilityEvidence,
    pub llm: VoiceCapabilityEvidence,
    pub tts: VoiceCapabilityEvidence,
    pub playback: VoiceCapabilityEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at_ms: Option<i64>,
    #[serde(default)]
    pub generation: u64,
    #[serde(default)]
    pub can_start_full_call: bool,
    #[serde(default)]
    pub can_wake: bool,
    #[serde(default)]
    pub can_transcribe: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(default)]
    pub degraded_text_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceSelfCheckRequest {
    /// Explicit opt-in: the check may call the configured dialogue provider.
    #[serde(default)]
    pub include_llm: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTtsSelfCheckLanguage {
    pub language: String,
    pub voice: String,
    pub ok: bool,
    pub sample_rate: u32,
    pub samples: usize,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub rms_milli: u16,
    #[serde(default)]
    pub peak_milli: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceEngineSelfCheckResponse {
    pub kws_ok: bool,
    pub vad_ok: bool,
    pub asr_ok: bool,
    pub fixtures_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub kws_hits: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asr_text: Option<String>,
    /// Echoed request identity; a late reply for another check is discarded.
    #[serde(default)]
    pub request_id: String,
    /// Daemon runtime instance that owns the worker.
    #[serde(default)]
    pub instance_id: String,
    /// Worker generation; a restarted worker cannot satisfy an old check.
    #[serde(default)]
    pub worker_epoch: u64,
    /// Installed model identity observed by the worker that ran the check.
    #[serde(default)]
    pub model_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixtures_source: Option<String>,
    #[serde(default)]
    pub fixtures_online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixtures_generated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceTtsSelfCheckResponse {
    pub ok: bool,
    pub provider: String,
    pub online: bool,
    pub playback_ok: bool,
    pub languages: Vec<VoiceTtsSelfCheckLanguage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Classifies a voice utterance into a task-control action only when the
/// evidence is unambiguous. A negated, mixed or object-less cancel is never a
/// cancel command; the caller must fall back to a normal reply or a safe
/// clarification. Status questions remain read-only.
pub fn classify_voice_task_control(input: &str) -> Option<VoiceTaskControlAction> {
    const PREFIX_IGNORES: &[&str] = &["读取", "创建", "分析", "扫描", "整理", "备份"];
    const EN_NEGATORS: &[&str] = &[
        "not", "never", "no", "don't", "dont", "doesn't", "doesnt", "didn't", "didnt",
        "won't", "wont", "wouldn't", "wouldnt", "shouldn't", "shouldnt", "can't", "cant",
        "cannot", "isn't", "isnt", "aren't", "arent", "without", "avoid", "prevent",
        "nor", "neither", "hardly", "barely",
    ];
    const EN_AFTER_NEGATORS: &[&str] = &[
        "is not needed", "is not necessary", "not needed", "not necessary",
        "isn't needed", "isnt needed",
    ];
    const ZH_NEGATORS: &[&str] = &[
        "千万别", "千万不要", "不要", "别", "不用", "不能", "请勿", "无需", "不必",
        "不准", "绝不", "永不", "禁止", "勿", "不",
    ];
    const ZH_AFTER_NEGATORS: &[&str] = &["是不", "不需要", "不必", "无需", "不用", "不成"];

    let trimmed = input.trim();
    if trimmed.is_empty() || PREFIX_IGNORES.iter().any(|prefix| trimmed.starts_with(prefix)) {
        return None;
    }
    let lower = normalize_voice_text(trimmed);
    let tokens = english_tokens(&lower);
    let has_task_word = trimmed.contains("任务") || tokens.iter().any(|token| is_task_object_token(token));

    // Chat/weather must not become a task query merely because it contains
    // "情况怎么样". Only an explicit task object may continue.
    if !has_task_word {
        const WEATHER_OR_CHAT_ZH: &[&str] = &[
            "天气", "气温", "下雨", "下雪", "阴天", "晴天", "家里", "身体", "家人",
            "最近怎么样", "最近好吗", "过得怎么样",
        ];
        const WEATHER_OR_CHAT_EN: &[&str] = &[
            "weather", "temperature", "rain", "forecast", "how's it going",
            "how is it going", "how are you",
        ];
        if WEATHER_OR_CHAT_ZH.iter().any(|word| trimmed.contains(word))
            || WEATHER_OR_CHAT_EN.iter().any(|word| lower.contains(word))
        {
            return None;
        }
    }
    let is_query = is_voice_status_query(trimmed, &lower, &tokens, has_task_word);
    let is_question = trimmed.contains('？') || trimmed.contains('?');

    let (mut affirmative, mut negated) = (0u8, 0u8);

    for verb in ["取消", "停止", "终止", "结束"] {
        for (offset, _) in trimmed.match_indices(verb) {
            if !has_task_word {
                continue;
            }
            // A preceding clause can make the apparent command conditional.
            if trimmed[..offset].chars().any(is_clause_punctuation) {
                continue;
            }
            let before: String = trimmed[..offset]
                .chars()
                .rev()
                .take(12)
                .take_while(|c| !is_clause_punctuation(*c))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let after: String = trimmed[offset + verb.len()..].chars().take(32).collect();
            // "任务取消了" / "任务已经结束" are descriptions, not commands.
            // Require verb-object order or an imperative marker.
            if !is_chinese_imperative_cancel(&before, &after) {
                continue;
            }
            let negated_mention = ZH_NEGATORS.iter().any(|word| before.contains(word))
                || ZH_AFTER_NEGATORS.iter().any(|word| after.contains(word));
            if negated_mention {
                negated = negated.saturating_add(1);
            } else {
                affirmative = affirmative.saturating_add(1);
            }
        }
    }

    for (index, token) in tokens.iter().enumerate() {
        if token != "cancel" && token != "stop" {
            continue;
        }
        if !has_task_word {
            continue;
        }
        if token == "stop" && tokens[index + 1..].iter().take(4).any(|next| next == "from") {
            continue;
        }
        let after = tokens[index + 1..].iter().take(8).cloned().collect::<Vec<_>>().join(" ");
        let before_negated = tokens[index.saturating_sub(6)..index]
            .iter()
            .any(|word| EN_NEGATORS.contains(&word.as_str()));
        let after_negated = EN_AFTER_NEGATORS.iter().any(|phrase| after.contains(phrase));
        if before_negated || after_negated {
            negated = negated.saturating_add(1);
        } else if is_english_imperative_cancel(&tokens, index) {
            affirmative = affirmative.saturating_add(1);
        }
    }

    match (affirmative > 0, negated > 0, is_query) {
        (true, true, _) => is_query.then_some(VoiceTaskControlAction::QueryStatus),
        (true, false, true) => Some(VoiceTaskControlAction::QueryStatus),
        (true, false, false) if !is_question => Some(VoiceTaskControlAction::CancelTask),
        (true, false, false) => None,
        (false, _, true) => Some(VoiceTaskControlAction::QueryStatus),
        (false, _, false) => None,
    }
}

/// True only for a positive, object-explicit cancel command. The host uses the
/// same classifier at the control boundary so a loose model decision cannot
/// cancel after a negation or an ambiguous utterance.
pub fn is_affirmative_cancel_command(input: &str) -> bool {
    classify_voice_task_control(input) == Some(VoiceTaskControlAction::CancelTask)
}

fn normalize_voice_text(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            '’' | '‘' | '`' => '\'',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

fn english_tokens(input: &str) -> Vec<String> {
    input
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '\''))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn is_task_object_token(token: &str) -> bool {
    let token = token.strip_suffix("'s").unwrap_or(token);
    matches!(token, "task" | "tasks" | "job" | "jobs" | "operation" | "operations")
}

fn is_english_imperative_cancel(tokens: &[String], verb_index: usize) -> bool {
    let prefix: Vec<&str> = tokens[..verb_index].iter().map(String::as_str).collect();
    if !matches!(
        prefix.as_slice(),
        [] | ["please"] | ["can", "you"] | ["could", "you"] | ["would", "you"]
            | ["could", "you", "please"] | ["would", "you", "please"]
    ) {
        return false;
    }
    let tail = &tokens[verb_index + 1..];
    let Some(object) = tail.iter().position(|token| is_task_object_token(token)) else {
        return false;
    };
    tail[..object].iter().all(|token| matches!(token.as_str(), "the" | "a" | "my" | "our" | "this" | "that" | "current" | "active" | "running"))
        && tail[object + 1..].iter().all(|token| matches!(token.as_str(), "please" | "now" | "immediately" | "right" | "away" | "thanks" | "thank" | "you"))
}

fn is_chinese_imperative_cancel(before: &str, after: &str) -> bool {
    let after_clause = after
        .split(is_clause_punctuation)
        .next()
        .unwrap_or("")
        .trim_start();
    let full_after = after.trim();
    if ["如果", "假如", "要是", "除非", "以后", "稍后", "明天", "将来", "之后"].iter().any(|word| full_after.contains(word)) {
        return false;
    }
    let before = before.trim();
    let mut prefix = if let Some((prefix, object)) = before.split_once('把') {
        if !matches!(object, "任务" | "当前任务" | "这个任务" | "该任务" | "正在进行的任务") {
            return false;
        }
        prefix
    } else {
        before
    };
    const DIRECT_PREFIXES: &[&str] = &[
        "为我们", "给我们", "帮我们", "帮我", "帮忙", "替我", "为我", "给我",
        "麻烦", "烦请", "劳驾", "立即", "马上", "立刻", "赶紧", "快点", "抓紧", "尽快", "现在", "请", "你", "您", "妳",
    ];
    while !prefix.is_empty() {
        let Some(marker) = DIRECT_PREFIXES.iter().find(|marker| prefix.starts_with(**marker)) else {
            return false;
        };
        prefix = &prefix[marker.len()..];
    }
    const QUESTION_TAILS: &[&str] = &[
        "吗", "呢", "么", "好不好", "行不行", "可不可以", "可以", "好吗", "行吗",
        "对不对", "是不是", "对吧", "是吧", "能不能", "会不会", "要不要", "是否",
        "可否", "能否", "有没有", "干嘛", "干啥", "哪", "谁", "多少", "几天",
        "几点", "多久", "多长时间",
    ];
    if full_after.contains('？')
        || full_after.contains('?')
        || QUESTION_TAILS.iter().any(|tail| full_after.contains(*tail))
    {
        return false;
    }
    const NEGATORS: &[&str] = &[
        "不要", "别", "不用", "不能", "请勿", "无需", "不必", "不准", "绝不", "永不",
        "禁止", "勿", "不",
    ];
    if NEGATORS.iter().any(|word| full_after.contains(*word)) {
        return false;
    }

    if ["他们", "她们", "他", "她", "它"]
        .iter()
        .any(|subject| before.contains(*subject))
    {
        return false;
    }
    let has_service = ["帮我", "帮忙", "替我", "为我", "为我们", "给我们", "帮我们"]
        .iter()
        .any(|marker| before.contains(*marker));
    if ["我们", "咱们", "我", "咱", "自己", "本人"]
        .iter()
        .any(|subject| before.contains(*subject))
        && !has_service
    {
        return false;
    }

    const STRONG_MARKERS: &[&str] = &[
        "请", "帮我", "帮忙", "麻烦", "烦请", "劳驾", "立即", "马上", "立刻",
        "赶紧", "快点", "抓紧", "尽快", "现在", "给我", "替我", "为我",
        "为我们", "给我们", "帮我们",
    ];
    let has_strong = STRONG_MARKERS
        .iter()
        .any(|marker| before.starts_with(*marker));
    let has_second_person = ["你", "您", "妳"]
        .iter()
        .any(|subject| before.contains(*subject));
    let has_weak = before.contains('把');
    if !has_weak && before.contains('的') {
        return false;
    }

    if let Some(task_index) = after_clause.find("任务") {
        if !matches!(&after_clause[..task_index], "" | "当前" | "这个" | "该" | "正在进行的" | "正在执行的") {
            return false;
        }
        let suffix = &after_clause[task_index + "任务".len()..];
        return is_imperative_cancel_tail(suffix)
            && (before.trim().is_empty() || has_strong || has_second_person);
    }

    if !is_imperative_cancel_tail(after_clause) {
        return false;
    }
    if has_weak {
        let subject = before.split('把').next().unwrap_or("");
        if subject.contains('的') {
            return false;
        }
        return subject.trim().is_empty() || has_strong || has_second_person;
    }
    has_strong
}

fn is_imperative_cancel_tail(tail: &str) -> bool {
    const TAILS: &[&str] = &[
        "", "吧", "啊", "呀", "哈", "哦", "啦", "咯", "呗", "嘛", "掉", "一下", "谢谢",
        "多谢", "拜托", "掉吧", "一下吧", "一下呗",
    ];
    TAILS.contains(&tail)
}

fn is_clause_punctuation(c: char) -> bool {
    matches!(c, '，' | '。' | '！' | '？' | '；' | '：' | ',' | '.' | '!' | '?' | ';' | ':')
}

fn is_voice_status_query(trimmed: &str, lower: &str, tokens: &[String], has_task_word: bool) -> bool {
    const QUERY_KEYWORDS_ZH: &[&str] = &[
        "任务进度", "任务状态", "查询进度", "查询状态", "查询任务", "查看进度", "查看状态",
        "情况怎么样", "怎么样了", "进展怎么样", "如何了", "做完了吗", "做到哪了", "完成了吗",
        "执行得怎么样", "重听", "再播一遍", "再说一遍", "刚才说了什么", "再读一遍",
    ];
    const QUERY_KEYWORDS_EN: &[&str] = &[
        "task status", "task progress", "what's the status", "what is the status",
        "how is the task", "how's the task", "are you done", "is it done", "say again",
        "repeat that", "pardon",
    ];
    if QUERY_KEYWORDS_ZH.iter().any(|keyword| trimmed.contains(keyword))
        || QUERY_KEYWORDS_EN.iter().any(|keyword| lower.contains(keyword))
    {
        return true;
    }
    if has_task_word {
        const TASK_QUERY_ZH: &[&str] = &[
            "进度", "状态", "情况", "进展", "任务如何", "结束了吗", "取消了吗", "停止了吗",
            "完成了吗", "完了吗", "怎么样了",
        ];
        if TASK_QUERY_ZH.iter().any(|keyword| trimmed.contains(keyword)) {
            return true;
        }
        if tokens.iter().any(|token| matches!(token.as_str(), "progress" | "status")) {
            return true;
        }
    }
    false
}

/// Returns true if the optional locale string represents an English locale
/// (e.g. "en", "en-US", "EN-US", "En-us", "en_GB", "EN_gb", "en.utf8").
/// Case-insensitive, robust against separators ('-', '_', ' ', '.'), and panic-safe for any UTF-8 string.
pub fn is_english_locale(locale: Option<&str>) -> bool {
    locale.is_some_and(|l| {
        l.split(['-', '_', ' ', '.'])
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case("en"))
    })
}

#[cfg(test)]
mod locale_helper_tests {
    use super::*;

    #[test]
    fn test_is_english_locale_cases() {
        assert!(is_english_locale(Some("en")));
        assert!(is_english_locale(Some("EN")));
        assert!(is_english_locale(Some("en-US")));
        assert!(is_english_locale(Some("EN-US")));
        assert!(is_english_locale(Some("En-us")));
        assert!(is_english_locale(Some("en_GB")));
        assert!(is_english_locale(Some("EN_gb")));
        assert!(is_english_locale(Some("en.UTF-8")));

        assert!(!is_english_locale(Some("zh-CN")));
        assert!(!is_english_locale(Some("ZH-CN")));
        assert!(!is_english_locale(Some("zh")));
        assert!(!is_english_locale(Some("fr-FR")));
        assert!(!is_english_locale(Some("")));
        assert!(!is_english_locale(Some("   ")));
        assert!(!is_english_locale(None));
        assert!(!is_english_locale(Some("中")));
        assert!(!is_english_locale(Some("中文")));
        assert!(!is_english_locale(Some("日本語")));
    }
}

#[cfg(test)]
mod voice_task_control_contract_tests {
    use super::*;

    #[test]
    fn control_command_roundtrips_optional_provenance() {
        let command = VoiceTaskControlCommand {
            scope: VoiceSessionControlRequest {
                instance_id: "instance".into(),
                session_id: Some("session".into()),
                turn_id: Some("turn".into()),
            },
            task: VoiceTaskReference {
                command_id: "command".into(),
                conversation_id: Some("conversation".into()),
                message_id: Some("message".into()),
            },
            action: VoiceTaskControlAction::CancelTask,
            raw_user_text: Some("cancel the task".into()),
            locale: Some("en-US".into()),
        };
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(encoded["raw_user_text"], "cancel the task");
        assert_eq!(encoded["locale"], "en-US");
        let decoded: VoiceTaskControlCommand = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.action, VoiceTaskControlAction::CancelTask);
        assert_eq!(decoded.raw_user_text.as_deref(), Some("cancel the task"));
        assert_eq!(decoded.locale.as_deref(), Some("en-US"));
    }
}

#[cfg(test)]
mod task_control_classifier_tests {
    use super::*;

    #[test]
    fn cancel_requires_an_imperative_target() {
        use VoiceTaskControlAction::{CancelTask, QueryStatus};
        let cases = [
            ("取消任务", Some(CancelTask)),
            ("把任务取消", Some(CancelTask)),
            ("请取消任务", Some(CancelTask)),
            ("请帮我取消当前任务", Some(CancelTask)),
            ("立即停止任务", Some(CancelTask)),
            ("取消任务吧", Some(CancelTask)),
            ("把任务取消吧", Some(CancelTask)),
            ("请把任务取消", Some(CancelTask)),
            ("取消任务，谢谢", Some(CancelTask)),
            ("你取消任务", Some(CancelTask)),
            ("取消任务。", Some(CancelTask)),
            ("取消", None),
            ("stop", None),
            ("stop the task", Some(CancelTask)),
            ("please cancel the current task", Some(CancelTask)),
            ("cancel the task tomorrow", None),
            ("We should cancel the task tomorrow", None),
            ("I will cancel the task tomorrow", None),
            ("Explain how to cancel the task", None),
            ("If it fails, cancel the task", None),
            ("cancel the task if it fails", None),
            ("stop the playback of the task", None),
            ("请明天取消任务", None),
            ("请稍后取消任务", None),
            ("如果出错，请取消任务", None),
            ("取消任务，如果它失败", None),
            ("请告诉我如何取消任务", None),
            ("取消任务并查询进度", Some(QueryStatus)),
            ("这个任务如何", Some(QueryStatus)),
            ("我把任务取消了", None),
            ("把任务取消了", None),
            ("把任务停止了", None),
            ("取消任务很重要", None),
            ("取消任务，可以吗", None),
            ("取消任务可以吗", None),
            ("取消任务好吗", None),
            ("取消任务，好吗", None),
            ("取消任务，多久", None),
            ("取消任务？", None),
            ("取消？", None),
            ("stop?", None),
            ("stop the task?", None),
            ("cancel?", None),
            ("cancel the task?", None),
            ("我取消任务", None),
            ("他取消任务", None),
            ("取消任务了", None),
            ("任务取消了", None),
            ("任务取消", None),
            ("不要取消任务", None),
            ("取消任务，但是不要停止", None),
            ("任务已经结束了", None),
        ];
        for (input, expected) in cases {
            assert_eq!(
                classify_voice_task_control(input),
                expected,
                "input={input:?}"
            );
            assert_eq!(
                is_affirmative_cancel_command(input),
                expected == Some(CancelTask),
                "host guard input={input:?}"
            );
        }
    }
}
