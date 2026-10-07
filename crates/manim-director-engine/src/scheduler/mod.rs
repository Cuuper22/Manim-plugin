//! The job queue: submit (resolve once, cache, coalesce), run on a bounded
//! worker pool, cancel and wait.

mod artifacts;
mod direct;
mod latest;
mod request;

pub use artifacts::{confine, probe_media, Confinement};
pub use direct::init_project;
pub use latest::{latest, latest_render, Latest};
use request::ProjectContext;
pub use request::{cli_project_path, parse_params, parse_request, Frontend};

use crate::{
    cache, BridgeConfig, BridgeEvent, BridgeOutcome, Invocation, NewJob, RuntimeBridge, Store,
};
use manim_director_core::{
    python_sources, CancelledBy, DirectorSpec, DiscoverResult, DiscoverTask, EngineError,
    EngineEvent, ErrorBody, Finding, JobOrigin, JobRecord, JobStatus, JobSummary, LogLevel,
    LogStream, MediaInfo, MediaSource, Operation, OperationRequest, OperationResult, Progress,
    ProgressPhase, RuntimeLogLevel, Task, Timestamp,
};
use parking_lot::Mutex;
use request::ResolvedJob;
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, mpsc, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const DISCOVER_TIMEOUT: Duration = Duration::from_secs(30);
const PROGRESS_PERSIST_INTERVAL: Duration = Duration::from_millis(500);
const MAX_LOG_MESSAGE_CHARS: usize = 2000;

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Jobs run concurrently by this process.
    pub workers: usize,
    /// Queued jobs this process accepts before `queue_full`.
    pub queue_capacity: usize,
    pub bridge: BridgeConfig,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            workers: env_usize("MANIM_DIRECTOR_WORKERS", 2).clamp(1, 32),
            queue_capacity: env_usize("MANIM_DIRECTOR_QUEUE", 128).clamp(1, 4096),
            bridge: BridgeConfig::default(),
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
    store: Arc<Store>,
    bridge: RuntimeBridge,
    events: broadcast::Sender<EngineEvent>,
    queue: mpsc::Sender<Queued>,
    queue_capacity: usize,
    tokens: Mutex<HashMap<Uuid, CancellationToken>>,
    last_valid_spec: Mutex<Option<DirectorSpec>>,
}

struct Queued {
    job: JobRecord,
    context: RunContext,
}

/// Per-job facts resolved at submit that the record does not carry.
struct RunContext {
    source_media: Option<MediaInfo>,
    source: Option<MediaSource>,
    artifact_budget: u64,
}

