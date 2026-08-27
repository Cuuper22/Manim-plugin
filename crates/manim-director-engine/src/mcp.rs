use crate::{apply_source_mutation, scaffold_params, Scheduler, SourceMutation};
use anyhow::Result;
use manim_director_core::{
    CursorPage, JobRecord, JobRequest, JobSummary, LogRecord, Operation, ProjectInventory,
    SPEC_FILE,
};
use serde_json::{json, Map, Value};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

const MAX_SPEC_RESOURCE_BYTES: usize = 128 * 1024;

pub async fn run_mcp(project_root: PathBuf, scheduler: Scheduler) -> Result<()> {
    let project_root = project_root.canonicalize()?;
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_request(&project_root, &scheduler, request).await,
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

async fn handle_request(root: &Path, scheduler: &Scheduler, request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if id.is_none() {
        return None;
    }
    let id = id.unwrap();
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
        "tools/call" => call_tool(root, scheduler, params).await,
        "resources/list" => Ok(json!({"resources": resources(root, scheduler)})),
        "resources/read" => read_resource(root, scheduler, params),
        _ => Err((-32601, "method not found".to_owned(), None)),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err((code, message, data)) => rpc_error(id, code, &message, data),
    })
}

type RpcFailure = (i64, String, Option<Value>);

async fn call_tool(
    root: &Path,
    scheduler: &Scheduler,
    params: Value,
) -> std::result::Result<Value, RpcFailure> {
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
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    root.file_name()
                        .and_then(|v| v.to_str())
                        .unwrap_or("manim-project")
                });
            let force = arguments
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let seed = arguments
                .get("seed")
                .and_then(Value::as_u64)
                .map(u32::try_from)
                .transpose()
                .map_err(|_| invalid("seed must be between 0 and 2147483647"))?;
            let scaffold = scaffold_params(root, name, force, seed).map_err(invalid)?;
            submit_tool(root, scheduler, Operation::Scaffold, scaffold).await
        }
        "project_inspect" => {
            let inventory = ProjectInventory::scan(root).map_err(internal)?;
            Ok(tool_result(
                format!(
                    "{}: {} scenes, {} assets; manim://project/spec",
                    inventory.spec.project.name,
                    inventory.source_files.len(),
                    inventory.asset_files.len()
                ),
                json!({"name":inventory.spec.project.name,"source_count":inventory.source_files.len(),"asset_count":inventory.asset_files.len(),"resource":"manim://project/spec"}),
            ))
        }
        "project_apply" => {
            if let Some(paths) = arguments.get("ingest") {
                if !paths.is_array() || paths.as_array().is_some_and(Vec::is_empty) {
                    return Err(invalid("ingest must be a non-empty path array"));
                }
                return submit_tool(root, scheduler, Operation::Ingest, json!({"sources":paths}))
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
        "doctor" => submit_tool(root, scheduler, Operation::Doctor, arguments).await,
        "render" => submit_tool(root, scheduler, Operation::Render, arguments).await,
        "preview" => submit_tool(root, scheduler, Operation::Preview, arguments).await,
        "qa" => submit_tool(root, scheduler, Operation::Qa, arguments).await,
        "debug" => submit_tool(root, scheduler, Operation::Debug, arguments).await,
        "export" => submit_tool(root, scheduler, Operation::Export, arguments).await,
        "job_status" => {
            let id = parse_job_id(&arguments)?;
            let job = scheduler
                .store()
                .get_job(id)
                .map_err(internal)?
                .ok_or_else(|| (-32004, format!("job {id} not found"), None))?;
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
                .logs(Some(id), cursor, limit)
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
                json!({"job_id":id,"status":job.status,"operation":job.operation,"events":logs.items,"next_cursor":logs.next_cursor,"resource":resource,"logs_resource":format!("manim://logs/{id}?cursor=0&limit=100")}),
            ))
        }
        _ => Err((-32602, format!("unknown tool: {name}"), None)),
    }
}

async fn submit_tool(
    root: &Path,
    scheduler: &Scheduler,
    operation: Operation,
    arguments: Value,
) -> std::result::Result<Value, RpcFailure> {
    let job = scheduler
        .submit(
            root,
            JobRequest {
                operation,
                params: arguments,
                priority: 0,
            },
        )
        .await
        .map_err(internal)?;
    let resource = format!("manim://jobs/{}", job.id);
    let verb = if job.cached { "cached" } else { "queued" };
    Ok(tool_result(
        format!("{verb} {} {}; {resource}", job.operation, job.id),
        json!({"job_id":job.id,"status":job.status,"cached":job.cached,"resource":resource}),
    ))
}

fn resources(root: &Path, scheduler: &Scheduler) -> Vec<Value> {
    let mut values = vec![
        json!({"uri":"manim://project/spec","name":"Project spec","mimeType":"text/yaml"}),
        json!({"uri":"manim://jobs/recent","name":"Recent jobs","mimeType":"application/json"}),
    ];
    if let Ok(page) = scheduler.store().jobs(None, 20) {
        values.extend(page.items.into_iter().map(|job| json!({"uri":format!("manim://jobs/{}",job.id),"name":format!("{} {}",job.operation,job.id),"mimeType":"application/json"})));
    }
    let _ = root;
    values
}

