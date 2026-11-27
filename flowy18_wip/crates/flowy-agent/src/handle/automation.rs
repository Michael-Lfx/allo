//! Background automation runs. Each run is an ordinary Standard session the
//! user can open later; it is never made active and never gets the
//! automation management tools.

use std::sync::{Arc, Weak};
use std::time::Duration;

use flowy_agent_core::{SessionOrigin, SessionRole, WorkAccessMode};
use flowy_automation::{
    Access, Action, Automation, AutomationService, CancelToken as RunCancel,
    RunExecutor, RunFuture, RunId, RunOutcome, RunReport,
};

use super::observer::persist_index;
use super::workspace_lock::WorkspaceAccessMode;
use super::{AgentHandle, AgentWorkspace, Harness, TaskTarget, lock};
use crate::projection::TurnStatus;
use crate::title::{TITLE_MAX_CHARS, clamp_manual_title};

const POLL: Duration = Duration::from_millis(500);
/// How long a cancelled turn may take to stop before the run is closed.
const CANCEL_SETTLE: Duration = Duration::from_secs(30);

impl AgentHandle {
    /// Installs the service behind the model's automation tools. `None`
    /// removes the tools from later turns.
    pub fn set_automation_service(
        &self,
        service: Option<Arc<AutomationService>>,
    ) {
        *lock(&self.inner.state.automation) = service;
    }

    /// Marks sessions created by earlier automation runs (read back from the
    /// automation store at startup) so they stay without management tools.
    pub fn mark_automation_sessions(
        &self,
        session_ids: impl IntoIterator<Item = String>,
    ) {
        lock(&self.inner.state.automation_sessions).extend(session_ids);
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
    /// The service whose tools this session's turn should get, if any.
    pub(super) fn automation_tools_for(
        &self,
        session_id: &str,
    ) -> Option<Arc<AutomationService>> {
        if lock(&self.state.automation_sessions).contains(session_id) {
            return None;
        }
        lock(&self.state.automation).clone()
    }

    /// Creates the run's session and submits the prompt. On failure the
    /// returned report is final.
    fn start_automation_turn(
        self: &Arc<Self>,
        automation: &Automation,
        prompt: &str,
    ) -> Result<String, RunReport> {
        let write = automation.access == Access::Write;
        let (origin, workspace) = match &automation.cwd {
            Some(root) => (
                SessionOrigin::Project { project_id: None },
                Some(AgentWorkspace {
                    project_id: String::new(),
                    root: root.clone(),
                }),
            ),
            // Home sessions get their own directory, so they never conflict.
            None => (SessionOrigin::Home, None),
        };
        // Check the lock before creating a session so a busy workspace does
        // not leave an empty conversation behind.
        if let Some(workspace) = &workspace {
            let mode = if write {
                WorkspaceAccessMode::Write
            } else {
                WorkspaceAccessMode::Read
            };
            if self
                .state
                .workspace_locks
                .try_acquire(&workspace.root, mode, "automation-probe")
                .is_none()
            {
                return Err(report(None, RunOutcome::Busy));
            }
        }
        let access = if write {
            WorkAccessMode::ReadWrite
        } else {
            WorkAccessMode::ReadOnly
        };
        let id = self
            .create_session_with_role_inner(
                origin,
                workspace,
                false,
                SessionRole::Standard,
                None,
                access,
            )
            .map_err(|key| report(None, RunOutcome::Failed(key.to_owned())))?;
        lock(&self.state.automation_sessions).insert(id.clone());
        self.title_automation_session(&id, &automation.title);

        let mut bytes = [0_u8; 16];
        if getrandom::fill(&mut bytes).is_err() {
            return Err(report(
                Some(id),
                RunOutcome::Failed("notice.agent_failed".to_owned()),
            ));
        }
        let submission = u128::from_le_bytes(bytes);
        match self.submit_message_targeted(
            Some(&id),
            prompt,
            Some(submission),
            None,
            None,
            None,
            None,
        ) {
            Ok(_) => Ok(id),
            Err("notice.agent_busy") => Err(report(Some(id), RunOutcome::Busy)),
            Err(key) => {
                Err(report(Some(id), RunOutcome::Failed(key.to_owned())))
            }
        }
    }

    fn title_automation_session(&self, session_id: &str, title: &str) {
        let title = clamp_manual_title(title, TITLE_MAX_CHARS);
        if title.is_empty() {
            return;
        }
        let engine = lock(&self.engine).clone();
        if let Some(engine) = engine
            && engine
                .set_session_title(session_id, title.clone(), true)
                .is_err()
        {
            self.state.report_storage_error();
        }
        if let Some(session) = lock(&self.state.live).get_mut(session_id) {
            session.title = title;
            session.titled = true;
            session.auto_title_pending = false;
        }
        if persist_index(&self.state).is_err() {
            self.state.report_storage_error();
        }
    }

    fn live_status(&self, session_id: &str) -> Option<TurnStatus> {
        lock(&self.state.live)
            .get(session_id)
            .map(|session| session.status.clone())
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
        _run_id: RunId,
        cancel: RunCancel,
    ) -> RunFuture<'a> {
        Box::pin(async move {
            let Action::AgentTask { prompt } = &automation.action else {
                return report(None, RunOutcome::Succeeded);
            };
            let harness = self.harness.clone();
            let (automation, prompt) = (automation.clone(), prompt.clone());
            // Session creation and submission take blocking locks and do
            // file I/O, so they stay off the async workers.
            let started = tokio::task::spawn_blocking(move || {
                harness.upgrade().map(|harness| {
                    harness.start_automation_turn(&automation, &prompt)
                })
            })
            .await
            .ok()
            .flatten();
            match started {
                Some(Ok(session_id)) => {
                    let outcome = self.wait(&session_id, &cancel).await;
                    report(Some(session_id), outcome)
                }
                Some(Err(report)) => report,
                None => unavailable(),
            }
        })
    }
}

