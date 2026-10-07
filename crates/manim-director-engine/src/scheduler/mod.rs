//! The job queue: submit (resolve once, cache, coalesce), run on a bounded
//! worker pool, cancel, wait and shut down. Every database and filesystem
//! step runs on the blocking pool.

mod active;
mod artifacts;
mod direct;
mod latest;
mod maintenance;
mod prune;
mod recorder;
mod request;

pub use artifacts::probe_media;
pub use direct::init_project;
pub use latest::{latest, latest_render, Latest};
pub use prune::{prune, PrunePolicy, Pruned};
pub use request::{cli_project_path, parse_params, parse_request, Frontend};

use crate::{
    cache, now_millis, state_db_path, BridgeConfig, BridgeOutcome, EngineMode, Finish, Invocation,
    NewJob, RuntimeBridge, Store,
};
use active::Active;
use manim_director_core::{
    python_sources, CancelledBy, DirectorSpec, DiscoverResult, DiscoverTask, EngineError,
    EngineEvent, ErrorBody, Finding, JobOrigin, JobRecord, JobStatus, JobSummary, MediaInfo,
    MediaSource, Operation, OperationRequest, OperationResult, ProgressPhase, Task,
};
use parking_lot::Mutex;
use recorder::Recorder;
use request::{ProjectContext, ResolvedJob};
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{broadcast, mpsc, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const DISCOVER_TIMEOUT: Duration = Duration::from_secs(30);
/// Covers the bridge's kill grace for every running job.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub mode: EngineMode,
    /// Jobs run concurrently by this process.
    pub workers: usize,
    /// Queued jobs this process accepts before `queue_full`.
    pub queue_capacity: usize,
    pub bridge: BridgeConfig,
    pub prune: PrunePolicy,
}

impl SchedulerConfig {
    pub fn new(mode: EngineMode) -> Self {
        Self {
            mode,
            workers: env_usize("MANIM_DIRECTOR_WORKERS", 2).clamp(1, 32),
            queue_capacity: env_usize("MANIM_DIRECTOR_QUEUE", 128).clamp(1, 4096),
            bridge: BridgeConfig::default(),
            prune: PrunePolicy::from_env(),
        }
    }
}

fn env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

/// How a submission was satisfied: HTTP answers 202 for `Queued`, 200 otherwise.
#[derive(Debug, Clone)]
pub enum Submission {
    Queued(JobRecord),
    Cached(JobRecord),
    Coalesced(JobRecord),
}

impl Submission {
    pub fn job(&self) -> &JobRecord {
        match self {
            Self::Queued(job) | Self::Cached(job) | Self::Coalesced(job) => job,
        }
    }

    pub fn into_job(self) -> JobRecord {
        match self {
            Self::Queued(job) | Self::Cached(job) | Self::Coalesced(job) => job,
        }
    }
}

#[derive(Clone)]
pub struct Scheduler {
    inner: Arc<Inner>,
}

struct Inner {
    root: PathBuf,
    instance_id: Uuid,
    mode: EngineMode,
    store: Arc<Store>,
    bridge: RuntimeBridge,
    events: broadcast::Sender<EngineEvent>,
    queue: mpsc::UnboundedSender<Queued>,
    slots: Arc<Semaphore>,
    queue_capacity: usize,
    active: Mutex<HashMap<Uuid, Arc<Active>>>,
    last_valid_spec: Mutex<Option<DirectorSpec>>,
    prune_policy: PrunePolicy,
    /// Cancelled when shutdown begins: upkeep stops, submissions are refused.
    closed: CancellationToken,
}

struct Queued {
    job: JobRecord,
    context: RunContext,
    active: Arc<Active>,
}

/// Per-job facts resolved at submit that the record does not carry.
struct RunContext {
    source_media: Option<MediaInfo>,
    source: Option<MediaSource>,
    artifact_budget: u64,
}

