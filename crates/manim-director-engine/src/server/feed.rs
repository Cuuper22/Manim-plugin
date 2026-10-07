//! Job and progress events (HTTP §8.3–§8.4): this engine's own transitions
//! as they commit, other engines' from polling the shared store. Each job is
//! published at most once per status rank, so the reaper and the poller can
//! both see a transition without a client seeing it twice.

use super::{
    events::{ResyncReason, ServerEvent},
    state::AppState,
};
use crate::workspace::{job_summary, Section};
use manim_director_core::{EngineEvent, JobRecord, JobStatus, Operation, Progress, Timestamp};
use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};
use tokio::{
    sync::broadcast::{self, error::RecvError},
    time::{Instant, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const FOREIGN_POLL: Duration = Duration::from_millis(500);
/// At most four progress events per second per job; the latest wins.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Finished jobs remembered for de-duplication.
const REMEMBERED: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Queued,
    Running,
    Terminal,
}

impl Rank {
    fn of(status: JobStatus) -> Self {
        match status {
            JobStatus::Queued => Self::Queued,
            JobStatus::Running => Self::Running,
            _ => Self::Terminal,
        }
    }
}

#[derive(Default)]
struct Throttle {
    sent: Option<Instant>,
    pending: Option<Progress>,
}

struct Feed {
    state: AppState,
    published: HashMap<Uuid, Rank>,
    finished: VecDeque<Uuid>,
    throttles: HashMap<Uuid, Throttle>,
    /// The newest sequence the poller has seen.
    foreign_after: i64,
    /// Active foreign jobs and the progress time last published for them.
    foreign_active: HashMap<Uuid, Option<Timestamp>>,
    dirty: Vec<Section>,
}

/// `events` is subscribed before anything can submit, so no transition of
/// this engine is missed.
pub async fn run(
    state: AppState,
    mut events: broadcast::Receiver<EngineEvent>,
    shutdown: CancellationToken,
) {
    let store = state.scheduler.store().clone();
    let foreign_after = match store.blocking(|store| store.last_sequence()).await {
        Ok(sequence) => sequence,
        Err(error) => {
            tracing::warn!(%error, "could not read the job store");
            0
        }
    };
    let mut feed = Feed {
        state,
        published: HashMap::new(),
        finished: VecDeque::new(),
        throttles: HashMap::new(),
        foreign_after,
        foreign_active: HashMap::new(),
        dirty: Vec::new(),
    };
    let mut poll = tokio::time::interval(FOREIGN_POLL);
    let mut flush = tokio::time::interval(PROGRESS_INTERVAL);
    for ticker in [&mut poll, &mut flush] {
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    }
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            event = events.recv() => match event {
                Ok(EngineEvent::Job(job)) => feed.job(&job).await,
                Ok(EngineEvent::Progress { job_id, progress }) => feed.progress(job_id, progress),
                // Events were lost: every client must reload.
                Err(RecvError::Lagged(_)) => feed.state.hub.publish(&ServerEvent::Resync {
                    reason: ResyncReason::Lagged,
                }),
                Err(RecvError::Closed) => break,
            },
            _ = poll.tick() => feed.poll_foreign().await,
            _ = flush.tick() => feed.flush_progress(),
        }
        // Patches wait until the burst of transitions behind them is out.
        if events.is_empty() {
            feed.publish_dirty().await;
        }
    }
}

impl Feed {
    async fn job(&mut self, job: &JobRecord) {
        let rank = Rank::of(job.status);
        if self
            .published
            .get(&job.id)
            .is_some_and(|seen| *seen >= rank)
        {
            return;
        }
        self.published.insert(job.id, rank);
        if rank == Rank::Terminal {
            self.throttles.remove(&job.id);
            self.finished.push_back(job.id);
            if self.finished.len() > REMEMBERED {
                if let Some(evicted) = self.finished.pop_front() {
                    self.published.remove(&evicted);
                }
            }
            self.dirty.extend(affected_sections(job.operation));
        }
        let root = self.state.root().to_path_buf();
        let record = job.clone();
        match tokio::task::spawn_blocking(move || job_summary(&root, &record)).await {
            Ok(view) => self.state.hub.publish(&ServerEvent::Job {
                job: Box::new(view),
            }),
            Err(error) => tracing::warn!(%error, "could not project a job"),
        }
    }

    fn progress(&mut self, job_id: Uuid, progress: Progress) {
        if self.published.get(&job_id) != Some(&Rank::Running) {
            return;
        }
        let throttle = self.throttles.entry(job_id).or_default();
        if throttle
            .sent
            .is_some_and(|sent| sent.elapsed() < PROGRESS_INTERVAL)
        {
            throttle.pending = Some(progress);
            return;
        }
        throttle.sent = Some(Instant::now());
        throttle.pending = None;
        self.state
            .hub
            .publish(&ServerEvent::Progress { job_id, progress });
    }

    fn flush_progress(&mut self) {
        for (job_id, throttle) in &mut self.throttles {
            if throttle
                .sent
                .is_some_and(|sent| sent.elapsed() < PROGRESS_INTERVAL)
            {
                continue;
            }
            if let Some(progress) = throttle.pending.take() {
                throttle.sent = Some(Instant::now());
                self.state.hub.publish(&ServerEvent::Progress {
                    job_id: *job_id,
                    progress,
                });
            }
        }
    }

    /// Other engines' jobs: new ones, active ones, and those active when last
    /// seen (to catch their terminal transition).
    async fn poll_foreign(&mut self) {
        let me = self.state.scheduler.instance_id();
        let after = self.foreign_after;
        let watched: Vec<Uuid> = self.foreign_active.keys().copied().collect();
        let polled = self
            .state
            .scheduler
            .store()
            .blocking(move |store| store.foreign_jobs(me, after, &watched))
            .await;
        let jobs = match polled {
            Ok(jobs) => jobs,
            Err(error) => {
                tracing::warn!(%error, "could not poll other engines' jobs");
                return;
            }
        };
        for job in jobs {
            self.foreign_after = self.foreign_after.max(job.sequence);
            self.job(&job).await;
            match job.status {
                JobStatus::Queued => {
                    self.foreign_active.insert(job.id, None);
                }
                JobStatus::Running => {
                    let updated = job.progress.as_ref().map(|progress| progress.updated_at);
                    let seen = self.foreign_active.insert(job.id, updated);
                    if let Some(progress) = job.progress.filter(|_| seen != Some(updated)) {
                        self.progress(job.id, progress);
                    }
                }
                _ => {
                    self.foreign_active.remove(&job.id);
                }
            }
        }
    }

    async fn publish_dirty(&mut self) {
        if self.dirty.is_empty() {
            return;
        }
        let mut wanted = std::mem::take(&mut self.dirty);
        wanted.sort_by_key(|section| Section::ALL.iter().position(|known| known == section));
        wanted.dedup();
        self.state.publish_sections(&wanted).await;
    }
}

/// The workspace sections a finished job can change.
fn affected_sections(operation: Operation) -> &'static [Section] {
    match operation {
        Operation::Render
        | Operation::Still
        | Operation::Frame
        | Operation::ContactSheet
        | Operation::Qa => &[Section::Latest, Section::Findings],
        Operation::Doctor => &[Section::Doctor, Section::Findings],
        Operation::Export | Operation::Captions | Operation::Ingest => &[Section::Project],
        Operation::Init | Operation::Discover | Operation::Diagnose | Operation::ValidateMath => {
            &[]
        }
    }
}
