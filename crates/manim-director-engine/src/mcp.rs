use crate::{
    apply_source_mutation, init_project, inspect, parse_params, BridgeConfig, Scheduler,
    SourceMutation,
};
use anyhow::Result;
use manim_director_core::{
    CursorPage, EngineError, InitParams, JobOrigin, JobRecord, JobSummary, LogRecord, Operation,
    SPEC_FILE,
};
use serde_json::{json, Map, Value};
use std::{path::Path, str::FromStr};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

const MAX_SPEC_RESOURCE_BYTES: usize = 128 * 1024;

pub async fn run_mcp(scheduler: Scheduler) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_request(&scheduler, request).await,
            Err(error) => Some(rpc_error(
                Value::Null,
                -32700,
                "parse error",
                Some(json!({"detail": error.to_string()})),
            )),
        };
        if let Some(response) = response {
            stdout
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

async fn handle_request(scheduler: &Scheduler, request: Value) -> Option<Value> {
    let id = request.get("id").cloned()?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = request
        .get("params")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {"listChanged": false}, "resources": {"subscribe": false, "listChanged": false}},
            "serverInfo": {"name": "manim-director", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Use resource URIs for detail; tool responses stay compact."
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_contracts()})),
        "tools/call" => call_tool(scheduler, params).await,
        "resources/list" => Ok(json!({"resources": resources(scheduler)})),
        "resources/read" => read_resource(scheduler, params),
        _ => Err((-32601, "method not found".to_owned(), None)),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err((code, message, data)) => rpc_error(id, code, &message, data),
    })
}

type RpcFailure = (i64, String, Option<Value>);

async fn call_tool(scheduler: &Scheduler, params: Value) -> std::result::Result<Value, RpcFailure> {
    let root = scheduler.root();
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing tool name"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match name {
        "project_init" => {
            let params: InitParams = serde_json::from_value(arguments)
                .map_err(|error| invalid(format!("invalid init arguments: {error}")))?;
            let result = init_project(&BridgeConfig::default(), root, params)
                .await
                .map_err(engine_failure)?;
            Ok(tool_result(
                format!("created {} in {}", result.scene.name, result.scene.file),
                serde_json::to_value(result).map_err(internal)?,
            ))
        }
        "project_inspect" => {
            let summary = inspect(scheduler).await.map_err(engine_failure)?;
            Ok(tool_result(
                format!(
                    "{}: {} scenes, {} profiles",
                    summary.name,
                    summary.scenes.len(),
                    summary.profiles.len()
                ),
                serde_json::to_value(summary).map_err(internal)?,
            ))
        }
        "project_apply" => {
            if let Some(paths) = arguments.get("ingest") {
                let sources: Vec<Value> = paths
                    .as_array()
                    .filter(|paths| !paths.is_empty())
                    .ok_or_else(|| invalid("ingest must be a non-empty path array"))?
                    .iter()
                    .map(|path| json!({"path": path}))
                    .collect();
                return submit_tool(scheduler, Operation::Ingest, json!({"sources": sources}))
                    .await;
            }
            let mutation: SourceMutation = serde_json::from_value(arguments)
                .map_err(|error| invalid(format!("invalid edit: {error}")))?;
            let result = apply_source_mutation(root, mutation)
                .await
                .map_err(internal)?;
            Ok(tool_result(
                format!(
                    "updated {} @ {}; undo {}",
                    result.path,
                    &result.revision[..12],
                    result.undo_path.as_deref().unwrap_or("new file")
                ),
                serde_json::to_value(result).map_err(internal)?,
            ))
        }
        "doctor" => submit_tool(scheduler, Operation::Doctor, arguments).await,
        "render" => submit_tool(scheduler, Operation::Render, arguments).await,
        "qa" => submit_tool(scheduler, Operation::Qa, arguments).await,
        "diagnose" => submit_tool(scheduler, Operation::Diagnose, arguments).await,
        "export" => submit_tool(scheduler, Operation::Export, arguments).await,
        "job_status" => {
            let id = parse_job_id(&arguments)?;
            let job = scheduler
                .store()
                .get_job(id)
                .map_err(internal)?
                .ok_or_else(|| engine_failure(EngineError::job_not_found(id)))?;
            let cursor = arguments
                .get("cursor")
                .and_then(Value::as_str)
                .map(str::parse::<i64>)
                .transpose()
                .map_err(|_| invalid("cursor must be an integer string"))?;
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 100) as usize;
            let logs = scheduler
                .store()
                .logs(id, cursor, limit)
                .map_err(internal)?;
            let logs = bounded_log_page(logs, 56 * 1024);
            let resource = format!("manim://jobs/{id}");
            Ok(tool_result(
                format!(
                    "{} {} {id}; {} events; {resource}",
                    job.status,
                    job.operation,
                    logs.items.len()
                ),
                json!({"job": JobSummary::from(&job),"error":job.error,"events":logs.items,"next_cursor":logs.next_cursor,"resource":resource}),
            ))
        }
        _ => Err((-32602, format!("unknown tool: {name}"), None)),
    }
}

