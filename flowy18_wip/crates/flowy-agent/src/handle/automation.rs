//! Automations live inside the conversation that created them.
//!
//! - Only a read-write Worker the user dispatched in a conversation gets the
//!   automation tools; it creates automations owned by that conversation.
//! - A due `AgentTask` dispatches a new Worker under the conversation's Router,
//!   exactly like `dispatch_work`. When it ends, the Router's usual
//!   `[WorkEventBatch]` announcement tells the user in that conversation.
//! - A due `Reminder` is saved as a message in the conversation.
//!
//! - `automation_run_now` starts the same kind of run immediately (a test
//!   run). It reports in the conversation like a scheduled run and leaves
//!   the schedule unchanged.
//!
//! No session is created outside the conversation.

use std::sync::{Arc, Weak};
use std::time::Duration;

use flowy_agent_core::{SessionRecord, SessionRole, WorkAccessMode};
use flowy_automation::{
    Access, Action, Automation, AutomationService, CancelToken as RunCancel,
    RunExecutor, RunFuture, RunId, RunOutcome, RunReport,
};

use super::observer::persist_index;
use super::router::DispatchWorkRequest;
use super::{AgentHandle, ChatLine, Harness, lock};

const POLL: Duration = Duration::from_millis(500);
/// How long a cancelled Worker may take to stop before the run is closed.
const CANCEL_SETTLE: Duration = Duration::from_secs(30);
/// Dispatch `request_id` prefix that marks Workers started by a run.
const RUN_REQUEST_PREFIX: &str = "automation-run-";

impl AgentHandle {
    /// Installs the service behind the automation tools. `None` removes the
    /// tools from later turns.
    pub fn set_automation_service(
        &self,
        service: Option<Arc<AutomationService>>,
    ) {
        *lock(&self.inner.state.automation) = service;
    }

    #[must_use]
    pub fn automation_executor(&self) -> Arc<dyn RunExecutor> {
        Arc::new(AgentRunExecutor {
            harness: Arc::downgrade(&self.inner),
        })
    }

    /// The agent's runtime, for the automation worker.
    #[must_use]
    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.inner.runtime.clone()
    }
}

impl Harness {
    /// The service and owning conversation for this turn's automation tools.
    /// Only read-write Workers in a conversation qualify; Workers started by
    /// a run never do, so an automation cannot create automations.
    pub(super) fn automation_tools_for(
        &self,
        session: &SessionRecord,
    ) -> Option<(Arc<AutomationService>, String)> {
        if session.role != SessionRole::Worker
            || session.access_mode != WorkAccessMode::ReadWrite
        {
            return None;
        }
        let conversation = session.root_session_id.clone()?;
        if self.started_by_run(&conversation, &session.id) {
            return None;
        }
        let service = lock(&self.state.automation).clone()?;
        Some((service, conversation))
    }

    fn started_by_run(&self, conversation: &str, worker_id: &str) -> bool {
        let engine = lock(&self.engine).clone();
        engine
            .and_then(|engine| {
                engine.with_session(conversation, |router| {
                    router.dispatch_requests.values().any(|record| {
                        record.worker_id == worker_id
                            && record.request_id.starts_with(RUN_REQUEST_PREFIX)
                    })
                })
            })
            .unwrap_or(false)
    }

    /// Dispatches the run's Worker in the owning conversation.
    fn dispatch_run(
        self: &Arc<Self>,
        automation: &Automation,
        run_id: RunId,
        prompt: &str,
    ) -> Result<String, &'static str> {
        let mode = match automation.access {
            Access::Read => WorkAccessMode::ReadOnly,
            Access::Write => WorkAccessMode::ReadWrite,
        };
        self.dispatch_work_from_router(
            &automation.conversation_id,
            DispatchWorkRequest {
                brief: format!("[Automation: {}] {prompt}", automation.title),
                requested_mode: Some(mode),
                target_work_id: None,
                target_object_ids: Vec::new(),
                request_id: Some(format!("{RUN_REQUEST_PREFIX}{run_id}")),
            },
        )
        .map(|result| result.work_id)
    }

    /// This conversation's automations as the host stored them, each with
    /// `next_run_local` and its most recent run, for `query_work_status`.
    /// Empty when automations are unavailable.
    pub(super) fn conversation_automations(
        &self,
        conversation: &str,
    ) -> Vec<serde_json::Value> {
        let Some(service) = lock(&self.state.automation).clone() else {
            return Vec::new();
        };
        service
            .list_for(conversation)
            .unwrap_or_default()
            .iter()
            .map(|automation| {
                let mut value =
                    crate::automation_tools::automation_to_json(automation);
                let last_run = service
                    .history(automation.id, 1)
                    .ok()
                    .and_then(|runs| runs.into_iter().next());
                if let Some(object) = value.as_object_mut() {
                    object.insert(
                        "last_run".to_owned(),
                        serde_json::to_value(last_run)
                            .unwrap_or(serde_json::Value::Null),
                    );
                }
                value
            })
            .collect()
    }

    fn work_state(&self, conversation: &str, work_id: &str) -> Option<String> {
        self.child_works(conversation)
            .into_iter()
            .find(|work| work.work_id == work_id)
            .map(|work| work.state)
    }

    /// Saves the fixed reminder in the conversation without a model turn.
    fn post_reminder(
        &self,
        automation: &Automation,
        text: &str,
    ) -> Result<(), &'static str> {
        let conversation = &automation.conversation_id;
        let line = ChatLine::Assistant {
            text: format!("**{}**\n\n{text}", automation.title),
        };
        lock(&self.state.live)
            .get_mut(conversation)
            .filter(|session| session.role == SessionRole::Router)
            .ok_or("ROUTER_NOT_FOUND")?
            .projection
            .lines
            .push(line);
        persist_index(&self.state)?;
        self.state.stream_changed(conversation);
        Ok(())
    }
}

