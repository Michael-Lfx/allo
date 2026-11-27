//! Background loop: turns due automations into queued runs and starts queued
//! runs. Starts paused; the app resumes it after sign-in.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::Notify;

use crate::executor::{CancelToken, RunExecutor, RunOutcome, RunReport};
use crate::model::{
    Action, Automation, AutomationId, Run, RunId, RunStatus, RunTrigger, now_ms,
};
use crate::store::{AutomationStore, StoreError};

/// Called when a run ends (not when the user cancelled it). The app turns
/// this into a system notification.
pub type RunListener = Arc<dyn Fn(&Automation, &Run) + Send + Sync>;

#[derive(Clone, Copy, Debug)]
pub struct WorkerConfig {
    pub tick: Duration,
    pub run_timeout: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(30),
            run_timeout: Duration::from_mins(10),
        }
    }
}

pub struct Worker {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Worker")
    }
}

struct Inner {
    store: Arc<AutomationStore>,
    executor: Arc<dyn RunExecutor>,
    on_finished: RunListener,
    config: WorkerConfig,
    runtime: Handle,
    active: AtomicBool,
    stopped: AtomicBool,
    wake: Notify,
    inflight: Mutex<HashMap<RunId, (AutomationId, CancelToken)>>,
}

impl Worker {
    pub fn start(
        store: Arc<AutomationStore>,
        executor: Arc<dyn RunExecutor>,
        on_finished: RunListener,
        config: WorkerConfig,
        runtime: Handle,
    ) -> Self {
        let inner = Arc::new(Inner {
            store,
            executor,
            on_finished,
            config,
            runtime: runtime.clone(),
            active: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            wake: Notify::new(),
            inflight: Mutex::new(HashMap::new()),
        });
        let task = Arc::clone(&inner);
        runtime.spawn(async move {
            while !task.stopped.load(Ordering::Acquire) {
                if task.active.load(Ordering::Acquire) {
                    task.tick();
                }
                let _ = tokio::time::timeout(
                    task.config.tick,
                    task.wake.notified(),
                )
                .await;
            }
        });
        Self { inner }
    }

    pub fn wake(&self) {
        self.inner.wake.notify_one();
    }

    pub fn resume(&self) {
        self.inner.active.store(true, Ordering::Release);
        self.wake();
    }

    /// Stops scheduling and cancels every run in progress.
    pub fn pause(&self) {
        self.inner.active.store(false, Ordering::Release);
        self.cancel_inflight(|_| true);
    }

    pub fn cancel_automation(&self, id: AutomationId) {
        self.cancel_inflight(|automation| automation == id);
    }

    /// Cancels a running or queued run. Returns false if it already ended.
    pub fn cancel(&self, run_id: RunId) -> Result<bool, StoreError> {
        if let Some((_, token)) = self.inner.inflight().get(&run_id) {
            token.cancel();
            return Ok(true);
        }
        self.inner.store.transition(
            run_id,
            RunStatus::Queued,
            RunStatus::Cancelled,
            now_ms(),
        )
    }

    fn cancel_inflight(&self, matches: impl Fn(AutomationId) -> bool) {
        for (automation, token) in self.inner.inflight().values() {
            if matches(*automation) {
                token.cancel();
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.inner.stopped.store(true, Ordering::Release);
        self.inner.wake.notify_one();
    }
}

impl Inner {
    fn inflight(
        &self,
    ) -> MutexGuard<'_, HashMap<RunId, (AutomationId, CancelToken)>> {
        self.inflight.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn tick(self: &Arc<Self>) {
        if let Err(error) =
            self.enqueue_due().and_then(|()| self.start_queued())
        {
            tracing::warn!(%error, "automation tick failed");
        }
    }

    /// However many occurrences were missed, each due automation gets one run
    /// and its next run moves past now.
    fn enqueue_due(&self) -> Result<(), StoreError> {
        let now = now_ms();
        for mut automation in self.store.due(now)? {
            automation.next_run_at_ms =
                automation.trigger.next_after(now).ok().flatten();
            self.store.save(&automation)?;
            self.store.save_run(&Run::queued(
                automation.id,
                RunTrigger::Scheduled,
                now,
            ))?;
        }
        Ok(())
    }

    fn start_queued(self: &Arc<Self>) -> Result<(), StoreError> {
        for mut run in self.store.queued_runs()? {
            let Some(automation) = self.store.get(run.automation_id)? else {
                continue;
            };
            let now = now_ms();
            if !self.store.transition(
                run.id,
                RunStatus::Queued,
                RunStatus::Running,
                now,
            )? {
                continue;
            }
            run.status = RunStatus::Running;
            run.started_at_ms = Some(now);
            let cancel = CancelToken::default();
            self.inflight()
                .insert(run.id, (automation.id, cancel.clone()));
            let inner = Arc::clone(self);
            self.runtime.spawn(async move {
                inner.execute(automation, run, cancel).await;
            });
        }
        Ok(())
    }

    async fn execute(
        self: Arc<Self>,
        automation: Automation,
        mut run: Run,
        cancel: CancelToken,
    ) {
        let report = match &automation.action {
            Action::Reminder { .. } => RunReport {
                session_id: None,
                outcome: RunOutcome::Succeeded,
            },
            Action::AgentTask { .. } => {
                self.run_with_timeout(&automation, run.id, cancel).await
            }
        };
        self.inflight().remove(&run.id);

        run.session_id = report.session_id;
        run.finished_at_ms = Some(now_ms());
        (run.status, run.error) = match report.outcome {
            RunOutcome::Succeeded => (RunStatus::Succeeded, None),
            RunOutcome::Busy => {
                (RunStatus::Skipped, Some("workspace is busy".to_owned()))
            }
            RunOutcome::Failed(message) => (RunStatus::Failed, Some(message)),
            RunOutcome::Cancelled => (RunStatus::Cancelled, None),
        };
        if let Err(error) = self.store.save_run(&run) {
            tracing::warn!(%error, run = %run.id, "saving automation run failed");
        }
        if run.status != RunStatus::Cancelled {
            (self.on_finished)(&automation, &run);
        }
    }

    /// On timeout the turn is cancelled through the same token, so the
    /// executor still stops the agent before returning.
    async fn run_with_timeout(
        &self,
        automation: &Automation,
        run_id: RunId,
        cancel: CancelToken,
    ) -> RunReport {
        let timed_out = Arc::new(AtomicBool::new(false));
        let timer = {
            let (cancel, timed_out) = (cancel.clone(), Arc::clone(&timed_out));
            let limit = self.config.run_timeout;
            self.runtime.spawn(async move {
                tokio::time::sleep(limit).await;
                timed_out.store(true, Ordering::Release);
                cancel.cancel();
            })
        };
        let mut report = self.executor.run(automation, run_id, cancel).await;
        timer.abort();
        if report.outcome == RunOutcome::Cancelled
            && timed_out.load(Ordering::Acquire)
        {
            report.outcome = RunOutcome::Failed("timed out".to_owned());
        }
        report
    }
}