async fn submit_tool(
    scheduler: &Scheduler,
    operation: Operation,
    arguments: Value,
) -> std::result::Result<Value, RpcFailure> {
    let request = parse_params(operation, arguments).map_err(engine_failure)?;
    let job = scheduler
        .submit(JobOrigin::Mcp, request)
        .await
        .map_err(engine_failure)?
        .into_job();
    let resource = format!("manim://jobs/{}", job.id);
    let verb = if job.cached { "cached" } else { "queued" };
    Ok(tool_result(
        format!("{verb} {} {}; {resource}", job.operation, job.id),
        json!({"job_id":job.id,"status":job.status,"cached":job.cached,"resource":resource}),
    ))
}

fn resources(scheduler: &Scheduler) -> Vec<Value> {
    let mut values = vec![
        json!({"uri":"manim://project/spec","name":"Project spec","mimeType":"text/yaml"}),
        json!({"uri":"manim://jobs/recent","name":"Recent jobs","mimeType":"application/json"}),
    ];
    if let Ok(page) = scheduler.store().jobs(None, 20) {
        values.extend(page.items.into_iter().map(|job| json!({"uri":format!("manim://jobs/{}",job.id),"name":format!("{} {}",job.operation,job.id),"mimeType":"application/json"})));
    }
    values
}

fn read_resource(scheduler: &Scheduler, params: Value) -> std::result::Result<Value, RpcFailure> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing resource uri"))?;
    let (mime, text) = match uri {
        "manim://project/spec" => {
            let text = read_spec_resource(&scheduler.root().join(SPEC_FILE))?;
            ("text/yaml", text)
        }
        "manim://jobs/recent" => {
            let jobs = scheduler.store().jobs(None, 50).map_err(internal)?;
            let summaries = jobs.items.iter().map(JobSummary::from).collect::<Vec<_>>();
            (
                "application/json",
                serde_json::to_string(&json!({
                    "items": summaries,
                    "next_cursor": jobs.next_cursor,
                }))
                .map_err(internal)?,
            )
        }
        value if value.starts_with("manim://jobs/") => {
            let id = Uuid::from_str(value.trim_start_matches("manim://jobs/"))
                .map_err(|_| invalid("invalid job resource uri"))?;
            let job = scheduler
                .store()
                .get_job(id)
                .map_err(internal)?
                .ok_or_else(|| engine_failure(EngineError::job_not_found(id)))?;
            (
                "application/json",
                serde_json::to_string(&compact_job_resource(&job)).map_err(internal)?,
            )
        }
        value if value.starts_with("manim://logs/") => {
            let tail = value.trim_start_matches("manim://logs/");
            let (id_text, query) = tail.split_once('?').unwrap_or((tail, ""));
            let id = Uuid::from_str(id_text).map_err(|_| invalid("invalid log resource uri"))?;
            let mut cursor = None;
            let mut limit = 100_usize;
            for pair in query.split('&').filter(|pair| !pair.is_empty()) {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                match key {
                    "cursor" => {
                        cursor = Some(
                            value
                                .parse::<i64>()
                                .map_err(|_| invalid("invalid log cursor"))?,
                        )
                    }
                    "limit" => {
                        limit = value
                            .parse::<usize>()
                            .map_err(|_| invalid("invalid log limit"))?
                            .clamp(1, 100)
                    }
                    _ => {}
                }
            }
            let logs = scheduler
                .store()
                .logs(id, cursor, limit)
                .map_err(internal)?;
            let logs = bounded_log_page(logs, 96 * 1024);
            (
                "application/json",
                serde_json::to_string(&logs).map_err(internal)?,
            )
        }
        _ => return Err((-32004, format!("resource not found: {uri}"), None)),
    };
    Ok(json!({"contents":[{"uri":uri,"mimeType":mime,"text":text}]}))
}

