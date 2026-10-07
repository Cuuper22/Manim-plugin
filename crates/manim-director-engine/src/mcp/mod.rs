//! The MCP server over stdio (OPS §1.7): ten coarse tools with bounded
//! output. Only malformed JSON-RPC and unknown tools are protocol errors;
//! everything that fails inside a tool is an `isError` result.

mod bound;
mod schema;
mod tools;

use crate::Scheduler;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL_VERSION: &str = "2025-06-18";

pub async fn run_mcp(scheduler: Scheduler) -> Result<()> {
    let served = serve_stdio(&scheduler).await;
    scheduler.shutdown().await;
    served
}

async fn serve_stdio(scheduler: &Scheduler) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(scheduler, message).await,
            Err(error) => Some(rpc_error(
                Value::Null,
                -32700,
                &format!("parse error: {error}"),
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

/// Answers one JSON-RPC message; notifications (no `id`) get no answer.
async fn handle(scheduler: &Scheduler, message: Value) -> Option<Value> {
    let id = message.get("id").cloned()?;
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "manim-director", "version": env!("CARGO_PKG_VERSION")},
            "instructions": "Start with inspect. Job tools wait up to wait_seconds (default 20) and list artifact paths you can open; follow longer jobs with job_status."
        }),
        "ping" => json!({}),
        "tools/list" => schema::tool_list(),
        "resources/list" => json!({"resources": []}),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(rpc_error(id, -32602, "tools/call needs a tool name"));
            };
            if !schema::TOOLS.contains(&name) {
                return Some(rpc_error(id, -32602, &format!("unknown tool: {name}")));
            }
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            tools::call(scheduler, name, arguments).await
        }
        _ => {
            return Some(rpc_error(
                id,
                -32601,
                &format!("method not found: {method}"),
            ))
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests;
