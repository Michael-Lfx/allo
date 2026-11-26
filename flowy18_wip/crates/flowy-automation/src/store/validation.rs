//! Typed validation complements SQL shape and registered parent-reference checks.
//! No persisted JSON or side-store binding can bypass its Rust value contract.

use rusqlite::{Connection, Row};

use super::audit::{AuditFinding, SideRecord, SideStoreInventory};
use super::records::{
    AttemptRecord, AutomationRecord, DeletionPlanRecord, OutboxRecord,
    ReceiptRecord, ReceiptState, RevisionRecord, RunRecord, WatermarkRecord,
    validate_definition,
};
use super::{StoreError, repository};
use crate::schema_contract::{LOGICAL_REFERENCES, SideStore};
use crate::{
    ArtifactState, AutomationCommand, CommandOutcome, HistoryCommitState,
    ResultRef, RunPhase, RunSpec,
};

fn all<T>(
    connection: &Connection,
    table: &str,
    read: fn(&Row<'_>) -> Result<T, StoreError>,
) -> Result<Vec<T>, StoreError> {
    let mut statement =
        connection.prepare(&format!("SELECT * FROM {table}"))?;
    let mut rows = statement.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(read(row)?);
    }
    Ok(result)
}

fn add(findings: &mut Vec<AuditFinding>, code: &'static str) {
    findings.push(AuditFinding {
        reference: LOGICAL_REFERENCES.len(),
        code,
        count: 1,
    });
}

const fn terminal(run: &RunRecord) -> bool {
    matches!(run.phase, RunPhase::Settled | RunPhase::Interrupted)
}

fn spec_matches_run(spec: &RunSpec, run: &RunRecord) -> bool {
    spec.validate().is_ok()
        && spec.principal == run.principal
        && spec.automation_id == run.automation_id
        && spec.run_id == run.run_id
        && spec.revision == run.revision
        && spec.latest_start_at_ms == run.latest_start_at_ms
        && run
            .current_attempt_id
            .as_ref()
            .is_none_or(|id| *id == spec.attempt_id)
}

fn spec_matches_revision(spec: &RunSpec, revision: &RevisionRecord) -> bool {
    spec.action == revision.definition.action
        && spec.source_session_id == revision.context.source_session_id
        && spec.context_summary == revision.context.summary
        && spec.workspace == revision.definition.workspace
        && spec.capabilities == revision.definition.capabilities
        && spec.model == revision.definition.model
        && spec.budget == revision.definition.budget
}

fn validate_deletion_plan(
    plan: &DeletionPlanRecord,
    attempts: &[AttemptRecord],
    findings: &mut Vec<AuditFinding>,
) {
    // A syntactically valid opaque receipt does not prove which Run owns it.
    // Retention must introduce typed, scoped artifact proof before accepting
    // receipt targets; storage never treats arbitrary strings as permission.
    if !plan.plan.artifact_receipts.is_empty() {
        add(findings, "unproven_deletion_receipt_reference");
    }
    let belongs_to_run =
        |attempt: &AttemptRecord, session: &crate::SessionRef| {
            attempt.spec.principal == plan.principal
                && attempt.spec.run_id == plan.run_id
                && attempt.spec.session_id.as_ref() == Some(session)
        };
    if plan.plan.sessions.iter().any(|session| {
        !attempts
            .iter()
            .any(|attempt| belongs_to_run(attempt, session))
    }) {
        add(findings, "deletion_session_binding_mismatch");
    }
    if plan.plan.private_workspaces.iter().any(|session| {
        !attempts.iter().any(|attempt| {
            belongs_to_run(attempt, session)
                // An explicit workspace is a user resource, never the
                // session-managed Home directory owned by retention.
                && attempt.spec.workspace.is_none()
        })
    }) {
        add(findings, "deletion_private_workspace_binding_mismatch");
    }
}

