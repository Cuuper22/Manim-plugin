//! Resolved tasks: what the runtime executes. Paths are absolute; every
//! default is filled in by the engine.

use crate::{names::named_enum, Operation, RenderSettings, SafeArea};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

named_enum! {
    pub enum InitMode {
        Create = "create",
        Overwrite = "overwrite",
        AddScene = "add_scene",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitTask {
    pub mode: InitMode,
    pub name: Option<String>,
    pub template: Option<String>,
    pub scene_template: Option<String>,
    pub theme: Option<String>,
    pub seed: Option<u32>,
    /// Set iff `mode` is `add_scene`.
    pub source_dir: Option<PathBuf>,
    /// `add_scene` only: overwrite an existing scene file. (Create mode says
    /// the same with `overwrite`.)
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverTask {
    pub files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoctorTask {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderTask {
    pub scene: Option<String>,
    pub files: Vec<PathBuf>,
    pub settings: RenderSettings,
    pub media_dir: PathBuf,
    pub out_dir: PathBuf,
    pub sections: bool,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StillTask {
    pub scene: Option<String>,
    pub files: Vec<PathBuf>,
    pub settings: RenderSettings,
    pub media_dir: PathBuf,
    pub out_dir: PathBuf,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameTask {
    pub video: PathBuf,
    pub at_seconds: f64,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactSheetTask {
    pub video: PathBuf,
    pub count: u8,
    pub columns: u8,
    pub timeline: Option<PathBuf>,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Video,
    Image,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QaTask {
    pub source: PathBuf,
    pub source_kind: SourceKind,
    pub frames: u8,
    pub safe_area: SafeArea,
    pub timeline: Option<PathBuf>,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnoseTask {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateMathTask {
    pub steps: Vec<String>,
    pub ranges: BTreeMap<String, [f64; 2]>,
    pub samples: u32,
    pub tolerance: f64,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptionsTask {
    pub path: PathBuf,
    pub shift_seconds: f64,
    pub scale: f64,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestTask {
    pub sources: Vec<IngestTaskSource>,
    pub normalize: bool,
    pub force: bool,
    pub manifest: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestTaskSource {
    pub path: PathBuf,
    pub kind: String,
    pub destination_dir: PathBuf,
    pub id: Option<String>,
    pub license: Option<String>,
    pub attribution: Option<String>,
}

/// `export` is discriminated by `format`: a zip bundle or one media file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExportTask {
    Zip(ZipExportTask),
    Media(MediaExportTask),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZipFormat {
    Zip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZipExportTask {
    pub format: ZipFormat,
    pub output: PathBuf,
    pub project_name: String,
    pub source_job_id: Option<Uuid>,
    pub entries: Vec<ExportEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportEntry {
    pub path: PathBuf,
    pub archive_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaExportFormat {
    Mp4,
    Webm,
    Gif,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaExportTask {
    pub format: MediaExportFormat,
    pub output: PathBuf,
    pub source: PathBuf,
    pub alpha: bool,
    pub gif: Option<GifSettings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GifSettings {
    pub fps: u32,
    pub width: u32,
}

/// One task per operation; serialized as the bare task object (the job's
/// operation selects the variant when reading it back).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Task {
    Init(InitTask),
    Discover(DiscoverTask),
    Doctor(DoctorTask),
    Render(RenderTask),
    Still(StillTask),
    Frame(FrameTask),
    ContactSheet(ContactSheetTask),
    Qa(QaTask),
    Diagnose(DiagnoseTask),
    ValidateMath(ValidateMathTask),
    Captions(CaptionsTask),
    Ingest(IngestTask),
    Export(ExportTask),
}

impl Task {
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

    pub fn from_json(operation: Operation, value: Value) -> serde_json::Result<Self> {
        fn parse<T: DeserializeOwned>(value: Value) -> serde_json::Result<T> {
            serde_json::from_value(value)
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

    /// The engine-owned output directory, for operations that have one.
    pub fn out_dir(&self) -> Option<&PathBuf> {
        match self {
            Self::Render(task) => Some(&task.out_dir),
            Self::Still(task) => Some(&task.out_dir),
            Self::Frame(task) => Some(&task.out_dir),
            Self::ContactSheet(task) => Some(&task.out_dir),
            Self::Qa(task) => Some(&task.out_dir),
            _ => None,
        }
    }

    /// The task as it enters the cache fingerprint: `out_dir` and `fresh`
    /// never change what a render produces.
    pub fn cache_identity(&self) -> Task {
        let mut task = self.clone();
        match &mut task {
            Self::Render(render) => {
                render.out_dir = PathBuf::new();
                render.fresh = false;
            }
            Self::Still(still) => {
                still.out_dir = PathBuf::new();
                still.fresh = false;
            }
            _ => {}
        }
        task
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MediaFormat, Renderer};
    use serde_json::json;

    fn settings() -> RenderSettings {
        RenderSettings {
            profile: "preview".into(),
            width: 1280,
            height: 720,
            fps: 30,
            renderer: Renderer::Cairo,
            format: MediaFormat::Mp4,
            transparent: false,
        }
    }

    #[test]
    fn render_task_matches_the_contract_wire_shape() {
        let task = Task::Render(RenderTask {
            scene: Some("Recurrence".into()),
            files: vec!["/p/scenes/main.py".into()],
            settings: settings(),
            media_dir: "/p/.manim-director/media".into(),
            out_dir: "/p/.manim-director/artifacts/7f1c".into(),
            sections: false,
            fresh: false,
        });
        let wire = json!({"scene":"Recurrence","files":["/p/scenes/main.py"],
            "settings":{"profile":"preview","width":1280,"height":720,"fps":30,"renderer":"cairo","format":"mp4","transparent":false},
            "media_dir":"/p/.manim-director/media","out_dir":"/p/.manim-director/artifacts/7f1c","sections":false,"fresh":false});
        assert_eq!(serde_json::to_value(&task).unwrap(), wire);
        assert_eq!(Task::from_json(Operation::Render, wire).unwrap(), task);
    }

    #[test]
    fn export_tasks_are_discriminated_by_format() {
        let zip = json!({"format":"zip","output":"/p/output/film.zip","project_name":"Film","source_job_id":null,
            "entries":[{"path":"/p/director.yaml","archive_path":"director.yaml"}]});
        let media = json!({"format":"gif","output":"/p/output/A.gif","source":"/p/a.mp4","alpha":false,"gif":{"fps":15,"width":960}});
        assert!(matches!(
            Task::from_json(Operation::Export, zip.clone()).unwrap(),
            Task::Export(ExportTask::Zip(_))
        ));
        let parsed = Task::from_json(Operation::Export, media.clone()).unwrap();
        assert!(matches!(parsed, Task::Export(ExportTask::Media(_))));
        assert_eq!(serde_json::to_value(parsed).unwrap(), media);
    }

    #[test]
    fn cache_identity_ignores_out_dir_and_fresh() {
        let make = |out: &str, fresh: bool| {
            Task::Still(StillTask {
                scene: None,
                files: vec![],
                settings: settings(),
                media_dir: "/m".into(),
                out_dir: out.into(),
                fresh,
            })
        };
        assert_eq!(
            make("/a", true).cache_identity(),
            make("/b", false).cache_identity()
        );
    }
}
