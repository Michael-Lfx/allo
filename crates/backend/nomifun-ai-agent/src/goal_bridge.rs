//! Conversions between the engine's in-memory goal state
//! (`nomi_agent::goal::state::GoalState`), the persisted `goals` row
//! (`nomifun_db::GoalRow`) and the wire DTO
//! (`nomifun_api_types::GoalStatusResponse`).
//!
//! Kept in one place (and `pub`) so the Nomi manager (persist / restore) and
//! `nomifun-conversation` (no-runtime DB fallback path) share the exact same
//! column semantics: `goals.max_turns` carries the engine's
//! `max_auto_continuations` budget; `subgoals_json` / `contract_json` hold the
//! engine-owned JSON payloads verbatim; status/verdict strings are the
//! snake_case serde names locked by the `GoalState` wire-contract tests.

use nomi_agent::goal::runtime::GoalRuntime;
use nomi_agent::goal::state::{GoalContract, GoalState, GoalStatus, GoalVerdict};
use nomifun_api_types::{GoalContractDto, GoalStatusResponse};
use nomifun_db::{GoalRow, UpsertGoalParams};

/// Serialize a status enum to its snake_case wire string ("active", …).
pub fn goal_status_str(status: GoalStatus) -> &'static str {
    match status {
        GoalStatus::Active => "active",
        GoalStatus::Complete => "complete",
        GoalStatus::Blocked => "blocked",
        GoalStatus::Paused => "paused",
        GoalStatus::Waiting => "waiting",
        GoalStatus::Cleared => "cleared",
    }
}

fn goal_verdict_str(verdict: GoalVerdict) -> &'static str {
    match verdict {
        GoalVerdict::Done => "done",
        GoalVerdict::Continue => "continue",
        GoalVerdict::Wait => "wait",
        GoalVerdict::Skipped => "skipped",
    }
}

fn parse_goal_status(s: &str) -> Option<GoalStatus> {
    match s {
        "active" => Some(GoalStatus::Active),
        "complete" => Some(GoalStatus::Complete),
        "blocked" => Some(GoalStatus::Blocked),
        "paused" => Some(GoalStatus::Paused),
        "waiting" => Some(GoalStatus::Waiting),
        "cleared" => Some(GoalStatus::Cleared),
        _ => None,
    }
}

fn parse_goal_verdict(s: &str) -> Option<GoalVerdict> {
    match s {
        "done" => Some(GoalVerdict::Done),
        "continue" => Some(GoalVerdict::Continue),
        "wait" => Some(GoalVerdict::Wait),
        "skipped" => Some(GoalVerdict::Skipped),
        _ => None,
    }
}

/// Engine snapshot → upsert params for the `goals` table.
pub fn goal_state_to_upsert(session_id: &str, state: &GoalState) -> UpsertGoalParams {
    UpsertGoalParams {
        session_id: session_id.to_owned(),
        objective: state.objective.clone(),
        status: goal_status_str(state.status).to_owned(),
        turns_used: state.turns_used as i64,
        max_turns: state.max_auto_continuations as i64,
        created_at: state.created_at as i64,
        last_turn_at: state.last_turn_at.map(|v| v as i64),
        last_verdict: state.last_verdict.map(|v| goal_verdict_str(v).to_owned()),
        last_reason: state.last_reason.clone(),
        paused_reason: state.paused_reason.clone(),
        consecutive_parse_failures: state.consecutive_parse_failures as i64,
        consecutive_transport_failures: state.consecutive_transport_failures as i64,
        subgoals_json: serde_json::to_string(&state.subgoals).unwrap_or_else(|_| "[]".to_owned()),
        contract_json: state
            .contract
            .as_ref()
            .and_then(|c| serde_json::to_string(c).ok()),
        waiting_on_pid: state.waiting_on_pid.map(|v| v as i64),
        waiting_on_session: state.waiting_on_session.clone(),
        waiting_until: state.waiting_until.map(|v| v as i64),
        waiting_reason: state.waiting_reason.clone(),
        updated_at: now_epoch_ms(),
    }
}

