use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, str::FromStr};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Scaffold,
    Ingest,
    Discover,
    Inspect,
    Doctor,
    Render,
    Preview,
    Still,
    ContactSheet,
    Qa,
    Debug,
    ValidateMath,
    Captions,
    Assets,
    Export,
    Sample,
}

impl Operation {
    pub fn runtime_method(self) -> &'static str {
        match self {
            Self::Scaffold => "scaffold",
            Self::Ingest => "ingest",
            Self::Discover => "discover",
            Self::Inspect => "inspect",
            Self::Doctor => "doctor",
            Self::Render => "render",
            Self::Preview => "preview",
            Self::Still => "still",
            Self::ContactSheet => "contact_sheet",
            Self::Qa => "qa",
            Self::Debug => "diagnose",
            Self::ValidateMath => "math_validate",
            Self::Captions => "captions",
            Self::Assets => "assets",
            Self::Export => "export",
            Self::Sample => "sample",
        }
    }

    pub fn cacheable(self) -> bool {
        matches!(
            self,
            Self::Render | Self::Preview | Self::Still | Self::Inspect
        )
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.runtime_method())
    }
}

impl FromStr for Operation {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let operation = match value.replace('-', "_").as_str() {
            "scaffold" | "init" => Self::Scaffold,
            "ingest" => Self::Ingest,
            "discover" => Self::Discover,
            "inspect" => Self::Inspect,
            "doctor" => Self::Doctor,
            "render" => Self::Render,
            "preview" => Self::Preview,
            "still" => Self::Still,
            "contact_sheet" => Self::ContactSheet,
            "qa" => Self::Qa,
            "debug" | "diagnose" => Self::Debug,
            "validate_math" | "math_validate" => Self::ValidateMath,
            "captions" => Self::Captions,
            "assets" => Self::Assets,
            "export" => Self::Export,
            "sample" => Self::Sample,
            other => return Err(format!("unsupported operation: {other}")),
        };
        Ok(operation)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        })
    }
}

impl FromStr for JobStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("invalid job status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub operation: Operation,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: Uuid,
    pub sequence: i64,
    pub project_root: String,
    pub operation: Operation,
    pub status: JobStatus,
    pub params: Value,
    pub fingerprint: Option<String>,
    pub result: Option<Value>,
    pub error: Option<ProtocolError>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSummary {
    pub id: Uuid,
    pub operation: Operation,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub cached: bool,
}

impl From<&JobRecord> for JobSummary {
    fn from(job: &JobRecord) -> Self {
        Self {
            id: job.id,
            operation: job.operation,
            status: job.status,
            created_at: job.created_at,
            finished_at: job.finished_at,
            cached: job.cached,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub cursor: i64,
    pub job_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub event: String,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ProtocolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    JobQueued {
        job: JobSummary,
    },
    JobStarted {
        job: JobSummary,
    },
    JobProgress {
        job_id: Uuid,
        event: String,
        data: Value,
    },
    JobFinished {
        job: JobSummary,
        result: Option<Value>,
        error: Option<ProtocolError>,
    },
}

#[cfg(test)]
mod tests {
    use super::Operation;

    #[test]
    fn only_repeatable_read_or_render_operations_are_cacheable() {
        for operation in [
            Operation::Render,
            Operation::Preview,
            Operation::Still,
            Operation::Inspect,
        ] {
            assert!(operation.cacheable(), "{operation} should be cacheable");
        }
        for operation in [
            Operation::Scaffold,
            Operation::Ingest,
            Operation::Discover,
            Operation::Doctor,
            Operation::ContactSheet,
            Operation::Qa,
            Operation::Debug,
            Operation::ValidateMath,
            Operation::Captions,
            Operation::Assets,
            Operation::Export,
            Operation::Sample,
        ] {
            assert!(!operation.cacheable(), "{operation} must not be cached");
        }
    }
}
