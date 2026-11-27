//! Immutable lineage bootstrap, forward-only migration and validation of the
//! live SQLite schema.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::StoreError;
use crate::schema_contract::{
    DATASET_LINEAGE, LOGICAL_REFERENCES, PRODUCT_TABLES, ReferenceEndpoint,
    validate_product_ddl,
};

pub(super) const SCHEMA_VERSION: u32 = 4;
/// Applied resources are immutable; a new version only appends a migration.
const SCHEMA_001: &str = include_str!("../../schema/001.sql");
const MIGRATION_002: &str = include_str!("../../schema/002.sql");
const MIGRATION_003: &str = include_str!("../../schema/003.sql");
const MIGRATION_004: &str = include_str!("../../schema/004.sql");
pub(super) const SCHEMA_SQL: &str = concat!(
    include_str!("../../schema/001.sql"),
    "\n",
    include_str!("../../schema/002.sql"),
    "\n",
    include_str!("../../schema/003.sql"),
    "\n",
    include_str!("../../schema/004.sql"),
);
/// Test-only crash point: a child process exits before the migration commits.
#[cfg(test)]
const MIGRATION_ABORT_ENV: &str = "FLOWY_AUTOMATION_MIGRATION_TEST_PATH";

/// Existing data is inspected read-only before any setting that can change
/// its journal or main file. A zero-byte existing file is not a new dataset.
pub(super) fn open(path: &Path) -> Result<Connection, StoreError> {
    let exists = match fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(StoreError::Incompatible(
                    "database path is not a file",
                ));
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    let mut applied = SCHEMA_VERSION;
    if exists {
        let inspection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        inspection.busy_timeout(Duration::from_secs(5))?;
        applied = inspect_version(&inspection)?;
        if applied < SCHEMA_VERSION {
            validate_database_references(&inspection)?;
        }
        drop(inspection);
    } else {
        let parent = path
            .parent()
            .ok_or(StoreError::Invalid("database path has no parent"))?;
        fs::create_dir_all(parent)?;
        // Acquire an actual new file rather than treating an existing empty
        // database as our own. The host already owns instance.lock.
        let new_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        new_file.sync_all()?;
    }

    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    if !exists {
        connection.pragma_update(None, "synchronous", "FULL")?;
        initialize(&mut connection)?;
    }
    // Recheck identity on the actual read/write connection, even though the
    // caller's exclusive store owner normally excludes another initializer.
    // The full integrity/structure pass already ran on the inspection handle;
    // only migration needs a fresh full validation afterwards.
    if exists {
        validate_metadata(&connection, applied)?;
    } else {
        validate_version(&connection, applied)?;
    }
    let journal: String =
        connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Unavailable);
    }
    connection.pragma_update(None, "synchronous", "FULL")?;
    let synchronous: u32 =
        connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    if synchronous != 2 {
        return Err(StoreError::Unavailable);
    }
    if applied < SCHEMA_VERSION {
        migrate(&mut connection, applied)?;
        validate_live(&connection)?;
    }
    validate_database_references(&connection)?;
    Ok(connection)
}

fn validate_database_references(
    connection: &Connection,
) -> Result<(), StoreError> {
    if !super::audit::database_findings(connection)?.is_empty() {
        return Err(StoreError::Corrupt("logical reference audit failed"));
    }
    Ok(())
}

fn initialize(connection: &mut Connection) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(&canonical_schema_sql())?;
    transaction.execute(
        "INSERT INTO automation_schema(singleton, lineage, version, schema_sha256) VALUES(1, ?1, ?2, ?3)",
        params![DATASET_LINEAGE, SCHEMA_VERSION, schema_checksum()],
    )?;
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(())
}

/// Reads the applied version and validates the dataset exactly as that
/// version was applied. Unknown, zero or future versions are never touched.
fn inspect_version(connection: &Connection) -> Result<u32, StoreError> {
    let version: u32 =
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 0 || version > SCHEMA_VERSION {
        return Err(StoreError::Incompatible(
            "unknown or future automation schema version",
        ));
    }
    validate_version(connection, version)?;
    Ok(version)
}

