//! The MCP server over stdio (OPS §1.7): ten coarse tools with bounded
//! output. Only malformed JSON-RPC and unknown tools are protocol errors;
//! everything that fails inside a tool is an `isError` result.

mod bound;
mod schema;
mod tools;

use crate::Scheduler;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader},
    sync::mpsc,
};

const PROTOCOL_VERSION: &str = "2025-06-18";

/// Serves until stdin ends or a signal arrives, then cancels this engine's
/// jobs and gives up its lease.
pub async fn run_mcp(scheduler: Scheduler) -> Result<()> {
    let served = tokio::select! {
        served = serve(&scheduler, tokio::io::stdin(), tokio::io::stdout()) => served,
        _ = crate::shutdown_signal() => Ok(()),
    };
    scheduler.shutdown().await;
    served
}

/// Answers every message on its own task: a job tool waiting for its job
/// must not hold up `job_status`, a cancel or another tool. Once the input
/// ends, the answers still in flight are written before returning.
async fn serve(
    scheduler: &Scheduler,
    input: impl AsyncRead + Unpin,
    mut output: impl AsyncWrite + Unpin,
) -> Result<()> {
    let (sender, mut outbox) = mpsc::unbounded_channel::<Value>();
    // Dropped at the end of input, so `outbox` closes after the last answer.
    let mut sender = Some(sender);
    let mut lines = BufReader::new(input).lines();
    loop {
        tokio::select! {
            line = lines.next_line(), if sender.is_some() => match (line?, &sender) {
                (Some(line), Some(responses)) => dispatch(scheduler, responses, &line),
                _ => sender = None,
            },
            response = outbox.recv() => {
                let Some(response) = response else {
                    return Ok(());
                };
                let mut line = serde_json::to_vec(&response)?;
                line.push(b'\n');
                output.write_all(&line).await?;
                output.flush().await?;
            }
        }
    }
}

fn dispatch(scheduler: &Scheduler, responses: &mpsc::UnboundedSender<Value>, line: &str) {
    if line.trim().is_empty() {
        return;
    }
    let message = match serde_json::from_str::<Value>(line) {
        Ok(message) => message,
        Err(error) => {
            let _ = responses.send(rpc_error(
                Value::Null,
                -32700,
                &format!("parse error: {error}"),
            ));
            return;
        }
    };
    let (scheduler, responses) = (scheduler.clone(), responses.clone());
    tokio::spawn(async move {
        if let Some(response) = handle(&scheduler, message).await {
            let _ = responses.send(response);
        }
    });
}

/// Answers one JSON-RPC message; notifications (no `id`) get no answer.
async fn handle(scheduler: &Scheduler, message: Value) -> Option<Value> {
    if !message.is_object() {
        return Some(rpc_error(
            Value::Null,
            -32600,
            "invalid request: not an object",
        ));
    }
    let id = message.get("id").cloned()?;
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Some(rpc_error(id, -32600, "invalid request: no method"));
    };
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
