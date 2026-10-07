//! Running the ten tools. Tool failures are `isError` results carrying the
//! engine's `ErrorBody`; a job that fails or is cancelled is one too.

use super::bound::{bound, size};
use crate::{init_project, inspect, parse_request, Frontend, Scheduler};
use manim_director_core::{
    parse_value, summary as verdicts, Artifact, CursorPage, EngineError, JobOrigin, JobRecord,
    JobStatus, JobSummary, LogRecord, NoParams, Operation, OperationRequest, OperationResult,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Map, Value};
use std::{fmt::Write as _, time::Duration};
use uuid::Uuid;

const MAX_STRUCTURED_BYTES: usize = 48 * 1024;
const MAX_INSPECT_BYTES: usize = 32 * 1024;
/// What a job's status, result and error may use before events fill the rest.
const MAX_JOB_BYTES: usize = 32 * 1024;
const DEFAULT_WAIT_SECONDS: u64 = 20;
const MAX_WAIT_SECONDS: u64 = 50;
const MAX_LISTED_ARTIFACTS: usize = 64;
const MAX_LISTED_FINDINGS: usize = 5;

/// Runs a tool from the catalog; every other name is refused by the caller.
/// Dedicated job tools are named after their operation.
pub(super) async fn call(scheduler: &Scheduler, name: &str, arguments: Value) -> Value {
    let outcome = match name {
        "init" => init(scheduler, arguments).await,
        "inspect" => inspect_tool(scheduler, arguments).await,
        "job_status" => job_status(scheduler, arguments).await,
        "submit" => {
            submit(scheduler, arguments, |params| {
                parse_request(Frontend::McpSubmit, params)
            })
            .await
        }
        tool => match tool.parse::<Operation>() {
            Ok(operation) => {
                submit(scheduler, arguments, |params| {
                    OperationRequest::from_params(operation, params)
                })
                .await
            }
            Err(error) => Err(EngineError::internal(error)),
        },
    };
    outcome.unwrap_or_else(|error| {
        let mut structured = json!({ "error": error.body() });
        bound(&mut structured, MAX_STRUCTURED_BYTES);
        response(error.to_string(), structured, true)
    })
}

async fn init(scheduler: &Scheduler, arguments: Value) -> Result<Value, EngineError> {
    let OperationRequest::Init(params) = OperationRequest::from_params(Operation::Init, arguments)?
    else {
        return Err(EngineError::internal("init parsed as another operation"));
    };
    let result = init_project(scheduler.bridge_config(), scheduler.root(), params).await?;
    let mut text = format!(
        "init ({}) wrote {} in {}.",
        result.mode, result.scene.name, result.scene.file
    );
    list_paths(
        &mut text,
        &absolute_paths(scheduler, &result.artifacts),
        result.artifacts.len(),
    );
    let mut structured = serde_json::to_value(&result).map_err(EngineError::internal)?;
    bound(&mut structured, MAX_STRUCTURED_BYTES);
    Ok(response(text, structured, false))
}

async fn inspect_tool(scheduler: &Scheduler, arguments: Value) -> Result<Value, EngineError> {
    strict::<NoParams>(arguments)?;
    let summary = inspect(scheduler).await?;
    let mut text = format!(
        "{}: {} scenes, {} profiles (default {}).",
        summary.name,
        summary.scenes.len(),
        summary.profiles.len(),
        summary.default_profile
    );
    for scene in summary.scenes.iter().take(20) {
        let _ = write!(
            text,
            "\n{} — {} beats, {} sections",
            scene.scene_id, scene.beats, scene.sections
        );
    }
    let mut structured = serde_json::to_value(&summary).map_err(EngineError::internal)?;
    bound(&mut structured, MAX_INSPECT_BYTES);
    Ok(response(text, structured, false))
}

