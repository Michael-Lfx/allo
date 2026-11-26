//! Database references are checked against the live registry. Side stores are
//! explicit trusted inventories; absent or partial inventories never pass.
use super::{StoreError, records::RevisionContext};
use crate::schema_contract::{
    LOGICAL_REFERENCES, OrphanAuditPolicy, ReferenceEndpoint,
    ReferenceValueContract, SideStore,
};
use crate::{
    AttemptId, AutomationId, GrantId, PrincipalId, RunId, SessionRef,
    SubmissionRef,
};
use rusqlite::Connection;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryCompleteness {
    Complete,
    Unavailable,
    Truncated,
}

/// Automation children require explicit ownership. Ordinary sessions may be
/// provided only as reference parents and are never claimed by this audit.
#[derive(Clone, Debug)]
pub struct SideRecord {
    pub store: SideStore,
    /// Ordinary source sessions may be inventoried as parents without being
    /// claimed by automation or subjected to automation child rules.
    pub automation_owned: bool,
    pub principal: Option<PrincipalId>,
    pub automation_id: Option<AutomationId>,
    pub run_id: Option<RunId>,
    pub attempt_id: Option<AttemptId>,
    pub session_id: Option<SessionRef>,
    pub submission_id: Option<SubmissionRef>,
    pub grant_id: Option<GrantId>,
    pub source_removed: bool,
}

#[derive(Clone, Debug)]
pub struct SideStoreInventory {
    /// Exactly one status for each registered `SideStore` is required.
    pub completeness: Vec<(SideStore, InventoryCompleteness)>,
    pub records: Vec<SideRecord>,
}

