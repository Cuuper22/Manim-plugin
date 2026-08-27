use crate::{project_fingerprint, BridgeConfig, BridgeOutcome, RuntimeBridge, Store};
use anyhow::{anyhow, Result};
use manim_director_core::{
    DirectorSpec, EngineEvent, JobRecord, JobRequest, JobStatus, JobSummary, Operation,
    ProtocolError,
};
use parking_lot::Mutex;
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{broadcast, mpsc, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub concurrency: usize,
    pub queue_capacity: usize,
    pub job_timeout: Duration,
    pub bridge: BridgeConfig,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            concurrency: env_usize("MANIM_DIRECTOR_WORKERS", 2).clamp(1, 32),
            queue_capacity: env_usize("MANIM_DIRECTOR_QUEUE", 128).clamp(1, 4096),
            job_timeout: Duration::from_secs(
                env_usize("MANIM_DIRECTOR_TIMEOUT_SECONDS", 3600).clamp(10, 86400) as u64,
            ),
            bridge: BridgeConfig::default(),
        }
    }
}

#[derive(Clone)]
pub struct Scheduler {
    store: Arc<Store>,
    tx: mpsc::Sender<QueuedJob>,
    events: broadcast::Sender<EngineEvent>,
    cancellations: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
}

struct QueuedJob {
    record: JobRecord,
}

impl Scheduler {
    pub fn start(store: Arc<Store>, config: SchedulerConfig) -> Self {
        let (tx, mut rx) = mpsc::channel::<QueuedJob>(config.queue_capacity);
        let (events, _) = broadcast::channel(1024);
        let cancellations = Arc::new(Mutex::new(HashMap::new()));
        let scheduler = Self {
            store: store.clone(),
            tx,
            events: events.clone(),
            cancellations: cancellations.clone(),
        };
        let bridge = RuntimeBridge::new(config.bridge);
        let semaphore = Arc::new(Semaphore::new(config.concurrency));

        tokio::spawn(async move {
            while let Some(queued) = rx.recv().await {
                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                let store = store.clone();
                let events = events.clone();
                let cancellations = cancellations.clone();
                let bridge = bridge.clone();
                let timeout = config.job_timeout;
                tokio::spawn(async move {
                    let _permit = permit;
                    run_job(store, bridge, events, cancellations, queued.record, timeout).await;
                });
            }
        });
        scheduler
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }
    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub async fn submit(
        &self,
        project_root: impl AsRef<Path>,
        mut request: JobRequest,
    ) -> Result<JobRecord> {
        let root = project_root.as_ref().canonicalize()?;
        if request.operation == Operation::Debug {
            hydrate_debug_request(&self.store, &mut request)?;
        }
        if request.operation == Operation::Export {
            hydrate_export_request(&root, &self.store, &mut request)?;
        }
        merge_project_defaults(&root, &mut request);
        if request.operation == Operation::Qa {
            hydrate_qa_request(&root, &self.store, &mut request)?;
        }
        let disable_director_cache = request
            .params
            .get("disable_caching")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let flush_director_cache = request
            .params
            .get("flush_cache")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if flush_director_cache {
            self.store.cache_clear()?;
        }
        let fingerprint =
            (request.operation.cacheable() && !disable_director_cache && !flush_director_cache)
                .then(|| project_fingerprint(&root, &request))
                .transpose()?;
        let id = Uuid::new_v4();
        if let Some(fingerprint) = fingerprint.as_deref() {
            if let Some(result) = self.store.cache_get(fingerprint)? {
                if validate_runtime_result(&root, request.operation, &request.params, &result)
                    .await
                    .is_ok()
                {
                    let record = self.store.create_cached_job(
                        id,
                        &root,
                        request.operation,
                        &request.params,
                        fingerprint,
                        &result,
                    )?;
                    let _ = self.events.send(EngineEvent::JobFinished {
                        job: JobSummary::from(&record),
                        result: Some(result),
                        error: None,
                    });
                    return Ok(record);
                }
                self.store.cache_delete(fingerprint)?;
            }
        }

        let record = self.store.create_job(
            id,
            &root,
            request.operation,
            &request.params,
            fingerprint.as_deref(),
        )?;
        self.cancellations
            .lock()
            .insert(id, CancellationToken::new());
        self.tx
            .try_send(QueuedJob {
                record: record.clone(),
            })
            .map_err(|error| {
                let protocol = ProtocolError::new(
                    "queue_full",
                    "render queue is full; retry after an active job finishes",
                );
                let _ = self.store.finish_error(id, JobStatus::Failed, &protocol);
                anyhow!(error)
            })?;
        let _ = self.events.send(EngineEvent::JobQueued {
            job: JobSummary::from(&record),
        });
        Ok(record)
    }

    pub fn cancel(&self, id: Uuid) -> Result<JobRecord> {
        let token = self.cancellations.lock().get(&id).cloned();
        if let Some(token) = token {
            token.cancel();
        } else if self.store.get_job(id)?.is_none() {
            return Err(anyhow!("job {id} not found"));
        }
        self.store
            .get_job(id)?
            .ok_or_else(|| anyhow!("job {id} not found"))
    }

    pub async fn wait(&self, id: Uuid, poll: Duration) -> Result<JobRecord> {
        loop {
            let job = self
                .store
                .get_job(id)?
                .ok_or_else(|| anyhow!("job {id} not found"))?;
            if matches!(
                job.status,
                JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
            ) {
                return Ok(job);
            }
            tokio::time::sleep(poll).await;
        }
    }
}

fn hydrate_debug_request(store: &Store, request: &mut JobRequest) -> Result<()> {
    let Some(params) = request.params.as_object_mut() else {
        return Ok(());
    };
    let Some(id) = params.get("job_id").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    let id = Uuid::parse_str(id)?;
    let job = store
        .get_job(id)?
        .ok_or_else(|| anyhow!("debug target job {id} not found"))?;
    let logs = store.logs(Some(id), None, 100)?;
    let mut diagnostic = String::new();
    if let Some(error) = &job.error {
        diagnostic.push_str(&format!("{}: {}\n", error.code, error.message));
    }
    for log in logs.items {
        use std::fmt::Write;
        let _ = writeln!(diagnostic, "[{}] {} {}", log.level, log.event, log.data);
        if diagnostic.len() > 64 * 1024 {
            diagnostic.truncate(64 * 1024);
            break;
        }
    }
    params.entry("log").or_insert_with(|| json!(diagnostic));
    params
        .entry("text")
        .or_insert_with(|| json!(format!("Diagnose failed {} job {id}", job.operation)));
    Ok(())
}

