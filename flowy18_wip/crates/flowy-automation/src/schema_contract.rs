//! Automation dataset contract.
//!
//! This inventory records the intended schema and side-store audits. It does
//! not validate a live database; PA-B02 checks the actual SQLite schema.
//! Side-store entries apply only to explicit automation-owned records. Unknown
//! owners stop execution; ordinary conversations are not reassigned.

pub const DATASET_LINEAGE: &str = "flowy-gpui-automation-v1";
pub const DATABASE_RELATIVE_PATH: &str = "automation/automations.sqlite3";
pub const TECHNICAL_PRIMARY_KEY: &str = "id";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DdlShapeViolation {
    #[error("physical foreign keys are forbidden")]
    ForeignKey,
    #[error("REFERENCES clauses are forbidden")]
    References,
    #[error("SQLite triggers are forbidden")]
    Trigger,
    #[error("database cascades are forbidden")]
    Cascade,
    #[error("technical row IDs cannot be referenced as *_row_id")]
    RowIdReference,
    #[error("DDL contains no product CREATE TABLE statement")]
    MissingProductTable,
    #[error(
        "each product table must start with id INTEGER PRIMARY KEY AUTOINCREMENT"
    )]
    InvalidTechnicalPrimaryKey,
}

#[derive(Debug)]
enum SqlToken {
    Word(String),
    QuotedIdentifier(String),
    Symbol(char),
}

impl SqlToken {
    fn is_keyword(&self, expected: &str) -> bool {
        matches!(self, Self::Word(value) if value == expected)
    }

    fn identifier(&self) -> Option<&str> {
        match self {
            Self::Word(value) | Self::QuotedIdentifier(value) => Some(value),
            Self::Symbol(_) => None,
        }
    }

    const fn is_symbol(&self, expected: char) -> bool {
        matches!(self, Self::Symbol(value) if *value == expected)
    }
}

/// Checks SQLite DDL against the registered shape.
///
/// It recognizes CREATE TABLE statements and relevant constraint tokens.
/// PA-B02 must still validate the actual database schema.
pub fn validate_product_ddl(ddl: &str) -> Result<(), DdlShapeViolation> {
    let tokens = sql_tokens(ddl);
    let mut table_count = 0;

    for index in 0..tokens.len() {
        if tokens[index].is_keyword("foreign")
            && tokens
                .get(index + 1)
                .is_some_and(|token| token.is_keyword("key"))
        {
            return Err(DdlShapeViolation::ForeignKey);
        }
        if tokens[index].is_keyword("references") {
            return Err(DdlShapeViolation::References);
        }
        if tokens[index]
            .identifier()
            .is_some_and(|name| name.ends_with("_row_id"))
        {
            return Err(DdlShapeViolation::RowIdReference);
        }

        if tokens[index].is_keyword("on")
            && tokens.get(index + 1).is_some_and(|token| {
                token.is_keyword("delete") || token.is_keyword("update")
            })
            && tokens
                .get(index + 2)
                .is_some_and(|token| token.is_keyword("cascade"))
        {
            return Err(DdlShapeViolation::Cascade);
        }

        if !tokens[index].is_keyword("create") {
            continue;
        }
        let mut kind = index + 1;
        if tokens.get(kind).is_some_and(|token| {
            token.is_keyword("temp") || token.is_keyword("temporary")
        }) {
            kind += 1;
        }
        if tokens
            .get(kind)
            .is_some_and(|token| token.is_keyword("trigger"))
        {
            return Err(DdlShapeViolation::Trigger);
        }
        if !tokens
            .get(kind)
            .is_some_and(|token| token.is_keyword("table"))
        {
            continue;
        }

        table_count += 1;
        let Some(open) = (kind + 1..tokens.len()).find(|candidate| {
            tokens[*candidate].is_symbol('(')
                || tokens[*candidate].is_symbol(';')
        }) else {
            return Err(DdlShapeViolation::InvalidTechnicalPrimaryKey);
        };
        if !tokens[open].is_symbol('(') {
            return Err(DdlShapeViolation::InvalidTechnicalPrimaryKey);
        }
        let primary_key = ["integer", "primary", "key", "autoincrement"];
        let id_is_first = tokens
            .get(open + 1)
            .and_then(SqlToken::identifier)
            .is_some_and(|name| name == "id");
        let declaration_matches =
            primary_key.iter().enumerate().all(|(offset, word)| {
                tokens
                    .get(open + offset + 2)
                    .is_some_and(|token| token.is_keyword(word))
            });
        let declaration_ends = tokens
            .get(open + 6)
            .is_some_and(|token| token.is_symbol(',') || token.is_symbol(')'));
        if !id_is_first || !declaration_matches || !declaration_ends {
            return Err(DdlShapeViolation::InvalidTechnicalPrimaryKey);
        }
    }

    if table_count == 0 {
        return Err(DdlShapeViolation::MissingProductTable);
    }
    Ok(())
}

