use crate::{apply_source_mutation, read_source, Scheduler, SourceMutation, SourceReadQuery};
use anyhow::Result;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{header, HeaderValue, StatusCode, Uri},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use manim_director_core::{
    CursorPage, DirectorSpec, EngineEvent, JobRecord, JobRequest, LogRecord, Operation,
    ProjectInventory, ProtocolError, SceneSpec, StoryboardBeat,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    convert::Infallible,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};
use tokio::net::TcpListener;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use tokio_util::io::ReaderStream;
use tower_http::{services::ServeDir, trace::TraceLayer};
use uuid::Uuid;

include!(concat!(env!("OUT_DIR"), "/embedded_workbench.rs"));

#[derive(Clone)]
struct ApiState {
    project_root: Arc<PathBuf>,
    scheduler: Scheduler,
}

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub address: SocketAddr,
    pub project_root: PathBuf,
    pub workbench_dir: Option<PathBuf>,
}

pub async fn serve(config: ServeConfig, scheduler: Scheduler) -> Result<()> {
    let state = ApiState {
        project_root: Arc::new(config.project_root.canonicalize()?),
        scheduler,
    };
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/state", get(project_state))
        .route("/api/state/source", get(source_read).put(source_write))
        .route("/api/intents", post(create_intent))
        .route("/api/ingest", post(create_ingest))
        .route("/api/renders", post(create_render))
        .route("/api/renders/{id}", get(get_render))
        .route("/api/renders/{id}/cancel", post(cancel_render))
        .route("/api/qa", post(create_qa))
        .route("/api/exports", post(create_export))
        .route("/api/logs", get(logs))
        .route("/api/events", get(events))
        .route("/api/files", get(download_file))
        // Source replacement is capped at 2 MiB by edit.rs; bounded JSON
        // framing headroom keeps every loadable file saveable.
        .layer(DefaultBodyLimit::max(3 * 1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let app = if let Some(directory) = config.workbench_dir.filter(|path| path.is_dir()) {
        api.fallback_service(ServeDir::new(directory).append_index_html_on_directories(true))
    } else {
        api.fallback(embedded_workbench)
    };
    let listener = TcpListener::bind(config.address).await?;
    tracing::info!(address = %listener.local_addr()?, "Manim Director server ready");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct FileQuery {
    path: String,
}

async fn download_file(
    State(state): State<ApiState>,
    Query(query): Query<FileQuery>,
) -> ApiResult<Response> {
    let path = safe_download_path(state.project_root.as_ref(), &query.path)
        .map_err(ApiError::bad_request)?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(ApiError::not_found)?;
    if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 * 1024 {
        return Err(ApiError::bad_request(
            "file is not a bounded regular artifact",
        ));
    }
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(ApiError::not_found)?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact")
        .replace('"', "_")
        .replace('\r', "_")
        .replace('\n', "_");
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(ApiError::internal)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .as_ref(),
        )
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from_stream(ReaderStream::new(file)))
        .map_err(ApiError::internal)
}

fn safe_download_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("artifact path must be project-relative and cannot contain ..");
    }
    if relative.starts_with(".manim-director/state.db")
        || relative.starts_with(".manim-director/undo")
        || relative.starts_with(".manim-director/tmp")
    {
        anyhow::bail!("working-state files cannot be downloaded");
    }
    let extension = relative
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "mp4"
            | "mov"
            | "webm"
            | "gif"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "svg"
            | "wav"
            | "mp3"
            | "ogg"
            | "vtt"
            | "srt"
            | "zip"
            | "json"
            | "yaml"
            | "yml"
            | "py"
            | "tex"
            | "typ"
            | "md"
            | "csv"
            | "txt"
            | "pdf"
    ) {
        anyhow::bail!("artifact extension is not downloadable");
    }
    let root = root.canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(&root) || !path.is_file() {
        anyhow::bail!("artifact is outside the project or not a file");
    }
    Ok(path)
}

async fn source_read(
    State(state): State<ApiState>,
    Query(query): Query<SourceReadQuery>,
) -> ApiResult<Json<crate::SourceView>> {
    let source = read_source(state.project_root.as_ref(), &query).map_err(ApiError::bad_request)?;
    Ok(Json(source))
}

