use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;
use walkdir::WalkDir;

pub const SPEC_FILE: &str = "director.yaml";

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    #[error("invalid project spec: {0}")]
    Invalid(String),
    #[error("no {SPEC_FILE} found from {0}")]
    NotFound(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorSpec {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<serde_yaml::Value>,
    pub project: ProjectSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<BriefSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<EngineSpec>,
    #[serde(default)]
    pub render: RenderSpec,
    #[serde(default)]
    pub theme: ThemeSpec,
    #[serde(default)]
    pub safe_area: SafeArea,
    #[serde(default)]
    pub captions: CaptionSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<InputSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub storyboard: Vec<StoryboardBeat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<SceneSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narration: Option<NarrationSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessibility: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub themes: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, RenderProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<BudgetSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, serde_yaml::Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

fn default_version() -> u32 {
    1
}

impl DirectorSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: 1,
            schema: Some(serde_yaml::Value::String("manim-director/v1".into())),
            project: ProjectSpec::new(name),
            brief: None,
            engine: Some(EngineSpec::default()),
            render: RenderSpec::default(),
            theme: ThemeSpec::default(),
            safe_area: SafeArea::default(),
            captions: CaptionSpec::default(),
            inputs: None,
            storyboard: Vec::new(),
            scenes: Vec::new(),
            narration: None,
            audio: None,
            assets: None,
            accessibility: None,
            themes: None,
            profiles: BTreeMap::new(),
            validation: None,
            outputs: None,
            budgets: Some(BudgetSpec::default()),
            extensions: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }

    pub fn load(root: impl AsRef<Path>) -> Result<Self, SpecError> {
        let path = root.as_ref().join(SPEC_FILE);
        let source = fs::read_to_string(&path).map_err(|source| SpecError::Read {
            path: path.clone(),
            source,
        })?;
        let mut spec: Self = serde_yaml::from_str(&source).map_err(|source| SpecError::Parse {
            path: path.clone(),
            source,
        })?;
        if spec.project.name.trim().is_empty() {
            spec.project.name = spec
                .project
                .title
                .clone()
                .or_else(|| spec.project.id.clone())
                .unwrap_or_default();
        }
        spec.validate()?;
        Ok(spec)
    }

    pub fn save(&self, root: impl AsRef<Path>) -> Result<(), SpecError> {
        self.validate()?;
        let path = root.as_ref().join(SPEC_FILE);
        let value = serde_yaml::to_string(self).map_err(|source| SpecError::Parse {
            path: path.clone(),
            source,
        })?;
        fs::write(&path, value).map_err(|source| SpecError::Read { path, source })
    }

    pub fn validate(&self) -> Result<(), SpecError> {
        if self.version != 1 {
            return Err(SpecError::Invalid(format!(
                "unsupported version {}; expected 1",
                self.version
            )));
        }
        if self.project.name.trim().is_empty() {
            return Err(SpecError::Invalid("project.name cannot be empty".into()));
        }
        for (label, value) in [
            ("source_dir", &self.project.source_dir),
            ("asset_dir", &self.project.asset_dir),
            ("output_dir", &self.project.output_dir),
            ("media_dir", &self.project.media_dir),
        ] {
            let path = Path::new(value);
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(SpecError::Invalid(format!(
                    "project.{label} must stay inside the project"
                )));
            }
        }
        if self.render.width == 0 || self.render.height == 0 || self.render.fps == 0 {
            return Err(SpecError::Invalid(
                "render width, height, and fps must be positive".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSpec {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    #[serde(default = "default_source_dir")]
    pub source_dir: String,
    #[serde(default = "default_asset_dir")]
    pub asset_dir: String,
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_media_dir")]
    pub media_dir: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl ProjectSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            title: None,
            description: None,
            language: Some("en-US".into()),
            seed: Some(0),
            source_dir: default_source_dir(),
            asset_dir: default_asset_dir(),
            output_dir: default_output_dir(),
            media_dir: default_media_dir(),
            extra: BTreeMap::new(),
        }
    }
}

fn default_source_dir() -> String {
    "scenes".into()
}
fn default_asset_dir() -> String {
    "assets".into()
}
fn default_output_dir() -> String {
    "output".into()
}
fn default_media_dir() -> String {
    ".manim-director/media".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderSpec {
    #[serde(default = "default_renderer")]
    pub renderer: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for RenderSpec {
    fn default() -> Self {
        Self {
            renderer: default_renderer(),
            profile: default_profile(),
            format: default_format(),
            transparent: false,
            width: 1920,
            height: 1080,
            fps: 60,
            extra: BTreeMap::new(),
        }
    }
}

fn default_renderer() -> String {
    "cairo".into()
}
fn default_profile() -> String {
    "preview".into()
}
fn default_format() -> String {
    "mp4".into()
}
fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}
fn default_fps() -> u32 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSpec {
    #[serde(default = "default_preset")]
    pub preset: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_foreground")]
    pub foreground: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default = "default_font")]
    pub font: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for ThemeSpec {
    fn default() -> Self {
        Self {
            preset: default_preset(),
            background: default_background(),
            foreground: default_foreground(),
            accent: default_accent(),
            font: default_font(),
            extra: BTreeMap::new(),
        }
    }
}

fn default_preset() -> String {
    "midnight".into()
}
fn default_background() -> String {
    "#0B1020".into()
}
fn default_foreground() -> String {
    "#F5F7FF".into()
}
fn default_accent() -> String {
    "#69D2FF".into()
}
fn default_font() -> String {
    "Inter".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeArea {
    #[serde(default = "default_safe")]
    pub top: f32,
    #[serde(default = "default_safe")]
    pub right: f32,
    #[serde(default = "default_safe")]
    pub bottom: f32,
    #[serde(default = "default_safe")]
    pub left: f32,
}

impl Default for SafeArea {
    fn default() -> Self {
        Self {
            top: 0.05,
            right: 0.05,
            bottom: 0.05,
            left: 0.05,
        }
    }
}
fn default_safe() -> f32 {
    0.05
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionSpec {
    #[serde(default = "default_caption_format")]
    pub format: String,
    #[serde(default)]
    pub burn_in: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_area_percent: Option<f32>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for CaptionSpec {
    fn default() -> Self {
        Self {
            format: default_caption_format(),
            burn_in: false,
            source: None,
            safe_area_percent: None,
            extra: BTreeMap::new(),
        }
    }
}
fn default_caption_format() -> String {
    "vtt".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BriefSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rigor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_claims: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden_elements: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSpec {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_engine_source")]
    pub source: String,
    #[serde(default = "default_main_scene")]
    pub main_scene: String,
    #[serde(default = "default_compatible")]
    pub compatible: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for EngineSpec {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            source: default_engine_source(),
            main_scene: default_main_scene(),
            compatible: default_compatible(),
            fallback: None,
            extra: BTreeMap::new(),
        }
    }
}
fn default_backend() -> String {
    "manim-ce".into()
}
fn default_engine_source() -> String {
    "scenes/main.py".into()
}
fn default_main_scene() -> String {
    "MainScene".into()
}
fn default_compatible() -> String {
    ">=0.19,<1".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<serde_yaml::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<ClaimSpec>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceSpec {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClaimSpec {
    Text(String),
    Structured {
        #[serde(default)]
        id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        assumptions: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evidence: Option<serde_yaml::Value>,
        #[serde(flatten)]
        extra: BTreeMap<String, serde_yaml::Value>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoryboardBeat {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual: Option<String>,
    #[serde(
        default,
        alias = "duration_seconds",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narration_cue: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneSpec {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "class", skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NarrationSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenderProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSpec {
    #[serde(default = "default_render_budget")]
    pub render_seconds: u64,
    #[serde(default = "default_memory_budget")]
    pub memory_mb: u64,
    #[serde(default = "default_output_budget")]
    pub output_mb: u64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for BudgetSpec {
    fn default() -> Self {
        Self {
            render_seconds: 1800,
            memory_mb: 8192,
            output_mb: 2048,
            extra: BTreeMap::new(),
        }
    }
}
fn default_render_budget() -> u64 {
    1800
}
fn default_memory_budget() -> u64 {
    8192
}
fn default_output_budget() -> u64 {
    2048
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInventory {
    pub root: PathBuf,
    pub spec: DirectorSpec,
    pub source_files: Vec<PathBuf>,
    pub asset_files: Vec<PathBuf>,
    pub output_files: Vec<PathBuf>,
}

impl ProjectInventory {
    pub fn scan(root: impl AsRef<Path>) -> Result<Self, SpecError> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|source| SpecError::Read {
                path: root.as_ref().to_path_buf(),
                source,
            })?;
        let spec = DirectorSpec::load(&root)?;
        let scan = |relative: &str, excluded: &[&str], source_mode: bool| -> Vec<PathBuf> {
            let base = root.join(relative);
            if !base.exists() {
                return Vec::new();
            }
            let excluded = excluded
                .iter()
                .map(|path| root.join(path))
                .filter(|path| path != &base)
                .collect::<Vec<_>>();
            let mut items = WalkDir::new(base)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| {
                    if entry.depth() == 0 {
                        return true;
                    }
                    if excluded
                        .iter()
                        .any(|path| entry.path() == path || entry.path().starts_with(path))
                    {
                        return false;
                    }
                    if !entry.file_type().is_dir() {
                        return true;
                    }
                    let name = entry.file_name().to_string_lossy();
                    if matches!(
                        name.as_ref(),
                        ".git"
                            | ".manim-director"
                            | "__pycache__"
                            | ".venv"
                            | "venv"
                            | "node_modules"
                            | "target"
                            | "partial_movie_files"
                    ) {
                        return false;
                    }
                    !source_mode || !matches!(name.as_ref(), "media" | "output" | "dist")
                })
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_file())
                .filter_map(|entry| entry.path().strip_prefix(&root).ok().map(Path::to_path_buf))
                .collect::<Vec<_>>();
            items.sort();
            items
        };
        Ok(Self {
            source_files: scan(
                &spec.project.source_dir,
                &[
                    &spec.project.asset_dir,
                    &spec.project.output_dir,
                    &spec.project.media_dir,
                ],
                true,
            ),
            asset_files: scan(&spec.project.asset_dir, &[], false),
            output_files: scan(&spec.project.output_dir, &[], false),
            root,
            spec,
        })
    }
}

#[cfg(test)]
mod inventory_tests {
    use super::*;

    #[test]
    fn root_source_inventory_excludes_generated_and_separately_classified_trees() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(
            root.join(SPEC_FILE),
            "version: 1\nproject:\n  name: Demo\n  source_dir: .\n  asset_dir: assets\n  output_dir: dist\n  media_dir: .manim-director/media\n",
        )
        .unwrap();
        for path in [
            "scenes.py",
            "data/values.csv",
            "assets/logo.svg",
            "dist/final.mp4",
            ".manim-director/state.db",
            "media/Tex/cache.svg",
            "__pycache__/scenes.pyc",
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }

        let inventory = ProjectInventory::scan(root).unwrap();
        assert!(inventory.source_files.contains(&PathBuf::from("scenes.py")));
        assert!(inventory
            .source_files
            .contains(&PathBuf::from("data/values.csv")));
        assert!(!inventory
            .source_files
            .iter()
            .any(|path| path.starts_with("assets")
                || path.starts_with("dist")
                || path.starts_with("media")
                || path.starts_with(".manim-director")
                || path.starts_with("__pycache__")));
        assert_eq!(
            inventory.asset_files,
            vec![PathBuf::from("assets/logo.svg")]
        );
        assert_eq!(
            inventory.output_files,
            vec![PathBuf::from("dist/final.mp4")]
        );
    }
}

pub fn find_project(start: impl AsRef<Path>) -> Result<PathBuf, SpecError> {
    let start = start.as_ref();
    let mut current = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };
    loop {
        if current.join(SPEC_FILE).is_file() {
            return current.canonicalize().map_err(|source| SpecError::Read {
                path: current.to_path_buf(),
                source,
            });
        }
        current = current
            .parent()
            .ok_or_else(|| SpecError::NotFound(start.to_path_buf()))?;
    }
}