pub(super) fn validate(
    connection: &Connection,
) -> Result<Vec<AuditFinding>, StoreError> {
    // Read every row through exactly the decoder used by the repository. A
    // syntactically valid JSON number/object is not a valid PrivateText or
    // command merely because SQLite's json_valid accepted it.
    let automations: Vec<AutomationRecord> =
        all(connection, "automations", repository::read_automation)?;
    let revisions: Vec<RevisionRecord> = all(
        connection,
        "automation_revisions",
        repository::read_revision,
    )?;
    let runs: Vec<RunRecord> = all(connection, "runs", repository::read_run)?;
    let attempts: Vec<AttemptRecord> =
        all(connection, "run_attempts", repository::read_attempt)?;
    let receipts: Vec<ReceiptRecord> =
        all(connection, "command_receipts", repository::read_receipt)?;
    let outbox: Vec<OutboxRecord> =
        all(connection, "delivery_outbox", repository::read_outbox)?;
    let plans: Vec<DeletionPlanRecord> =
        all(connection, "deletion_plans", repository::read_plan)?;
    // Schema 001 is audited before its forward migration creates this table;
    // after migration the live schema check proves the table is present.
    let clocks_present: bool = connection.query_row(
        "SELECT count(*) > 0 FROM sqlite_schema WHERE type='table' AND name='command_clocks'",
        [],
        |row| row.get(0),
    )?;
    if clocks_present {
        let _: Vec<super::records::CommandClockRecord> =
            all(connection, "command_clocks", repository::read_command_clock)?;
    }
    let _: Vec<WatermarkRecord> = all(
        connection,
        "schedule_watermarks",
        repository::read_watermark,
    )?;
    let mut findings = Vec::new();
    for automation in &automations {
        if automation.title.expose().trim().is_empty()
            || automation.updated_at_ms < automation.created_at_ms
        {
            add(&mut findings, "invalid_automation_value");
        }
        if let Some(revision) = revisions.iter().find(|revision| {
            (
                &revision.principal,
                &revision.automation_id,
                revision.revision,
            ) == (
                &automation.principal,
                &automation.automation_id,
                automation.current_revision,
            )
        }) && revision.definition.title != automation.title
        {
            add(&mut findings, "current_revision_title_mismatch");
        }
    }
    for revision in &revisions {
        if validate_definition(&revision.definition).is_err()
            || revision.grant_ref != revision.definition.capabilities.grant_ref
        {
            add(&mut findings, "revision_json_mismatch");
        }
    }
    for run in &runs {
        if terminal(run) != run.outcome.is_some()
            || (run.source_removed && !terminal(run))
        {
            add(&mut findings, "invalid_run_state");
        }
        if let Some(key) = &run.occurrence_key
            && crate::identity::validate_reference(key).is_err()
        {
            add(&mut findings, "invalid_occurrence_token");
        }
        if let Some(spec) = &run.spec {
            if !spec_matches_run(spec, run) {
                add(&mut findings, "run_spec_binding_mismatch");
            }
            if let Some(revision) = revisions.iter().find(|revision| {
                revision.principal == run.principal
                    && revision.automation_id == run.automation_id
                    && revision.revision == run.revision
            }) && !spec_matches_revision(spec, revision)
            {
                add(&mut findings, "run_revision_snapshot_mismatch");
            }
        }
    }
    for attempt in &attempts {
        let spec = &attempt.spec;
        if spec.validate().is_err()
            || spec.session_id.is_none()
            || spec.submission_id.is_none()
        {
            add(&mut findings, "invalid_attempt_specification");
        }
        if let Some(run) = runs.iter().find(|run| {
            run.run_id == spec.run_id && run.principal == spec.principal
        }) {
            // An old attempt remains valid when a later attempt is selected;
            // only the immutable Run identity and revision bind every attempt.
            if spec.automation_id != run.automation_id
                || spec.revision != run.revision
                || (run.current_attempt_id.as_ref() == Some(&spec.attempt_id)
                    && spec.latest_start_at_ms != run.latest_start_at_ms)
            {
                add(&mut findings, "attempt_run_snapshot_mismatch");
            }
            if run.current_attempt_id.as_ref() == Some(&spec.attempt_id)
                && run.spec.as_ref().is_some_and(|snapshot| snapshot != spec)
            {
                add(&mut findings, "current_attempt_spec_mismatch");
            }
        }
        if let Some(revision) = revisions.iter().find(|revision| {
            revision.principal == spec.principal
                && revision.automation_id == spec.automation_id
                && revision.revision == spec.revision
        }) && !spec_matches_revision(spec, revision)
        {
            add(&mut findings, "attempt_revision_snapshot_mismatch");
        }
        if let Some(completion) = &attempt.completion {
            let reference_matches = |reference: &crate::CommitRef| {
                spec.session_id.as_ref() == Some(&reference.session_id)
                    && spec.submission_id.as_ref()
                        == Some(&reference.submission_id)
            };
            if completion
                .commit_ref
                .as_ref()
                .is_some_and(|reference| !reference_matches(reference))
            {
                add(&mut findings, "attempt_completion_commit_mismatch");
            }
            match &completion.result_ref {
                Some(ResultRef::SessionTurn { commit })
                    if !reference_matches(commit) =>
                {
                    add(&mut findings, "attempt_completion_result_mismatch");
                }
                Some(ResultRef::Reminder { .. }) => {
                    add(&mut findings, "agent_attempt_has_reminder_result");
                }
                _ => {}
            }
            if completion.artifact_state == ArtifactState::Committed
                && completion.history_commit != HistoryCommitState::Committed
            {
                add(&mut findings, "artifact_precedes_history_commit");
            }
        }
    }
    for receipt in &receipts {
        if (receipt.state == ReceiptState::Applied) != receipt.outcome.is_some()
            || receipt.envelope.expires_at_ms <= receipt.envelope.issued_at_ms
        {
            add(&mut findings, "receipt_state_mismatch");
        }
        if let Some(outcome) = &receipt.outcome {
            let valid = match (&receipt.envelope.command, outcome) {
                (
                    AutomationCommand::CreateManual { definition },
                    CommandOutcome::Created {
                        automation_id,
                        revision,
                    },
                ) => revisions
                    .iter()
                    .find(|record| {
                        record.principal == receipt.principal
                            && record.automation_id == *automation_id
                            && record.revision == *revision
                    })
                    .map_or(receipt.source_removed, |record| {
                        record.definition == **definition
                    }),
                (
                    AutomationCommand::RunNow {
                        automation_id,
                        expected_revision,
                    },
                    CommandOutcome::RunQueued { run_id },
                ) => runs
                    .iter()
                    .find(|record| {
                        record.principal == receipt.principal
                            && record.run_id == *run_id
                    })
                    .map_or(receipt.source_removed, |record| {
                        record.automation_id == *automation_id
                            && record.revision == *expected_revision
                    }),
                (
                    AutomationCommand::CancelRun { run_id: requested },
                    CommandOutcome::CancelRequested { run_id, .. },
                )
                | (
                    AutomationCommand::RetryDelivery { run_id: requested }
                    | AutomationCommand::RetryDeliveryTarget {
                        run_id: requested,
                        ..
                    }
                    | AutomationCommand::AbandonPendingArtifacts {
                        run_id: requested,
                    },
                    CommandOutcome::DeliveryRetryQueued { run_id },
                ) => requested == run_id,
                (
                    AutomationCommand::RetryInterrupted { run_id: source },
                    CommandOutcome::RunQueued { run_id },
                ) => source == run_id,
                (
                    AutomationCommand::SetResultKept {
                        run_id: requested,
                        kept: requested_kept,
                    },
                    CommandOutcome::ResultKept { run_id, kept },
                ) => requested == run_id && requested_kept == kept,
                (
                    AutomationCommand::UpdateManual {
                        automation_id: requested,
                        expected_revision,
                        definition,
                    },
                    CommandOutcome::Updated {
                        automation_id,
                        revision,
                    },
                ) => {
                    requested == automation_id
                        && expected_revision.get().checked_add(1)
                            == Some(revision.get())
                        && revisions
                            .iter()
                            .find(|record| {
                                record.principal == receipt.principal
                                    && record.automation_id == *automation_id
                                    && record.revision == *revision
                            })
                            .map_or(receipt.source_removed, |record| {
                                record.definition == **definition
                            })
                }
                (
                    AutomationCommand::Pause {
                        automation_id: requested,
                        expected_revision,
                    }
                    | AutomationCommand::Resume {
                        automation_id: requested,
                        expected_revision,
                    }
                    | AutomationCommand::Archive {
                        automation_id: requested,
                        expected_revision,
                    },
                    CommandOutcome::StateChanged {
                        automation_id,
                        revision,
                        state,
                    },
                ) => {
                    requested == automation_id
                        && expected_revision.get().checked_add(1)
                            == Some(revision.get())
                        && *state
                            == match receipt.envelope.command {
                                AutomationCommand::Pause { .. } => {
                                    crate::DefinitionState::Paused
                                }
                                AutomationCommand::Resume { .. } => {
                                    crate::DefinitionState::Enabled
                                }
                                _ => crate::DefinitionState::Archived,
                            }
                }
                _ => false,
            };
            if !valid {
                add(&mut findings, "receipt_command_target_mismatch");
            }
        }
    }
    for delivery in &outbox {
        if [
            &delivery.delivery_key,
            &delivery.target,
            &delivery.message_kind,
        ]
        .iter()
        .any(|value| crate::identity::validate_reference(value).is_err())
        {
            add(&mut findings, "invalid_delivery_token");
        }
    }
    for plan in &plans {
        validate_deletion_plan(plan, &attempts, &mut findings);
    }
    Ok(findings)
}

