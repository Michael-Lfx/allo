//! Integration tests for the goals repository (goal persistence for the
//! Nomi goal-driven session loop).

use nomifun_db::{
    IGoalRepository, SqliteGoalRepository, UpsertGoalParams, init_database_memory,
};
use std::sync::Arc;

fn sample_goal(session_id: &str) -> UpsertGoalParams {
    UpsertGoalParams {
        session_id: session_id.to_string(),
        objective: "ship the feature".into(),
        status: "active".into(),
        turns_used: 0,
        max_turns: 20,
        created_at: 1_000,
        last_turn_at: None,
        last_verdict: None,
        last_reason: None,
        paused_reason: None,
        consecutive_parse_failures: 0,
        consecutive_transport_failures: 0,
        subgoals_json: "[]".into(),
        contract_json: None,
        waiting_on_pid: None,
        waiting_on_session: None,
        waiting_until: None,
        waiting_reason: None,
        updated_at: 1_000,
    }
}

#[tokio::test]
async fn goal_upsert_insert_and_load_roundtrip() {
    let db = init_database_memory().await.unwrap();
    let repo: Arc<dyn IGoalRepository> = Arc::new(SqliteGoalRepository::new(db.pool().clone()));

    let created = repo.upsert(&sample_goal("conv-1")).await.unwrap();
    assert_eq!(created.session_id, "conv-1");
    assert_eq!(created.status, "active");
    assert_eq!(created.max_turns, 20);
    assert_eq!(created.version, 0);
    assert_eq!(created.subgoals_json, "[]");

    let loaded = repo.load_by_session("conv-1").await.unwrap().expect("row");
    assert_eq!(loaded.id, created.id);
    assert_eq!(loaded.objective, "ship the feature");
    assert!(repo.load_by_session("missing").await.unwrap().is_none());
}

#[tokio::test]
async fn goal_upsert_conflict_replaces_snapshot_and_bumps_version() {
    let db = init_database_memory().await.unwrap();
    let repo = SqliteGoalRepository::new(db.pool().clone());

    let first = repo.upsert(&sample_goal("conv-1")).await.unwrap();
    let mut next = sample_goal("conv-1");
    next.status = "paused".into();
    next.paused_reason = Some("judge API unreachable 5 turns in a row".into());
    next.turns_used = 5;
    next.last_verdict = Some("continue".into());
    next.last_reason = Some("keep going".into());
    next.last_turn_at = Some(2_000);
    next.consecutive_transport_failures = 5;
    next.updated_at = 2_000;

    let updated = repo.upsert(&next).await.unwrap();
    // Same physical row, replaced snapshot, bumped optimistic-lock counter.
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.version, first.version + 1);
    assert_eq!(updated.status, "paused");
    assert_eq!(updated.turns_used, 5);
    assert_eq!(updated.consecutive_transport_failures, 5);
    assert_eq!(updated.last_verdict.as_deref(), Some("continue"));
    assert_eq!(
        updated.paused_reason.as_deref(),
        Some("judge API unreachable 5 turns in a row")
    );

    let third = repo.upsert(&sample_goal("conv-1")).await.unwrap();
    assert_eq!(third.version, first.version + 2);
}

#[tokio::test]
async fn goal_json_columns_round_trip_verbatim() {
    let db = init_database_memory().await.unwrap();
    let repo = SqliteGoalRepository::new(db.pool().clone());

    let mut params = sample_goal("conv-json");
    params.subgoals_json = r#"["write tests","run tests"]"#.into();
    params.contract_json = Some(
        r#"{"outcome":"green CI","verification":"cargo test","constraints":[],"boundaries":[],"stop_when":null}"#
            .into(),
    );
    let row = repo.upsert(&params).await.unwrap();
    assert_eq!(row.subgoals_json, params.subgoals_json);
    assert_eq!(row.contract_json, params.contract_json);
}

#[tokio::test]
async fn goal_clear_deletes_row_and_is_idempotent() {
    let db = init_database_memory().await.unwrap();
    let repo = SqliteGoalRepository::new(db.pool().clone());

    repo.upsert(&sample_goal("conv-1")).await.unwrap();
    repo.clear("conv-1").await.unwrap();
    assert!(repo.load_by_session("conv-1").await.unwrap().is_none());
    // Clearing an absent goal is not an error.
    repo.clear("conv-1").await.unwrap();
}

#[tokio::test]
async fn goal_migrate_session_id_rebinds_row() {
    let db = init_database_memory().await.unwrap();
    let repo = SqliteGoalRepository::new(db.pool().clone());

    let created = repo.upsert(&sample_goal("conv-old")).await.unwrap();
    assert!(repo.migrate_session_id("conv-old", "conv-new").await.unwrap());
    assert!(repo.load_by_session("conv-old").await.unwrap().is_none());
    let moved = repo.load_by_session("conv-new").await.unwrap().expect("row");
    assert_eq!(moved.id, created.id);
    assert_eq!(moved.objective, "ship the feature");

    // Missing source → false; same id → no-op true.
    assert!(!repo.migrate_session_id("ghost", "conv-x").await.unwrap());
    assert!(repo.migrate_session_id("conv-new", "conv-new").await.unwrap());
}

#[tokio::test]
async fn goal_migrate_session_id_replaces_stale_target_row() {
    let db = init_database_memory().await.unwrap();
    let repo = SqliteGoalRepository::new(db.pool().clone());

    let source = repo.upsert(&sample_goal("conv-src")).await.unwrap();
    let mut stale = sample_goal("conv-dst");
    stale.objective = "stale leftover".into();
    repo.upsert(&stale).await.unwrap();

    assert!(repo.migrate_session_id("conv-src", "conv-dst").await.unwrap());
    let winner = repo.load_by_session("conv-dst").await.unwrap().expect("row");
    assert_eq!(winner.id, source.id);
    assert_eq!(winner.objective, "ship the feature");
}
