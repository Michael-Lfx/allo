//! Two-table SQLite store. Each table has a technical `id` key and a
//! database this version does not recognise is moved aside and replaced with
//! an empty one.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::types::Type;
use rusqlite::{Connection, Params, Row, params};

use crate::model::{
    Access, Automation, AutomationId, Run, RunId, RunStatus, RunTrigger, now_ms,
};

const SCHEMA_VERSION: i32 = 1;
const SCHEMA_V1: &str = "
BEGIN;
CREATE TABLE automations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    automation_id TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    action TEXT NOT NULL,
    trigger TEXT NOT NULL,
    cwd TEXT,
    access TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    next_run_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
CREATE INDEX automations_due ON automations (enabled, next_run_at_ms);
CREATE TABLE runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL UNIQUE,
    automation_id TEXT NOT NULL,
    status TEXT NOT NULL,
    trigger TEXT NOT NULL,
    session_id TEXT,
    created_at_ms INTEGER NOT NULL,
    started_at_ms INTEGER,
    finished_at_ms INTEGER,
    error TEXT
);
CREATE INDEX runs_by_automation ON runs (automation_id, created_at_ms);
CREATE INDEX runs_by_status ON runs (status);
PRAGMA user_version = 1;
COMMIT;
";

const AUTOMATION_COLUMNS: &str = "automation_id, title, action, trigger, cwd, access, \
    enabled, next_run_at_ms, created_at_ms, updated_at_ms";
const RUN_COLUMNS: &str = "run_id, automation_id, status, trigger, session_id, \
    created_at_ms, started_at_ms, finished_at_ms, error";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("automation database: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("automation record: {0}")]
    Json(#[from] serde_json::Error),
    #[error("automation database file: {0}")]
    Io(#[from] std::io::Error),
}

pub struct AutomationStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for AutomationStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AutomationStore")
    }
}

