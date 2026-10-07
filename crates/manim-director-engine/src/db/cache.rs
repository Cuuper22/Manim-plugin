//! The result cache keyed by fingerprint (OPS §1.5).

use super::Store;
use anyhow::{anyhow, Context, Result};
use manim_director_core::{DiscoverResult, Operation, OperationResult, Timestamp};
use rusqlite::{params, OptionalExtension};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub operation: Operation,
    pub job_id: Option<Uuid>,
    pub result: OperationResult,
}

type CacheRow = (String, Option<String>, String);

impl Store {
    pub fn cache_get(&self, fingerprint: &str) -> Result<Option<CacheEntry>> {
        let row: Option<CacheRow> = self
            .conn
            .lock()
            .query_row(
                "SELECT operation, job_id, result FROM cache WHERE fingerprint=?1",
                [fingerprint],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        row.map(entry).transpose()
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

    /// Fingerprints and results of the entries that carry artifacts
    /// (everything but `discover`); `None` for a result that no longer parses.
    pub fn artifact_cache_rows(&self) -> Result<Vec<(String, Option<OperationResult>)>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare(
            "SELECT fingerprint, operation, job_id, result FROM cache WHERE operation != 'discover'",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    (row.get(1)?, row.get(2)?, row.get(3)?),
                ))
            })?
            .collect::<rusqlite::Result<Vec<(String, CacheRow)>>>()?;
        Ok(rows
            .into_iter()
            .map(|(fingerprint, row)| (fingerprint, entry(row).ok().map(|entry| entry.result)))
            .collect())
    }

    pub fn delete_cache_rows(&self, fingerprints: &[String]) -> Result<usize> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        let mut deleted = 0;
        for fingerprint in fingerprints {
            deleted +=
                transaction.execute("DELETE FROM cache WHERE fingerprint=?1", [fingerprint])?;
        }
        transaction.commit()?;
        Ok(deleted)
    }

    /// Keeps the newest `keep` discover results; returns how many went.
    pub fn trim_discover_cache(&self, keep: usize) -> Result<usize> {
        Ok(self.conn.lock().execute(
            "DELETE FROM cache WHERE operation='discover' AND fingerprint NOT IN (
                SELECT fingerprint FROM cache WHERE operation='discover'
                ORDER BY created_at DESC LIMIT ?1)",
            [keep as i64],
        )?)
    }
}

fn entry((operation, job_id, result): CacheRow) -> Result<CacheEntry> {
    let operation = Operation::from_str(&operation)?;
    Ok(CacheEntry {
        operation,
        job_id: job_id.map(|id| Uuid::parse_str(&id)).transpose()?,
        result: OperationResult::from_json(operation, serde_json::from_str(&result)?)
            .map_err(|error| anyhow!("cached {operation} result: {error}"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{diagnosis, store};
    use super::*;
    use manim_director_core::DiscoverResult;

    #[test]
    fn cache_entries_round_trip() {
        let (_dir, store) = store();
        let job = Uuid::new_v4();
        store
            .cache_put("fp", Some(job), &diagnosis(), Operation::Diagnose)
            .unwrap();
        let entry = store.cache_get("fp").unwrap().unwrap();
        assert_eq!(entry.job_id, Some(job));
        assert_eq!(entry.result, diagnosis());
        let rows = store.artifact_cache_rows().unwrap();
        assert_eq!(rows, [("fp".to_owned(), Some(diagnosis()))]);
        store.cache_delete("fp").unwrap();
        assert!(store.cache_get("fp").unwrap().is_none());
        store
            .cache_put("other", None, &diagnosis(), Operation::Diagnose)
            .unwrap();
        assert_eq!(store.delete_cache_rows(&["other".into()]).unwrap(), 1);
    }

    #[test]
    fn only_the_newest_discover_results_are_kept() {
        let (_dir, store) = store();
        let index = OperationResult::Discover(DiscoverResult {
            files: 0,
            truncated: false,
            scenes: vec![],
            findings: vec![],
            artifacts: vec![],
        });
        for index_number in 0..5 {
            store
                .cache_put(
                    &format!("discover-{index_number}"),
                    None,
                    &index,
                    Operation::Discover,
                )
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(store.trim_discover_cache(2).unwrap(), 3);
        assert!(store.cache_get("discover-4").unwrap().is_some());
        assert!(store.cache_get("discover-0").unwrap().is_none());
        assert!(store.newest_discover().unwrap().is_some());
    }
}