fn sql_tokens(sql: &str) -> Vec<SqlToken> {
    let chars = sql.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '-' if chars.get(index + 1) == Some(&'-') => {
                index += 2;
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '/' if chars.get(index + 1) == Some(&'*') => {
                index += 2;
                while index + 1 < chars.len()
                    && !(chars[index] == '*' && chars[index + 1] == '/')
                {
                    index += 1;
                }
                index = (index + 2).min(chars.len());
            }
            quote @ ('\'' | '"' | '`') => {
                index += 1;
                let mut quoted = String::new();
                while index < chars.len() {
                    if chars[index] == quote {
                        if chars.get(index + 1) == Some(&quote) {
                            quoted.push(quote);
                            index += 2;
                        } else {
                            index += 1;
                            break;
                        }
                    } else {
                        quoted.push(chars[index]);
                        index += 1;
                    }
                }
                if quote == '\'' {
                    continue;
                }
                tokens.push(SqlToken::QuotedIdentifier(
                    quoted.to_ascii_lowercase(),
                ));
            }
            '[' => {
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != ']' {
                    index += 1;
                }
                let quoted = chars[start..index].iter().collect::<String>();
                index = (index + 1).min(chars.len());
                tokens.push(SqlToken::QuotedIdentifier(
                    quoted.to_ascii_lowercase(),
                ));
            }
            ch if ch.is_ascii_alphanumeric() || ch == '_' => {
                let start = index;
                index += 1;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric()
                        || chars[index] == '_')
                {
                    index += 1;
                }
                tokens.push(SqlToken::Word(
                    chars[start..index]
                        .iter()
                        .collect::<String>()
                        .to_ascii_lowercase(),
                ));
            }
            symbol => {
                if symbol.is_ascii_punctuation() {
                    tokens.push(SqlToken::Symbol(symbol));
                }
                index += 1;
            }
        }
    }
    tokens
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductTable {
    pub name: &'static str,
    /// Local i64 autoincrement primary key; never exported or referenced.
    pub technical_primary_key: &'static str,
    pub business_key: Option<&'static str>,
    pub natural_unique: &'static [&'static str],
}

