//! The shared job store (`<root>/.manim-director/state.db`, OPS §1.8). Every
//! engine process of a project opens its own connection; WAL plus a busy
//! timeout lets `serve`, `mcp` and one-off CLI commands share it.

mod cache;
mod jobs;
mod leases;
mod locks;
mod logs;
mod runtime;
mod schema;

pub use cache::CacheEntry;
pub use jobs::{Finish, JobFilter, JobLinks, NewJob};
pub use leases::{now_millis, EngineMode, LEASE_STALE_MILLIS};
pub use logs::NewLog;
pub use runtime::{RuntimeIdentity, StoredRuntime};

use anyhow::{anyhow, Result};
use manim_director_core::{scene_id, JobRecord, LogRecord, OperationResult, Task};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, Row};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const JOB_COLUMNS: &str = "sequence, id, operation, status, origin, owner, request, task, limits, \
    fingerprint, cached, cached_from, source_job_id, scene_file, scene_class, scene_revision, \
    profile, cancel_requested, progress, result, error, created_at, started_at, finished_at";

/// Where a project keeps its job store.
pub fn state_db_path(root: &Path) -> PathBuf {
    root.join(".manim-director/state.db")
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Opens (creating or migrating) the database. Blocking.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        schema::retire_one_x(path)?;
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        schema::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Runs database work on the blocking pool so no rusqlite call ever
    /// stalls an async worker thread.
    pub async fn blocking<T, F>(self: &Arc<Self>, work: F) -> Result<T>
    where
        F: FnOnce(&Store) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let store = Arc::clone(self);
        tokio::task::spawn_blocking(move || work(&store))
            .await
            .map_err(|error| anyhow!("database task failed: {error}"))?
    }

    pub fn get_job(&self, id: Uuid) -> Result<Option<JobRecord>> {
        job_by_id(&self.conn.lock(), id)
    }
}

fn job_by_id(conn: &Connection, id: Uuid) -> Result<Option<JobRecord>> {
    conn.query_row(
        &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"),
        [id.to_string()],
        row_to_job,
    )
    .optional()
    .map_err(Into::into)
}

fn row_to_job(row: &Row<'_>) -> rusqlite::Result<JobRecord> {
    let operation = parsed(row, "operation")?;
    let scene_file: Option<String> = row.get("scene_file")?;
    let scene_class: Option<String> = row.get("scene_class")?;
    Ok(JobRecord {
        id: parsed(row, "id")?,
        sequence: row.get("sequence")?,
        operation,
        status: parsed(row, "status")?,
        origin: parsed(row, "origin")?,
        owner: parsed(row, "owner")?,
        request: json_column(row, "request")?,
        task: Task::from_json(operation, json_column(row, "task")?).map_err(conversion)?,
        limits: json_column(row, "limits")?,
        fingerprint: row.get("fingerprint")?,
        cached: row.get("cached")?,
        cached_from: optional_parsed(row, "cached_from")?,
        source_job_id: optional_parsed(row, "source_job_id")?,
        scene_id: scene_id(scene_file.as_deref(), scene_class.as_deref()),
        scene_revision: row.get("scene_revision")?,
        profile: row.get("profile")?,
        cancel_requested: row.get("cancel_requested")?,
        progress: optional_json(row, "progress")?,
        result: optional_json::<Value>(row, "result")?
            .map(|value| OperationResult::from_json(operation, value))
            .transpose()
            .map_err(|error| conversion(std::io::Error::other(error)))?,
        error: optional_json(row, "error")?,
        created_at: parsed(row, "created_at")?,
        started_at: optional_parsed(row, "started_at")?,
        finished_at: optional_parsed(row, "finished_at")?,
        scene_class,
        scene_file,
    })
}

fn log_record(row: &Row<'_>) -> rusqlite::Result<LogRecord> {
    Ok(LogRecord {
        cursor: row.get::<_, i64>("cursor")?.to_string(),
        timestamp: parsed(row, "timestamp")?,
        stream: parsed(row, "stream")?,
        level: parsed(row, "level")?,
        message: row.get("message")?,
        data: optional_json(row, "data")?,
    })
}

fn parsed<T>(row: &Row<'_>, column: &str) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    row.get::<_, String>(column)?.parse().map_err(conversion)
}

fn optional_parsed<T>(row: &Row<'_>, column: &str) -> rusqlite::Result<Option<T>>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    row.get::<_, Option<String>>(column)?
        .map(|text| text.parse().map_err(conversion))
        .transpose()
}

fn json_column<T: DeserializeOwned>(row: &Row<'_>, column: &str) -> rusqlite::Result<T> {
    serde_json::from_str(&row.get::<_, String>(column)?).map_err(conversion)
}

fn optional_json<T: DeserializeOwned>(row: &Row<'_>, column: &str) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<String>>(column)?
        .map(|text| serde_json::from_str(&text).map_err(conversion))
        .transpose()
}

fn conversion(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use manim_director_core::{
        DiagnoseParams, DiagnoseResult, DiagnoseTask, JobOrigin, Limits, OperationRequest,
    };

    /// Inserts a queued `diagnose` job owned by `owner`.
    pub(crate) fn queued_job(store: &Store, id: Uuid, owner: Uuid) -> JobRecord {
        let request = OperationRequest::Diagnose(DiagnoseParams {
            job_id: None,
            text: Some("boom".into()),
        });
        let task = Task::Diagnose(DiagnoseTask {
            text: "boom".into(),
        });
        store
            .insert_job(&NewJob {
                id,
                origin: JobOrigin::Cli,
                owner,
                request: &request,
                task: &task,
                limits: Limits {
                    timeout_seconds: 60,
                    memory_mb: None,
                },
                fingerprint: Some("fp"),
                source_job_id: None,
                scene_class: Some("Intro"),
                scene_file: None,
                scene_revision: None,
                profile: None,
            })
            .unwrap()
    }

    pub(crate) fn diagnosis() -> OperationResult {
        OperationResult::Diagnose(DiagnoseResult {
            recognized: false,
            findings: vec![],
            artifacts: vec![],
        })
    }

    pub(crate) fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        (dir, store)
    }
}