/// Submits the request built from `arguments` minus `wait_seconds`, then
/// waits for the job up to that long.
async fn submit(
    scheduler: &Scheduler,
    arguments: Value,
    build: impl FnOnce(Value) -> Result<OperationRequest, EngineError>,
) -> Result<Value, EngineError> {
    let (wait, params) = take_wait(arguments)?;
    let request = build(params)?;
    let job = scheduler.submit(JobOrigin::Mcp, request).await?.into_job();
    let job = settle(scheduler, job, wait).await?;
    Ok(job_response(scheduler, &job, None))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobStatusArguments {
    job_id: Uuid,
    #[serde(default)]
    cancel: bool,
    cursor: Option<String>,
    limit: Option<usize>,
    wait_seconds: Option<u64>,
}

async fn job_status(scheduler: &Scheduler, arguments: Value) -> Result<Value, EngineError> {
    let arguments: JobStatusArguments = strict(arguments)?;
    let wait = wait_duration(arguments.wait_seconds)?;
    let limit = arguments.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err(EngineError::invalid("limit", "must be between 1 and 100"));
    }
    let cursor = arguments
        .cursor
        .as_deref()
        .map(str::parse::<i64>)
        .transpose()
        .map_err(|_| EngineError::invalid("cursor", "must be a next_cursor value"))?;
    let id = arguments.job_id;
    let job = match arguments.cancel {
        true => scheduler.cancel(id).await?,
        false => scheduler
            .store()
            .blocking(move |store| store.get_job(id))
            .await
            .map_err(EngineError::internal)?
            .ok_or_else(|| EngineError::job_not_found(id))?,
    };
    let job = settle(scheduler, job, wait).await?;
    let page = scheduler
        .store()
        .blocking(move |store| store.logs(id, cursor, limit))
        .await
        .map_err(EngineError::internal)?;
    let events = Events {
        page,
        cursor: arguments.cursor,
    };
    Ok(job_response(scheduler, &job, Some(events)))
}

struct Events {
    page: CursorPage<LogRecord>,
    /// The cursor the caller passed, returned again when nothing is new.
    cursor: Option<String>,
}

/// Waits until the job is terminal or `wait` passes, whichever is first.
async fn settle(
    scheduler: &Scheduler,
    job: JobRecord,
    wait: Duration,
) -> Result<JobRecord, EngineError> {
    if job.status.is_terminal() || wait.is_zero() {
        return Ok(job);
    }
    match tokio::time::timeout(wait, scheduler.wait(job.id)).await {
        Ok(finished) => finished,
        Err(_) => {
            let id = job.id;
            Ok(scheduler
                .store()
                .blocking(move |store| store.get_job(id))
                .await
                .map_err(EngineError::internal)?
                .unwrap_or(job))
        }
    }
}

/// `{job, result, error, paths}` (plus `events`, `next_cursor` for
/// `job_status`); the text repeats the status, verdict and paths. `paths`
/// holds every artifact's absolute path, since some hosts show the model only
/// the structured content and artifact paths are project-relative.
fn job_response(scheduler: &Scheduler, job: &JobRecord, events: Option<Events>) -> Value {
    let artifacts = job
        .result
        .as_ref()
        .map_or(&[][..], OperationResult::artifacts);
    let paths = absolute_paths(scheduler, artifacts);
    let mut structured = json!({
        "job": JobSummary::from(job),
        "result": job.result,
        "error": job.error,
    });
    bound(&mut structured, MAX_JOB_BYTES);
    if let Value::Object(fields) = &mut structured {
        fields.insert("paths".into(), json!(paths));
        if let Some(events) = events {
            let budget = MAX_STRUCTURED_BYTES.saturating_sub(size(&Value::Object(fields.clone())));
            let (records, next_cursor) = fit_events(events, budget);
            fields.insert("events".into(), Value::Array(records));
            fields.insert("next_cursor".into(), json!(next_cursor));
        }
    }
    let mut text = summary(job);
    list_paths(&mut text, &paths, artifacts.len());
    let failed = matches!(job.status, JobStatus::Failed | JobStatus::Cancelled);
    response(text, structured, failed)
}

