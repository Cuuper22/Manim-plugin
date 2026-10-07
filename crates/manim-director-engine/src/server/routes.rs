//! The JSON routes of HTTP §6: health, the workspace snapshot, jobs, job
//! logs and the source API.

use super::{
    error::{json_value, parse_json, ApiError, ApiQuery, ApiResult, JobId},
    state::{AppState, EngineInfo, REQUEST_BODY_BYTES, SOURCE_BODY_BYTES},
    watch::SPEC_SECTIONS,
};
use crate::{
    parse_request, read_source,
    workspace::{job_detail, job_summary, JobDetail, JobView, Section, Sections},
    write_source, Frontend, SourcePage, SourceWrite, SourceWriteResult, Submission,
};
use axum::{
    body::Bytes,
    extract::{rejection::BytesRejection, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use manim_director_core::{files, EngineError, JobOrigin, JobRecord, LogRecord, SPEC_FILE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const JOBS_IN_STATE: usize = 50;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "api_version": 2,
        "instance_id": state.scheduler.instance_id(),
    }))
}

#[derive(Serialize)]
pub struct WorkspaceState {
    engine: EngineInfo,
    /// The id of the newest event this snapshot includes.
    event_cursor: String,
    jobs: Vec<JobView>,
    jobs_next_before: Option<String>,
    #[serde(flatten)]
    sections: Sections,
}

/// What the engine knows now; never waits for an index refresh.
pub async fn workspace(State(state): State<AppState>) -> ApiResult<Json<WorkspaceState>> {
    // Read first: events after this cursor may already be in the snapshot,
    // which the client tolerates; the reverse would lose them.
    let event_cursor = state.hub.cursor();
    let snapshot = state.clone();
    let built = tokio::task::spawn_blocking(move || -> Result<_, EngineError> {
        let sections = snapshot.sections(&Section::ALL)?;
        let page = snapshot
            .scheduler
            .store()
            .jobs(None, JOBS_IN_STATE)
            .map_err(EngineError::internal)?;
        let root = snapshot.root();
        let jobs = page
            .items
            .iter()
            .map(|job| job_summary(root, job))
            .collect();
        Ok((sections, jobs, page.next_cursor))
    })
    .await
    .map_err(EngineError::internal)??;
    let (sections, jobs, jobs_next_before) = built;
    Ok(Json(WorkspaceState {
        engine: state.engine_info(),
        event_cursor,
        jobs,
        jobs_next_before,
        sections,
    }))
}

pub async fn submit_job(
    State(state): State<AppState>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let body = body.map_err(|rejection| ApiError::body_rejection(rejection, REQUEST_BODY_BYTES))?;
    let request = parse_request(Frontend::Http, json_value(&body)?)?;
    let submission = state.scheduler.submit(JobOrigin::Http, request).await?;
    let (status, job) = match submission {
        Submission::Queued(job) => (StatusCode::ACCEPTED, job),
        Submission::Cached(job) | Submission::Coalesced(job) => (StatusCode::OK, job),
    };
    let location = format!("/api/jobs/{}", job.id);
    let view = summarize(&state, job).await?;
    Ok(match status {
        StatusCode::ACCEPTED => {
            (status, [(header::LOCATION, location)], Json(view)).into_response()
        }
        _ => (status, Json(view)).into_response(),
    })
}

#[derive(Debug, Deserialize)]
pub struct JobsQuery {
    before: Option<String>,
    limit: Option<String>,
}

#[derive(Serialize)]
pub struct JobPage {
    items: Vec<JobView>,
    next_before: Option<String>,
}

pub async fn list_jobs(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<JobsQuery>,
) -> ApiResult<Json<JobPage>> {
    let limit = bounded("limit", query.limit.as_deref(), 1..=200, 50)?;
    let before = query
        .before
        .as_deref()
        .map(|before| {
            before
                .parse::<i64>()
                .map_err(|_| EngineError::invalid("before", "not a job sequence"))
        })
        .transpose()?;
    let root = state.root().to_path_buf();
    let page = state
        .scheduler
        .store()
        .blocking(move |store| {
            let page = store.jobs(before, limit as usize)?;
            let items = page
                .items
                .iter()
                .map(|job| job_summary(&root, job))
                .collect();
            Ok(JobPage {
                items,
                next_before: page.next_cursor,
            })
        })
        .await
        .map_err(EngineError::internal)?;
    Ok(Json(page))
}

