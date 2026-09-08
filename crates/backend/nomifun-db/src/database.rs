use std::collections::hash_map::DefaultHasher;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fs2::FileExt;
use sqlx::migrate::Migrator;
use sqlx::pool::PoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{Row, Sqlite, SqlitePool};
use tracing::{info, warn};

use crate::error::DbError;

/// Maximum number of connections in the pool.
const MAX_CONNECTIONS: u32 = 5;

/// SQLite busy timeout in milliseconds.
const BUSY_TIMEOUT_MS: u64 = 5000;

static DB_MIGRATOR: Migrator = sqlx::migrate!();
const V3_BASELINE_MIGRATION_VERSION: i64 = 1;

/// Compatibility result for a persisted sqlx migration lineage.
///
/// A strict prefix is safe to hand to the embedded migrator for an incremental
/// upgrade. Anything else (a gap, unknown version, failed row, or checksum
/// mismatch) is unsupported and must fail closed before writable startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationLineageStatus {
    Current,
    UpgradeRequired,
}

/// Wraps a SQLite connection pool with lifecycle management.
#[derive(Clone, Debug)]
pub struct Database {
    pool: SqlitePool,
    /// Per-run snapshot-copy cleanup. `Some` only for memory databases
    /// restored from the shared snapshot template; dropped together with the
    /// last clone of this handle so the run file outlives the pool.
    snapshot_run: Option<Arc<SnapshotRunFile>>,
}

impl Database {
    /// Returns a reference to the underlying connection pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Closes all connections in the pool.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Create a transactionally consistent SQLite snapshot at `destination`.
    ///
    /// This reads through SQLite rather than copying the main file, so
    /// committed pages still resident in WAL are included. The caller is
    /// responsible for placing the snapshot in a broader bundle manifest with
    /// the dataset generation and checksums for non-database files.
    pub async fn snapshot_into(&self, destination: &Path) -> Result<(), DbError> {
        if destination.exists() {
            return Err(DbError::Conflict(format!(
                "snapshot destination already exists: {}",
                destination.display()
            )));
        }
        if let Some(parent) = destination.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                DbError::Init(format!(
                    "failed to create snapshot directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let destination_text = destination.to_str().ok_or_else(|| {
            DbError::SafetyBackup(format!(
                "snapshot destination is not valid UTF-8: {}",
                destination.display()
            ))
        })?;
        sqlx::query("VACUUM main INTO ?")
            .bind(destination_text)
            .execute(&self.pool)
            .await
            .map_err(|error| {
                DbError::SafetyBackup(format!(
                    "could not create WAL-safe SQLite snapshot {}: {error}",
                    destination.display()
                ))
            })?;
        validate_sqlite_snapshot(destination).await
    }
}

pub(crate) async fn validate_sqlite_snapshot(path: &Path) -> Result<(), DbError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS));
    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(DbError::Query)?;
    let result = async {
        validate_quick_check(&pool).await?;
        validate_restorable_database_contract(&pool).await
    }
    .await;
    pool.close().await;
    result
}

/// Open an existing v3 database for an offline snapshot without running
/// migrations, recovery, or quarantine/rebuild logic against the source.
///
/// Backup is a preservation operation: an unsupported or invalid source must
/// fail closed instead of being transformed before it is captured.
pub async fn open_database_for_backup(path: &Path) -> Result<Database, DbError> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS));
    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(DbError::Query)?;
    let validation = async {
        validate_quick_check(&pool).await?;
        validate_restorable_database_contract(&pool).await
    }
    .await;
    if let Err(error) = validation {
        pool.close().await;
        return Err(error);
    }
    Ok(Database { pool, snapshot_run: None })
}

async fn validate_restorable_database_contract(pool: &SqlitePool) -> Result<(), DbError> {
    validate_current_migration_lineage(pool).await?;
    crate::id_schema_contract::validate_id_schema_contract(pool).await?;
    crate::id_schema_contract::validate_id_data_contract(pool).await?;

    let identities =
        sqlx::query("SELECT singleton_key, owner_user_id FROM installation_identity")
            .fetch_all(pool)
            .await
            .map_err(DbError::Query)?;
    if identities.len() != 1 {
        return Err(DbError::Init(format!(
            "backup installation_identity must contain exactly one row, found {}",
            identities.len()
        )));
    }
    let key: String = identities[0]
        .try_get("singleton_key")
        .map_err(DbError::Query)?;
    let owner_user_id: String = identities[0]
        .try_get("owner_user_id")
        .map_err(DbError::Query)?;
    if key != "installation" {
        return Err(DbError::Init(
            "backup installation_identity contains an invalid singleton key".into(),
        ));
    }
    nomifun_common::UserId::parse(owner_user_id.clone()).map_err(|error| {
        DbError::Init(format!(
            "backup installation owner ID is not canonical: {owner_user_id}: {error}"
        ))
    })?;
    let owner_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE user_id = ?")
        .bind(&owner_user_id)
        .fetch_one(pool)
        .await
        .map_err(DbError::Query)?;
    if owner_rows != 1 {
        return Err(DbError::Init(format!(
            "backup installation identity references missing owner user {owner_user_id}"
        )));
    }
    Ok(())
}

