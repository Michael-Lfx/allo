//! Durable voice deliveries, adapted to the existing Conversation authority.
//! Recovery observes receipts; it never reconstructs a send request on its own.
use std::{path::PathBuf, sync::Arc};
use async_trait::async_trait;
use nomifun_api_types::voice::{VoiceExecutionConfig, VoiceTaskCommand, VoiceTaskDelivery, VoiceTaskNotification, VoiceTaskSnapshot, VoiceTaskStatus};
use nomifun_api_types::ResolvedPresetSnapshot;
use nomifun_api_types::voice::{VoiceToolConfirmation,VoiceToolConfirmationRequest};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub(crate) struct TaskReceipt {
    pub message_id: String,
    pub runtime_turn_id: Option<String>,
    pub replayed: bool,
    pub completed: bool,
    pub result_ok: Option<bool>,
    pub result_text: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug)]
pub(crate) enum TaskMutationError {
    Rejected(String),
    Uncertain(String),
}

#[async_trait]
pub(crate) trait VoiceTaskRuntime: Send + Sync {
    fn progress_events(&self,_conversation_id:&str)->Option<tokio::sync::broadcast::Receiver<nomifun_ai_agent::AgentStreamEvent>> {None}
    fn transport_healthy(&self,_conversation_id:&str)->Option<bool> {None}
    async fn resolve_configuration(&self, config: &VoiceExecutionConfig) -> Result<VoiceExecutionConfig, String>;
    async fn prepare_configuration(&self, config: &VoiceExecutionConfig) -> Result<(VoiceExecutionConfig,Option<ResolvedPresetSnapshot>),String>;
    async fn create(&self, owner: &str, config: &VoiceExecutionConfig, preset:Option<&ResolvedPresetSnapshot>, creation_key: &str) -> Result<String, TaskMutationError>;
    async fn find_created(&self, owner: &str, creation_key: &str) -> Result<Option<String>, String>;
    async fn send(&self, owner: &str, conversation_id: &str, send_key: &str, goal: &str) -> Result<TaskReceipt, TaskMutationError>;
    async fn observe(&self, owner: &str, conversation_id: &str, send_key: &str) -> Result<Option<TaskReceipt>, String>;
    async fn cancel(&self, owner: &str, conversation_id: &str, send_key:&str,message_id:&str) -> Result<(), String>;
    async fn confirmations(&self,owner:&str,conversation_id:&str,send_key:&str,message_id:&str)->Result<Vec<VoiceToolConfirmation>,String>;
    async fn confirm(&self,owner:&str,conversation_id:&str,send_key:&str,message_id:&str,confirmation:&VoiceToolConfirmation,approved:bool)->Result<(),String>;
}