pub async fn get_job(
    State(state): State<AppState>,
    JobId(id): JobId,
) -> ApiResult<Json<JobDetail>> {
    let root = state.root().to_path_buf();
    let detail = state
        .scheduler
        .store()
        .blocking(move |store| Ok(store.get_job(id)?.map(|job| job_detail(&root, job))))
        .await
        .map_err(EngineError::internal)?
        .ok_or_else(|| EngineError::job_not_found(id))?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoParams {}

/// 200 with the unchanged job when it already ended; 202 while the owner
/// acts on the request.
pub async fn cancel_job(
    State(state): State<AppState>,
    JobId(id): JobId,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Response> {
    let body = body.map_err(|rejection| ApiError::body_rejection(rejection, REQUEST_BODY_BYTES))?;
    parse_json::<NoParams>(&body)?;
    let current = state
        .scheduler
        .store()
        .blocking(move |store| store.get_job(id))
        .await
        .map_err(EngineError::internal)?
        .ok_or_else(|| EngineError::job_not_found(id))?;
    if current.status.is_terminal() {
        return Ok((StatusCode::OK, Json(summarize(&state, current).await?)).into_response());
    }
    let job = state.scheduler.cancel(id).await?;
    Ok((StatusCode::ACCEPTED, Json(summarize(&state, job).await?)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    after: Option<String>,
    limit: Option<String>,
}

#[derive(Serialize)]
pub struct LogPage {
    items: Vec<LogRecord>,
    next_after: Option<String>,
}

pub async fn job_logs(
    State(state): State<AppState>,
    JobId(id): JobId,
    ApiQuery(query): ApiQuery<LogsQuery>,
) -> ApiResult<Json<LogPage>> {
    let limit = bounded("limit", query.limit.as_deref(), 1..=500, 200)?;
    let after = query
        .after
        .as_deref()
        .map(|after| {
            after
                .parse::<i64>()
                .map_err(|_| EngineError::invalid("after", "not a log cursor"))
        })
        .transpose()?;
    let page = state
        .scheduler
        .store()
        .blocking(move |store| {
            if store.get_job(id)?.is_none() {
                return Ok(None);
            }
            Ok(Some(store.logs(id, after, limit as usize)?))
        })
        .await
        .map_err(EngineError::internal)?
        .ok_or_else(|| EngineError::job_not_found(id))?;
    Ok(Json(LogPage {
        items: page.items,
        next_after: page.next_cursor,
    }))
}

#[derive(Debug, Deserialize)]
pub struct SourceQuery {
    path: String,
    start_line: Option<String>,
    end_line: Option<String>,
}

pub async fn source_page(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<SourceQuery>,
) -> ApiResult<Json<SourcePage>> {
    let line = |field, value: Option<&str>| {
        value
            .map(|value| {
                value
                    .parse::<u64>()
                    .map_err(|_| EngineError::invalid(field, "not a line number"))
            })
            .transpose()
    };
    let start = line("start_line", query.start_line.as_deref())?;
    let end = line("end_line", query.end_line.as_deref())?;
    let root = state.root().to_path_buf();
    let page = tokio::task::spawn_blocking(move || read_source(&root, &query.path, start, end))
        .await
        .map_err(EngineError::internal)??;
    Ok(Json(page))
}

/// Writes, then announces the new revision and refreshes what depends on it.
pub async fn source_write(
    State(state): State<AppState>,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<Json<SourceWriteResult>> {
    let body = body.map_err(|rejection| ApiError::body_rejection(rejection, SOURCE_BODY_BYTES))?;
    let write: SourceWrite = parse_json(&body)?;
    let root = state.root().to_path_buf();
    let python = state.scheduler.python().to_path_buf();
    let store = state.scheduler.store().clone();
    let result = tokio::task::spawn_blocking(move || {
        let index = store.newest_discover().map_err(EngineError::internal)?;
        write_source(&root, &python, &write, index.as_ref())
    })
    .await
    .map_err(EngineError::internal)??;
    let is_spec = result.path == SPEC_FILE;
    if is_spec || files::has_extension(&result.path, &["py"]) {
        state.file_changed(&result.path, Some(result.revision.clone()));
        state.request_reindex();
    }
    if is_spec {
        let state = state.clone();
        tokio::spawn(async move { state.publish_sections(&SPEC_SECTIONS).await });
    }
    Ok(Json(result))
}

/// A summary with artifact URLs. Stats files off the async runtime.
async fn summarize(state: &AppState, job: JobRecord) -> Result<JobView, EngineError> {
    let root = state.root().to_path_buf();
    tokio::task::spawn_blocking(move || job_summary(&root, &job))
        .await
        .map_err(EngineError::internal)
}

fn bounded(
    field: &str,
    value: Option<&str>,
    range: std::ops::RangeInclusive<u32>,
    default: u32,
) -> Result<u32, EngineError> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .parse::<u32>()
        .ok()
        .filter(|value| range.contains(value))
        .ok_or_else(|| {
            EngineError::invalid(
                field,
                format!("must be between {} and {}", range.start(), range.end()),
            )
        })
}
