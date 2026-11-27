//! Automations live inside the conversation that created them.
//!
//! - Only a read-write Worker the user dispatched in a conversation gets the
//!   automation tools; it creates automations owned by that conversation.
//! - A due `AgentTask` dispatches a new Worker under the conversation's Router,
//!   exactly like `dispatch_work`. When it ends, the Router's usual
//!   `[WorkEventBatch]` announcement tells the user in that conversation.
//! - A due `Reminder` becomes a system-event turn in the conversation.
//!
//! No session is created outside the conversation and nothing runs manually.

use std::sync::{Arc, Weak};
use std::time::Duration;

use flowy_agent_core::{
    MessageSource, SessionRecord, SessionRole, WorkAccessMode,
};
use flowy_automation::{
    Access, Action, Automation, AutomationService, CancelToken as RunCancel,
    RunExecutor, RunFuture, RunId, RunOutcome, RunReport,
};

use super::router::DispatchWorkRequest;
use super::{AgentHandle, Harness, ResumedSubmission, lock};

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

    fn work_state(&self, conversation: &str, work_id: &str) -> Option<String> {
        self.child_works(conversation)
            .into_iter()
            .find(|work| work.work_id == work_id)
            .map(|work| work.state)
    }

    /// Tells the user in the owning conversation through a system-event turn.
    fn post_reminder(
        self: &Arc<Self>,
        automation: &Automation,
        text: &str,
    ) -> Result<(), &'static str> {
        if self.session_role(&automation.conversation_id) != SessionRole::Router
        {
            return Err("ROUTER_NOT_FOUND");
        }
        let prompt = format!(
            "[AutomationReminder] The automation \"{}\" is due. Remind the user: {text}\nDo not dispatch work for this reminder.",
            automation.title
        );
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| "notice.agent_failed")?;
        self.submit_message_targeted(
            Some(&automation.conversation_id),
            &prompt,
            Some(u128::from_le_bytes(bytes)),
            None,
            None,
            None,
            Some(ResumedSubmission {
                skip_contract_evolve: true,
                model_prompt_override: Some(prompt.clone()),
                display_text_override: Some(String::new()),
                skip_push_user_line: true,
                prompt_source: MessageSource::SystemEvent {
                    event_kind: "automation_reminder".to_owned(),
                },
                bypass_busy_queue: true,
                ..ResumedSubmission::default()
            }),
        )
        .map(|_| ())
    }
}

struct AgentRunExecutor {
    /// Weak so the worker never keeps the agent (and its runtime) alive.
    harness: Weak<Harness>,
}

impl RunExecutor for AgentRunExecutor {
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

    /// Waits while the conversation is busy or showing a card.
    async fn remind(
        &self,
        automation: &Automation,
        text: &str,
        cancel: &RunCancel,
    ) -> RunReport {
        loop {
            let (automation, text) = (automation.clone(), text.to_owned());
            match self
                .blocking(move |harness| {
                    harness.post_reminder(&automation, &text)
                })
                .await
            {
                None => return report(None, failed("agent unavailable")),
                Some(Ok(())) => return report(None, RunOutcome::Succeeded),
                Some(Err("notice.agent_busy" | "notice.card_pending")) => {}
                Some(Err(key)) => return report(None, failed(key)),
            }
            tokio::select! {
                () = cancel.cancelled() => {
                    return report(None, RunOutcome::Cancelled);
                }
                () = tokio::time::sleep(POLL) => {}
            }
        }
    }

    async fn dispatch(
        &self,
        automation: &Automation,
        run_id: RunId,
        prompt: &str,
        cancel: &RunCancel,
    ) -> RunReport {
        let (owned, prompt) = (automation.clone(), prompt.to_owned());
        let work_id = match self
            .blocking(move |harness| {
                harness.dispatch_run(&owned, run_id, &prompt)
            })
            .await
        {
            None => return report(None, failed("agent unavailable")),
            Some(Err(code)) => return report(None, failed(code)),
            Some(Ok(work_id)) => work_id,
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
            if let Some(outcome) = settled(&state, cancel_deadline.is_some()) {
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
fn settled(state: &str, cancelling: bool) -> Option<RunOutcome> {
    Some(match state {
        "completed" | "delivered" | "partial" => RunOutcome::Succeeded,
        "failed" => failed("the work failed"),
        "stopped" => RunOutcome::Cancelled,
        "awaiting_approval" if !cancelling => {
            failed("waiting for the user in the conversation")
        }
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
