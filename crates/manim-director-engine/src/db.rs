use anyhow::{anyhow, Context, Result};
use manim_director_core::{
    scene_id, CursorPage, DiscoverResult, ErrorBody, JobOrigin, JobRecord, JobStatus, Limits,
    LogLevel, LogRecord, LogStream, Operation, OperationRequest, OperationResult, Progress, Task,
    Timestamp,
};
use parking_lot::Mutex;
use rusqlite::{
    params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension, Row,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 2;
const MAX_LOG_RECORDS_PER_JOB: i64 = 5_000;
const MAX_LOG_BYTES_PER_JOB: i64 = 2 * 1024 * 1024;
const MAX_LOG_DATA_BYTES: usize = 16 * 1024;

const JOB_COLUMNS: &str = "sequence, id, operation, status, origin, owner, request, task, limits, \
    fingerprint, cached, cached_from, source_job_id, scene_file, scene_class, scene_revision, \
    profile, cancel_requested, progress, result, error, created_at, started_at, finished_at";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS jobs (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    operation TEXT NOT NULL,
    status TEXT NOT NULL,
    origin TEXT NOT NULL,
    owner TEXT NOT NULL,
    request TEXT NOT NULL,
    task TEXT NOT NULL,
    limits TEXT NOT NULL,
    fingerprint TEXT,
    cached INTEGER NOT NULL DEFAULT 0,
    cached_from TEXT,
    source_job_id TEXT,
    scene_file TEXT,
    scene_class TEXT,
    scene_revision TEXT,
    profile TEXT,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    progress TEXT,
    result TEXT,
    error TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS jobs_status_sequence ON jobs(status, sequence DESC);
CREATE INDEX IF NOT EXISTS jobs_owner_status ON jobs(owner, status);
CREATE INDEX IF NOT EXISTS jobs_scene ON jobs(scene_class, scene_file, operation, status, sequence DESC);
CREATE INDEX IF NOT EXISTS jobs_fingerprint ON jobs(fingerprint);
CREATE TABLE IF NOT EXISTS logs (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    stream TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    data TEXT
);
CREATE INDEX IF NOT EXISTS logs_job_cursor ON logs(job_id, cursor);
CREATE TABLE IF NOT EXISTS log_usage (
    job_id TEXT PRIMARY KEY,
    records INTEGER NOT NULL DEFAULT 0,
    bytes INTEGER NOT NULL DEFAULT 0,
    truncated INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS cache (
    fingerprint TEXT PRIMARY KEY,
    operation TEXT NOT NULL,
    job_id TEXT,
    result TEXT NOT NULL,
    created_at TEXT NOT NULL
);";

/// What a new job row records at submit.
#[derive(Debug, Clone)]
pub struct NewJob<'a> {
    pub id: Uuid,
    pub origin: JobOrigin,
    pub owner: Uuid,
    pub request: &'a OperationRequest,
    pub task: &'a Task,
    pub limits: Limits,
    pub fingerprint: Option<&'a str>,
    pub source_job_id: Option<Uuid>,
    pub scene_class: Option<&'a str>,
    pub scene_file: Option<&'a str>,
    pub scene_revision: Option<&'a str>,
    pub profile: Option<&'a str>,
}

/// Filters for walking succeeded jobs newest-first.
#[derive(Debug, Clone, Copy, Default)]
pub struct JobFilter<'a> {
    pub operations: &'a [Operation],
    pub scene_class: Option<&'a str>,
    pub scene_file: Option<&'a str>,
    pub profile: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub operation: Operation,
    pub job_id: Option<Uuid>,
    pub result: OperationResult,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        retire_old_schema(path)?;
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.fail_interrupted()?;
        Ok(store)
    }

    pub fn insert_job(&self, job: &NewJob<'_>) -> Result<JobRecord> {
        self.insert(job, None)
    }

    /// Inserts a cache hit: already succeeded, all three timestamps equal.
    pub fn insert_cached_job(
        &self,
        job: &NewJob<'_>,
        cached_from: Option<Uuid>,
        result: &OperationResult,
    ) -> Result<JobRecord> {
        self.insert(job, Some((cached_from, result)))
    }

    fn insert(
        &self,
        job: &NewJob<'_>,
        cached: Option<(Option<Uuid>, &OperationResult)>,
    ) -> Result<JobRecord> {
        let now = Timestamp::now().to_string();
        let (status, finished, cached_from, result) = match cached {
            Some((from, result)) => (
                JobStatus::Succeeded,
                Some(now.clone()),
                from.map(|id| id.to_string()),
                Some(serde_json::to_string(result)?),
            ),
            None => (JobStatus::Queued, None, None, None),
        };
        self.conn.lock().execute(
            "INSERT INTO jobs(id, operation, status, origin, owner, request, task, limits, fingerprint,
                cached, cached_from, source_job_id, scene_file, scene_class, scene_revision, profile,
                result, created_at, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?19)",
            params![
                job.id.to_string(),
                job.task.operation().as_str(),
                status.as_str(),
                job.origin.as_str(),
                job.owner.to_string(),
                serde_json::to_string(job.request)?,
                serde_json::to_string(job.task)?,
                serde_json::to_string(&job.limits)?,
                job.fingerprint,
                cached.is_some(),
                cached_from,
                job.source_job_id.map(|id| id.to_string()),
                job.scene_file,
                job.scene_class,
                job.scene_revision,
                job.profile,
                result,
                now,
                finished,
            ],
        )?;
        self.get_job(job.id)?
            .ok_or_else(|| anyhow!("job {} disappeared after insertion", job.id))
    }

    pub fn set_running(&self, id: Uuid) -> Result<bool> {
        Ok(self.conn.lock().execute(
            "UPDATE jobs SET status='running', started_at=?2 WHERE id=?1 AND status='queued'",
            params![id.to_string(), Timestamp::now().to_string()],
        )? == 1)
    }

    pub fn set_progress(&self, id: Uuid, progress: &Progress) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE jobs SET progress=?2 WHERE id=?1 AND status='running'",
            params![id.to_string(), serde_json::to_string(progress)?],
        )?;
        Ok(())
    }

    /// Records success; the rendered scene in the result replaces the
    /// submit-time association.
    pub fn finish_success(
        &self,
        id: Uuid,
        result: &OperationResult,
        scene_revision: Option<&str>,
    ) -> Result<bool> {
        let scene = result.scene();
        Ok(self.conn.lock().execute(
            "UPDATE jobs SET status='succeeded', result=?2, error=NULL, progress=NULL, finished_at=?3,
                scene_file=COALESCE(?4, scene_file), scene_class=COALESCE(?5, scene_class),
                scene_revision=COALESCE(?6, scene_revision)
             WHERE id=?1 AND status='running'",
            params![
                id.to_string(),
                serde_json::to_string(result)?,
                Timestamp::now().to_string(),
                scene.map(|scene| scene.file.as_str()),
                scene.map(|scene| scene.name.as_str()),
                scene_revision,
            ],
        )? == 1)
    }

    pub fn finish_error(&self, id: Uuid, status: JobStatus, error: &ErrorBody) -> Result<bool> {
        if !matches!(status, JobStatus::Failed | JobStatus::Cancelled) {
            return Err(anyhow!(
                "an error ends a job as failed or cancelled, not {status}"
            ));
        }
        Ok(self.conn.lock().execute(
            "UPDATE jobs SET status=?2, error=?3, progress=NULL, finished_at=?4
             WHERE id=?1 AND status IN ('queued','running')",
            params![
                id.to_string(),
                status.as_str(),
                serde_json::to_string(error)?,
                Timestamp::now().to_string()
            ],
        )? == 1)
    }

    /// Ends a job that has not started; `false` once a worker has taken it.
    pub fn cancel_queued(&self, id: Uuid, error: &ErrorBody) -> Result<bool> {
        Ok(self.conn.lock().execute(
            "UPDATE jobs SET status='cancelled', error=?2, finished_at=?3 WHERE id=?1 AND status='queued'",
            params![
                id.to_string(),
                serde_json::to_string(error)?,
                Timestamp::now().to_string()
            ],
        )? == 1)
    }

    /// Appends one record unless the job's quota is spent; the first record
    /// over quota is replaced by a single truncation marker.
    pub fn append_log(
        &self,
        id: Uuid,
        stream: LogStream,
        level: LogLevel,
        message: &str,
        data: Option<&Value>,
    ) -> Result<()> {
        let id = id.to_string();
        let data = data.map(bounded_log_data).transpose()?;
        let bytes = (message.len() + data.as_ref().map_or(0, String::len)) as i64;
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        transaction.execute(
            "INSERT OR IGNORE INTO log_usage(job_id, records, bytes, truncated) VALUES (?1, 0, 0, 0)",
            [&id],
        )?;
        let (records, used, truncated): (i64, i64, bool) = transaction.query_row(
            "SELECT records, bytes, truncated FROM log_usage WHERE job_id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let over_quota = records >= MAX_LOG_RECORDS_PER_JOB
            || used.saturating_add(bytes) > MAX_LOG_BYTES_PER_JOB;
        let now = Timestamp::now().to_string();
        if over_quota {
            if !truncated {
                transaction.execute(
                    "INSERT INTO logs(job_id, timestamp, stream, level, message, data)
                     VALUES (?1, ?2, 'engine', 'warning', 'Log truncated.', NULL)",
                    params![id, now],
                )?;
                transaction.execute(
                    "UPDATE log_usage SET records=records+1, truncated=1 WHERE job_id=?1",
                    [&id],
                )?;
            }
        } else {
            transaction.execute(
                "INSERT INTO logs(job_id, timestamp, stream, level, message, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, now, stream.as_str(), level.as_str(), message, data],
            )?;
            transaction.execute(
                "UPDATE log_usage SET records=records+1, bytes=bytes+?2 WHERE job_id=?1",
                params![id, bytes],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn get_job(&self, id: Uuid) -> Result<Option<JobRecord>> {
        self.conn
            .lock()
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"),
                [id.to_string()],
                row_to_job,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Jobs newest-first; `before` is an exclusive sequence cursor.
    pub fn jobs(&self, before: Option<i64>, limit: usize) -> Result<CursorPage<JobRecord>> {
        let limit = limit.clamp(1, 200);
        let conn = self.conn.lock();
        let mut statement = conn.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs WHERE sequence < ?1 ORDER BY sequence DESC LIMIT ?2"
        ))?;
        let items = statement
            .query_map(
                params![before.unwrap_or(i64::MAX), limit as i64],
                row_to_job,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_cursor = (items.len() == limit)
            .then(|| items.last().map(|job| job.sequence.to_string()))
            .flatten();
        Ok(CursorPage { items, next_cursor })
    }

    /// The queued or running job computing `fingerprint`, for coalescing.
    pub fn active_job_with_fingerprint(&self, fingerprint: &str) -> Result<Option<JobRecord>> {
        self.conn
            .lock()
            .query_row(
                &format!(
                    "SELECT {JOB_COLUMNS} FROM jobs WHERE fingerprint=?1 AND status IN ('queued','running')
                     ORDER BY sequence DESC LIMIT 1"
                ),
                [fingerprint],
                row_to_job,
            )
            .optional()
            .map_err(Into::into)
    }

    /// The newest succeeded job matching `filter` that `accept` approves.
    /// Walks candidates newest-first with no window, so stale rows (whose
    /// artifacts are gone) never hide an older usable one.
    pub fn find_succeeded(
        &self,
        filter: JobFilter<'_>,
        mut accept: impl FnMut(&JobRecord) -> bool,
    ) -> Result<Option<JobRecord>> {
        let mut sql = format!("SELECT {JOB_COLUMNS} FROM jobs WHERE status='succeeded'");
        let mut values: Vec<SqlValue> = Vec::new();
        if !filter.operations.is_empty() {
            let marks = vec!["?"; filter.operations.len()].join(", ");
            sql.push_str(&format!(" AND operation IN ({marks})"));
            values.extend(
                filter
                    .operations
                    .iter()
                    .map(|operation| SqlValue::Text(operation.to_string())),
            );
        }
        for (column, value) in [
            ("scene_class", filter.scene_class),
            ("scene_file", filter.scene_file),
            ("profile", filter.profile),
        ] {
            if let Some(value) = value {
                sql.push_str(&format!(" AND {column}=?"));
                values.push(SqlValue::Text(value.to_owned()));
            }
        }
        sql.push_str(" ORDER BY sequence DESC");
        let conn = self.conn.lock();
        let mut statement = conn.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values))?;
        while let Some(row) = rows.next()? {
            let job = row_to_job(row)?;
            if accept(&job) {
                return Ok(Some(job));
            }
        }
        Ok(None)
    }

    pub fn logs(
        &self,
        job_id: Uuid,
        after: Option<i64>,
        limit: usize,
    ) -> Result<CursorPage<LogRecord>> {
        let limit = limit.clamp(1, 500);
        let conn = self.conn.lock();
        let mut statement = conn.prepare(
            "SELECT cursor, timestamp, stream, level, message, data FROM logs
             WHERE job_id=?1 AND cursor>?2 ORDER BY cursor LIMIT ?3",
        )?;
        let items = statement
            .query_map(
                params![job_id.to_string(), after.unwrap_or(0), limit as i64],
                log_record,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_cursor = (items.len() == limit)
            .then(|| items.last().map(|record| record.cursor.clone()))
            .flatten();
        Ok(CursorPage { items, next_cursor })
    }

    /// The job's newest `limit` records, oldest first.
    pub fn tail_logs(&self, job_id: Uuid, limit: usize) -> Result<Vec<LogRecord>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare(
            "SELECT cursor, timestamp, stream, level, message, data FROM logs
             WHERE job_id=?1 ORDER BY cursor DESC LIMIT ?2",
        )?;
        let mut records = statement
            .query_map(params![job_id.to_string(), limit as i64], log_record)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        records.reverse();
        Ok(records)
    }

    /// Flags a non-terminal job for cancellation; its owner acts on the flag.
    pub fn request_cancel(&self, id: Uuid) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE jobs SET cancel_requested=1 WHERE id=?1 AND status IN ('queued','running')",
            [id.to_string()],
        )?;
        Ok(())
    }

    pub fn cache_get(&self, fingerprint: &str) -> Result<Option<CacheEntry>> {
        let row: Option<(String, Option<String>, String)> = self
            .conn
            .lock()
            .query_row(
                "SELECT operation, job_id, result FROM cache WHERE fingerprint=?1",
                [fingerprint],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((operation, job_id, result)) = row else {
            return Ok(None);
        };
        let operation = Operation::from_str(&operation)?;
        Ok(Some(CacheEntry {
            operation,
            job_id: job_id.map(|id| Uuid::parse_str(&id)).transpose()?,
            result: OperationResult::from_json(operation, serde_json::from_str(&result)?)
                .map_err(|error| anyhow!("cached {operation} result: {error}"))?,
        }))
    }

    pub fn cache_put(
        &self,
        fingerprint: &str,
        job_id: Option<Uuid>,
        result: &OperationResult,
        operation: Operation,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO cache(fingerprint, operation, job_id, result, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(fingerprint) DO UPDATE SET operation=excluded.operation, job_id=excluded.job_id,
                result=excluded.result, created_at=excluded.created_at",
            params![
                fingerprint,
                operation.as_str(),
                job_id.map(|id| id.to_string()),
                serde_json::to_string(result)?,
                Timestamp::now().to_string()
            ],
        )?;
        Ok(())
    }

    pub fn cache_delete(&self, fingerprint: &str) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM cache WHERE fingerprint=?1", [fingerprint])?;
        Ok(())
    }

    /// The newest cached `discover` result, used to associate jobs with files.
    pub fn newest_discover(&self) -> Result<Option<DiscoverResult>> {
        let result: Option<String> = self
            .conn
            .lock()
            .query_row(
                "SELECT result FROM cache WHERE operation='discover' ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        result
            .map(|text| serde_json::from_str(&text).context("cached discover result"))
            .transpose()
    }

    fn fail_interrupted(&self) -> Result<()> {
        let error = ErrorBody::new(
            "engine_lost",
            "The engine stopped before the job finished.",
            None,
        );
        self.conn.lock().execute(
            "UPDATE jobs SET status='failed', error=?1, progress=NULL, finished_at=?2
             WHERE status IN ('queued','running')",
            params![serde_json::to_string(&error)?, Timestamp::now().to_string()],
        )?;
        Ok(())
    }
}

