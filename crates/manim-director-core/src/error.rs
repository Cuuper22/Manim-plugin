use crate::{names::named_enum, Operation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The error payload shared by HTTP, MCP, job records and bridge `error` frames.
/// `data` is always serialized (`null` when empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub data: Option<Value>,
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, message: impl Into<String>, data: Option<Value>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data,
        }
    }

    pub fn cancelled(by: CancelledBy) -> Self {
        Self::new(
            "cancelled",
            "The job was cancelled.",
            Some(json!({ "by": by })),
        )
    }

    pub fn timeout(timeout_seconds: u64) -> Self {
        Self::new(
            "timeout",
            format!("The job exceeded its {timeout_seconds} s timeout."),
            Some(json!({ "timeout_seconds": timeout_seconds })),
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message, None)
    }

    /// Keeps a runtime error verbatim when its code is part of the contract and
    /// folds anything else into `internal` so clients only see known codes.
    pub fn from_runtime(error: ErrorBody) -> Self {
        if RUNTIME_ERROR_CODES.contains(&error.code.as_str()) {
            return error;
        }
        let mut data = match error.data {
            Some(Value::Object(map)) => map,
            _ => Default::default(),
        };
        data.insert("runtime_code".into(), Value::String(error.code));
        Self::new("internal", error.message, Some(Value::Object(data)))
    }
}

