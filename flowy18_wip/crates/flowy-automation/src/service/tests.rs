use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::runtime::Handle;

use super::*;
use crate::executor::{
    CancelToken, RunExecutor, RunFuture, RunOutcome, RunReport,
};
use crate::model::{Access, RunStatus};
use crate::schedule::{CalendarFrequency, CalendarRule, TriggerSpec};
use crate::worker::WorkerConfig;

#[derive(Default)]
struct FakeExecutor {
    calls: AtomicUsize,
    /// `None` waits until cancelled.
    outcome: Mutex<Option<RunOutcome>>,
}

impl FakeExecutor {
    fn returning(outcome: Option<RunOutcome>) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            outcome: Mutex::new(outcome),
        })
    }
}

impl RunExecutor for FakeExecutor {
    fn run<'a>(
        &'a self,
        _automation: &'a Automation,
        _run_id: RunId,
        cancel: CancelToken,
    ) -> RunFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let outcome = self.outcome.lock().unwrap().clone();
            let outcome = if let Some(outcome) = outcome {
                outcome
            } else {
                cancel.cancelled().await;
                RunOutcome::Cancelled
            };
            RunReport {
                session_id: Some("session-1".to_owned()),
                outcome,
            }
        })
    }
}

struct Harness {
    service: AutomationService,
    store: Arc<AutomationStore>,
    executor: Arc<FakeExecutor>,
    notified: Arc<Mutex<Vec<RunStatus>>>,
}

fn harness(outcome: Option<RunOutcome>, run_timeout: Duration) -> Harness {
    let store = Arc::new(AutomationStore::open_in_memory().unwrap());
    let executor = FakeExecutor::returning(outcome);
    let notified = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&notified);
    let worker = Worker::start(
        Arc::clone(&store),
        executor.clone(),
        Arc::new(move |_, run| sink.lock().unwrap().push(run.status)),
        WorkerConfig {
            tick: Duration::from_secs(3600),
            run_timeout,
        },
        Handle::current(),
    );
    Harness {
        service: AutomationService::new(Arc::clone(&store), worker),
        store,
        executor,
        notified,
    }
}

fn agent_task(trigger: TriggerSpec) -> AutomationInput {
    AutomationInput {
        title: " Daily summary ".to_owned(),
        action: Action::AgentTask {
            prompt: "summarise my inbox".to_owned(),
        },
        trigger,
        cwd: None,
        access: Access::Read,
    }
}

fn daily() -> TriggerSpec {
    TriggerSpec::Calendar(CalendarRule {
        time_zone: "Etc/UTC".parse().unwrap(),
        local_time: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        frequency: CalendarFrequency::Daily,
    })
}

async fn wait_final(service: &AutomationService, run_id: RunId) -> Run {
    for _ in 0..500 {
        if let Some(run) = service.run(run_id).unwrap()
            && run.status.is_final()
        {
            return run;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("run {run_id} did not finish");
}

const LONG: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread")]
async fn run_now_executes_and_records_the_session() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    h.service.set_signed_in(true);
    let automation = h.service.create(agent_task(TriggerSpec::Manual)).unwrap();
    assert_eq!(automation.title, "Daily summary");
    assert_eq!(automation.next_run_at_ms, None);

    let run = h.service.run_now(automation.id).unwrap();
    let run = wait_final(&h.service, run.id).await;
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.session_id.as_deref(), Some("session-1"));
    assert_eq!(*h.notified.lock().unwrap(), vec![RunStatus::Succeeded]);
    assert_eq!(h.service.history(automation.id, 10).unwrap(), vec![run]);
}