/// A 1.x database cannot be migrated meaningfully; it is kept beside the new
/// one as `state.v1.db` and history starts fresh.
fn retire_old_schema(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let (version, tables): (i64, i64) = {
        let conn = Connection::open(path)?;
        (
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?,
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |row| row.get(0),
            )?,
        )
    };
    if version >= SCHEMA_VERSION || tables == 0 {
        return Ok(());
    }
    let retired = path.with_file_name("state.v1.db");
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", path.display()));
        if from.exists() {
            fs::rename(&from, format!("{}{suffix}", retired.display()))
                .with_context(|| format!("retiring {}", from.display()))?;
        }
    }
    Ok(())
}

fn bounded_log_data(data: &Value) -> Result<String> {
    let encoded = serde_json::to_string(data)?;
    if encoded.len() <= MAX_LOG_DATA_BYTES {
        Ok(encoded)
    } else {
        Ok(json!({"truncated": true}).to_string())
    }
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

fn row_to_job(row: &Row<'_>) -> rusqlite::Result<JobRecord> {
    let operation: Operation = parsed(row, "operation")?;
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
mod tests {
    use super::*;
    use manim_director_core::{DiagnoseParams, DiagnoseResult, DiagnoseTask};

    fn diagnose(store: &Store, id: Uuid) -> JobRecord {
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
                owner: Uuid::new_v4(),
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

    fn diagnosis() -> OperationResult {
        OperationResult::Diagnose(DiagnoseResult {
            recognized: false,
            findings: vec![],
            artifacts: vec![],
        })
    }

    #[test]
    fn jobs_round_trip_with_typed_request_task_and_result() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let id = Uuid::new_v4();
        let queued = diagnose(&store, id);
        assert_eq!(queued.status, JobStatus::Queued);
        assert_eq!(queued.scene_id, None);
        assert_eq!(queued.scene_class.as_deref(), Some("Intro"));
        assert_eq!(
            store.active_job_with_fingerprint("fp").unwrap().unwrap().id,
            id
        );
        assert!(store.set_running(id).unwrap());
        assert!(store.finish_success(id, &diagnosis(), None).unwrap());
        assert!(!store.finish_success(id, &diagnosis(), None).unwrap());
        let job = store.get_job(id).unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Succeeded);
        assert_eq!(job.result, Some(diagnosis()));
        assert!(matches!(job.task, Task::Diagnose(_)));
        assert!(store.active_job_with_fingerprint("fp").unwrap().is_none());
    }

    #[test]
    fn cache_entries_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let job = Uuid::new_v4();
        store
            .cache_put("fp", Some(job), &diagnosis(), Operation::Diagnose)
            .unwrap();
        let entry = store.cache_get("fp").unwrap().unwrap();
        assert_eq!(entry.job_id, Some(job));
        assert_eq!(entry.result, diagnosis());
        store.cache_delete("fp").unwrap();
        assert!(store.cache_get("fp").unwrap().is_none());
    }

    #[test]
    fn per_job_logs_are_bounded_and_end_with_one_truncation_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let id = Uuid::new_v4();
        diagnose(&store, id);
        let line = "x".repeat(24 * 1024);
        for _ in 0..100 {
            store
                .append_log(id, LogStream::Stderr, LogLevel::Info, &line, None)
                .unwrap();
        }
        let logs = store.logs(id, None, 500).unwrap().items;
        assert!(logs.len() < 100);
        let markers: Vec<_> = logs
            .iter()
            .filter(|log| log.stream == LogStream::Engine)
            .collect();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].message, "Log truncated.");
        assert_eq!(logs.last().unwrap().message, "Log truncated.");
    }

    #[test]
    fn oversized_log_data_is_replaced_by_a_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let id = Uuid::new_v4();
        diagnose(&store, id);
        let data = json!({"blob": "x".repeat(MAX_LOG_DATA_BYTES)});
        store
            .append_log(id, LogStream::Engine, LogLevel::Info, "big", Some(&data))
            .unwrap();
        let log = store.logs(id, None, 1).unwrap().items.remove(0);
        assert_eq!(log.data, Some(json!({"truncated": true})));
    }

    #[test]
    fn a_one_x_database_is_retired_beside_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE jobs (sequence INTEGER PRIMARY KEY, params TEXT);")
            .unwrap();
        let store = Store::open(&path).unwrap();
        assert!(dir.path().join("state.v1.db").is_file());
        assert!(store.jobs(None, 10).unwrap().items.is_empty());
        drop(store);
        Store::open(&path).unwrap();
        assert!(dir.path().join("state.v1.db").is_file());
    }

    #[test]
    fn find_succeeded_walks_past_rejected_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let (older, newer) = (Uuid::new_v4(), Uuid::new_v4());
        for id in [older, newer] {
            diagnose(&store, id);
            store.set_running(id).unwrap();
            store.finish_success(id, &diagnosis(), None).unwrap();
        }
        let filter = JobFilter {
            operations: &[Operation::Diagnose],
            scene_class: Some("Intro"),
            ..JobFilter::default()
        };
        let found = store.find_succeeded(filter, |job| job.id != newer).unwrap();
        assert_eq!(found.unwrap().id, older);
        let other = JobFilter {
            scene_class: Some("Outro"),
            ..filter
        };
        assert!(store.find_succeeded(other, |_| true).unwrap().is_none());
    }
}
