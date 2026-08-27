use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use manim_director_core::{CursorPage, JobRecord, JobStatus, LogRecord, Operation, ProtocolError};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;
use std::{path::Path, str::FromStr};
use uuid::Uuid;

const MAX_LOG_EVENTS_PER_JOB: i64 = 2_000;
const MAX_LOG_BYTES_PER_JOB: i64 = 2 * 1024 * 1024;
const MAX_LOG_EVENT_BYTES: usize = 16 * 1024;
const MAX_GLOBAL_LOG_EVENTS: i64 = 50_000;
const MAX_GLOBAL_LOG_BYTES: i64 = 64 * 1024 * 1024;

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS jobs (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                project_root TEXT NOT NULL,
                operation TEXT NOT NULL,
                status TEXT NOT NULL,
                params TEXT NOT NULL,
                fingerprint TEXT,
                result TEXT,
                error TEXT,
                created_at TEXT NOT NULL,
                started_at TEXT,
                finished_at TEXT,
                cached INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS jobs_status_sequence ON jobs(status, sequence DESC);
            CREATE TABLE IF NOT EXISTS logs (
                cursor INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                level TEXT NOT NULL,
                event TEXT NOT NULL,
                data TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS logs_job_cursor ON logs(job_id, cursor);
            CREATE TABLE IF NOT EXISTS log_usage (
                job_id TEXT PRIMARY KEY,
                event_count INTEGER NOT NULL DEFAULT 0,
                byte_count INTEGER NOT NULL DEFAULT 0,
                truncated INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO log_usage(job_id,event_count,byte_count,truncated)
                SELECT job_id,COUNT(*),COALESCE(SUM(LENGTH(level)+LENGTH(event)+LENGTH(data)),0),0
                FROM logs GROUP BY job_id;
            CREATE TABLE IF NOT EXISTS cache (
                fingerprint TEXT PRIMARY KEY,
                project_root TEXT NOT NULL,
                operation TEXT NOT NULL,
                result TEXT NOT NULL,
                created_at TEXT NOT NULL
            );",
        )?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.fail_interrupted()?;
        Ok(store)
    }

    pub fn create_job(
        &self,
        id: Uuid,
        root: &Path,
        operation: Operation,
        params_value: &Value,
        fingerprint: Option<&str>,
    ) -> Result<JobRecord> {
        let now = Utc::now();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO jobs(id, project_root, operation, status, params, fingerprint, created_at) VALUES (?1, ?2, ?3, 'queued', ?4, ?5, ?6)",
            params![id.to_string(), root.to_string_lossy(), operation.to_string(), serde_json::to_string(params_value)?, fingerprint, now.to_rfc3339()],
        )?;
        let sequence = conn.last_insert_rowid();
        drop(conn);
        self.get_job(id)?
            .ok_or_else(|| anyhow!("job {id} disappeared after insertion"))
            .map(|mut job| {
                job.sequence = sequence;
                job
            })
    }

    pub fn create_cached_job(
        &self,
        id: Uuid,
        root: &Path,
        operation: Operation,
        params_value: &Value,
        fingerprint: &str,
        result: &Value,
    ) -> Result<JobRecord> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO jobs(id, project_root, operation, status, params, fingerprint, result, created_at, started_at, finished_at, cached)
             VALUES (?1, ?2, ?3, 'succeeded', ?4, ?5, ?6, ?7, ?7, ?7, 1)",
            params![id.to_string(), root.to_string_lossy(), operation.to_string(), serde_json::to_string(params_value)?, fingerprint, serde_json::to_string(result)?, now],
        )?;
        drop(conn);
        self.get_job(id)?
            .ok_or_else(|| anyhow!("cached job {id} disappeared after insertion"))
    }

    pub fn set_running(&self, id: Uuid) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE jobs SET status='running', started_at=?2 WHERE id=?1 AND status='queued'",
            params![id.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn finish_success(&self, id: Uuid, result: &Value) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE jobs SET status='succeeded', result=?2, error=NULL, finished_at=?3 WHERE id=?1 AND status='running'",
            params![id.to_string(), serde_json::to_string(result)?, Utc::now().to_rfc3339()],
        )?;
        prune_terminal_logs(&conn, MAX_GLOBAL_LOG_EVENTS, MAX_GLOBAL_LOG_BYTES)?;
        Ok(())
    }

    pub fn finish_error(&self, id: Uuid, status: JobStatus, error: &ProtocolError) -> Result<()> {
        if !matches!(status, JobStatus::Failed | JobStatus::Cancelled) {
            return Err(anyhow!(
                "terminal error requires failed or cancelled status"
            ));
        }
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE jobs SET status=?2, error=?3, finished_at=?4 WHERE id=?1 AND status IN ('queued','running')",
            params![id.to_string(), status.to_string(), serde_json::to_string(error)?, Utc::now().to_rfc3339()],
        )?;
        prune_terminal_logs(&conn, MAX_GLOBAL_LOG_EVENTS, MAX_GLOBAL_LOG_BYTES)?;
        Ok(())
    }

    pub fn append_log(&self, id: Uuid, level: &str, event: &str, data: &Value) -> Result<i64> {
        let id = id.to_string();
        let mut encoded = serde_json::to_string(data)?;
        if encoded.len() > MAX_LOG_EVENT_BYTES {
            let original_bytes = encoded.len();
            let mut end = 12 * 1024;
            while !encoded.is_char_boundary(end) {
                end -= 1;
            }
            encoded = serde_json::to_string(&serde_json::json!({
                "truncated": true,
                "original_bytes": original_bytes,
                "preview": &encoded[..end],
            }))?;
        }
        let bytes = (level.len() + event.len() + encoded.len()) as i64;
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        transaction.execute(
            "INSERT OR IGNORE INTO log_usage(job_id,event_count,byte_count,truncated) VALUES (?1,0,0,0)",
            [&id],
        )?;
        let (count, used, truncated): (i64, i64, i64) = transaction.query_row(
            "SELECT event_count,byte_count,truncated FROM log_usage WHERE job_id=?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if count >= MAX_LOG_EVENTS_PER_JOB || used.saturating_add(bytes) > MAX_LOG_BYTES_PER_JOB {
            if truncated != 0 {
                transaction.commit()?;
                return Ok(0);
            }
            let marker = serde_json::to_string(&serde_json::json!({
                "truncated": true,
                "reason": "per_job_log_quota",
                "max_events": MAX_LOG_EVENTS_PER_JOB,
                "max_bytes": MAX_LOG_BYTES_PER_JOB,
            }))?;
            transaction.execute(
                "INSERT INTO logs(job_id,timestamp,level,event,data) VALUES (?1,?2,'warn','logs_truncated',?3)",
                params![id, Utc::now().to_rfc3339(), marker],
            )?;
            let cursor = transaction.last_insert_rowid();
            transaction.execute(
                "UPDATE log_usage SET event_count=event_count+1,byte_count=byte_count+?2,truncated=1 WHERE job_id=?1",
                params![id, marker.len() as i64],
            )?;
            transaction.commit()?;
            return Ok(cursor);
        }
        transaction.execute(
            "INSERT INTO logs(job_id,timestamp,level,event,data) VALUES (?1,?2,?3,?4,?5)",
            params![id, Utc::now().to_rfc3339(), level, event, encoded],
        )?;
        let cursor = transaction.last_insert_rowid();
        transaction.execute(
            "UPDATE log_usage SET event_count=event_count+1,byte_count=byte_count+?2 WHERE job_id=?1",
            params![id, bytes],
        )?;
        transaction.commit()?;
        Ok(cursor)
    }

    pub fn get_job(&self, id: Uuid) -> Result<Option<JobRecord>> {
        self.conn
            .lock()
            .query_row(
                "SELECT * FROM jobs WHERE id=?1",
                [id.to_string()],
                row_to_job,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn jobs(&self, cursor: Option<i64>, limit: usize) -> Result<CursorPage<JobRecord>> {
        let limit = limit.clamp(1, 200);
        let cursor = cursor.unwrap_or(i64::MAX);
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT * FROM jobs WHERE sequence < ?1 ORDER BY sequence DESC LIMIT ?2")?;
        let items = stmt
            .query_map(params![cursor, limit as i64], row_to_job)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_cursor =
            (items.len() == limit).then(|| items.last().unwrap().sequence.to_string());
        Ok(CursorPage { items, next_cursor })
    }

    pub fn logs(
        &self,
        job_id: Option<Uuid>,
        cursor: Option<i64>,
        limit: usize,
    ) -> Result<CursorPage<LogRecord>> {
        let limit = limit.clamp(1, 500);
        let after = cursor.unwrap_or(0);
        let conn = self.conn.lock();
        let (query, job) = if let Some(id) = job_id {
            ("SELECT cursor,job_id,timestamp,level,event,data FROM logs WHERE cursor>?1 AND job_id=?3 ORDER BY cursor LIMIT ?2", Some(id.to_string()))
        } else {
            ("SELECT cursor,job_id,timestamp,level,event,data FROM logs WHERE cursor>?1 ORDER BY cursor LIMIT ?2", None)
        };
        let mut stmt = conn.prepare(query)?;
        let map = |row: &Row<'_>| -> rusqlite::Result<LogRecord> {
            Ok(LogRecord {
                cursor: row.get(0)?,
                job_id: parse_uuid(row.get::<_, String>(1)?)?,
                timestamp: parse_time(row.get::<_, String>(2)?)?,
                level: row.get(3)?,
                event: row.get(4)?,
                data: parse_json(row.get::<_, String>(5)?)?,
            })
        };
        let items = if let Some(job) = job {
            stmt.query_map(params![after, limit as i64, job], map)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map(params![after, limit as i64], map)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let next_cursor = (items.len() == limit).then(|| items.last().unwrap().cursor.to_string());
        Ok(CursorPage { items, next_cursor })
    }

    pub fn cache_get(&self, fingerprint: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .conn
            .lock()
            .query_row(
                "SELECT result FROM cache WHERE fingerprint=?1",
                [fingerprint],
                |row| row.get(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw).map_err(Into::into))
            .transpose()
    }

    pub fn cache_put(
        &self,
        fingerprint: &str,
        root: &Path,
        operation: Operation,
        result: &Value,
    ) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO cache(fingerprint,project_root,operation,result,created_at) VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(fingerprint) DO UPDATE SET result=excluded.result,created_at=excluded.created_at",
            params![fingerprint, root.to_string_lossy(), operation.to_string(), serde_json::to_string(result)?, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn cache_delete(&self, fingerprint: &str) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .execute("DELETE FROM cache WHERE fingerprint=?1", [fingerprint])?
            > 0)
    }

    pub fn cache_clear(&self) -> Result<usize> {
        Ok(self.conn.lock().execute("DELETE FROM cache", [])?)
    }

    fn fail_interrupted(&self) -> Result<()> {
        let error = serde_json::to_string(&ProtocolError::new(
            "engine_restarted",
            "engine stopped before the job completed",
        ))?;
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE jobs SET status='failed',error=?1,finished_at=?2 WHERE status IN ('queued','running')",
            params![error, Utc::now().to_rfc3339()],
        )?;
        prune_terminal_logs(&conn, MAX_GLOBAL_LOG_EVENTS, MAX_GLOBAL_LOG_BYTES)?;
        Ok(())
    }
}

