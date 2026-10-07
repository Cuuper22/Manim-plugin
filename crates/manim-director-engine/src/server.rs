use crate::{
    confine, latest_render, parse_params, read_source, write_source, Scheduler, SourcePage,
    SourceWrite, SourceWriteResult, Submission,
};
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
    files, ArtifactKind, CursorPage, DirectorSpec, EngineError, EngineEvent, ErrorBody, JobOrigin,
    JobRecord, LogRecord, Operation, ProjectInventory, SceneSpec, StoryboardBeat, ARTIFACTS_DIR,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tokio::net::TcpListener;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use tokio_util::io::ReaderStream;
use tower_http::{services::ServeDir, trace::TraceLayer};
use uuid::Uuid;

include!(concat!(env!("OUT_DIR"), "/embedded_workbench.rs"));

#[derive(Clone)]
struct ApiState {
    scheduler: Scheduler,
}

impl ApiState {
    fn root(&self) -> &Path {
        self.scheduler.root()
    }
}

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub address: SocketAddr,
    pub workbench_dir: Option<PathBuf>,
}

pub async fn serve(config: ServeConfig, scheduler: Scheduler) -> Result<()> {
    let state = ApiState {
        scheduler: scheduler.clone(),
    };
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/state", get(project_state))
        .route("/api/source", get(source_read).put(source_write))
        .route("/api/renders", post(create_render))
        .route("/api/renders/{id}", get(get_job))
        .route("/api/renders/{id}/cancel", post(cancel_job))
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
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    scheduler.shutdown().await;
    Ok(served?)
}

#[derive(Debug, Deserialize)]
struct FileQuery {
    path: String,
}

async fn download_file(
    State(state): State<ApiState>,
    Query(query): Query<FileQuery>,
) -> ApiResult<Response> {
    let path = downloadable(state.root(), &query.path)?;
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|_| not_found_file(&query.path))?;
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| not_found_file(&query.path))?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("artifact")
        .replace(['"', '\r', '\n'], "_");
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
        .map_err(EngineError::internal)?;
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
        .map_err(|error| EngineError::internal(error).into())
}

/// Project files the workbench may fetch: allowlisted extensions, and from
/// engine state only job artifacts.
fn downloadable(root: &Path, relative: &str) -> Result<PathBuf, ApiError> {
    let path = confine(root, relative).map_err(|_| not_found_file(relative))?;
    let state = root.join(".manim-director");
    let artifacts = root.join(ARTIFACTS_DIR);
    if !files::has_extension(&path, files::DOWNLOADABLE)
        || (path.starts_with(&state) && !path.starts_with(&artifacts))
    {
        return Err(EngineError::invalid("path", "denied").into());
    }
    Ok(path)
}

