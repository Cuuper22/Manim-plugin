//! Background upkeep (OPS §1.8). Every engine heartbeats its lease and polls
//! cross-process cancel requests each second; long-lived engines also reap
//! stale leases every two seconds and prune history at start and every ten
//! minutes. Both loops end when the scheduler shuts down or is dropped.

use super::{artifacts, prune, Inner};
use crate::now_millis;
use manim_director_core::{CancelledBy, JobRecord};
use std::{
    path::Path,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::time::MissedTickBehavior;

const TICK: Duration = Duration::from_secs(1);
const REAP_EVERY_TICKS: u64 = 2;
const PRUNE_INTERVAL: Duration = Duration::from_secs(600);

pub(super) fn spawn(inner: &Arc<Inner>) {
    let weak = Arc::downgrade(inner);
    let closed = inner.closed.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        for tick in 1_u64.. {
            tokio::select! {
                _ = closed.cancelled() => break,
                _ = ticker.tick() => {}
            }
            let Some(inner) = Weak::upgrade(&weak) else {
                break;
            };
            let reap = inner.mode.is_long_lived() && tick % REAP_EVERY_TICKS == 0;
            inner.upkeep(reap).await;
        }
    });
    if !inner.mode.is_long_lived() {
        return;
    }
    let weak = Arc::downgrade(inner);
    let closed = inner.closed.clone();
    tokio::spawn(async move {
        loop {
            let Some(inner) = Weak::upgrade(&weak) else {
                break;
            };
            inner.prune().await;
            drop(inner);
            tokio::select! {
                _ = closed.cancelled() => break,
                _ = tokio::time::sleep(PRUNE_INTERVAL) => {}
            }
        }
    });
}

impl Inner {
    async fn upkeep(&self, reap: bool) {
        let (me, mode, root) = (self.instance_id, self.mode, self.root.clone());
        let work = self
            .store
            .blocking(move |store| {
                store.renew_lease(me, mode, now_millis())?;
                let cancels = store.cancel_requests(me)?;
                let reaped = match reap {
                    true => store.reap(me, now_millis())?,
                    false => Vec::new(),
                };
                remove_out_dirs(&root, &reaped);
                Ok((cancels, reaped))
            })
            .await;
        let (cancels, reaped) = match work {
            Ok(work) => work,
            Err(error) => {
                tracing::warn!(%error, "engine upkeep failed");
                return;
            }
        };
        for id in cancels {
            if let Err(error) = self.cancel_local(id, CancelledBy::Client).await {
                tracing::warn!(%id, %error, "could not cancel a job on request");
            }
        }
        for job in &reaped {
            self.emit_finished(job);
        }
    }

    async fn prune(&self) {
        let (root, policy) = (self.root.clone(), self.prune_policy);
        match self
            .store
            .blocking(move |store| prune::prune(store, &root, policy))
            .await
        {
            Ok(pruned) => tracing::debug!(?pruned, "pruned history"),
            Err(error) => tracing::warn!(%error, "pruning failed"),
        }
    }
}

/// Deletes the artifact directories of jobs another engine abandoned.
pub(super) fn remove_out_dirs(root: &Path, jobs: &[JobRecord]) {
    for job in jobs.iter().filter(|job| job.task.out_dir().is_some()) {
        if let Err(error) = artifacts::remove_out_dir(root, job.id) {
            tracing::warn!(id = %job.id, %error, "could not remove an abandoned job's artifacts");
        }
    }
}
