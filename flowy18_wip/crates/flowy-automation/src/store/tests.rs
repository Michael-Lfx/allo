use super::*;
use crate::model::Action;
use crate::schedule::TriggerSpec;

fn automation(
    title: &str,
    conversation: &str,
    created_at_ms: i64,
) -> Automation {
    Automation {
        id: AutomationId::generate(),
        conversation_id: conversation.to_owned(),
        title: title.to_owned(),
        action: Action::AgentTask {
            prompt: "summarise".to_owned(),
        },
        trigger: TriggerSpec::Once {
            scheduled_at_ms: 1_000,
        },
        access: Access::Write,
        enabled: true,
        next_run_at_ms: None,
        created_at_ms,
        updated_at_ms: created_at_ms,
    }
}

fn finished(automation: &Automation, created_at_ms: i64) -> Run {
    Run {
        status: RunStatus::Succeeded,
        work_id: Some("w-1".to_owned()),
        finished_at_ms: Some(created_at_ms + 1),
        ..Run::started(automation.id, created_at_ms)
    }
}

fn temp_db() -> PathBuf {
    std::env::temp_dir()
        .join(format!("flowy-automation-{}", AutomationId::generate()))
        .join("automations.sqlite3")
}

#[test]
fn saves_reads_and_deletes_with_runs() {
    let store = AutomationStore::open_in_memory().unwrap();
    let mut first = automation("first", "r-1", 1);
    let second = automation("second", "r-2", 2);
    store.save(&first).unwrap();
    store.save(&second).unwrap();
    assert_eq!(store.get(first.id).unwrap(), Some(first.clone()));

    first.title = "renamed".to_owned();
    first.next_run_at_ms = Some(10);
    store.save(&first).unwrap();
    assert_eq!(store.list().unwrap(), vec![first.clone(), second.clone()]);
    assert_eq!(store.list_for("r-2").unwrap(), vec![second.clone()]);
    assert_eq!(store.due(9).unwrap(), vec![]);
    assert_eq!(store.due(10).unwrap(), vec![first.clone()]);

    let run = finished(&first, 5);
    store.save_run(&run).unwrap();
    assert_eq!(store.run(run.id).unwrap(), Some(run.clone()));
    assert!(store.delete(first.id).unwrap());
    assert!(!store.delete(first.id).unwrap());
    assert_eq!(store.run(run.id).unwrap(), None);
    // A run finishing after its automation was deleted is not written back.
    store.save_run(&run).unwrap();
    assert_eq!(store.run(run.id).unwrap(), None);
    assert_eq!(store.list().unwrap(), vec![second]);
}

#[test]
fn reopening_keeps_data_and_unknown_databases_are_backed_up() {
    let path = temp_db();
    let saved = automation("kept", "r-1", 1);
    AutomationStore::open(&path).unwrap().save(&saved).unwrap();
    let reopened = AutomationStore::open(&path).unwrap();
    assert_eq!(reopened.list().unwrap(), vec![saved]);
    drop(reopened);

    let unknown = temp_db();
    fs::create_dir_all(unknown.parent().unwrap()).unwrap();
    let legacy = Connection::open(&unknown).unwrap();
    legacy
        .execute_batch(
            "CREATE TABLE legacy (id TEXT); PRAGMA user_version = 1;",
        )
        .unwrap();
    drop(legacy);

    let fresh = AutomationStore::open(&unknown).unwrap();
    assert_eq!(fresh.list().unwrap(), vec![]);
    let backups = fs::read_dir(unknown.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".bak-"))
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn unfinished_runs_become_interrupted_on_startup() {
    let store = AutomationStore::open_in_memory().unwrap();
    let owner = automation("owner", "r-1", 1);
    store.save(&owner).unwrap();
    let running = Run {
        status: RunStatus::Running,
        started_at_ms: Some(3),
        ..Run::started(owner.id, 2)
    };
    store.save_run(&running).unwrap();
    let done = finished(&owner, 4);
    store.save_run(&done).unwrap();

    assert_eq!(store.mark_running_interrupted(9).unwrap(), 1);
    let interrupted = store.run(running.id).unwrap().unwrap();
    assert_eq!(interrupted.status, RunStatus::Interrupted);
    assert_eq!(interrupted.finished_at_ms, Some(9));
    assert_eq!(store.run(done.id).unwrap(), Some(done));
}

#[test]
fn prune_keeps_the_newest_finished_runs_and_all_active_ones() {
    let store = AutomationStore::open_in_memory().unwrap();
    let owner = automation("owner", "r-1", 1);
    store.save(&owner).unwrap();
    let runs: Vec<_> = (10..15).map(|at| finished(&owner, at)).collect();
    for run in &runs {
        store.save_run(run).unwrap();
    }
    let running = Run {
        status: RunStatus::Running,
        ..Run::started(owner.id, 1)
    };
    store.save_run(&running).unwrap();

    assert_eq!(store.prune(2).unwrap(), 3);
    let kept: Vec<_> = store
        .runs_for(owner.id, 10)
        .unwrap()
        .into_iter()
        .map(|run| run.id)
        .collect();
    assert_eq!(kept, vec![runs[4].id, runs[3].id, running.id]);
}

#[test]
fn claiming_an_occurrence_preserves_concurrent_edits_and_records_it_once() {
    let store = AutomationStore::open_in_memory().unwrap();
    let mut task = automation("before", "r-1", 1);
    task.next_run_at_ms = Some(10);
    store.save(&task).unwrap();
    let stale = task.clone();
    task.title = "after".to_owned();
    store.save(&task).unwrap();

    let run = Run::started(task.id, 10);
    assert!(!store.claim(&stale, None, &run).unwrap());
    assert_eq!(store.get(task.id).unwrap(), Some(task.clone()));
    assert_eq!(store.run(run.id).unwrap(), None);

    assert!(store.claim(&task, None, &run).unwrap());
    assert!(!store.claim(&task, None, &run).unwrap());
    assert_eq!(store.due(10).unwrap(), Vec::new());
    assert_eq!(store.run(run.id).unwrap(), Some(run.clone()));

    assert!(store.delete(task.id).unwrap());
    assert!(!store.claim(&task, None, &run).unwrap());
    assert_eq!(store.get(task.id).unwrap(), None);
}
