//! Job rows: insertion, the status transitions of OPS §1.8, and queries.

use super::{job_by_id, row_to_job, Store, JOB_COLUMNS};
use anyhow::{anyhow, bail, Result};
use manim_director_core::{
    CursorPage, ErrorBody, JobOrigin, JobRecord, JobStatus, Limits, Operation, OperationRequest,
    OperationResult, Progress, Task, Timestamp,
};
use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension};
use uuid::Uuid;

const TERMINAL: &str = "('succeeded','failed','cancelled')";
/// Ids per `IN (…)` list, well under SQLite's variable limit.
const DELETE_CHUNK: usize = 500;

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

/// How a terminal update landed. There is exactly one terminal transition per
/// job, so a caller that loses the race learns who won instead of a silent
/// zero-row update.
#[derive(Debug)]
pub enum Finish {
    /// This call ended the job.
    Ended(JobRecord),
    /// The job had already ended (reaped by another engine, or cancelled).
    Superseded(JobRecord),
}

/// The references that keep another job alive during pruning.
#[derive(Debug, Clone)]
pub struct JobLinks {
    pub id: Uuid,
    pub cached_from: Option<Uuid>,
    pub source_job_id: Option<Uuid>,
}

impl Store {
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
        let conn = self.conn.lock();
        conn.execute(
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
        job_by_id(&conn, job.id)?.ok_or_else(|| anyhow!("job {} vanished after insertion", job.id))
    }