/// Require the complete migration lineage shipped with this build.
///
/// Backup and restore artifacts must already be Current; they are preservation
/// boundaries and must not be mutated as part of validation.
pub async fn validate_current_migration_lineage(pool: &SqlitePool) -> Result<(), DbError> {
    match inspect_supported_migration_lineage(pool).await? {
        MigrationLineageStatus::Current => Ok(()),
        MigrationLineageStatus::UpgradeRequired => Err(DbError::Init(
            "database migration lineage is a supported prefix but is not fully upgraded".into(),
        )),
    }
}

fn sql_lf(sql: &str) -> String {
    sql.replace("\r\n", "\n").replace('\r', "\n")
}

fn checksum_sha384(bytes: &[u8]) -> Vec<u8> {
    <sha2::Sha384 as sha2::Digest>::digest(bytes).to_vec()
}

/// Accept exact checksums and LF/CRLF-only encodings of the same embedded SQL.
///
/// Windows editors/`core.autocrlf` can apply a migration with CRLF bytes, then a
/// later LF checkout rebuilds a different sqlx checksum for identical schema.
fn migration_checksum_compatible(
    stored: &[u8],
    embedded_sql: &str,
    embedded_checksum: &[u8],
) -> bool {
    if stored == embedded_checksum {
        return true;
    }
    let lf = sql_lf(embedded_sql);
    if stored == checksum_sha384(lf.as_bytes()).as_slice() {
        return true;
    }
    let crlf = lf.replace('\n', "\r\n");
    stored == checksum_sha384(crlf.as_bytes()).as_slice()
}

/// Validate that the applied migration rows are an exact, non-empty prefix of
/// the migrations embedded in this binary.
///
/// This is intentionally less strict than the backup/restore contract:
/// startup must admit an older supported prefix so [`init_database`] can apply
/// the missing suffix. It still rejects every lineage that the migrator cannot
/// authenticate, including unknown future versions and edited checksums.
/// Line-ending-only checksum drift (LF vs CRLF of the same SQL) is treated as
/// compatible; [`heal_eol_only_migration_checksums`] rewrites those rows before
/// sqlx's migrator runs so VersionMismatch cannot fire on the same drift.
async fn sqlx_migrations_table_exists(
    executor: impl sqlx::Executor<'_, Database = Sqlite>,
) -> Result<bool, DbError> {
    sqlx::query_scalar(
        "SELECT EXISTS(\
             SELECT 1 FROM sqlite_schema \
             WHERE type = 'table' AND name = '_sqlx_migrations'\
         )",
    )
    .fetch_one(executor)
    .await
    .map_err(DbError::Query)
}

pub async fn inspect_supported_migration_lineage(
    pool: &SqlitePool,
) -> Result<MigrationLineageStatus, DbError> {
    let expected = DB_MIGRATOR.iter().collect::<Vec<_>>();
    if expected
        .first()
        .is_none_or(|migration| migration.version != V3_BASELINE_MIGRATION_VERSION)
    {
        return Err(DbError::Init(
            "v3 migration lineage must begin with the published baseline".into(),
        ));
    }

    if !sqlx_migrations_table_exists(pool).await? {
        return Ok(MigrationLineageStatus::UpgradeRequired);
    }

    let rows = sqlx::query("SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version")
        .fetch_all(pool)
        .await
        .map_err(DbError::Query)?;
    if rows.is_empty() {
        return Err(DbError::Init(format!(
            "database migration lineage must begin with embedded migration {}",
            V3_BASELINE_MIGRATION_VERSION,
        )));
    }
    if rows.len() > expected.len() {
        return Err(DbError::Init(format!(
            "database migration lineage contains {} rows but this binary embeds only {}",
            rows.len(),
            expected.len(),
        )));
    }

    for (row, expected) in rows.iter().zip(expected.iter()) {
        let version: i64 = row.try_get("version").map_err(DbError::Query)?;
        let success: bool = row.try_get("success").map_err(DbError::Query)?;
        let checksum: Vec<u8> = row.try_get("checksum").map_err(DbError::Query)?;
        let checksum_ok = migration_checksum_compatible(
            checksum.as_slice(),
            expected.sql.as_ref(),
            expected.checksum.as_ref(),
        );
        if version != expected.version || !success || !checksum_ok {
            return Err(DbError::Init(format!(
                "database migration lineage does not match embedded migration {}",
                expected.version
            )));
        }
    }
    Ok(if rows.len() == expected.len() {
        MigrationLineageStatus::Current
    } else {
        MigrationLineageStatus::UpgradeRequired
    })
}