impl Scheduler {
    /// Opens the project's job store and starts an engine on it.
    pub async fn open(root: impl AsRef<Path>, config: SchedulerConfig) -> anyhow::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let (root, store) = tokio::task::spawn_blocking(move || {
            let root = root.canonicalize()?;
            let store = Store::open(state_db_path(&root))?;
            anyhow::Ok((root, store))
        })
        .await??;
        Self::start(root, Arc::new(store), config).await
    }

    /// Starts an engine instance: takes its lease, fails the jobs of engines
    /// whose lease went stale, then accepts work.
    pub async fn start(
        root: impl AsRef<Path>,
        store: Arc<Store>,
        config: SchedulerConfig,
    ) -> anyhow::Result<Self> {
        let instance_id = Uuid::new_v4();
        let mode = config.mode;
        let root = root.as_ref().to_path_buf();
        let root = store
            .blocking(move |store| {
                let root = root.canonicalize()?;
                store.renew_lease(instance_id, mode, now_millis())?;
                let reaped = store.reap(instance_id, now_millis())?;
                maintenance::remove_out_dirs(&root, &reaped);
                Ok(root)
            })
            .await?;
        let (queue, receiver) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(1024);
        let inner = Arc::new(Inner {
            root,
            instance_id,
            mode,
            store,
            bridge: RuntimeBridge::new(config.bridge),
            events,
            queue,
            slots: Arc::new(Semaphore::new(config.queue_capacity)),
            queue_capacity: config.queue_capacity,
            active: Mutex::new(HashMap::new()),
            last_valid_spec: Mutex::new(None),
            prune_policy: config.prune,
            closed: CancellationToken::new(),
        });
        dispatch(&inner, receiver, config.workers);
        maintenance::spawn(&inner);
        Ok(Self { inner })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.inner.store
    }

    pub fn instance_id(&self) -> Uuid {
        self.inner.instance_id
    }

    /// The interpreter the bridge runs, for tools that need the same Python.
    pub fn python(&self) -> &Path {
        &self.inner.bridge.config().python
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    /// Validates, resolves and queues a job request, or answers it from the
    /// cache or an identical in-flight job.
    pub async fn submit(
        &self,
        origin: JobOrigin,
        request: OperationRequest,
    ) -> Result<Submission, EngineError> {
        request.validate()?;
        let operation = request.operation();
        if operation.is_direct() {
            return Err(EngineError::OperationNotAllowed {
                operation,
                allowed: Operation::job_operations().collect(),
            });
        }
        if self.inner.closed.is_cancelled() {
            return Err(EngineError::internal("the engine is shutting down"));
        }
        let inner = self.inner.clone();
        let plan = tokio::task::spawn_blocking(move || inner.plan(origin, request))
            .await
            .map_err(EngineError::internal)??;
        let pending = match plan {
            Plan::Done(submission) => {
                if let Submission::Cached(job) = submission.as_ref() {
                    self.inner.emit_finished(job);
                }
                return Ok(*submission);
            }
            Plan::Enqueue(pending) => pending,
        };
        let slot =
            self.inner
                .slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| EngineError::QueueFull {
                    capacity: self.inner.queue_capacity,
                })?;
        let inner = self.inner.clone();
        let (job, context) = tokio::task::spawn_blocking(move || inner.insert(pending))
            .await
            .map_err(EngineError::internal)??;
        let active = Arc::new(Active::new(slot));
        self.inner.active.lock().insert(job.id, active.clone());
        let _ = self.inner.events.send(EngineEvent::JobQueued {
            job: JobSummary::from(&job),
        });
        let queued = Queued {
            job: job.clone(),
            context,
            active,
        };
        if self.inner.queue.send(queued).is_err() {
            self.inner.active.lock().remove(&job.id);
            return Err(EngineError::internal("the job dispatcher stopped"));
        }
        if self.inner.closed.is_cancelled() {
            self.inner
                .cancel_local(job.id, CancelledBy::Shutdown)
                .await
                .map_err(EngineError::internal)?;
        }
        Ok(Submission::Queued(job))
    }

    /// Requests cancellation from any engine; terminal jobs stay as they are.
    /// This engine's own jobs end at once (queued) or once their runtime is
    /// killed (running); another engine acts on the flag within a second.
    pub async fn cancel(&self, id: Uuid) -> Result<JobRecord, EngineError> {
        self.inner
            .store
            .blocking(move |store| store.request_cancel(id))
            .await
            .map_err(EngineError::internal)?;
        self.inner
            .cancel_local(id, CancelledBy::Client)
            .await
            .map_err(EngineError::internal)?;
        self.inner
            .store
            .blocking(move |store| store.get_job(id))
            .await
            .map_err(EngineError::internal)?
            .ok_or_else(|| EngineError::job_not_found(id))
    }

    /// Resolves when the job reaches a terminal status, whichever engine runs it.
    pub async fn wait(&self, id: Uuid) -> Result<JobRecord, EngineError> {
        let mut events = self.subscribe();
        loop {
            let job = self
                .inner
                .store
                .blocking(move |store| store.get_job(id))
                .await
                .map_err(EngineError::internal)?
                .ok_or_else(|| EngineError::job_not_found(id))?;
            if job.status.is_terminal() {
                return Ok(job);
            }
            let _ = tokio::time::timeout(Duration::from_millis(250), events.recv()).await;
        }
    }

    /// Cancels this engine's jobs (`by: shutdown`), waits for their runtimes
    /// to exit, and gives up the lease.
    pub async fn shutdown(&self) {
        self.inner.closed.cancel();
        let ids: Vec<Uuid> = self.inner.active.lock().keys().copied().collect();
        for id in ids {
            if let Err(error) = self.inner.cancel_local(id, CancelledBy::Shutdown).await {
                tracing::warn!(%id, %error, "could not cancel a job at shutdown");
            }
        }
        let deadline = tokio::time::Instant::now() + SHUTDOWN_GRACE;
        while !self.inner.active.lock().is_empty() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let me = self.inner.instance_id;
        if let Err(error) = self
            .inner
            .store
            .blocking(move |store| store.release_lease(me))
            .await
        {
            tracing::warn!(%error, "could not release the engine lease");
        }
    }

    /// Scans the project's Python sources (a direct, cached operation).
    pub async fn discover(&self) -> Result<DiscoverResult, EngineError> {
        let inner = self.inner.clone();
        let prepared = tokio::task::spawn_blocking(move || inner.prepare_discover())
            .await
            .map_err(EngineError::internal)??;
        let mut result = match prepared.cached {
            Some(result) => result,
            None => {
                let outcome = direct::run(
                    &self.inner.bridge,
                    &self.inner.root,
                    &prepared.task,
                    DISCOVER_TIMEOUT,
                )
                .await?;
                let OperationResult::Discover(result) = outcome else {
                    return Err(EngineError::internal("discover returned another result"));
                };
                let cached = OperationResult::Discover(result.clone());
                let fingerprint = prepared.fingerprint.clone();
                self.inner
                    .store
                    .blocking(move |store| {
                        store.cache_put(&fingerprint, None, &cached, Operation::Discover)
                    })
                    .await
                    .map_err(EngineError::internal)?;
                result
            }
        };
        result.files = prepared.files;
        result.truncated |= prepared.truncated;
        for path in prepared.oversized {
            result.findings.push(Finding::warning(
                "file_too_large",
                format!("{path} is over 2 MiB and was not scanned."),
            ));
        }
        if prepared.truncated {
            result.findings.push(Finding::warning(
                "index_truncated",
                "Only the first 500 Python files were scanned.",
            ));
        }
        Ok(result)
    }

    #[cfg(test)]
    fn active_jobs(&self) -> usize {
        self.inner.active.lock().len()
    }
}

