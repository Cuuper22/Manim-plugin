//! Background upkeep (OPS §1.8). Every engine heartbeats its lease and polls
//! cross-process cancel requests each second; long-lived engines also reap
//! stale leases every two seconds and prune history at start and every ten
//! minutes. Both loops end when the scheduler shuts down or is dropped.

use super::{artifacts, prune, Inner};
use crate::{now_millis, LEASE_STALE_MILLIS};
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
/// A pause between ticks this long means this engine was frozen.
const FREEZE_MILLIS: i64 = LEASE_STALE_MILLIS / 2;

pub(super) fn spawn(inner: &Arc<Inner>) {
    let weak = Arc::downgrade(inner);
    let closed = inner.closed.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut thaw = Thaw::new(now_millis());
        for tick in 1_u64.. {
            tokio::select! {
                _ = closed.cancelled() => break,
                _ = ticker.tick() => {}
            }
            let Some(inner) = Weak::upgrade(&weak) else {
                break;
            };
            let settled = thaw.settled(now_millis());
            let reap = inner.mode.is_long_lived() && tick % REAP_EVERY_TICKS == 0;
            inner.upkeep(settled && reap).await;
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

/// Holds the reaper back for a while after this engine was frozen (a system
/// suspend, SIGSTOP): every other engine's heartbeat then looks stale until
/// it has ticked again too. Wall-clock time, because the monotonic clock
/// stands still through a suspend.
struct Thaw {
    last: i64,
    woke: Option<i64>,
}

impl Thaw {
    fn new(now: i64) -> Self {
        Self {
            last: now,
            woke: None,
        }
    }

    fn settled(&mut self, now: i64) -> bool {
        if now - self.last > FREEZE_MILLIS {
            self.woke = Some(now);
        }
        self.last = now;
        self.woke.is_none_or(|woke| now - woke >= FREEZE_MILLIS)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frozen_engine_waits_for_the_others_to_tick_before_reaping() {
        let mut thaw = Thaw::new(0);
        assert!(thaw.settled(1_000));
        assert!(!thaw.settled(13_000), "woke from a 12 s freeze");
        assert!(!thaw.settled(14_000));
        assert!(thaw.settled(13_000 + FREEZE_MILLIS));
    }
}