/// Rewrite applied checksums that only differ by LF/CRLF so sqlx's migrator
/// accepts the same SQL content the probe already authenticated.
///
/// Fresh databases have no `_sqlx_migrations` table yet — sqlx creates it on
/// the first migrate pass — so this heal is a no-op until rows exist.
async fn heal_eol_only_migration_checksums(
    conn: &mut sqlx::SqliteConnection,
) -> Result<(), DbError> {
    if !sqlx_migrations_table_exists(&mut *conn).await? {
        return Ok(());
    }

    let expected = DB_MIGRATOR.iter().collect::<Vec<_>>();
    let rows = sqlx::query("SELECT version, success, checksum FROM _sqlx_migrations ORDER BY version")
        .fetch_all(&mut *conn)
        .await
        .map_err(DbError::Query)?;
    for (row, expected) in rows.iter().zip(expected.iter()) {
        let version: i64 = row.try_get("version").map_err(DbError::Query)?;
        let success: bool = row.try_get("success").map_err(DbError::Query)?;
        let checksum: Vec<u8> = row.try_get("checksum").map_err(DbError::Query)?;
        if version != expected.version || !success {
            continue;
        }
        if checksum.as_slice() == expected.checksum.as_ref() {
            continue;
        }
        if !migration_checksum_compatible(
            checksum.as_slice(),
            expected.sql.as_ref(),
            expected.checksum.as_ref(),
        ) {
            continue;
        }
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(expected.checksum.as_ref())
            .bind(version)
            .execute(&mut *conn)
            .await
            .map_err(DbError::Query)?;
        info!(
            version,
            "normalized LF/CRLF-only migration checksum to the embedded lineage"
        );
    }
    Ok(())
}

/// Initialize a file-backed SQLite database.
///
/// Creates the database file and parent directories if they don't exist,
/// configures the busy timeout and WAL journal mode, runs migrations, and
/// ensures the canonical installation owner exists. Migration-lineage errors
/// fail fast; the app bootstrap owns any explicit dataset reset.
pub async fn init_database(path: &Path) -> Result<Database, DbError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| DbError::Init(format!("Failed to create database directory: {e}")))?;
    }

    // The database crate never renames, repairs, migrates, or replaces an
    // existing dataset. The app bootstrap owns the v3 hard-reset lifecycle
    // before any pool is opened; direct callers fail closed on corruption or
    // unsupported lineage.
    try_init_file(path).await
}

/// Initialize an in-memory SQLite database (for testing).
///
/// Uses a single connection to ensure all queries share the same in-memory database.
/// Note: WAL journal mode is not available for in-memory databases.
pub async fn init_database_memory() -> Result<Database, DbError> {
    init_database_memory_inner(None).await
}

// ── In-memory snapshot template ─────────────────────────────────────────────
//
// Every memory init used to replay all migrations plus the v3 schema-contract
// validation (~1.2s in debug builds), which dominated test-suite runtime
// (398 repository tests x 1.2s = ~8 minutes of fixed cost). Instead, the
// first init in a process builds a validated, owner-less template database
// once (`VACUUM main INTO`), and every init afterwards restores from a copy.
//
// Correctness contract:
// - The template is keyed by a fingerprint over every migration's
//   version/description/sql plus the template semantics version, so any
//   schema or init change rebuilds it.
// - The template passes the exact same validation chain as the legacy path;
//   a restored copy is byte-identical to that validated state, so re-running
//   the contract validators per init would be a no-op and is skipped.
// - The template carries NO installation owner: the owner row is stripped
//   before `VACUUM INTO`, so each copy inserts its own owner through
//   `ensure_installation_owner`, preserving the legacy one-random-owner-per-
//   init semantics (and the hard error on a mismatched requested owner).
// - Any snapshot-machinery failure falls back to the legacy full init: the
//   cache can only make things faster, never wrong or red.

