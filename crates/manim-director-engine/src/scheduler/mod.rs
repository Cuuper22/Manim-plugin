//! The job queue: submit (resolve once, cache, coalesce), run on a bounded
//! worker pool, cancel, wait and shut down. Every database and filesystem
//! step runs on the blocking pool.

mod active;
mod artifacts;
mod direct;
mod execute;
mod latest;
mod maintenance;
mod prune;
mod recorder;
mod request;
mod runtime;

pub use artifacts::probe_media;
pub use direct::init_project;
pub use latest::{latest, latest_render, Latest};
pub use prune::{prune, PrunePolicy, Pruned};
pub use request::{cli_project_path, parse_request, Frontend};

use crate::{
    cache, now_millis, state_db_path, BridgeConfig, EngineMode, NewJob, PrewarmPolicy,
    RuntimeBridge, RuntimeIdentity, Store,
};
use active::Active;
use execute::accept_result;
use manim_director_core::{
    CancelledBy, DirectorSpec, DiscoverResult, EngineError, EngineEvent, ErrorBody, JobOrigin,
    JobRecord, MediaInfo, MediaSource, Operation, OperationRequest, OperationResult,
};
use parking_lot::Mutex;
use request::{ProjectContext, ResolvedJob};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::{broadcast, mpsc, watch, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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
    /// Keep one preloaded worker idle; `None` spawns a worker per job.
    pub prewarm: Option<PrewarmPolicy>,
}

impl SchedulerConfig {
    /// Long-lived engines pre-warm unless `MANIM_DIRECTOR_PREWARM=0`.
    pub fn new(mode: EngineMode) -> Self {
        let prewarm = mode.is_long_lived()
            && std::env::var("MANIM_DIRECTOR_PREWARM").map_or(true, |value| value.trim() != "0");
        Self {
            mode,
            workers: env_usize("MANIM_DIRECTOR_WORKERS", 2).clamp(1, 32),
            queue_capacity: env_usize("MANIM_DIRECTOR_QUEUE", 128).clamp(1, 4096),
            bridge: BridgeConfig::default(),
            prune: PrunePolicy::from_env(),
            prewarm: prewarm.then(PrewarmPolicy::default),
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
    /// The identity of the newest `ready` frame this engine saw.
    runtime: watch::Sender<Option<RuntimeIdentity>>,
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
    /// The submit-time spec of a cacheable job, for its start fingerprint.
    spec: Option<Arc<DirectorSpec>>,
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
    /// whose lease went stale, starts pre-warming, then accepts work.
    pub async fn start(
        root: impl AsRef<Path>,
        store: Arc<Store>,
        config: SchedulerConfig,
    ) -> anyhow::Result<Self> {
        let instance_id = Uuid::new_v4();
        let mode = config.mode;
        let root = root.as_ref().to_path_buf();
        let python = config.bridge.python.clone();
        let (root, known, spawn_key) = store
            .blocking(move |store| {
                let root = root.canonicalize()?;
                store.renew_lease(instance_id, mode, now_millis())?;
                let reaped = store.reap(instance_id, now_millis())?;
                maintenance::remove_out_dirs(&root, &reaped);
                let known = store.runtime(&python)?.map(|stored| stored.identity);
                let spec = DirectorSpec::load(&root).ok();
                let key = runtime::spawn_key(&root, &request::limits(spec.as_ref()));
                Ok((root, known, key))
            })
            .await?;
        let (runtime, _) = watch::channel(known);
        let sink =
            runtime::ready_sink(store.clone(), config.bridge.python.clone(), runtime.clone());
        let mut bridge = RuntimeBridge::with_ready_sink(config.bridge, sink);
        if let Some(policy) = config.prewarm {
            bridge.start_prewarm(&root, spawn_key, policy);
        }
        let (queue, receiver) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(1024);
        let inner = Arc::new(Inner {
            root,
            instance_id,
            mode,
            store,
            bridge,
            runtime,
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

    pub fn bridge_config(&self) -> &BridgeConfig {
        self.inner.bridge.config()
    }

    /// The identity and catalog of the newest `ready` frame this engine saw
    /// (seeded from the store), updated as workers start.
    pub fn runtime(&self) -> watch::Receiver<Option<RuntimeIdentity>> {
        self.inner.runtime.subscribe()
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
        let _ = self
            .inner
            .events
            .send(EngineEvent::Job(Arc::new(job.clone())));
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
        self.inner.bridge.close().await;
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
        direct::discover(&self.inner).await
    }

    #[cfg(test)]
    fn active_jobs(&self) -> usize {
        self.inner.active.lock().len()
    }
}

/// Hands queued jobs to at most `workers` concurrent runs, in order. A
/// worker is taken only for a job in hand, so a job that gave its worker
/// back while it waits for a scene lock can always get one again.
fn dispatch(inner: &Arc<Inner>, mut receiver: mpsc::UnboundedReceiver<Queued>, workers: usize) {
    let workers = Arc::new(Semaphore::new(workers));
    let dispatcher: Weak<Inner> = Arc::downgrade(inner);
    tokio::spawn(async move {
        loop {
            let Some(queued) = receiver.recv().await else {
                break;
            };
            let Ok(permit) = workers.clone().acquire_owned().await else {
                break;
            };
            let Some(inner) = dispatcher.upgrade() else {
                break;
            };
            let mut slot = Slot {
                workers: workers.clone(),
                permit: Some(permit),
            };
            tokio::spawn(async move { inner.run(queued, &mut slot).await });
        }
    });
}

/// A running job's claim on a worker, given back while the job only waits
/// for a scene lock so unrelated jobs can run meanwhile.
struct Slot {
    workers: Arc<Semaphore>,
    permit: Option<OwnedSemaphorePermit>,
}

impl Slot {
    async fn take(&mut self) {
        if self.permit.is_none() {
            self.permit = self.workers.clone().acquire_owned().await.ok();
        }
    }

    fn give_back(&mut self) {
        self.permit = None;
    }
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
    spec: Option<Arc<DirectorSpec>>,
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

    /// Resolves a request and answers it from the cache or an in-flight job
    /// when its submit fingerprint allows; that needs a known runtime
    /// identity and a request that is not `fresh`.
    fn plan(&self, origin: JobOrigin, request: OperationRequest) -> Result<Plan, EngineError> {
        let spec = self.load_spec();
        let id = Uuid::new_v4();
        let context = ProjectContext {
            root: &self.root,
            spec: &spec,
            store: &self.store,
        };
        let resolved = request::resolve(&context, id, &request)?;
        crate::request_line(&id.to_string(), &self.root, &resolved.task)?;
        let cacheable = request.operation().cacheable();
        let fresh = matches!(
            &request,
            OperationRequest::Render(params) if params.fresh
        ) || matches!(&request, OperationRequest::Still(params) if params.fresh);
        let runtime = match cacheable && !fresh {
            true => self.known_runtime()?,
            false => None,
        };
        let fingerprint = match (&spec, runtime) {
            (Ok(spec), Some(runtime)) => Some(
                cache::fingerprint(&self.root, spec, &runtime, &resolved.task)
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
            spec: spec.ok().filter(|_| cacheable).map(Arc::new),
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
            spec,
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
            spec,
        };
        Ok((job, context))
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

    /// Publishes a job's terminal state at most once per job.
    fn publish(&self, active: &Active, job: &JobRecord) {
        if active.claim_publication() {
            self.emit_finished(job);
        }
    }

    fn emit_finished(&self, job: &JobRecord) {
        let _ = self.events.send(EngineEvent::Job(Arc::new(job.clone())));
    }
}

#[cfg(test)]
mod tests;
