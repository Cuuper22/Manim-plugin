//! The idle worker of a long-lived engine (OPS §2.1): one preloaded process,
//! already past `ready` and blocked on stdin, handed to the next job whose
//! spawn key matches and replaced at once.

use super::{failure::stderr_tail, spawn::Launcher, worker::Step, worker::Worker};
use manim_director_core::ErrorBody;
use parking_lot::Mutex;
use std::{future::pending, path::PathBuf, time::Duration};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{sleep_until, Instant},
};
use tokio_util::sync::CancellationToken;

/// What a preloaded worker fixed at startup besides the interpreter, module
/// and `--preload`, which never change for one bridge. A job takes the idle
/// worker only when its key is equal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpawnKey {
    /// The opt-in address-space ceiling (`RLIMIT_AS`).
    pub memory_mb: Option<u64>,
    /// blake3 of `<root>/manim.cfg`, which Manim reads when it is imported.
    pub manim_cfg: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct PrewarmPolicy {
    /// An idle worker older than this is replaced.
    pub max_idle: Duration,
    /// Waits before respawning after the first, second and third
    /// consecutive failure; one more failure stops pre-warming until a job
    /// brings a different key.
    pub backoff: [Duration; 3],
}

impl Default for PrewarmPolicy {
    fn default() -> Self {
        Self {
            max_idle: Duration::from_secs(15 * 60),
            backoff: [1, 5, 30].map(Duration::from_secs),
        }
    }
}

struct Take {
    key: SpawnKey,
    reply: oneshot::Sender<Option<Worker>>,
}

pub(super) struct Prewarm {
    takes: mpsc::UnboundedSender<Take>,
    closed: CancellationToken,
    keeper: Mutex<Option<JoinHandle<()>>>,
}

impl Prewarm {
    pub(super) fn start(
        launcher: Launcher,
        root: PathBuf,
        key: SpawnKey,
        policy: PrewarmPolicy,
    ) -> Self {
        let (takes, requests) = mpsc::unbounded_channel();
        let closed = CancellationToken::new();
        let keeper = Keeper {
            launcher,
            root,
            key,
            policy,
            idle: None,
            failures: 0,
            respawn_at: Some(Instant::now()),
        };
        let handle = tokio::spawn(keeper.run(requests, closed.clone()));
        Self {
            takes,
            closed,
            keeper: Mutex::new(Some(handle)),
        }
    }

    /// A worker for a job with `key` (possibly still starting), or `None`
    /// when pre-warming is backing off or has stopped.
    pub(super) async fn take(&self, key: &SpawnKey) -> Option<Worker> {
        let (reply, answer) = oneshot::channel();
        self.takes
            .send(Take {
                key: key.clone(),
                reply,
            })
            .ok()?;
        answer.await.ok().flatten()
    }

    /// Retires the idle worker and stops replacing it.
    pub(super) async fn close(&self) {
        self.closed.cancel();
        let keeper = self.keeper.lock().take();
        if let Some(keeper) = keeper {
            let _ = keeper.await;
        }
    }
}

struct Keeper {
    launcher: Launcher,
    root: PathBuf,
    key: SpawnKey,
    policy: PrewarmPolicy,
    idle: Option<Worker>,
    /// Consecutive workers that failed before a job took them.
    failures: usize,
    /// When to spawn the next idle worker; `None` while one exists or once
    /// pre-warming has stopped.
    respawn_at: Option<Instant>,
}

impl Keeper {
    async fn run(mut self, mut takes: mpsc::UnboundedReceiver<Take>, closed: CancellationToken) {
        loop {
            self.replenish();
            let expires = self
                .idle
                .as_ref()
                .map(|worker| worker.spawned_at() + self.policy.max_idle);
            let respawn_at = self.respawn_at.filter(|_| self.idle.is_none());
            tokio::select! {
                biased;
                _ = closed.cancelled() => break,
                take = takes.recv() => match take {
                    Some(take) => self.hand_over(take),
                    None => break,
                },
                step = step(&mut self.idle) => if let Step::Failed(error) = step {
                    if let Some(worker) = self.idle.take() {
                        tokio::spawn(worker.discard());
                    }
                    self.failed(&error);
                },
                _ = at(expires) => {
                    if let Some(worker) = self.idle.take() {
                        tokio::spawn(worker.retire());
                    }
                    self.respawn_at = Some(Instant::now());
                }
                _ = at(respawn_at) => {}
            }
        }
        if let Some(worker) = self.idle.take() {
            worker.retire().await;
        }
    }

    fn replenish(&mut self) {
        let due = self.respawn_at.is_some_and(|at| at <= Instant::now());
        if self.idle.is_some() || !due {
            return;
        }
        self.respawn_at = None;
        match self.launcher.spawn(&self.root, true, self.key.memory_mb) {
            Ok(worker) => self.idle = Some(worker),
            Err(error) => self.failed(&error),
        }
    }

    /// Hands the idle worker to a job with the same key. A job with another
    /// key retires it and gets a worker spawned with its key, which becomes
    /// the key of every later idle worker.
    fn hand_over(&mut self, Take { key, reply }: Take) {
        if key != self.key {
            if let Some(worker) = self.idle.take() {
                tokio::spawn(worker.retire());
            }
            self.key = key;
            self.failures = 0;
            self.respawn_at = Some(Instant::now());
            self.replenish();
        }
        // A worker that died unnoticed stays idle: its next step reports the
        // failure, which schedules its replacement.
        let worker = match self.idle.as_mut().is_some_and(Worker::is_alive) {
            true => self.idle.take(),
            false => None,
        };
        if worker.is_some() {
            self.failures = 0;
            self.respawn_at = Some(Instant::now());
        }
        if let Err(Some(unwanted)) = reply.send(worker) {
            tokio::spawn(unwanted.retire());
        }
    }

    fn failed(&mut self, error: &ErrorBody) {
        self.failures += 1;
        tracing::warn!(
            code = %error.code,
            message = %error.message,
            stderr_tail = %stderr_tail(error),
            "the idle runtime worker failed"
        );
        self.respawn_at = match self.policy.backoff.get(self.failures - 1) {
            Some(delay) => Some(Instant::now() + *delay),
            None => {
                tracing::warn!(
                    failures = self.failures,
                    "pre-warming stopped; jobs start their own runtime workers"
                );
                None
            }
        };
    }
}

async fn step(idle: &mut Option<Worker>) -> Step {
    match idle {
        Some(worker) => worker.step().await,
        None => pending().await,
    }
}

async fn at(instant: Option<Instant>) {
    match instant {
        Some(instant) => sleep_until(instant).await,
        None => pending().await,
    }
}