const RUNTIME_ERROR_CODES: &[&str] = &[
    "invalid_request",
    "invalid_params",
    "unknown_method",
    "dependency_missing",
    "scene_not_found",
    "scene_ambiguous",
    "scene_required",
    "render_failed",
    "project_not_empty",
    "media_error",
    "invalid_source",
    "invalid_expression",
    "invalid_captions",
    "io_error",
    "internal",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelledBy {
    Client,
    Shutdown,
}

named_enum! {
    pub enum Budget {
        PythonSources = "python_sources",
        IngestFileBytes = "ingest_file_bytes",
        IngestTotalBytes = "ingest_total_bytes",
        ExportEntries = "export_entries",
        ExportBytes = "export_bytes",
        OutputBytes = "output_bytes",
    }
}

named_enum! {
    pub enum Resource {
        Job = "job",
        File = "file",
        Route = "route",
    }
}

/// Errors returned synchronously when a request is submitted, a direct
/// operation runs, or a source file is read or written. Each kind has one wire
/// code and one HTTP status.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EngineError {
    #[error("{}", invalid_params_message(field.as_deref(), reason))]
    InvalidParams {
        field: Option<String>,
        reason: String,
        allowed: Vec<String>,
    },
    #[error("{reason}")]
    InvalidSpec {
        reason: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    #[error("Operation {operation} is not accepted here.")]
    OperationNotAllowed {
        operation: Operation,
        allowed: Vec<Operation>,
    },
    #[error("No {resource} {key}.")]
    NotFound { resource: Resource, key: String },
    #[error("{}", source_not_found_message(scene.as_deref(), profile.as_deref()))]
    SourceNotFound {
        scene: Option<String>,
        profile: Option<String>,
    },
    #[error("{}", project_not_empty_message(*project))]
    ProjectNotEmpty {
        entries: Vec<String>,
        /// The directory already holds a `director.yaml`.
        project: bool,
    },
    #[error("The request is {} bytes; the limit is {limit_bytes}.", actual_bytes.map_or("too many".to_owned(), |n| n.to_string()))]
    RequestTooLarge {
        limit_bytes: u64,
        actual_bytes: Option<u64>,
    },
    #[error("{budget} is {actual}, over the limit of {limit}.")]
    BudgetExceeded {
        budget: Budget,
        limit: u64,
        actual: u64,
    },
    #[error("The job queue is full ({capacity} jobs); retry when a job finishes.")]
    QueueFull { capacity: usize },
    #[error("{path} is not an editable project path ({reason}).")]
    InvalidPath { path: String, reason: &'static str },
    #[error("{path}: .{extension} files are not accepted here.")]
    UnsupportedFileType { path: String, extension: String },
    #[error("{path} is not UTF-8 text.")]
    NotUtf8 { path: String },
    #[error("{path} is {bytes} bytes; the limit is {limit_bytes}.")]
    FileTooLarge {
        path: String,
        bytes: u64,
        limit_bytes: u64,
    },
    #[error("Lines {start_line}..={end_line} are outside the file ({total_lines} lines).")]
    LineOutOfRange {
        start_line: u64,
        end_line: u64,
        total_lines: u64,
    },
    #[error("{}", source_invalid_message(path, *line, *column, message))]
    SourceInvalid {
        path: String,
        language: String,
        message: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    #[error("{}", revision_conflict_message(path, current_revision.as_deref()))]
    RevisionConflict {
        path: String,
        expected_revision: Option<String>,
        current_revision: Option<String>,
    },
    /// A direct operation (`init`, `discover`) failed with a job failure code.
    #[error("{}", .0.message)]
    Operation(ErrorBody),
    #[error("Internal error: {0}")]
    Internal(String),
}

fn invalid_params_message(field: Option<&str>, reason: &str) -> String {
    match field {
        Some(field) => format!("Invalid {field}: {reason}."),
        None => format!("Invalid request: {reason}."),
    }
}

fn source_not_found_message(scene: Option<&str>, profile: Option<&str>) -> String {
    let scene = scene
        .map(|scene| format!(" of {scene}"))
        .unwrap_or_default();
    let profile = profile
        .map(|profile| format!(" at {profile}"))
        .unwrap_or_default();
    format!("No successful render{scene}{profile}; render it first.")
}

fn project_not_empty_message(project: bool) -> &'static str {
    match project {
        true => "This directory is already a project; add a scene with scene_template (--scene-template). force (--force) would replace director.yaml and scenes/main.py, and keeps no copy.",
        false => "The target directory is not empty; force (--force) writes the template into it, replacing any of its files with the same names.",
    }
}

fn source_invalid_message(
    path: &str,
    line: Option<u32>,
    column: Option<u32>,
    message: &str,
) -> String {
    match (line, column) {
        (Some(line), Some(column)) => format!("{path}:{line}:{column}: {message}"),
        (Some(line), None) => format!("{path}:{line}: {message}"),
        _ => format!("{path}: {message}"),
    }
}

fn revision_conflict_message(path: &str, current: Option<&str>) -> String {
    match current {
        Some(_) => format!("{path} changed since it was read; reload it before saving."),
        None => format!("{path} no longer exists; reload before saving."),
    }
}

impl EngineError {
    pub fn invalid(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidParams {
            field: Some(field.into()),
            reason: reason.into(),
            allowed: Vec::new(),
        }
    }

    pub fn invalid_choice(
        field: impl Into<String>,
        reason: impl Into<String>,
        allowed: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::InvalidParams {
            field: Some(field.into()),
            reason: reason.into(),
            allowed: allowed.into_iter().map(Into::into).collect(),
        }
    }

    pub fn job_not_found(id: impl ToString) -> Self {
        Self::NotFound {
            resource: Resource::Job,
            key: id.to_string(),
        }
    }

    pub fn internal(error: impl ToString) -> Self {
        Self::Internal(error.to_string())
    }

    pub fn code(&self) -> &str {
        match self {
            Self::InvalidParams { .. } => "invalid_params",
            Self::InvalidSpec { .. } => "invalid_spec",
            Self::OperationNotAllowed { .. } => "operation_not_allowed",
            Self::NotFound { .. } => "not_found",
            Self::SourceNotFound { .. } => "source_not_found",
            Self::ProjectNotEmpty { .. } => "project_not_empty",
            Self::RequestTooLarge { .. } => "request_too_large",
            Self::BudgetExceeded { .. } => "budget_exceeded",
            Self::QueueFull { .. } => "queue_full",
            Self::InvalidPath { .. } => "invalid_path",
            Self::UnsupportedFileType { .. } => "unsupported_file_type",
            Self::NotUtf8 { .. } => "not_utf8",
            Self::FileTooLarge { .. } => "file_too_large",
            Self::LineOutOfRange { .. } => "line_out_of_range",
            Self::SourceInvalid { .. } => "source_invalid",
            Self::RevisionConflict { .. } => "revision_conflict",
            Self::Operation(body) => &body.code,
            Self::Internal(_) => "internal",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::InvalidParams { .. }
            | Self::InvalidSpec { .. }
            | Self::OperationNotAllowed { .. }
            | Self::InvalidPath { .. }
            | Self::UnsupportedFileType { .. }
            | Self::NotUtf8 { .. }
            | Self::FileTooLarge { .. }
            | Self::LineOutOfRange { .. }
            | Self::SourceInvalid { .. } => 400,
            Self::NotFound { .. } | Self::SourceNotFound { .. } => 404,
            Self::ProjectNotEmpty { .. } | Self::RevisionConflict { .. } => 409,
            Self::RequestTooLarge { .. } | Self::BudgetExceeded { .. } => 413,
            Self::QueueFull { .. } => 429,
            Self::Operation(body) => match body.code.as_str() {
                "invalid_params" | "invalid_request" => 400,
                "project_not_empty" => 409,
                _ => 500,
            },
            Self::Internal(_) => 500,
        }
    }

    pub fn data(&self) -> Option<Value> {
        let data = match self {
            Self::InvalidParams {
                field,
                reason,
                allowed,
            } if allowed.is_empty() => json!({ "field": field, "reason": reason }),
            Self::InvalidParams {
                field,
                reason,
                allowed,
            } => json!({ "field": field, "reason": reason, "allowed": allowed }),
            Self::InvalidSpec {
                reason,
                line,
                column,
            } => {
                json!({ "path": crate::SPEC_FILE, "reason": reason, "line": line, "column": column })
            }
            Self::OperationNotAllowed { operation, allowed } => {
                json!({ "operation": operation, "allowed": allowed })
            }
            Self::NotFound { resource, key } => json!({ "resource": resource, "key": key }),
            Self::SourceNotFound { scene, profile } => {
                json!({ "scene": scene, "profile": profile })
            }
            Self::ProjectNotEmpty { entries, .. } => json!({ "entries": entries }),
            Self::RequestTooLarge {
                limit_bytes,
                actual_bytes,
            } => json!({ "limit_bytes": limit_bytes, "actual_bytes": actual_bytes }),
            Self::BudgetExceeded {
                budget,
                limit,
                actual,
            } => json!({ "budget": budget, "limit": limit, "actual": actual }),
            Self::QueueFull { capacity } => json!({ "capacity": capacity }),
            Self::InvalidPath { path, reason } => json!({ "path": path, "reason": reason }),
            Self::UnsupportedFileType { path, extension } => {
                json!({ "path": path, "extension": extension })
            }
            Self::NotUtf8 { path } => json!({ "path": path }),
            Self::FileTooLarge {
                path,
                bytes,
                limit_bytes,
            } => json!({ "path": path, "bytes": bytes, "limit_bytes": limit_bytes }),
            Self::LineOutOfRange {
                start_line,
                end_line,
                total_lines,
            } => json!({
                "start_line": start_line,
                "end_line": end_line,
                "total_lines": total_lines,
            }),
            Self::SourceInvalid {
                path,
                language,
                line,
                column,
                ..
            } => json!({ "path": path, "language": language, "line": line, "column": column }),
            Self::RevisionConflict {
                path,
                expected_revision,
                current_revision,
            } => json!({
                "path": path,
                "expected_revision": expected_revision,
                "current_revision": current_revision,
            }),
            Self::Operation(body) => return body.data.clone(),
            Self::Internal(_) => return None,
        };
        Some(data)
    }

    pub fn body(&self) -> ErrorBody {
        match self {
            Self::Operation(body) => body.clone(),
            _ => ErrorBody::new(self.code(), self.to_string(), self.data()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_contract_code_and_status() {
        let cases = [
            (
                EngineError::invalid("scene", "too long"),
                "invalid_params",
                400,
            ),
            (
                EngineError::InvalidSpec {
                    reason: "bad".into(),
                    line: Some(3),
                    column: None,
                },
                "invalid_spec",
                400,
            ),
            (
                EngineError::OperationNotAllowed {
                    operation: Operation::Ingest,
                    allowed: vec![Operation::Render],
                },
                "operation_not_allowed",
                400,
            ),
            (EngineError::job_not_found("x"), "not_found", 404),
            (
                EngineError::SourceNotFound {
                    scene: None,
                    profile: None,
                },
                "source_not_found",
                404,
            ),
            (
                EngineError::ProjectNotEmpty {
                    entries: vec![],
                    project: false,
                },
                "project_not_empty",
                409,
            ),
            (
                EngineError::RequestTooLarge {
                    limit_bytes: 1,
                    actual_bytes: Some(2),
                },
                "request_too_large",
                413,
            ),
            (
                EngineError::BudgetExceeded {
                    budget: Budget::PythonSources,
                    limit: 500,
                    actual: 501,
                },
                "budget_exceeded",
                413,
            ),
            (EngineError::QueueFull { capacity: 4 }, "queue_full", 429),
            (
                EngineError::InvalidPath {
                    path: ".git/x".into(),
                    reason: "hidden",
                },
                "invalid_path",
                400,
            ),
            (
                EngineError::FileTooLarge {
                    path: "a.py".into(),
                    bytes: 3,
                    limit_bytes: 2,
                },
                "file_too_large",
                400,
            ),
            (
                EngineError::SourceInvalid {
                    path: "a.py".into(),
                    language: "python".into(),
                    message: "invalid syntax".into(),
                    line: Some(2),
                    column: None,
                },
                "source_invalid",
                400,
            ),
            (
                EngineError::RevisionConflict {
                    path: "a.py".into(),
                    expected_revision: Some("old".into()),
                    current_revision: Some("new".into()),
                },
                "revision_conflict",
                409,
            ),
            (EngineError::internal("boom"), "internal", 500),
        ];
        for (error, code, status) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.status(), status);
            assert_eq!(error.body().code, code);
        }
    }

    #[test]
    fn source_not_found_names_what_was_searched() {
        let error = EngineError::SourceNotFound {
            scene: Some("Recurrence".into()),
            profile: Some("preview".into()),
        };
        assert_eq!(
            error.to_string(),
            "No successful render of Recurrence at preview; render it first."
        );
    }

    #[test]
    fn unknown_runtime_codes_become_internal_and_keep_the_original() {
        let known = ErrorBody::new("scene_not_found", "missing", None);
        assert_eq!(ErrorBody::from_runtime(known.clone()), known);
        let unknown = ErrorBody::from_runtime(ErrorBody::new("weird", "odd", None));
        assert_eq!(unknown.code, "internal");
        assert_eq!(unknown.data.unwrap()["runtime_code"], "weird");
    }

    #[test]
    fn error_body_always_serializes_data() {
        let body = ErrorBody::new("internal", "x", None);
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            json!({"code":"internal","message":"x","data":null})
        );
    }
}