impl AutomationStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        // An unreadable file counts as unknown and is kept as a backup.
        if !is_current_or_empty(&conn).unwrap_or(false) {
            drop(conn);
            fs::rename(path, backup_path(path))?;
            conn = Connection::open(path)?;
        }
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        if user_version(&conn)? == 0 {
            conn.execute_batch(SCHEMA_V1)?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Inserts or replaces the automation.
    pub fn save(&self, automation: &Automation) -> Result<(), StoreError> {
        let action = serde_json::to_string(&automation.action)?;
        let trigger = serde_json::to_string(&automation.trigger)?;
        self.lock().execute(
            &format!(
                "INSERT INTO automations ({AUTOMATION_COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
                 ON CONFLICT (automation_id) DO UPDATE SET title = excluded.title, \
                 action = excluded.action, trigger = excluded.trigger, \
                 cwd = excluded.cwd, access = excluded.access, \
                 enabled = excluded.enabled, \
                 next_run_at_ms = excluded.next_run_at_ms, \
                 updated_at_ms = excluded.updated_at_ms"
            ),
            params![
                automation.id.to_string(),
                automation.title,
                action,
                trigger,
                automation
                    .cwd
                    .as_ref()
                    .map(|cwd| cwd.to_string_lossy().into_owned()),
                automation.access.as_str(),
                automation.enabled,
                automation.next_run_at_ms,
                automation.created_at_ms,
                automation.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    /// Deletes the automation and its runs. Returns whether it existed.
    pub fn delete(&self, id: AutomationId) -> Result<bool, StoreError> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM runs WHERE automation_id = ?1",
            [id.to_string()],
        )?;
        let removed = tx.execute(
            "DELETE FROM automations WHERE automation_id = ?1",
            [id.to_string()],
        )?;
        tx.commit()?;
        drop(conn);
        Ok(removed == 1)
    }

    pub fn get(
        &self,
        id: AutomationId,
    ) -> Result<Option<Automation>, StoreError> {
        Ok(self
            .automations("WHERE automation_id = ?1", [id.to_string()])?
            .pop())
    }

    pub fn list(&self) -> Result<Vec<Automation>, StoreError> {
        self.automations("ORDER BY created_at_ms, id", [])
    }

    /// Enabled automations whose next run is at or before `now_ms`.
    pub fn due(&self, now_ms: i64) -> Result<Vec<Automation>, StoreError> {
        self.automations(
            "WHERE enabled = 1 AND next_run_at_ms <= ?1 ORDER BY next_run_at_ms",
            [now_ms],
        )
    }

    fn automations(
        &self,
        filter: &str,
        params: impl Params,
    ) -> Result<Vec<Automation>, StoreError> {
        let conn = self.lock();
        let mut statement = conn.prepare(&format!(
            "SELECT {AUTOMATION_COLUMNS} FROM automations {filter}"
        ))?;
        let rows = statement
            .query_map(params, automation_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(conn);
        Ok(rows)
    }

    /// Inserts or replaces the run. A run whose automation was deleted is
    /// not written back.
    pub fn save_run(&self, run: &Run) -> Result<(), StoreError> {
        self.lock().execute(
            &format!(
                "INSERT INTO runs ({RUN_COLUMNS}) \
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9 \
                 WHERE EXISTS (SELECT 1 FROM automations WHERE automation_id = ?2) \
                 ON CONFLICT (run_id) DO UPDATE SET status = excluded.status, \
                 session_id = excluded.session_id, \
                 started_at_ms = excluded.started_at_ms, \
                 finished_at_ms = excluded.finished_at_ms, \
                 error = excluded.error"
            ),
            params![
                run.id.to_string(),
                run.automation_id.to_string(),
                run.status.as_str(),
                run.trigger.as_str(),
                run.session_id,
                run.created_at_ms,
                run.started_at_ms,
                run.finished_at_ms,
                run.error,
            ],
        )?;
        Ok(())
    }

    /// Moves a run from `from` to `to` only if it is still in `from`, so a
    /// cancel and a start cannot both win.
    pub fn transition(
        &self,
        id: RunId,
        from: RunStatus,
        to: RunStatus,
        at_ms: i64,
    ) -> Result<bool, StoreError> {
        let column = if to == RunStatus::Running {
            "started_at_ms"
        } else {
            "finished_at_ms"
        };
        let changed = self.lock().execute(
            &format!(
                "UPDATE runs SET status = ?3, {column} = ?4 \
                 WHERE run_id = ?1 AND status = ?2"
            ),
            params![id.to_string(), from.as_str(), to.as_str(), at_ms],
        )?;
        Ok(changed == 1)
    }

    pub fn run(&self, id: RunId) -> Result<Option<Run>, StoreError> {
        Ok(self.runs("WHERE run_id = ?1", [id.to_string()])?.pop())
    }

    /// Newest first.
    pub fn runs_for(
        &self,
        automation_id: AutomationId,
        limit: usize,
    ) -> Result<Vec<Run>, StoreError> {
        self.runs(
            "WHERE automation_id = ?1 ORDER BY created_at_ms DESC, id DESC LIMIT ?2",
            params![
                automation_id.to_string(),
                i64::try_from(limit).unwrap_or(i64::MAX)
            ],
        )
    }

    /// Oldest first.
    pub fn queued_runs(&self) -> Result<Vec<Run>, StoreError> {
        self.runs("WHERE status = 'queued' ORDER BY created_at_ms, id", [])
    }

    fn runs(
        &self,
        filter: &str,
        params: impl Params,
    ) -> Result<Vec<Run>, StoreError> {
        let conn = self.lock();
        let mut statement =
            conn.prepare(&format!("SELECT {RUN_COLUMNS} FROM runs {filter}"))?;
        let rows = statement
            .query_map(params, run_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(conn);
        Ok(rows)
    }

    /// Called once at startup: runs left `running` by the previous process
    /// become `interrupted`. They are not retried.
    pub fn mark_running_interrupted(
        &self,
        now_ms: i64,
    ) -> Result<usize, StoreError> {
        Ok(self.lock().execute(
            "UPDATE runs SET status = 'interrupted', finished_at_ms = ?1 \
             WHERE status = 'running'",
            [now_ms],
        )?)
    }

    /// Keeps the newest `keep` runs of each automation; queued and running
    /// runs are never removed.
    pub fn prune(&self, keep: usize) -> Result<usize, StoreError> {
        Ok(self.lock().execute(
            "DELETE FROM runs WHERE status NOT IN ('queued', 'running') \
             AND id NOT IN (SELECT id FROM (SELECT id, ROW_NUMBER() OVER ( \
             PARTITION BY automation_id ORDER BY created_at_ms DESC, id DESC \
             ) AS position FROM runs) WHERE position <= ?1)",
            [i64::try_from(keep).unwrap_or(i64::MAX)],
        )?)
    }

    /// Sessions created by earlier runs. They must not get the automation
    /// management tools after a restart.
    pub fn run_session_ids(&self) -> Result<Vec<String>, StoreError> {
        let conn = self.lock();
        let mut statement = conn.prepare(
            "SELECT DISTINCT session_id FROM runs WHERE session_id IS NOT NULL",
        )?;
        let ids = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        drop(statement);
        drop(conn);
        Ok(ids)
    }
}

fn user_version(conn: &Connection) -> rusqlite::Result<i32> {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
}

fn is_current_or_empty(conn: &Connection) -> rusqlite::Result<bool> {
    let version = user_version(conn)?;
    if version == SCHEMA_VERSION {
        return Ok(true);
    }
    let objects: i64 =
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get(0)
        })?;
    Ok(version == 0 && objects == 0)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".bak-{}", now_ms()));
    PathBuf::from(name)
}

type ParseResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

fn decode<T>(
    row: &Row<'_>,
    index: usize,
    parse: impl FnOnce(&str) -> ParseResult<T>,
) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    parse(&text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Text, error)
    })
}

fn known<T>(value: Option<T>, text: &str) -> ParseResult<T> {
    value.ok_or_else(|| format!("unknown value {text:?}").into())
}

fn automation_from_row(row: &Row<'_>) -> rusqlite::Result<Automation> {
    Ok(Automation {
        id: decode(row, 0, |text| Ok(AutomationId::parse(text)?))?,
        title: row.get(1)?,
        action: decode(row, 2, |text| Ok(serde_json::from_str(text)?))?,
        trigger: decode(row, 3, |text| Ok(serde_json::from_str(text)?))?,
        cwd: row.get::<_, Option<String>>(4)?.map(PathBuf::from),
        access: decode(row, 5, |text| known(Access::parse(text), text))?,
        enabled: row.get(6)?,
        next_run_at_ms: row.get(7)?,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: decode(row, 0, |text| Ok(RunId::parse(text)?))?,
        automation_id: decode(row, 1, |text| Ok(AutomationId::parse(text)?))?,
        status: decode(row, 2, |text| known(RunStatus::parse(text), text))?,
        trigger: decode(row, 3, |text| known(RunTrigger::parse(text), text))?,
        session_id: row.get(4)?,
        created_at_ms: row.get(5)?,
        started_at_ms: row.get(6)?,
        finished_at_ms: row.get(7)?,
        error: row.get(8)?,
    })
}

#[cfg(test)]
mod tests;