/// Hands queued jobs to at most `workers` concurrent runs, in order. A worker
/// is reserved before the next job is taken, so the channel holds exactly the
/// jobs still waiting.
fn dispatch(inner: &Arc<Inner>, mut receiver: mpsc::UnboundedReceiver<Queued>, workers: usize) {
    let workers = Arc::new(Semaphore::new(workers));
    let dispatcher: Weak<Inner> = Arc::downgrade(inner);
    tokio::spawn(async move {
        loop {
            let Ok(permit) = workers.clone().acquire_owned().await else {
                break;
            };
            let Some(queued) = receiver.recv().await else {
                break;
            };
            let Some(inner) = dispatcher.upgrade() else {
                break;
            };
            tokio::spawn(async move {
                let _permit = permit;
                inner.run(queued).await;
            });
        }
    });
}

enum Plan {
    Done(Box<Submission>),
    Enqueue(Box<Pending>),
}

struct Pending {
    id: Uuid,
    origin: JobOrigin,
    request: OperationRequest,
    resolved: ResolvedJob,
    fingerprint: Option<String>,
    scene_revision: Option<String>,
}

struct PreparedDiscover {
    task: Task,
    fingerprint: String,
    cached: Option<DiscoverResult>,
    files: u32,
    truncated: bool,
    oversized: Vec<String>,
}

enum Outcome {
    Succeeded(Box<OperationResult>),
    Failed(ErrorBody),
    Cancelled(CancelledBy),
}

impl Inner {
    fn load_spec(&self) -> Result<DirectorSpec, EngineError> {
        let spec = DirectorSpec::load(&self.root).map_err(EngineError::from)?;
        *self.last_valid_spec.lock() = Some(spec.clone());
        Ok(spec)
    }