pub const DEFAULT_VOICE_TASK_TIMEOUT_MS: i64 = 30 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DeliveryRecord {
    #[serde(default)]
    pub announcement_claimed: bool,
    #[serde(default)]
    pub notification_id: Option<String>,
    #[serde(default)]
    pub attempt_id: Option<String>,
    #[serde(default)]
    pub last_attempt_at_ms: Option<i64>,
    #[serde(default)]
    pub notification_status: Option<String>,
    #[serde(default)]
    pub replay_ids: Vec<String>,
    pub owner: String,
    pub command: VoiceTaskCommand,
    pub config: VoiceExecutionConfig,
    #[serde(default)]
    pub preset_snapshot: Option<ResolvedPresetSnapshot>,
    pub creation_key: String,
    pub send_key: String,
    pub task: VoiceTaskSnapshot,
    #[serde(default)]
    pub send_started_at_ms: Option<i64>,
    #[serde(default)]
    pub deadline_at_ms: Option<i64>,
    #[serde(default)]
    pub last_observed_at_ms: Option<i64>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub last_receipt_at_ms: Option<i64>,
    #[serde(default)]
    pub handoff_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeliveryLedger { owner:String, records:Vec<DeliveryRecord> }

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredDeliveries { Ledger(DeliveryLedger), Single(DeliveryRecord) }

struct TaskProgressObserver {
    receiver:Option<tokio::sync::broadcast::Receiver<nomifun_ai_agent::AgentStreamEvent>>,
    stage:String,last_event:Option<i64>,last_progress:Option<i64>,last_fact:Option<String>,
}

pub(crate) struct VoiceTaskService {
    owner: String,
    path: PathBuf,
    runtime: Arc<dyn VoiceTaskRuntime>,
    timeout_ms: i64,
    operation: Mutex<()>,
    progress: std::sync::Mutex<std::collections::HashMap<String,TaskProgressObserver>>,
}

fn is_explicit_goal_failure(text: &str) -> bool {
    const FAILURE_INDICATORS: &[&str] = &[
        "未能获取", "未能查询", "无法获取", "无法查询", "未能找到", "查询失败",
        "获取失败", "调用失败", "未成功", "未能成功", "抱歉，未能", "抱歉，无法",
        "未能为您", "无法为您", "未能完成", "很抱歉，未能", "很抱歉，无法",
    ];
    FAILURE_INDICATORS.iter().any(|k| text.contains(k))
}

impl VoiceTaskService {
    pub fn new(owner: String, path: PathBuf, runtime: Arc<dyn VoiceTaskRuntime>) -> Self {
        Self { owner, path, runtime, timeout_ms: DEFAULT_VOICE_TASK_TIMEOUT_MS, operation: Mutex::new(()), progress: Default::default() }
    }

    #[cfg(test)]
    pub fn with_timeout_ms(mut self, timeout_ms: i64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub async fn resolve_configuration(&self, config: &VoiceExecutionConfig) -> Result<VoiceExecutionConfig, String> {
        self.read()?;
        self.runtime.resolve_configuration(config).await
    }

    pub fn progress_for(&self,task:&VoiceTaskSnapshot)->Result<nomifun_api_types::voice::VoiceTaskProgress,String> {
        let record=self.read_command(&task.command_id)?.ok_or("task_not_found")?;
        let now=nomifun_common::now_ms();
        let mut observers=self.progress.lock().unwrap_or_else(|e|e.into_inner());
        observers.retain(|key,_|key==&task.command_id);
        let observer=observers.entry(task.command_id.clone()).or_insert_with(||TaskProgressObserver {
            receiver:task.conversation_id.as_deref().and_then(|id|self.runtime.progress_events(id)),
            stage:"waiting".into(),last_event:None,last_progress:None,last_fact:None,
        });
        if observer.receiver.is_none() {observer.receiver=task.conversation_id.as_deref().and_then(|id|self.runtime.progress_events(id));}
        if let Some(receiver)=observer.receiver.as_mut() {
            loop {match receiver.try_recv() {
                Ok(event)=>{
                    use nomifun_ai_agent::AgentStreamEvent;
                    observer.last_event=Some(now);
                    let fact=serde_json::to_string(&event).unwrap_or_default();
                    let stage=match event {
                        AgentStreamEvent::Text(text) if !text.content.trim().is_empty()=>Some("answering"),
                        AgentStreamEvent::Thinking(_)=>Some("thinking"),
                        AgentStreamEvent::ToolCall(_)|AgentStreamEvent::AcpToolCall(_)=>Some("tool"),
                        AgentStreamEvent::Permission(_)|AgentStreamEvent::AcpPermission(_)=>Some("approval"),
                        AgentStreamEvent::Plan(_)=>Some("planning"),
                        _=>None,
                    };
                    if let Some(stage)=stage && observer.last_fact.as_ref()!=Some(&fact) {observer.stage=stage.into();observer.last_progress=Some(now);observer.last_fact=Some(fact);}
                },
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_))=>{observer.last_event=None;break;},
                Err(_)=>break,
            }}
        }
        let receipt_fresh=record.last_receipt_at_ms.is_some_and(|at|now.saturating_sub(at)<=30_000);
        let healthy=task.conversation_id.as_deref().and_then(|id|self.runtime.transport_healthy(id))==Some(true);
        let progress_at=observer.last_progress.or(record.send_started_at_ms).unwrap_or(0);
        let health=if !receipt_fresh || !healthy {"unverified"}else if now.saturating_sub(progress_at)>=90_000 {"no_recent_progress"}else{"fresh"};
        Ok(nomifun_api_types::voice::VoiceTaskProgress {command_id:task.command_id.clone(),stage:observer.stage.clone(),health:health.into(),last_event_at_ms:observer.last_event,last_progress_at_ms:observer.last_progress})
    }

    pub async fn status_for(&self, command_id: &str) -> Result<VoiceTaskSnapshot, String> {
        let guard = self.operation.try_lock();
        let record = self.read_command(command_id)?.ok_or("task_not_found")?;
        if guard.is_err() {return Ok(record.task);}
        self.reconcile(record).await
    }

    pub fn current_task(&self) -> Result<Option<VoiceTaskSnapshot>,String> {
        Ok(self.read()?.map(|record|record.task))
    }

    pub async fn replay(&self, command:&VoiceTaskCommand) -> Result<Option<VoiceTaskDelivery>,String> {
        let Some(record)=self.read_command(&command.command_id)? else {return Ok(None);};
        if record.command!=*command {return Err("task_command_conflict".into());}
        Ok(Some(VoiceTaskDelivery {task:self.status_for(&command.command_id).await?,freshly_accepted:false}))
    }

    pub fn announcement_claimed(&self, command_id: &str) -> Result<bool, String> {
        Ok(self.read_command(command_id)?.is_none_or(|record|record.announcement_claimed))
    }

    pub fn pending_result(&self) -> Result<Option<(VoiceTaskSnapshot,String)>,String> {
        Ok(self.read_ledger()?.and_then(|ledger|ledger.records.into_iter().find(|record|
            !record.announcement_claimed && matches!(record.task.status,VoiceTaskStatus::Completed|VoiceTaskStatus::Failed)
        )).map(|record|(record.task,record.command.goal)))
    }

    pub async fn claim_announcement(&self, command_id: &str, claimed: bool) -> Result<bool, String> {
        let _guard = self.operation.lock().await;
        let mut record = self.read_command(command_id)?.ok_or("task_not_found")?;
        if !matches!(record.task.status,VoiceTaskStatus::Completed|VoiceTaskStatus::Failed) {return Ok(false);}
        if record.announcement_claimed==claimed {return Ok(false);}
        record.announcement_claimed=claimed;
        record.notification_status=Some(if claimed {"dispatched"}else{"pending"}.into());
        if claimed {
            if record.notification_id.is_none() {
                record.notification_id = Some(format!("voice-notif:{}", command_id));
            }
            let next_seq = record.attempt_id.as_deref()
                .and_then(|id| id.rsplit_once(':').and_then(|(_, s)| s.parse::<u64>().ok()))
                .unwrap_or(0) + 1;
            record.attempt_id = Some(format!("voice-attempt:{}:{}", command_id, next_seq));
            record.last_attempt_at_ms = Some(nomifun_common::now_ms());
        }
        self.save(&record)?;
        Ok(true)
    }

    pub fn notification_for(&self, command_id: &str) -> Result<Option<VoiceTaskNotification>, String> {
        let record = match self.read_command(command_id)? {
            Some(record) => record,
            None => return Ok(None),
        };
        if !matches!(record.task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) {
            return Ok(None);
        }
        let notification_id = record.notification_id.unwrap_or_else(|| format!("voice-notif:{}", command_id));
        let attempt_id = record.attempt_id.unwrap_or_else(|| format!("voice-attempt:{}:0", command_id));
        let summary = record.task.result_text.unwrap_or_else(|| {
            if record.task.status == VoiceTaskStatus::Completed {
                "任务已完成".into()
            } else {
                "任务未完成".into()
            }
        });
        Ok(Some(VoiceTaskNotification {
            command_id: command_id.to_string(),
            notification_id,
            attempt_id,
            status: record.notification_status.unwrap_or_else(||if record.announcement_claimed {"unknown"}else{"pending"}.into()),
            result_summary: summary,
            created_at: record.last_attempt_at_ms.or(record.send_started_at_ms).unwrap_or(0),
        }))
    }

    pub async fn finish_announcement(&self,command_id:&str,attempt_id:&str,status:&str)->Result<(),String> {
        if !matches!(status,"dispatched"|"started"|"played"|"failed"|"unknown"|"suppressed") {return Err("task_announcement_failed".into());}
        let _guard=self.operation.lock().await;
        let mut record=self.read_command(command_id)?.ok_or("task_not_found")?;
        if record.attempt_id.as_deref()!=Some(attempt_id) {return Err("task_scope_changed".into());}
        if record.notification_status.as_deref()==Some(status) {return Ok(());}
        record.notification_status=Some(status.into());
        self.save(&record)
    }

    pub fn unresolved_announcements(&self)->Result<Vec<(String,String)>,String> {
        Ok(self.read_ledger()?.into_iter().flat_map(|l|l.records).filter(|r|
            r.announcement_claimed && matches!(r.notification_status.as_deref(),Some("dispatched"|"started"|"unknown")|None))
            .filter_map(|r|r.attempt_id.map(|a|(r.command.command_id,a))).collect())
    }

    pub async fn request_replay(&self,command_id:&str,replay_id:&str)->Result<bool,String> {
        if replay_id.is_empty() || replay_id.len()>128 {return Err("task_scope_invalid".into());}
        let _guard=self.operation.lock().await;
        let mut record=self.read_command(command_id)?.ok_or("task_not_found")?;
        if record.replay_ids.iter().any(|id|id==replay_id) {return Ok(false);}
        if matches!(record.notification_status.as_deref(),Some("dispatched"|"started")) {return Err("voice_busy".into());}
        record.replay_ids.push(replay_id.into());
        record.announcement_claimed=false;
        record.notification_status=Some("pending".into());
        self.save(&record)?;
        Ok(true)
    }

    fn read_ledger(&self) -> Result<Option<DeliveryLedger>, String> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("task_state_read_failed".into()),
        };
        let stored:StoredDeliveries = serde_json::from_slice(&bytes).map_err(|_| "task_state_invalid")?;
        // Read the previously written single-record format once; all subsequent
        // writes retain the full ledger. There is still one delivery authority.
        let ledger = match stored {
            StoredDeliveries::Ledger(ledger) => ledger,
            StoredDeliveries::Single(record) => DeliveryLedger {owner:record.owner.clone(),records:vec![record]},
        };
        if ledger.owner!=self.owner {return Ok(None);}
        let mut commands=std::collections::HashSet::new();
        for record in &ledger.records {
            if record.owner!=ledger.owner || record.command.command_id!=record.task.command_id
                || record.command.turn_id!=record.task.acoustic_turn_id || record.command.session_id!=record.task.session_id
                || !commands.insert(&record.command.command_id)
            {return Err("task_state_invalid".into());}
        }
        Ok(Some(ledger))
    }

    fn read(&self) -> Result<Option<DeliveryRecord>,String> {
        Ok(self.read_ledger()?.and_then(|mut ledger|ledger.records.pop()))
    }

    fn read_command(&self, command_id:&str) -> Result<Option<DeliveryRecord>,String> {
        Ok(self.read_ledger()?.and_then(|ledger|ledger.records.into_iter().find(|record|record.command.command_id==command_id)))
    }

    fn save(&self, record: &DeliveryRecord) -> Result<(), String> {
        let previous=self.read_ledger()?;
        if previous.is_none() && self.path.exists() {return Err("task_owner_changed".into());}
        let mut ledger=previous.unwrap_or_else(||DeliveryLedger {owner:self.owner.clone(),records:Vec::new()});
        if let Some(index)=ledger.records.iter().position(|saved|saved.command.command_id==record.command.command_id) {
            let saved=&ledger.records[index];
            if saved.command!=record.command || saved.config!=record.config || saved.preset_snapshot!=record.preset_snapshot || saved.creation_key!=record.creation_key || saved.send_key!=record.send_key {
                return Err("task_state_invalid".into());
            }
            ledger.records[index]=record.clone();
        } else {ledger.records.push(record.clone());}
        let bytes = serde_json::to_vec(&ledger).map_err(|_| "task_state_invalid")?;
        nomifun_common::dir_config::write_atomic_replace(&self.path, &bytes).map_err(|_| "task_state_write_failed".into())
    }

    pub(crate) fn validate_command(command: &VoiceTaskCommand) -> Result<(), String> {
        for id in [&command.command_id, &command.session_id, &command.turn_id] {
            if id.is_empty() || id.len()>128 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) { return Err("task_scope_invalid".into()); }
        }
        if command.goal.trim().is_empty() || command.goal.len()>16_384 { return Err("task_goal_invalid".into()); }
        Ok(())
    }

    fn persist_mutation_failure(&self, record:&mut DeliveryRecord,error:TaskMutationError,unknown_code:&str) -> Result<String,String> {
        let code = match error {
            TaskMutationError::Rejected(code) => {
                record.task.status=VoiceTaskStatus::Failed;
                record.task.result_ok=Some(false);
                record.task.error_code=Some(code.clone());
                code
            },
            TaskMutationError::Uncertain(code) => {
                record.task.status=VoiceTaskStatus::Unknown;
                record.task.error_code=Some(code);
                unknown_code.to_owned()
            },
        };
        self.save(record)?;
        Ok(code)
    }

    fn apply_receipt(record: &mut DeliveryRecord, receipt: TaskReceipt) -> Result<(), String> {
        if receipt.message_id.is_empty() || record.task.message_id.as_ref().is_some_and(|id| *id != receipt.message_id) {
            return Err("task_receipt_scope_changed".into());
        }
        record.task.message_id = Some(receipt.message_id);
        if let Some(turn) = receipt.runtime_turn_id { record.task.runtime_turn_id = Some(turn); }
        let effective_ok = match receipt.result_ok {
            Some(true) => {
                if let Some(text) = &receipt.result_text {
                    if is_explicit_goal_failure(text) {
                        Some(false)
                    } else {
                        Some(true)
                    }
                } else {
                    Some(true)
                }
            }
            other => other,
        };
        if receipt.completed {
            record.task.status = match effective_ok {
                Some(true) => VoiceTaskStatus::Completed,
                Some(false) => VoiceTaskStatus::Failed,
                None => VoiceTaskStatus::Unknown,
            };
        } else if record.task.status != VoiceTaskStatus::Cancelling {
            record.task.status = VoiceTaskStatus::Accepted;
        }
        record.task.result_ok = effective_ok;
        record.task.result_text = receipt.result_text;
        record.task.error_code = if receipt.completed && effective_ok.is_none() {
            Some("task_result_unverified".into())
        } else if receipt.completed && effective_ok == Some(false) && receipt.error_code.is_none() {
            Some("goal_unfulfilled".into())
        } else {
            receipt.error_code
        };
        Ok(())
    }

    async fn reconcile(&self, mut record: DeliveryRecord) -> Result<VoiceTaskSnapshot, String> {
        if matches!(record.task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) { return Ok(record.task); }
        let now_ms = nomifun_common::now_ms();
        let clock_rollback = record.last_observed_at_ms.is_some_and(|last| now_ms + 5_000 < last);
        let deadline_exceeded = record.deadline_at_ms.is_some_and(|dl| now_ms >= dl);
        if deadline_exceeded || clock_rollback {
            record.cancel_requested = true;
            record.task.error_code = Some(if clock_rollback {
                "task_clock_rollback_detected".into()
            } else { "task_deadline_exceeded".into() });
        }
        if record.cancel_requested { record.task.status = VoiceTaskStatus::Cancelling; }
        // Persist control intent before any fallible or stalled observation.
        self.save(&record)?;
        if !clock_rollback {
            record.last_observed_at_ms = Some(now_ms);
        }

        if record.task.conversation_id.is_none() {
            record.task.conversation_id=tokio::time::timeout(std::time::Duration::from_secs(2),
                self.runtime.find_created(&self.owner,&record.creation_key)).await.ok().and_then(Result::ok).flatten();
        }
        if let Some(conversation) = &record.task.conversation_id {
            let cancellation_code = record.task.error_code.clone();
            match tokio::time::timeout(std::time::Duration::from_secs(2), self.runtime.observe(&self.owner, conversation, &record.send_key)).await {
                Ok(Ok(Some(receipt))) => {record.last_receipt_at_ms=Some(now_ms);Self::apply_receipt(&mut record, receipt)?;},
                Ok(Ok(None)) => { if !record.cancel_requested { record.task.status = VoiceTaskStatus::Unknown; record.task.error_code = Some("task_receipt_missing".into()); } },
                _ => { if !record.cancel_requested { record.task.error_code = Some("task_receipt_unavailable".into()); } },
            }
            if record.cancel_requested && !matches!(record.task.status,VoiceTaskStatus::Completed|VoiceTaskStatus::Failed) {record.task.error_code=cancellation_code;}
        } else {
            if !record.cancel_requested {
                record.task.status = VoiceTaskStatus::Unknown;
                record.task.error_code = Some("task_creation_unknown".into());
            }
        }

        // Terminal status from observation wins (completion vs cancellation race)
        if matches!(record.task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) {
            self.save(&record)?;
            return Ok(record.task);
        }

        if record.cancel_requested && record.task.status != VoiceTaskStatus::Cancelling {
            record.task.status = VoiceTaskStatus::Cancelling;
        }

        // Deadline check or Clock rollback:
        if record.cancel_requested {
            self.save(&record)?;

            if let (Some(conversation), Some(message_id)) = (&record.task.conversation_id, &record.task.message_id) {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2),self.runtime.cancel(&self.owner, conversation, &record.send_key, message_id)).await;
            }
            return Ok(record.task);
        }

        self.save(&record)?;
        Ok(record.task)
    }

    pub async fn status(&self) -> Result<Option<VoiceTaskSnapshot>, String> {
        let guard = self.operation.try_lock();
        if guard.is_err() {return self.current_task();}
        match self.read()? { Some(record) => self.reconcile(record).await.map(Some), None => Ok(None) }
    }

    #[cfg(test)]
    pub async fn submit(&self, command: VoiceTaskCommand, config: VoiceExecutionConfig) -> Result<VoiceTaskDelivery, String> {
        self.submit_frozen(command,config,None).await
    }
    #[allow(dead_code)]
    pub async fn submit_frozen(&self, command:VoiceTaskCommand, config:VoiceExecutionConfig,expected_preset:Option<Option<ResolvedPresetSnapshot>>)->Result<VoiceTaskDelivery,String> {
        self.submit_frozen_with_handoff(command, config, expected_preset, None).await
    }

    pub async fn submit_frozen_with_handoff(
        &self,
        command: VoiceTaskCommand,
        config: VoiceExecutionConfig,
        expected_preset: Option<Option<ResolvedPresetSnapshot>>,
        handoff_content: Option<String>,
    ) -> Result<VoiceTaskDelivery, String> {
        let _guard = self.operation.lock().await;
        if let Some(previous)=self.read_command(&command.command_id)? {
            if previous.command!=command {return Err("task_command_conflict".into());}
            return Ok(VoiceTaskDelivery {task:self.reconcile(previous).await?,freshly_accepted:false});
        }
        Self::validate_command(&command)?;
        if let Some(ledger) = self.read_ledger()? {
            for record in ledger.records {
                let previous = self.reconcile(record).await?;
                if !matches!(previous.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) { return Err("task_already_active".into()); }
            }
        }
        let (prepared,preset_snapshot) = self.runtime.prepare_configuration(&config).await?;
        if expected_preset.as_ref().is_some_and(|expected|expected!=&preset_snapshot || prepared!=config) {return Err("binding_invalid".into());}
        let config=prepared;
        let now_ms = nomifun_common::now_ms();
        let deadline_ms = now_ms + self.timeout_ms;
        let mut continuous_creation_key = None;
        if let Some(ledger) = self.read_ledger()? {
            for saved in ledger.records.iter().rev() {
                if saved.command.session_id == command.session_id {
                    let last_interaction = saved.last_receipt_at_ms
                        .or(saved.last_observed_at_ms)
                        .or(saved.send_started_at_ms)
                        .unwrap_or(0);
                    let idle = now_ms.saturating_sub(last_interaction);
                    if idle < DEFAULT_VOICE_TASK_TIMEOUT_MS && saved.config == config && saved.task.conversation_id.is_some() {
                        continuous_creation_key = Some(saved.creation_key.clone());
                    }
                    break;
                }
            }
        }
        let creation_key = continuous_creation_key.unwrap_or_else(|| {
            format!("voice-create:{}:{}", command.session_id, now_ms)
        });
        let mut record = DeliveryRecord {
            announcement_claimed:false,
            notification_id: None,
            attempt_id: None,
            last_attempt_at_ms: None,
            notification_status: None,
            replay_ids: Vec::new(),
            owner: self.owner.clone(),
            creation_key,
            send_key: format!("voice-send:{}",command.command_id),
            task: VoiceTaskSnapshot { command_id:command.command_id.clone(), session_id:command.session_id.clone(), acoustic_turn_id:command.turn_id.clone(), conversation_id:None,message_id:None,runtime_turn_id:None,status:VoiceTaskStatus::Delivering,result_ok:None,result_text:None,error_code:None },
            command, config, preset_snapshot,
            send_started_at_ms: Some(now_ms),
            deadline_at_ms: Some(deadline_ms),
            last_observed_at_ms: Some(now_ms),
            cancel_requested: false,
            last_receipt_at_ms: None,
            handoff_content,
        };
        // This is the required pre-side-effect boundary. Failure cannot create or send.
        self.save(&record)?;
        let remaining=std::time::Duration::from_millis(deadline_ms.saturating_sub(nomifun_common::now_ms()).max(1) as u64);
        let creation=tokio::time::timeout(remaining,self.runtime.create(&self.owner,&record.config,record.preset_snapshot.as_ref(),&record.creation_key)).await
            .unwrap_or_else(|_|{record.cancel_requested=true;Err(TaskMutationError::Uncertain("task_deadline_exceeded".into()))});
        let conversation = match creation {
            Ok(conversation) => conversation,
            Err(error) => {return Err(self.persist_mutation_failure(&mut record,error,"task_creation_unknown")?);},
        };
        if conversation.is_empty() { return Err("task_creation_unknown".into()); }
        record.task.conversation_id=Some(conversation.clone());
        self.save(&record)?;
        let remaining=std::time::Duration::from_millis(deadline_ms.saturating_sub(nomifun_common::now_ms()).max(1) as u64);
        let message_to_send = record.handoff_content.as_deref().unwrap_or(&record.command.goal);
        let sending=tokio::time::timeout(remaining,self.runtime.send(&self.owner,&conversation,&record.send_key,message_to_send)).await
            .unwrap_or_else(|_|{record.cancel_requested=true;Err(TaskMutationError::Uncertain("task_deadline_exceeded".into()))});
        let receipt = match sending {
            Ok(receipt) => receipt,
            Err(error) => {return Err(self.persist_mutation_failure(&mut record,error,"task_delivery_unknown")?);},
        };
        let fresh = !receipt.replayed && !receipt.completed;
        Self::apply_receipt(&mut record,receipt)?;
        record.last_receipt_at_ms=Some(nomifun_common::now_ms());
        self.save(&record)?;
        Ok(VoiceTaskDelivery {task:record.task, freshly_accepted:fresh})
    }

    pub async fn cancel(&self, command_id: &str) -> Result<VoiceTaskSnapshot, String> {
        let _guard=self.operation.lock().await;
        let mut record=self.read()?.ok_or("task_not_found")?;
        if record.task.command_id != command_id { return Err("task_scope_changed".into()); }
        if matches!(record.task.status, VoiceTaskStatus::Completed | VoiceTaskStatus::Failed) { return Ok(record.task); }
        record.cancel_requested = true;
        record.task.status=VoiceTaskStatus::Cancelling;
        self.save(&record)?;
        // A successful cancel call is not by itself a terminal receipt.
        self.reconcile(record).await
    }

    pub async fn confirmations(&self,command_id:&str)->Result<Vec<VoiceToolConfirmation>,String> {
        self.status_for(command_id).await?;
        let record=self.read_command(command_id)?.ok_or("task_not_found")?;
        if !matches!(record.task.status,VoiceTaskStatus::Accepted) {return Ok(Vec::new());}
        let conversation=record.task.conversation_id.ok_or("task_delivery_pending")?;
        self.runtime.confirmations(&self.owner,&conversation,&record.send_key,record.task.message_id.as_deref().ok_or("task_delivery_pending")?).await
    }

    pub async fn confirm(&self,approval_id:&str,request:&VoiceToolConfirmationRequest)->Result<(),String> {
        let _guard=self.operation.lock().await;
        let record=self.read_command(&request.command_id)?.ok_or("task_approval_changed")?;
        let task=self.reconcile(record.clone()).await?;
        if !request.task_reference().matches(&task) || task.status!=VoiceTaskStatus::Accepted {return Err("task_approval_changed".into());}
        let conversation=task.conversation_id.ok_or("task_delivery_pending")?;
        let message_id=task.message_id.as_deref().ok_or("task_delivery_pending")?;
        let pending=self.runtime.confirmations(&self.owner,&conversation,&record.send_key,message_id).await?;
        let confirmation=pending.into_iter().find(|pending|pending.approval_id==approval_id && pending.msg_id==request.msg_id && pending.call_id==request.call_id)
            .ok_or("task_approval_changed")?;
        if confirmation.expires_at_ms<=nomifun_common::now_ms(){return Err("task_approval_changed".into());}
        self.runtime.confirm(&self.owner,&conversation,&record.send_key,message_id,&confirmation,request.approved).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex as StdMutex, atomic::{AtomicUsize, Ordering}};

    #[derive(Default)]
    struct Runtime {
        created: AtomicUsize,
        sent: AtomicUsize,
        cancelled: AtomicUsize,
        observed: AtomicUsize,
        fail_observe: std::sync::atomic::AtomicBool,
        transport_failed:std::sync::atomic::AtomicBool,
        events:StdMutex<Option<tokio::sync::broadcast::Sender<nomifun_ai_agent::AgentStreamEvent>>>,
        receipt: StdMutex<Option<TaskReceipt>>,
        lose_send_response: std::sync::atomic::AtomicBool,
        lose_create_response: std::sync::atomic::AtomicBool,
        reject_create: std::sync::atomic::AtomicBool,
        block_send: std::sync::atomic::AtomicBool,
        send_started: tokio::sync::Notify,
        release_send: tokio::sync::Notify,
        state_path: StdMutex<Option<PathBuf>>,
        keys: StdMutex<Vec<String>>,
        confirmations: StdMutex<Vec<VoiceToolConfirmation>>,
        confirmed: StdMutex<Vec<(String, bool)>>,
    }
    fn receipt(completed: bool) -> TaskReceipt { TaskReceipt {message_id:"message-real".into(),runtime_turn_id:Some("runtime-turn-real".into()),replayed:false,completed,result_ok:completed.then_some(true),result_text:completed.then(|| "actual result".into()),error_code:None} }
    #[async_trait]
    impl VoiceTaskRuntime for Runtime {
        fn progress_events(&self,_:&str)->Option<tokio::sync::broadcast::Receiver<nomifun_ai_agent::AgentStreamEvent>> {self.events.lock().unwrap().as_ref().map(|tx|tx.subscribe())}
        fn transport_healthy(&self,_:&str)->Option<bool> {Some(!self.transport_failed.load(Ordering::SeqCst))}
        async fn confirmations(&self,_:&str,_:&str,_:&str,_:&str)->Result<Vec<VoiceToolConfirmation>,String>{
            Ok(self.confirmations.lock().unwrap().clone())
        }
        async fn confirm(&self,_:&str,_:&str,_:&str,_:&str,confirmation:&VoiceToolConfirmation,approved:bool)->Result<(),String>{
            let mut list = self.confirmations.lock().unwrap();
            if let Some(pos) = list.iter().position(|c| c.approval_id == confirmation.approval_id) {
                list.remove(pos);
            }
            self.confirmed.lock().unwrap().push((confirmation.approval_id.clone(), approved));
            Ok(())
        }
        async fn resolve_configuration(&self, config: &VoiceExecutionConfig) -> Result<VoiceExecutionConfig,String> { Ok(config.clone()) }
        async fn prepare_configuration(&self, config:&VoiceExecutionConfig) -> Result<(VoiceExecutionConfig,Option<ResolvedPresetSnapshot>),String> {Ok((config.clone(),None))}
        async fn create(&self, owner:&str, _: &VoiceExecutionConfig, _:Option<&ResolvedPresetSnapshot>, key:&str) -> Result<String,TaskMutationError> {
            assert_eq!(owner,"owner");
            let path=self.state_path.lock().unwrap().clone().unwrap();
            let mut saved:DeliveryLedger=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            let saved=saved.records.pop().unwrap();
            assert_eq!(saved.task.status,VoiceTaskStatus::Delivering);
            assert!(saved.task.conversation_id.is_none());
            if self.reject_create.load(Ordering::SeqCst) {return Err(TaskMutationError::Rejected("task_creation_rejected".into()));}
            self.created.fetch_add(1,Ordering::SeqCst);
            self.keys.lock().unwrap().push(key.into());
            if self.lose_create_response.load(Ordering::SeqCst) {return Err(TaskMutationError::Uncertain("response_lost".into()));}
            Ok("dedicated-conversation".into())
        }
        async fn find_created(&self,owner:&str,key:&str) -> Result<Option<String>,String> {
            assert_eq!(owner,"owner");
            Ok(self.keys.lock().unwrap().iter().any(|saved|saved==key).then(||"dedicated-conversation".into()))
        }
        async fn send(&self, owner:&str, conversation:&str, key:&str, goal:&str) -> Result<TaskReceipt,TaskMutationError> {
            assert_eq!((owner,conversation),("owner","dedicated-conversation"));
            assert!(goal == "inspect workspace" || goal.starts_with("来源：语音委派"), "unexpected goal/handoff: {goal}");
            let mut saved:DeliveryLedger=serde_json::from_slice(&std::fs::read(self.state_path.lock().unwrap().as_ref().unwrap()).unwrap()).unwrap();
            let saved=saved.records.pop().unwrap();
            assert_eq!(saved.task.conversation_id.as_deref(),Some(conversation));
            self.sent.fetch_add(1,Ordering::SeqCst);
            self.keys.lock().unwrap().push(key.into());
            let receipt=receipt(false);
            *self.receipt.lock().unwrap()=Some(receipt.clone());
            if self.block_send.load(Ordering::SeqCst) {
                self.send_started.notify_one();
                self.release_send.notified().await;
            }
            if self.lose_send_response.load(Ordering::SeqCst) { Err(TaskMutationError::Uncertain("response_lost".into())) } else { Ok(receipt) }
        }
        async fn observe(&self, _: &str, _: &str, _: &str) -> Result<Option<TaskReceipt>,String> {
            self.observed.fetch_add(1,Ordering::SeqCst);
            if self.fail_observe.load(Ordering::SeqCst) { return Err("runtime_offline".into()); }
            Ok(self.receipt.lock().unwrap().clone().map(|mut receipt| {receipt.replayed=true;receipt}))
        }
        async fn cancel(&self, _: &str, _: &str, key:&str,message_id:&str) -> Result<(),String> {
            assert!(key.starts_with("voice-send:"));assert_eq!(message_id,"message-real");
            self.cancelled.fetch_add(1,Ordering::SeqCst); Ok(())
        }
    }
    fn config() -> VoiceExecutionConfig { VoiceExecutionConfig {workspace:"configured-workspace".into(),model:None,preset_id:Some("preset".into()),session_mode:None} }
    fn command() -> VoiceTaskCommand { VoiceTaskCommand {command_id:"command-a".into(),session_id:"acoustic-session".into(),turn_id:"acoustic-turn".into(),goal:"inspect workspace".into(),record_id:None,binding_id:None} }
    fn service(path:PathBuf,runtime:Arc<Runtime>) -> VoiceTaskService {
        *runtime.state_path.lock().unwrap()=Some(path.clone());
        VoiceTaskService::new("owner".into(),path,runtime)
    }

    #[tokio::test]
    async fn replay_and_restart_observe_the_same_task_without_sending_or_reacknowledging() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());let path=dir.path().join("task.json");
        let first=service(path.clone(),runtime.clone()).submit(command(),config()).await.unwrap();
        assert!(first.freshly_accepted);assert_eq!(first.task.status,VoiceTaskStatus::Accepted);
        assert_eq!(first.task.acoustic_turn_id,"acoustic-turn");assert_eq!(first.task.runtime_turn_id.as_deref(),Some("runtime-turn-real"));
        let restarted=service(path,runtime.clone());
        let mut changed_config=config();changed_config.workspace="new-default-must-not-rebuild-existing-task".into();
        let replay=restarted.submit(command(),changed_config).await.unwrap();
        assert!(!replay.freshly_accepted);assert_eq!(replay.task,first.task);
        *runtime.receipt.lock().unwrap()=Some(receipt(true));
        let done=restarted.status().await.unwrap().unwrap();assert_eq!(done.status,VoiceTaskStatus::Completed);assert_eq!(done.result_text.as_deref(),Some("actual result"));
        assert_eq!(runtime.created.load(Ordering::SeqCst),1);assert_eq!(runtime.sent.load(Ordering::SeqCst),1);
        let keys = runtime.keys.lock().unwrap().clone();
        assert_eq!(keys.len(), 2);
        assert!(keys[0].starts_with("voice-create:acoustic-session:"));
        assert_eq!(keys[1], "voice-send:command-a");
    }

    #[tokio::test]
    async fn lost_send_response_recovers_by_receipt_without_a_second_execution() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());runtime.lose_send_response.store(true,Ordering::SeqCst);
        let path=dir.path().join("task.json");let first=service(path.clone(),runtime.clone());
        assert_eq!(first.submit(command(),config()).await.unwrap_err(),"task_delivery_unknown");
        let recovered=service(path,runtime.clone()).status().await.unwrap().unwrap();assert_eq!(recovered.status,VoiceTaskStatus::Accepted);
        assert_eq!(runtime.sent.load(Ordering::SeqCst),1);assert_eq!(runtime.created.load(Ordering::SeqCst),1);
    }

    #[tokio::test]
    async fn lost_creation_response_is_recovered_read_only_without_sending_a_new_task() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("task.json");
        let runtime=Arc::new(Runtime::default());runtime.lose_create_response.store(true,Ordering::SeqCst);
        let tasks=service(path.clone(),runtime.clone());
        assert_eq!(tasks.submit(command(),config()).await.unwrap_err(),"task_creation_unknown");
        let restored=service(path,runtime.clone());
        let task=restored.status().await.unwrap().unwrap();
        assert_eq!(task.conversation_id.as_deref(),Some("dedicated-conversation"));
        assert_eq!(task.status,VoiceTaskStatus::Unknown,"creation recovery cannot infer a send or acceptance");
        assert_eq!(runtime.created.load(Ordering::SeqCst),1);
        assert_eq!(runtime.sent.load(Ordering::SeqCst),0);
    }

    #[tokio::test]
    async fn a_definitively_rejected_creation_is_failed_without_claiming_acceptance() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());
        runtime.reject_create.store(true,Ordering::SeqCst);
        let tasks=service(dir.path().join("task.json"),runtime.clone());
        assert!(tasks.submit(command(),config()).await.is_err());
        let task=tasks.status().await.unwrap().unwrap();
        assert_eq!(task.status,VoiceTaskStatus::Failed);
        assert_eq!(task.result_ok,Some(false));assert!(task.message_id.is_none());
        assert_eq!(runtime.sent.load(Ordering::SeqCst),0);
    }

    #[tokio::test]
    async fn an_older_command_keeps_its_goal_validation_after_a_new_task_starts() {
        let dir=tempfile::tempdir().unwrap(); let runtime=Arc::new(Runtime::default());
        let tasks=service(dir.path().join("task.json"),runtime.clone());
        tasks.submit(command(),config()).await.unwrap();
        *runtime.receipt.lock().unwrap()=Some(receipt(true));
        let first=tasks.status().await.unwrap().unwrap();
        // Existing T1 installations may still have a single-record file.
        let legacy=tasks.read().unwrap().unwrap();
        std::fs::write(&tasks.path,serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut next=command();next.command_id="command-b".into();
        tasks.submit(next,config()).await.unwrap();
        assert_eq!(tasks.status_for("command-a").await.unwrap(),first,"new admission cannot erase the previous delivery identity");
        let mut changed=command();changed.goal="a different historical goal".into();
        assert_eq!(tasks.submit(changed,config()).await.unwrap_err(),"task_command_conflict");
        let mut changed=command();changed.goal=String::new();
        assert_eq!(tasks.submit(changed,config()).await.unwrap_err(),"task_command_conflict");
        let replay=tasks.submit(command(),config()).await.unwrap();
        assert_eq!(replay.task,first); assert!(!replay.freshly_accepted);
        assert_eq!(runtime.created.load(Ordering::SeqCst),2);
        assert_eq!(runtime.sent.load(Ordering::SeqCst),2);
    }

    #[tokio::test]
    async fn an_unannounced_result_survives_the_next_task_and_host_restart() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());let path=dir.path().join("task.json");
        let tasks=service(path.clone(),runtime.clone());
        tasks.submit(command(),config()).await.unwrap();
        *runtime.receipt.lock().unwrap()=Some(receipt(true));
        tasks.status().await.unwrap();
        let mut next=command();next.command_id="command-b".into();
        tasks.submit(next,config()).await.unwrap();
        let restored=service(path,runtime.clone());
        assert_eq!(restored.pending_result().unwrap().unwrap().0.command_id,"command-a");
        assert!(!restored.announcement_claimed("command-a").unwrap(),"a newer task cannot erase a pending result");
        assert!(restored.claim_announcement("command-a",true).await.unwrap());
        assert!(restored.announcement_claimed("command-a").unwrap());
        assert!(!restored.claim_announcement("command-a",true).await.unwrap());
        assert_eq!(restored.current_task().unwrap().unwrap().command_id,"command-b");
        assert_eq!(runtime.sent.load(Ordering::SeqCst),2);
    }

    #[tokio::test]
    async fn status_remains_readable_while_the_owning_send_is_in_flight() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());
        runtime.block_send.store(true,Ordering::SeqCst);
        let tasks=Arc::new(service(dir.path().join("task.json"),runtime.clone()));
        let submitting=tasks.clone();
        let delivery=tokio::spawn(async move {submitting.submit(command(),config()).await});
        tokio::time::timeout(std::time::Duration::from_secs(2),runtime.send_started.notified()).await.unwrap();
        let snapshot=tokio::time::timeout(std::time::Duration::from_millis(100),tasks.status()).await;
        runtime.release_send.notify_one();
        delivery.await.unwrap().unwrap();
        assert_eq!(snapshot.expect("status cannot wait for a remote send response").unwrap().unwrap().status,VoiceTaskStatus::Delivering);
        assert_eq!(runtime.sent.load(Ordering::SeqCst),1);
    }

    #[tokio::test]
    async fn a_terminal_receipt_without_an_outcome_is_not_reported_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        let tasks = service(dir.path().join("task.json"), runtime.clone());
        tasks.submit(command(), config()).await.unwrap();
        let mut incomplete = receipt(true);
        incomplete.result_ok = None;
        incomplete.result_text = None;
        *runtime.receipt.lock().unwrap() = Some(incomplete);
        let state = tasks.status().await.unwrap().unwrap();
        assert_eq!(state.status, VoiceTaskStatus::Unknown);
        assert_eq!(state.error_code.as_deref(), Some("task_result_unverified"));
        assert_eq!(runtime.sent.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn persistence_failure_prevents_conversation_creation_and_send() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());
        let parent=dir.path().join("file-not-directory");std::fs::write(&parent,"owned-test-file").unwrap();
        let tasks=service(parent.join("task.json"),runtime.clone());
        assert!(tasks.submit(command(),config()).await.is_err());
        assert_eq!(runtime.created.load(Ordering::SeqCst),0);assert_eq!(runtime.sent.load(Ordering::SeqCst),0);
        assert_eq!(std::fs::read_to_string(parent).unwrap(),"owned-test-file");
    }

    #[tokio::test]
    async fn cancelling_waits_for_a_terminal_receipt_and_stale_controls_cannot_retarget() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());let tasks=service(dir.path().join("task.json"),runtime.clone());
        tasks.submit(command(),config()).await.unwrap();
        assert_eq!(tasks.cancel("other-command").await.unwrap_err(),"task_scope_changed");
        assert_eq!(runtime.cancelled.load(Ordering::SeqCst),0);
        let cancelling=tasks.cancel("command-a").await.unwrap();assert_eq!(cancelling.status,VoiceTaskStatus::Cancelling);
        assert_eq!(tasks.status().await.unwrap().unwrap().status,VoiceTaskStatus::Cancelling);
        *runtime.receipt.lock().unwrap()=Some(receipt(true));assert_eq!(tasks.status().await.unwrap().unwrap().status,VoiceTaskStatus::Completed);
    }

    #[tokio::test]
    async fn a_different_owner_cannot_observe_or_cancel_the_saved_task() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());let path=dir.path().join("task.json");
        service(path.clone(),runtime.clone()).submit(command(),config()).await.unwrap();
        let other=VoiceTaskService::new("other-owner".into(),path,runtime.clone());
        assert!(other.status().await.unwrap().is_none());assert_eq!(other.cancel("command-a").await.unwrap_err(),"task_not_found");
        assert_eq!(runtime.observed.load(Ordering::SeqCst),0);assert_eq!(runtime.cancelled.load(Ordering::SeqCst),0);
    }

    #[tokio::test]
    async fn duplicate_identity_cannot_change_goal_and_a_second_active_task_is_refused() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());let tasks=service(dir.path().join("task.json"),runtime.clone());
        tasks.submit(command(),config()).await.unwrap();let mut changed=command();changed.goal="different goal".into();
        assert_eq!(tasks.submit(changed,config()).await.unwrap_err(),"task_command_conflict");
        let mut next=command();next.command_id="command-b".into();assert_eq!(tasks.submit(next,config()).await.unwrap_err(),"task_already_active");
        assert_eq!(runtime.sent.load(Ordering::SeqCst),1);
    }

    #[tokio::test]
    async fn bv_10_repeated_cancel_and_completion_race() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        let tasks = service(dir.path().join("task.json"), runtime.clone());
        tasks.submit(command(), config()).await.unwrap();

        // 1. Initial cancel puts task in Cancelling
        let cancelling = tasks.cancel("command-a").await.unwrap();
        assert_eq!(cancelling.status, VoiceTaskStatus::Cancelling);

        // 2. Repeated cancel is idempotent and stays in Cancelling
        let cancelling_again = tasks.cancel("command-a").await.unwrap();
        assert_eq!(cancelling_again.status, VoiceTaskStatus::Cancelling);

        // 3. Second active task submission while first is Cancelling is refused
        let mut second = command();
        second.command_id = "command-b".into();
        assert_eq!(tasks.submit(second, config()).await.unwrap_err(), "task_already_active");

        // 4. Natural completion race: runtime actually completes, status transitions to Completed
        *runtime.receipt.lock().unwrap() = Some(receipt(true));
        let finished = tasks.status().await.unwrap().unwrap();
        assert_eq!(finished.status, VoiceTaskStatus::Completed);
        assert_eq!(finished.result_ok, Some(true));
    }

    #[tokio::test]
    async fn a_stalled_send_cannot_hold_the_delivery_lock_past_its_saved_deadline() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());runtime.block_send.store(true,Ordering::SeqCst);
        let tasks=service(dir.path().join("task.json"),runtime.clone()).with_timeout_ms(500);
        assert_eq!(tasks.submit(command(),config()).await.unwrap_err(),"task_delivery_unknown");
        assert_eq!(tasks.status().await.unwrap().unwrap().status,VoiceTaskStatus::Cancelling);
        assert_eq!(runtime.sent.load(Ordering::SeqCst),1);assert_eq!(runtime.cancelled.load(Ordering::SeqCst),1);
        assert!(!tasks.replay(&command()).await.unwrap().unwrap().freshly_accepted);
        assert_eq!(runtime.sent.load(Ordering::SeqCst),1);
    }

    #[tokio::test]
    async fn progress_reports_real_events_and_separates_stalls_from_observation_failure() {
        let dir=tempfile::tempdir().unwrap();let runtime=Arc::new(Runtime::default());
        let (tx,_)=tokio::sync::broadcast::channel(16);*runtime.events.lock().unwrap()=Some(tx.clone());
        let tasks=service(dir.path().join("task.json"),runtime.clone());
        let task=tasks.submit(command(),config()).await.unwrap().task;
        let first=tasks.progress_for(&task).unwrap();assert_eq!(first.stage,"waiting");assert!(first.last_progress_at_ms.is_none());
        let mut record=tasks.read_command("command-a").unwrap().unwrap();record.send_started_at_ms=Some(nomifun_common::now_ms()-91_000);tasks.save(&record).unwrap();
        assert_eq!(tasks.progress_for(&task).unwrap().health,"no_recent_progress","liveness alone is not business progress");
        let event=serde_json::from_value(serde_json::json!({"type":"content","data":{"content":"实际运行产生的新结果"}})).unwrap();
        tx.send(event).unwrap();
        let progressing=tasks.progress_for(&task).unwrap();assert_eq!(progressing.stage,"answering");assert_eq!(progressing.health,"fresh");assert!(progressing.last_progress_at_ms.is_some());
        let mut record=tasks.read_command("command-a").unwrap().unwrap();record.last_receipt_at_ms=Some(nomifun_common::now_ms()-31_000);tasks.save(&record).unwrap();
        assert_eq!(tasks.progress_for(&task).unwrap().health,"unverified");
        runtime.transport_failed.store(true,Ordering::SeqCst);record.last_receipt_at_ms=Some(nomifun_common::now_ms());tasks.save(&record).unwrap();
        assert_eq!(tasks.progress_for(&task).unwrap().health,"unverified","a fresh saved receipt cannot prove a healthy transport");
        assert_eq!(runtime.sent.load(Ordering::SeqCst),1);
    }

    #[tokio::test]
    async fn notification_playback_outcome_and_replay_identity_survive_restart() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("task.json");let runtime=Arc::new(Runtime::default());
        let tasks=service(path.clone(),runtime.clone());tasks.submit(command(),config()).await.unwrap();
        *runtime.receipt.lock().unwrap()=Some(receipt(true));tasks.status().await.unwrap();
        assert_eq!(tasks.notification_for("command-a").unwrap().unwrap().status,"pending");
        tasks.claim_announcement("command-a",true).await.unwrap();
        let first=tasks.notification_for("command-a").unwrap().unwrap();assert_eq!(first.status,"dispatched");
        assert_eq!(tasks.request_replay("command-a","new-replay").await.unwrap_err(),"voice_busy");
        tasks.finish_announcement("command-a",&first.attempt_id,"failed").await.unwrap();
        let restored=service(path,runtime.clone());assert_eq!(restored.notification_for("command-a").unwrap().unwrap().status,"failed");
        assert_eq!(restored.status_for("command-a").await.unwrap().status,VoiceTaskStatus::Completed);
        assert!(restored.request_replay("command-a","new-replay").await.unwrap());
        restored.claim_announcement("command-a",true).await.unwrap();let next=restored.notification_for("command-a").unwrap().unwrap();
        assert_ne!(first.attempt_id,next.attempt_id);assert_eq!(first.notification_id,next.notification_id);
        assert!(!restored.request_replay("command-a","new-replay").await.unwrap());
        assert_eq!(restored.finish_announcement("command-a",&first.attempt_id,"played").await.unwrap_err(),"task_scope_changed");
        assert_eq!(runtime.sent.load(Ordering::SeqCst),1);
    }

    #[tokio::test]
    async fn deadline_and_cancel_intent_survive_an_unreadable_runtime_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("task.json");
        let runtime = Arc::new(Runtime::default());
        let tasks = service(path.clone(), runtime.clone()).with_timeout_ms(500);
        tasks.submit(command(), config()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(510)).await;
        runtime.fail_observe.store(true, Ordering::SeqCst);
        let expired = tasks.status().await.unwrap().unwrap();
        assert_eq!(expired.status, VoiceTaskStatus::Cancelling);
        assert_eq!(expired.error_code.as_deref(), Some("task_deadline_exceeded"));
        assert_eq!(runtime.cancelled.load(Ordering::SeqCst), 1);
        let restored = service(path, runtime.clone());
        assert_eq!(restored.status().await.unwrap().unwrap().status, VoiceTaskStatus::Cancelling);
        assert_eq!(runtime.cancelled.load(Ordering::SeqCst), 2);
        assert_eq!(runtime.sent.load(Ordering::SeqCst), 1);
        runtime.fail_observe.store(false, Ordering::SeqCst);
        *runtime.receipt.lock().unwrap() = Some(receipt(true));
        assert_eq!(restored.status().await.unwrap().unwrap().status, VoiceTaskStatus::Completed);
    }

    #[tokio::test]
    async fn bv_11_deadline_expiration_and_clock_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        // Use a short deadline with enough headroom for Windows atomic file writes.
        let tasks = service(dir.path().join("task.json"), runtime.clone()).with_timeout_ms(500);
        tasks.submit(command(), config()).await.unwrap();

        // Initially status is Accepted
        let initial = tasks.status().await.unwrap().unwrap();
        assert_eq!(initial.status, VoiceTaskStatus::Accepted);

        // Wait past the saved 500ms deadline
        tokio::time::sleep(std::time::Duration::from_millis(510)).await;

        // reconcile detects deadline expiration: triggers cancellation and marks Cancelling
        let expired = tasks.status().await.unwrap().unwrap();
        assert_eq!(expired.status, VoiceTaskStatus::Cancelling);
        assert_eq!(expired.error_code.as_deref(), Some("task_deadline_exceeded"));
        assert_eq!(runtime.cancelled.load(Ordering::SeqCst), 1);

        // Test clock rollback: create a new task with normal timeout, artificially set last_observed_at_ms into future
        let dir2 = tempfile::tempdir().unwrap();
        let runtime2 = Arc::new(Runtime::default());
        let tasks2 = service(dir2.path().join("task.json"), runtime2.clone());
        tasks2.submit(command(), config()).await.unwrap();

        // Mutate saved record's last_observed_at_ms to future (+100_000ms) to simulate severe clock rollback
        let mut record = tasks2.read_command("command-a").unwrap().unwrap();
        record.last_observed_at_ms = Some(nomifun_common::now_ms() + 100_000);
        tasks2.save(&record).unwrap();

        // reconcile detects clock rollback: triggers cancellation and marks Cancelling
        let rollback_task = tasks2.status().await.unwrap().unwrap();
        assert_eq!(rollback_task.status, VoiceTaskStatus::Cancelling);
        assert_eq!(rollback_task.error_code.as_deref(), Some("task_clock_rollback_detected"));
        assert_eq!(runtime2.cancelled.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn bv_14_saved_result_relisten_without_reexecution_and_notification_failure_preserves_text() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        let tasks = service(dir.path().join("task.json"), runtime.clone());
        tasks.submit(command(), config()).await.unwrap();

        // 1. Task completes with durable result text
        let result_payload = "关键读取产物内容-01a144b2-real-payload";
        *runtime.receipt.lock().unwrap() = Some(TaskReceipt {
            message_id: "message-real".into(),
            runtime_turn_id: Some("turn-real".into()),
            replayed: false,
            completed: true,
            result_ok: Some(true),
            result_text: Some(result_payload.into()),
            error_code: None,
        });

        let completed = tasks.status().await.unwrap().unwrap();
        assert_eq!(completed.status, VoiceTaskStatus::Completed);
        assert_eq!(completed.result_text.as_deref(), Some(result_payload));
        assert_eq!(runtime.sent.load(Ordering::SeqCst), 1);

        // 2. First notification claim assigns stable notification_id and first attempt_id
        assert!(tasks.claim_announcement("command-a", true).await.unwrap());
        let notif1 = tasks.notification_for("command-a").unwrap().unwrap();
        assert_eq!(notif1.command_id, "command-a");
        assert_eq!(notif1.notification_id, "voice-notif:command-a");
        assert_eq!(notif1.attempt_id, "voice-attempt:command-a:1");
        assert_eq!(notif1.result_summary, result_payload);
        assert_eq!(notif1.status, "dispatched", "execution completion is independent of playback");

        // 3. Notification dispatch fails or response lost: unclaim to allow retry
        // Text and completed status are fully preserved ("失败保字")
        assert!(tasks.claim_announcement("command-a", false).await.unwrap());
        let observed = tasks.status_for("command-a").await.unwrap();
        assert_eq!(observed.status, VoiceTaskStatus::Completed);
        assert_eq!(observed.result_text.as_deref(), Some(result_payload), "failure cannot erase result text");
        assert_eq!(runtime.sent.load(Ordering::SeqCst), 1, "failed notification cannot re-execute task");

        // 4. Retry / Re-listen attempt advances attempt_id while keeping the same notification_id
        assert!(tasks.claim_announcement("command-a", true).await.unwrap());
        let notif2 = tasks.notification_for("command-a").unwrap().unwrap();
        assert_eq!(notif2.notification_id, notif1.notification_id, "logic notification_id must be stable");
        assert_eq!(notif2.attempt_id, "voice-attempt:command-a:2", "attempt_id must be independent and advanced");
        assert_eq!(notif2.result_summary, result_payload);

        // 5. Query status / Re-listen returns the exact same result without executing model or tool
        let relisten = tasks.status_for("command-a").await.unwrap();
        assert_eq!(relisten.status, VoiceTaskStatus::Completed);
        assert_eq!(relisten.result_text.as_deref(), Some(result_payload));
        assert_eq!(runtime.sent.load(Ordering::SeqCst), 1, "re-listen must be zero new model or tool executions");
        assert_eq!(runtime.created.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn bv_15_tool_approval_expired_rejected_and_cannot_be_bypassed() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        let tasks = service(dir.path().join("task.json"), runtime.clone());
        let delivery = tasks.submit(command(), config()).await.unwrap();
        assert_eq!(delivery.task.status, VoiceTaskStatus::Accepted);
        assert_eq!(delivery.task.conversation_id.as_deref(), Some("dedicated-conversation"));
        assert_eq!(delivery.task.message_id.as_deref(), Some("message-real"));

        let now_ms = nomifun_common::now_ms();
        let approval = VoiceToolConfirmation {
            approval_id: "app-bv15".into(),
            msg_id: "msg-bv15".into(),
            call_id: "call-bv15".into(),
            tool_name: "Bash".into(),
            arguments: serde_json::json!({"command": "Remove-Item -LiteralPath 'acceptance.txt'"}),
            expires_at_ms: now_ms + 60_000,
        };

        // 1. Expired approval cannot be authorized
        let expired_approval = VoiceToolConfirmation {
            expires_at_ms: now_ms - 1000,
            ..approval.clone()
        };
        *runtime.confirmations.lock().unwrap() = vec![expired_approval];
        let req = VoiceToolConfirmationRequest {
            instance_id: "inst-1".into(),
            command_id: "command-a".into(),
            conversation_id: Some("dedicated-conversation".into()),
            message_id: Some("message-real".into()),
            msg_id: "msg-bv15".into(),
            call_id: "call-bv15".into(),
            approved: true,
        };
        let err = tasks.confirm("app-bv15", &req).await.unwrap_err();
        assert_eq!(err, "task_approval_changed", "expired approval must be rejected");
        assert!(runtime.confirmed.lock().unwrap().is_empty());

        // 2. Mismatched msg_id or call_id is rejected
        *runtime.confirmations.lock().unwrap() = vec![approval.clone()];
        let mut mismatch_msg = req.clone();
        mismatch_msg.msg_id = "stale-msg".into();
        assert_eq!(tasks.confirm("app-bv15", &mismatch_msg).await.unwrap_err(), "task_approval_changed");
        let mut mismatch_call = req.clone();
        mismatch_call.call_id = "stale-call".into();
        assert_eq!(tasks.confirm("app-bv15", &mismatch_call).await.unwrap_err(), "task_approval_changed");
        assert!(runtime.confirmed.lock().unwrap().is_empty());

        // 3. Mismatched command/task scope is rejected
        let mut mismatch_cmd = req.clone();
        mismatch_cmd.command_id = "other-command".into();
        assert_eq!(tasks.confirm("app-bv15", &mismatch_cmd).await.unwrap_err(), "task_approval_changed");
        assert!(runtime.confirmed.lock().unwrap().is_empty());

        // 4. Extra fields like 'always_allow' or 'yolo' cannot bypass confirmation request deserialization
        let yolo_json = serde_json::json!({
            "instance_id": "inst-1",
            "command_id": "command-a",
            "conversation_id": "dedicated-conversation",
            "message_id": "message-real",
            "msg_id": "msg-bv15",
            "call_id": "call-bv15",
            "approved": true,
            "always_allow": true
        });
        assert!(
            serde_json::from_value::<VoiceToolConfirmationRequest>(yolo_json).is_err(),
            "deny_unknown_fields must reject always_allow or yolo bypass attempts"
        );

        // 5. Rejection (approved: false) is forwarded without executing dangerous tool
        let mut reject_req = req.clone();
        reject_req.approved = false;
        tasks.confirm("app-bv15", &reject_req).await.unwrap();
        assert_eq!(*runtime.confirmed.lock().unwrap(), vec![("app-bv15".to_string(), false)]);

        // 6. Single-use consumption: duplicate approval fails immediately
        let err = tasks.confirm("app-bv15", &req).await.unwrap_err();
        assert_eq!(err, "task_approval_changed", "confirmation is single-use and cannot be re-approved");

        // 7. Non-active task (completed) rejects any subsequent approvals
        *runtime.confirmations.lock().unwrap() = vec![approval.clone()];
        *runtime.receipt.lock().unwrap() = Some(receipt(true));
        let _ = tasks.status().await.unwrap(); // transitions to Completed
        assert_eq!(
            tasks.confirm("app-bv15", &req).await.unwrap_err(),
            "task_approval_changed",
            "completed task cannot accept tool approvals"
        );
    }

    #[tokio::test]
    async fn handoff_content_is_forwarded_and_persisted_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        let path = dir.path().join("task.json");
        let tasks = service(path.clone(), runtime.clone());
        let handoff = "来源：语音委派\n用户原文：查下北京天气\n任务目标：inspect workspace";
        let delivery = tasks
            .submit_frozen_with_handoff(command(), config(), None, Some(handoff.to_string()))
            .await
            .unwrap();
        assert!(delivery.freshly_accepted);
        let restored = service(path, runtime.clone());
        let record = restored.read_command("command-a").unwrap().unwrap();
        assert_eq!(record.handoff_content.as_deref(), Some(handoff));
    }

    #[tokio::test]
    async fn continuous_session_reuses_creation_key_within_idle_window_and_rotates_after_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        let path = dir.path().join("task.json");
        let tasks = service(path.clone(), runtime.clone());

        let mut cmd1 = command();
        cmd1.command_id = "cmd-turn-1".into();
        cmd1.session_id = "continuous-sess".into();
        tasks.submit(cmd1.clone(), config()).await.unwrap();
        *runtime.receipt.lock().unwrap() = Some(receipt(true));
        tasks.status().await.unwrap();

        let keys_after_1 = runtime.keys.lock().unwrap().clone();
        assert_eq!(keys_after_1.len(), 2);
        let creation_key_1 = keys_after_1[0].clone();
        assert!(creation_key_1.starts_with("voice-create:continuous-sess:"));

        // 2nd turn in same session within 30 min reuses creation_key
        let mut cmd2 = command();
        cmd2.command_id = "cmd-turn-2".into();
        cmd2.session_id = "continuous-sess".into();
        tasks.submit(cmd2.clone(), config()).await.unwrap();
        *runtime.receipt.lock().unwrap() = Some(receipt(true));
        tasks.status().await.unwrap();

        let keys_after_2 = runtime.keys.lock().unwrap().clone();
        assert_eq!(keys_after_2.len(), 4);
        assert_eq!(keys_after_2[2], creation_key_1, "must reuse continuous creation key");
        assert_eq!(keys_after_2[3], "voice-send:cmd-turn-2");

        // Simulate 31 minutes idle on the latest record
        let mut ledger = tasks.read_ledger().unwrap().unwrap();
        for rec in &mut ledger.records {
            let expired = rec.last_observed_at_ms.unwrap_or(0) - (31 * 60 * 1000);
            rec.last_receipt_at_ms = Some(expired);
            rec.last_observed_at_ms = Some(expired);
            rec.send_started_at_ms = Some(expired);
        }
        std::fs::write(&tasks.path, serde_json::to_vec(&ledger).unwrap()).unwrap();

        // 3rd turn after 31 min idle must rotate to a new creation_key (new epoch)
        let mut cmd3 = command();
        cmd3.command_id = "cmd-turn-3".into();
        cmd3.session_id = "continuous-sess".into();
        tasks.submit(cmd3.clone(), config()).await.unwrap();
        *runtime.receipt.lock().unwrap() = Some(receipt(true));
        tasks.status().await.unwrap();

        let keys_after_3 = runtime.keys.lock().unwrap().clone();
        assert_eq!(keys_after_3.len(), 6);
        let creation_key_3 = keys_after_3[4].clone();
        assert_ne!(creation_key_3, creation_key_1, "must rotate to new creation key after 30m idle");
        assert!(creation_key_3.starts_with("voice-create:continuous-sess:"));
        assert_eq!(keys_after_3[5], "voice-send:cmd-turn-3");
    }

    #[tokio::test]
    async fn goal_failure_in_result_text_is_not_reported_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(Runtime::default());
        let tasks = service(dir.path().join("task.json"), runtime.clone());
        tasks.submit(command(), config()).await.unwrap();

        let mut failed_turn = receipt(true);
        failed_turn.result_ok = Some(true); // Runtime turn succeeded
        failed_turn.result_text = Some("很抱歉，未能获取到北京的天气信息。".into()); // But goal was unfulfilled
        *runtime.receipt.lock().unwrap() = Some(failed_turn);

        let state = tasks.status().await.unwrap().unwrap();
        assert_eq!(state.status, VoiceTaskStatus::Failed, "must be failed when goal is unfulfilled");
        assert_eq!(state.result_ok, Some(false));
        assert_eq!(state.error_code.as_deref(), Some("goal_unfulfilled"));
    }
}