/// Forward-only, single transaction: the appended DDL, metadata and
/// `user_version` commit together or the dataset stays at its prior version.
fn migrate(connection: &mut Connection, from: u32) -> Result<(), StoreError> {
    if !matches!(from, 1..=3) {
        return Err(StoreError::Incompatible(
            "no registered automation schema migration path",
        ));
    }
    let previous = checksum_for(from).ok_or(StoreError::Incompatible(
        "unknown automation schema version",
    ))?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if from == 1 {
        transaction.execute_batch(&canonical(MIGRATION_002))?;
    }
    if from < 3 {
        transaction.execute_batch(&canonical(MIGRATION_003))?;
    }
    transaction.execute_batch(&canonical(MIGRATION_004))?;
    let updated = transaction.execute(
        "UPDATE automation_schema SET version=?1, schema_sha256=?2 WHERE singleton=1 AND lineage=?3 AND version=?4 AND schema_sha256=?5",
        params![
            SCHEMA_VERSION,
            schema_checksum(),
            DATASET_LINEAGE,
            from,
            previous
        ],
    )?;
    if updated != 1 {
        return Err(StoreError::Incompatible(
            "automation schema metadata changed during migration",
        ));
    }
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    migration_crash_point();
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
fn migration_crash_point() {
    if std::env::var_os(MIGRATION_ABORT_ENV).is_some() {
        std::process::exit(43);
    }
}

#[cfg(not(test))]
const fn migration_crash_point() {}

fn canonical(sql: &str) -> String {
    // Git may check immutable resources out with CRLF on Windows. The
    // dataset fingerprint and stored DDL must be portable across platforms.
    sql.replace("\r\n", "\n")
}

/// The complete DDL as applied at `version`, or None for unknown versions.
fn schema_for(version: u32) -> Option<String> {
    match version {
        1 => Some(canonical(SCHEMA_001)),
        2 => Some(canonical(&format!("{SCHEMA_001}\n{MIGRATION_002}"))),
        3 => Some(canonical(&format!(
            "{SCHEMA_001}\n{MIGRATION_002}\n{MIGRATION_003}"
        ))),
        SCHEMA_VERSION => Some(canonical_schema_sql()),
        _ => None,
    }
}

fn checksum_for(version: u32) -> Option<String> {
    schema_for(version)
        .map(|sql| format!("{:x}", Sha256::digest(sql.as_bytes())))
}

fn schema_checksum() -> String {
    format!("{:x}", Sha256::digest(canonical_schema_sql().as_bytes()))
}

fn canonical_schema_sql() -> String {
    canonical(SCHEMA_SQL)
}

pub(super) fn validate_live(connection: &Connection) -> Result<(), StoreError> {
    validate_version(connection, SCHEMA_VERSION)
}

fn validate_metadata(
    connection: &Connection,
    expected: u32,
) -> Result<(), StoreError> {
    let expected_checksum = checksum_for(expected).ok_or(
        StoreError::Incompatible("unknown or future automation schema version"),
    )?;
    let version: u32 =
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != expected {
        return Err(StoreError::Incompatible(
            "unknown or future automation schema version",
        ));
    }
    let metadata: Option<(String, u32, String)> = connection
        .query_row(
            "SELECT lineage, version, schema_sha256 FROM automation_schema WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|_| StoreError::Incompatible("automation schema metadata unavailable"))?;
    let Some((lineage, applied_version, checksum)) = metadata else {
        return Err(StoreError::Incompatible(
            "automation schema metadata missing",
        ));
    };
    if lineage != DATASET_LINEAGE
        || applied_version != expected
        || checksum != expected_checksum
    {
        return Err(StoreError::Incompatible(
            "automation lineage or applied schema checksum mismatch",
        ));
    }
    let count: u32 = connection.query_row(
        "SELECT count(*) FROM automation_schema",
        [],
        |row| row.get(0),
    )?;
    if count != 1 {
        return Err(StoreError::Incompatible(
            "automation schema metadata is not singular",
        ));
    }
    Ok(())
}

fn validate_version(
    connection: &Connection,
    expected: u32,
) -> Result<(), StoreError> {
    let mut integrity =
        connection.prepare("PRAGMA integrity_check").map_err(|_| {
            StoreError::Corrupt("SQLite integrity check could not run")
        })?;
    let results = integrity.query_map([], |row| row.get::<_, String>(0))?;
    if results.collect::<Result<Vec<_>, _>>()?.as_slice() != ["ok"] {
        return Err(StoreError::Corrupt("SQLite integrity check failed"));
    }
    validate_metadata(connection, expected)?;
    validate_structure(connection, expected)
}

type SchemaInventory = BTreeMap<(String, String, String), Option<String>>;

fn inventory(connection: &Connection) -> Result<SchemaInventory, StoreError> {
    let mut statement = connection.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY type, name",
    )?;
    let entries = statement
        .query_map([], |row| {
            Ok(((row.get(0)?, row.get(1)?, row.get(2)?), row.get(3)?))
        })?
        .collect::<Result<SchemaInventory, _>>()?;
    Ok(entries)
}

fn validate_structure(
    connection: &Connection,
    expected: u32,
) -> Result<(), StoreError> {
    let applied_sql = schema_for(expected).ok_or(StoreError::Incompatible(
        "unknown or future automation schema version",
    ))?;
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(&applied_sql)?;
    let actual = inventory(connection)?;
    let reference_inventory = inventory(&reference)?;
    if actual != reference_inventory {
        return Err(StoreError::Incompatible(
            "actual SQLite tables, indexes or constraints differ from applied schema",
        ));
    }
    let mut product_ddl = String::new();
    for table in PRODUCT_TABLES {
        let key = (
            "table".to_owned(),
            table.name.to_owned(),
            table.name.to_owned(),
        );
        // A table appended by a later migration is absent from an older
        // applied version; the inventory comparison above already proved
        // the older dataset is exactly its applied schema.
        if expected != SCHEMA_VERSION && !reference_inventory.contains_key(&key)
        {
            continue;
        }
        let sql = actual.get(&key).and_then(Option::as_ref).ok_or(
            StoreError::Incompatible("registered product table is absent"),
        )?;
        product_ddl.push_str(sql);
        product_ddl.push(';');
        let foreign_keys: u32 = connection.query_row(
            "SELECT count(*) FROM pragma_foreign_key_list(?1)",
            [table.name],
            |row| row.get(0),
        )?;
        if foreign_keys != 0 {
            return Err(StoreError::Incompatible(
                "physical foreign keys are forbidden",
            ));
        }
        if table_details(connection, table.name)?
            != table_details(&reference, table.name)?
            || index_details(connection, table.name)?
                != index_details(&reference, table.name)?
        {
            return Err(StoreError::Incompatible(
                "live primary key or index metadata differs",
            ));
        }
    }
    validate_product_ddl(&product_ddl).map_err(|_| {
        StoreError::Incompatible("product DDL violates registered shape")
    })?;
    for relationship in LOGICAL_REFERENCES {
        if let (ReferenceEndpoint::Table { name, .. }, Some(required)) =
            (relationship.child, relationship.required_index)
        {
            let key =
                ("index".to_owned(), required.to_owned(), name.to_owned());
            if expected != SCHEMA_VERSION
                && !reference_inventory.contains_key(&key)
            {
                continue;
            }
            if !actual.contains_key(&key) {
                return Err(StoreError::Incompatible(
                    "registered relationship index is absent",
                ));
            }
        }
    }
    Ok(())
}

type ColumnDetails = (i64, String, String, bool, Option<String>, i64);

fn table_details(
    connection: &Connection,
    table: &str,
) -> Result<Vec<ColumnDetails>, StoreError> {
    let mut statement = connection.prepare("SELECT cid, name, type, [notnull], dflt_value, pk FROM pragma_table_info(?1) ORDER BY cid")?;
    let rows = statement
        .query_map([table], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

type IndexDetails =
    (String, bool, String, bool, Vec<(i64, i64, Option<String>)>);

fn index_details(
    connection: &Connection,
    table: &str,
) -> Result<Vec<IndexDetails>, StoreError> {
    let mut statement = connection.prepare("SELECT name, [unique], origin, partial FROM pragma_index_list(?1) ORDER BY name")?;
    let indexes = statement
        .query_map([table], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    indexes.into_iter().map(|(name, unique, origin, partial)| {
        let mut statement = connection.prepare("SELECT seqno, cid, name FROM pragma_index_info(?1) ORDER BY seqno")?;
        let columns = statement.query_map([&name], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok((name, unique, origin, partial, columns))
    }).collect()
}

#[cfg(test)]
mod tests;
