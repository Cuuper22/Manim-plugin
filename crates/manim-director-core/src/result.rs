//! Operation results. Runtime fields arrive over the bridge; fields marked
//! (engine) are filled by the engine after the runtime returns.

use crate::{names::named_enum, task::InitMode, ExportFormat, Operation, Renderer};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Video,
    Section,
    Image,
    ContactSheet,
    Captions,
    Timeline,
    Archive,
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    /// Project-relative POSIX path.
    pub path: String,
    /// Section name for `section`; otherwise null.
    #[serde(default)]
    pub label: Option<String>,
    /// (engine) size after validation.
    #[serde(default)]
    pub bytes: u64,
    /// (engine) probe for video, section, image and contact_sheet.
    #[serde(default)]
    pub media: Option<MediaInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    /// `mp4`, `mov`, `webm`, `gif` or `png` for artifacts; image sources may
    /// also be `jpeg` or `webp`.
    pub container: String,
    pub codec: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub has_alpha: bool,
}

named_enum! {
    /// Ordered most severe first.
    pub enum Severity {
        Error = "error",
        Warning = "warning",
        Info = "info",
    }
}

/// 1-based; `file` is project-relative, or absolute outside the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub line: u32,
    #[serde(default)]
    pub column: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(default)]
    pub hint: Option<String>,
    #[serde(default)]
    pub location: Option<SourceLocation>,
    #[serde(default)]
    pub at_seconds: Option<f64>,
    #[serde(default)]
    pub beat: Option<String>,
    #[serde(default)]
    pub frame: Option<String>,
}