async fn source_write(
    State(state): State<ApiState>,
    Json(mutation): Json<SourceMutation>,
) -> ApiResult<Json<crate::SourceMutationResult>> {
    let result = apply_source_mutation(state.project_root.as_ref(), mutation)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(result))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}))
}

#[derive(Debug, Deserialize)]
struct ProjectQuery {
    project: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct StateResponse {
    project_root: String,
    spec: DirectorSpec,
    files: FileSummary,
    jobs: CursorPage<JobRecord>,
    scenes: Vec<SceneSpec>,
    storyboard: Vec<StoryboardBeat>,
    duration_seconds: Option<f64>,
    latest_artifact: Option<ArtifactRef>,
}

#[derive(Debug, Serialize)]
struct ArtifactRef {
    path: String,
    download_url: String,
    job_id: Uuid,
}

#[derive(Debug, Serialize)]
struct FileSummary {
    source_count: usize,
    asset_count: usize,
    output_count: usize,
    sources: Vec<String>,
    assets: Vec<String>,
}

async fn project_state(
    State(state): State<ApiState>,
    Query(query): Query<ProjectQuery>,
) -> ApiResult<Json<StateResponse>> {
    ensure_project(&state, query.project.as_deref())?;
    let inventory =
        ProjectInventory::scan(state.project_root.as_ref()).map_err(ApiError::bad_request)?;
    let files = FileSummary {
        source_count: inventory.source_files.len(),
        asset_count: inventory.asset_files.len(),
        output_count: inventory.output_files.len(),
        sources: inventory
            .source_files
            .iter()
            .take(200)
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        assets: inventory
            .asset_files
            .iter()
            .take(200)
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    };
    let jobs = state
        .scheduler
        .store()
        .jobs(None, 50)
        .map_err(ApiError::internal)?;
    let scenes = inventory.spec.scenes.clone();
    let storyboard = inventory.spec.storyboard.clone();
    let duration_seconds = inventory
        .spec
        .brief
        .as_ref()
        .and_then(|brief| brief.duration_seconds)
        .or_else(|| {
            let sum = storyboard
                .iter()
                .filter_map(|beat| beat.duration)
                .sum::<f64>();
            (sum > 0.0).then_some(sum)
        });
    let latest_artifact = latest_artifact(inventory.root.as_path(), &jobs.items);
    Ok(Json(StateResponse {
        project_root: inventory.root.to_string_lossy().into_owned(),
        spec: inventory.spec,
        files,
        jobs,
        scenes,
        storyboard,
        duration_seconds,
        latest_artifact,
    }))
}

fn latest_artifact(root: &Path, jobs: &[JobRecord]) -> Option<ArtifactRef> {
    for job in jobs
        .iter()
        .filter(|job| job.status == manim_director_core::JobStatus::Succeeded)
    {
        let mut candidates = Vec::new();
        let Some(result) = job.result.as_ref() else {
            continue;
        };
        collect_result_strings(result, &mut candidates);
        for candidate in candidates {
            let raw = Path::new(&candidate);
            let path = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                root.join(raw)
            };
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            if !path.is_file() || !path.starts_with(root) {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            let encoded = percent_encode(&relative);
            return Some(ArtifactRef {
                path: relative,
                download_url: format!("/api/files?path={encoded}"),
                job_id: job.id,
            });
        }
    }
    None
}

fn collect_result_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) if is_artifact_name(value) => output.push(value.clone()),
        Value::String(_) => {}
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_result_strings(value, output)),
        Value::Object(values) => values
            .values()
            .for_each(|value| collect_result_strings(value, output)),
        _ => {}
    }
}

fn is_artifact_name(value: &str) -> bool {
    matches!(
        Path::new(value)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "mp4"
                | "mov"
                | "webm"
                | "gif"
                | "png"
                | "jpg"
                | "jpeg"
                | "webp"
                | "svg"
                | "wav"
                | "mp3"
                | "ogg"
                | "vtt"
                | "srt"
                | "zip"
                | "json"
                | "yaml"
                | "yml"
                | "py"
                | "tex"
                | "typ"
                | "md"
                | "csv"
                | "txt"
                | "pdf"
        )
    )
}

fn percent_encode(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            output.push(byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(output, "%{byte:02X}");
        }
    }
    output
}

#[derive(Debug, Deserialize)]
struct IntentRequest {
    intent: String,
    operation: Option<String>,
    #[serde(default)]
    params: Map<String, Value>,
}