fn prune_terminal_logs(conn: &Connection, max_events: i64, max_bytes: i64) -> Result<usize> {
    let (count, bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*),COALESCE(SUM(LENGTH(job_id)+LENGTH(timestamp)+LENGTH(level)+LENGTH(event)+LENGTH(data)+64),0) FROM logs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if count <= max_events && bytes <= max_bytes {
        return Ok(0);
    }

    let mut removed_count = 0_i64;
    let mut removed_bytes = 0_i64;
    let mut cutoff = None;
    {
        let mut statement = conn.prepare(
            "SELECT logs.cursor,LENGTH(logs.job_id)+LENGTH(logs.timestamp)+LENGTH(logs.level)+LENGTH(logs.event)+LENGTH(logs.data)+64
             FROM logs JOIN jobs ON jobs.id=logs.job_id
             WHERE jobs.status IN ('succeeded','failed','cancelled')
             ORDER BY logs.cursor ASC",
        )?;
        let rows =
            statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        for row in rows {
            let (cursor, row_bytes) = row?;
            removed_count += 1;
            removed_bytes = removed_bytes.saturating_add(row_bytes);
            cutoff = Some(cursor);
            if count.saturating_sub(removed_count) <= max_events
                && bytes.saturating_sub(removed_bytes) <= max_bytes
            {
                break;
            }
        }
    }
    let Some(cutoff) = cutoff else {
        // Active jobs are intentionally never pruned. Their independent
        // per-job quotas bound this temporary overage until they finish.
        return Ok(0);
    };
    let removed = conn.execute(
        "DELETE FROM logs WHERE cursor<=?1 AND job_id IN (
            SELECT id FROM jobs WHERE status IN ('succeeded','failed','cancelled')
         )",
        [cutoff],
    )?;
    conn.execute(
        "DELETE FROM log_usage WHERE job_id IN (
            SELECT id FROM jobs WHERE status IN ('succeeded','failed','cancelled')
         )",
        [],
    )?;
    conn.execute(
        "INSERT INTO log_usage(job_id,event_count,byte_count,truncated)
         SELECT logs.job_id,COUNT(*),COALESCE(SUM(LENGTH(logs.level)+LENGTH(logs.event)+LENGTH(logs.data)),0),
                MAX(CASE WHEN logs.event='logs_truncated' THEN 1 ELSE 0 END)
         FROM logs JOIN jobs ON jobs.id=logs.job_id
         WHERE jobs.status IN ('succeeded','failed','cancelled')
         GROUP BY logs.job_id",
        [],
    )?;
    Ok(removed)
}