/// Bump when the template-building semantics change in a way the migration
/// fingerprint cannot see (e.g. new seed rows, changed validation chain).
const MEMORY_SNAPSHOT_SEMANTICS: &str = "v1";

/// Process-wide handle for the built template path. A cached failure keeps
/// every later init on the legacy full-init path instead of retry-storming.
/// Concurrent first callers await the same cell; a concurrent process race
/// resolves through the rename protocol in the builder.
static MEMORY_SNAPSHOT_TEMPLATE: tokio::sync::OnceCell<Result<PathBuf, String>> =
    tokio::sync::OnceCell::const_new();

/// Monotonic counter naming run files inside the per-process run directory.
static SNAPSHOT_RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Fingerprint over the full migration set plus the template semantics.
fn memory_snapshot_fingerprint() -> String {
    let mut hasher = DefaultHasher::new();
    for migration in DB_MIGRATOR.iter() {
        migration.version.hash(&mut hasher);
        migration.description.hash(&mut hasher);
        migration.sql.hash(&mut hasher);
    }
    MEMORY_SNAPSHOT_SEMANTICS.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Root directory for snapshot templates and per-run copies.
fn memory_snapshot_dir() -> PathBuf {
    std::env::temp_dir().join("flowy-db-mem-snapshots")
}

/// Build the validated, owner-less template at `final_path` (not yet present).
///
/// The staging database is a real FILE, not `:memory:`: in this environment
/// `VACUUM main INTO` from a `:memory:` source reports success while writing
/// nothing, so the template is produced as a direct file init instead. The
/// file is compact by construction (fresh init), and `close` checkpoints the
/// WAL so the single file carries all pages.
async fn build_memory_snapshot_template(final_path: &Path) -> Result<(), DbError> {
    let staging = final_path.with_extension(format!(
        "staging-{}-{}",
        std::process::id(),
        SNAPSHOT_RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let opts = SqliteConnectOptions::new()
        .filename(&staging)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS));
    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(|error| {
            let _ = std::fs::remove_file(&staging);
            DbError::Query(error)
        })?;

    // The exact legacy validation chain (a random owner is inserted first so
    // the data contract sees the same shape of state it always validated).
    let init = async {
        run_migrations(&pool).await?;
        crate::id_schema_contract::validate_id_schema_contract(&pool).await?;
        crate::id_schema_contract::repair_logical_reference_orphans(&pool).await?;
        ensure_installation_owner(&pool, None).await?;
        crate::id_schema_contract::validate_id_data_contract(&pool).await
    }
    .await;
    if let Err(error) = init {
        pool.close().await;
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    pool.close().await;

    // Backup-grade validation on the actual artifact while it still carries
    // the owner row (the restorable contract requires exactly one).
    validate_sqlite_snapshot(&staging).await?;

    // Strip the owner so restored copies insert their own. A fresh database
    // holds exactly the installation identity and its admin user row.
    let strip_opts = SqliteConnectOptions::new()
        .filename(&staging)
        .create_if_missing(false)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS));
    let strip_pool = PoolOptions::<Sqlite>::new()
        .max_connections(1)
        .connect_with(strip_opts)
        .await
        .map_err(|error| {
            let _ = std::fs::remove_file(&staging);
            DbError::Query(error)
        })?;
    let strip = async {
        let mut transaction = strip_pool.begin().await.map_err(DbError::Query)?;
        sqlx::query("DELETE FROM installation_identity")
            .execute(&mut *transaction)
            .await
            .map_err(DbError::Query)?;
        sqlx::query("DELETE FROM users WHERE username = 'admin'")
            .execute(&mut *transaction)
            .await
            .map_err(DbError::Query)?;
        transaction.commit().await.map_err(DbError::Query)
    }
    .await;
    strip_pool.close().await;
    if let Err(error) = strip {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }

    match std::fs::rename(&staging, final_path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Lost the race (or Windows held the file): the winner's template
            // is byte-equivalent, so keep ours only if the final is missing.
            if final_path.exists() {
                let _ = std::fs::remove_file(&staging);
                Ok(())
            } else {
                Err(DbError::Init(format!(
                    "could not move snapshot template into place: {}",
                    final_path.display()
                )))
            }
        }
    }
}