impl Scheduler {
    pub fn start(
        root: impl AsRef<Path>,
        store: Arc<Store>,
        config: SchedulerConfig,
    ) -> std::io::Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let (queue, mut receiver) = mpsc::channel::<Queued>(config.queue_capacity);
        let (events, _) = broadcast::channel(1024);
        let inner = Arc::new(Inner {
            root,
            instance_id: Uuid::new_v4(),
            store,
            bridge: RuntimeBridge::new(config.bridge),
            events,
            queue,
            queue_capacity: config.queue_capacity,
            tokens: Mutex::new(HashMap::new()),
            last_valid_spec: Mutex::new(None),
        });
        let workers = Arc::new(Semaphore::new(config.workers));
        let dispatcher: Weak<Inner> = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while let Some(queued) = receiver.recv().await {
                let Ok(permit) = workers.clone().acquire_owned().await else {
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
        Ok(Self { inner })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.inner.store
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
        let permit =
            self.inner
                .queue
                .clone()
                .try_reserve_owned()
                .map_err(|_| EngineError::QueueFull {
                    capacity: self.inner.queue_capacity,
                })?;
        let inner = self.inner.clone();
        let (job, context) = tokio::task::spawn_blocking(move || inner.insert(pending))
            .await
            .map_err(EngineError::internal)??;
        self.inner
            .tokens
            .lock()
            .insert(job.id, CancellationToken::new());
        permit.send(Queued {
            job: job.clone(),
            context,
        });
        let _ = self.inner.events.send(EngineEvent::JobQueued {
            job: JobSummary::from(&job),
        });
        Ok(Submission::Queued(job))
    }

    /// Requests cancellation; terminal jobs are left as they are. A queued job
    /// ends at once and its worker later skips it without spawning.
    pub fn cancel(&self, id: Uuid) -> Result<JobRecord, EngineError> {
        let store = &self.inner.store;
        let token = self.inner.tokens.lock().get(&id).cloned();
        if let Some(token) = token {
            token.cancel();
            let error = ErrorBody::cancelled(CancelledBy::Client);
            if store
                .cancel_queued(id, &error)
                .map_err(EngineError::internal)?
            {
                if let Ok(Some(job)) = store.get_job(id) {
                    self.inner.emit_finished(&job);
                }
            }
        }
        store.request_cancel(id).map_err(EngineError::internal)?;
        store
            .get_job(id)
            .map_err(EngineError::internal)?
            .ok_or_else(|| EngineError::job_not_found(id))
    }

    /// Resolves when the job reaches a terminal status.
    pub async fn wait(&self, id: Uuid) -> Result<JobRecord, EngineError> {
        let mut events = self.subscribe();
        loop {
            let job = self
                .inner
                .store
                .get_job(id)
                .map_err(EngineError::internal)?
                .ok_or_else(|| EngineError::job_not_found(id))?;
            if job.status.is_terminal() {
                return Ok(job);
            }
            let _ = tokio::time::timeout(Duration::from_millis(250), events.recv()).await;
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
                let store = self.inner.store.clone();
                let cached = OperationResult::Discover(result.clone());
                let fingerprint = prepared.fingerprint.clone();
                tokio::task::spawn_blocking(move || {
                    store.cache_put(&fingerprint, None, &cached, Operation::Discover)
                })
                .await
                .map_err(EngineError::internal)?
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
                if self.artifacts_intact(&entry.result) {
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

    /// A cache entry is usable only while every artifact is on disk unchanged in size.
    fn artifacts_intact(&self, result: &OperationResult) -> bool {
        result.artifacts().iter().all(|artifact| {
            artifacts::existing(&self.root, artifact)
                .and_then(|path| path.metadata().ok())
                .is_some_and(|metadata| metadata.len() == artifact.bytes)
        })
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

    async fn run(self: Arc<Self>, queued: Queued) {
        let Queued { job, context } = queued;
        let id = job.id;
        let token = self.tokens.lock().get(&id).cloned().unwrap_or_default();
        let outcome = self.execute(&job, &context, &token).await;
        if let Some(outcome) = outcome {
            self.finish(&job, outcome).await;
        }
        self.tokens.lock().remove(&id);
    }

    /// Runs one queued job; `None` when it was no longer queued.
    async fn execute(
        &self,
        job: &JobRecord,
        context: &RunContext,
        token: &CancellationToken,
    ) -> Option<Outcome> {
        let id = job.id;
        if !self.store.set_running(id).unwrap_or(false) {
            return None;
        }
        if token.is_cancelled() {
            return Some(Outcome::Cancelled);
        }
        if let Ok(Some(started)) = self.store.get_job(id) {
            let _ = self.events.send(EngineEvent::JobStarted {
                job: JobSummary::from(&started),
            });
        }
        let mut recorder = Recorder::new(self.store.clone(), self.events.clone(), id);
        recorder.engine_phase(
            ProgressPhase::Starting,
            None,
            Some("waiting for the runtime"),
        );
        if let Some(out_dir) = job.task.out_dir() {
            if let Err(error) = artifacts::create_out_dir(&self.root, out_dir) {
                return Some(Outcome::Failed(ErrorBody::internal(format!(
                    "Could not create the job's artifact directory: {error}"
                ))));
            }
        }
        let request_id = id.to_string();
        let attempt = token.child_token();
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
        Some(match outcome {
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
            BridgeOutcome::Cancelled => Outcome::Cancelled,
        })
    }

    async fn finish(&self, job: &JobRecord, outcome: Outcome) {
        let id = job.id;
        let recorded = match &outcome {
            Outcome::Succeeded(result) => {
                let revision = result.scene().and_then(|scene| {
                    (job.scene_file.as_deref() != Some(scene.file.as_str()))
                        .then(|| cache::file_revision(&self.root.join(&scene.file)).ok())
                        .flatten()
                });
                let stored = self.store.finish_success(id, result, revision.as_deref());
                if let (Ok(true), Some(fingerprint)) = (&stored, &job.fingerprint) {
                    if let Err(error) =
                        self.store
                            .cache_put(fingerprint, Some(id), result, job.operation)
                    {
                        tracing::warn!(%id, %error, "could not cache the result");
                    }
                }
                stored
            }
            Outcome::Failed(error) => self.store.finish_error(id, JobStatus::Failed, error),
            Outcome::Cancelled => self.store.finish_error(
                id,
                JobStatus::Cancelled,
                &ErrorBody::cancelled(CancelledBy::Client),
            ),
        };
        let ended = match recorded {
            Ok(ended) => ended,
            Err(error) => {
                tracing::error!(%id, %error, "could not record the job outcome");
                false
            }
        };
        let kept = ended && matches!(outcome, Outcome::Succeeded(_));
        if !kept {
            if let Some(out_dir) = job.task.out_dir() {
                if let Err(error) = artifacts::remove_out_dir(&self.root, out_dir) {
                    tracing::warn!(%id, %error, "could not remove the job's artifact directory");
                }
            }
        }
        // Exactly one terminal transition per job, so exactly one event.
        if ended {
            if let Ok(Some(finished)) = self.store.get_job(id) {
                self.emit_finished(&finished);
            }
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

enum Outcome {
    Succeeded(Box<OperationResult>),
    Failed(ErrorBody),
    Cancelled,
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

/// Turns bridge events into progress events and log records.
struct Recorder {
    store: Arc<Store>,
    events: broadcast::Sender<EngineEvent>,
    id: Uuid,
    persisted_at: Option<Instant>,
}

impl Recorder {
    fn new(store: Arc<Store>, events: broadcast::Sender<EngineEvent>, id: Uuid) -> Self {
        Self {
            store,
            events,
            id,
            persisted_at: None,
        }
    }

    fn record(&mut self, event: BridgeEvent<'_>) {
        match event {
            BridgeEvent::Ready(ready) => {
                let message = format!(
                    "Runtime {} ready (Python {}, Manim {}).",
                    ready.runtime_version,
                    ready.python,
                    ready.manim.as_deref().unwrap_or("not installed")
                );
                self.log(LogStream::Engine, LogLevel::Info, &message);
                for failure in &ready.preload_failed {
                    let message =
                        format!("Preloading {} failed: {}", failure.module, failure.message);
                    self.log(LogStream::Engine, LogLevel::Warning, &message);
                }
            }
            BridgeEvent::Progress(frame) => self.progress(Progress {
                phase: frame.phase,
                current: frame.current,
                total: frame.total,
                scene_seconds: frame.scene_seconds,
                message: frame.message,
                updated_at: Timestamp::now(),
            }),
            BridgeEvent::Log(frame) => {
                let level = match frame.level {
                    RuntimeLogLevel::Info => LogLevel::Info,
                    RuntimeLogLevel::Warning => LogLevel::Warning,
                };
                self.log(LogStream::Runtime, level, &frame.message);
            }
            BridgeEvent::Stderr(line) => self.log(LogStream::Stderr, LogLevel::Info, &line),
        }
    }

    /// Progress the engine itself reports (`starting`, `validate`).
    fn engine_phase(&mut self, phase: ProgressPhase, total: Option<u64>, message: Option<&str>) {
        self.progress(Progress {
            phase,
            current: 0,
            total,
            scene_seconds: None,
            message: message.map(str::to_owned),
            updated_at: Timestamp::now(),
        });
    }

    fn progress(&mut self, progress: Progress) {
        let due = self
            .persisted_at
            .is_none_or(|at| at.elapsed() >= PROGRESS_PERSIST_INTERVAL);
        if due {
            self.persisted_at = Some(Instant::now());
            if let Err(error) = self.store.set_progress(self.id, &progress) {
                tracing::warn!(id = %self.id, %error, "could not persist progress");
            }
        }
        let _ = self.events.send(EngineEvent::JobProgress {
            job_id: self.id,
            progress,
        });
    }

    /// One record per ≤ 2000-character piece of `message`.
    fn log(&self, stream: LogStream, level: LogLevel, message: &str) {
        let chars: Vec<char> = message.chars().collect();
        for piece in chars.chunks(MAX_LOG_MESSAGE_CHARS) {
            let piece: String = piece.iter().collect();
            if let Err(error) = self.store.append_log(self.id, stream, level, &piece, None) {
                tracing::warn!(id = %self.id, %error, "could not store a log record");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