struct AgentRunExecutor {
    /// Weak so the worker never keeps the agent (and its runtime) alive.
    harness: Weak<Harness>,
}

impl RunExecutor for AgentRunExecutor {
    fn is_ready(&self, automation: &Automation) -> bool {
        self.harness.upgrade().is_some_and(|harness| {
            let engine_ready = lock(&harness.engine).is_some();
            engine_ready
                && (matches!(automation.action, Action::Reminder { .. })
                    || harness.can_run_turns())
        })
    }

    fn run<'a>(
        &'a self,
        automation: &'a Automation,
        run_id: RunId,
        cancel: RunCancel,
    ) -> RunFuture<'a> {
        Box::pin(async move {
            match &automation.action {
                Action::Reminder { text } => {
                    self.remind(automation, text, &cancel).await
                }
                Action::AgentTask { prompt } => {
                    self.dispatch(automation, run_id, prompt, &cancel).await
                }
            }
        })
    }
}

impl AgentRunExecutor {
    /// Host calls take blocking locks and do file I/O, so they stay off the
    /// async workers.
    async fn blocking<T: Send + 'static>(
        &self,
        call: impl FnOnce(&Arc<Harness>) -> T + Send + 'static,
    ) -> Option<T> {
        let harness = self.harness.clone();
        tokio::task::spawn_blocking(move || {
            harness.upgrade().map(|harness| call(&harness))
        })
        .await
        .ok()
        .flatten()
    }

    async fn remind(
        &self,
        automation: &Automation,
        text: &str,
        cancel: &RunCancel,
    ) -> RunReport {
        let (automation, text, cancel) =
            (automation.clone(), text.to_owned(), cancel.clone());
        let outcome = self
            .blocking(move |harness| {
                if cancel.is_cancelled() {
                    return RunOutcome::Cancelled;
                }
                match harness.post_reminder(&automation, &text) {
                    Ok(()) => RunOutcome::Succeeded,
                    Err(key) => failed(key),
                }
            })
            .await
            .unwrap_or_else(|| failed("agent unavailable"));
        report(None, outcome)
    }

    async fn dispatch(
        &self,
        automation: &Automation,
        run_id: RunId,
        prompt: &str,
        cancel: &RunCancel,
    ) -> RunReport {
        let (owned, prompt) = (automation.clone(), prompt.to_owned());
        let dispatch_cancel = cancel.clone();
        let work_id = match self
            .blocking(move |harness| {
                (!dispatch_cancel.is_cancelled())
                    .then(|| harness.dispatch_run(&owned, run_id, &prompt))
            })
            .await
        {
            None => return report(None, failed("agent unavailable")),
            Some(None) => return report(None, RunOutcome::Cancelled),
            Some(Some(Err(code))) => return report(None, failed(code)),
            Some(Some(Ok(work_id))) => work_id,
        };
        let outcome = self
            .wait(&automation.conversation_id, &work_id, cancel)
            .await;
        report(Some(work_id), outcome)
    }

    async fn wait(
        &self,
        conversation: &str,
        work_id: &str,
        cancel: &RunCancel,
    ) -> RunOutcome {
        let mut cancel_deadline = None;
        loop {
            let (owner, work) = (conversation.to_owned(), work_id.to_owned());
            let Some(state) = self
                .blocking(move |harness| harness.work_state(&owner, &work))
                .await
            else {
                return failed("agent unavailable");
            };
            let Some(state) = state else {
                return failed("the run's work was removed");
            };
            if let Some(outcome) = settled(&state) {
                return outcome;
            }
            if let Some(deadline) = cancel_deadline {
                if tokio::time::Instant::now() >= deadline {
                    return RunOutcome::Cancelled;
                }
                tokio::time::sleep(POLL).await;
                continue;
            }
            tokio::select! {
                () = cancel.cancelled() => {
                    cancel_deadline =
                        Some(tokio::time::Instant::now() + CANCEL_SETTLE);
                    let (owner, work) =
                        (conversation.to_owned(), work_id.to_owned());
                    let _ = self
                        .blocking(move |harness| harness.cancel_work(&owner, &work))
                        .await;
                }
                () = tokio::time::sleep(POLL) => {}
            }
        }
    }
}

/// Maps a child Work state to the run outcome, or `None` while it is still
/// queued or running.
fn settled(state: &str) -> Option<RunOutcome> {
    Some(match state {
        "completed" | "delivered" | "partial" => RunOutcome::Succeeded,
        "failed" => failed("the work failed"),
        "stopped" => RunOutcome::Cancelled,

        _ => return None,
    })
}

fn failed(message: &str) -> RunOutcome {
    RunOutcome::Failed(message.to_owned())
}

const fn report(work_id: Option<String>, outcome: RunOutcome) -> RunReport {
    RunReport { work_id, outcome }
}

#[cfg(test)]
mod tests;