fn read_spec_resource(path: &Path) -> std::result::Result<String, RpcFailure> {
    let bytes = std::fs::read(path).map_err(internal)?;
    if bytes.len() > MAX_SPEC_RESOURCE_BYTES {
        return Err((
            -32005,
            "project spec exceeds the MCP resource byte limit".into(),
            Some(json!({
                "code": "resource_too_large",
                "bytes": bytes.len(),
                "max_bytes": MAX_SPEC_RESOURCE_BYTES,
            })),
        ));
    }
    String::from_utf8(bytes).map_err(|_| invalid("project spec must be UTF-8"))
}

fn bounded_log_page(mut page: CursorPage<LogRecord>, max_bytes: usize) -> CursorPage<LogRecord> {
    let mut used = 64_usize;
    let mut keep = 0_usize;
    for item in &page.items {
        let bytes = serde_json::to_vec(item)
            .map(|value| value.len())
            .unwrap_or(max_bytes);
        if used.saturating_add(bytes) > max_bytes {
            break;
        }
        used += bytes;
        keep += 1;
    }
    let trimmed = keep < page.items.len();
    page.items.truncate(keep);
    if trimmed || page.next_cursor.is_some() {
        page.next_cursor = page.items.last().map(|item| item.cursor.clone());
    }
    page
}

/// A job resource bounded in size: oversized results shrink to their artifact list.
fn compact_job_resource(job: &JobRecord) -> Value {
    let result = job.result.as_ref().map(|result| {
        let value = serde_json::to_value(result).unwrap_or(Value::Null);
        let bytes = serde_json::to_vec(&value).map_or(0, |encoded| encoded.len());
        if bytes <= 64 * 1024 {
            value
        } else {
            let paths: Vec<_> = result
                .artifacts()
                .iter()
                .take(100)
                .map(|artifact| artifact.path.clone())
                .collect();
            json!({"truncated": true, "original_bytes": bytes, "artifact_paths": paths})
        }
    });
    json!({
        "job": JobSummary::from(job),
        "request": job.request,
        "result": result,
        "error": job.error,
        "logs": format!("manim://logs/{}?cursor=0&limit=100", job.id),
    })
}

fn parse_job_id(arguments: &Value) -> std::result::Result<Uuid, RpcFailure> {
    let value = arguments
        .get("job_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing job_id"))?;
    Uuid::parse_str(value).map_err(|_| invalid("job_id must be a UUID"))
}

fn tool_result(text: String, structured: Value) -> Value {
    json!({"content":[{"type":"text","text":text}],"structuredContent":structured,"isError":false})
}