    /// Stands in for the runtime's own identity until the bridge records it.
    fn runtime_identity(&self) -> String {
        let config = self.bridge.config();
        format!("{}\0{}", config.python.display(), config.module)
    }

    fn plan(&self, origin: JobOrigin, request: OperationRequest) -> Result<Plan, EngineError> {
        let spec = self.load_spec();
        let id = Uuid::new_v4();
        let context = ProjectContext {
            root: &self.root,
            spec: &spec,
            store: &self.store,
        };
        let resolved = request::resolve(&context, id, &request)?;
        request::check_request_size(&self.root, &id.to_string(), &resolved.task)?;
        let fresh = matches!(
            &request,
            OperationRequest::Render(params) if params.fresh
        ) || matches!(&request, OperationRequest::Still(params) if params.fresh);
        let fingerprint = match &spec {
            Ok(spec) if request.operation().cacheable() && !fresh => Some(
                cache::fingerprint(&self.root, spec, &self.runtime_identity(), &resolved.task)
                    .map_err(EngineError::internal)?,
            ),
            _ => None,
        };
        let scene_revision = resolved.scene_revision.clone().or_else(|| {
            request::scene_revision(
                &self.root,
                resolved.scene_file.as_deref(),
                fingerprint.as_ref(),
            )
        });
        if let Some(fingerprint) = &fingerprint {
            let store = &self.store;
            if let Some(entry) = store
                .cache_get(&fingerprint.value)
                .map_err(EngineError::internal)?
            {
                if artifacts::intact(&self.root, &entry.result) {
                    let scene = entry.result.scene().cloned();
                    let new_job = NewJob {
                        id,
                        origin,
                        owner: self.instance_id,
                        request: &request,
                        task: &resolved.task,
                        limits: resolved.limits,
                        fingerprint: Some(&fingerprint.value),
                        source_job_id: None,
                        scene_class: scene.as_ref().map(|scene| scene.name.as_str()),
                        scene_file: scene.as_ref().map(|scene| scene.file.as_str()),
                        scene_revision: scene_revision.as_deref(),
                        profile: resolved.profile.as_deref(),
                    };
                    let job = store
                        .insert_cached_job(&new_job, entry.job_id, &entry.result)
                        .map_err(EngineError::internal)?;
                    return Ok(Plan::Done(Box::new(Submission::Cached(job))));
                }
                store
                    .cache_delete(&fingerprint.value)
                    .map_err(EngineError::internal)?;
            }
            if let Some(job) = store
                .active_job_with_fingerprint(&fingerprint.value)
                .map_err(EngineError::internal)?
            {
                return Ok(Plan::Done(Box::new(Submission::Coalesced(job))));
            }
        }
        Ok(Plan::Enqueue(Box::new(Pending {
            id,
            origin,
            request,
            resolved,
            fingerprint: fingerprint.map(|fingerprint| fingerprint.value),
            scene_revision,
        })))
    }

    fn insert(&self, pending: Box<Pending>) -> Result<(JobRecord, RunContext), EngineError> {
        let Pending {
            id,
            origin,
            request,
            resolved,
            fingerprint,
            scene_revision,
        } = *pending;
        let source = resolved.source.as_ref();
        let job = self
            .store
            .insert_job(&NewJob {
                id,
                origin,
                owner: self.instance_id,
                request: &request,
                task: &resolved.task,
                limits: resolved.limits,
                fingerprint: fingerprint.as_deref(),
                source_job_id: source.and_then(|source| source.reference.job_id),
                scene_class: resolved.scene_class.as_deref(),
                scene_file: resolved.scene_file.as_deref(),
                scene_revision: scene_revision.as_deref(),
                profile: resolved.profile.as_deref(),
            })
            .map_err(EngineError::internal)?;
        let context = RunContext {
            source_media: source.map(|source| source.media.clone()),
            source: source.map(|source| source.reference.clone()),
            artifact_budget: resolved.artifact_budget,
        };
        Ok((job, context))
    }