/// Resolve the template path, building it once per process. Concurrent
/// callers await the same cell; a concurrent process race resolves through
/// the rename protocol in [`build_memory_snapshot_template`].
async fn ensure_memory_snapshot_template() -> Result<PathBuf, DbError> {
    let dir = memory_snapshot_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| DbError::Init(format!("failed to create {}: {error}", dir.display())))?;
    let final_path = dir.join(format!("template-{}.db", memory_snapshot_fingerprint()));
    let result = MEMORY_SNAPSHOT_TEMPLATE
        .get_or_init(|| async {
            if final_path.exists() {
                return Ok(final_path);
            }
            // Best-effort GC of stale templates from older fingerprints and
            // run files abandoned by crashed processes.
            gc_stale_snapshot_files(&dir);
            match build_memory_snapshot_template(&final_path).await {
                Ok(()) => Ok(final_path),
                Err(error) => Err(error.to_string()),
            }
        })
        .await;
    result.clone().map_err(DbError::Init)
}

/// Delete snapshot files older than a day: superseded templates and run
/// directories of processes that are long gone. Locked files are skipped.
fn gc_stale_snapshot_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = std::time::SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified < cutoff {
            let path = entry.path();
            if metadata.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// One restored run file. Deleted when the last `Database` clone drops; the
/// pool's connections may close asynchronously after that, so removal retries
/// briefly in a background thread and gives up quietly on failure.
struct SnapshotRunFile {
    path: PathBuf,
    run_dir: PathBuf,
}

impl std::fmt::Debug for SnapshotRunFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotRunFile").field("path", &self.path).finish()
    }
}

impl Drop for SnapshotRunFile {
    fn drop(&mut self) {
        // Only runs once the last Arc reference (i.e. the last Database
        // clone) is gone, so this is always the final cleanup.
        // gone, so this is always the final cleanup.
        let path = self.path.clone();
        let run_dir = self.run_dir.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                match std::fs::remove_file(&path) {
                    Ok(()) => break,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    // Windows holds the file until the pool's background close
                    // finishes; retry briefly, then leave it for the day-GC.
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
            let _ = std::fs::remove_dir(&run_dir);
        });
    }
}

/// Restore one run database from the template: copy the file, connect to it,
/// cheaply confirm the migration state, and insert the installation owner.
async fn restore_database_from_snapshot(
    template: &Path,
    requested_owner_user_id: Option<&str>,
) -> Result<Database, DbError> {
    let run_dir = memory_snapshot_dir().join(format!("runs-{}", std::process::id()));
    std::fs::create_dir_all(&run_dir)
        .map_err(|error| DbError::Init(format!("failed to create {}: {error}", run_dir.display())))?;
    let run_file =
        run_dir.join(format!("mem-{}.db", SNAPSHOT_RUN_COUNTER.fetch_add(1, Ordering::Relaxed)));
    std::fs::copy(template, &run_file).map_err(|error| {
        DbError::Init(format!(
            "failed to copy snapshot template {}: {error}",
            template.display()
        ))
    })?;

    let opts = SqliteConnectOptions::new()
        .filename(&run_file)
        .create_if_missing(false)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS));
    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(|error| {
            let _ = std::fs::remove_file(&run_file);
            DbError::Query(error)
        })?;

    let init = async {
        // The template carries the full `_sqlx_migrations` ledger, so this is
        // a cheap applied-version check, not a replay — and still fails closed
        // if a rebuilt template ever drifted from the binary's migrations.
        run_migrations(&pool).await?;
        ensure_installation_owner(&pool, requested_owner_user_id).await
    }
    .await;
    if let Err(error) = init {
        pool.close().await;
        let _ = std::fs::remove_file(&run_file);
        return Err(error);
    }
    Ok(Database {
        pool,
        snapshot_run: Some(Arc::new(SnapshotRunFile { path: run_file, run_dir })),
    })
}

/// The legacy full path: replay migrations and the whole validation chain on
/// a fresh in-memory database. Used to build the template and as the fallback
/// whenever the snapshot machinery cannot deliver.
async fn init_database_memory_full(
    requested_owner_user_id: Option<&str>,
) -> Result<Database, DbError> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")
        .map_err(|e| DbError::Init(format!("Invalid memory connection string: {e}")))?
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS));

    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(DbError::Query)?;

    run_migrations(&pool).await?;
    crate::id_schema_contract::validate_id_schema_contract(&pool).await?;
    crate::id_schema_contract::repair_logical_reference_orphans(&pool).await?;
    ensure_installation_owner(&pool, requested_owner_user_id).await?;
    crate::id_schema_contract::validate_id_data_contract(&pool).await?;

    info!("In-memory database initialized");
    Ok(Database { pool, snapshot_run: None })
}