async fn create_intent(
    State(state): State<ApiState>,
    Json(body): Json<IntentRequest>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    let operation = if let Some(operation) = body.operation {
        Operation::from_str(&operation).map_err(ApiError::bad_request)?
    } else {
        classify_intent(&body.intent)
    };
    let mut params = body.params;
    params.insert("intent".into(), Value::String(body.intent));
    submit(
        &state,
        JobRequest {
            operation,
            params: Value::Object(params),
            priority: 0,
        },
    )
    .await
}

#[derive(Debug, Deserialize)]
struct IngestRequest {
    paths: Vec<String>,
    #[serde(default)]
    params: Map<String, Value>,
}

async fn create_ingest(
    State(state): State<ApiState>,
    Json(body): Json<IngestRequest>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    if body.paths.is_empty() {
        return Err(ApiError::bad_request("paths cannot be empty"));
    }
    let mut params = body.params;
    params.insert("sources".into(), json!(body.paths));
    submit(
        &state,
        JobRequest {
            operation: Operation::Ingest,
            params: Value::Object(params),
            priority: 0,
        },
    )
    .await
}

#[derive(Debug, Deserialize)]
struct RenderRequest {
    scene: Option<String>,
    profile: Option<String>,
    section: Option<String>,
    #[serde(default)]
    params: Map<String, Value>,
}

async fn create_render(
    State(state): State<ApiState>,
    Json(body): Json<RenderRequest>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    let mut params = body.params;
    insert_some(&mut params, "scene", body.scene);
    insert_some(&mut params, "profile", body.profile);
    insert_some(&mut params, "section", body.section);
    submit(
        &state,
        JobRequest {
            operation: Operation::Render,
            params: Value::Object(params),
            priority: 0,
        },
    )
    .await
}

#[derive(Debug, Deserialize)]
struct QaRequest {
    job_id: Option<Uuid>,
    scene: Option<String>,
    profile: Option<String>,
    source: Option<String>,
    images: Option<Vec<String>>,
    #[serde(default)]
    params: Map<String, Value>,
}

async fn create_qa(
    State(state): State<ApiState>,
    Json(body): Json<QaRequest>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    let mut params = body.params;
    if let Some(id) = body.job_id {
        params.insert("job_id".into(), json!(id));
    }
    insert_some(&mut params, "scene", body.scene);
    insert_some(&mut params, "profile", body.profile);
    insert_some(&mut params, "source", body.source);
    if let Some(images) = body.images {
        params.insert("images".into(), json!(images));
    }
    submit(
        &state,
        JobRequest {
            operation: Operation::Qa,
            params: Value::Object(params),
            priority: 0,
        },
    )
    .await
}

#[derive(Debug, Deserialize)]
struct ExportRequest {
    format: Option<String>,
    output: Option<String>,
    job_id: Option<Uuid>,
    #[serde(default)]
    params: Map<String, Value>,
}

async fn create_export(
    State(state): State<ApiState>,
    Json(body): Json<ExportRequest>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    let mut params = body.params;
    insert_some(&mut params, "format", body.format);
    insert_some(&mut params, "output", body.output);
    if let Some(id) = body.job_id {
        params.insert("job_id".into(), json!(id));
    }
    submit(
        &state,
        JobRequest {
            operation: Operation::Export,
            params: Value::Object(params),
            priority: 0,
        },
    )
    .await
}

async fn submit(state: &ApiState, request: JobRequest) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    let job = state
        .scheduler
        .submit(state.project_root.as_ref(), request)
        .await
        .map_err(ApiError::internal)?;
    let status = if job.cached {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    Ok((status, Json(job)))
}

async fn get_render(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<Uuid>,
) -> ApiResult<Json<JobRecord>> {
    let job = state
        .scheduler
        .store()
        .get_job(id)
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::not_found(format!("job {id} not found")))?;
    Ok(Json(job))
}

async fn cancel_render(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<Uuid>,
) -> ApiResult<Json<JobRecord>> {
    let job = state.scheduler.cancel(id).map_err(ApiError::not_found)?;
    Ok(Json(job))
}

#[derive(Debug, Deserialize)]
struct LogQuery {
    cursor: Option<i64>,
    limit: Option<usize>,
    job_id: Option<Uuid>,
}