impl AgentRunExecutor {
    async fn wait(&self, session_id: &str, cancel: &RunCancel) -> RunOutcome {
        let mut cancel_deadline = None;
        loop {
            let Some(status) = self
                .harness
                .upgrade()
                .map(|harness| harness.live_status(session_id))
            else {
                return RunOutcome::Failed("agent unavailable".to_owned());
            };
            match status {
                None => {
                    return RunOutcome::Failed(
                        "the run's conversation was removed".to_owned(),
                    );
                }
                Some(status) if !status.is_busy() => {
                    return finished(status, cancel_deadline.is_some());
                }
                Some(_) => {}
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
                    let harness = self.harness.clone();
                    let target = TaskTarget::Session(session_id.to_owned());
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Some(harness) = harness.upgrade() {
                            let _ = harness.cancel_task(target);
                        }
                    })
                    .await;
                }
                () = tokio::time::sleep(POLL) => {}
            }
        }
    }
}

fn finished(status: TurnStatus, cancel_requested: bool) -> RunOutcome {
    match status {
        TurnStatus::Completed => RunOutcome::Succeeded,
        TurnStatus::Stopped => RunOutcome::Cancelled,
        TurnStatus::Error { message } => RunOutcome::Failed(message),
        TurnStatus::DeliveryFailed => {
            RunOutcome::Failed("file delivery failed".to_owned())
        }
        TurnStatus::WaitingForUser => {
            RunOutcome::Failed("the run is waiting for user input".to_owned())
        }
        _ if cancel_requested => RunOutcome::Cancelled,
        _ => RunOutcome::Failed("the turn ended without a result".to_owned()),
    }
}

const fn report(session_id: Option<String>, outcome: RunOutcome) -> RunReport {
    RunReport {
        session_id,
        outcome,
    }
}

fn unavailable() -> RunReport {
    report(None, RunOutcome::Failed("agent unavailable".to_owned()))
}

#[cfg(test)]
mod tests;