    fn prepare_discover(&self) -> Result<PreparedDiscover, EngineError> {
        let spec = self
            .load_spec()
            .ok()
            .or_else(|| self.last_valid_spec.lock().clone())
            .unwrap_or_else(DirectorSpec::defaults);
        let sources = python_sources(&self.root, &spec);
        let truncated = sources.truncated();
        let files = sources.files.len() as u32;
        let task = Task::Discover(DiscoverTask {
            files: sources.files,
        });
        let fingerprint = cache::fingerprint(&self.root, &spec, &self.runtime_identity(), &task)
            .map_err(EngineError::internal)?
            .value;
        let cached = match self
            .store
            .cache_get(&fingerprint)
            .map_err(EngineError::internal)?
        {
            Some(entry) => match entry.result {
                OperationResult::Discover(result) => Some(result),
                _ => None,
            },
            None => None,
        };
        Ok(PreparedDiscover {
            task,
            fingerprint,
            cached,
            files,
            truncated,
            oversized: sources.oversized,
        })
    }

    /// Cancels one of this engine's jobs. A queued job ends here; a running
    /// one ends when its worker sees the token and kills the runtime.
    async fn cancel_local(&self, id: Uuid, by: CancelledBy) -> anyhow::Result<()> {
        let Some(active) = self.active.lock().get(&id).cloned() else {
            return Ok(());
        };
        active.cancel(by);
        let error = ErrorBody::cancelled(by);
        let cancelled = self
            .store
            .blocking(move |store| store.cancel_queued(id, &error))
            .await?;
        if let Some(job) = cancelled {
            active.leave_queue();
            self.active.lock().remove(&id);
            self.publish(&active, &job);
        }
        Ok(())
    }

    async fn run(self: Arc<Self>, queued: Queued) {
        let Queued {
            job,
            context,
            active,
        } = queued;
        active.leave_queue();
        let id = job.id;
        let root = self.root.clone();
        let started =
            self.store
                .blocking(move |store| {
                    let Some(running) = store.set_running(id)? else {
                        return Ok(Err(store.get_job(id)?));
                    };
                    let out_dir = match running.task.out_dir() {
                        Some(out_dir) => artifacts::create_out_dir(&root, out_dir)
                            .map_err(|error| error.to_string()),
                        None => Ok(()),
                    };
                    Ok(Ok((running, out_dir)))
                })
                .await;
        match started {
            Ok(Ok((running, out_dir))) => {
                let _ = self.events.send(EngineEvent::JobStarted {
                    job: JobSummary::from(&running),
                });
                let recorder = Recorder::start(self.store.clone(), self.events.clone(), id);
                recorder.engine_phase(
                    ProgressPhase::Starting,
                    None,
                    Some("waiting for the runtime"),
                );
                let outcome = match out_dir {
                    _ if active.token.is_cancelled() => Outcome::Cancelled(active.cancelled_by()),
                    Err(error) => Outcome::Failed(ErrorBody::internal(format!(
                        "Could not create the job's artifact directory: {error}"
                    ))),
                    Ok(()) => self.execute(&job, &context, &active, &recorder).await,
                };
                recorder.close().await;
                self.finish(&job, &active, outcome).await;
            }
            // Cancelled while queued, or failed by another engine's reaper.
            Ok(Err(current)) => {
                if let Some(current) = current.filter(|job| job.status.is_terminal()) {
                    self.publish(&active, &current);
                }
            }
            Err(error) => tracing::error!(%id, %error, "could not start the job"),
        }
        self.active.lock().remove(&id);
    }

    async fn execute(
        &self,
        job: &JobRecord,
        context: &RunContext,
        active: &Active,
        recorder: &Recorder,
    ) -> Outcome {
        let request_id = job.id.to_string();
        let attempt = active.token.child_token();
        let timeout = Duration::from_secs(job.limits.timeout_seconds);
        let (outcome, timed_out) = {
            let run = self.bridge.run(
                Invocation {
                    request_id: &request_id,
                    project_root: &self.root,
                    task: &job.task,
                    preload: true,
                    memory_mb: job.limits.memory_mb,
                },
                attempt.clone(),
                |event| recorder.record(event),
            );
            tokio::pin!(run);
            tokio::select! {
                outcome = &mut run => (outcome, false),
                _ = tokio::time::sleep(timeout) => {
                    attempt.cancel();
                    ((&mut run).await, true)
                }
            }
        };
        match outcome {
            BridgeOutcome::Succeeded(value) => {
                let count = value["artifacts"].as_array().map_or(0, Vec::len) as u64;
                if count > 0 {
                    recorder.engine_phase(ProgressPhase::Validate, Some(count), None);
                }
                let root = self.root.clone();
                let task = job.task.clone();
                let source_media = context.source_media.clone();
                let budget = context.artifact_budget;
                let accepted = tokio::task::spawn_blocking(move || {
                    accept_result(&root, &task, value, source_media.as_ref(), budget)
                })
                .await
                .unwrap_or_else(|error| Err(ErrorBody::internal(error.to_string())));
                match accepted {
                    Ok(mut result) => {
                        if let Some(source) = &context.source {
                            result.set_source(source.clone());
                        }
                        Outcome::Succeeded(Box::new(result))
                    }
                    Err(error) => Outcome::Failed(error),
                }
            }
            BridgeOutcome::Failed(error) => Outcome::Failed(error),
            BridgeOutcome::Cancelled if timed_out => {
                Outcome::Failed(ErrorBody::timeout(job.limits.timeout_seconds))
            }
            BridgeOutcome::Cancelled => Outcome::Cancelled(active.cancelled_by()),
        }
    }