/// Initialize an in-memory database with an explicitly supplied canonical
/// installation owner.
///
/// This deterministic variant exists for large integration fixtures that need
/// to thread the same owner through many rows. It never opens an existing
/// dataset and therefore cannot replace or alias a persisted owner.
#[doc(hidden)]
pub async fn init_database_memory_with_owner(
    owner_user_id: nomifun_common::UserId,
) -> Result<Database, DbError> {
    init_database_memory_inner(Some(owner_user_id.into_string())).await
}

async fn init_database_memory_inner(requested_owner_user_id: Option<String>) -> Result<Database, DbError> {
    match ensure_memory_snapshot_template().await {
        Ok(template) => {
            match restore_database_from_snapshot(&template, requested_owner_user_id.as_deref())
                .await
            {
                Ok(database) => {
                    info!("In-memory database restored from snapshot template");
                    return Ok(database);
                }
                // Restore is an optimization only: fall back to the full init.
                Err(error) => {
                    warn!("snapshot restore failed, falling back to full init: {error}")
                }
            }
        }
        Err(error) => warn!("snapshot template unavailable, falling back to full init: {error}"),
    }
    init_database_memory_full(requested_owner_user_id.as_deref()).await
}

async fn try_init_file(path: &Path) -> Result<Database, DbError> {
    // Serialize the whole file-backed startup path, not only the sqlx
    // migrator. Opening a fresh SQLite file also runs connection-level PRAGMAs
    // such as WAL setup, which can race before migrations start.
    let lock_path = migrate_lock_path(path);
    let _guard = match MigrateLockGuard::acquire(&lock_path) {
        Ok(guard) => Some(guard),
        Err(e) => {
            // Don't fail startup if flock isn't available (e.g. on some
            // network filesystems) - fall back to SQLite busy-timeout and
            // retry-on-conflict behavior below.
            warn!("Could not acquire database startup lock {}: {e}", lock_path.display());
            None
        }
    };

    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS))
        .journal_mode(SqliteJournalMode::Wal);

    let pool = PoolOptions::<Sqlite>::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(opts)
        .await
        .map_err(DbError::Query)?;

    let setup = async {
        run_migrations(&pool).await?;
        crate::id_schema_contract::validate_id_schema_contract(&pool).await?;
        crate::id_schema_contract::repair_logical_reference_orphans(&pool).await?;
        ensure_installation_owner(&pool, None).await?;
        crate::id_schema_contract::validate_id_data_contract(&pool).await
    }
    .await;
    if let Err(e) = setup {
        // Release every file handle before bubbling up so the caller can
        // rename/backup the database file (Windows refuses to rename files
        // with open handles).
        pool.close().await;
        return Err(e);
    }

    info!("Database initialized at {}", path.display());
    Ok(Database { pool, snapshot_run: None })
}

/// Path of the cross-process advisory lock file used to serialize concurrent
/// migrators on the same database.
///
/// We put it next to the DB file so it lives on the same filesystem (avoids
/// odd flock semantics across mount points) and gets cleaned up alongside the
/// DB if a user resets their data directory.
fn migrate_lock_path(db_path: &Path) -> PathBuf {
    let mut p = db_path.to_path_buf();
    let new_name = match p.file_name().and_then(|s| s.to_str()) {
        Some(name) => format!("{name}.migrate.lock"),
        None => "nomifun.migrate.lock".to_string(),
    };
    p.set_file_name(new_name);
    p
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
    // File-backed callers hold a cross-process startup lock before opening the
    // SQLite pool. sqlx-sqlite's Migrate impl has no-op
    // lock()/unlock() and the migrator does list_applied -> apply without an
    // outer transaction, so two processes opening the same DB simultaneously
    // (e.g. an auto-update spawning the new version while the old one is
    // still shutting down, or `nomicore doctor` racing the server) can both
    // decide to apply the same version and the slower one's INSERT into
    // `_sqlx_migrations` blows up with `UNIQUE constraint failed:
    // _sqlx_migrations.version`. The outer startup lock also covers connection
    // setup before migration execution.
    let mut conn = pool.acquire().await.map_err(DbError::Query)?;
    heal_eol_only_migration_checksums(&mut conn).await?;
    run_migrations_with_retry(&mut conn).await?;
    validate_quick_check_on_connection(&mut conn).await
}