fn hydrate_qa_request(root: &Path, store: &Store, request: &mut JobRequest) -> Result<()> {
    let Some(params) = request.params.as_object_mut() else {
        return Ok(());
    };
    if params.contains_key("source") || params.contains_key("images") {
        return Ok(());
    }
    let explicit_id = params
        .get("job_id")
        .and_then(serde_json::Value::as_str)
        .map(Uuid::parse_str)
        .transpose()?;
    let scene = params
        .get("scene")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let profile = params
        .get("profile")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let source_job = if let Some(id) = explicit_id {
        let job = store
            .get_job(id)?
            .ok_or_else(|| anyhow!("QA source job {id} not found"))?;
        if job.status != JobStatus::Succeeded
            || !matches!(
                job.operation,
                Operation::Render | Operation::Preview | Operation::Still
            )
        {
            return Err(anyhow!(
                "QA source job {id} must be a successful render, preview, or still"
            ));
        }
        job
    } else {
        store
            .jobs(None, 200)?
            .items
            .into_iter()
            .find(|job| {
                job.status == JobStatus::Succeeded
                    && matches!(
                        job.operation,
                        Operation::Render | Operation::Preview | Operation::Still
                    )
                    && scene.as_ref().is_none_or(|value| {
                        job.params.get("scene").and_then(serde_json::Value::as_str)
                            == Some(value.as_str())
                    })
                    && profile.as_ref().is_none_or(|value| {
                        job.params
                            .get("profile")
                            .and_then(serde_json::Value::as_str)
                            == Some(value.as_str())
                    })
            })
            .ok_or_else(|| {
                anyhow!(
                    "no successful render artifact matches the requested scene/profile; render first or pass --artifact"
                )
            })?
    };
    let artifacts = final_artifacts(
        source_job.operation,
        source_job
            .result
            .as_ref()
            .unwrap_or(&serde_json::Value::Null),
    );
    let mut videos = Vec::new();
    let mut images = Vec::new();
    for artifact in artifacts {
        let raw = Path::new(&artifact.path);
        let path = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            root.join(raw)
        };
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !canonical.is_file() || !canonical.starts_with(root) {
            continue;
        }
        let value = canonical.to_string_lossy().into_owned();
        match canonical
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("mp4" | "mov" | "webm" | "gif") => videos.push(value),
            Some("png" | "jpg" | "jpeg" | "webp") => images.push(value),
            _ => {}
        }
    }
    if let Some(source) = videos.into_iter().next() {
        params.insert("source".into(), json!(source));
    } else if !images.is_empty() {
        params.insert("images".into(), json!(images));
    } else {
        return Err(anyhow!(
            "successful source job {} has no readable video or image artifact",
            source_job.id
        ));
    }
    params.insert("source_job_id".into(), json!(source_job.id));
    Ok(())
}

fn hydrate_export_request(root: &Path, store: &Store, request: &mut JobRequest) -> Result<()> {
    let Some(params) = request.params.as_object_mut() else {
        return Ok(());
    };
    let format = params
        .get("format")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("bundle")
        .to_owned();
    if !matches!(
        format.as_str(),
        "bundle" | "zip" | "mp4" | "webm" | "gif" | "captions"
    ) {
        return Err(anyhow!(
            "unsupported export format {format}; use bundle, zip, mp4, webm, gif, or captions"
        ));
    }
    params
        .entry("format")
        .or_insert_with(|| json!(format.clone()));
    let Some(id) = params.get("job_id").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    let id = Uuid::parse_str(id)?;
    let job = store
        .get_job(id)?
        .ok_or_else(|| anyhow!("export source job {id} not found"))?;
    if job.status != JobStatus::Succeeded {
        return Err(anyhow!("export source job {id} has not succeeded"));
    }
    let mut includes = Vec::new();
    collect_artifact_paths(
        root,
        job.result.as_ref().unwrap_or(&serde_json::Value::Null),
        &mut includes,
    );
    includes.sort();
    includes.dedup();
    includes.truncate(200);
    if includes.is_empty() {
        return Err(anyhow!(
            "job {id} has no project-contained artifacts to export"
        ));
    }
    if matches!(format.as_str(), "mp4" | "webm" | "gif") {
        if let Some(source) = includes.iter().find(|path| {
            matches!(
                Path::new(path).extension().and_then(|value| value.to_str()),
                Some("mp4" | "mov" | "webm" | "gif")
            )
        }) {
            params.entry("source").or_insert_with(|| json!(source));
        } else {
            return Err(anyhow!(
                "job {id} has no media artifact to export as {format}"
            ));
        }
        params
            .entry("artifacts")
            .or_insert_with(|| json!(includes.clone()));
        for key in ["width", "height", "fps", "transparent", "profile"] {
            if !params.contains_key(key) {
                if let Some(value) = job.params.get(key) {
                    params.insert(key.into(), value.clone());
                } else if let Some(value) = job
                    .result
                    .as_ref()
                    .and_then(|result| result.get("profile"))
                    .and_then(|profile| profile.get(key))
                {
                    params.insert(key.into(), value.clone());
                }
            }
        }
    } else {
        // The Python exporter treats these as explicitly selected
        // deliverables. Keeping them separate from source include globs lets
        // it safely remap generated media out of the excluded state tree.
        params.insert("artifacts".into(), json!(includes.clone()));
        let caption_only = format == "captions";
        let mut patterns = if caption_only {
            vec![
                "*.vtt".into(),
                "*.srt".into(),
                "**/*.vtt".into(),
                "**/*.srt".into(),
                "*transcript*.txt".into(),
                "*narration*.json".into(),
                "**/*transcript*.txt".into(),
                "**/*narration*.json".into(),
            ]
        } else {
            bundle_include_patterns(root)
        };
        patterns.extend(json_strings(params.get("include")));
        patterns.extend(includes.clone());
        patterns.sort();
        patterns.dedup();
        params.insert("include".into(), json!(patterns));

        let mut excludes = json_strings(params.get("exclude"));
        if excludes.is_empty() {
            excludes = vec![
                "**/__pycache__/**".into(),
                "**/*.pyc".into(),
                ".git/**".into(),
                "**/.DS_Store".into(),
            ];
        }
        excludes.retain(|pattern| pattern != ".manim-director/**");
        excludes.extend([
            ".manim-director/state.db*".into(),
            ".manim-director/tmp/**".into(),
            ".manim-director/undo/**".into(),
        ]);
        excludes.sort();
        excludes.dedup();
        params.insert("exclude".into(), json!(excludes));
    }
    Ok(())
}