fn read_resource(
    root: &Path,
    scheduler: &Scheduler,
    params: Value,
) -> std::result::Result<Value, RpcFailure> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("missing resource uri"))?;
    let (mime, text) = match uri {
        "manim://project/spec" => {
            let text = read_spec_resource(&root.join(SPEC_FILE))?;
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
                .ok_or_else(|| (-32004, format!("job {id} not found"), None))?;
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
                .logs(Some(id), cursor, limit)
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
        page.next_cursor = page.items.last().map(|item| item.cursor.to_string());
    }
    page
}

fn compact_job_resource(job: &JobRecord) -> Value {
    let compact = |value: Option<&Value>, max_bytes: usize| -> Value {
        let Some(value) = value else {
            return Value::Null;
        };
        let encoded = serde_json::to_vec(value).unwrap_or_default();
        if encoded.len() <= max_bytes {
            value.clone()
        } else {
            let mut paths = Vec::new();
            collect_path_strings(value, &mut paths);
            paths.sort();
            paths.dedup();
            paths.truncate(100);
            json!({"truncated":true,"original_bytes":encoded.len(),"artifact_paths":paths})
        }
    };
    let error = job
        .error
        .as_ref()
        .and_then(|value| serde_json::to_value(value).ok());
    json!({
        "job": JobSummary::from(job),
        "params": compact(Some(&job.params), 16 * 1024),
        "result": compact(job.result.as_ref(), 64 * 1024),
        "error": compact(error.as_ref(), 16 * 1024),
        "logs": format!("manim://logs/{}?cursor=0&limit=100", job.id),
    })
}

fn collect_path_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) if value.len() <= 2048 && value.contains('.') => {
            output.push(value.clone())
        }
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_path_strings(value, output)),
        Value::Object(values) => values
            .values()
            .for_each(|value| collect_path_strings(value, output)),
        _ => {}
    }
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
            "Scaffold this project.",
            json!({"type":"object","properties":{"name":{"type":"string"},"force":{"type":"boolean","default":false},"seed":{"type":"integer","minimum":0,"maximum":2147483647}}}),
        ),
        tool(
            "project_inspect",
            "Read compact project state.",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "project_apply",
            "Atomically edit project source or ingest source paths.",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":0},"replacement":{"type":"string"},"merge_patch":{"type":"object"},"expected_revision":{"type":"string"},"ingest":{"type":"array","minItems":1,"items":{"type":"string"}}},"anyOf":[{"required":["path"]},{"required":["ingest"]}]}),
        ),
        tool(
            "doctor",
            "Check runtime dependencies.",
            json!({"type":"object","properties":{}}),
        ),
        tool("render", "Queue a production render.", job_schema()),
        tool("preview", "Queue a fast preview.", job_schema()),
        tool("qa", "Inspect rendered frames and reports.", job_schema()),
        tool("debug", "Diagnose a failed or broken scene.", job_schema()),
        tool(
            "export",
            "Package source and deliverables.",
            json!({"type":"object","properties":{"format":{"type":"string"},"output":{"type":"string"},"job_id":{"type":"string"}},"additionalProperties":true}),
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
fn job_schema() -> Value {
    json!({"type":"object","properties":{"scene":{"type":"string"},"profile":{"type":"string"},"section":{"type":"string"},"job_id":{"type":"string"},"source":{"type":"string"},"images":{"type":"array","items":{"type":"string"}}},"additionalProperties":true})
}

fn rpc_error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code":code,"message":message});
    if let Some(data) = data {
        error.as_object_mut().unwrap().insert("data".into(), data);
    }
    json!({"jsonrpc":"2.0","id":id,"error":error})
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
    use chrono::Utc;
    use manim_director_core::{JobStatus, Operation};

    #[test]
    fn log_pages_obey_serialized_byte_budget() {
        let id = Uuid::new_v4();
        let page = CursorPage {
            items: (1..=20)
                .map(|cursor| LogRecord {
                    cursor,
                    job_id: id,
                    timestamp: Utc::now(),
                    level: "info".into(),
                    event: "chunk".into(),
                    data: json!({"message":"x".repeat(8 * 1024)}),
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
    fn job_resource_compacts_large_results() {
        let job = JobRecord {
            id: Uuid::new_v4(),
            sequence: 1,
            project_root: "/project".into(),
            operation: Operation::Render,
            status: JobStatus::Succeeded,
            params: json!({}),
            fingerprint: None,
            result: Some(json!({"blob":"x".repeat(256 * 1024),"path":"output/final.mp4"})),
            error: None,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            finished_at: Some(Utc::now()),
            cached: false,
        };
        let value = compact_job_resource(&job);
        assert!(serde_json::to_vec(&value).unwrap().len() < 96 * 1024);
        assert_eq!(value["result"]["truncated"], true);
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
}