    /// `queued → running`; `None` when the job is no longer queued.
    pub fn set_running(&self, id: Uuid) -> Result<Option<JobRecord>> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        let changed = transaction.execute(
            "UPDATE jobs SET status='running', started_at=?2 WHERE id=?1 AND status='queued'",
            params![id.to_string(), Timestamp::now().to_string()],
        )?;
        let record = match changed {
            1 => job_by_id(&transaction, id)?,
            _ => None,
        };
        transaction.commit()?;
        Ok(record)
    }

    pub fn set_progress(&self, id: Uuid, progress: &Progress) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE jobs SET progress=?2 WHERE id=?1 AND status='running'",
            params![id.to_string(), serde_json::to_string(progress)?],
        )?;
        Ok(())
    }

    /// `running → succeeded`; the rendered scene in the result replaces the
    /// submit-time association.
    pub fn finish_success(
        &self,
        id: Uuid,
        result: &OperationResult,
        scene_revision: Option<&str>,
    ) -> Result<Finish> {
        let scene = result.scene();
        self.finish(
            id,
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
        )
    }

    /// `queued|running → failed|cancelled`.
    pub fn finish_error(&self, id: Uuid, status: JobStatus, error: &ErrorBody) -> Result<Finish> {
        if !matches!(status, JobStatus::Failed | JobStatus::Cancelled) {
            bail!("an error ends a job as failed or cancelled, not {status}");
        }
        self.finish(
            id,
            "UPDATE jobs SET status=?2, error=?3, progress=NULL, finished_at=?4
             WHERE id=?1 AND status IN ('queued','running')",
            params![
                id.to_string(),
                status.as_str(),
                serde_json::to_string(error)?,
                Timestamp::now().to_string()
            ],
        )
    }

    fn finish(&self, id: Uuid, sql: &str, values: impl rusqlite::Params) -> Result<Finish> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        let changed = transaction.execute(sql, values)?;
        let record =
            job_by_id(&transaction, id)?.ok_or_else(|| anyhow!("job {id} does not exist"))?;
        transaction.commit()?;
        match changed {
            1 => Ok(Finish::Ended(record)),
            _ if record.status.is_terminal() => Ok(Finish::Superseded(record)),
            _ => bail!("job {id} is {}, so it cannot finish", record.status),
        }
    }

    /// `queued → cancelled`; `None` once a worker has taken the job.
    pub fn cancel_queued(&self, id: Uuid, error: &ErrorBody) -> Result<Option<JobRecord>> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        let changed = transaction.execute(
            "UPDATE jobs SET status='cancelled', error=?2, finished_at=?3 WHERE id=?1 AND status='queued'",
            params![
                id.to_string(),
                serde_json::to_string(error)?,
                Timestamp::now().to_string()
            ],
        )?;
        let record = match changed {
            1 => job_by_id(&transaction, id)?,
            _ => None,
        };
        transaction.commit()?;
        Ok(record)
    }

    /// Flags a non-terminal job for cancellation; its owner acts on the flag.
    pub fn request_cancel(&self, id: Uuid) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE jobs SET cancel_requested=1 WHERE id=?1 AND status IN ('queued','running')",
            [id.to_string()],
        )?;
        Ok(())
    }

    /// `owner`'s active jobs that someone asked to cancel.
    pub fn cancel_requests(&self, owner: Uuid) -> Result<Vec<Uuid>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare_cached(
            "SELECT id FROM jobs WHERE owner=?1 AND status IN ('queued','running') AND cancel_requested=1",
        )?;
        let ids = statement
            .query_map([owner.to_string()], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| Uuid::parse_str(id).map_err(Into::into))
            .collect()
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

    /// Terminal jobs past the newest `keep` or created before `cutoff`.
    pub fn prune_candidates(&self, keep: usize, cutoff: Timestamp) -> Result<Vec<Uuid>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare(&format!(
            "SELECT id FROM jobs WHERE status IN {TERMINAL} AND (created_at < ?1 OR sequence NOT IN (
                SELECT sequence FROM jobs WHERE status IN {TERMINAL} ORDER BY sequence DESC LIMIT ?2))"
        ))?;
        let ids = statement
            .query_map(params![cutoff.to_string(), keep as i64], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter()
            .map(|id| Uuid::parse_str(id).map_err(Into::into))
            .collect()
    }

    /// Every `(scene_class, scene_file)` pair some job is associated with.
    pub fn scene_pairs(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare(
            "SELECT DISTINCT scene_class, scene_file FROM jobs
             WHERE scene_class IS NOT NULL AND scene_file IS NOT NULL",
        )?;
        let pairs = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(pairs)
    }

    pub fn job_links(&self) -> Result<Vec<JobLinks>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare("SELECT id, cached_from, source_job_id FROM jobs")?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let parse = |text: Option<String>| text.and_then(|text| Uuid::parse_str(&text).ok());
        rows.into_iter()
            .map(|(id, cached_from, source_job_id)| {
                Ok(JobLinks {
                    id: Uuid::parse_str(&id)?,
                    cached_from: parse(cached_from),
                    source_job_id: parse(source_job_id),
                })
            })
            .collect()
    }

    /// Deletes terminal jobs with their logs, log accounting and cache rows;
    /// returns how many job rows went.
    pub fn delete_jobs(&self, ids: &[Uuid]) -> Result<usize> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        let mut deleted = 0;
        for chunk in ids.chunks(DELETE_CHUNK) {
            deleted += delete_chunk(&transaction, chunk)?;
        }
        transaction.commit()?;
        Ok(deleted)
    }
}

