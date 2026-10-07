//! Job log records with per-job quotas kept incrementally in `log_usage`.

use super::{log_record, Store};
use anyhow::Result;
use manim_director_core::{CursorPage, LogLevel, LogRecord, LogStream, Timestamp};
use rusqlite::{params, Transaction};
use serde_json::{json, Value};
use uuid::Uuid;

const MAX_RECORDS_PER_JOB: i64 = 5_000;
const MAX_BYTES_PER_JOB: i64 = 2 * 1024 * 1024;
const MAX_DATA_BYTES: usize = 16 * 1024;

/// A record before it has a cursor.
#[derive(Debug, Clone)]
pub struct NewLog {
    pub timestamp: Timestamp,
    pub stream: LogStream,
    pub level: LogLevel,
    pub message: String,
    pub data: Option<Value>,
}

impl NewLog {
    pub fn now(stream: LogStream, level: LogLevel, message: impl Into<String>) -> Self {
        Self {
            timestamp: Timestamp::now(),
            stream,
            level,
            message: message.into(),
            data: None,
        }
    }
}

struct Usage {
    records: i64,
    bytes: i64,
    truncated: bool,
}

impl Store {
    /// Appends a batch in one transaction. Records past the job's quota are
    /// dropped; the first one dropped becomes a single truncation marker.
    pub fn append_logs(&self, job_id: Uuid, records: &[NewLog]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let id = job_id.to_string();
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        let mut usage = usage(&transaction, &id)?;
        let start = usage.records;
        for record in records {
            if usage.truncated {
                break;
            }
            let data = record.data.as_ref().map(bounded_data).transpose()?;
            let bytes = (record.message.len() + data.as_ref().map_or(0, String::len)) as i64;
            if usage.records >= MAX_RECORDS_PER_JOB || usage.bytes + bytes > MAX_BYTES_PER_JOB {
                transaction.execute(
                    "INSERT INTO logs(job_id, timestamp, stream, level, message, data)
                     VALUES (?1, ?2, 'engine', 'warning', 'Log truncated.', NULL)",
                    params![id, record.timestamp.to_string()],
                )?;
                usage.records += 1;
                usage.truncated = true;
                break;
            }
            transaction.execute(
                "INSERT INTO logs(job_id, timestamp, stream, level, message, data)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    record.timestamp.to_string(),
                    record.stream.as_str(),
                    record.level.as_str(),
                    record.message,
                    data
                ],
            )?;
            usage.records += 1;
            usage.bytes += bytes;
        }
        if usage.records != start {
            transaction.execute(
                "UPDATE log_usage SET records=?2, bytes=?3, truncated=?4 WHERE job_id=?1",
                params![id, usage.records, usage.bytes, usage.truncated],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn logs(
        &self,
        job_id: Uuid,
        after: Option<i64>,
        limit: usize,
    ) -> Result<CursorPage<LogRecord>> {
        let limit = limit.clamp(1, 500);
        let conn = self.conn.lock();
        let mut statement = conn.prepare_cached(
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
        let mut statement = conn.prepare_cached(
            "SELECT cursor, timestamp, stream, level, message, data FROM logs
             WHERE job_id=?1 ORDER BY cursor DESC LIMIT ?2",
        )?;
        let mut records = statement
            .query_map(params![job_id.to_string(), limit as i64], log_record)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        records.reverse();
        Ok(records)
    }
}

fn usage(transaction: &Transaction<'_>, id: &str) -> Result<Usage> {
    transaction.execute(
        "INSERT OR IGNORE INTO log_usage(job_id, records, bytes, truncated) VALUES (?1, 0, 0, 0)",
        [id],
    )?;
    Ok(transaction.query_row(
        "SELECT records, bytes, truncated FROM log_usage WHERE job_id=?1",
        [id],
        |row| {
            Ok(Usage {
                records: row.get(0)?,
                bytes: row.get(1)?,
                truncated: row.get(2)?,
            })
        },
    )?)
}

fn bounded_data(data: &Value) -> Result<String> {
    let encoded = serde_json::to_string(data)?;
    if encoded.len() <= MAX_DATA_BYTES {
        Ok(encoded)
    } else {
        Ok(json!({"truncated": true}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{queued_job, store};
    use super::*;

    fn line(message: &str) -> NewLog {
        NewLog::now(LogStream::Stderr, LogLevel::Info, message)
    }

    #[test]
    fn a_batch_lands_in_order_in_one_call() {
        let (_dir, store) = store();
        let id = Uuid::new_v4();
        queued_job(&store, id, Uuid::new_v4());
        let batch: Vec<_> = (0..250).map(|index| line(&index.to_string())).collect();
        store.append_logs(id, &batch).unwrap();
        let page = store.logs(id, None, 500).unwrap();
        assert_eq!(page.items.len(), 250);
        assert_eq!(page.items[249].message, "249");
        assert_eq!(store.tail_logs(id, 2).unwrap()[1].message, "249");
    }

    #[test]
    fn per_job_logs_are_bounded_and_end_with_one_truncation_marker() {
        let (_dir, store) = store();
        let id = Uuid::new_v4();
        queued_job(&store, id, Uuid::new_v4());
        let big = "x".repeat(24 * 1024);
        for _ in 0..10 {
            let batch: Vec<_> = (0..10).map(|_| line(&big)).collect();
            store.append_logs(id, &batch).unwrap();
        }
        let logs = store.logs(id, None, 500).unwrap().items;
        assert!(logs.len() < 100);
        let markers = logs
            .iter()
            .filter(|log| log.stream == LogStream::Engine)
            .count();
        assert_eq!(markers, 1);
        assert_eq!(logs.last().unwrap().message, "Log truncated.");
        let used: i64 = logs.iter().map(|log| log.message.len() as i64).sum();
        assert!(used <= MAX_BYTES_PER_JOB + "Log truncated.".len() as i64);
    }

    #[test]
    fn oversized_log_data_is_replaced_by_a_marker() {
        let (_dir, store) = store();
        let id = Uuid::new_v4();
        queued_job(&store, id, Uuid::new_v4());
        let mut record = line("big");
        record.data = Some(json!({"blob": "x".repeat(MAX_DATA_BYTES)}));
        store.append_logs(id, &[record]).unwrap();
        let log = store.logs(id, None, 1).unwrap().items.remove(0);
        assert_eq!(log.data, Some(json!({"truncated": true})));
    }
}
