//! Public operation params (what CLI, HTTP and MCP send) and their field-level
//! validation. Nothing here touches the disk; resolution against the project
//! happens in the engine.

use crate::{names::named_enum, EngineError, Operation};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

pub const ARTIFACTS_DIR: &str = ".manim-director/artifacts";

/// Picks an existing media artifact: `{"job_id": …}` or `{"path": …}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceRef {
    JobId(Uuid),
    Path(String),
}

named_enum! {
    pub enum ExportFormat {
        Zip = "zip",
        Mp4 = "mp4",
        Webm = "webm",
        Gif = "gif",
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverParams {}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorParams {}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default)]
    pub sections: bool,
    #[serde(default)]
    pub fresh: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StillParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default)]
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameParams {
    pub at_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactSheetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default = "default_sheet_count")]
    pub count: u8,
    #[serde(default = "default_sheet_columns")]
    pub columns: u8,
}

impl Default for ContactSheetParams {
    fn default() -> Self {
        Self {
            source: None,
            scene: None,
            profile: None,
            count: default_sheet_count(),
            columns: default_sheet_columns(),
        }
    }
}

fn default_sheet_count() -> u8 {
    6
}
fn default_sheet_columns() -> u8 {
    3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QaParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default = "default_qa_frames")]
    pub frames: u8,
}

impl Default for QaParams {
    fn default() -> Self {
        Self {
            source: None,
            scene: None,
            profile: None,
            frames: default_qa_frames(),
        }
    }
}

fn default_qa_frames() -> u8 {
    8
}

/// Exactly one of `job_id` (a failed or cancelled job) or `text`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnoseParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateMathParams {
    pub steps: Vec<String>,
    #[serde(default)]
    pub ranges: BTreeMap<String, [f64; 2]>,
    #[serde(default = "default_samples")]
    pub samples: u32,
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