    /// Records the job's one terminal transition. Losing that race to another
    /// engine's reaper is logged and the winner's record is published instead.
    async fn finish(&self, job: &JobRecord, active: &Active, outcome: Outcome) {
        let id = job.id;
        let root = self.root.clone();
        let job_file = job.scene_file.clone();
        let fingerprint = job.fingerprint.clone();
        let operation = job.operation;
        let out_dir = job.task.out_dir().cloned();
        let recorded = self
            .store
            .blocking(move |store| {
                let finish = match &outcome {
                    Outcome::Succeeded(result) => {
                        let revision = result
                            .scene()
                            .filter(|scene| job_file.as_deref() != Some(scene.file.as_str()))
                            .and_then(|scene| cache::file_revision(&root.join(&scene.file)).ok());
                        store.finish_success(id, result, revision.as_deref())?
                    }
                    Outcome::Failed(error) => store.finish_error(id, JobStatus::Failed, error)?,
                    Outcome::Cancelled(by) => store.finish_error(
                        id,
                        JobStatus::Cancelled,
                        &ErrorBody::cancelled(*by),
                    )?,
                };
                let kept = matches!(
                    (&finish, &outcome),
                    (Finish::Ended(_), Outcome::Succeeded(_))
                );
                if let (true, Some(fingerprint), Outcome::Succeeded(result)) =
                    (kept, &fingerprint, &outcome)
                {
                    if let Err(error) = store.cache_put(fingerprint, Some(id), result, operation) {
                        tracing::warn!(%id, %error, "could not cache the result");
                    }
                }
                if let (false, Some(out_dir)) = (kept, &out_dir) {
                    if let Err(error) = artifacts::remove_out_dir(&root, out_dir) {
                        tracing::warn!(%id, %error, "could not remove the job's artifact directory");
                    }
                }
                Ok(finish)
            })
            .await;
        match recorded {
            Ok(Finish::Ended(record)) => self.publish(active, &record),
            Ok(Finish::Superseded(record)) => {
                tracing::warn!(
                    %id,
                    status = %record.status,
                    "the job had already ended elsewhere; this run's outcome was discarded"
                );
                self.publish(active, &record);
            }
            Err(error) => tracing::error!(%id, %error, "could not record the job outcome"),
        }
    }

    /// Publishes a job's terminal state at most once per job.
    fn publish(&self, active: &Active, job: &JobRecord) {
        if active.claim_publication() {
            self.emit_finished(job);
        }
    }

    fn emit_finished(&self, job: &JobRecord) {
        let _ = self.events.send(EngineEvent::JobFinished {
            job: JobSummary::from(job),
            result: job.result.clone().map(Box::new),
            error: job.error.clone(),
        });
    }
}

/// Parses a runtime result into the operation's type and enforces the
/// artifact contract, filling in each artifact's (engine) fields.
pub(crate) fn accept_result(
    root: &Path,
    task: &Task,
    value: serde_json::Value,
    source_media: Option<&MediaInfo>,
    budget_bytes: u64,
) -> Result<OperationResult, ErrorBody> {
    let operation = task.operation();
    let mut result = OperationResult::from_json(operation, value).map_err(|detail| {
        ErrorBody::new(
            "runtime_protocol",
            format!("The runtime's {operation} result does not match the contract: {detail}."),
            Some(json!({"detail": detail, "stderr_tail": null})),
        )
    })?;
    artifacts::validate(
        &artifacts::Expectations {
            root,
            task,
            source: source_media,
            budget_bytes,
        },
        &mut result,
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests;