fn row_to_job(row: &Row<'_>) -> rusqlite::Result<JobRecord> {
    let operation = Operation::from_str(&row.get::<_, String>(3)?).map_err(sql_message)?;
    let status = JobStatus::from_str(&row.get::<_, String>(4)?).map_err(sql_message)?;
    Ok(JobRecord {
        sequence: row.get(0)?,
        id: parse_uuid(row.get::<_, String>(1)?)?,
        project_root: row.get(2)?,
        operation,
        status,
        params: parse_json(row.get::<_, String>(5)?)?,
        fingerprint: row.get(6)?,
        result: row
            .get::<_, Option<String>>(7)?
            .map(parse_json)
            .transpose()?,
        error: row
            .get::<_, Option<String>>(8)?
            .map(|value| serde_json::from_str(&value).map_err(sql_conversion))
            .transpose()?,
        created_at: parse_time(row.get::<_, String>(9)?)?,
        started_at: row
            .get::<_, Option<String>>(10)?
            .map(parse_time)
            .transpose()?,
        finished_at: row
            .get::<_, Option<String>>(11)?
            .map(parse_time)
            .transpose()?,
        cached: row.get::<_, i64>(12)? != 0,
    })
}

fn parse_uuid(value: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(sql_conversion)
}
fn parse_time(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(sql_conversion)
}
fn parse_json(value: String) -> rusqlite::Result<Value> {
    serde_json::from_str(&value).map_err(sql_conversion)
}
fn sql_conversion(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn sql_message(message: String) -> rusqlite::Error {
    sql_conversion(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stores_jobs_logs_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let id = Uuid::new_v4();
        store
            .create_job(
                id,
                dir.path(),
                Operation::Render,
                &json!({"scene":"A"}),
                Some("fp"),
            )
            .unwrap();
        store.set_running(id).unwrap();
        store
            .append_log(id, "info", "frame", &json!({"n": 1}))
            .unwrap();
        store
            .finish_success(id, &json!({"path":"output/a.mp4"}))
            .unwrap();
        let job = store.get_job(id).unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Succeeded);
        assert_eq!(store.logs(Some(id), None, 10).unwrap().items.len(), 1);
        store
            .cache_put(
                "fp",
                dir.path(),
                Operation::Render,
                job.result.as_ref().unwrap(),
            )
            .unwrap();
        assert!(store.cache_get("fp").unwrap().is_some());
    }

    #[test]
    fn per_job_logs_are_bounded_and_end_with_one_truncation_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let id = Uuid::new_v4();
        store
            .create_job(id, dir.path(), Operation::Render, &json!({}), None)
            .unwrap();
        let payload = json!({"message":"x".repeat(24 * 1024)});
        for _ in 0..250 {
            store.append_log(id, "info", "chunk", &payload).unwrap();
        }
        let logs = store.logs(Some(id), None, 500).unwrap().items;
        assert!(logs.len() < 250);
        assert_eq!(
            logs.iter()
                .filter(|log| log.event == "logs_truncated")
                .count(),
            1
        );
        assert_eq!(logs.last().unwrap().event, "logs_truncated");
        assert!(serde_json::to_vec(&logs[0].data).unwrap().len() <= MAX_LOG_EVENT_BYTES);
    }

    #[test]
    fn global_retention_prunes_old_terminal_logs_but_keeps_active_logs() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let terminal = Uuid::new_v4();
        store
            .create_job(terminal, dir.path(), Operation::Render, &json!({}), None)
            .unwrap();
        store.set_running(terminal).unwrap();
        for index in 0..8 {
            store
                .append_log(terminal, "info", "frame", &json!({"index":index}))
                .unwrap();
        }
        store.finish_success(terminal, &json!({})).unwrap();

        let active = Uuid::new_v4();
        store
            .create_job(active, dir.path(), Operation::Render, &json!({}), None)
            .unwrap();
        store.set_running(active).unwrap();
        for index in 0..3 {
            store
                .append_log(active, "info", "frame", &json!({"index":index}))
                .unwrap();
        }
        let removed = {
            let conn = store.conn.lock();
            prune_terminal_logs(&conn, 5, i64::MAX).unwrap()
        };
        assert_eq!(removed, 6);
        assert_eq!(store.logs(Some(active), None, 10).unwrap().items.len(), 3);
        assert_eq!(store.logs(Some(terminal), None, 10).unwrap().items.len(), 2);
    }
}
