use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn monitored(
    limits: RunBudget,
) -> (RunBudgetState, Arc<Mutex<Vec<RunBudgetCheckpoint>>>) {
    let start = RunBudgetStart::new(limits, 1_000).unwrap();
    let saves = Arc::new(Mutex::new(vec![start.checkpoint()]));
    let out = Arc::clone(&saves);
    let state = start.track(move |checkpoint| {
        out.lock().unwrap().push(checkpoint);
        Ok(())
    });
    (state, saves)
}

#[test]
fn budget_limits_must_be_positive_and_deadlines_must_fit() {
    assert!(RunBudget::new(0, 1, 1).is_none());
    assert!(RunBudget::new(100, 0, 1).is_none());
    assert!(RunBudget::new(100, 1, 0).is_none());
    let limits = RunBudget::new(100, 1, 1).unwrap();
    assert_eq!(
        RunBudgetStart::new(limits, i64::MAX).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[tokio::test]
async fn each_dispatch_reserves_one_unit_before_returning() {
    let (state, saves) = monitored(RunBudget::new(60_000, 1, 2).unwrap());
    assert_eq!(state.reserve_model(), Ok(()));
    assert_eq!(state.reserve_tool(), Ok(()));
    assert_eq!(state.reserve_tool(), Ok(()));
    assert!(!state.is_stopping());
    assert_eq!(saves.lock().unwrap().len(), 4);

    let reason = BudgetStopReason::ToolCallBudget;
    assert_eq!(state.reserve_tool(), Err(BudgetStop::Limit(reason)));
    assert_eq!(state.wait_for_stop().await, BudgetStop::Limit(reason));
    assert_eq!(state.stop_requested(), Some(BudgetStop::Limit(reason)));
    assert_eq!(state.reserve_model(), Err(BudgetStop::Limit(reason)));
    let saves = saves.lock().unwrap();
    assert_eq!(saves.len(), 5);
    let final_state = *saves.last().unwrap();
    drop(saves);
    assert_eq!(final_state.started_at_ms, 1_000);
    assert_eq!(final_state.deadline_at_ms, 61_000);
    assert_eq!(final_state.model_calls, 1);
    assert_eq!(final_state.tool_calls, 2);
    assert_eq!(final_state.stop_reason, Some(reason));
    assert!(final_state.is_valid());
    let mut corrupt = final_state;
    corrupt.tool_calls = 1;
    assert!(!corrupt.is_valid());
    assert_eq!(
        final_state,
        serde_json::from_str(&serde_json::to_string(&final_state).unwrap())
            .unwrap()
    );
}

#[test]
fn model_reservations_cannot_race_past_the_limit() {
    let (state, saves) = monitored(RunBudget::new(60_000, 3, 20).unwrap());
    let accepted = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                if state.reserve_model().is_ok() {
                    accepted.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    assert_eq!(accepted.load(Ordering::Relaxed), 3);
    let saves = saves.lock().unwrap();
    let final_state = saves.last().unwrap();
    assert_eq!(saves.len(), 5);
    assert_eq!(final_state.model_calls, 3);
    assert_eq!(final_state.tool_calls, 0);
    assert_eq!(
        final_state.stop_reason,
        Some(BudgetStopReason::ModelCallBudget)
    );
}

#[tokio::test]
async fn a_stalled_stream_can_wake_the_supervisor_without_more_calls() {
    let limits = RunBudget::new(250, 9, 9).unwrap();
    let elapsed_start = Instant::now() - Duration::from_secs(1);
    let start =
        RunBudgetStart::with_instant(limits, -10_000, elapsed_start).unwrap();
    let committed = Arc::new(Mutex::new(start.checkpoint()));
    let out = Arc::clone(&committed);
    let state = start.track(move |checkpoint| {
        *out.lock().unwrap() = checkpoint;
        Ok(())
    });
    assert_eq!(
        state.wait_for_stop().await,
        BudgetStop::Limit(BudgetStopReason::Deadline)
    );
    assert_eq!(
        state.reserve_model(),
        Err(BudgetStop::Limit(BudgetStopReason::Deadline))
    );
    let committed = committed.lock().unwrap();
    assert_eq!(committed.deadline_at_ms, -9_750);
    assert_eq!(committed.stop_reason, Some(BudgetStopReason::Deadline));
    assert_eq!(committed.model_calls, 0);
}

#[test]
fn a_checkpoint_that_runs_past_deadline_does_not_admit_dispatch() {
    let limits = RunBudget::new(1_000, 4, 5).unwrap();
    let (state, saves) = monitored(limits);
    let reads = AtomicUsize::new(0);
    let result = state.reserve(RequestKind::Tool, || {
        if reads.fetch_add(1, Ordering::Relaxed) == 0 {
            state.deadline - Duration::from_millis(1)
        } else {
            state.deadline
        }
    });
    assert_eq!(result, Err(BudgetStop::Limit(BudgetStopReason::Deadline)));
    let saves = saves.lock().unwrap();
    assert_eq!(saves.len(), 3);
    assert_eq!(saves[1].tool_calls, 1);
    assert_eq!(saves[1].stop_reason, None);
    assert_eq!(saves[2].stop_reason, Some(BudgetStopReason::Deadline));
}

#[tokio::test]
async fn failed_checkpoint_stops_calls_without_claiming_a_durable_limit() {
    let start =
        RunBudgetStart::new(RunBudget::new(60_000, 1, 2).unwrap(), 1_000)
            .unwrap();
    let saves = Arc::new(Mutex::new(vec![start.checkpoint()]));
    let out = Arc::clone(&saves);
    let state = start.track(move |checkpoint| {
        if checkpoint.stop_reason.is_some() {
            return Err(BudgetCheckpointError::Storage);
        }
        out.lock().unwrap().push(checkpoint);
        Ok(())
    });
    assert_eq!(state.reserve_model(), Ok(()));
    assert_eq!(state.reserve_model(), Err(BudgetStop::Storage));
    assert_eq!(state.reserve_tool(), Err(BudgetStop::Storage));
    assert_eq!(state.wait_for_stop().await, BudgetStop::Storage);
    let saves = saves.lock().unwrap();
    assert_eq!(saves.len(), 2);
    assert_eq!(saves[1].model_calls, 1);
    assert_eq!(saves[1].stop_reason, None);
}

#[test]
fn stale_checkpoint_cannot_charge_a_replaced_submission() {
    let limits = RunBudget::new(60_000, 1, 1).unwrap();
    let state = RunBudgetStart::new(limits, 1_000)
        .unwrap()
        .track(|_| Err(BudgetCheckpointError::StaleSubmission));
    assert_eq!(state.reserve_model(), Err(BudgetStop::Cancelled));
    assert_eq!(state.stop_requested(), Some(BudgetStop::Cancelled));
}
