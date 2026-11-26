use super::*;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const FIXTURE_AUTOMATION: &str = "0190f5fe-7c00-7a00-8000-000000000003";
const CHILD_PATH: &str = "FLOWY_AUTOMATION_SCHEMA_TEST_PATH";
const CHILD_MODE: &str = "FLOWY_AUTOMATION_SCHEMA_TEST_MODE";

struct TestDatabase {
    root: PathBuf,
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let root = std::env::temp_dir().join(format!(
                "flowy-automation-schema-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&root) {
                Ok(()) => {
                    let path = root.join("automations.sqlite3");
                    return Self { root, path };
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    panic!("create isolated schema test root: {error}")
                }
            }
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        // This directory was acquired exclusively by create_dir above; each
        // test closes every connection and waits for its child before Drop.
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn insert_fixture(connection: &Connection) {
    connection.execute(
        r#"INSERT INTO automations(automation_id,principal_authority,principal_user_id,current_revision,state,title_json,created_at_ms,updated_at_ms) VALUES(?1,'schema.fixture',7,1,'enabled','"fixture"',0,0)"#,
        [FIXTURE_AUTOMATION],
    ).unwrap();
    connection.execute(
        "INSERT INTO automation_revisions(automation_id,principal_authority,principal_user_id,revision,definition_json,context_json,created_at_ms) VALUES(?1,'schema.fixture',7,1,?2,'{}',0)",
        params![FIXTURE_AUTOMATION, r#"{"title":"fixture","action":{"kind":"reminder","text":"fixture reminder"},"workspace":null,"capabilities":{"tools":[],"resources":[],"grant_ref":null},"model":null,"budget":null}"#],
    ).unwrap();
}

#[test]
fn new_database_has_registered_tables_indexes_and_durable_pragmas() {
    let database = TestDatabase::new();
    let connection = open(&database.path).unwrap();
    let tables: u32 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT GLOB 'sqlite_*' AND name <> 'automation_schema'", [], |row| row.get(0),
    ).unwrap();
    assert_eq!(tables, 8);
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    let synchronous: u32 = connection
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    assert_eq!(synchronous, 2);
    for table in PRODUCT_TABLES {
        let first_column: (String, String, u32) = connection
            .query_row(
                "SELECT name,type,pk FROM pragma_table_info(?1) WHERE cid=0",
                [table.name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(first_column, ("id".to_owned(), "INTEGER".to_owned(), 1));
        let physical_references: u32 = connection
            .query_row(
                "SELECT count(*) FROM pragma_foreign_key_list(?1)",
                [table.name],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(physical_references, 0);
    }
    let triggers: u32 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='trigger'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(triggers, 0);
    validate_live(&connection).unwrap();
}

#[test]
fn valid_dataset_reopens_without_rewriting_schema_or_losing_rows() {
    let database = TestDatabase::new();
    let mut connection = open(&database.path).unwrap();
    let transaction = connection.transaction().unwrap();
    insert_fixture(&transaction);
    transaction.commit().unwrap();
    let original_inventory = inventory(&connection).unwrap();
    drop(connection);
    let connection = open(&database.path).unwrap();
    assert_eq!(inventory(&connection).unwrap(), original_inventory);
    let rows: u32 = connection
        .query_row(
            "SELECT count(*) FROM automations WHERE automation_id=?1",
            [FIXTURE_AUTOMATION],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1);
    validate_live(&connection).unwrap();
}

#[test]
fn existing_empty_and_corrupt_files_are_never_claimed_or_rewritten() {
    for contents in [
        b"".as_slice(),
        b"not a SQLite database: preserve the original".as_slice(),
    ] {
        let database = TestDatabase::new();
        fs::write(&database.path, contents).unwrap();
        assert!(open(&database.path).is_err());
        assert_eq!(fs::read(&database.path).unwrap(), contents);
    }
}

#[test]
fn incompatible_versions_lineages_checksums_and_live_schema_preserve_bytes() {
    for mutation in [
        "PRAGMA user_version = 2",
        "UPDATE automation_schema SET lineage='unknown-lineage'",
        "UPDATE automation_schema SET version=2",
        "UPDATE automation_schema SET schema_sha256='not-the-applied-resource'",
        "DELETE FROM automation_schema",
        "DROP INDEX runs_owner_revision",
        "CREATE TABLE unexpected_product(id INTEGER PRIMARY KEY AUTOINCREMENT)",
        "CREATE TRIGGER unexpected_trigger AFTER INSERT ON automations BEGIN SELECT 1; END",
    ] {
        let database = TestDatabase::new();
        let connection = open(&database.path).unwrap();
        connection.execute_batch(mutation).unwrap();
        drop(connection);
        let original = fs::read(&database.path).unwrap();
        assert!(
            matches!(open(&database.path), Err(StoreError::Incompatible(_))),
            "{mutation}"
        );
        assert_eq!(fs::read(&database.path).unwrap(), original, "{mutation}");
    }
}

#[test]
fn read_only_connection_rejects_product_writes_and_keeps_data() {
    let database = TestDatabase::new();
    drop(open(&database.path).unwrap());
    let original = fs::read(&database.path).unwrap();
    let connection = Connection::open_with_flags(
        &database.path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    validate_live(&connection).unwrap();
    let error = connection.execute(
        r#"INSERT INTO automations(automation_id,principal_authority,principal_user_id,current_revision,state,title_json,created_at_ms,updated_at_ms) VALUES(?1,'schema.fixture',7,1,'enabled','"fixture"',0,0)"#,
        [FIXTURE_AUTOMATION],
    ).unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::ReadOnly)
    );
    drop(connection);
    assert_eq!(fs::read(&database.path).unwrap(), original);
}

#[test]
fn startup_rejects_logical_orphans_without_rewriting_the_dataset() {
    let database = TestDatabase::new();
    let mut connection = open(&database.path).unwrap();
    let transaction = connection.transaction().unwrap();
    insert_fixture(&transaction);
    transaction.commit().unwrap();
    connection
        .execute("DELETE FROM automation_revisions", [])
        .unwrap();
    drop(connection);
    let original = fs::read(&database.path).unwrap();
    assert!(matches!(
        open(&database.path),
        Err(StoreError::Corrupt("logical reference audit failed"))
    ));
    assert_eq!(fs::read(&database.path).unwrap(), original);
}

#[test]
fn sqlite_full_rolls_back_all_earlier_product_writes_in_the_transaction() {
    let database = TestDatabase::new();
    let mut connection = open(&database.path).unwrap();
    let pages: u32 = connection
        .pragma_query_value(None, "page_count", |row| row.get(0))
        .unwrap();
    let limit: u32 = connection
        .query_row(&format!("PRAGMA max_page_count = {pages}"), [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(limit, pages);
    let transaction = connection.transaction().unwrap();
    insert_fixture(&transaction);
    let oversized_title =
        serde_json::to_string(&"x".repeat(4 * 1024 * 1024)).unwrap();
    let error = transaction
        .execute(
            "UPDATE automations SET title_json=?1 WHERE automation_id=?2",
            params![oversized_title, FIXTURE_AUTOMATION],
        )
        .unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DiskFull)
    );
    drop(transaction);
    let definitions: u32 = connection
        .query_row("SELECT count(*) FROM automations", [], |row| row.get(0))
        .unwrap();
    let revisions: u32 = connection
        .query_row("SELECT count(*) FROM automation_revisions", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!((definitions, revisions), (0, 0));
    validate_live(&connection).unwrap();
    drop(connection);
    let connection = open(&database.path).unwrap();
    validate_live(&connection).unwrap();
}

#[test]
fn process_exit_before_and_after_commit_recovers_atomic_product_state() {
    for (mode, exit_code, expected) in
        [("uncommitted", 41, 0), ("committed", 42, 1)]
    {
        let database = TestDatabase::new();
        drop(open(&database.path).unwrap());
        let output = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("store::schema::tests::transaction_crash_fixture")
            .arg("--nocapture")
            .env(CHILD_PATH, &database.path)
            .env(CHILD_MODE, mode)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(exit_code),
            "child stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let connection = open(&database.path).unwrap();
        let definitions: u32 = connection
            .query_row("SELECT count(*) FROM automations", [], |row| row.get(0))
            .unwrap();
        let revisions: u32 = connection
            .query_row("SELECT count(*) FROM automation_revisions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((definitions, revisions), (expected, expected));
        validate_live(&connection).unwrap();
    }
}

#[test]
fn transaction_crash_fixture() {
    let Some(path) = std::env::var_os(CHILD_PATH) else {
        return;
    };
    let mode = std::env::var(CHILD_MODE).unwrap();
    let mut connection = open(Path::new(&path)).unwrap();
    let transaction = connection.transaction().unwrap();
    insert_fixture(&transaction);
    if mode == "committed" {
        transaction.commit().unwrap();
        std::process::exit(42);
    }
    assert_eq!(mode, "uncommitted");
    // Exit deliberately skips Transaction and Connection destructors. This is
    // a child-only durable crash point, never a production failpoint.
    std::process::exit(41);
}
