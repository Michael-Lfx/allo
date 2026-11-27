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
    /// Conversation each run was dispatched into.
    conversations: Mutex<Vec<String>>,
    /// `None` waits until cancelled.
    outcome: Mutex<Option<RunOutcome>>,
}

impl FakeExecutor {
    fn returning(outcome: Option<RunOutcome>) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(outcome),
            ..Self::default()
        })
    }
}

impl RunExecutor for FakeExecutor {
    fn run<'a>(
        &'a self,
        automation: &'a Automation,
        _run_id: RunId,
        cancel: CancelToken,
    ) -> RunFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.conversations
                .lock()
                .unwrap()
                .push(automation.conversation_id.clone());
            let outcome = self.outcome.lock().unwrap().clone();
            let outcome = if let Some(outcome) = outcome {
                outcome
            } else {
                cancel.cancelled().await;
                RunOutcome::Cancelled
            };
            RunReport {
                work_id: Some("w-1".to_owned()),
                outcome,
            }
        })
    }
}

struct Harness {
    service: AutomationService,
    store: Arc<AutomationStore>,
    executor: Arc<FakeExecutor>,
}

fn harness(outcome: Option<RunOutcome>, run_timeout: Duration) -> Harness {
    let store = Arc::new(AutomationStore::open_in_memory().unwrap());
    let executor = FakeExecutor::returning(outcome);
    let worker = Worker::start(
        Arc::clone(&store),
        executor.clone(),
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
    }
}

fn daily() -> TriggerSpec {
    TriggerSpec::Calendar(CalendarRule {
        time_zone: "Etc/UTC".parse().unwrap(),
        local_time: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        frequency: CalendarFrequency::Daily,
    })
}

fn agent_task() -> AutomationInput {
    AutomationInput {
        title: " Daily summary ".to_owned(),
        action: Action::AgentTask {
            prompt: "summarise my inbox".to_owned(),
        },
        trigger: daily(),
        access: Access::Read,
    }
}

/// Makes `automation` due now, so the next tick starts it.
fn make_due(h: &Harness, automation: &Automation) {
    let mut due = automation.clone();
    due.next_run_at_ms = Some(now_ms() - 3 * 86_400_000);
    h.store.save(&due).unwrap();
    h.service.worker.wake();
}

async fn wait_runs(
    service: &AutomationService,
    id: AutomationId,
    done: impl Fn(&[Run]) -> bool,
) -> Vec<Run> {
    for _ in 0..500 {
        let runs = service.history(id, 10).unwrap();
        if done(&runs) {
            return runs;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("runs of {id} did not settle");
}

fn any_final(runs: &[Run]) -> bool {
    runs.iter().any(|run| run.status.is_final())
}

const LONG: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread")]
async fn create_binds_the_conversation_and_schedules_the_next_run() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    let automation = h.service.create("r-1", agent_task()).unwrap();
    assert_eq!(automation.title, "Daily summary");
    assert_eq!(automation.conversation_id, "r-1");
    assert!(automation.next_run_at_ms.unwrap() > now_ms());
    assert_eq!(h.service.list_for("r-1").unwrap(), vec![automation]);
    assert_eq!(h.service.list_for("r-2").unwrap(), vec![]);
    assert!(matches!(
        h.service.create(" ", agent_task()),
        Err(ServiceError::Invalid(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn missed_occurrences_run_once_in_the_owning_conversation() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    let automation = h.service.create("r-1", agent_task()).unwrap();
    make_due(&h, &automation);
    h.service.set_signed_in(true);

    let runs = wait_runs(&h.service, automation.id, any_final).await;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::Succeeded);
    assert_eq!(runs[0].work_id.as_deref(), Some("w-1"));
    assert_eq!(*h.executor.conversations.lock().unwrap(), vec!["r-1"]);
    assert!(
        h.service
            .get(automation.id)
            .unwrap()
            .next_run_at_ms
            .unwrap()
            > now_ms()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signed_out_worker_starts_nothing() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    let automation = h.service.create("r-1", agent_task()).unwrap();
    make_due(&h, &automation);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.service.history(automation.id, 10).unwrap(), vec![]);
    assert_eq!(h.executor.calls.load(Ordering::SeqCst), 0);

    h.service.set_signed_in(true);
    wait_runs(&h.service, automation.id, any_final).await;
    assert_eq!(h.executor.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_and_sign_out_stop_a_running_run() {
    let h = harness(None, LONG);
    h.service.set_signed_in(true);
    let automation = h.service.create("r-1", agent_task()).unwrap();
    make_due(&h, &automation);
    let runs = wait_runs(&h.service, automation.id, |runs| {
        runs.iter().any(|run| run.status == RunStatus::Running)
    })
    .await;
    // Wait until the executor has picked it up before cancelling.
    while h.executor.calls.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(h.service.cancel(runs[0].id));
    let runs = wait_runs(&h.service, automation.id, any_final).await;
    assert_eq!(runs[0].status, RunStatus::Cancelled);
    assert!(!h.service.cancel(runs[0].id));

    let second = h.service.create("r-1", agent_task()).unwrap();
    make_due(&h, &second);
    wait_runs(&h.service, second.id, |runs| !runs.is_empty()).await;
    while h.executor.calls.load(Ordering::SeqCst) < 2 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.service.set_signed_in(false);
    let runs = wait_runs(&h.service, second.id, any_final).await;
    assert_eq!(runs[0].status, RunStatus::Cancelled);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_exceeds_the_limit_fails_as_timed_out() {
    let h = harness(None, Duration::from_millis(50));
    h.service.set_signed_in(true);
    let automation = h.service.create("r-1", agent_task()).unwrap();
    make_due(&h, &automation);
    let runs = wait_runs(&h.service, automation.id, any_final).await;
    assert_eq!(runs[0].status, RunStatus::Failed);
    assert_eq!(runs[0].error.as_deref(), Some("timed out"));
}

#[tokio::test(flavor = "multi_thread")]
async fn paused_automations_are_not_due_and_resume_skips_missed_runs() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    h.service.set_signed_in(true);
    let automation = h.service.create("r-1", agent_task()).unwrap();
    let paused = h.service.pause(automation.id).unwrap();
    assert!(!paused.enabled);
    assert_eq!(paused.next_run_at_ms, None);
    let resumed = h.service.resume(automation.id).unwrap();
    assert!(resumed.next_run_at_ms.unwrap() > now_ms());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.executor.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn update_keeps_the_conversation_and_delete_conversation_removes_all() {
    let h = harness(Some(RunOutcome::Succeeded), LONG);
    let first = h.service.create("r-1", agent_task()).unwrap();
    h.service.create("r-1", agent_task()).unwrap();
    let other = h.service.create("r-2", agent_task()).unwrap();

    let updated = h
        .service
        .update(
            first.id,
            AutomationInput {
                title: "Stand-up".to_owned(),
                action: Action::Reminder {
                    text: "Stand-up in 5 minutes".to_owned(),
                },
                ..agent_task()
            },
        )
        .unwrap();
    assert_eq!(updated.conversation_id, "r-1");
    assert_eq!(updated.title, "Stand-up");

    h.service.delete_conversation("r-1").unwrap();
    assert_eq!(h.service.list().unwrap(), vec![other]);
    assert!(matches!(
        h.service.delete(first.id),
        Err(ServiceError::NotFound)
    ));
}