fn not_found_file(path: &str) -> ApiError {
    EngineError::NotFound {
        resource: manim_director_core::Resource::File,
        key: path.to_owned(),
    }
    .into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceQuery {
    path: String,
    start_line: Option<u64>,
    end_line: Option<u64>,
}

async fn source_read(
    State(state): State<ApiState>,
    Query(query): Query<SourceQuery>,
) -> ApiResult<Json<SourcePage>> {
    let root = state.root().to_path_buf();
    let page = tokio::task::spawn_blocking(move || {
        read_source(&root, &query.path, query.start_line, query.end_line)
    })
    .await
    .map_err(EngineError::internal)??;
    Ok(Json(page))
}

async fn source_write(
    State(state): State<ApiState>,
    Json(write): Json<SourceWrite>,
) -> ApiResult<Json<SourceWriteResult>> {
    let root = state.root().to_path_buf();
    let python = state.scheduler.python().to_path_buf();
    let store = state.scheduler.store().clone();
    let result = tokio::task::spawn_blocking(move || {
        let index = store.newest_discover().map_err(EngineError::internal)?;
        write_source(&root, &python, &write, index.as_ref())
    })
    .await
    .map_err(EngineError::internal)??;
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

async fn project_state(State(state): State<ApiState>) -> ApiResult<Json<StateResponse>> {
    let root = state.root().to_path_buf();
    let store = state.scheduler.store().clone();
    let response = tokio::task::spawn_blocking(move || -> Result<StateResponse, EngineError> {
        let spec = DirectorSpec::load(&root)?;
        let inventory = ProjectInventory::scan(&root, &spec);
        let listed = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .take(200)
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect()
        };
        let files = FileSummary {
            source_count: inventory.source_files.len(),
            asset_count: inventory.asset_files.len(),
            output_count: inventory.output_files.len(),
            sources: listed(&inventory.source_files),
            assets: listed(&inventory.asset_files),
        };
        let jobs = store.jobs(None, 50).map_err(EngineError::internal)?;
        let duration_seconds = spec.brief.duration_seconds.or_else(|| {
            let sum: f64 = spec
                .storyboard
                .iter()
                .filter_map(|beat| beat.duration)
                .sum();
            (sum > 0.0).then_some(sum)
        });
        let latest_artifact = latest_render(&store, &root, None, None, None)
            .map_err(EngineError::internal)?
            .and_then(|job| {
                let video = job.result.as_ref()?.artifact(ArtifactKind::Video)?;
                Some(ArtifactRef {
                    download_url: format!("/api/files?path={}", percent_encode(&video.path)),
                    path: video.path.clone(),
                    job_id: job.id,
                })
            });
        Ok(StateResponse {
            project_root: root.to_string_lossy().into_owned(),
            files,
            jobs,
            scenes: spec.scenes.clone(),
            storyboard: spec.storyboard.clone(),
            duration_seconds,
            latest_artifact,
            spec,
        })
    })
    .await
    .map_err(EngineError::internal)??;
    Ok(Json(response))
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

async fn create_render(
    State(state): State<ApiState>,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    submit(&state, Operation::Render, body).await
}

async fn create_qa(
    State(state): State<ApiState>,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    submit(&state, Operation::Qa, body).await
}

async fn create_export(
    State(state): State<ApiState>,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    submit(&state, Operation::Export, body).await
}

async fn submit(
    state: &ApiState,
    operation: Operation,
    params: Value,
) -> ApiResult<(StatusCode, Json<JobRecord>)> {
    let request = parse_params(operation, params)?;
    let submission = state.scheduler.submit(JobOrigin::Http, request).await?;
    let status = match submission {
        Submission::Queued(_) => StatusCode::ACCEPTED,
        Submission::Cached(_) | Submission::Coalesced(_) => StatusCode::OK,
    };
    Ok((status, Json(submission.into_job())))
}

async fn get_job(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<Uuid>,
) -> ApiResult<Json<JobRecord>> {
    let job = state
        .scheduler
        .store()
        .blocking(move |store| store.get_job(id))
        .await
        .map_err(EngineError::internal)?
        .ok_or_else(|| EngineError::job_not_found(id))?;
    Ok(Json(job))
}

async fn cancel_job(
    State(state): State<ApiState>,
    AxumPath(id): AxumPath<Uuid>,
) -> ApiResult<Json<JobRecord>> {
    Ok(Json(state.scheduler.cancel(id).await?))
}

#[derive(Debug, Deserialize)]
struct LogQuery {
    job_id: Uuid,
    cursor: Option<i64>,
    limit: Option<usize>,
}

async fn logs(
    State(state): State<ApiState>,
    Query(query): Query<LogQuery>,
) -> ApiResult<Json<CursorPage<LogRecord>>> {
    let page = state
        .scheduler
        .store()
        .blocking(move |store| store.logs(query.job_id, query.cursor, query.limit.unwrap_or(100)))
        .await
        .map_err(EngineError::internal)?;
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

async fn embedded_workbench(uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    if requested.starts_with("api/") {
        return ApiError::from(EngineError::NotFound {
            resource: manim_director_core::Resource::Route,
            key: uri.path().to_owned(),
        })
        .into_response();
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
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

struct ApiError {
    status: StatusCode,
    error: ErrorBody,
}

impl From<EngineError> for ApiError {
    fn from(error: EngineError) -> Self {
        Self {
            status: StatusCode::from_u16(error.status())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error: error.body(),
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
    fn downloads_are_confined_allowlisted_and_skip_engine_state() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        for path in [
            "video.mp4",
            "secret.bin",
            ".manim-director/state.db",
            ".manim-director/undo/a.json",
            ".manim-director/artifacts/job/clip.mp4",
        ] {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        assert!(downloadable(&root, "video.mp4").is_ok());
        assert!(downloadable(&root, ".manim-director/artifacts/job/clip.mp4").is_ok());
        for denied in [
            "../video.mp4",
            "secret.bin",
            ".manim-director/state.db",
            ".manim-director/undo/a.json",
        ] {
            assert!(downloadable(&root, denied).is_err(), "{denied}");
        }
    }
}