#[tokio::test(flavor = "multi_thread")]
async fn missed_occurrences_run_once_and_advance() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    let mut automation = h.service.create(agent_task(daily())).unwrap();
    let now = now_ms();
    assert!(automation.next_run_at_ms.unwrap() > now);
    automation.next_run_at_ms = Some(now - 3 * 86_400_000);
    h.store.save(&automation).unwrap();

    h.service.set_signed_in(true);
    for _ in 0..500 {
        let runs = h.service.history(automation.id, 10).unwrap();
        if runs.iter().any(|run| run.status.is_final()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let runs = h.service.history(automation.id, 10).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].trigger, RunTrigger::Scheduled);
    assert!(
        h.service
            .get(automation.id)
            .unwrap()
            .next_run_at_ms
            .unwrap()
            > now
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signed_out_worker_leaves_runs_queued_and_queued_runs_cancel() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    let automation = h.service.create(agent_task(TriggerSpec::Manual)).unwrap();
    let first = h.service.run_now(automation.id).unwrap();
    let second = h.service.run_now(automation.id).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        h.service.run(first.id).unwrap().unwrap().status,
        RunStatus::Queued
    );

    assert!(h.service.cancel(second.id).unwrap());
    h.service.set_signed_in(true);
    assert_eq!(
        wait_final(&h.service, first.id).await.status,
        RunStatus::Succeeded
    );
    assert_eq!(
        h.service.run(second.id).unwrap().unwrap().status,
        RunStatus::Cancelled
    );
    assert_eq!(h.executor.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn busy_workspace_skips_the_run() {
    let h = harness(Some(RunOutcome::Busy), LONG);
    h.service.set_signed_in(true);
    let automation = h.service.create(agent_task(TriggerSpec::Manual)).unwrap();
    let run = h.service.run_now(automation.id).unwrap();
    let run = wait_final(&h.service, run.id).await;
    assert_eq!(run.status, RunStatus::Skipped);
    assert_eq!(*h.notified.lock().unwrap(), vec![RunStatus::Skipped]);
}

#[tokio::test(flavor = "multi_thread")]
async fn timeout_cancels_the_turn_and_fails_the_run() {
    let h = harness(None, Duration::from_millis(50));
    h.service.set_signed_in(true);
    let automation = h.service.create(agent_task(TriggerSpec::Manual)).unwrap();
    let run = h.service.run_now(automation.id).unwrap();
    let run = wait_final(&h.service, run.id).await;
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.error.as_deref(), Some("timed out"));
    assert_eq!(run.session_id.as_deref(), Some("session-1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_running_run_is_not_notified() {
    let h = harness(None, LONG);
    h.service.set_signed_in(true);
    let automation = h.service.create(agent_task(TriggerSpec::Manual)).unwrap();
    let run = h.service.run_now(automation.id).unwrap();
    while h.executor.calls.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(h.service.cancel(run.id).unwrap());
    assert_eq!(
        wait_final(&h.service, run.id).await.status,
        RunStatus::Cancelled
    );
    assert!(h.notified.lock().unwrap().is_empty());
    assert!(!h.service.cancel(run.id).unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn reminders_notify_without_the_executor() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    h.service.set_signed_in(true);
    let automation = h
        .service
        .create(AutomationInput {
            action: Action::Reminder {
                text: "stand up".to_owned(),
            },
            ..agent_task(TriggerSpec::Manual)
        })
        .unwrap();
    let run = h.service.run_now(automation.id).unwrap();
    assert_eq!(
        wait_final(&h.service, run.id).await.status,
        RunStatus::Succeeded
    );
    assert_eq!(h.executor.calls.load(Ordering::SeqCst), 0);
    assert_eq!(*h.notified.lock().unwrap(), vec![RunStatus::Succeeded]);
}

#[tokio::test(flavor = "multi_thread")]
async fn validation_pause_resume_update_and_delete() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    assert!(matches!(
        h.service.create(AutomationInput {
            title: "  ".to_owned(),
            ..agent_task(daily())
        }),
        Err(ServiceError::Invalid(_))
    ));

    let automation = h.service.create(agent_task(daily())).unwrap();
    let paused = h.service.pause(automation.id).unwrap();
    assert!(!paused.enabled);
    assert_eq!(paused.next_run_at_ms, None);
    let resumed = h.service.resume(automation.id).unwrap();
    assert!(resumed.enabled);
    assert!(resumed.next_run_at_ms.is_some());

    let updated = h
        .service
        .update(automation.id, agent_task(TriggerSpec::Manual))
        .unwrap();
    assert_eq!(updated.next_run_at_ms, None);
    assert_eq!(updated.created_at_ms, automation.created_at_ms);

    h.service.delete(automation.id).unwrap();
    assert!(matches!(
        h.service.get(automation.id),
        Err(ServiceError::NotFound)
    ));
    assert!(matches!(
        h.service.delete(automation.id),
        Err(ServiceError::NotFound)
    ));
}
