//! Cross-process render serialization (OPS §1.5): one render or still per
//! scene key at a time, whichever engine runs it.

use super::Store;
use anyhow::Result;
use manim_director_core::Timestamp;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

impl Store {
    /// Takes `key` for `job`; `false` while another live job holds it. A
    /// lock left behind by a job that already ended is taken over.
    pub fn try_lock_scene(&self, key: &str, job: Uuid, owner: Uuid) -> Result<bool> {
        let mut conn = self.conn.lock();
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM scene_locks WHERE key=?1 AND job_id IN (
                SELECT id FROM jobs WHERE status IN ('succeeded','failed','cancelled'))",
            [key],
        )?;
        transaction.execute(
            "INSERT INTO scene_locks(key, job_id, owner, acquired_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO NOTHING",
            params![
                key,
                job.to_string(),
                owner.to_string(),
                Timestamp::now().to_string()
            ],
        )?;
        let holder: Option<String> = transaction
            .query_row(
                "SELECT job_id FROM scene_locks WHERE key=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        transaction.commit()?;
        Ok(holder.as_deref() == Some(job.to_string().as_str()))
    }

    pub fn unlock_scene(&self, job: Uuid) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM scene_locks WHERE job_id=?1", [job.to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{diagnosis, queued_job, store};
    use super::*;

    #[test]
    fn a_scene_key_has_one_holder_until_it_is_released_or_its_job_ends() {
        let (_dir, store) = store();
        let owner = Uuid::new_v4();
        let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
        queued_job(&store, first, owner);
        queued_job(&store, second, owner);
        assert!(store.try_lock_scene("Intro", first, owner).unwrap());
        assert!(store.try_lock_scene("Intro", first, owner).unwrap());
        assert!(!store.try_lock_scene("Intro", second, owner).unwrap());
        assert!(store.try_lock_scene("Outro", second, owner).unwrap());
        store.unlock_scene(second).unwrap();

        store.set_running(first).unwrap();
        store.finish_success(first, &diagnosis(), None).unwrap();
        assert!(
            store.try_lock_scene("Intro", second, owner).unwrap(),
            "a finished job's lock does not block"
        );
    }
}