/// As many records as fit `budget` bytes; the cursor continues after the
/// last one returned.
fn fit_events(events: Events, budget: usize) -> (Vec<Value>, Option<String>) {
    let mut used = 64;
    let mut records = Vec::new();
    let mut next_cursor = events.cursor;
    for record in events.page.items {
        let value = serde_json::to_value(&record).unwrap_or(Value::Null);
        used += size(&value) + 1;
        if used > budget {
            break;
        }
        next_cursor = Some(record.cursor);
        records.push(value);
    }
    (records, next_cursor)
}

fn summary(job: &JobRecord) -> String {
    let mut text = format!("{} {} {}", job.operation, job.id, job.status);
    if job.cached {
        text.push_str(" (cached)");
    }
    match (&job.error, &job.progress) {
        (Some(error), _) => {
            let _ = write!(text, ": {} — {}", error.code, error.message);
        }
        (None, Some(progress)) if !job.status.is_terminal() => {
            let _ = write!(text, " ({}", progress.phase);
            if let Some(message) = &progress.message {
                let _ = write!(text, ": {message}");
            }
            text.push(')');
        }
        _ => {}
    }
    if !job.status.is_terminal() {
        text.push_str("; follow it with job_status.");
    }
    let error_findings = job.error.as_ref().map(verdicts::error_findings);
    let findings = match &job.result {
        Some(result) => verdicts::findings(result),
        None => error_findings.as_deref().unwrap_or_default(),
    };
    let verdict = job.result.iter().flat_map(verdicts::verdict);
    let listed = findings.iter().take(MAX_LISTED_FINDINGS);
    for line in verdict.chain(listed.flat_map(verdicts::finding_lines)) {
        let _ = write!(text, "\n{line}");
    }
    if findings.len() > MAX_LISTED_FINDINGS {
        let more = findings.len() - MAX_LISTED_FINDINGS;
        let _ = write!(text, "\n… {more} more findings");
    }
    text
}

/// Absolute paths of the first `MAX_LISTED_ARTIFACTS` artifacts.
fn absolute_paths(scheduler: &Scheduler, artifacts: &[Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .take(MAX_LISTED_ARTIFACTS)
        .map(|artifact| scheduler.root().join(&artifact.path).display().to_string())
        .collect()
}

fn list_paths(text: &mut String, paths: &[String], total: usize) {
    for path in paths {
        let _ = write!(text, "\n{path}");
    }
    if total > paths.len() {
        let _ = write!(text, "\n… {} more", total - paths.len());
    }
}

fn response(text: String, structured: Value, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": is_error,
    })
}

/// Splits `wait_seconds` off a job tool's arguments.
fn take_wait(arguments: Value) -> Result<(Duration, Value), EngineError> {
    let Value::Object(mut fields) = arguments else {
        return Err(EngineError::invalid("arguments", "must be an object"));
    };
    let wait = match fields.remove("wait_seconds") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or_else(invalid_wait)?),
    };
    Ok((wait_duration(wait)?, Value::Object(fields)))
}

fn wait_duration(seconds: Option<u64>) -> Result<Duration, EngineError> {
    let seconds = seconds.unwrap_or(DEFAULT_WAIT_SECONDS);
    if seconds > MAX_WAIT_SECONDS {
        return Err(invalid_wait());
    }
    Ok(Duration::from_secs(seconds))
}

fn invalid_wait() -> EngineError {
    EngineError::invalid(
        "wait_seconds",
        format!("must be an integer between 0 and {MAX_WAIT_SECONDS}"),
    )
}

fn strict<T: DeserializeOwned>(arguments: Value) -> Result<T, EngineError> {
    match arguments {
        Value::Null => parse_value(Value::Object(Map::new())),
        arguments => parse_value(arguments),
    }
}
