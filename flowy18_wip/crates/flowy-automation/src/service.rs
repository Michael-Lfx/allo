//! The single entry point used by both the automation page and the model
//! tools.

use std::sync::Arc;

use crate::model::{
    Action, Automation, AutomationId, AutomationInput, Run, RunId, RunTrigger,
    now_ms,
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

    pub fn create(
        &self,
        input: AutomationInput,
    ) -> Result<Automation, ServiceError> {
        validate(&input)?;
        let now = now_ms();
        let automation = Automation {
            id: AutomationId::generate(),
            title: input.title.trim().to_owned(),
            action: input.action,
            trigger: input.trigger,
            cwd: input.cwd,
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
        let automation = Automation {
            title: input.title.trim().to_owned(),
            action: input.action,
            trigger: input.trigger,
            cwd: input.cwd,
            access: input.access,
            ..self.get(id)?
        };
        self.save(automation, now_ms())
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
        self.worker.cancel_automation(id);
        if self.store.delete(id)? {
            Ok(())
        } else {
            Err(ServiceError::NotFound)
        }
    }

    pub fn run_now(&self, id: AutomationId) -> Result<Run, ServiceError> {
        self.get(id)?;
        let run = Run::queued(id, RunTrigger::Manual, now_ms());
        self.store.save_run(&run)?;
        self.worker.wake();
        Ok(run)
    }

    /// Returns false if the run had already ended.
    pub fn cancel(&self, run_id: RunId) -> Result<bool, ServiceError> {
        Ok(self.worker.cancel(run_id)?)
    }

    pub fn list(&self) -> Result<Vec<Automation>, ServiceError> {
        Ok(self.store.list()?)
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