pub const PRODUCT_TABLES: &[ProductTable] = &[
    ProductTable {
        name: "automations",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: Some("automation_id"),
        natural_unique: &[],
    },
    ProductTable {
        name: "automation_revisions",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: None,
        natural_unique: &["automation_id", "revision"],
    },
    ProductTable {
        name: "runs",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: Some("run_id"),
        natural_unique: &["occurrence_key"],
    },
    ProductTable {
        name: "run_attempts",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: Some("attempt_id"),
        natural_unique: &["session_id"],
    },
    ProductTable {
        name: "command_receipts",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: None,
        natural_unique: &[
            "principal_authority",
            "principal_user_id",
            "request_key",
        ],
    },
    ProductTable {
        name: "delivery_outbox",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: None,
        natural_unique: &["delivery_key"],
    },
    ProductTable {
        name: "deletion_plans",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: Some("deletion_plan_id"),
        natural_unique: &[],
    },
    ProductTable {
        name: "schedule_watermarks",
        technical_primary_key: TECHNICAL_PRIMARY_KEY,
        business_key: None,
        natural_unique: &["automation_id"],
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideStore {
    AgentHistory,
    AgentIndex,
    ArtifactManifest,
    GrantLedger,
    PrivateWorkspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceEndpoint {
    Table {
        name: &'static str,
        fields: &'static [&'static str],
    },
    JsonColumn {
        table: &'static str,
        column: &'static str,
        fields: &'static [&'static str],
    },
    SideStore {
        store: SideStore,
        fields: &'static [&'static str],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceValueContract {
    CanonicalUuidV7,
    UuidV7AndRevision,
    OpaqueHostReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletePolicy {
    Restrict,
    CascadeInService,
    SetNull,
    KeepHistory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebuildPolicy {
    PreserveBusinessIdentity,
    ReconcileSideStore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrphanAuditPolicy {
    RequireParent,
    /// Only an explicitly source-removed, settled summary may retain a missing
    /// historical parent. Active or un-compacted rows still require it.
    AllowRemovedHistoricalSource,
    StopExecutionUntilReconciled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalReference {
    pub child: ReferenceEndpoint,
    pub parent: ReferenceEndpoint,
    pub value_contract: ReferenceValueContract,
    /// Every relation also validates the authority + external user ID scope.
    pub same_principal: bool,
    pub nullable: bool,
    /// Required for database children; side-store checks use bounded inventory.
    pub required_index: Option<&'static str>,
    pub delete_policy: DeletePolicy,
    pub rebuild_policy: RebuildPolicy,
    pub orphan_audit: OrphanAuditPolicy,
}

const fn table(
    name: &'static str,
    fields: &'static [&'static str],
) -> ReferenceEndpoint {
    ReferenceEndpoint::Table { name, fields }
}

const fn side(
    store: SideStore,
    fields: &'static [&'static str],
) -> ReferenceEndpoint {
    ReferenceEndpoint::SideStore { store, fields }
}

pub const LOGICAL_REFERENCES: &[LogicalReference] = &[
    LogicalReference {
        child: table("runs", &["current_attempt_id"]),
        parent: table("run_attempts", &["attempt_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: true,
        required_index: Some("runs_current_attempt"),
        delete_policy: DeletePolicy::Restrict,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::RequireParent,
    },
    LogicalReference {
        child: table("schedule_watermarks", &["automation_id", "revision"]),
        parent: table("automation_revisions", &["automation_id", "revision"]),
        value_contract: ReferenceValueContract::UuidV7AndRevision,
        same_principal: true,
        nullable: false,
        required_index: Some("schedule_watermarks_owner_automation"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: table("automations", &["automation_id", "current_revision"]),
        parent: table("automation_revisions", &["automation_id", "revision"]),
        value_contract: ReferenceValueContract::UuidV7AndRevision,
        same_principal: true,
        nullable: false,
        required_index: Some("automations_owner_revision"),
        delete_policy: DeletePolicy::Restrict,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::RequireParent,
    },
    LogicalReference {
        child: table("runs", &["retry_of_run_id"]),
        parent: table("runs", &["run_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: true,
        required_index: Some("runs_owner_retry"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: ReferenceEndpoint::JsonColumn {
            table: "automation_revisions",
            column: "context_json",
            fields: &["source_session_id"],
        },
        parent: side(SideStore::AgentHistory, &["session_id"]),
        value_contract: ReferenceValueContract::OpaqueHostReference,
        same_principal: true,
        nullable: true,
        required_index: None,
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::ReconcileSideStore,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: table("automation_revisions", &["automation_id"]),
        parent: table("automations", &["automation_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: false,
        required_index: Some("automation_revisions_owner_revision"),
        delete_policy: DeletePolicy::Restrict,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::RequireParent,
    },
    LogicalReference {
        child: table("runs", &["automation_id", "revision"]),
        parent: table("automation_revisions", &["automation_id", "revision"]),
        value_contract: ReferenceValueContract::UuidV7AndRevision,
        same_principal: true,
        nullable: false,
        required_index: Some("runs_owner_revision"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: table("run_attempts", &["run_id"]),
        parent: table("runs", &["run_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: false,
        required_index: Some("run_attempts_owner_run"),
        delete_policy: DeletePolicy::CascadeInService,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::RequireParent,
    },
    LogicalReference {
        child: table("delivery_outbox", &["run_id"]),
        parent: table("runs", &["run_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: false,
        required_index: Some("delivery_outbox_owner_run"),
        delete_policy: DeletePolicy::Restrict,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::RequireParent,
    },
    LogicalReference {
        child: table("command_receipts", &["result_automation_id"]),
        parent: table("automations", &["automation_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: true,
        required_index: Some("command_receipts_owner_automation"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: table("command_receipts", &["result_run_id"]),
        parent: table("runs", &["run_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: true,
        required_index: Some("command_receipts_owner_run"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: table("deletion_plans", &["run_id"]),
        parent: table("runs", &["run_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: false,
        required_index: Some("deletion_plans_owner_run"),
        delete_policy: DeletePolicy::Restrict,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::RequireParent,
    },
    LogicalReference {
        child: table("schedule_watermarks", &["automation_id"]),
        parent: table("automations", &["automation_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: false,
        required_index: Some("schedule_watermarks_owner_automation"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::PreserveBusinessIdentity,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: table("automation_revisions", &["grant_ref"]),
        parent: side(SideStore::GrantLedger, &["grant_id"]),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: true,
        required_index: Some("automation_revisions_owner_grant"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::ReconcileSideStore,
        orphan_audit: OrphanAuditPolicy::StopExecutionUntilReconciled,
    },
    LogicalReference {
        child: table("run_attempts", &["session_id", "submission_id"]),
        parent: side(SideStore::AgentHistory, &["session_id", "submission_id"]),
        value_contract: ReferenceValueContract::OpaqueHostReference,
        same_principal: true,
        nullable: false,
        required_index: Some("run_attempts_owner_session_submission"),
        delete_policy: DeletePolicy::KeepHistory,
        rebuild_policy: RebuildPolicy::ReconcileSideStore,
        orphan_audit: OrphanAuditPolicy::StopExecutionUntilReconciled,
    },
    LogicalReference {
        child: side(
            SideStore::AgentHistory,
            &["automation_id", "run_id", "attempt_id"],
        ),
        parent: table(
            "run_attempts",
            &["automation_id", "run_id", "attempt_id"],
        ),
        value_contract: ReferenceValueContract::CanonicalUuidV7,
        same_principal: true,
        nullable: false,
        required_index: None,
        delete_policy: DeletePolicy::CascadeInService,
        rebuild_policy: RebuildPolicy::ReconcileSideStore,
        orphan_audit: OrphanAuditPolicy::StopExecutionUntilReconciled,
    },
    LogicalReference {
        child: side(SideStore::AgentIndex, &["session_id"]),
        parent: side(SideStore::AgentHistory, &["session_id"]),
        value_contract: ReferenceValueContract::OpaqueHostReference,
        same_principal: true,
        nullable: false,
        required_index: None,
        delete_policy: DeletePolicy::CascadeInService,
        rebuild_policy: RebuildPolicy::ReconcileSideStore,
        orphan_audit: OrphanAuditPolicy::AllowRemovedHistoricalSource,
    },
    LogicalReference {
        child: side(
            SideStore::ArtifactManifest,
            &["session_id", "submission_id"],
        ),
        parent: side(SideStore::AgentHistory, &["session_id", "submission_id"]),
        value_contract: ReferenceValueContract::OpaqueHostReference,
        same_principal: true,
        nullable: false,
        required_index: None,
        delete_policy: DeletePolicy::CascadeInService,
        rebuild_policy: RebuildPolicy::ReconcileSideStore,
        orphan_audit: OrphanAuditPolicy::StopExecutionUntilReconciled,
    },
    LogicalReference {
        child: side(SideStore::PrivateWorkspace, &["session_id"]),
        parent: side(SideStore::AgentHistory, &["session_id"]),
        value_contract: ReferenceValueContract::OpaqueHostReference,
        same_principal: true,
        nullable: false,
        required_index: None,
        delete_policy: DeletePolicy::CascadeInService,
        rebuild_policy: RebuildPolicy::ReconcileSideStore,
        orphan_audit: OrphanAuditPolicy::StopExecutionUntilReconciled,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn target_inventory_has_local_keys_and_concrete_scoped_reference_policies()
    {
        let names = PRODUCT_TABLES
            .iter()
            .map(|table| table.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), 8);
        assert!(
            PRODUCT_TABLES
                .iter()
                .all(|table| table.technical_primary_key == "id")
        );
        assert!(LOGICAL_REFERENCES.len() >= 17);
        for reference in LOGICAL_REFERENCES {
            assert!(reference.same_principal);
            for endpoint in [reference.child, reference.parent] {
                if let ReferenceEndpoint::Table { name, fields } = endpoint {
                    assert!(names.contains(name));
                    assert_ne!(fields.len(), 0);
                    assert!(!fields.contains(&"id"));
                }
                if let ReferenceEndpoint::JsonColumn {
                    table,
                    column,
                    fields,
                } = endpoint
                {
                    assert!(names.contains(table));
                    assert_ne!(column, "");
                    assert_ne!(fields.len(), 0);
                }
            }
            if matches!(reference.child, ReferenceEndpoint::Table { .. }) {
                assert!(reference.required_index.is_some());
            }
        }
        assert!(LOGICAL_REFERENCES.iter().any(|reference| matches!(
            reference.parent,
            ReferenceEndpoint::SideStore {
                store: SideStore::GrantLedger,
                ..
            }
        )));
    }

    #[test]
    fn product_ddl_rejects_forbidden_shapes_and_ignores_comments_and_literals()
    {
        let valid = "CREATE TABLE runs (id INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL, note TEXT DEFAULT 'FOREIGN KEY REFERENCES'); -- REFERENCES are forbidden";
        assert_eq!(validate_product_ddl(valid), Ok(()));
        assert_eq!(
            validate_product_ddl(
                "CREATE TEMP TABLE \"foreign key\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT, \"references\" TEXT)"
            ),
            Ok(())
        );
        assert_eq!(
            validate_product_ddl(
                "CREATE TEMPORARY TABLE [runs] ([id] INTEGER PRIMARY KEY AUTOINCREMENT)"
            ),
            Ok(())
        );
        for (ddl, expected) in [
            (
                "CREATE TABLE child (id INTEGER PRIMARY KEY AUTOINCREMENT, parent_id TEXT, FOREIGN KEY(parent_id) REFERENCES parent(id))",
                DdlShapeViolation::ForeignKey,
            ),
            (
                "CREATE TABLE child (id INTEGER PRIMARY KEY AUTOINCREMENT, parent_id TEXT REFERENCES parent(id))",
                DdlShapeViolation::References,
            ),
            (
                "CREATE TEMP TRIGGER IF NOT EXISTS child_cleanup AFTER DELETE ON child BEGIN SELECT 1; END",
                DdlShapeViolation::Trigger,
            ),
            (
                "CREATE TABLE child (id INTEGER PRIMARY KEY AUTOINCREMENT, parent_id TEXT ON /* split tokens */ DELETE CASCADE)",
                DdlShapeViolation::Cascade,
            ),
            (
                "CREATE TABLE child (id INTEGER PRIMARY KEY AUTOINCREMENT, \"parent_row_id\" INTEGER)",
                DdlShapeViolation::RowIdReference,
            ),
            (
                "CREATE TABLE child (id INTEGER PRIMARY KEY)",
                DdlShapeViolation::InvalidTechnicalPrimaryKey,
            ),
            (
                "CREATE TABLE child (other INTEGER, id INTEGER PRIMARY KEY AUTOINCREMENT)",
                DdlShapeViolation::InvalidTechnicalPrimaryKey,
            ),
        ] {
            assert_eq!(validate_product_ddl(ddl), Err(expected), "{ddl}");
        }
        assert_eq!(
            validate_product_ddl("CREATE INDEX runs_by_id ON runs(run_id)"),
            Err(DdlShapeViolation::MissingProductTable)
        );
    }

    #[test]
    fn production_dependencies_stay_inside_the_host_independent_allowlist() {
        const MANIFEST: &str = include_str!("../Cargo.toml");
        assert_eq!(validate_runtime_dependencies(MANIFEST), Ok(()));

        let forbidden = MANIFEST.replace(
            "[dependencies]",
            "[dependencies]\nflowy-agent = { path = \"../flowy-agent\" }",
        );
        assert_eq!(
            validate_runtime_dependencies(&forbidden),
            Err(DependencyContractViolation::Forbidden("flowy-agent".into()))
        );

        let aliased = MANIFEST.replace(
            "serde = { workspace = true }",
            "serde = { package = \"gpui-pre\", workspace = true }",
        );
        assert_eq!(
            validate_runtime_dependencies(&aliased),
            Err(DependencyContractViolation::Forbidden("gpui-pre".into()))
        );

        let target_specific = MANIFEST.replace(
            "[dependencies]",
            "[target.'cfg(windows)'.dependencies]\nuuid = { workspace = true }\n[dependencies]",
        );
        assert!(matches!(
            validate_runtime_dependencies(&target_specific),
            Err(DependencyContractViolation::Forbidden(section))
                if section == "[target.'cfg(windows)'.dependencies]"
        ));

        let network_runtime_dependency = MANIFEST.replace(
            "[dependencies]",
            "[dependencies]\nreqwest = { workspace = true }",
        );
        assert_eq!(
            validate_runtime_dependencies(&network_runtime_dependency),
            Err(DependencyContractViolation::Forbidden("reqwest".into()))
        );
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum DependencyContractViolation {
        MissingDependencies,
        MalformedManifest,
        Forbidden(String),
    }

    fn validate_runtime_dependencies(
        manifest: &str,
    ) -> Result<(), DependencyContractViolation> {
        // SQLite owns durable data on a dedicated thread; Tokio is used only
        // for asynchronous replies, with the runtime supplied by the host.
        const RUNTIME_ALLOWED: &[&str] = &[
            "chrono",
            "chrono-tz",
            "serde",
            "thiserror",
            "uuid",
            "rusqlite",
            "serde_json",
            "sha2",
            "tokio",
        ];
        const DEV_ALLOWED: &[&str] = &["serde_json"];
        let mut dependency_section = None;
        let mut found_runtime_dependencies = false;
        for raw_line in manifest.lines() {
            let line = raw_line.split('#').next().unwrap_or_default().trim();
            if line.starts_with('[') && line.ends_with(']') {
                dependency_section = match line {
                    "[dependencies]" => {
                        found_runtime_dependencies = true;
                        Some(RUNTIME_ALLOWED)
                    }
                    "[dev-dependencies]" => Some(DEV_ALLOWED),
                    _ if line.contains("dependencies") => {
                        return Err(DependencyContractViolation::Forbidden(
                            line.to_owned(),
                        ));
                    }
                    _ => None,
                };
                continue;
            }
            let Some(allowed) = dependency_section else {
                continue;
            };
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(DependencyContractViolation::MalformedManifest);
            };
            let key = key.trim().trim_matches('\"');
            if !allowed.contains(&key) {
                return Err(DependencyContractViolation::Forbidden(
                    key.to_owned(),
                ));
            }
            if value.matches('{').count() != value.matches('}').count() {
                return Err(DependencyContractViolation::MalformedManifest);
            }
            for field in value.split(',') {
                let Some((field_key, field_value)) = field.split_once('=')
                else {
                    continue;
                };
                if field_key.trim().trim_start_matches('{').trim() != "package"
                {
                    continue;
                }
                let package = field_value
                    .trim()
                    .trim_matches(&['\"', ' ', '{', '}', ','][..]);
                if !allowed.contains(&package) {
                    return Err(DependencyContractViolation::Forbidden(
                        package.to_owned(),
                    ));
                }
            }
        }
        if !found_runtime_dependencies {
            return Err(DependencyContractViolation::MissingDependencies);
        }
        Ok(())
    }
}
