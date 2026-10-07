//! Python bridge protocol v2 (OPS §2): one request line in, JSON Lines out.

use crate::{ErrorBody, Operation, ProgressPhase, Task};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Serialize)]
pub struct BridgeRequest<'a> {
    pub protocol: u32,
    pub request_id: &'a str,
    pub method: Operation,
    pub project_root: &'a Path,
    pub params: &'a Task,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeFrame {
    Ready(ReadyFrame),
    Progress(ProgressFrame),
    Log(LogFrame),
    Result(ResultFrame),
    Error(ErrorFrame),
}

impl RuntimeFrame {
    /// The request id a post-`ready` frame carries (`None` for `ready` and for
    /// an error raised before the request could be read).
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Ready(_) => None,
            Self::Progress(frame) => Some(&frame.request_id),
            Self::Log(frame) => Some(&frame.request_id),
            Self::Result(frame) => Some(&frame.request_id),
            Self::Error(frame) => frame.request_id.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadyFrame {
    pub protocol: u32,
    pub runtime_version: String,
    pub python: String,
    pub manim: Option<String>,
    #[serde(default)]
    pub preloaded: Vec<String>,
    #[serde(default)]
    pub preload_failed: Vec<PreloadFailure>,
    #[serde(default)]
    pub preload_ms: u64,
    pub catalog: Catalog,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PreloadFailure {
    pub module: String,
    pub message: String,
}

/// Static runtime data advertised in `ready`: theme tokens (ordered pairs) and
/// template names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub themes: Vec<CatalogTheme>,
    pub project_templates: Vec<String>,
    pub scene_templates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogTheme {
    pub name: String,
    pub tokens: Vec<(String, String)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProgressFrame {
    pub request_id: String,
    pub phase: ProgressPhase,
    pub current: u64,
    pub total: Option<u64>,
    pub scene_seconds: Option<f64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLogLevel {
    Info,
    Warning,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogFrame {
    pub request_id: String,
    pub level: RuntimeLogLevel,
    pub message: String,
}

/// Parsed into the operation's typed result by the engine.
#[derive(Debug, Clone, Deserialize)]
pub struct ResultFrame {
    pub request_id: String,
    pub result: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorFrame {
    pub request_id: Option<String>,
    pub error: ErrorBody,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DoctorTask;
    use serde_json::json;

    #[test]
    fn request_line_has_the_five_contract_fields() {
        let task = Task::Doctor(DoctorTask {});
        let request = BridgeRequest {
            protocol: PROTOCOL_VERSION,
            request_id: "7f1c",
            method: Operation::Doctor,
            project_root: Path::new("/p"),
            params: &task,
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({"protocol":2,"request_id":"7f1c","method":"doctor","project_root":"/p","params":{}})
        );
    }

    #[test]
    fn frames_parse_by_type() {
        let ready: RuntimeFrame = serde_json::from_value(json!({"type":"ready","protocol":2,"runtime_version":"2.0.0",
            "python":"3.12.3","manim":null,"preloaded":[],"preload_failed":[{"module":"moderngl","message":"missing"}],
            "preload_ms":3,"catalog":{"themes":[{"name":"midnight","tokens":[["background","#0B1020"]]}],
            "project_templates":["explainer"],"scene_templates":[]}}))
        .unwrap();
        let RuntimeFrame::Ready(ready) = ready else {
            panic!("expected ready")
        };
        assert_eq!(ready.catalog.themes[0].tokens[0].1, "#0B1020");

        let error: RuntimeFrame = serde_json::from_value(json!({"type":"error","request_id":null,
            "error":{"code":"invalid_request","message":"Request is not valid JSON.","data":null}}))
        .unwrap();
        assert_eq!(error.request_id(), None);
        assert!(
            serde_json::from_value::<RuntimeFrame>(json!({"type":"event","event":"x"})).is_err()
        );
    }
}