/// Persisted row → complete engine snapshot (restore injection payload).
/// An unknown status string degrades to `Paused` — never resurrect a goal we
/// cannot interpret as silently `Active`.
pub fn goal_row_to_state(row: &GoalRow) -> GoalState {
    GoalState {
        objective: row.objective.clone(),
        status: parse_goal_status(&row.status).unwrap_or(GoalStatus::Paused),
        max_auto_continuations: row.max_turns.max(0) as usize,
        // Not persisted: the budget window restarts with the new process.
        auto_continuations: 0,
        blocked_threshold: 3,
        turns_used: row.turns_used.max(0) as usize,
        last_verdict: row.last_verdict.as_deref().and_then(parse_goal_verdict),
        last_reason: row.last_reason.clone(),
        paused_reason: row.paused_reason.clone(),
        // Deliberately reset on restore, like `auto_continuations`: the
        // breaker streaks describe one live run and restart with the new
        // process (docs/guides/goals.md). The columns still persist the last
        // run's values for inspection — they are just never re-injected.
        consecutive_parse_failures: 0,
        consecutive_transport_failures: 0,
        created_at: row.created_at.max(0) as u64,
        last_turn_at: row.last_turn_at.map(|v| v.max(0) as u64),
        subgoals: serde_json::from_str(&row.subgoals_json).unwrap_or_default(),
        contract: row
            .contract_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<GoalContract>(s).ok()),
        waiting_until: row.waiting_until.map(|v| v.max(0) as u64),
        waiting_on_pid: row.waiting_on_pid.map(|v| v.max(0) as u32),
        waiting_on_session: row.waiting_on_session.clone(),
        waiting_reason: row.waiting_reason.clone(),
    }
}

/// Engine contract → wire DTO (identical five snake_case fields).
pub fn engine_contract_to_dto(contract: &GoalContract) -> GoalContractDto {
    GoalContractDto {
        outcome: contract.outcome.clone(),
        verification: contract.verification.clone(),
        constraints: contract.constraints.clone(),
        boundaries: contract.boundaries.clone(),
        stop_when: contract.stop_when.clone(),
    }
}

/// Wire DTO → engine contract. An all-empty DTO round-trips to an all-empty
/// `GoalContract`, which `GoalRuntime::set_contract` normalizes back to
/// `None` (clears the contract).
pub fn dto_to_engine_contract(dto: GoalContractDto) -> GoalContract {
    GoalContract {
        outcome: dto.outcome,
        verification: dto.verification,
        constraints: dto.constraints,
        boundaries: dto.boundaries,
        stop_when: dto.stop_when,
    }
}

/// Engine snapshot → wire DTO for `/goal status` and every action response.
pub fn goal_state_to_response(state: &GoalState) -> GoalStatusResponse {
    GoalStatusResponse {
        active: true,
        objective: Some(state.objective.clone()),
        status: Some(goal_status_str(state.status).to_owned()),
        turns_used: Some(state.turns_used as u64),
        max_turns: Some(state.max_auto_continuations as u64),
        last_verdict: state.last_verdict.map(|v| goal_verdict_str(v).to_owned()),
        last_reason: state.last_reason.clone(),
        paused_reason: state.paused_reason.clone(),
        subgoals: state.subgoals.clone(),
        created_at: Some(state.created_at),
        last_turn_at: state.last_turn_at,
        waiting_until: state.waiting_until,
        waiting_reason: state.waiting_reason.clone(),
        waiting_on_pid: state.waiting_on_pid,
        waiting_on_session: state.waiting_on_session.clone(),
        contract: state.contract.as_ref().map(engine_contract_to_dto),
    }
}

/// Persisted row → wire DTO (no-runtime fallback: the conversation service
/// answers `/goal status` straight from the DB when the agent is not built).
pub fn goal_row_to_response(row: &GoalRow) -> GoalStatusResponse {
    goal_state_to_response(&goal_row_to_state(row))
}

/// Whether a persisted status string should be re-injected into a fresh
/// engine at session build. Terminal goals (complete/blocked/cleared) stay in
/// the DB for audit but never restart continuation.
pub fn goal_status_is_restorable(status: &str) -> bool {
    matches!(status, "active" | "paused" | "waiting")
}