impl SideStoreInventory {
    pub const fn unavailable() -> Self {
        Self {
            completeness: Vec::new(),
            records: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditFinding {
    /// Registry index, or `LOGICAL_REFERENCES.len()` for whole-record checks.
    /// No technical row ID leaves this module.
    pub reference: usize,
    pub code: &'static str,
    pub count: u64,
}

#[derive(Clone, Debug)]
pub struct AuditReport {
    pub completeness: InventoryCompleteness,
    pub checked_references: usize,
    pub findings: Vec<AuditFinding>,
}

impl AuditReport {
    pub fn is_valid(&self) -> bool {
        self.completeness == InventoryCompleteness::Complete
            && self.findings.is_empty()
    }
}

fn finding(
    result: &mut Vec<AuditFinding>,
    reference: usize,
    code: &'static str,
    count: u64,
) {
    if count > 0 {
        result.push(AuditFinding {
            reference,
            code,
            count,
        });
    }
}

pub(super) fn database_findings(
    connection: &Connection,
) -> Result<Vec<AuditFinding>, StoreError> {
    let mut result = Vec::new();
    for (index, relation) in LOGICAL_REFERENCES.iter().enumerate() {
        let (
            ReferenceEndpoint::Table {
                name: child,
                fields: child_fields,
            },
            ReferenceEndpoint::Table {
                name: parent,
                fields: parent_fields,
            },
        ) = (relation.child, relation.parent)
        else {
            continue;
        };
        if child_fields.len() != parent_fields.len() {
            return Err(StoreError::Invalid("registry relationship arity"));
        }
        let present = child_fields
            .iter()
            .map(|field| format!("c.{field} IS NOT NULL"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let any = child_fields
            .iter()
            .map(|field| format!("c.{field} IS NOT NULL"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let matching = child_fields
            .iter()
            .zip(parent_fields.iter())
            .map(|(a, b)| format!("p.{b}=c.{a}"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let scope = "p.principal_authority=c.principal_authority AND p.principal_user_id=c.principal_user_id";
        let partial = format!(
            "SELECT count(*) FROM {child} c WHERE ({any}) AND NOT ({present})"
        );
        finding(
            &mut result,
            index,
            "partial_reference",
            connection.query_row(&partial, [], |row| row.get(0))?,
        );
        let wrong_owner = format!(
            "SELECT count(*) FROM {child} c WHERE {present} AND EXISTS(SELECT 1 FROM {parent} p WHERE {matching}) AND NOT EXISTS(SELECT 1 FROM {parent} p WHERE {matching} AND {scope})"
        );
        finding(
            &mut result,
            index,
            "principal_mismatch",
            connection.query_row(&wrong_owner, [], |row| row.get(0))?,
        );
        let exemption = if relation.orphan_audit
            == OrphanAuditPolicy::AllowRemovedHistoricalSource
        {
            match child {
                "runs" => {
                    " AND NOT (c.source_removed=1 AND c.phase IN ('settled','interrupted'))"
                }
                "command_receipts" => {
                    " AND NOT (c.source_removed=1 AND c.state IN ('applied','rejected'))"
                }
                "schedule_watermarks" => " AND c.source_removed=0",
                _ => "",
            }
        } else {
            ""
        };
        let missing = format!(
            "SELECT count(*) FROM {child} c WHERE {present} AND NOT EXISTS(SELECT 1 FROM {parent} p WHERE {matching}){exemption}"
        );
        finding(
            &mut result,
            index,
            "missing_parent",
            connection.query_row(&missing, [], |row| row.get(0))?,
        );
        if relation.value_contract
            != ReferenceValueContract::OpaqueHostReference
        {
            let mut statement = connection.prepare(&format!(
                "SELECT {} FROM {child} c WHERE {present}",
                child_fields
                    .iter()
                    .map(|field| format!("c.{field}"))
                    .collect::<Vec<_>>()
                    .join(",")
            ))?;
            let mut rows = statement.query([])?;
            let mut invalid = 0;
            while let Some(row) = rows.next()? {
                let mut valid = true;
                for (offset, field) in child_fields.iter().enumerate() {
                    if *field == "revision" || *field == "current_revision" {
                        valid &= row.get::<_, i64>(offset)? > 0;
                    } else {
                        valid &=
                            AutomationId::parse(row.get::<_, String>(offset)?)
                                .is_ok();
                    }
                }
                if !valid {
                    invalid += 1;
                }
            }
            finding(&mut result, index, "invalid_reference_value", invalid);
        }
    }
    // A current Attempt must belong to this exact Run, not merely its owner.
    let wrong_attempt:u64=connection.query_row("SELECT count(*) FROM runs r JOIN run_attempts a ON a.attempt_id=r.current_attempt_id WHERE a.run_id<>r.run_id OR a.automation_id<>r.automation_id OR a.revision<>r.revision",[],|row|row.get(0))?;
    finding(
        &mut result,
        LOGICAL_REFERENCES.len(),
        "attempt_binding_mismatch",
        wrong_attempt,
    );
    let wrong_revision:u64=connection.query_row("SELECT count(*) FROM run_attempts a JOIN runs r ON r.run_id=a.run_id WHERE a.automation_id<>r.automation_id OR a.revision<>r.revision",[],|row|row.get(0))?;
    finding(
        &mut result,
        LOGICAL_REFERENCES.len(),
        "attempt_revision_mismatch",
        wrong_revision,
    );
    validate_json_bindings(connection, &mut result)?;
    result.extend(super::validation::validate(connection)?);
    Ok(result)
}

fn validate_json_bindings(
    connection: &Connection,
    result: &mut Vec<AuditFinding>,
) -> Result<(), StoreError> {
    let mut statement=connection.prepare("SELECT principal_authority,principal_user_id,automation_id,revision,grant_ref,definition_json FROM automation_revisions")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let input: crate::ManualAutomationInput = super::repository::decode(
            &row.get::<_, String>("definition_json")?,
        )?;
        if super::records::validate_definition(&input).is_err()
            || input.capabilities.grant_ref.as_ref().map(GrantId::as_str)
                != row.get::<_, Option<String>>("grant_ref")?.as_deref()
        {
            finding(
                result,
                LOGICAL_REFERENCES.len(),
                "revision_json_mismatch",
                1,
            );
        }
    }
    let mut statement = connection.prepare("SELECT * FROM run_attempts")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let spec: crate::RunSpec =
            super::repository::decode(&row.get::<_, String>("spec_json")?)?;
        let matches = spec.validate().is_ok()
            && spec.principal.authority()
                == row.get::<_, String>("principal_authority")?
            && spec.principal.external_user_id()
                == row.get::<_, i64>("principal_user_id")?
            && spec.automation_id.as_str()
                == row.get::<_, String>("automation_id")?
            && spec.run_id.as_str() == row.get::<_, String>("run_id")?
            && spec.attempt_id.as_str()
                == row.get::<_, String>("attempt_id")?
            && i64::try_from(spec.revision.get()).ok()
                == Some(row.get("revision")?)
            && spec.session_id.as_ref().map(SessionRef::as_str)
                == Some(row.get::<_, String>("session_id")?.as_str())
            && spec.submission_id.as_ref().map(SubmissionRef::as_str)
                == Some(row.get::<_, String>("submission_id")?.as_str());
        if !matches {
            finding(
                result,
                LOGICAL_REFERENCES.len(),
                "attempt_json_mismatch",
                1,
            );
        }
    }
    let mut statement=connection.prepare("SELECT result_automation_id,result_run_id,outcome_json FROM command_receipts")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let outcome = row
            .get::<_, Option<String>>("outcome_json")?
            .map(|value| {
                super::repository::decode::<crate::CommandOutcome>(&value)
            })
            .transpose()?;
        let (automation, run) =
            super::repository::outcome_parents(outcome.as_ref());
        if automation
            != row
                .get::<_, Option<String>>("result_automation_id")?
                .as_deref()
            || run != row.get::<_, Option<String>>("result_run_id")?.as_deref()
        {
            finding(
                result,
                LOGICAL_REFERENCES.len(),
                "receipt_json_mismatch",
                1,
            );
        }
    }
    Ok(())
}

fn value(record: &SideRecord, field: &str) -> Option<String> {
    match field {
        "automation_id" => record
            .automation_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        "run_id" => record.run_id.as_ref().map(|id| id.as_str().to_owned()),
        "attempt_id" => {
            record.attempt_id.as_ref().map(|id| id.as_str().to_owned())
        }
        "session_id" | "source_session_id" => {
            record.session_id.as_ref().map(|id| id.as_str().to_owned())
        }
        "submission_id" => record
            .submission_id
            .as_ref()
            .map(|id| id.as_str().to_owned()),
        "grant_id" | "grant_ref" => {
            record.grant_id.as_ref().map(|id| id.as_str().to_owned())
        }
        _ => None,
    }
}

fn side_parent(
    inventory: &SideStoreInventory,
    store: SideStore,
    fields: &[&str],
    values: &[String],
    owner: &PrincipalId,
) -> bool {
    inventory
        .records
        .iter()
        .filter(|record| record.store == store)
        .any(|record| {
            record.principal.as_ref() == Some(owner)
                && fields.iter().zip(values).all(|(field, expected)| {
                    value(record, field).as_ref() == Some(expected)
                })
        })
}

fn side_identity_exists(
    inventory: &SideStoreInventory,
    store: SideStore,
    fields: &[&str],
    values: &[String],
) -> bool {
    inventory.records.iter().any(|record| {
        record.store == store
            && fields.iter().zip(values).all(|(field, expected)| {
                value(record, field).as_ref() == Some(expected)
            })
    })
}

pub(super) fn audit(
    connection: &Connection,
    inventory: &SideStoreInventory,
) -> Result<AuditReport, StoreError> {
    let mut findings = database_findings(connection)?;
    findings.extend(super::validation::validate_side_bindings(
        connection, inventory,
    )?);
    for record in inventory
        .records
        .iter()
        .filter(|record| record.automation_owned)
    {
        if record.principal.is_none() {
            finding(
                &mut findings,
                LOGICAL_REFERENCES.len(),
                "unknown_side_owner",
                1,
            );
        }
    }
    let mut completeness = InventoryCompleteness::Complete;
    for store in [
        SideStore::AgentHistory,
        SideStore::AgentIndex,
        SideStore::ArtifactManifest,
        SideStore::GrantLedger,
        SideStore::PrivateWorkspace,
    ] {
        let entries = inventory
            .completeness
            .iter()
            .filter(|(candidate, _)| *candidate == store)
            .collect::<Vec<_>>();
        if entries.len() != 1 || entries[0].1 != InventoryCompleteness::Complete
        {
            completeness = if entries.len() == 1
                && entries[0].1 == InventoryCompleteness::Truncated
                && completeness != InventoryCompleteness::Unavailable
            {
                InventoryCompleteness::Truncated
            } else {
                InventoryCompleteness::Unavailable
            };
            finding(
                &mut findings,
                LOGICAL_REFERENCES.len(),
                "incomplete_side_store_inventory",
                1,
            );
        }
    }
    for (index, relation) in LOGICAL_REFERENCES.iter().enumerate() {
        match (relation.child, relation.parent) {
            (
                ReferenceEndpoint::Table { name, fields },
                ReferenceEndpoint::SideStore {
                    store,
                    fields: parent_fields,
                },
            ) => {
                let sql = format!(
                    "SELECT principal_authority,principal_user_id,{} FROM {name}",
                    fields.join(",")
                );
                let mut statement = connection.prepare(&sql)?;
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    let owner = PrincipalId::cloud(
                        row.get::<_, String>(0)?,
                        row.get(1)?,
                    )
                    .map_err(|_| {
                        StoreError::Corrupt("invalid audit principal")
                    })?;
                    let values = (0..fields.len())
                        .map(|offset| row.get::<_, Option<String>>(offset + 2))
                        .collect::<Result<Vec<_>, _>>()?;
                    if values.iter().all(Option::is_none) && relation.nullable {
                        continue;
                    }
                    let Some(values) =
                        values.into_iter().collect::<Option<Vec<_>>>()
                    else {
                        finding(
                            &mut findings,
                            index,
                            "missing_side_reference_value",
                            1,
                        );
                        continue;
                    };
                    if !side_parent(
                        inventory,
                        store,
                        parent_fields,
                        &values,
                        &owner,
                    ) {
                        finding(
                            &mut findings,
                            index,
                            "missing_or_wrong_owner_side_parent",
                            1,
                        );
                    }
                }
            }
            (
                ReferenceEndpoint::JsonColumn { table, column, .. },
                ReferenceEndpoint::SideStore { store, fields },
            ) => {
                let mut statement=connection.prepare(&format!("SELECT principal_authority,principal_user_id,{column} FROM {table}"))?;
                let mut rows = statement.query([])?;
                while let Some(row) = rows.next()? {
                    let owner = PrincipalId::cloud(
                        row.get::<_, String>(0)?,
                        row.get(1)?,
                    )
                    .map_err(|_| {
                        StoreError::Corrupt("invalid audit principal")
                    })?;
                    let context: RevisionContext =
                        super::repository::decode(&row.get::<_, String>(2)?)?;
                    if let Some(source) = context.source_session_id
                        && !side_parent(
                            inventory,
                            store,
                            fields,
                            &[source.as_str().to_owned()],
                            &owner,
                        )
                        && (!context.source_removed
                            || side_identity_exists(
                                inventory,
                                store,
                                fields,
                                &[source.as_str().to_owned()],
                            ))
                    {
                        finding(
                            &mut findings,
                            index,
                            "missing_context_source",
                            1,
                        );
                    }
                }
            }
            (
                ReferenceEndpoint::SideStore { store, fields },
                ReferenceEndpoint::Table {
                    name,
                    fields: parent_fields,
                },
            ) => {
                for record in inventory.records.iter().filter(|record| {
                    record.store == store && record.automation_owned
                }) {
                    let Some(owner) = &record.principal else {
                        finding(&mut findings, index, "unknown_side_owner", 1);
                        continue;
                    };
                    let Some(values) = fields
                        .iter()
                        .map(|field| value(record, field))
                        .collect::<Option<Vec<_>>>()
                    else {
                        finding(
                            &mut findings,
                            index,
                            "incomplete_side_binding",
                            1,
                        );
                        continue;
                    };
                    let predicate = parent_fields
                        .iter()
                        .enumerate()
                        .map(|(offset, field)| {
                            format!("{field}=?{}", offset + 3)
                        })
                        .collect::<Vec<_>>()
                        .join(" AND ");
                    let sql = format!(
                        "SELECT count(*) FROM {name} WHERE principal_authority=?1 AND principal_user_id=?2 AND {predicate}"
                    );
                    let mut bindings = vec![
                        rusqlite::types::Value::Text(
                            owner.authority().to_owned(),
                        ),
                        rusqlite::types::Value::Integer(
                            owner.external_user_id(),
                        ),
                    ];
                    bindings.extend(
                        values.into_iter().map(rusqlite::types::Value::Text),
                    );
                    let count: u64 = connection.query_row(
                        &sql,
                        rusqlite::params_from_iter(bindings),
                        |row| row.get(0),
                    )?;
                    if count != 1 {
                        finding(
                            &mut findings,
                            index,
                            "missing_or_wrong_owner_database_parent",
                            1,
                        );
                    }
                }
            }
            (
                ReferenceEndpoint::SideStore { store, fields },
                ReferenceEndpoint::SideStore {
                    store: parent,
                    fields: parent_fields,
                },
            ) => {
                for record in inventory.records.iter().filter(|record| {
                    record.store == store && record.automation_owned
                }) {
                    let Some(owner) = &record.principal else {
                        finding(&mut findings, index, "unknown_side_owner", 1);
                        continue;
                    };
                    let Some(values) = fields
                        .iter()
                        .map(|field| value(record, field))
                        .collect::<Option<Vec<_>>>()
                    else {
                        finding(
                            &mut findings,
                            index,
                            "incomplete_side_binding",
                            1,
                        );
                        continue;
                    };
                    if !(side_parent(
                        inventory,
                        parent,
                        parent_fields,
                        &values,
                        owner,
                    ) || (relation.orphan_audit
                        == OrphanAuditPolicy::AllowRemovedHistoricalSource
                        && record.source_removed
                        && !side_identity_exists(
                            inventory,
                            parent,
                            parent_fields,
                            &values,
                        )))
                    {
                        finding(
                            &mut findings,
                            index,
                            "missing_or_wrong_owner_side_parent",
                            1,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    Ok(AuditReport {
        completeness,
        checked_references: LOGICAL_REFERENCES.len(),
        findings,
    })
}