fn default_samples() -> u32 {
    200
}
fn default_tolerance() -> f64 {
    1e-9
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptionsParams {
    pub path: String,
    #[serde(default)]
    pub shift_seconds: f64,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestParams {
    pub sources: Vec<IngestSource>,
    #[serde(default)]
    pub normalize: bool,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestSource {
    /// Absolute host path; frontends resolve relative input before building it.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportParams {
    #[serde(default = "default_export_format")]
    pub format: ExportFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gif_fps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gif_width: Option<u32>,
}

impl Default for ExportParams {
    fn default() -> Self {
        Self {
            format: default_export_format(),
            source: None,
            scene: None,
            profile: None,
            output: None,
            gif_fps: None,
            gif_width: None,
        }
    }
}

fn default_export_format() -> ExportFormat {
    ExportFormat::Zip
}

/// A typed operation request. The wire form is the flattened, internally
/// tagged object: `{"operation":"render","scene":"Recurrence"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum OperationRequest {
    Init(InitParams),
    Discover(DiscoverParams),
    Doctor(DoctorParams),
    Render(RenderParams),
    Still(StillParams),
    Frame(FrameParams),
    ContactSheet(ContactSheetParams),
    Qa(QaParams),
    Diagnose(DiagnoseParams),
    ValidateMath(ValidateMathParams),
    Captions(CaptionsParams),
    Ingest(IngestParams),
    Export(ExportParams),
}

impl OperationRequest {
    pub fn operation(&self) -> Operation {
        match self {
            Self::Init(_) => Operation::Init,
            Self::Discover(_) => Operation::Discover,
            Self::Doctor(_) => Operation::Doctor,
            Self::Render(_) => Operation::Render,
            Self::Still(_) => Operation::Still,
            Self::Frame(_) => Operation::Frame,
            Self::ContactSheet(_) => Operation::ContactSheet,
            Self::Qa(_) => Operation::Qa,
            Self::Diagnose(_) => Operation::Diagnose,
            Self::ValidateMath(_) => Operation::ValidateMath,
            Self::Captions(_) => Operation::Captions,
            Self::Ingest(_) => Operation::Ingest,
            Self::Export(_) => Operation::Export,
        }
    }

    /// Parses the tagged wire object. Errors name the offending field.
    pub fn from_json(value: Value) -> Result<Self, EngineError> {
        let Value::Object(mut object) = value else {
            return Err(EngineError::InvalidParams {
                field: None,
                reason: "expected a JSON object".into(),
                allowed: Vec::new(),
            });
        };
        let operation = match object.remove("operation") {
            Some(Value::String(name)) => name.parse::<Operation>().map_err(|error| {
                EngineError::invalid_choice("operation", "unknown operation", error.allowed)
            })?,
            Some(_) => return Err(EngineError::invalid("operation", "expected a string")),
            None => return Err(EngineError::invalid("operation", "missing field")),
        };
        Self::from_params(operation, Value::Object(object))
    }

    /// Parses an operation's params object (no `operation` key).
    pub fn from_params(operation: Operation, params: Value) -> Result<Self, EngineError> {
        Ok(match operation {
            Operation::Init => Self::Init(parse(params)?),
            Operation::Discover => Self::Discover(parse(params)?),
            Operation::Doctor => Self::Doctor(parse(params)?),
            Operation::Render => Self::Render(parse(params)?),
            Operation::Still => Self::Still(parse(params)?),
            Operation::Frame => Self::Frame(parse(params)?),
            Operation::ContactSheet => Self::ContactSheet(parse(params)?),
            Operation::Qa => Self::Qa(parse(params)?),
            Operation::Diagnose => Self::Diagnose(parse(params)?),
            Operation::ValidateMath => Self::ValidateMath(parse(params)?),
            Operation::Captions => Self::Captions(parse(params)?),
            Operation::Ingest => Self::Ingest(parse(params)?),
            Operation::Export => Self::Export(parse(params)?),
        })
    }

    /// Field-level rules: ranges, exclusivity and the lexical path rule.
    pub fn validate(&self) -> Result<(), EngineError> {
        match self {
            Self::Init(params) => params.validate(),
            Self::Discover(_) | Self::Doctor(_) => Ok(()),
            Self::Render(params) => validate_target(
                params.scene.as_deref(),
                params.file.as_deref(),
                params.profile.as_deref(),
            ),
            Self::Still(params) => validate_target(
                params.scene.as_deref(),
                params.file.as_deref(),
                params.profile.as_deref(),
            ),
            Self::Frame(params) => {
                if !params.at_seconds.is_finite() || params.at_seconds < 0.0 {
                    return Err(EngineError::invalid(
                        "at_seconds",
                        "must be a finite time >= 0",
                    ));
                }
                validate_selection(params.source.as_ref(), &params.scene, &params.profile)
            }
            Self::ContactSheet(params) => {
                check_range("count", params.count, 1, 24)?;
                check_range("columns", params.columns, 1, 8)?;
                validate_selection(params.source.as_ref(), &params.scene, &params.profile)
            }
            Self::Qa(params) => {
                check_range("frames", params.frames, 1, 40)?;
                validate_selection(params.source.as_ref(), &params.scene, &params.profile)
            }
            Self::Diagnose(params) => params.validate(),
            Self::ValidateMath(params) => params.validate(),
            Self::Captions(params) => params.validate(),
            Self::Ingest(params) => params.validate(),
            Self::Export(params) => params.validate(),
        }
    }
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, EngineError> {
    serde_path_to_error::deserialize(params).map_err(|error| {
        let path = error.path().to_string();
        EngineError::InvalidParams {
            field: (path != ".").then_some(path),
            reason: error.into_inner().to_string(),
            allowed: Vec::new(),
        }
    })
}

impl InitParams {
    fn validate(&self) -> Result<(), EngineError> {
        if self.scene_template.is_some() {
            let create_only = [
                ("name", self.name.is_some()),
                ("template", self.template.is_some()),
                ("theme", self.theme.is_some()),
                ("seed", self.seed.is_some()),
            ];
            if let Some((field, _)) = create_only.iter().find(|(_, set)| *set) {
                return Err(EngineError::invalid(
                    *field,
                    "only applies when creating a project, not with scene_template",
                ));
            }
        }
        if let Some(name) = &self.name {
            check_text("name", name.trim(), 120)?;
        }
        if self.seed.is_some_and(|seed| seed > 2_147_483_647) {
            return Err(EngineError::invalid("seed", "must be <= 2147483647"));
        }
        Ok(())
    }
}

impl DiagnoseParams {
    fn validate(&self) -> Result<(), EngineError> {
        match (&self.job_id, &self.text) {
            (Some(_), None) => Ok(()),
            (None, Some(text)) if !text.is_empty() => Ok(()),
            (None, Some(_)) => Err(EngineError::invalid("text", "cannot be empty")),
            _ => Err(EngineError::InvalidParams {
                field: None,
                reason: "provide exactly one of job_id or text".into(),
                allowed: Vec::new(),
            }),
        }
    }
}

impl ValidateMathParams {
    fn validate(&self) -> Result<(), EngineError> {
        if !(2..=32).contains(&self.steps.len()) {
            return Err(EngineError::invalid("steps", "needs 2 to 32 steps"));
        }
        for (index, step) in self.steps.iter().enumerate() {
            check_text(&format!("steps[{index}]"), step, 2000)?;
        }
        for (name, [low, high]) in &self.ranges {
            let field = format!("ranges.{name}");
            if !is_identifier(name) {
                return Err(EngineError::invalid(field, "key must be an identifier"));
            }
            if !(low.is_finite() && high.is_finite() && low < high) {
                return Err(EngineError::invalid(
                    field,
                    "needs finite bounds with lo < hi",
                ));
            }
        }
        check_range("samples", self.samples, 1, 10_000)?;
        if !(self.tolerance > 0.0 && self.tolerance <= 1.0) {
            return Err(EngineError::invalid("tolerance", "must be in (0, 1]"));
        }
        Ok(())
    }
}

impl CaptionsParams {
    fn validate(&self) -> Result<(), EngineError> {
        check_project_path("path", &self.path, true)?;
        check_extension("path", &self.path, crate::files::CAPTIONS)?;
        if !(self.shift_seconds.is_finite() && self.shift_seconds.abs() <= 86_400.0) {
            return Err(EngineError::invalid(
                "shift_seconds",
                "must be within ±86400",
            ));
        }
        if !(self.scale > 0.0 && self.scale <= 10.0) {
            return Err(EngineError::invalid("scale", "must be in (0, 10]"));
        }
        if let Some(output) = &self.output {
            check_project_path("output", output, false)?;
            check_extension("output", output, crate::files::CAPTIONS)?;
        }
        Ok(())
    }
}

impl IngestParams {
    fn validate(&self) -> Result<(), EngineError> {
        if !(1..=64).contains(&self.sources.len()) {
            return Err(EngineError::invalid("sources", "needs 1 to 64 sources"));
        }
        for (index, source) in self.sources.iter().enumerate() {
            let field = |name: &str| format!("sources[{index}].{name}");
            if !std::path::Path::new(&source.path).is_absolute() {
                return Err(EngineError::invalid(
                    field("path"),
                    "must be an absolute host path",
                ));
            }
            if let Some(id) = &source.id {
                check_text(&field("id"), id, 64)?;
                if !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
                {
                    return Err(EngineError::invalid(
                        field("id"),
                        "may only contain A-Z a-z 0-9 _ . -",
                    ));
                }
            }
            if let Some(license) = &source.license {
                check_text(&field("license"), license, 200)?;
            }
            if let Some(attribution) = &source.attribution {
                check_text(&field("attribution"), attribution, 500)?;
            }
        }
        Ok(())
    }
}

impl ExportParams {
    fn validate(&self) -> Result<(), EngineError> {
        validate_selection(self.source.as_ref(), &self.scene, &self.profile)?;
        if let Some(output) = &self.output {
            check_project_path("output", output, false)?;
            check_extension("output", output, &[self.format.as_str()])?;
        }
        let gif = self.format == ExportFormat::Gif;
        if let Some(fps) = self.gif_fps {
            if !gif {
                return Err(EngineError::invalid("gif_fps", "only applies to gif"));
            }
            check_range("gif_fps", fps, 1, 50)?;
        }
        if let Some(width) = self.gif_width {
            if !gif {
                return Err(EngineError::invalid("gif_width", "only applies to gif"));
            }
            check_range("gif_width", width, 64, 3840)?;
        }
        Ok(())
    }
}

fn validate_target(
    scene: Option<&str>,
    file: Option<&str>,
    profile: Option<&str>,
) -> Result<(), EngineError> {
    if let Some(scene) = scene {
        check_text("scene", scene, 200)?;
    }
    if let Some(file) = file {
        check_project_path("file", file, false)?;
        check_extension("file", file, &["py"])?;
    }
    if let Some(profile) = profile {
        check_text("profile", profile, 200)?;
    }
    Ok(())
}

fn validate_selection(
    source: Option<&SourceRef>,
    scene: &Option<String>,
    profile: &Option<String>,
) -> Result<(), EngineError> {
    if let Some(SourceRef::Path(path)) = source {
        check_project_path("source", path, true)?;
    }
    validate_target(scene.as_deref(), None, profile.as_deref())
}

fn check_text(field: &str, value: &str, max_chars: usize) -> Result<(), EngineError> {
    if value.is_empty() {
        return Err(EngineError::invalid(field, "cannot be empty"));
    }
    if value.chars().count() > max_chars {
        return Err(EngineError::invalid(
            field,
            format!("longer than {max_chars} characters"),
        ));
    }
    Ok(())
}

fn check_range<T: PartialOrd + std::fmt::Display>(
    field: &str,
    value: T,
    low: T,
    high: T,
) -> Result<(), EngineError> {
    if value < low || value > high {
        return Err(EngineError::invalid(
            field,
            format!("must be between {low} and {high}"),
        ));
    }
    Ok(())
}

fn check_extension(field: &str, value: &str, allowed: &[&str]) -> Result<(), EngineError> {
    if crate::files::has_extension(value, allowed) {
        Ok(())
    } else {
        Err(EngineError::invalid_choice(
            field,
            "extension",
            allowed.iter().copied(),
        ))
    }
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Rules 1–3 of the public path rule (OPS §1.2): a project-relative POSIX path
/// with no traversal and no hidden segments. `allow_artifacts` admits paths
/// under `.manim-director/artifacts/`.
pub fn check_project_path(
    field: &str,
    value: &str,
    allow_artifacts: bool,
) -> Result<(), EngineError> {
    match path_rule_violation(value, allow_artifacts) {
        Some(reason) => Err(EngineError::invalid(field, reason)),
        None => Ok(()),
    }
}

/// The lexical rule `value` breaks: `absolute`, `traversal` or `hidden`.
pub fn path_rule_violation(value: &str, allow_artifacts: bool) -> Option<&'static str> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with('/')
        || value.contains(['\\', '\0', ':'])
    {
        return Some("absolute");
    }
    if value
        .split('/')
        .any(|segment| matches!(segment, "" | "." | ".."))
    {
        return Some("traversal");
    }
    let visible = match value.strip_prefix(ARTIFACTS_DIR) {
        Some(rest) if allow_artifacts && rest.starts_with('/') => &rest[1..],
        _ => value,
    };
    visible
        .split('/')
        .any(|segment| segment.starts_with('.'))
        .then_some("hidden")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rejection(error: EngineError) -> (Option<String>, String) {
        match error {
            EngineError::InvalidParams { field, reason, .. } => (field, reason),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn every_variant_round_trips_through_the_wire_form() {
        let requests = [
            json!({"operation":"init","name":"Film","force":false}),
            json!({"operation":"discover"}),
            json!({"operation":"doctor"}),
            json!({"operation":"render","scene":"A","file":"scenes/main.py","profile":"draft","sections":true,"fresh":false}),
            json!({"operation":"still","scene":"A","fresh":true}),
            json!({"operation":"frame","at_seconds":1.5,"source":{"job_id":"7f1c2b0e-0d5e-4b1a-9a57-3c1f7d0c2a11"}}),
            json!({"operation":"contact_sheet","source":{"path":".manim-director/artifacts/x/a.mp4"},"count":4,"columns":2}),
            json!({"operation":"qa","scene":"A","frames":8}),
            json!({"operation":"diagnose","text":"Traceback"}),
            json!({"operation":"validate_math","steps":["a","a"],"ranges":{"a":[-1.0,1.0]},"samples":10,"tolerance":0.001,"seed":4}),
            json!({"operation":"captions","path":"captions/en.vtt","shift_seconds":0.5,"scale":1.0,"output":"captions/en.srt"}),
            json!({"operation":"ingest","sources":[{"path":"/tmp/notes.md","id":"notes"}],"normalize":false,"force":false}),
            json!({"operation":"export","format":"gif","scene":"A","gif_fps":12}),
        ];
        assert_eq!(requests.len(), Operation::ALL.len());
        for wire in requests {
            let request = OperationRequest::from_json(wire.clone()).unwrap();
            request.validate().unwrap();
            assert_eq!(request.operation().as_str(), wire["operation"]);
            assert_eq!(serde_json::to_value(&request).unwrap(), wire);
            let reparsed: OperationRequest = serde_json::from_value(wire).unwrap();
            assert_eq!(reparsed, request);
        }
    }

    #[test]
    fn reserialization_omits_absent_options_and_keeps_constant_defaults() {
        let request =
            OperationRequest::from_json(json!({"operation":"render","scene":null})).unwrap();
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({"operation":"render","sections":false,"fresh":false})
        );
        let sheet = OperationRequest::from_json(json!({"operation":"contact_sheet"})).unwrap();
        assert_eq!(
            serde_json::to_value(&sheet).unwrap(),
            json!({"operation":"contact_sheet","count":6,"columns":3})
        );
    }

    #[test]
    fn unknown_fields_and_operations_name_the_offender() {
        let (field, reason) = rejection(
            OperationRequest::from_json(json!({"operation":"render","quality":"high"}))
                .unwrap_err(),
        );
        assert_eq!(field.as_deref(), Some("quality"));
        assert!(reason.contains("unknown field"), "{reason}");

        let (field, _) = rejection(
            OperationRequest::from_json(
                json!({"operation":"ingest","sources":[{"path":"/a","kind":"pdf"}]}),
            )
            .unwrap_err(),
        );
        assert_eq!(field.as_deref(), Some("sources[0].kind"));

        match OperationRequest::from_json(json!({"operation":"preview"})).unwrap_err() {
            EngineError::InvalidParams { field, allowed, .. } => {
                assert_eq!(field.as_deref(), Some("operation"));
                assert!(allowed.contains(&"contact_sheet".to_owned()));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn public_path_rule_reports_each_lexical_rejection() {
        for (path, expected) in [
            ("/etc/passwd", "absolute"),
            ("C:/x.py", "absolute"),
            ("scenes\\a.py", "absolute"),
            ("", "absolute"),
            ("scenes/../a.py", "traversal"),
            ("./a.py", "traversal"),
            ("scenes//a.py", "traversal"),
            (".env", "hidden"),
            ("scenes/.cache/a.py", "hidden"),
            (".manim-director/artifacts/x/a.mp4", "hidden"),
        ] {
            let (_, reason) = rejection(check_project_path("file", path, false).unwrap_err());
            assert_eq!(reason, expected, "{path}");
        }
        check_project_path("source", ".manim-director/artifacts/x/a.mp4", true).unwrap();
        let (_, reason) =
            rejection(check_project_path("source", ".manim-director/state.db", true).unwrap_err());
        assert_eq!(reason, "hidden");
        let (_, reason) = rejection(
            check_project_path("source", ".manim-director/artifacts/x/.a.mp4", true).unwrap_err(),
        );
        assert_eq!(reason, "hidden");
    }

    #[test]
    fn field_rules_are_enforced() {
        for (wire, field) in [
            (
                json!({"operation":"render","file":"scenes/main.txt"}),
                "file",
            ),
            (json!({"operation":"frame","at_seconds":-1.0}), "at_seconds"),
            (json!({"operation":"contact_sheet","count":25}), "count"),
            (json!({"operation":"qa","frames":0}), "frames"),
            (json!({"operation":"validate_math","steps":["a"]}), "steps"),
            (
                json!({"operation":"validate_math","steps":["a","b"],"ranges":{"2x":[0.0,1.0]}}),
                "ranges.2x",
            ),
            (
                json!({"operation":"validate_math","steps":["a","b"],"tolerance":0.0}),
                "tolerance",
            ),
            (json!({"operation":"captions","path":"a.txt"}), "path"),
            (
                json!({"operation":"captions","path":"a.vtt","scale":0.0}),
                "scale",
            ),
            (
                json!({"operation":"ingest","sources":[{"path":"notes.md"}]}),
                "sources[0].path",
            ),
            (
                json!({"operation":"ingest","sources":[{"path":"/n.md","id":"a b"}]}),
                "sources[0].id",
            ),
            (
                json!({"operation":"export","format":"mp4","gif_fps":10}),
                "gif_fps",
            ),
            (
                json!({"operation":"export","format":"mp4","output":"output/a.gif"}),
                "output",
            ),
            (
                json!({"operation":"init","scene_template":"x","theme":"y"}),
                "theme",
            ),
        ] {
            let request = OperationRequest::from_json(wire.clone()).unwrap();
            let (got, _) = rejection(request.validate().unwrap_err());
            assert_eq!(got.as_deref(), Some(field), "{wire}");
        }
        let both = OperationRequest::from_json(
            json!({"operation":"diagnose","text":"x","job_id":"7f1c2b0e-0d5e-4b1a-9a57-3c1f7d0c2a11"}),
        )
        .unwrap();
        assert!(both.validate().is_err());
    }
}