fn delete_chunk(conn: &Connection, ids: &[Uuid]) -> Result<usize> {
    let marks = vec!["?"; ids.len()].join(", ");
    let values = || ids.iter().map(Uuid::to_string);
    for table in ["logs", "log_usage", "cache"] {
        conn.execute(
            &format!("DELETE FROM {table} WHERE job_id IN ({marks})"),
            params_from_iter(values()),
        )?;
    }
    Ok(conn.execute(
        &format!("DELETE FROM jobs WHERE status IN {TERMINAL} AND id IN ({marks})"),
        params_from_iter(values()),
    )?)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{diagnosis, queued_job, store};
    use super::*;

    #[test]
    fn jobs_round_trip_with_typed_request_task_and_result() {
        let (_dir, store) = store();
        let id = Uuid::new_v4();
        let queued = queued_job(&store, id, Uuid::new_v4());
        assert_eq!(queued.status, JobStatus::Queued);
        assert_eq!(queued.scene_id, None);
        assert_eq!(queued.scene_class.as_deref(), Some("Intro"));
        assert_eq!(
            store.active_job_with_fingerprint("fp").unwrap().unwrap().id,
            id
        );
        let running = store.set_running(id).unwrap().unwrap();
        assert_eq!(running.status, JobStatus::Running);
        assert!(store.set_running(id).unwrap().is_none());
        let Finish::Ended(job) = store.finish_success(id, &diagnosis(), None).unwrap() else {
            panic!("the first finish ends the job")
        };
        assert_eq!(job.status, JobStatus::Succeeded);
        assert_eq!(job.result, Some(diagnosis()));
        assert!(matches!(job.task, Task::Diagnose(_)));
        assert!(store.active_job_with_fingerprint("fp").unwrap().is_none());
    }

    #[test]
    fn a_lost_finish_reports_the_winning_transition() {
        let (_dir, store) = store();
        let id = Uuid::new_v4();
        queued_job(&store, id, Uuid::new_v4());
        store.set_running(id).unwrap();
        let reaped = ErrorBody::new("engine_lost", "gone", None);
        assert!(matches!(
            store.finish_error(id, JobStatus::Failed, &reaped).unwrap(),
            Finish::Ended(_)
        ));
        let Finish::Superseded(winner) = store.finish_success(id, &diagnosis(), None).unwrap()
        else {
            panic!("a second finish must not report success")
        };
        assert_eq!(winner.status, JobStatus::Failed);
        assert_eq!(winner.error.unwrap().code, "engine_lost");
        assert!(store
            .finish_success(Uuid::new_v4(), &diagnosis(), None)
            .is_err());

        let queued = Uuid::new_v4();
        queued_job(&store, queued, Uuid::new_v4());
        assert!(
            store.finish_success(queued, &diagnosis(), None).is_err(),
            "a queued job cannot succeed"
        );
    }

    #[test]
    fn cancel_flags_are_listed_for_their_owner_only() {
        let (_dir, store) = store();
        let (mine, theirs) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        queued_job(&store, a, mine);
        queued_job(&store, b, theirs);
        store.request_cancel(a).unwrap();
        store.request_cancel(b).unwrap();
        assert_eq!(store.cancel_requests(mine).unwrap(), [a]);
        let cancelled = store
            .cancel_queued(a, &ErrorBody::new("cancelled", "x", None))
            .unwrap()
            .unwrap();
        assert!(cancelled.cancel_requested);
        assert!(store.cancel_requests(mine).unwrap().is_empty());
        assert!(store
            .cancel_queued(a, &ErrorBody::new("cancelled", "x", None))
            .unwrap()
            .is_none());
    }

    #[test]
    fn find_succeeded_walks_past_rejected_candidates() {
        let (_dir, store) = store();
        let (older, newer) = (Uuid::new_v4(), Uuid::new_v4());
        for id in [older, newer] {
            queued_job(&store, id, Uuid::new_v4());
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

    #[test]
    fn prune_candidates_are_terminal_jobs_past_the_cap_or_the_cutoff() {
        let (_dir, store) = store();
        let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        for id in &ids {
            queued_job(&store, *id, Uuid::new_v4());
        }
        for id in &ids[..3] {
            store.set_running(*id).unwrap();
            store.finish_success(*id, &diagnosis(), None).unwrap();
        }
        let future = Timestamp::from(chrono::Utc::now() + chrono::Duration::days(1));
        let past = Timestamp::from(chrono::Utc::now() - chrono::Duration::days(1));
        assert_eq!(store.prune_candidates(2, past).unwrap(), [ids[0]]);
        let mut aged = store.prune_candidates(100, future).unwrap();
        aged.sort();
        let mut terminal = ids[..3].to_vec();
        terminal.sort();
        assert_eq!(aged, terminal, "the queued job is never a candidate");

        assert_eq!(store.delete_jobs(&[ids[0], ids[3]]).unwrap(), 1);
        assert!(store.get_job(ids[0]).unwrap().is_none());
        assert!(store.get_job(ids[3]).unwrap().is_some());
    }
}
