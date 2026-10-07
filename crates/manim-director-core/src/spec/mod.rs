//! The `director.yaml` surface the engine reads (OPS §1.11). Keys outside it
//! are ignored, never validated.

mod inventory;
mod profiles;
mod storyboard;

pub use inventory::*;
pub use profiles::*;
pub use storyboard::*;

use crate::{files::IgnoreSet, EngineError};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Component, Path, PathBuf},
};

pub const SPEC_FILE: &str = "director.yaml";

#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("no {SPEC_FILE} in {} or any parent directory", .0.display())]
    NotFound(PathBuf),
    #[error("could not read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{message}")]
    Parse {
        message: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    #[error("{0}")]
    Invalid(String),
}

impl From<SpecError> for EngineError {
    fn from(error: SpecError) -> Self {
        let (line, column) = match &error {
            SpecError::Parse { line, column, .. } => (*line, *column),
            _ => (None, None),
        };
        let reason = match &error {
            SpecError::NotFound(_) => error.to_string(),
            SpecError::Read { source, .. } if source.kind() == io::ErrorKind::NotFound => {
                format!("{SPEC_FILE} is missing")
            }
            SpecError::Read { source, .. } => format!("could not read {SPEC_FILE}: {source}"),
            SpecError::Parse { message, .. } => format!("{SPEC_FILE}: {message}"),
            SpecError::Invalid(reason) => format!("{SPEC_FILE}: {reason}"),
        };
        EngineError::InvalidSpec {
            reason,
            line,
            column,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorSpec {
    #[serde(default = "spec_version")]
    pub version: u32,
    pub project: ProjectSpec,
    #[serde(default)]
    pub brief: BriefSpec,
    #[serde(default)]
    pub engine: EngineSpec,
    #[serde(default)]
    pub render: RenderSpec,
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileSpec>,
    #[serde(default)]
    pub theme: Option<ThemeSetting>,
    #[serde(default)]
    pub safe_area: SafeArea,
    #[serde(default)]
    pub scenes: Vec<SceneSpec>,
    #[serde(default)]
    pub storyboard: Vec<StoryboardBeat>,
    #[serde(default)]
    pub budgets: BudgetSpec,
    #[serde(default)]
    pub inputs: InputSpec,
    #[serde(default)]
    pub captions: CaptionSpec,
    #[serde(default)]
    pub narration: NarrationSpec,
    #[serde(skip)]
    resolved_profiles: Vec<RenderSettings>,
}

fn spec_version() -> u32 {
    1
}

impl DirectorSpec {
    pub fn load(root: impl AsRef<Path>) -> Result<Self, SpecError> {
        let path = root.as_ref().join(SPEC_FILE);
        let source =
            fs::read_to_string(&path).map_err(|source| SpecError::Read { path, source })?;
        Self::parse(&source)
    }

    /// The defaults of every key, for the operations that tolerate a missing
    /// or invalid `director.yaml`.
    pub fn defaults() -> Self {
        Self::parse("version: 1\nproject:\n  name: project\n").expect("the defaults are valid")
    }

    /// Parses, validates and resolves every profile; a spec that loads is usable.
    pub fn parse(source: &str) -> Result<Self, SpecError> {
        let mut spec: Self = serde_yaml::from_str(source).map_err(|error| {
            let location = error.location();
            SpecError::Parse {
                message: error.to_string(),
                line: location.as_ref().map(|at| at.line() as u32),
                column: location.as_ref().map(|at| at.column() as u32),
            }
        })?;
        spec.validate()?;
        spec.resolved_profiles = resolve_profiles(&spec.render, &spec.profiles)?;
        Ok(spec)
    }

    fn validate(&self) -> Result<(), SpecError> {
        if self.version != 1 {
            return Err(SpecError::Invalid(format!(
                "version must be 1, found {}",
                self.version
            )));
        }
        if self.project.name.trim().is_empty() {
            return Err(SpecError::Invalid("project.name cannot be empty".into()));
        }
        for (key, value) in self.project.dirs() {
            if !stays_inside(value) {
                return Err(SpecError::Invalid(format!(
                    "project.{key} must be a relative path inside the project"
                )));
            }
        }
        if let Some(source) = &self.engine.source {
            if !stays_inside(source) {
                return Err(SpecError::Invalid(
                    "engine.source must be a relative path inside the project".into(),
                ));
            }
        }
        self.safe_area.validate()?;
        if let Some(index) = self
            .scenes
            .iter()
            .position(|scene| scene.id.trim().is_empty())
        {
            return Err(SpecError::Invalid(format!(
                "scenes[{index}].id cannot be empty"
            )));
        }
        if let Some(index) = self
            .storyboard
            .iter()
            .position(|beat| beat.id.trim().is_empty())
        {
            return Err(SpecError::Invalid(format!(
                "storyboard[{index}].id cannot be empty"
            )));
        }
        Ok(())
    }

    /// Every resolvable profile: built-ins in table order (project overrides in
    /// place), then project profiles sorted by name.
    pub fn profiles(&self) -> &[RenderSettings] {
        &self.resolved_profiles
    }

    pub fn profile(&self, name: &str) -> Option<&RenderSettings> {
        self.resolved_profiles
            .iter()
            .find(|profile| profile.profile == name)
    }

    pub fn theme_name(&self) -> Option<&str> {
        match self.theme.as_ref()? {
            ThemeSetting::Name(name) => Some(name),
            ThemeSetting::Legacy { preset } => preset.as_deref(),
        }
    }

    pub fn ignore_set(&self, root: &Path) -> IgnoreSet {
        IgnoreSet::new(root, &[&self.project.media_dir, &self.project.output_dir])
    }
}

fn stays_inside(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSpec {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default = "default_source_dir")]
    pub source_dir: String,
    #[serde(default = "default_asset_dir")]
    pub asset_dir: String,
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_media_dir")]
    pub media_dir: String,
}

impl ProjectSpec {
    fn dirs(&self) -> [(&'static str, &str); 4] {
        [
            ("source_dir", &self.source_dir),
            ("asset_dir", &self.asset_dir),
            ("output_dir", &self.output_dir),
            ("media_dir", &self.media_dir),
        ]
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
    crate::files::DEFAULT_MEDIA_DIR.into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BriefSpec {
    #[serde(default)]
    pub duration_seconds: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineSpec {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub main_scene: Option<String>,
}

/// `theme: <name>`, or the legacy `theme: {preset: <name>, …}` mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ThemeSetting {
    Name(String),
    Legacy {
        #[serde(default)]
        preset: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SafeArea {
    #[serde(default = "default_safe_side")]
    pub top: f64,
    #[serde(default = "default_safe_side")]
    pub right: f64,
    #[serde(default = "default_safe_bottom")]
    pub bottom: f64,
    #[serde(default = "default_safe_side")]
    pub left: f64,
}

impl Default for SafeArea {
    fn default() -> Self {
        Self {
            top: default_safe_side(),
            right: default_safe_side(),
            bottom: default_safe_bottom(),
            left: default_safe_side(),
        }
    }
}

fn default_safe_side() -> f64 {
    0.05
}
fn default_safe_bottom() -> f64 {
    0.08
}

impl SafeArea {
    fn validate(&self) -> Result<(), SpecError> {
        for (side, value) in [
            ("top", self.top),
            ("right", self.right),
            ("bottom", self.bottom),
            ("left", self.left),
        ] {
            if !(0.0..=0.45).contains(&value) {
                return Err(SpecError::Invalid(format!(
                    "safe_area.{side} must be between 0 and 0.45"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSpec {
    pub id: String,
    #[serde(default, rename = "class")]
    pub class_name: Option<String>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetSpec {
    #[serde(default)]
    pub render_seconds: Option<u64>,
    #[serde(default)]
    pub output_mb: Option<u64>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputSpec {
    #[serde(default)]
    pub data: Vec<String>,
    #[serde(default)]
    pub sources: Vec<InputSource>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputSource {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptionSpec {
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NarrationSpec {
    #[serde(default)]
    pub manifest: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "version: 1\nproject:\n  name: Demo\n";

    #[test]
    fn minimal_spec_gets_contract_defaults() {
        let spec = DirectorSpec::parse(MINIMAL).unwrap();
        assert_eq!(spec.project.source_dir, "scenes");
        assert_eq!(spec.project.media_dir, ".manim-director/media");
        assert_eq!(spec.render.profile, "preview");
        assert_eq!(spec.budgets.memory_mb, None);
        assert!(spec.engine.main_scene.is_none());
        assert_eq!(spec.theme_name(), None);
    }

    #[test]
    fn safe_area_default_matches_the_contract() {
        let defaults = SafeArea::default();
        assert_eq!(
            (defaults.top, defaults.right, defaults.bottom, defaults.left),
            (0.05, 0.05, 0.08, 0.05)
        );
        let partial = DirectorSpec::parse(&format!("{MINIMAL}safe_area:\n  top: 0.1\n")).unwrap();
        assert_eq!(partial.safe_area.top, 0.1);
        assert_eq!(partial.safe_area.bottom, 0.08);
        let error = DirectorSpec::parse(&format!("{MINIMAL}safe_area:\n  left: 0.5\n"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("safe_area.left"), "{error}");
    }

    #[test]
    fn theme_accepts_a_name_or_the_legacy_mapping() {
        let named = DirectorSpec::parse(&format!("{MINIMAL}theme: midnight\n")).unwrap();
        assert_eq!(named.theme_name(), Some("midnight"));
        let legacy = DirectorSpec::parse(&format!(
            "{MINIMAL}theme:\n  preset: paper\n  background: '#fff'\n"
        ))
        .unwrap();
        assert_eq!(legacy.theme_name(), Some("paper"));
    }

    #[test]
    fn keys_outside_the_surface_are_ignored() {
        let source = format!(
            "{MINIMAL}direction:\n  composition: {{max_active: 99}}\nstoryboard:\n  - id: hook\n    objective: ignored\n    max_active: 99\naccessibility: {{}}\n"
        );
        let spec = DirectorSpec::parse(&source).unwrap();
        assert_eq!(spec.storyboard[0].id, "hook");
    }

    #[test]
    fn type_errors_carry_the_yaml_location() {
        let error =
            DirectorSpec::parse("version: 1\nproject:\n  name: Demo\nrender:\n  fps: fast\n")
                .unwrap_err();
        match EngineError::from(error) {
            EngineError::InvalidSpec { line, .. } => assert_eq!(line, Some(5)),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn invalid_versions_names_and_dirs_are_rejected() {
        for (source, needle) in [
            ("version: 2\nproject:\n  name: Demo\n", "version must be 1"),
            ("version: 1\nproject:\n  name: ' '\n", "project.name"),
            (
                "version: 1\nproject:\n  name: Demo\n  output_dir: ../out\n",
                "project.output_dir",
            ),
            (
                "version: 1\nproject:\n  name: Demo\nstoryboard:\n  - id: ''\n",
                "storyboard[0].id",
            ),
        ] {
            let error = DirectorSpec::parse(source).unwrap_err().to_string();
            assert!(error.contains(needle), "{error}");
        }
    }

    #[test]
    fn missing_file_is_an_invalid_spec_naming_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let error = EngineError::from(DirectorSpec::load(directory.path()).unwrap_err());
        assert_eq!(error.code(), "invalid_spec");
        assert!(error.to_string().contains("director.yaml is missing"));
    }
}
