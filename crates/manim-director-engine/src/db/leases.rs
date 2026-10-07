//! Engine leases and the stale-lease reaper (OPS §1.8): a job is failed as
//! `engine_lost` only when the engine that owns it has stopped heartbeating.

use super::{job_by_id, Store};
use anyhow::Result;
use manim_director_core::{named_enum, ErrorBody, JobRecord, Timestamp};
use rusqlite::{params, TransactionBehavior};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// A lease whose heartbeat is older than this is stale.
pub const LEASE_STALE_MILLIS: i64 = 10_000;

named_enum! {
    /// How an engine process runs; long-lived modes also prune history.
    pub enum EngineMode {
        Serve = "serve",
        Mcp = "mcp",
        Cli = "cli",
    }
}

impl EngineMode {
    pub fn is_long_lived(self) -> bool {
        matches!(self, Self::Serve | Self::Mcp)
    }
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

impl Store {
    /// Creates or refreshes this engine's lease.
    pub fn renew_lease(&self, instance: Uuid, mode: EngineMode, now: i64) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO engine_leases(instance_id, pid, mode, started_at, heartbeat_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(instance_id) DO UPDATE SET heartbeat_at=excluded.heartbeat_at",
            params![instance.to_string(), std::process::id(), mode.as_str(), now],
        )?;
        Ok(())
    }

    /// Ends a lease and frees its scene locks (clean shutdown).
    pub fn release_lease(&self, instance: Uuid) -> Result<()> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction()?;
        let id = instance.to_string();
        transaction.execute("DELETE FROM scene_locks WHERE owner=?1", [&id])?;
        transaction.execute("DELETE FROM engine_leases WHERE instance_id=?1", [&id])?;
        transaction.commit()?;
        Ok(())
    }

    /// Fails the queued and running jobs of every other engine whose lease is
    /// stale or missing, then drops stale leases and their scene locks. Runs
    /// as one immediate transaction so two reapers never both fail a job.
    pub fn reap(&self, me: Uuid, now: i64) -> Result<Vec<JobRecord>> {
        let cutoff = now - LEASE_STALE_MILLIS;
        let me = me.to_string();
        let mut conn = self.conn.lock();
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let orphans = {
            let mut statement = transaction.prepare(
                "SELECT id, owner FROM jobs
                 WHERE status IN ('queued','running') AND owner != ?1
                   AND owner NOT IN (SELECT instance_id FROM engine_leases WHERE heartbeat_at >= ?2)",
            )?;
            let rows = statement
                .query_map(params![me, cutoff], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        let finished_at = Timestamp::now().to_string();
        let mut reaped = Vec::with_capacity(orphans.len());
        for (id, owner) in orphans {
            let error = ErrorBody::new(
                "engine_lost",
                "The engine that owned this job stopped before it finished.",
                Some(json!({ "owner": owner })),
            );
            transaction.execute(
                "UPDATE jobs SET status='failed', error=?2, progress=NULL, finished_at=?3
                 WHERE id=?1 AND status IN ('queued','running')",
                params![id, serde_json::to_string(&error)?, finished_at],
            )?;
            if let Some(job) = job_by_id(&transaction, Uuid::parse_str(&id)?)? {
                reaped.push(job);
            }
        }
        transaction.execute(
            "DELETE FROM scene_locks WHERE owner != ?1
               AND owner NOT IN (SELECT instance_id FROM engine_leases WHERE heartbeat_at >= ?2)",
            params![me, cutoff],
        )?;
        transaction.execute(
            "DELETE FROM engine_leases WHERE instance_id != ?1 AND heartbeat_at < ?2",
            params![me, cutoff],
        )?;
        transaction.commit()?;
        Ok(reaped)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::queued_job;
    use super::*;
    use manim_director_core::JobStatus;

    #[test]
    fn opening_a_second_store_spares_jobs_with_a_live_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let first = Store::open(&path).unwrap();
        let (owner, other) = (Uuid::new_v4(), Uuid::new_v4());
        first
            .renew_lease(owner, EngineMode::Serve, now_millis())
            .unwrap();
        let job = Uuid::new_v4();
        queued_job(&first, job, owner);
        first.set_running(job).unwrap();

        let second = Store::open(&path).unwrap();
        second
            .renew_lease(other, EngineMode::Cli, now_millis())
            .unwrap();
        assert!(second.reap(other, now_millis()).unwrap().is_empty());
        assert_eq!(
            first.get_job(job).unwrap().unwrap().status,
            JobStatus::Running
        );
    }

    #[test]
    fn stale_and_missing_owners_lose_their_jobs_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("state.db")).unwrap();
        let (me, stale, vanished) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let now = now_millis();
        store.renew_lease(me, EngineMode::Mcp, now).unwrap();
        store
            .renew_lease(stale, EngineMode::Cli, now - LEASE_STALE_MILLIS - 1)
            .unwrap();
        let (mine, stale_job, orphan) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        queued_job(&store, mine, me);
        queued_job(&store, stale_job, stale);
        queued_job(&store, orphan, vanished);
        store.set_running(stale_job).unwrap();

        let mut reaped = store.reap(me, now).unwrap();
        reaped.sort_by_key(|job| job.sequence);
        assert_eq!(
            reaped.iter().map(|job| job.id).collect::<Vec<_>>(),
            [stale_job, orphan]
        );
        for job in &reaped {
            assert_eq!(job.status, JobStatus::Failed);
            let error = job.error.as_ref().unwrap();
            assert_eq!(error.code, "engine_lost");
        }
        assert_eq!(
            reaped[0].error.as_ref().unwrap().data.as_ref().unwrap()["owner"],
            stale.to_string()
        );
        assert_eq!(
            store.get_job(mine).unwrap().unwrap().status,
            JobStatus::Queued
        );
        assert!(
            store.reap(me, now).unwrap().is_empty(),
            "reaped exactly once"
        );
    }
}
