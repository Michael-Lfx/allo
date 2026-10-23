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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceCallStartRequest {
    pub request_id: String,
    pub target: VoiceCallTarget,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceConversationTurn {
    pub turn_id: String,
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