async fn validate_quick_check(pool: &SqlitePool) -> Result<(), DbError> {
    let rows: Vec<String> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_all(pool)
        .await
        .map_err(DbError::Query)?;
    require_quick_check_ok(rows)
}

async fn validate_quick_check_on_connection(
    conn: &mut sqlx::SqliteConnection,
) -> Result<(), DbError> {
    let rows: Vec<String> = sqlx::query_scalar("PRAGMA quick_check")
        .fetch_all(&mut *conn)
        .await
        .map_err(DbError::Query)?;
    require_quick_check_ok(rows)
}

fn require_quick_check_ok(rows: Vec<String>) -> Result<(), DbError> {
    if rows.len() == 1 && rows[0] == "ok" {
        return Ok(());
    }
    Err(DbError::Init(format!(
        "post-migration SQLite quick_check failed: {}",
        rows.join("; ")
    )))
}

/// Run sqlx migrations with bounded retries for known recoverable failures.
///
/// The advisory file lock above already serialises well-behaved processes, but
/// a `_sqlx_migrations` UNIQUE conflict can still leak through when:
/// - flock() failed (network FS, sandbox restrictions) and we proceeded.
/// - Two processes that both bypassed the lock raced.
///
/// In every UNIQUE-conflict scenario the failing migration's transaction was
/// rolled back, so re-running `sqlx::migrate!().run` is safe: the second
/// pass sees the row that the winner committed, checksum matches (same
/// shipped binary), and the migration is treated as already applied.
async fn run_migrations_with_retry(conn: &mut sqlx::SqliteConnection) -> Result<(), DbError> {
    let mut retried_unique_conflict = false;

    loop {
        match DB_MIGRATOR.run(&mut *conn).await {
            Ok(()) => return Ok(()),
            Err(e)
                if !retried_unique_conflict && is_migrations_table_unique_conflict(&e) =>
            {
                retried_unique_conflict = true;
                warn!(
                    "Concurrent migrator detected (UNIQUE conflict on _sqlx_migrations); retrying"
                );
            }
            Err(e) => return Err(DbError::Migration(e)),
        }
    }
}

/// Detect the specific "another process inserted this version first" error.
///
/// sqlx wraps the SQLite error inside `MigrateError::Execute(sqlx::Error)`.
/// We match on the textual message rather than the SQLite extended error code
/// because sqlx loses the structured code by the time it bubbles up here.
fn is_migrations_table_unique_conflict(err: &sqlx::migrate::MigrateError) -> bool {
    let msg = err.to_string();
    msg.contains("UNIQUE constraint failed: _sqlx_migrations.version")
}

/// RAII guard that holds an exclusive file lock for the lifetime of the
/// migration run. Drop unlocks and best-effort closes the file handle.
struct MigrateLockGuard {
    file: std::fs::File,
}

impl MigrateLockGuard {
    fn acquire(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        // Blocking lock via fs2 has no async variant. We're inside an async
        // context but startup blocks anyway and the critical section is
        // bounded (single-process migration run), so this is acceptable.
        FileExt::lock_exclusive(&file)?;
        Ok(Self { file })
    }
}

