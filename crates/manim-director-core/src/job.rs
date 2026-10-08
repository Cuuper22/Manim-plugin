use crate::{
    names::named_enum, ErrorBody, JobOrigin, JobStatus, Operation, OperationRequest,
    OperationResult, Task,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::{fmt, str::FromStr, sync::Arc};
use uuid::Uuid;

/// RFC 3339 UTC with millisecond precision and `Z`, on the wire and in the DB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(DateTime<Utc>);

impl Timestamp {
    pub fn now() -> Self {
        Self(Utc::now())
    }

    pub fn as_datetime(&self) -> DateTime<Utc> {
        self.0
    }
}

impl From<DateTime<Utc>> for Timestamp {
    fn from(value: DateTime<Utc>) -> Self {
        Self(value)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.to_rfc3339_opts(SecondsFormat::Millis, true))
    }
}

impl FromStr for Timestamp {
    type Err = chrono::ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        DateTime::parse_from_rfc3339(value).map(|time| Self(time.with_timezone(&Utc)))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub timeout_seconds: u64,
    pub memory_mb: Option<u64>,
}

named_enum! {
    pub enum ProgressPhase {
        Starting = "starting",
        Import = "import",
        Animate = "animate",
        Encode = "encode",
        Extract = "extract",
        Analyze = "analyze",
        Package = "package",
        Transcode = "transcode",
        Ingest = "ingest",
        Validate = "validate",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub phase: ProgressPhase,
    pub current: u64,
    pub total: Option<u64>,
    pub scene_seconds: Option<f64>,
    pub message: Option<String>,
    pub updated_at: Timestamp,
}

/// The persisted job (OPS §1.8): the DB row, the CLI `--json` output and the
/// base of every frontend's job view.
#[derive(Debug, Clone, Serialize)]
pub struct JobRecord {
    pub id: Uuid,
    pub sequence: i64,
    pub operation: Operation,
    pub status: JobStatus,
    pub origin: JobOrigin,
    pub owner: Uuid,
    pub request: OperationRequest,
    pub task: Task,
    pub limits: Limits,
    pub fingerprint: Option<String>,
    pub cached: bool,
    pub cached_from: Option<Uuid>,
    pub source_job_id: Option<Uuid>,
    pub scene_id: Option<String>,
    pub scene_revision: Option<String>,
    pub profile: Option<String>,
    pub cancel_requested: bool,
    pub progress: Option<Progress>,
    pub result: Option<OperationResult>,
    pub error: Option<ErrorBody>,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    #[serde(skip)]
    pub scene_class: Option<String>,
    #[serde(skip)]
    pub scene_file: Option<String>,
}

/// `"<file>#<Class>"`, how every surface names a scene.
pub fn scene_key(file: &str, class: &str) -> String {
    format!("{file}#{class}")
}

/// The scene id when both halves are known.
pub fn scene_id(file: Option<&str>, class: Option<&str>) -> Option<String> {
    Some(scene_key(file?, class?))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobSummary {
    pub id: Uuid,
    pub operation: Operation,
    pub status: JobStatus,
    pub cached: bool,
    pub scene_id: Option<String>,
    pub progress: Option<Progress>,
    pub created_at: Timestamp,
    pub finished_at: Option<Timestamp>,
}

impl From<&JobRecord> for JobSummary {
    fn from(job: &JobRecord) -> Self {
        Self {
            id: job.id,
            operation: job.operation,
            status: job.status,
            cached: job.cached,
            scene_id: job.scene_id.clone(),
            progress: job.progress.clone(),
            created_at: job.created_at,
            finished_at: job.finished_at,
        }
    }
}

named_enum! {
    pub enum LogStream {
        Runtime = "runtime",
        Stderr = "stderr",
        Engine = "engine",
    }
}

named_enum! {
    pub enum LogLevel {
        Info = "info",
        Warning = "warning",
        Error = "error",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRecord {
    /// Decimal DB cursor; opaque to clients.
    pub cursor: String,
    pub timestamp: Timestamp,
    pub stream: LogStream,
    pub level: LogLevel,
    pub message: String,
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

/// What an engine announces in process, after the change is committed.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// A job was queued, started or ended (also a cache hit or a reaped job).
    Job(Arc<JobRecord>),
    Progress {
        job_id: Uuid,
        progress: Progress,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_always_carry_milliseconds_and_z() {
        let whole: Timestamp = "2026-10-07T10:28:00Z".parse().unwrap();
        assert_eq!(whole.to_string(), "2026-10-07T10:28:00.000Z");
        let fine: Timestamp = "2026-10-07T10:28:00.123456+00:00".parse().unwrap();
        assert_eq!(
            serde_json::to_value(fine).unwrap(),
            "2026-10-07T10:28:00.123Z"
        );
    }

    #[test]
    fn scene_ids_need_both_halves() {
        assert_eq!(
            scene_id(Some("scenes/main.py"), Some("Recurrence")).as_deref(),
            Some("scenes/main.py#Recurrence")
        );
        assert_eq!(scene_id(None, Some("Recurrence")), None);
    }
}
