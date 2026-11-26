use super::*;
use crate::schedule::{
    CalendarFrequency, CalendarRule, MisfirePolicy, SchedulePolicy, TriggerSpec,
};
use chrono::NaiveTime;
use std::num::NonZeroU64;

async fn run_history(
    store: &AutomationStore,
    caller: &CallerContext,
    id: &AutomationId,
) -> Vec<crate::RunSnapshot> {
    let QuerySnapshot::History(runs) = store
        .query(
            caller.principal.clone(),
            AutomationQuery::History {
                automation_id: id.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("history")
    };
    runs
}

#[test]
fn calendar_scan_is_atomic_and_survives_restart_and_clock_rollback() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(())).await.unwrap();
        let c = caller();
        let mut definition = pair().1.definition;
        definition.schedule = Some(SchedulePolicy {
            trigger: TriggerSpec::Calendar(CalendarRule {
                time_zone: chrono_tz::UTC,
                local_time: NaiveTime::from_hms_opt(0,0,0).unwrap(),
                frequency: CalendarFrequency::Daily,
            }),
            misfire: MisfirePolicy::RunOnce,
            max_lateness_ms: NonZeroU64::new(3_600_000).unwrap(),
        });
        let CommandOutcome::Created { automation_id, .. } = apply(&store,&c,envelope(AutomationCommand::CreateManual { definition: Box::new(definition) })).await.unwrap().outcome else { panic!("created") };
        let due = 3 * 86_400_000 + 10_000;
        store.call(|c| { c.execute_batch("CREATE TEMP TRIGGER abort_cursor BEFORE UPDATE ON schedule_watermarks BEGIN SELECT RAISE(ABORT,'cursor fault'); END;")?; Ok(()) }).await.unwrap();
        assert!(store.scan_due(c.principal.clone(), due).await.is_err());
        assert_eq!(run_history(&store,&c,&automation_id).await, Vec::new());
        store.call(|c| { c.execute_batch("DROP TRIGGER abort_cursor")?; Ok(()) }).await.unwrap();
        assert_eq!(store.scan_due(c.principal.clone(), due).await.unwrap(),1);
        let runs = run_history(&store,&c,&automation_id).await;
        assert_eq!(runs.len(),1);
        assert_eq!(runs[0].phase, RunPhase::Queued);
        assert_eq!(runs[0].scheduled_at_ms,Some(3 * 86_400_000));
        assert_eq!(runs[0].covers_from_ms,Some(86_400_000));
        store.shutdown().await.unwrap();
        let store = AutomationStore::open(root.0.clone(), Arc::new(())).await.unwrap();
        for time in [due, 100, due] {
            assert_eq!(store.scan_due(c.principal.clone(),time).await.unwrap(),0);
        }
        let tomorrow = due + 86_400_000;
        assert_eq!(store.scan_due(c.principal.clone(),tomorrow).await.unwrap(),1);
        let after = run_history(&store,&c,&automation_id).await;
        assert_eq!(after[0],runs[0]);
        assert_eq!(after[1].outcome,Some(RunOutcome::Skipped));
        let QuerySnapshot::Automation(snapshot) = store.query(c.principal.clone(), AutomationQuery::Automation { automation_id }).await.unwrap() else { panic!("automation") };
        assert_eq!(snapshot.next_run_at_ms,Some(5 * 86_400_000));
        store.shutdown().await.unwrap();
    });
}

#[test]
fn once_late_is_expired_and_calendar_skip_records_misfire() {
    runtime().block_on(async {
        for (trigger, scan_at, outcome) in [
            (
                TriggerSpec::Once {
                    scheduled_at_ms: 200,
                },
                1_000,
                RunOutcome::Expired,
            ),
            (
                TriggerSpec::Calendar(CalendarRule {
                    time_zone: chrono_tz::UTC,
                    local_time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
                    frequency: CalendarFrequency::Daily,
                }),
                86_406_000,
                RunOutcome::Skipped,
            ),
        ] {
            let root = TestRoot::new();
            let store = AutomationStore::open(root.0.clone(), Arc::new(()))
                .await
                .unwrap();
            let c = caller();
            let mut definition = pair().1.definition;
            definition.schedule = Some(SchedulePolicy {
                trigger,
                misfire: MisfirePolicy::Skip,
                max_lateness_ms: NonZeroU64::new(300).unwrap(),
            });
            let CommandOutcome::Created { automation_id, .. } = apply(
                &store,
                &c,
                envelope(AutomationCommand::CreateManual {
                    definition: Box::new(definition),
                }),
            )
            .await
            .unwrap()
            .outcome
            else {
                panic!("created")
            };
            assert_eq!(
                store.scan_due(c.principal.clone(), scan_at).await.unwrap(),
                1
            );
            let runs = run_history(&store, &c, &automation_id).await;
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].outcome, Some(outcome));
            store.shutdown().await.unwrap();
        }
    });
}

#[test]
fn once_due_while_waiting_for_approval_keeps_its_original_lateness_window() {
    runtime().block_on(async {
        let root = TestRoot::new();
        let store = AutomationStore::open(root.0.clone(), Arc::new(()))
            .await
            .unwrap();
        let c = caller();
        let mut definition = pair().1.definition;
        definition.schedule = Some(SchedulePolicy {
            trigger: TriggerSpec::Once {
                scheduled_at_ms: 150,
            },
            misfire: MisfirePolicy::Skip,
            max_lateness_ms: NonZeroU64::new(250).unwrap(),
        });
        let request = envelope(AutomationCommand::CreateManual {
            definition: Box::new(definition),
        });
        let pending = store
            .apply_command(
                c.clone(),
                request.clone(),
                100,
                HostAccessDecision::Blocked(
                    crate::BlockedReason::ApprovalRequired,
                ),
            )
            .await
            .unwrap();
        assert_eq!(pending.outcome, CommandOutcome::PendingApproval);
        assert_eq!(store.scan_due(c.principal.clone(), 170).await.unwrap(), 0);
        let CommandOutcome::Created { automation_id, .. } = store
            .apply_command(c.clone(), request, 180, HostAccessDecision::Allowed)
            .await
            .unwrap()
            .outcome
        else {
            panic!("created")
        };
        assert_eq!(store.scan_due(c.principal.clone(), 180).await.unwrap(), 1);
        let history = run_history(&store, &c, &automation_id).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].scheduled_at_ms, Some(150));
        assert_eq!(history[0].latest_start_at_ms, 400);
        assert_eq!(history[0].phase, RunPhase::Queued);
        store.shutdown().await.unwrap();
    });
}