impl Drop for MigrateLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Ensure exactly one canonical installation owner exists.
///
/// The owner is a normal UUIDv7-addressed user entity. The singleton
/// `installation_identity` row is the durable indirection used by
/// repositories and logical-reference checks; restoring a database therefore
/// preserves the same owner ID, while a fresh dataset mints an unrelated one.
async fn ensure_installation_owner(
    pool: &SqlitePool,
    requested_owner_user_id: Option<&str>,
) -> Result<String, DbError> {
    let mut transaction = pool.begin().await.map_err(DbError::Query)?;

    let existing: Option<String> = sqlx::query_scalar(
        "SELECT owner_user_id FROM installation_identity \
         WHERE singleton_key = 'installation'",
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::Query)?;

    let owner_user_id = if let Some(owner_user_id) = existing {
        if let Some(requested_owner_user_id) = requested_owner_user_id
            && requested_owner_user_id != owner_user_id
        {
            return Err(DbError::Init(format!(
                "existing installation owner {owner_user_id} does not match requested test owner {requested_owner_user_id}"
            )));
        }
        nomifun_common::UserId::parse(owner_user_id.clone()).map_err(|error| {
            DbError::Init(format!(
                "installation owner ID is not canonical: {owner_user_id}: {error}"
            ))
        })?;
        let owner_exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE user_id = ?")
                .bind(&owner_user_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(DbError::Query)?;
        if owner_exists != 1 {
            return Err(DbError::Init(format!(
                "installation identity references missing owner user {owner_user_id}"
            )));
        }
        owner_user_id
    } else {
        let identity_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM installation_identity")
            .fetch_one(&mut *transaction)
            .await
            .map_err(DbError::Query)?;
        if identity_rows != 0 {
            return Err(DbError::Init(
                "installation_identity contains an invalid singleton key".to_owned(),
            ));
        }
        let owner_user_id = requested_owner_user_id
            .map(str::to_owned)
            .unwrap_or_else(|| nomifun_common::UserId::new().into_string());
        nomifun_common::UserId::parse(owner_user_id.clone()).map_err(|error| {
            DbError::Init(format!(
                "requested installation owner ID is not canonical: {owner_user_id}: {error}"
            ))
        })?;
        let now = nomifun_common::now_ms();
        sqlx::query(
            "INSERT INTO users (user_id, username, password_hash, created_at, updated_at) \
             VALUES (?, 'admin', '', ?, ?)",
        )
        .bind(&owner_user_id)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(DbError::Query)?;
        sqlx::query(
            "INSERT INTO installation_identity (singleton_key, owner_user_id) \
             VALUES ('installation', ?)",
        )
        .bind(&owner_user_id)
        .execute(&mut *transaction)
        .await
        .map_err(DbError::Query)?;
        owner_user_id
    };

    transaction.commit().await.map_err(DbError::Query)?;
    Ok(owner_user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_checksum_compatible_accepts_lf_crlf_only_drift() {
        let sql = "CREATE TABLE t (id INTEGER);\n";
        let lf_sum = checksum_sha384(sql.as_bytes());
        let crlf = sql.replace('\n', "\r\n");
        let crlf_sum = checksum_sha384(crlf.as_bytes());
        assert!(migration_checksum_compatible(
            lf_sum.as_slice(),
            sql,
            lf_sum.as_slice()
        ));
        assert!(migration_checksum_compatible(
            crlf_sum.as_slice(),
            sql,
            lf_sum.as_slice()
        ));
        assert!(migration_checksum_compatible(
            lf_sum.as_slice(),
            &crlf,
            crlf_sum.as_slice()
        ));
        let edited = "CREATE TABLE t (id INTEGER, x INTEGER);\n";
        let edited_sum = checksum_sha384(edited.as_bytes());
        assert!(!migration_checksum_compatible(
            edited_sum.as_slice(),
            sql,
            lf_sum.as_slice()
        ));
    }

    #[tokio::test]
    async fn fresh_file_database_init_applies_migrations_without_prior_migrations_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flowy-backend.db");
        assert!(!path.exists());

        let database = init_database(&path).await.unwrap();
        let table_exists: bool = sqlx_migrations_table_exists(database.pool())
            .await
            .unwrap();
        assert!(table_exists, "fresh bootstrap must create _sqlx_migrations");
        let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert!(applied > 0, "embedded migrations must be recorded on first boot");
        database.close().await;
    }

    #[tokio::test]
    async fn inspect_supported_migration_lineage_treats_missing_table_as_upgrade_required() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.db");
        let pool = PoolOptions::<Sqlite>::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();

        assert_eq!(
            inspect_supported_migration_lineage(&pool).await.unwrap(),
            MigrationLineageStatus::UpgradeRequired
        );
        pool.close().await;
    }

    #[tokio::test]
    async fn public_snapshot_includes_committed_wal_pages_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.db");
        let snapshot = dir.path().join("bundle").join("main.db");
        let database = init_database(&source).await.unwrap();
        sqlx::query(
            "INSERT INTO client_preferences (key, value, updated_at) \
             VALUES ('snapshot_probe', 'committed', ?)",
        )
            .bind(nomifun_common::now_ms())
            .execute(database.pool())
            .await
            .unwrap();
        database.snapshot_into(&snapshot).await.unwrap();
        let options = SqliteConnectOptions::new()
            .filename(&snapshot)
            .create_if_missing(false)
            .read_only(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        let value: String =
            sqlx::query_scalar("SELECT value FROM client_preferences WHERE key = 'snapshot_probe'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(value, "committed");
        pool.close().await;
        assert!(database.snapshot_into(&snapshot).await.is_err());
        database.close().await;
    }
}