/// Upsert params for a freshly set goal (`/goal <objective>` on a
/// conversation without a live agent runtime — the next session build
/// restore-injects it).
pub fn fresh_goal_upsert(session_id: &str, objective: String, max_turns: usize) -> UpsertGoalParams {
    goal_state_to_upsert(session_id, &GoalState::new(objective, max_turns))
}

/// Pause a persisted goal row without a live runtime. Round-trips through
/// [`GoalRuntime`] so the transition guards (only Active|Waiting pause;
/// terminal states never flip) are the engine's own, not a re-implementation.
pub fn pause_persisted_row(row: &GoalRow, reason: &str) -> UpsertGoalParams {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    rt.pause(reason);
    goal_state_to_upsert(&row.session_id, &rt.snapshot())
}

/// Resume a persisted goal row without a live runtime (only Paused resumes;
/// counters and the continuation budget restart, engine semantics).
pub fn resume_persisted_row(row: &GoalRow) -> UpsertGoalParams {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    rt.resume();
    goal_state_to_upsert(&row.session_id, &rt.snapshot())
}

/// Append a criterion to a persisted goal row without a live runtime
/// (`/subgoal <text>`). Whitespace-trimmed; empty text is the engine's
/// no-op, callers validate first for a clear error.
pub fn add_subgoal_row(row: &GoalRow, text: &str) -> UpsertGoalParams {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    rt.add_subgoal(text);
    goal_state_to_upsert(&row.session_id, &rt.snapshot())
}

/// Remove the 1-based `index`-th criterion from a persisted goal row.
/// `None` = out of range (incl. 0), engine semantics — the caller surfaces
/// the error instead of upserting an unchanged row.
pub fn remove_subgoal_row(row: &GoalRow, index_1based: usize) -> Option<UpsertGoalParams> {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    rt.remove_subgoal(index_1based)
        .then(|| goal_state_to_upsert(&row.session_id, &rt.snapshot()))
}

/// Drop every criterion from a persisted goal row (`/subgoal clear`) while
/// leaving the goal itself untouched. Implemented as repeated engine-guarded
/// removal, mirroring the live-runtime path.
pub fn clear_subgoals_row(row: &GoalRow) -> UpsertGoalParams {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    while rt.remove_subgoal(1) {}
    goal_state_to_upsert(&row.session_id, &rt.snapshot())
}

/// Set / replace the completion contract on a persisted goal row without a
/// live runtime (`set_contract`, or applying a draft). An all-empty contract
/// clears it — `GoalRuntime::set_contract` normalization, not ours.
pub fn set_contract_row(row: &GoalRow, contract: GoalContract) -> UpsertGoalParams {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    rt.set_contract(contract);
    goal_state_to_upsert(&row.session_id, &rt.snapshot())
}

/// Park a persisted goal row on a pid barrier without a live runtime
/// (`/goal wait <pid>`). `None` = rejected (pid 0, or the goal is neither
/// Active nor Waiting) — engine semantics via `GoalRuntime::wait_on_pid`.
pub fn wait_on_pid_row(row: &GoalRow, pid: u32) -> Option<UpsertGoalParams> {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    rt.wait_on_pid(pid)
        .then(|| goal_state_to_upsert(&row.session_id, &rt.snapshot()))
}