async fn logs(
    State(state): State<ApiState>,
    Query(query): Query<LogQuery>,
) -> ApiResult<Json<CursorPage<LogRecord>>> {
    let page = state
        .scheduler
        .store()
        .logs(query.job_id, query.cursor, query.limit.unwrap_or(100))
        .map_err(ApiError::internal)?;
    Ok(Json(page))
}

async fn events(
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream =
        BroadcastStream::new(state.scheduler.subscribe()).filter_map(|message| match message {
            Ok(event) => Some(Ok(Event::default()
                .event(event_name(&event))
                .json_data(event)
                .unwrap_or_else(|_| Event::default().event("serialization_error")))),
            Err(_) => None,
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn event_name(event: &EngineEvent) -> &'static str {
    match event {
        EngineEvent::JobQueued { .. } => "job_queued",
        EngineEvent::JobStarted { .. } => "job_started",
        EngineEvent::JobProgress { .. } => "job_progress",
        EngineEvent::JobFinished { .. } => "job_finished",
    }
}

fn classify_intent(value: &str) -> Operation {
    let normalized = value.to_ascii_lowercase();
    if normalized.contains("scaffold")
        || normalized.contains("initialize")
        || normalized.contains("init project")
        || normalized.contains("new project")
    {
        Operation::Scaffold
    } else if normalized.contains("ingest")
        || normalized.contains("import")
        || normalized.contains("attach")
    {
        Operation::Ingest
    } else if normalized.contains("export") || normalized.contains("package") {
        Operation::Export
    } else if normalized.contains("debug")
        || normalized.contains("fix")
        || normalized.contains("crash")
        || normalized.contains("error")
    {
        Operation::Debug
    } else if normalized.contains("quality")
        || normalized.contains("qa")
        || normalized.contains("clip")
        || normalized.contains("overlap")
    {
        Operation::Qa
    } else if normalized.contains("preview") || normalized.contains("draft") {
        Operation::Preview
    } else if normalized.contains("edit")
        || normalized.contains("change")
        || normalized.contains("update")
        || normalized.contains("replace")
        || normalized.contains("restyle")
    {
        Operation::Inspect
    } else if normalized.contains("migrate")
        || normalized.contains("upgrade")
        || normalized.contains("convert version")
    {
        Operation::Inspect
    } else if normalized.contains("inspect")
        || normalized.contains("explain")
        || normalized.contains("discover")
    {
        Operation::Inspect
    } else if normalized.contains("render")
        || normalized.contains("animate")
        || normalized.contains("create video")
    {
        Operation::Render
    } else {
        Operation::Inspect
    }
}

fn insert_some(map: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        map.insert(key.into(), Value::String(value));
    }
}

fn ensure_project(state: &ApiState, candidate: Option<&Path>) -> ApiResult<()> {
    let Some(candidate) = candidate else {
        return Ok(());
    };
    let candidate = candidate.canonicalize().map_err(ApiError::bad_request)?;
    if candidate != *state.project_root {
        return Err(ApiError::bad_request(
            "server is scoped to a different project",
        ));
    }
    Ok(())
}

async fn embedded_workbench(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    if requested.starts_with("api/") {
        return ApiError::not_found("API route not found").into_response();
    }
    let key = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let asset = embedded_asset(key).or_else(|| embedded_asset("index.html"));
    match asset {
        Some((bytes, mime)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(
                header::CACHE_CONTROL,
                if key == "index.html" {
                    "no-cache"
                } else {
                    "public, max-age=31536000, immutable"
                },
            )
            .body(Body::from(bytes))
            .unwrap(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

struct ApiError {
    status: StatusCode,
    error: ProtocolError,
}

impl ApiError {
    fn bad_request(error: impl ToString) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            error: ProtocolError::new("bad_request", error.to_string()),
        }
    }
    fn not_found(error: impl ToString) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            error: ProtocolError::new("not_found", error.to_string()),
        }
    }
    fn internal(error: impl ToString) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: ProtocolError::new("internal_error", error.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.error}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_paths_are_confined_and_allowlisted() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("video.mp4"), b"media").unwrap();
        std::fs::write(directory.path().join("secret.bin"), b"no").unwrap();
        assert!(safe_download_path(directory.path(), "video.mp4").is_ok());
        assert!(safe_download_path(directory.path(), "../video.mp4").is_err());
        assert!(safe_download_path(directory.path(), "secret.bin").is_err());
    }
}
