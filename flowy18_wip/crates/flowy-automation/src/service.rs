//! The single entry point used by the model tools (create and manage) and the
//! automation page (view, edit, pause, delete).

use std::sync::Arc;

use crate::model::{
    Action, Automation, AutomationId, AutomationInput, Run, RunId, now_ms,
};
use crate::schedule::ScheduleError;
use crate::store::{AutomationStore, StoreError};
use crate::worker::Worker;

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    #[error("automation not found")]
    NotFound,
    #[error("{0}")]
    Invalid(&'static str),
}

pub struct AutomationService {
    store: Arc<AutomationStore>,
    worker: Worker,
}

impl std::fmt::Debug for AutomationService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AutomationService")
    }
}

impl AutomationService {
    pub const fn new(store: Arc<AutomationStore>, worker: Worker) -> Self {
        Self { store, worker }
    }

    /// The worker only runs automations while the user is signed in. Signing
    /// out cancels runs in progress.
    pub fn set_signed_in(&self, signed_in: bool) {
        if signed_in {
            self.worker.resume();
        } else {
            self.worker.pause();
        }
    }

    /// Only the agent creates automations, from inside `conversation_id`.
    pub fn create(
        &self,
        conversation_id: &str,
        input: AutomationInput,
    ) -> Result<Automation, ServiceError> {
        validate(&input)?;
        if conversation_id.trim().is_empty() {
            return Err(ServiceError::Invalid("conversation is missing"));
        }
        let now = now_ms();
        validate_trigger(input.trigger, now)?;
        let automation = Automation {
            id: AutomationId::generate(),
            conversation_id: conversation_id.to_owned(),
            title: input.title.trim().to_owned(),
            action: input.action,
            trigger: input.trigger,
            access: input.access,
            enabled: true,
            next_run_at_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.save(automation, now)
    }

    pub fn update(
        &self,
        id: AutomationId,
        input: AutomationInput,
    ) -> Result<Automation, ServiceError> {
        validate(&input)?;
        let now = now_ms();
        validate_trigger(input.trigger, now)?;
        let automation = Automation {
            title: input.title.trim().to_owned(),
            action: input.action,
            trigger: input.trigger,
            access: input.access,
            ..self.get(id)?
        };
        self.save(automation, now)
    }

    pub fn pause(&self, id: AutomationId) -> Result<Automation, ServiceError> {
        self.set_enabled(id, false)
    }

    /// Missed occurrences while paused are not caught up.
    pub fn resume(&self, id: AutomationId) -> Result<Automation, ServiceError> {
        self.set_enabled(id, true)
    }

    fn set_enabled(
        &self,
        id: AutomationId,
        enabled: bool,
    ) -> Result<Automation, ServiceError> {
        let automation = Automation {
            enabled,
            ..self.get(id)?
        };
        self.save(automation, now_ms())
    }

    fn save(
        &self,
        mut automation: Automation,
        now: i64,
    ) -> Result<Automation, ServiceError> {
        automation.next_run_at_ms = if automation.enabled {
            automation.trigger.next_after(now)?
        } else {
            None
        };
        automation.updated_at_ms = now;
        self.store.save(&automation)?;
        self.worker.wake();
        Ok(automation)
    }

    pub fn delete(&self, id: AutomationId) -> Result<(), ServiceError> {
        if self.store.delete(id)? {
            self.worker.cancel_automation(id);
            Ok(())
        } else {
            Err(ServiceError::NotFound)
        }
    }

    /// Removes every automation of a deleted conversation.
    pub fn delete_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<(), ServiceError> {
        for automation in self.list_for(conversation_id)? {
            self.delete(automation.id)?;
        }
        Ok(())
    }

    /// Returns false if the run had already ended.
    pub fn cancel(&self, run_id: RunId) -> bool {
        self.worker.cancel(run_id)
    }

    pub fn list(&self) -> Result<Vec<Automation>, ServiceError> {
        Ok(self.store.list()?)
    }

    /// Automations created in `conversation_id`.
    pub fn list_for(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<Automation>, ServiceError> {
        Ok(self.store.list_for(conversation_id)?)
    }

    pub fn get(&self, id: AutomationId) -> Result<Automation, ServiceError> {
        self.store.get(id)?.ok_or(ServiceError::NotFound)
    }

    /// Newest first.
    pub fn history(
        &self,
        id: AutomationId,
        limit: usize,
    ) -> Result<Vec<Run>, ServiceError> {
        Ok(self.store.runs_for(id, limit)?)
    }

    pub fn run(&self, run_id: RunId) -> Result<Option<Run>, ServiceError> {
        Ok(self.store.run(run_id)?)
    }
}

fn validate_trigger(
    trigger: crate::schedule::TriggerSpec,
    now: i64,
) -> Result<(), ServiceError> {
    if trigger.next_after(now)?.is_none() {
        return Err(ServiceError::Invalid("schedule has no future occurrence"));
    }
    Ok(())
}

fn validate(input: &AutomationInput) -> Result<(), ServiceError> {
    let text = match &input.action {
        Action::AgentTask { prompt } => prompt,
        Action::Reminder { text } => text,
    };
    if input.title.trim().is_empty() {
        return Err(ServiceError::Invalid("title is empty"));
    }
    if text.trim().is_empty() {
        return Err(ServiceError::Invalid("prompt or reminder text is empty"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