fn json_strings(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::String(value)) => vec![value.clone()],
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn bundle_include_patterns(root: &Path) -> Vec<String> {
    let mut patterns = vec![
        "director.yaml".into(),
        "manim.cfg".into(),
        "requirements.txt".into(),
        "README.md".into(),
        "scenes/**".into(),
        "assets/**".into(),
        "sources/**".into(),
        "output/**".into(),
    ];
    if let Ok(spec) = DirectorSpec::load(root) {
        if let Some(engine) = spec.engine {
            patterns.push(engine.source);
        }
        for value in [
            spec.project.source_dir,
            spec.project.asset_dir,
            spec.project.output_dir,
        ] {
            let path = root.join(&value);
            patterns.push(if path.is_dir() {
                format!("{}/**", value.trim_end_matches('/'))
            } else {
                value
            });
        }
    }
    patterns
}

fn collect_artifact_paths(root: &Path, value: &serde_json::Value, paths: &mut Vec<String>) {
    match value {
        serde_json::Value::String(candidate) => {
            let raw = Path::new(candidate);
            let joined = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                root.join(raw)
            };
            if let Ok(canonical) = joined.canonicalize() {
                if canonical.is_file() && canonical.starts_with(root) {
                    if let Ok(relative) = canonical.strip_prefix(root) {
                        paths.push(relative.to_string_lossy().replace('\\', "/"));
                    }
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                collect_artifact_paths(root, value, paths);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values() {
                collect_artifact_paths(root, value, paths);
            }
        }
        _ => {}
    }
}

fn merge_project_defaults(root: &Path, request: &mut JobRequest) {
    if !matches!(
        request.operation,
        Operation::Render
            | Operation::Preview
            | Operation::Still
            | Operation::ContactSheet
            | Operation::Qa
            | Operation::Debug
            | Operation::Export
            | Operation::Inspect
            | Operation::Discover
            | Operation::ValidateMath
            | Operation::Captions
            | Operation::Assets
            | Operation::Sample
    ) {
        return;
    }
    let Ok(spec) = DirectorSpec::load(root) else {
        return;
    };
    let params = match &mut request.params {
        serde_json::Value::Object(params) => params,
        _ => return,
    };
    let requested_scene = params
        .get("scene")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let mapped_scene = requested_scene.as_deref().and_then(|requested| {
        spec.scenes
            .iter()
            .find(|scene| scene.id == requested || scene.class_name.as_deref() == Some(requested))
    });
    if let Some(scene) = mapped_scene {
        if let Some(file) = scene.file.as_deref() {
            params.entry("scene_file").or_insert_with(|| json!(file));
        }
        if let Some(class_name) = scene.class_name.as_deref() {
            params.insert("scene".into(), json!(class_name));
        }
    }
    if let Some(engine) = &spec.engine {
        params
            .entry("scene_file")
            .or_insert_with(|| json!(engine.source));
        params
            .entry("scene")
            .or_insert_with(|| json!(engine.main_scene));
    }
    let profile_name = params
        .get("profile")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&spec.render.profile)
        .to_owned();
    params
        .entry("profile")
        .or_insert_with(|| json!(profile_name));

    let selected = spec.profiles.get(&profile_name);
    let resolution = selected.and_then(|profile| profile.resolution);
    let renderer = selected
        .and_then(|profile| profile.renderer.as_deref())
        .unwrap_or(&spec.render.renderer);
    let format = selected
        .and_then(|profile| profile.format.as_deref())
        .unwrap_or(&spec.render.format);
    if selected.is_some() || profile_name == "custom" {
        let width = resolution
            .map(|value| value[0])
            .unwrap_or(spec.render.width);
        let height = resolution
            .map(|value| value[1])
            .unwrap_or(spec.render.height);
        let fps = selected
            .and_then(|profile| profile.fps)
            .unwrap_or(spec.render.fps);
        params.entry("width").or_insert_with(|| json!(width));
        params.entry("height").or_insert_with(|| json!(height));
        params.entry("fps").or_insert_with(|| json!(fps));
    }
    params.entry("renderer").or_insert_with(|| json!(renderer));
    params.entry("format").or_insert_with(|| json!(format));
    params.entry("transparent").or_insert_with(|| {
        json!(selected
            .and_then(|profile| profile.alpha)
            .unwrap_or(spec.render.transparent))
    });
    params
        .entry("media_dir")
        .or_insert_with(|| json!(spec.project.media_dir));
    if let Some(quality) = selected.and_then(|profile| profile.quality.as_deref()) {
        params.entry("quality").or_insert_with(|| json!(quality));
    }
    if let Some(section) = params.remove("section") {
        params
            .entry("sections")
            .or_insert_with(|| serde_json::Value::Array(vec![section]));
    }
}