impl Finding {
    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            hint: None,
            location: None,
            at_seconds: None,
            beat: None,
            frame: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneRef {
    /// Class name.
    pub name: String,
    /// Project-relative file.
    pub file: String,
}

/// (engine) The media a frame/contact_sheet/qa/export job read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaSource {
    pub path: String,
    pub job_id: Option<Uuid>,
    pub scene: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitResult {
    pub mode: InitMode,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub seed: Option<u32>,
    pub template: Option<String>,
    pub scene_template: Option<String>,
    pub theme: Option<String>,
    pub scene: SceneRef,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoverResult {
    /// (engine) number of files scanned.
    #[serde(default)]
    pub files: u32,
    pub truncated: bool,
    pub scenes: Vec<DiscoveredScene>,
    pub findings: Vec<Finding>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredScene {
    pub name: String,
    pub file: String,
    pub line: u32,
    pub end_line: u32,
    pub construct_line: Option<u32>,
    pub bases: Vec<String>,
    pub doc: Option<String>,
    pub theme: Option<String>,
    pub sections: Vec<SectionMark>,
    pub beats: Vec<BeatSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionMark {
    pub name: Option<String>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeatSpan {
    pub id: Option<String>,
    pub line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorResult {
    pub ok: bool,
    pub runtime: RuntimeInfo,
    pub checks: Vec<DoctorCheck>,
    pub capabilities: Capabilities,
    pub disk: DiskSpace,
    pub findings: Vec<Finding>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub version: String,
    pub protocol: u32,
    pub python: String,
    pub executable: String,
    pub platform: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Package,
    Executable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub kind: CheckKind,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub render: bool,
    pub renderers: Vec<Renderer>,
    pub latex: bool,
    pub video_tools: bool,
    pub visual_qa: bool,
    pub symbolic_math: bool,
    pub pdf_ingest: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskSpace {
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderResult {
    pub scene: SceneRef,
    pub duration_seconds: f64,
    pub animations: u32,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StillResult {
    pub scene: SceneRef,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameResult {
    pub at_seconds: f64,
    #[serde(default)]
    pub source: Option<MediaSource>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactSheetResult {
    pub frames: Vec<SheetFrame>,
    pub columns: u8,
    pub rows: u8,
    #[serde(default)]
    pub source: Option<MediaSource>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetFrame {
    pub at_seconds: f64,
    pub beat: Option<String>,
}

named_enum! {
    pub enum QaStatus {
        Pass = "pass",
        Warn = "warn",
        Fail = "fail",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QaResult {
    pub status: QaStatus,
    pub frames: Vec<QaFrame>,
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub source: Option<MediaSource>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QaFrame {
    /// Null for an image source.
    pub at_seconds: Option<f64>,
    pub path: String,
    pub metrics: QaMetrics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QaMetrics {
    pub mean_luminance: f64,
    pub luminance_stddev: f64,
    pub contrast_ratio: f64,
    pub foreground_fraction: f64,
    pub unsafe_fraction: f64,
    pub edge_activity: f64,
    pub foreground_bbox: Option<[u32; 4]>,
    pub background_rgb: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnoseResult {
    pub recognized: bool,
    pub findings: Vec<Finding>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidateMathResult {
    pub valid: Option<bool>,
    pub variables: Vec<String>,
    pub pairs: Vec<MathPair>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MathPair {
    pub index: u32,
    pub equivalent: Option<bool>,
    pub symbolic: SymbolicCheck,
    pub numeric: NumericCheck,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolicCheck {
    pub available: bool,
    pub equivalent: Option<bool>,
    pub difference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NumericCheck {
    pub samples_valid: u32,
    pub samples_skipped: u32,
    pub max_abs_error: Option<f64>,
    pub max_rel_error: Option<f64>,
    pub counterexample: Option<Counterexample>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Counterexample {
    pub variables: BTreeMap<String, f64>,
    pub left: Option<f64>,
    pub right: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptionsResult {
    pub cue_count: u32,
    pub duration_seconds: f64,
    pub valid: bool,
    pub findings: Vec<Finding>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestResult {
    pub sources: Vec<IngestedSource>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestedSource {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub origin: String,
    pub summary: Option<String>,
    pub headings: Vec<String>,
    pub columns: Vec<String>,
    pub rows: Option<u64>,
    pub pages: Option<u32>,
    pub scenes: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_seconds: Option<f64>,
    pub normalized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportResult {
    pub format: ExportFormat,
    pub transcoded: bool,
    pub effective_fps: Option<f64>,
    pub files: Option<u32>,
    pub uncompressed_bytes: Option<u64>,
    pub missing: Option<Vec<String>>,
    #[serde(default)]
    pub source: Option<MediaSource>,
    pub artifacts: Vec<Artifact>,
}

/// The beat timeline a DirectedScene render records (OPS §1.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub version: u32,
    pub scene: String,
    pub duration_seconds: f64,
    pub beats: Vec<TimelineBeat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineBeat {
    pub id: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub file: String,
    pub line: u32,
}

/// One result per operation, stored untagged: the job's operation selects the
/// variant when reading it back.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OperationResult {
    Init(InitResult),
    Discover(DiscoverResult),
    Doctor(DoctorResult),
    Render(RenderResult),
    Still(StillResult),
    Frame(FrameResult),
    ContactSheet(ContactSheetResult),
    Qa(QaResult),
    Diagnose(DiagnoseResult),
    ValidateMath(ValidateMathResult),
    Captions(CaptionsResult),
    Ingest(IngestResult),
    Export(ExportResult),
}

impl OperationResult {
    /// Parses a result object; the error names the offending field.
    pub fn from_json(operation: Operation, value: Value) -> Result<Self, String> {
        fn parse<T: DeserializeOwned>(value: Value) -> Result<T, String> {
            serde_path_to_error::deserialize(value)
                .map_err(|error| format!("{}: {}", error.path(), error.inner()))
        }
        Ok(match operation {
            Operation::Init => Self::Init(parse(value)?),
            Operation::Discover => Self::Discover(parse(value)?),
            Operation::Doctor => Self::Doctor(parse(value)?),
            Operation::Render => Self::Render(parse(value)?),
            Operation::Still => Self::Still(parse(value)?),
            Operation::Frame => Self::Frame(parse(value)?),
            Operation::ContactSheet => Self::ContactSheet(parse(value)?),
            Operation::Qa => Self::Qa(parse(value)?),
            Operation::Diagnose => Self::Diagnose(parse(value)?),
            Operation::ValidateMath => Self::ValidateMath(parse(value)?),
            Operation::Captions => Self::Captions(parse(value)?),
            Operation::Ingest => Self::Ingest(parse(value)?),
            Operation::Export => Self::Export(parse(value)?),
        })
    }

    pub fn artifacts(&self) -> &[Artifact] {
        match self {
            Self::Init(result) => &result.artifacts,
            Self::Discover(result) => &result.artifacts,
            Self::Doctor(result) => &result.artifacts,
            Self::Render(result) => &result.artifacts,
            Self::Still(result) => &result.artifacts,
            Self::Frame(result) => &result.artifacts,
            Self::ContactSheet(result) => &result.artifacts,
            Self::Qa(result) => &result.artifacts,
            Self::Diagnose(result) => &result.artifacts,
            Self::ValidateMath(result) => &result.artifacts,
            Self::Captions(result) => &result.artifacts,
            Self::Ingest(result) => &result.artifacts,
            Self::Export(result) => &result.artifacts,
        }
    }

    pub fn artifacts_mut(&mut self) -> &mut Vec<Artifact> {
        match self {
            Self::Init(result) => &mut result.artifacts,
            Self::Discover(result) => &mut result.artifacts,
            Self::Doctor(result) => &mut result.artifacts,
            Self::Render(result) => &mut result.artifacts,
            Self::Still(result) => &mut result.artifacts,
            Self::Frame(result) => &mut result.artifacts,
            Self::ContactSheet(result) => &mut result.artifacts,
            Self::Qa(result) => &mut result.artifacts,
            Self::Diagnose(result) => &mut result.artifacts,
            Self::ValidateMath(result) => &mut result.artifacts,
            Self::Captions(result) => &mut result.artifacts,
            Self::Ingest(result) => &mut result.artifacts,
            Self::Export(result) => &mut result.artifacts,
        }
    }

    pub fn artifact(&self, kind: ArtifactKind) -> Option<&Artifact> {
        self.artifacts()
            .iter()
            .find(|artifact| artifact.kind == kind)
    }

    /// The scene a render or still actually rendered.
    pub fn scene(&self) -> Option<&SceneRef> {
        match self {
            Self::Render(result) => Some(&result.scene),
            Self::Still(result) => Some(&result.scene),
            _ => None,
        }
    }

    /// Sets the (engine) `source` field of media-consuming results.
    pub fn set_source(&mut self, source: MediaSource) {
        match self {
            Self::Frame(result) => result.source = Some(source),
            Self::ContactSheet(result) => result.source = Some(source),
            Self::Qa(result) => result.source = Some(source),
            Self::Export(result) => result.source = Some(source),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_result_parses_runtime_artifacts_and_serializes_engine_fields() {
        let runtime = json!({"scene":{"name":"Recurrence","file":"scenes/main.py"},"duration_seconds":34.9,"animations":42,
            "artifacts":[{"kind":"video","path":".manim-director/artifacts/7f/Recurrence.mp4","label":null},
                         {"kind":"timeline","path":".manim-director/artifacts/7f/Recurrence.timeline.json","label":null}]});
        let result = OperationResult::from_json(Operation::Render, runtime).unwrap();
        assert_eq!(result.scene().unwrap().name, "Recurrence");
        assert_eq!(result.artifacts().len(), 2);
        let wire = serde_json::to_value(&result).unwrap();
        assert_eq!(
            wire["artifacts"][1],
            json!({"kind":"timeline","path":".manim-director/artifacts/7f/Recurrence.timeline.json","label":null,"bytes":0,"media":null})
        );
    }

    #[test]
    fn malformed_results_name_the_field() {
        let error = OperationResult::from_json(
            Operation::Diagnose,
            json!({"recognized":true,"findings":[{"code":"x","severity":"fatal","message":"m"}],"artifacts":[]}),
        )
        .unwrap_err();
        assert!(error.starts_with("findings[0].severity"), "{error}");
    }

    #[test]
    fn findings_serialize_all_eight_fields() {
        let finding = Finding::warning("index_truncated", "Only 500 files were scanned.");
        let wire = serde_json::to_value(finding).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 8);
        assert_eq!(wire["location"], Value::Null);
    }

    #[test]
    fn media_consumers_carry_an_engine_source() {
        let mut result = OperationResult::from_json(
            Operation::Frame,
            json!({"at_seconds":1.0,"artifacts":[{"kind":"image","path":"a.png","label":null}]}),
        )
        .unwrap();
        result.set_source(MediaSource {
            path: "a.mp4".into(),
            job_id: None,
            scene: Some("A".into()),
        });
        assert_eq!(
            serde_json::to_value(&result).unwrap()["source"]["scene"],
            "A"
        );
    }
}