fn triple_matches(child: &SideRecord, parent: &SideRecord) -> bool {
    child.automation_id.is_some()
        && child.run_id.is_some()
        && child.attempt_id.is_some()
        && child.automation_id == parent.automation_id
        && child.run_id == parent.run_id
        && child.attempt_id == parent.attempt_id
}

fn matches_attempt(record: &SideRecord, attempt: &AttemptRecord) -> bool {
    let spec = &attempt.spec;
    record.principal.as_ref() == Some(&spec.principal)
        && record.automation_id.as_ref() == Some(&spec.automation_id)
        && record.run_id.as_ref() == Some(&spec.run_id)
        && record.attempt_id.as_ref() == Some(&spec.attempt_id)
        && record.session_id == spec.session_id
        && record.submission_id == spec.submission_id
}

pub(super) fn validate_side_bindings(
    connection: &Connection,
    inventory: &SideStoreInventory,
) -> Result<Vec<AuditFinding>, StoreError> {
    let attempts: Vec<AttemptRecord> =
        all(connection, "run_attempts", repository::read_attempt)?;
    let runs: Vec<RunRecord> = all(connection, "runs", repository::read_run)?;
    let mut findings = Vec::new();
    for record in inventory
        .records
        .iter()
        .filter(|record| record.automation_owned)
    {
        if record.principal.is_none() {
            add(&mut findings, "unknown_side_owner");
            continue;
        }
        match record.store {
            SideStore::GrantLedger => {
                if record.grant_id.is_none() {
                    add(&mut findings, "missing_grant_ledger_identity");
                }
            }
            SideStore::AgentHistory => {
                if !attempts
                    .iter()
                    .any(|attempt| matches_attempt(record, attempt))
                {
                    add(&mut findings, "history_attempt_binding_mismatch");
                }
            }
            SideStore::AgentIndex
            | SideStore::ArtifactManifest
            | SideStore::PrivateWorkspace => {
                let matching_history =
                    inventory.records.iter().any(|history| {
                        history.store == SideStore::AgentHistory
                            && history.automation_owned
                            && history.principal == record.principal
                            && record.session_id.is_some()
                            && history.session_id == record.session_id
                            && (record.store != SideStore::ArtifactManifest
                                || record.submission_id.is_some())
                            && record.submission_id.as_ref().is_none_or(
                                |submission| {
                                    history.submission_id.as_ref()
                                        == Some(submission)
                                },
                            )
                            && triple_matches(record, history)
                    });
                // Index summaries alone may outlive a deliberately removed
                // history, and only after its matching Run records removal.
                let removed_index = record.store == SideStore::AgentIndex
                    && record.source_removed
                    && runs.iter().any(|run| {
                        terminal(run)
                            && run.source_removed
                            && record.principal.as_ref() == Some(&run.principal)
                            && record.run_id.as_ref() == Some(&run.run_id)
                            && record.automation_id.as_ref()
                                == Some(&run.automation_id)
                            && record.attempt_id.is_some()
                    });
                if !matching_history && !removed_index {
                    add(&mut findings, "side_execution_binding_mismatch");
                }
            }
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests;