async fn run_job(
    store: Arc<Store>,
    bridge: RuntimeBridge,
    events: broadcast::Sender<EngineEvent>,
    cancellations: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
    record: JobRecord,
    timeout: Duration,
) {
    let id = record.id;
    let cancellation = cancellations.lock().get(&id).cloned().unwrap_or_default();
    if cancellation.is_cancelled() {
        finish_cancelled(&store, &events, id, "cancelled before execution");
        cancellations.lock().remove(&id);
        return;
    }
    if let Err(error) = store.set_running(id) {
        tracing::error!(%id, %error, "failed to mark job running");
        cancellations.lock().remove(&id);
        return;
    }
    if let Ok(Some(started)) = store.get_job(id) {
        let _ = events.send(EngineEvent::JobStarted {
            job: JobSummary::from(&started),
        });
    }

    let event_store = store.clone();
    let event_bus = events.clone();
    let operation = record.operation;
    let project_root = PathBuf::from(&record.project_root);
    let params = record.params.clone();
    let request_id = id.to_string();
    let execution = bridge.execute(
        &request_id,
        operation,
        &project_root,
        params,
        cancellation.clone(),
        move |event, data| {
            let level = if event == "runtime_stderr" {
                "warn"
            } else {
                "info"
            };
            let _ = event_store.append_log(id, level, event, &data);
            let _ = event_bus.send(EngineEvent::JobProgress {
                job_id: id,
                event: event.to_owned(),
                data,
            });
        },
    );

    tokio::pin!(execution);
    let outcome = tokio::select! {
        result = &mut execution => Some(result),
        _ = tokio::time::sleep(timeout) => {
            cancellation.cancel();
            let _ = tokio::time::timeout(Duration::from_secs(5), &mut execution).await;
            None
        }
    };
    match outcome {
        Some(Ok(BridgeOutcome::Result(result))) => {
            if let Err(error) =
                validate_runtime_result(&project_root, operation, &record.params, &result).await
            {
                let protocol = error.into_protocol_error();
                let _ = store.finish_error(id, JobStatus::Failed, &protocol);
            } else if let Err(error) = store.finish_success(id, &result) {
                tracing::error!(%id, %error, "failed to persist job result");
            } else if let Some(fingerprint) = record.fingerprint.as_deref() {
                let _ = store.cache_put(fingerprint, &project_root, operation, &result);
            }
        }
        Some(Ok(BridgeOutcome::Error(error))) => {
            let _ = store.finish_error(id, JobStatus::Failed, &error);
        }
        Some(Ok(BridgeOutcome::Cancelled)) => {
            finish_cancelled(&store, &events, id, "cancelled by user");
        }
        Some(Err(error)) => {
            let protocol = ProtocolError {
                code: "runtime_transport".into(),
                message: error.to_string(),
                data: None,
            };
            let _ = store.finish_error(id, JobStatus::Failed, &protocol);
        }
        None => {
            let protocol = ProtocolError {
                code: "job_timeout".into(),
                message: format!("job exceeded {} seconds", timeout.as_secs()),
                data: Some(json!({"timeout_seconds": timeout.as_secs()})),
            };
            let _ = store.finish_error(id, JobStatus::Failed, &protocol);
        }
    }
    cancellations.lock().remove(&id);
    if let Ok(Some(finished)) = store.get_job(id) {
        let _ = events.send(EngineEvent::JobFinished {
            job: JobSummary::from(&finished),
            result: finished.result.clone(),
            error: finished.error.clone(),
        });
    }
}