fn tool_contracts() -> Vec<Value> {
    vec![
        tool(
            "project_init",
            "Create a project from a template, or add a scene template to this one.",
            json!({"type":"object","properties":{"name":{"type":"string"},"template":{"type":"string"},"scene_template":{"type":"string"},"theme":{"type":"string"},"force":{"type":"boolean","default":false},"seed":{"type":"integer","minimum":0,"maximum":2147483647}}}),
        ),
        tool(
            "project_inspect",
            "Read compact project state.",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "project_apply",
            "Atomically edit project source or ingest absolute source paths.",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":0},"replacement":{"type":"string"},"merge_patch":{"type":"object"},"expected_revision":{"type":"string"},"ingest":{"type":"array","minItems":1,"items":{"type":"string"}}},"anyOf":[{"required":["path"]},{"required":["ingest"]}]}),
        ),
        tool(
            "doctor",
            "Check runtime dependencies.",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "render",
            "Render one scene to video.",
            json!({"type":"object","properties":{"scene":{"type":"string"},"file":{"type":"string"},"profile":{"type":"string"},"sections":{"type":"boolean"},"fresh":{"type":"boolean"}},"additionalProperties":false}),
        ),
        tool(
            "qa",
            "Check a rendered video or image for blank frames, contrast and safe area.",
            json!({"type":"object","properties":{"source":{"type":"object"},"scene":{"type":"string"},"profile":{"type":"string"},"frames":{"type":"integer","minimum":1,"maximum":40}},"additionalProperties":false}),
        ),
        tool(
            "diagnose",
            "Explain a failed job or a pasted traceback with file:line findings.",
            json!({"type":"object","properties":{"job_id":{"type":"string"},"text":{"type":"string"}},"additionalProperties":false}),
        ),
        tool(
            "export",
            "Bundle the project as a zip, or deliver a render as mp4, webm or gif.",
            json!({"type":"object","properties":{"format":{"type":"string","enum":["zip","mp4","webm","gif"]},"source":{"type":"object"},"scene":{"type":"string"},"profile":{"type":"string"},"output":{"type":"string"},"gif_fps":{"type":"integer"},"gif_width":{"type":"integer"}},"additionalProperties":false}),
        ),
        tool(
            "job_status",
            "Read job status and a bounded event page.",
            json!({"type":"object","properties":{"job_id":{"type":"string"},"cursor":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":100,"default":20}},"required":["job_id"]}),
        ),
    ]
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":schema})
}

fn rpc_error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code":code,"message":message});
    if let (Some(data), Some(object)) = (data, error.as_object_mut()) {
        object.insert("data".into(), data);
    }
    json!({"jsonrpc":"2.0","id":id,"error":error})
}

fn engine_failure(error: EngineError) -> RpcFailure {
    let code = if error.status() < 500 { -32602 } else { -32000 };
    (
        code,
        error.to_string(),
        serde_json::to_value(error.body()).ok(),
    )
}

fn invalid(message: impl ToString) -> RpcFailure {
    (-32602, message.to_string(), None)
}

fn internal(error: impl ToString) -> RpcFailure {
    (-32000, error.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::{LogLevel, LogStream, Timestamp};

    #[test]
    fn log_pages_obey_serialized_byte_budget() {
        let page = CursorPage {
            items: (1..=20)
                .map(|cursor| LogRecord {
                    cursor: cursor.to_string(),
                    timestamp: Timestamp::now(),
                    stream: LogStream::Stderr,
                    level: LogLevel::Info,
                    message: "x".repeat(8 * 1024),
                    data: None,
                })
                .collect(),
            next_cursor: None,
        };
        let bounded = bounded_log_page(page, 24 * 1024);
        assert!(serde_json::to_vec(&bounded).unwrap().len() <= 24 * 1024);
        assert!(bounded.next_cursor.is_some());
        assert!(bounded.items.len() < 20);
    }

    #[test]
    fn project_spec_resource_rejects_oversize_content() {
        let directory = tempfile::tempdir().unwrap();
        let spec = directory.path().join(SPEC_FILE);
        std::fs::write(&spec, vec![b'x'; MAX_SPEC_RESOURCE_BYTES + 1]).unwrap();
        let error = read_spec_resource(&spec).unwrap_err();
        assert_eq!(error.0, -32005);
        assert_eq!(error.2.unwrap()["code"], "resource_too_large");
    }

    #[test]
    fn engine_errors_keep_their_contract_code() {
        let (code, _, data) = engine_failure(EngineError::invalid("scene", "too long"));
        assert_eq!(code, -32602);
        assert_eq!(data.unwrap()["code"], "invalid_params");
    }

    #[test]
    fn job_resources_shrink_large_results_to_their_artifacts() {
        use crate::{NewJob, Store};
        use manim_director_core::{
            Artifact, ArtifactKind, DiagnoseParams, DiagnoseResult, DiagnoseTask, Finding, Limits,
            OperationRequest, OperationResult, Task,
        };
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path().join("state.db")).unwrap();
        let id = Uuid::new_v4();
        let request = OperationRequest::Diagnose(DiagnoseParams {
            job_id: None,
            text: Some("x".into()),
        });
        store
            .insert_job(&NewJob {
                id,
                origin: JobOrigin::Mcp,
                owner: Uuid::new_v4(),
                request: &request,
                task: &Task::Diagnose(DiagnoseTask { text: "x".into() }),
                limits: Limits {
                    timeout_seconds: 60,
                    memory_mb: None,
                },
                fingerprint: None,
                source_job_id: None,
                scene_class: None,
                scene_file: None,
                scene_revision: None,
                profile: None,
            })
            .unwrap();
        let result = OperationResult::Diagnose(DiagnoseResult {
            recognized: false,
            findings: vec![Finding::warning("unclassified", "x".repeat(256 * 1024))],
            artifacts: vec![Artifact {
                kind: ArtifactKind::File,
                path: "notes.txt".into(),
                label: None,
                bytes: 1,
                media: None,
            }],
        });
        store.set_running(id).unwrap();
        store.finish_success(id, &result, None).unwrap();
        let job = store.get_job(id).unwrap().unwrap();
        let value = compact_job_resource(&job);
        assert!(serde_json::to_vec(&value).unwrap().len() < 96 * 1024);
        assert_eq!(value["result"]["truncated"], true);
        assert_eq!(value["result"]["artifact_paths"], json!(["notes.txt"]));
    }
}