/// Drop the wait barrier on a persisted goal row without a live runtime
/// (`/goal unwait`). `None` = the goal was not waiting.
pub fn unwait_row(row: &GoalRow) -> Option<UpsertGoalParams> {
    let rt = GoalRuntime::from_state(goal_row_to_state(row));
    rt.unwait()
        .then(|| goal_state_to_upsert(&row.session_id, &rt.snapshot()))
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> GoalState {
        let mut s = GoalState::new("ship the feature".into(), 8);
        s.status = GoalStatus::Paused;
        s.turns_used = 5;
        s.last_verdict = Some(GoalVerdict::Continue);
        s.last_reason = Some("keep going".into());
        s.paused_reason = Some("user-paused".into());
        s.consecutive_parse_failures = 2;
        s.consecutive_transport_failures = 1;
        s.created_at = 42;
        s.last_turn_at = Some(43);
        s.subgoals = vec!["a".into(), "b".into()];
        s
    }

    #[test]
    fn state_row_roundtrip_preserves_snapshot_fields() {
        let state = sample_state();
        let params = goal_state_to_upsert("conv-1", &state);
        assert_eq!(params.session_id, "conv-1");
        assert_eq!(params.status, "paused");
        assert_eq!(params.max_turns, 8);
        assert_eq!(params.last_verdict.as_deref(), Some("continue"));
        assert_eq!(params.subgoals_json, r#"["a","b"]"#);
        // The breaker streaks ARE persisted (audit trail of the last run)…
        assert_eq!(params.consecutive_parse_failures, 2);
        assert_eq!(params.consecutive_transport_failures, 1);

        let row = GoalRow {
            id: 1,
            session_id: params.session_id.clone(),
            objective: params.objective.clone(),
            status: params.status.clone(),
            turns_used: params.turns_used,
            max_turns: params.max_turns,
            created_at: params.created_at,
            last_turn_at: params.last_turn_at,
            last_verdict: params.last_verdict.clone(),
            last_reason: params.last_reason.clone(),
            paused_reason: params.paused_reason.clone(),
            consecutive_parse_failures: params.consecutive_parse_failures,
            consecutive_transport_failures: params.consecutive_transport_failures,
            subgoals_json: params.subgoals_json.clone(),
            contract_json: params.contract_json.clone(),
            waiting_on_pid: params.waiting_on_pid,
            waiting_on_session: params.waiting_on_session.clone(),
            waiting_until: params.waiting_until,
            waiting_reason: params.waiting_reason.clone(),
            updated_at: params.updated_at,
            version: 0,
        };
        let restored = goal_row_to_state(&row);
        assert_eq!(restored.objective, state.objective);
        assert_eq!(restored.status, GoalStatus::Paused);
        assert_eq!(restored.turns_used, 5);
        assert_eq!(restored.max_auto_continuations, 8);
        assert_eq!(restored.last_verdict, Some(GoalVerdict::Continue));
        assert_eq!(restored.paused_reason.as_deref(), Some("user-paused"));
        assert_eq!(restored.created_at, 42);
        assert_eq!(restored.subgoals, vec!["a".to_owned(), "b".to_owned()]);
        // Deliberately reset on restore: fresh continuation budget window…
        assert_eq!(restored.auto_continuations, 0);
        // …and fresh circuit-breaker streaks — a restored goal never resumes
        // mid-breaker (the persisted values are audit-only).
        assert_eq!(restored.consecutive_parse_failures, 0);
        assert_eq!(restored.consecutive_transport_failures, 0);
    }

    #[test]
    fn unknown_status_degrades_to_paused_not_active() {
        let state = sample_state();
        let mut params = goal_state_to_upsert("conv-1", &state);
        params.status = "some_future_status".into();
        let row = GoalRow {
            id: 1,
            session_id: params.session_id,
            objective: params.objective,
            status: params.status,
            turns_used: 0,
            max_turns: 8,
            created_at: 0,
            last_turn_at: None,
            last_verdict: None,
            last_reason: None,
            paused_reason: None,
            consecutive_parse_failures: 0,
            consecutive_transport_failures: 0,
            subgoals_json: "not json".into(),
            contract_json: Some("not json".into()),
            waiting_on_pid: None,
            waiting_on_session: None,
            waiting_until: None,
            waiting_reason: None,
            updated_at: 0,
            version: 0,
        };
        let restored = goal_row_to_state(&row);
        assert_eq!(restored.status, GoalStatus::Paused);
        // Corrupt JSON columns fail soft, never poison the restore.
        assert!(restored.subgoals.is_empty());
        assert!(restored.contract.is_none());
    }

    #[test]
    fn response_carries_wire_contract_fields() {
        let resp = goal_state_to_response(&sample_state());
        assert!(resp.active);
        assert_eq!(resp.status.as_deref(), Some("paused"));
        assert_eq!(resp.objective.as_deref(), Some("ship the feature"));
        assert_eq!(resp.turns_used, Some(5));
        assert_eq!(resp.max_turns, Some(8));
        assert_eq!(resp.last_verdict.as_deref(), Some("continue"));
        assert_eq!(resp.subgoals.len(), 2);
        assert_eq!(resp.created_at, Some(42));
    }

    #[test]
    fn restorable_statuses_exclude_terminal_states() {
        for s in ["active", "paused", "waiting"] {
            assert!(goal_status_is_restorable(s), "{s} must be restorable");
        }
        for s in ["complete", "blocked", "cleared", "junk"] {
            assert!(!goal_status_is_restorable(s), "{s} must not be restorable");
        }
    }

    #[test]
    fn db_only_pause_and_resume_reuse_engine_guards() {
        let state = GoalState::new("ship".into(), 8);
        let params = goal_state_to_upsert("conv-1", &state);
        let mut row = GoalRow {
            id: 1,
            session_id: params.session_id.clone(),
            objective: params.objective.clone(),
            status: params.status.clone(),
            turns_used: 4,
            max_turns: params.max_turns,
            created_at: params.created_at,
            last_turn_at: None,
            last_verdict: None,
            last_reason: None,
            paused_reason: None,
            consecutive_parse_failures: 2,
            consecutive_transport_failures: 0,
            subgoals_json: "[]".into(),
            contract_json: None,
            waiting_on_pid: None,
            waiting_on_session: None,
            waiting_until: None,
            waiting_reason: None,
            updated_at: 0,
            version: 0,
        };

        // active → paused carries the reason.
        let paused = pause_persisted_row(&row, "user-paused");
        assert_eq!(paused.status, "paused");
        assert_eq!(paused.paused_reason.as_deref(), Some("user-paused"));

        // paused → active resets counters + budget (engine resume semantics).
        row.status = "paused".into();
        row.paused_reason = Some("user-paused".into());
        let resumed = resume_persisted_row(&row);
        assert_eq!(resumed.status, "active");
        assert!(resumed.paused_reason.is_none());
        assert_eq!(resumed.turns_used, 0);
        assert_eq!(resumed.consecutive_parse_failures, 0);

        // Terminal rows never flip (guards come from GoalRuntime itself).
        row.status = "complete".into();
        let unchanged = pause_persisted_row(&row, "too late");
        assert_eq!(unchanged.status, "complete");
        let unchanged = resume_persisted_row(&row);
        assert_eq!(unchanged.status, "complete");
    }

    #[test]
    fn fresh_goal_upsert_starts_active() {
        let params = fresh_goal_upsert("conv-1", "ship it".into(), 12);
        assert_eq!(params.session_id, "conv-1");
        assert_eq!(params.status, "active");
        assert_eq!(params.max_turns, 12);
        assert_eq!(params.turns_used, 0);
    }

    #[test]
    fn response_carries_waiting_barrier_fields() {
        let mut state = sample_state();
        state.status = GoalStatus::Waiting;
        state.waiting_until = Some(99_000);
        state.waiting_reason = Some("cooling down".into());
        let resp = goal_state_to_response(&state);
        assert_eq!(resp.status.as_deref(), Some("waiting"));
        assert_eq!(resp.waiting_until, Some(99_000));
        assert_eq!(resp.waiting_reason.as_deref(), Some("cooling down"));

        // Waiting fields also survive the row round-trip (DB fallback path).
        let params = goal_state_to_upsert("conv-1", &state);
        assert_eq!(params.waiting_until, Some(99_000));
        assert_eq!(params.waiting_reason.as_deref(), Some("cooling down"));
    }

    #[test]
    fn db_only_subgoal_edits_reuse_engine_guards() {
        let state = GoalState::new("ship".into(), 8);
        let params = goal_state_to_upsert("conv-1", &state);
        let row = GoalRow {
            id: 1,
            session_id: params.session_id.clone(),
            objective: params.objective.clone(),
            status: params.status.clone(),
            turns_used: 0,
            max_turns: params.max_turns,
            created_at: params.created_at,
            last_turn_at: None,
            last_verdict: None,
            last_reason: None,
            paused_reason: None,
            consecutive_parse_failures: 0,
            consecutive_transport_failures: 0,
            subgoals_json: r#"["a","b"]"#.into(),
            contract_json: None,
            waiting_on_pid: None,
            waiting_on_session: None,
            waiting_until: None,
            waiting_reason: None,
            updated_at: 0,
            version: 0,
        };

        // Append trims and keeps existing criteria.
        let added = add_subgoal_row(&row, "  c  ");
        assert_eq!(added.subgoals_json, r#"["a","b","c"]"#);

        // 1-based removal; 0 / out-of-range → None (engine semantics).
        assert!(remove_subgoal_row(&row, 0).is_none());
        assert!(remove_subgoal_row(&row, 3).is_none());
        let removed = remove_subgoal_row(&row, 1).unwrap();
        assert_eq!(removed.subgoals_json, r#"["b"]"#);

        // Clear empties the list but never touches the goal itself.
        let cleared = clear_subgoals_row(&row);
        assert_eq!(cleared.subgoals_json, "[]");
        assert_eq!(cleared.status, "active");
        assert_eq!(cleared.objective, "ship");
    }

    #[test]
    fn response_carries_contract_and_process_barrier_fields() {
        let mut state = sample_state();
        state.status = GoalStatus::Waiting;
        state.waiting_on_pid = Some(4242);
        state.waiting_on_session = Some("conv-bg".into());
        state.contract = Some(GoalContract {
            outcome: "feature shipped".into(),
            verification: "cargo test passes".into(),
            ..Default::default()
        });

        let resp = goal_state_to_response(&state);
        assert_eq!(resp.waiting_on_pid, Some(4242));
        assert_eq!(resp.waiting_on_session.as_deref(), Some("conv-bg"));
        let dto = resp.contract.unwrap();
        assert_eq!(dto.outcome, "feature shipped");
        assert_eq!(dto.verification, "cargo test passes");
        assert!(dto.stop_when.is_empty());

        // The same fields ride the upsert (turn-persist path, phase-1 cols).
        let params = goal_state_to_upsert("conv-1", &state);
        assert_eq!(params.waiting_on_pid, Some(4242));
        assert_eq!(params.waiting_on_session.as_deref(), Some("conv-bg"));
        let json = params.contract_json.expect("contract must persist");
        let back: GoalContract = serde_json::from_str(&json).unwrap();
        assert_eq!(back.outcome, "feature shipped");

        // DTO ↔ engine conversion is lossless.
        let engine = dto_to_engine_contract(engine_contract_to_dto(state.contract.as_ref().unwrap()));
        assert_eq!(Some(engine), state.contract);
    }

    #[test]
    fn db_only_contract_and_wait_edits_reuse_engine_guards() {
        let state = GoalState::new("ship".into(), 8);
        let params = goal_state_to_upsert("conv-1", &state);
        let mut row = GoalRow {
            id: 1,
            session_id: params.session_id.clone(),
            objective: params.objective.clone(),
            status: params.status.clone(),
            turns_used: 0,
            max_turns: params.max_turns,
            created_at: params.created_at,
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
            updated_at: 0,
            version: 0,
        };

        // set_contract persists the JSON payload; an all-empty one clears it.
        let set = set_contract_row(
            &row,
            GoalContract {
                outcome: "shipped".into(),
                ..Default::default()
            },
        );
        assert!(set.contract_json.as_deref().unwrap().contains("shipped"));
        let cleared = set_contract_row(&row, GoalContract::default());
        assert!(cleared.contract_json.is_none());

        // wait: pid 0 rejected; a real pid parks the row on the barrier.
        assert!(wait_on_pid_row(&row, 0).is_none());
        let waiting = wait_on_pid_row(&row, 4242).unwrap();
        assert_eq!(waiting.status, "waiting");
        assert_eq!(waiting.waiting_on_pid, Some(4242));
        assert!(waiting.waiting_reason.is_some());

        // unwait: only a Waiting row flips back to Active.
        assert!(unwait_row(&row).is_none());
        row.status = "waiting".into();
        row.waiting_on_pid = Some(4242);
        let resumed = unwait_row(&row).unwrap();
        assert_eq!(resumed.status, "active");
        assert!(resumed.waiting_on_pid.is_none());

        // Terminal rows never flip (guards come from GoalRuntime itself).
        row.status = "complete".into();
        assert!(wait_on_pid_row(&row, 4242).is_none());
        assert!(unwait_row(&row).is_none());
    }
}