async fn validate_runtime_result(
    root: &Path,
    operation: Operation,
    params: &serde_json::Value,
    result: &serde_json::Value,
) -> std::result::Result<(), ArtifactContractError> {
    if !matches!(
        operation,
        Operation::Render
            | Operation::Preview
            | Operation::Still
            | Operation::ContactSheet
            | Operation::Export
            | Operation::Captions
    ) {
        return Ok(());
    }
    let artifacts = final_artifacts(operation, result);
    if artifacts.is_empty() {
        return Err(ArtifactContractError::simple(
            "artifact_missing",
            "runtime reported success without a final deliverable",
            json!({"operation": operation}),
        ));
    }
    let expectation = expected_artifact_contract(operation, params, result);
    let max_bytes = DirectorSpec::load(root)
        .ok()
        .and_then(|spec| {
            spec.budgets
                .map(|budget| budget.output_mb.saturating_mul(1024 * 1024))
        })
        .unwrap_or(2 * 1024 * 1024 * 1024);
    let mut total = 0_u64;
    for artifact in artifacts {
        let raw = Path::new(&artifact.path);
        let candidate = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            root.join(raw)
        };
        let path = candidate.canonicalize().map_err(|error| {
            ArtifactContractError::simple(
                "artifact_missing",
                format!("final artifact {} is missing", artifact.path),
                json!({"path":artifact.path,"error":error.to_string()}),
            )
        })?;
        if !path.starts_with(root) {
            return Err(ArtifactContractError::simple(
                "artifact_outside_project",
                "runtime returned a final artifact outside the project",
                json!({"path":artifact.path}),
            ));
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|error| ArtifactContractError::io(&relative, "metadata", error))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(ArtifactContractError::simple(
                "artifact_empty",
                format!("final artifact {relative} is missing or empty"),
                json!({"path":relative,"bytes":metadata.len()}),
            ));
        }
        total = total.saturating_add(metadata.len());
        if total > max_bytes {
            return Err(ArtifactContractError::simple(
                "artifact_budget_exceeded",
                "final artifacts exceed the configured output budget",
                json!({"bytes":total,"max_bytes":max_bytes}),
            ));
        }
        let mut file = tokio::fs::File::open(&path)
            .await
            .map_err(|error| ArtifactContractError::io(&relative, "open", error))?;
        use tokio::io::AsyncReadExt;
        let mut signature = [0_u8; 12];
        let count = file
            .read(&mut signature)
            .await
            .map_err(|error| ArtifactContractError::io(&relative, "read", error))?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match extension.as_str() {
            "zip" if !signature[..count].starts_with(b"PK") => {
                return Err(ArtifactContractError::signature(&relative, "zip"))
            }
            "png" if !signature[..count].starts_with(b"\x89PNG\r\n\x1a\n") => {
                return Err(ArtifactContractError::signature(&relative, "png"))
            }
            "jpg" | "jpeg" if !signature[..count].starts_with(b"\xff\xd8\xff") => {
                return Err(ArtifactContractError::signature(&relative, "jpeg"))
            }
            _ => {}
        }
        let expected = if artifact.enforce_profile {
            expectation.clone()
        } else {
            ArtifactExpectation {
                container: Some(extension.clone()),
                ..Default::default()
            }
        };
        if is_visual_extension(&extension) {
            let actual = probe_artifact(&path, &relative).await?;
            compare_artifact_contract(&relative, &expected, &actual)?;
        } else if let Some(container) = expected.container.clone() {
            if !container_matches(&container, &extension, &extension) {
                return Err(ArtifactContractError::mismatch(
                    &relative,
                    expected,
                    ArtifactProbe {
                        container: Some(extension.clone()),
                        extension,
                        ..Default::default()
                    },
                    vec![ContractMismatch::new(
                        "container",
                        json!(container),
                        json!(path.extension().and_then(|value| value.to_str())),
                    )],
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct FinalArtifact {
    path: String,
    enforce_profile: bool,
}

fn final_artifacts(operation: Operation, result: &serde_json::Value) -> Vec<FinalArtifact> {
    let mut artifacts = Vec::new();
    match operation {
        Operation::Render | Operation::Preview | Operation::Still => {
            if let Some(values) = result
                .get("artifacts")
                .and_then(serde_json::Value::as_array)
            {
                artifacts.extend(
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(|path| FinalArtifact {
                            path: path.to_owned(),
                            enforce_profile: true,
                        }),
                );
            }
            if let Some(path) = result
                .get("contact_sheet")
                .and_then(|value| value.get("path"))
                .and_then(serde_json::Value::as_str)
            {
                artifacts.push(FinalArtifact {
                    path: path.to_owned(),
                    enforce_profile: false,
                });
            }
        }
        Operation::ContactSheet => {
            if let Some(path) = result.get("path").and_then(serde_json::Value::as_str) {
                artifacts.push(FinalArtifact {
                    path: path.to_owned(),
                    enforce_profile: false,
                });
            }
        }
        Operation::Export | Operation::Captions => {
            if let Some(path) = result.get("path").and_then(serde_json::Value::as_str) {
                artifacts.push(FinalArtifact {
                    path: path.to_owned(),
                    enforce_profile: operation == Operation::Export
                        && matches!(
                            result.get("format").and_then(serde_json::Value::as_str),
                            Some("mp4" | "webm" | "gif")
                        ),
                });
            }
        }
        _ => {}
    }
    artifacts
}

#[derive(Debug, Clone, Default, serde::Serialize)]
struct ArtifactExpectation {
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    container: Option<String>,
    transparent: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
struct ArtifactProbe {
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fps_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    container: Option<String>,
    extension: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pix_fmt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alpha_mode: Option<String>,
    has_alpha: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
struct ContractMismatch {
    field: String,
    expected: serde_json::Value,
    actual: serde_json::Value,
}

impl ContractMismatch {
    fn new(field: &str, expected: serde_json::Value, actual: serde_json::Value) -> Self {
        Self {
            field: field.into(),
            expected,
            actual,
        }
    }
}

#[derive(Debug)]
struct ArtifactContractError {
    code: String,
    message: String,
    data: serde_json::Value,
}

impl ArtifactContractError {
    fn simple(code: &str, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data,
        }
    }

    fn io(path: &str, action: &str, error: std::io::Error) -> Self {
        Self::simple(
            "artifact_unreadable",
            format!("could not {action} final artifact {path}"),
            json!({"path":path,"action":action,"error":error.to_string()}),
        )
    }

    fn signature(path: &str, expected: &str) -> Self {
        Self::simple(
            "artifact_signature_mismatch",
            format!("final artifact {path} is not a readable {expected}"),
            json!({"path":path,"expected":expected}),
        )
    }

    fn mismatch(
        path: &str,
        expected: ArtifactExpectation,
        actual: ArtifactProbe,
        mismatches: Vec<ContractMismatch>,
    ) -> Self {
        Self::simple(
            "artifact_contract_mismatch",
            format!("final artifact {path} does not match the requested render contract"),
            json!({"path":path,"expected":expected,"actual":actual,"mismatches":mismatches}),
        )
    }

    fn into_protocol_error(self) -> ProtocolError {
        ProtocolError {
            code: self.code,
            message: self.message,
            data: Some(self.data),
        }
    }
}

impl std::fmt::Display for ArtifactContractError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ArtifactContractError {}

fn expected_artifact_contract(
    operation: Operation,
    params: &serde_json::Value,
    result: &serde_json::Value,
) -> ArtifactExpectation {
    let profile = result.get("profile");
    let width = value_u32(params.get("width")).or_else(|| {
        profile
            .and_then(|value| value.get("width"))
            .and_then(|value| value_u32(Some(value)))
    });
    let height = value_u32(params.get("height")).or_else(|| {
        profile
            .and_then(|value| value.get("height"))
            .and_then(|value| value_u32(Some(value)))
    });
    let mut fps = value_rate(params.get("fps")).or_else(|| {
        profile
            .and_then(|value| value.get("fps"))
            .and_then(|value| value_rate(Some(value)))
    });
    if matches!(operation, Operation::Still | Operation::ContactSheet) {
        fps = None;
    }
    let requested_format = params
        .get("format")
        .and_then(serde_json::Value::as_str)
        .or_else(|| result.get("format").and_then(serde_json::Value::as_str))
        .map(normalize_container)
        .or_else(|| {
            (operation == Operation::Still || operation == Operation::ContactSheet)
                .then(|| "png".into())
        });
    if operation == Operation::Export && requested_format.as_deref() == Some("gif") {
        fps = value_rate(result.get("effective_fps")).or(fps);
    }
    ArtifactExpectation {
        width,
        height,
        fps,
        container: requested_format,
        transparent: params
            .get("transparent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}

async fn probe_artifact(
    path: &Path,
    relative: &str,
) -> std::result::Result<ArtifactProbe, ArtifactContractError> {
    let probe = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name:stream=codec_type,width,height,pix_fmt,avg_frame_rate,r_frame_rate:stream_tags=alpha_mode",
            "-of",
            "json",
        ])
        .arg(path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    let output = tokio::time::timeout(Duration::from_secs(15), probe)
        .await
        .map_err(|_| {
            ArtifactContractError::simple(
                "artifact_probe_timeout",
                format!("ffprobe timed out for {relative}"),
                json!({"path":relative}),
            )
        })?
        .map_err(|error| ArtifactContractError::io(relative, "probe", error))?;
    if !output.status.success() {
        return Err(ArtifactContractError::simple(
            "artifact_probe_failed",
            format!("ffprobe rejected final artifact {relative}"),
            json!({"path":relative,"stderr":String::from_utf8_lossy(&output.stderr).trim()}),
        ));
    }
    let payload: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        ArtifactContractError::simple(
            "artifact_probe_invalid",
            format!("ffprobe returned invalid metadata for {relative}"),
            json!({"path":relative,"error":error.to_string()}),
        )
    })?;
    let video = payload
        .get("streams")
        .and_then(serde_json::Value::as_array)
        .and_then(|streams| {
            streams.iter().find(|stream| {
                stream.get("codec_type").and_then(serde_json::Value::as_str) == Some("video")
            })
        });
    let fps_raw = video
        .and_then(|stream| {
            stream
                .get("avg_frame_rate")
                .or_else(|| stream.get("r_frame_rate"))
        })
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let pix_fmt = video
        .and_then(|stream| stream.get("pix_fmt"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let alpha_mode = video
        .and_then(|stream| stream.get("tags"))
        .and_then(|tags| tags.get("alpha_mode"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let has_alpha = pix_fmt.as_deref().is_some_and(alpha_capable_pixel_format)
        || alpha_mode
            .as_deref()
            .is_some_and(|value| !value.is_empty() && value != "0");
    let container = payload
        .get("format")
        .and_then(|value| value.get("format_name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Ok(ArtifactProbe {
        width: video
            .and_then(|stream| stream.get("width"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        height: video
            .and_then(|stream| stream.get("height"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        fps: fps_raw.as_deref().and_then(parse_rate),
        fps_raw,
        container,
        extension: path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase(),
        alpha_mode,
        has_alpha,
        pix_fmt,
    })
}

fn compare_artifact_contract(
    path: &str,
    expected: &ArtifactExpectation,
    actual: &ArtifactProbe,
) -> std::result::Result<(), ArtifactContractError> {
    let mut mismatches = Vec::new();
    if let Some(width) = expected.width {
        if actual.width != Some(width) {
            mismatches.push(ContractMismatch::new(
                "width",
                json!(width),
                json!(actual.width),
            ));
        }
    }
    if let Some(height) = expected.height {
        if actual.height != Some(height) {
            mismatches.push(ContractMismatch::new(
                "height",
                json!(height),
                json!(actual.height),
            ));
        }
    }
    if let Some(fps) = expected.fps {
        let tolerance = (fps.abs() * 0.001).max(0.01);
        if actual
            .fps
            .is_none_or(|actual| (actual - fps).abs() > tolerance)
        {
            mismatches.push(ContractMismatch::new(
                "fps",
                json!(fps),
                json!({"value":actual.fps,"rational":actual.fps_raw}),
            ));
        }
    }
    if let Some(container) = expected.container.as_deref() {
        if !container_matches(
            container,
            actual.container.as_deref().unwrap_or_default(),
            &actual.extension,
        ) {
            mismatches.push(ContractMismatch::new(
                "container",
                json!(container),
                json!({"format_name":actual.container,"extension":actual.extension}),
            ));
        }
    }
    if expected.transparent && !actual.has_alpha {
        mismatches.push(ContractMismatch::new(
            "alpha",
            json!("alpha-capable pixel format"),
            json!({"pix_fmt":actual.pix_fmt,"alpha_mode":actual.alpha_mode,"has_alpha":actual.has_alpha}),
        ));
    }
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(ArtifactContractError::mismatch(
            path,
            expected.clone(),
            actual.clone(),
            mismatches,
        ))
    }
}

fn value_u32(value: Option<&serde_json::Value>) -> Option<u32> {
    value
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn value_rate(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(parse_rate))
    })
}

fn parse_rate(value: &str) -> Option<f64> {
    let rate = if let Some((numerator, denominator)) = value.split_once('/') {
        numerator.parse::<f64>().ok()? / denominator.parse::<f64>().ok()?
    } else {
        value.parse::<f64>().ok()?
    };
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

fn normalize_container(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "bundle" | "captions" => "zip".into(),
        other => other.into(),
    }
}

fn container_matches(expected: &str, format_name: &str, extension: &str) -> bool {
    let expected = normalize_container(expected);
    let formats = format_name
        .split(',')
        .map(str::trim)
        .collect::<std::collections::HashSet<_>>();
    match expected.as_str() {
        "mp4" => extension == "mp4" && (formats.contains("mp4") || formats.contains("mov")),
        "mov" => extension == "mov" && formats.contains("mov"),
        "webm" => extension == "webm" && (formats.contains("webm") || formats.contains("matroska")),
        "gif" => extension == "gif" && formats.contains("gif"),
        "png" => extension == "png" && (formats.contains("png_pipe") || formats.contains("png")),
        "jpg" | "jpeg" => {
            matches!(extension, "jpg" | "jpeg")
                && (formats.contains("image2") || formats.contains("mjpeg"))
        }
        "zip" => extension == "zip",
        other => extension == other || formats.contains(other),
    }
}

fn alpha_capable_pixel_format(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("rgba")
        || value.contains("bgra")
        || value.contains("argb")
        || value.contains("abgr")
        || value.starts_with("yuva")
        || value.starts_with("gbrap")
        || value.starts_with("ya")
        || value == "pal8"
}

fn is_visual_extension(value: &str) -> bool {
    matches!(
        value,
        "mp4" | "mov" | "webm" | "gif" | "png" | "jpg" | "jpeg"
    )
}

fn finish_cancelled(
    store: &Store,
    events: &broadcast::Sender<EngineEvent>,
    id: Uuid,
    message: &str,
) {
    let error = ProtocolError::new("cancelled", message);
    let _ = store.finish_error(id, JobStatus::Cancelled, &error);
    if let Ok(Some(finished)) = store.get_job(id) {
        let _ = events.send(EngineEvent::JobFinished {
            job: JobSummary::from(&finished),
            result: None,
            error: Some(error),
        });
    }
}

fn env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::{DirectorSpec, RenderProfile, SceneSpec};

    #[test]
    fn gif_export_contract_uses_declared_centisecond_cadence() {
        let contract = expected_artifact_contract(
            Operation::Export,
            &json!({"format":"gif","fps":15}),
            &json!({"format":"gif","effective_fps":14.28571429}),
        );
        assert!((contract.fps.unwrap() - 100.0 / 7.0).abs() < 0.000_001);
    }

    #[tokio::test]
    async fn stale_cached_artifact_is_evicted_and_job_is_requeued() {
        let directory = tempfile::tempdir().unwrap();
        DirectorSpec::new("cache").save(directory.path()).unwrap();
        std::fs::create_dir_all(directory.path().join("scenes")).unwrap();
        std::fs::write(
            directory.path().join("scenes/main.py"),
            "class MainScene: pass\n",
        )
        .unwrap();
        let store =
            Arc::new(Store::open(directory.path().join(".manim-director/state.db")).unwrap());
        let request = JobRequest {
            operation: Operation::Render,
            params: json!({}),
            priority: 0,
        };
        let mut resolved = request.clone();
        merge_project_defaults(directory.path(), &mut resolved);
        let fingerprint = project_fingerprint(directory.path(), &resolved).unwrap();
        store
            .cache_put(
                &fingerprint,
                directory.path(),
                Operation::Render,
                &json!({"artifacts":["output/deleted.mp4"]}),
            )
            .unwrap();
        let scheduler = Scheduler::start(
            store.clone(),
            SchedulerConfig {
                concurrency: 1,
                queue_capacity: 1,
                job_timeout: Duration::from_secs(10),
                bridge: BridgeConfig {
                    python: "missing-manim-director-test-python".into(),
                    module: "missing_runtime".into(),
                    memory_mb: 128,
                },
            },
        );
        let job = scheduler.submit(directory.path(), request).await.unwrap();
        assert!(!job.cached);
        assert!(store.cache_get(&fingerprint).unwrap().is_none());
    }

    #[tokio::test]
    async fn cache_control_params_bypass_and_flush_director_cache() {
        let directory = tempfile::tempdir().unwrap();
        DirectorSpec::new("cache-controls")
            .save(directory.path())
            .unwrap();
        std::fs::create_dir_all(directory.path().join("scenes")).unwrap();
        std::fs::write(
            directory.path().join("scenes/main.py"),
            "class MainScene: pass\n",
        )
        .unwrap();
        let store =
            Arc::new(Store::open(directory.path().join(".manim-director/state.db")).unwrap());
        store
            .cache_put(
                "sentinel",
                directory.path(),
                Operation::Inspect,
                &json!({"ok":true}),
            )
            .unwrap();
        let scheduler = Scheduler::start(
            store.clone(),
            SchedulerConfig {
                concurrency: 1,
                queue_capacity: 4,
                job_timeout: Duration::from_secs(10),
                bridge: BridgeConfig {
                    python: "missing-manim-director-test-python".into(),
                    module: "missing_runtime".into(),
                    memory_mb: 128,
                },
            },
        );
        let bypass = scheduler
            .submit(
                directory.path(),
                JobRequest {
                    operation: Operation::Inspect,
                    params: json!({"disable_caching":true}),
                    priority: 0,
                },
            )
            .await
            .unwrap();
        assert!(!bypass.cached);
        assert!(bypass.fingerprint.is_none());
        assert!(store.cache_get("sentinel").unwrap().is_some());

        let refresh = scheduler
            .submit(
                directory.path(),
                JobRequest {
                    operation: Operation::Inspect,
                    params: json!({"flush_cache":true}),
                    priority: 0,
                },
            )
            .await
            .unwrap();
        assert!(!refresh.cached);
        assert!(refresh.fingerprint.is_none());
        assert!(store.cache_get("sentinel").unwrap().is_none());
    }

    #[test]
    fn built_in_profile_keeps_runtime_dimensions_but_named_override_is_injected() {
        let directory = tempfile::tempdir().unwrap();
        let mut spec = DirectorSpec::new("profiles");
        spec.save(directory.path()).unwrap();
        let mut request = JobRequest {
            operation: Operation::Preview,
            params: json!({}),
            priority: 0,
        };
        merge_project_defaults(directory.path(), &mut request);
        assert!(request.params.get("width").is_none());
        spec.profiles.insert(
            "preview".into(),
            RenderProfile {
                resolution: Some([854, 480]),
                fps: Some(15),
                ..Default::default()
            },
        );
        spec.save(directory.path()).unwrap();
        let mut request = JobRequest {
            operation: Operation::Preview,
            params: json!({}),
            priority: 0,
        };
        merge_project_defaults(directory.path(), &mut request);
        assert_eq!(request.params["width"], 854);
        assert_eq!(request.params["height"], 480);
        assert_eq!(request.params["fps"], 15);
    }

    #[test]
    fn scene_id_and_class_resolve_to_their_declared_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut spec = DirectorSpec::new("mapping");
        spec.scenes = vec![
            SceneSpec {
                id: "intro".into(),
                class_name: Some("IntroScene".into()),
                file: Some("scenes/intro.py".into()),
                ..Default::default()
            },
            SceneSpec {
                id: "outro".into(),
                class_name: Some("OutroScene".into()),
                file: Some("scenes/outro.py".into()),
                ..Default::default()
            },
        ];
        spec.save(directory.path()).unwrap();

        let mut by_id = JobRequest {
            operation: Operation::Render,
            params: json!({"scene":"intro"}),
            priority: 0,
        };
        merge_project_defaults(directory.path(), &mut by_id);
        assert_eq!(by_id.params["scene"], "IntroScene");
        assert_eq!(by_id.params["scene_file"], "scenes/intro.py");

        let mut by_class = JobRequest {
            operation: Operation::Render,
            params: json!({"scene":"OutroScene"}),
            priority: 0,
        };
        merge_project_defaults(directory.path(), &mut by_class);
        assert_eq!(by_class.params["scene"], "OutroScene");
        assert_eq!(by_class.params["scene_file"], "scenes/outro.py");
    }

    #[test]
    fn qa_hydrates_latest_matching_render_artifact() {
        let directory = tempfile::tempdir().unwrap();
        DirectorSpec::new("qa").save(directory.path()).unwrap();
        std::fs::create_dir_all(directory.path().join(".manim-director/media")).unwrap();
        std::fs::write(
            directory.path().join(".manim-director/media/clip.mp4"),
            b"video",
        )
        .unwrap();
        let store = Store::open(directory.path().join(".manim-director/state.db")).unwrap();
        let render_id = Uuid::new_v4();
        store
            .create_job(
                render_id,
                directory.path(),
                Operation::Render,
                &json!({"scene":"IntroScene","profile":"preview"}),
                None,
            )
            .unwrap();
        store.set_running(render_id).unwrap();
        store
            .finish_success(
                render_id,
                &json!({"artifacts":[".manim-director/media/clip.mp4"]}),
            )
            .unwrap();
        let mut request = JobRequest {
            operation: Operation::Qa,
            params: json!({"scene":"IntroScene","profile":"preview"}),
            priority: 0,
        };
        hydrate_qa_request(directory.path(), &store, &mut request).unwrap();
        assert!(request.params["source"]
            .as_str()
            .unwrap()
            .ends_with(".manim-director/media/clip.mp4"));
        assert_eq!(request.params["source_job_id"], render_id.to_string());
    }

    #[test]
    fn bundle_export_unions_project_defaults_with_generated_artifact() {
        let directory = tempfile::tempdir().unwrap();
        DirectorSpec::new("bundle").save(directory.path()).unwrap();
        std::fs::create_dir_all(directory.path().join(".manim-director/media")).unwrap();
        std::fs::write(
            directory.path().join(".manim-director/media/clip.mp4"),
            b"video",
        )
        .unwrap();
        let store = Store::open(directory.path().join(".manim-director/state.db")).unwrap();
        let render_id = Uuid::new_v4();
        store
            .create_job(
                render_id,
                directory.path(),
                Operation::Render,
                &json!({"scene":"MainScene"}),
                None,
            )
            .unwrap();
        store.set_running(render_id).unwrap();
        store
            .finish_success(
                render_id,
                &json!({"artifacts":[".manim-director/media/clip.mp4"]}),
            )
            .unwrap();
        let mut request = JobRequest {
            operation: Operation::Export,
            params: json!({"format":"bundle","job_id":render_id}),
            priority: 0,
        };
        hydrate_export_request(directory.path(), &store, &mut request).unwrap();
        let includes = request.params["include"].as_array().unwrap();
        assert!(includes.iter().any(|value| value == "director.yaml"));
        assert!(includes
            .iter()
            .any(|value| value == ".manim-director/media/clip.mp4"));
        assert!(!request.params["exclude"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == ".manim-director/**"));
        assert!(request.params.get("source").is_none());
        assert_eq!(
            request.params["artifacts"],
            json!([".manim-director/media/clip.mp4"])
        );
    }

    #[test]
    fn artifact_contract_reports_dimension_fps_container_and_alpha_mismatches() {
        let expected = ArtifactExpectation {
            width: Some(1920),
            height: Some(1080),
            fps: Some(60.0),
            container: Some("mp4".into()),
            transparent: true,
        };
        let actual = ArtifactProbe {
            width: Some(1280),
            height: Some(720),
            fps: Some(30.0),
            fps_raw: Some("30/1".into()),
            container: Some("matroska,webm".into()),
            extension: "webm".into(),
            pix_fmt: Some("yuv420p".into()),
            alpha_mode: None,
            has_alpha: false,
        };
        let error = compare_artifact_contract("output/wrong.webm", &expected, &actual)
            .unwrap_err()
            .into_protocol_error();
        assert_eq!(error.code, "artifact_contract_mismatch");
        let data = error.data.unwrap();
        let fields = data["mismatches"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value["field"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(fields, ["width", "height", "fps", "container", "alpha"]);
    }

    #[test]
    fn rational_fps_and_alpha_capable_format_satisfy_contract() {
        let expected = ArtifactExpectation {
            width: Some(1920),
            height: Some(1080),
            fps: Some(59.94),
            container: Some("webm".into()),
            transparent: true,
        };
        let actual = ArtifactProbe {
            width: Some(1920),
            height: Some(1080),
            fps: parse_rate("60000/1001"),
            fps_raw: Some("60000/1001".into()),
            container: Some("matroska,webm".into()),
            extension: "webm".into(),
            pix_fmt: Some("yuva420p".into()),
            alpha_mode: None,
            has_alpha: true,
        };
        compare_artifact_contract("output/right.webm", &expected, &actual).unwrap();
    }

    #[test]
    fn webm_alpha_metadata_satisfies_transparency_contract() {
        let expected = ArtifactExpectation {
            transparent: true,
            container: Some("webm".into()),
            ..ArtifactExpectation::default()
        };
        let actual = ArtifactProbe {
            container: Some("matroska,webm".into()),
            extension: "webm".into(),
            pix_fmt: Some("yuv420p".into()),
            alpha_mode: Some("1".into()),
            has_alpha: true,
            ..ArtifactProbe::default()
        };
        compare_artifact_contract("output/transparent.webm", &expected, &actual).unwrap();
    }

    #[test]
    fn final_artifact_selection_ignores_source_inputs() {
        let artifacts = final_artifacts(
            Operation::Export,
            &json!({"path":"output/final.webm","source":"output/input.mp4","format":"webm"}),
        );
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "output/final.webm");
    }
}
