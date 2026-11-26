use super::StoreError;
use super::records::{
    AttemptRecord, AutomationRecord, DeletionPlanRecord, OutboxRecord,
    ReceiptRecord, ReceiptState, RevisionRecord, RunRecord, StoreMutation,
    WatermarkRecord, validate_definition,
};
use crate::{
    AutomationId, CommandOutcome, DeletionPlanId, PrincipalId, RequestKey,
    Revision, RunId, RunPhase,
};
use rusqlite::{
    Connection, OptionalExtension, Params, Row, Transaction, params,
};
use serde::{Serialize, de::DeserializeOwned};

pub(super) fn encode<T: Serialize>(value: &T) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)?)
}
fn text<T: Serialize>(value: &T) -> Result<String, StoreError> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or(StoreError::Invalid("enum must serialize to text"))
}
pub(super) fn decode<T: DeserializeOwned>(
    value: &str,
) -> Result<T, StoreError> {
    Ok(serde_json::from_str(value)?)
}
fn enum_value<T: DeserializeOwned>(value: String) -> Result<T, StoreError> {
    Ok(serde_json::from_value(serde_json::Value::String(value))?)
}
fn business<T: DeserializeOwned>(
    row: &Row<'_>,
    column: &str,
) -> Result<T, StoreError> {
    enum_value(row.get(column)?)
}
fn optional_business<T: DeserializeOwned>(
    row: &Row<'_>,
    column: &str,
) -> Result<Option<T>, StoreError> {
    row.get::<_, Option<String>>(column)?
        .map(enum_value)
        .transpose()
}
fn principal(row: &Row<'_>) -> Result<PrincipalId, StoreError> {
    PrincipalId::cloud(
        row.get::<_, String>("principal_authority")?,
        row.get("principal_user_id")?,
    )
    .map_err(|_| StoreError::Corrupt("invalid principal"))
}
fn revision(row: &Row<'_>, column: &str) -> Result<Revision, StoreError> {
    let value = row.get::<_, i64>(column)?;
    u64::try_from(value)
        .ok()
        .and_then(Revision::new)
        .ok_or(StoreError::Corrupt("invalid revision"))
}
fn revision_value(value: Revision) -> Result<i64, StoreError> {
    i64::try_from(value.get())
        .map_err(|_| StoreError::Invalid("revision exceeds SQLite integer"))
}
fn require_owner(
    actual: &PrincipalId,
    expected: &PrincipalId,
) -> Result<(), StoreError> {
    if actual != expected {
        return Err(StoreError::Reference("principal mismatch"));
    }
    Ok(())
}
fn require_parent(
    tx: &Transaction<'_>,
    table: &str,
    field: &str,
    value: &str,
    owner: &PrincipalId,
) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT 1 FROM {table} WHERE {field}=?1 AND principal_authority=?2 AND principal_user_id=?3"
    );
    if tx
        .query_row(
            &sql,
            params![value, owner.authority(), owner.external_user_id()],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Err(StoreError::Reference(
            "parent is missing or belongs to another principal",
        ));
    }
    Ok(())
}
fn require_revision(
    tx: &Transaction<'_>,
    owner: &PrincipalId,
    id: &AutomationId,
    rev: Revision,
) -> Result<(), StoreError> {
    let exists = tx.query_row("SELECT 1 FROM automation_revisions WHERE automation_id=?1 AND revision=?2 AND principal_authority=?3 AND principal_user_id=?4", params![id.as_str(), revision_value(rev)?, owner.authority(), owner.external_user_id()], |_| Ok(())).optional()?.is_some();
    if !exists {
        return Err(StoreError::Reference(
            "revision is missing or belongs to another principal",
        ));
    }
    Ok(())
}
fn validate_pair(
    owner: &PrincipalId,
    automation: &AutomationRecord,
    rev: &RevisionRecord,
) -> Result<(), StoreError> {
    require_owner(&automation.principal, owner)?;
    require_owner(&rev.principal, owner)?;
    if (&automation.automation_id, automation.current_revision)
        != (&rev.automation_id, rev.revision)
        || automation.title != rev.definition.title
        || rev.grant_ref != rev.definition.capabilities.grant_ref
    {
        return Err(StoreError::Invalid(
            "definition revision binding mismatch",
        ));
    }
    if automation.updated_at_ms < automation.created_at_ms {
        return Err(StoreError::Invalid("automation time order"));
    }
    validate_definition(&rev.definition)
}
fn insert_revision(
    tx: &Transaction<'_>,
    value: &RevisionRecord,
) -> Result<(), StoreError> {
    tx.execute("INSERT INTO automation_revisions (automation_id,revision,principal_authority,principal_user_id,definition_json,context_json,grant_ref,created_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![value.automation_id.as_str(),revision_value(value.revision)?,value.principal.authority(),value.principal.external_user_id(),encode(&value.definition)?,encode(&value.context)?,value.grant_ref.as_ref().map(crate::GrantId::as_str),value.created_at_ms])?;
    Ok(())
}

pub(super) fn commit(
    connection: &mut Connection,
    owner: &PrincipalId,
    mutations: Vec<StoreMutation>,
) -> Result<(), StoreError> {
    let tx = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for mutation in mutations {
        match mutation {
            StoreMutation::CreateAutomation {
                automation: a,
                revision: r,
            } => {
                validate_pair(owner, &a, &r)?;
                if r.revision.get() != 1 {
                    return Err(StoreError::Invalid(
                        "initial revision must be one",
                    ));
                }
                tx.execute("INSERT INTO automations (automation_id,principal_authority,principal_user_id,current_revision,state,title_json,created_at_ms,updated_at_ms,source_removed) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![a.automation_id.as_str(),owner.authority(),owner.external_user_id(),revision_value(a.current_revision)?,text(&a.state)?,encode(&a.title)?,a.created_at_ms,a.updated_at_ms,a.source_removed])?;
                insert_revision(&tx, &r)?;
            }
            StoreMutation::AdvanceRevision {
                expected,
                automation: a,
                revision: r,
            } => {
                validate_pair(owner, &a, &r)?;
                if expected.get().checked_add(1) != Some(r.revision.get()) {
                    return Err(StoreError::Invalid(
                        "revision must advance once",
                    ));
                }
                let updated = tx.execute("UPDATE automations SET current_revision=?1,state=?2,title_json=?3,updated_at_ms=?4,source_removed=?5 WHERE automation_id=?6 AND principal_authority=?7 AND principal_user_id=?8 AND current_revision=?9",params![revision_value(a.current_revision)?,text(&a.state)?,encode(&a.title)?,a.updated_at_ms,a.source_removed,a.automation_id.as_str(),owner.authority(),owner.external_user_id(),revision_value(expected)?])?;
                if updated != 1 {
                    return Err(StoreError::Conflict);
                }
                insert_revision(&tx, &r)?;
            }
            StoreMutation::InsertRun(r) => {
                require_owner(&r.principal, owner)?;
                require_revision(&tx, owner, &r.automation_id, r.revision)?;
                if let Some(retry) = &r.retry_of_run_id {
                    require_parent(
                        &tx,
                        "runs",
                        "run_id",
                        retry.as_str(),
                        owner,
                    )?;
                }
                if r.current_attempt_id.is_some() {
                    return Err(StoreError::Invalid(
                        "insert run before its attempt",
                    ));
                }
                if r.source_removed
                    && !matches!(
                        r.phase,
                        RunPhase::Settled | RunPhase::Interrupted
                    )
                {
                    return Err(StoreError::Invalid(
                        "active run cannot lose its source",
                    ));
                }
                if matches!(r.phase, RunPhase::Settled | RunPhase::Interrupted)
                    != r.outcome.is_some()
                {
                    return Err(StoreError::Invalid(
                        "run outcome and phase disagree",
                    ));
                }
                if let Some(spec) = &r.spec {
                    spec.validate()
                        .map_err(|_| StoreError::Invalid("invalid RunSpec"))?;
                    if spec.principal != *owner
                        || spec.run_id != r.run_id
                        || spec.automation_id != r.automation_id
                        || spec.revision != r.revision
                    {
                        return Err(StoreError::Invalid(
                            "RunSpec binding mismatch",
                        ));
                    }
                }
                if let Some(key) = &r.occurrence_key {
                    crate::identity::validate_reference(key).map_err(|_| {
                        StoreError::Invalid("invalid occurrence key")
                    })?;
                }
                tx.execute("INSERT INTO runs (run_id,automation_id,revision,principal_authority,principal_user_id,current_attempt_id,retry_of_run_id,occurrence_key,phase,outcome,latest_start_at_ms,source_removed,spec_json,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,?5,NULL,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![r.run_id.as_str(),r.automation_id.as_str(),revision_value(r.revision)?,owner.authority(),owner.external_user_id(),r.retry_of_run_id.as_ref().map(RunId::as_str),r.occurrence_key,text(&r.phase)?,r.outcome.as_ref().map(text).transpose()?,r.latest_start_at_ms,r.source_removed,r.spec.as_ref().map(encode).transpose()?,r.created_at_ms,r.updated_at_ms])?;
            }
            StoreMutation::InsertAttempt(a) => {
                let s = &a.spec;
                require_owner(&s.principal, owner)?;
                s.validate().map_err(|_| {
                    StoreError::Invalid("invalid attempt specification")
                })?;
                let Some(run) = run(&tx, owner, &s.run_id)? else {
                    return Err(StoreError::Reference(
                        "attempt run is missing",
                    ));
                };
                if run.automation_id != s.automation_id
                    || run.revision != s.revision
                {
                    return Err(StoreError::Reference(
                        "attempt revision differs from run",
                    ));
                }
                let (Some(session), Some(submission)) =
                    (&s.session_id, &s.submission_id)
                else {
                    return Err(StoreError::Invalid(
                        "Reminder has no Agent attempt",
                    ));
                };
                tx.execute("INSERT INTO run_attempts (attempt_id,run_id,automation_id,revision,principal_authority,principal_user_id,session_id,submission_id,spec_json,started_at_ms,completion_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![s.attempt_id.as_str(),s.run_id.as_str(),s.automation_id.as_str(),revision_value(s.revision)?,owner.authority(),owner.external_user_id(),session.as_str(),submission.as_str(),encode(s)?,a.started_at_ms,a.completion.as_ref().map(encode).transpose()?])?;
            }
            StoreMutation::SetCurrentAttempt {
                run_id,
                expected,
                attempt_id,
            } => {
                let bound=tx.query_row("SELECT 1 FROM run_attempts WHERE attempt_id=?1 AND run_id=?2 AND principal_authority=?3 AND principal_user_id=?4",params![attempt_id.as_str(),run_id.as_str(),owner.authority(),owner.external_user_id()],|_|Ok(())).optional()?.is_some();
                if !bound {
                    return Err(StoreError::Reference(
                        "current attempt belongs to another run",
                    ));
                }
                let updated=tx.execute("UPDATE runs SET current_attempt_id=?1 WHERE run_id=?2 AND principal_authority=?3 AND principal_user_id=?4 AND current_attempt_id IS ?5",params![attempt_id.as_str(),run_id.as_str(),owner.authority(),owner.external_user_id(),expected.as_ref().map(crate::AttemptId::as_str)])?;
                if updated != 1 {
                    return Err(StoreError::Conflict);
                }
            }
            StoreMutation::InsertReceipt(r) => {
                require_owner(&r.principal, owner)?;
                if r.envelope.expires_at_ms <= r.envelope.issued_at_ms {
                    return Err(StoreError::Invalid(
                        "invalid receipt interval",
                    ));
                }
                if (r.state == ReceiptState::Applied) != r.outcome.is_some() {
                    return Err(StoreError::Invalid(
                        "receipt state and outcome disagree",
                    ));
                }
                let (automation_id, run_id) =
                    outcome_parents(r.outcome.as_ref());
                if let Some(id) = automation_id {
                    require_parent(
                        &tx,
                        "automations",
                        "automation_id",
                        id,
                        owner,
                    )?;
                }
                if let Some(id) = run_id {
                    require_parent(&tx, "runs", "run_id", id, owner)?;
                }
                tx.execute("INSERT INTO command_receipts (principal_authority,principal_user_id,request_key,issued_at_ms,expires_at_ms,content_digest,state,command_json,outcome_json,result_automation_id,result_run_id,source_removed,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",params![owner.authority(),owner.external_user_id(),r.envelope.request_key.as_str(),r.envelope.issued_at_ms,r.envelope.expires_at_ms,r.envelope.content_digest.as_slice(),text(&r.state)?,encode(&r.envelope.command)?,r.outcome.as_ref().map(encode).transpose()?,automation_id,run_id,r.source_removed,r.created_at_ms,r.updated_at_ms])?;
            }
            StoreMutation::InsertOutbox(r) => {
                require_owner(&r.principal, owner)?;
                require_parent(
                    &tx,
                    "runs",
                    "run_id",
                    r.run_id.as_str(),
                    owner,
                )?;
                for value in [&r.delivery_key, &r.target, &r.message_kind] {
                    crate::identity::validate_reference(value).map_err(
                        |_| StoreError::Invalid("invalid delivery token"),
                    )?;
                }
                tx.execute("INSERT INTO delivery_outbox (principal_authority,principal_user_id,delivery_key,run_id,target,message_kind,state,payload_json,next_attempt_at_ms,attempt_count,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![owner.authority(),owner.external_user_id(),r.delivery_key,r.run_id.as_str(),r.target,r.message_kind,text(&r.state)?,encode(&r.payload)?,r.next_attempt_at_ms,i64::from(r.attempt_count),r.created_at_ms,r.updated_at_ms])?;
            }
            StoreMutation::InsertDeletionPlan(r) => {
                require_owner(&r.principal, owner)?;
                require_parent(
                    &tx,
                    "runs",
                    "run_id",
                    r.run_id.as_str(),
                    owner,
                )?;
                tx.execute("INSERT INTO deletion_plans (deletion_plan_id,principal_authority,principal_user_id,run_id,state,plan_json,created_at_ms,updated_at_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![r.deletion_plan_id.as_str(),owner.authority(),owner.external_user_id(),r.run_id.as_str(),text(&r.state)?,encode(&r.plan)?,r.created_at_ms,r.updated_at_ms])?;
            }
            StoreMutation::SetWatermark(r) => {
                require_owner(&r.principal, owner)?;
                require_revision(&tx, owner, &r.automation_id, r.revision)?;
                let previous = watermark(&tx, owner, &r.automation_id)?;
                if previous.as_ref().is_some_and(|value| {
                    value.processed_through_ms > r.processed_through_ms
                }) {
                    return Err(StoreError::Conflict);
                }
                tx.execute("INSERT INTO schedule_watermarks (automation_id,principal_authority,principal_user_id,revision,processed_through_ms,source_removed) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(automation_id) DO UPDATE SET revision=excluded.revision,processed_through_ms=excluded.processed_through_ms,source_removed=excluded.source_removed",params![r.automation_id.as_str(),owner.authority(),owner.external_user_id(),revision_value(r.revision)?,r.processed_through_ms,r.source_removed])?;
            }
        }
    }
    if !super::audit::database_findings(&tx)?.is_empty() {
        return Err(StoreError::Reference(
            "transaction leaves invalid logical references",
        ));
    }
    tx.commit()?;
    Ok(())
}

pub(super) const fn outcome_parents(
    outcome: Option<&CommandOutcome>,
) -> (Option<&str>, Option<&str>) {
    match outcome {
        Some(CommandOutcome::Created { automation_id, .. }) => {
            (Some(automation_id.as_str()), None)
        }
        Some(
            CommandOutcome::RunQueued { run_id }
            | CommandOutcome::CancelRequested { run_id, .. },
        ) => (None, Some(run_id.as_str())),
        None => (None, None),
    }
}

fn one<T>(
    connection: &Connection,
    sql: &str,
    args: impl Params,
    read: impl Fn(&Row<'_>) -> Result<T, StoreError>,
) -> Result<Option<T>, StoreError> {
    let mut statement = connection.prepare(sql)?;
    let mut rows = statement.query(args)?;
    rows.next()?.map(read).transpose()
}
fn all<T>(
    connection: &Connection,
    sql: &str,
    args: impl Params,
    read: impl Fn(&Row<'_>) -> Result<T, StoreError>,
) -> Result<Vec<T>, StoreError> {
    let mut statement = connection.prepare(sql)?;
    let mut rows = statement.query(args)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        result.push(read(row)?);
    }
    Ok(result)
}
pub(super) fn read_automation(
    row: &Row<'_>,
) -> Result<AutomationRecord, StoreError> {
    Ok(AutomationRecord {
        automation_id: business(row, "automation_id")?,
        principal: principal(row)?,
        current_revision: revision(row, "current_revision")?,
        state: enum_value(row.get("state")?)?,
        title: decode(&row.get::<_, String>("title_json")?)?,
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
        source_removed: row.get("source_removed")?,
    })
}
pub(super) fn read_revision(
    row: &Row<'_>,
) -> Result<RevisionRecord, StoreError> {
    Ok(RevisionRecord {
        automation_id: business(row, "automation_id")?,
        principal: principal(row)?,
        revision: revision(row, "revision")?,
        definition: decode(&row.get::<_, String>("definition_json")?)?,
        context: decode(&row.get::<_, String>("context_json")?)?,
        grant_ref: optional_business(row, "grant_ref")?,
        created_at_ms: row.get("created_at_ms")?,
    })
}
pub(super) fn read_run(row: &Row<'_>) -> Result<RunRecord, StoreError> {
    Ok(RunRecord {
        run_id: business(row, "run_id")?,
        automation_id: business(row, "automation_id")?,
        revision: revision(row, "revision")?,
        principal: principal(row)?,
        current_attempt_id: optional_business(row, "current_attempt_id")?,
        retry_of_run_id: optional_business(row, "retry_of_run_id")?,
        occurrence_key: row.get("occurrence_key")?,
        phase: enum_value(row.get("phase")?)?,
        outcome: row
            .get::<_, Option<String>>("outcome")?
            .map(enum_value)
            .transpose()?,
        latest_start_at_ms: row.get("latest_start_at_ms")?,
        source_removed: row.get("source_removed")?,
        spec: row
            .get::<_, Option<String>>("spec_json")?
            .map(|v| decode(&v))
            .transpose()?,
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
    })
}
pub(super) fn read_attempt(row: &Row<'_>) -> Result<AttemptRecord, StoreError> {
    Ok(AttemptRecord {
        spec: decode(&row.get::<_, String>("spec_json")?)?,
        started_at_ms: row.get("started_at_ms")?,
        completion: row
            .get::<_, Option<String>>("completion_json")?
            .map(|v| decode(&v))
            .transpose()?,
    })
}
pub(super) fn read_receipt(row: &Row<'_>) -> Result<ReceiptRecord, StoreError> {
    let digest: Vec<u8> = row.get("content_digest")?;
    Ok(ReceiptRecord {
        principal: principal(row)?,
        envelope: crate::CommandEnvelope {
            request_key: business(row, "request_key")?,
            issued_at_ms: row.get("issued_at_ms")?,
            expires_at_ms: row.get("expires_at_ms")?,
            content_digest: digest
                .try_into()
                .map_err(|_| StoreError::Corrupt("receipt digest length"))?,
            command: decode(&row.get::<_, String>("command_json")?)?,
        },
        state: enum_value(row.get("state")?)?,
        outcome: row
            .get::<_, Option<String>>("outcome_json")?
            .map(|v| decode(&v))
            .transpose()?,
        source_removed: row.get("source_removed")?,
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
    })
}
pub(super) fn read_outbox(row: &Row<'_>) -> Result<OutboxRecord, StoreError> {
    Ok(OutboxRecord {
        principal: principal(row)?,
        delivery_key: row.get("delivery_key")?,
        run_id: business(row, "run_id")?,
        target: row.get("target")?,
        message_kind: row.get("message_kind")?,
        state: enum_value(row.get("state")?)?,
        payload: decode(&row.get::<_, String>("payload_json")?)?,
        next_attempt_at_ms: row.get("next_attempt_at_ms")?,
        attempt_count: row.get("attempt_count")?,
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
    })
}
pub(super) fn read_plan(
    row: &Row<'_>,
) -> Result<DeletionPlanRecord, StoreError> {
    Ok(DeletionPlanRecord {
        deletion_plan_id: business(row, "deletion_plan_id")?,
        principal: principal(row)?,
        run_id: business(row, "run_id")?,
        state: enum_value(row.get("state")?)?,
        plan: decode(&row.get::<_, String>("plan_json")?)?,
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
    })
}
pub(super) fn read_watermark(
    row: &Row<'_>,
) -> Result<WatermarkRecord, StoreError> {
    Ok(WatermarkRecord {
        principal: principal(row)?,
        automation_id: business(row, "automation_id")?,
        revision: revision(row, "revision")?,
        processed_through_ms: row.get("processed_through_ms")?,
        source_removed: row.get("source_removed")?,
    })
}

pub(super) fn automation(
    c: &Connection,
    p: &PrincipalId,
    id: &AutomationId,
) -> Result<Option<AutomationRecord>, StoreError> {
    one(
        c,
        "SELECT * FROM automations WHERE principal_authority=?1 AND principal_user_id=?2 AND automation_id=?3",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_automation,
    )
}
pub(super) fn revisions(
    c: &Connection,
    p: &PrincipalId,
    id: &AutomationId,
) -> Result<Vec<RevisionRecord>, StoreError> {
    all(
        c,
        "SELECT * FROM automation_revisions WHERE principal_authority=?1 AND principal_user_id=?2 AND automation_id=?3 ORDER BY revision",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_revision,
    )
}
pub(super) fn run(
    c: &Connection,
    p: &PrincipalId,
    id: &RunId,
) -> Result<Option<RunRecord>, StoreError> {
    one(
        c,
        "SELECT * FROM runs WHERE principal_authority=?1 AND principal_user_id=?2 AND run_id=?3",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_run,
    )
}
pub(super) fn attempts(
    c: &Connection,
    p: &PrincipalId,
    id: &RunId,
) -> Result<Vec<AttemptRecord>, StoreError> {
    all(
        c,
        "SELECT * FROM run_attempts WHERE principal_authority=?1 AND principal_user_id=?2 AND run_id=?3 ORDER BY started_at_ms",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_attempt,
    )
}
pub(super) fn receipt(
    c: &Connection,
    p: &PrincipalId,
    id: &RequestKey,
) -> Result<Option<ReceiptRecord>, StoreError> {
    one(
        c,
        "SELECT * FROM command_receipts WHERE principal_authority=?1 AND principal_user_id=?2 AND request_key=?3",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_receipt,
    )
}
pub(super) fn outbox(
    c: &Connection,
    p: &PrincipalId,
    id: &RunId,
) -> Result<Vec<OutboxRecord>, StoreError> {
    all(
        c,
        "SELECT * FROM delivery_outbox WHERE principal_authority=?1 AND principal_user_id=?2 AND run_id=?3 ORDER BY created_at_ms",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_outbox,
    )
}
pub(super) fn deletion_plan(
    c: &Connection,
    p: &PrincipalId,
    id: &DeletionPlanId,
) -> Result<Option<DeletionPlanRecord>, StoreError> {
    one(
        c,
        "SELECT * FROM deletion_plans WHERE principal_authority=?1 AND principal_user_id=?2 AND deletion_plan_id=?3",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_plan,
    )
}
pub(super) fn watermark(
    c: &Connection,
    p: &PrincipalId,
    id: &AutomationId,
) -> Result<Option<WatermarkRecord>, StoreError> {
    one(
        c,
        "SELECT * FROM schedule_watermarks WHERE principal_authority=?1 AND principal_user_id=?2 AND automation_id=?3",
        params![p.authority(), p.external_user_id(), id.as_str()],
        read_watermark,
    )
}
