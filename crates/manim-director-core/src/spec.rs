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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<DirectionSpec>,
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
            direction: Some(DirectionSpec::default()),
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
        if let Some(direction) = &self.direction {
            direction.validate()?;
            for (index, beat) in self.storyboard.iter().enumerate() {
                beat.validate_direction_contract(index)?;
            }
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

/// Semantic visual direction consumed by the Python composition runtime.
///
/// This deliberately describes intent and bounded defaults, not Manim geometry.
/// Projects without this section remain valid v1 projects; declaring it opts a
/// storyboard into the directed beat contract validated below.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DirectionSpec {
    #[serde(default)]
    pub composition: CompositionDirectionSpec,
    #[serde(default)]
    pub typography: TypographyDirectionSpec,
    #[serde(default)]
    pub motion: MotionDirectionSpec,
    #[serde(default)]
    pub narrative: NarrativeDirectionSpec,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for DirectionSpec {
    fn default() -> Self {
        Self {
            composition: CompositionDirectionSpec::default(),
            typography: TypographyDirectionSpec::default(),
            motion: MotionDirectionSpec::default(),
            narrative: NarrativeDirectionSpec::default(),
            extra: BTreeMap::new(),
        }
    }
}

impl DirectionSpec {
    fn validate(&self) -> Result<(), SpecError> {
        if !(1..=8).contains(&self.composition.max_active) {
            return Err(SpecError::Invalid(
                "direction.composition.max_active must be between 1 and 8".into(),
            ));
        }
        for (role, size) in self.typography.scale.roles() {
            if !size.is_finite() || !(8.0..=120.0).contains(size) {
                return Err(SpecError::Invalid(format!(
                    "direction.typography.scale.{role} must be between 8 and 120"
                )));
            }
        }
        let scale = &self.typography.scale;
        if !(scale.hero >= scale.title
            && scale.title >= scale.section
            && scale.section >= scale.body
            && scale.body >= scale.label
            && scale.label >= scale.micro)
        {
            return Err(SpecError::Invalid(
                "direction.typography.scale must preserve hero >= title >= section >= body >= label >= micro"
                    .into(),
            ));
        }
        if self.narrative.audience.trim().is_empty() {
            return Err(SpecError::Invalid(
                "direction.narrative.audience cannot be empty".into(),
            ));
        }
        if self.narrative.principle.trim().is_empty() {
            return Err(SpecError::Invalid(
                "direction.narrative.principle cannot be empty".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompositionDensity {
    Spacious,
    Balanced,
    Dense,
}

impl Default for CompositionDensity {
    fn default() -> Self {
        Self::Spacious
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompositionDirectionSpec {
    #[serde(default)]
    pub density: CompositionDensity,
    #[serde(default = "default_max_active")]
    pub max_active: u8,
    #[serde(default = "default_caption_lane")]
    pub caption_lane: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for CompositionDirectionSpec {
    fn default() -> Self {
        Self {
            density: CompositionDensity::default(),
            max_active: default_max_active(),
            caption_lane: default_caption_lane(),
            extra: BTreeMap::new(),
        }
    }
}

fn default_max_active() -> u8 {
    4
}

fn default_caption_lane() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypographyDirectionSpec {
    #[serde(default)]
    pub scale: TypeScaleSpec,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for TypographyDirectionSpec {
    fn default() -> Self {
        Self {
            scale: TypeScaleSpec::default(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypeScaleSpec {
    #[serde(default = "default_type_hero")]
    pub hero: f32,
    #[serde(default = "default_type_title")]
    pub title: f32,
    #[serde(default = "default_type_section")]
    pub section: f32,
    #[serde(default = "default_type_body")]
    pub body: f32,
    #[serde(default = "default_type_math")]
    pub math: f32,
    #[serde(default = "default_type_label")]
    pub label: f32,
    #[serde(default = "default_type_caption")]
    pub caption: f32,
    #[serde(default = "default_type_micro")]
    pub micro: f32,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for TypeScaleSpec {
    fn default() -> Self {
        Self {
            hero: default_type_hero(),
            title: default_type_title(),
            section: default_type_section(),
            body: default_type_body(),
            math: default_type_math(),
            label: default_type_label(),
            caption: default_type_caption(),
            micro: default_type_micro(),
            extra: BTreeMap::new(),
        }
    }
}

impl TypeScaleSpec {
    fn roles(&self) -> [(&'static str, &f32); 8] {
        [
            ("hero", &self.hero),
            ("title", &self.title),
            ("section", &self.section),
            ("body", &self.body),
            ("math", &self.math),
            ("label", &self.label),
            ("caption", &self.caption),
            ("micro", &self.micro),
        ]
    }
}

fn default_type_hero() -> f32 {
    64.0
}
fn default_type_title() -> f32 {
    44.0
}
fn default_type_section() -> f32 {
    36.0
}
fn default_type_body() -> f32 {
    30.0
}
fn default_type_math() -> f32 {
    48.0
}
fn default_type_label() -> f32 {
    24.0
}
fn default_type_caption() -> f32 {
    25.0
}
fn default_type_micro() -> f32 {
    18.0
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationMotion {
    Morph,
    Crossfade,
    Hold,
}

impl Default for ContinuationMotion {
    fn default() -> Self {
        Self::Morph
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContrastMotion {
    Lateral,
    Crossfade,
}

impl Default for ContrastMotion {
    fn default() -> Self {
        Self::Lateral
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RevealMotion {
    Draw,
    Fade,
    Scale,
}

impl Default for RevealMotion {
    fn default() -> Self {
        Self::Draw
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChapterMotion {
    Reset,
    Crossfade,
}

impl Default for ChapterMotion {
    fn default() -> Self {
        Self::Reset
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MotionDirectionSpec {
    #[serde(default)]
    pub continuation: ContinuationMotion,
    #[serde(default)]
    pub contrast: ContrastMotion,
    #[serde(default)]
    pub reveal: RevealMotion,
    #[serde(default)]
    pub chapter: ChapterMotion,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for MotionDirectionSpec {
    fn default() -> Self {
        Self {
            continuation: ContinuationMotion::default(),
            contrast: ContrastMotion::default(),
            reveal: RevealMotion::default(),
            chapter: ChapterMotion::default(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NarrativeDirectionSpec {
    #[serde(default = "default_narrative_audience")]
    pub audience: String,
    #[serde(default = "default_narrative_principle")]
    pub principle: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Default for NarrativeDirectionSpec {
    fn default() -> Self {
        Self {
            audience: default_narrative_audience(),
            principle: default_narrative_principle(),
            extra: BTreeMap::new(),
        }
    }
}

fn default_narrative_audience() -> String {
    "curious general audience".into()
}

fn default_narrative_principle() -> String {
    "one-idea-per-beat".into()
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BeatIntent {
    Introduce,
    Explain,
    Compare,
    Reveal,
    Prove,
    Recap,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Continuation,
    Contrast,
    Reveal,
    Chapter,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoryboardBeat {
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<BeatIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience_question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeaway: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<TransitionKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_metaphor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_active: Option<u8>,
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

impl StoryboardBeat {
    fn validate_direction_contract(&self, index: usize) -> Result<(), SpecError> {
        let label = if self.id.trim().is_empty() {
            format!("storyboard[{index}]")
        } else {
            format!("storyboard beat {:?}", self.id)
        };
        if self.intent.is_none() {
            return Err(SpecError::Invalid(format!(
                "{label} requires intent when direction is enabled"
            )));
        }
        for (field, value) in [
            ("audience_question", self.audience_question.as_deref()),
            ("takeaway", self.takeaway.as_deref()),
            ("focus", self.focus.as_deref()),
            ("visual_metaphor", self.visual_metaphor.as_deref()),
        ] {
            if value.is_none_or(|text| text.trim().is_empty()) {
                return Err(SpecError::Invalid(format!(
                    "{label} requires a non-empty {field} when direction is enabled"
                )));
            }
        }
        if self.transition.is_none() {
            return Err(SpecError::Invalid(format!(
                "{label} requires transition when direction is enabled"
            )));
        }
        if self
            .max_active
            .is_some_and(|value| !(1..=8).contains(&value))
        {
            return Err(SpecError::Invalid(format!(
                "{label}.max_active must be between 1 and 8"
            )));
        }
        Ok(())
    }
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
mod direction_tests {
    use super::*;

    const PROJECT: &str = r#"
version: 1
project:
  name: Direction test
"#;

    #[test]
    fn legacy_v1_storyboards_remain_valid_without_direction() {
        let source = format!(
            "{PROJECT}\nstoryboard:\n  - id: hook\n    objective: Ask the question.\n    visual: A point appears.\n"
        );
        let spec: DirectorSpec = serde_yaml::from_str(&source).unwrap();
        assert!(spec.direction.is_none());
        spec.validate().unwrap();
    }

    #[test]
    fn direction_hydrates_runtime_defaults_and_typed_beat_intent() {
        let source = format!(
            r#"{PROJECT}
direction: {{}}
storyboard:
  - id: microscope
    intent: explain
    audience_question: Why does a nonzero determinant matter?
    takeaway: The map is locally invertible.
    focus: tangent_grid
    transition: continuation
    visual_metaphor: A microscope revealing one intact neighborhood
"#
        );
        let spec: DirectorSpec = serde_yaml::from_str(&source).unwrap();
        spec.validate().unwrap();
        let direction = spec.direction.unwrap();
        assert_eq!(direction.composition.density, CompositionDensity::Spacious);
        assert_eq!(direction.composition.max_active, 4);
        assert!(direction.composition.caption_lane);
        assert_eq!(direction.typography.scale.hero, 64.0);
        assert_eq!(direction.typography.scale.micro, 18.0);
        assert_eq!(direction.motion.continuation, ContinuationMotion::Morph);
        assert_eq!(direction.motion.contrast, ContrastMotion::Lateral);
        assert_eq!(direction.motion.reveal, RevealMotion::Draw);
        assert_eq!(direction.motion.chapter, ChapterMotion::Reset);
        assert_eq!(spec.storyboard[0].intent, Some(BeatIntent::Explain));
        assert_eq!(
            spec.storyboard[0].transition,
            Some(TransitionKind::Continuation)
        );
    }

    #[test]
    fn direction_rejects_unbounded_active_count() {
        let source = format!(
            r#"{PROJECT}
direction:
  composition:
    max_active: 9
  typography:
    scale:
      caption: 4
"#
        );
        let spec: DirectorSpec = serde_yaml::from_str(&source).unwrap();
        let error = spec.validate().unwrap_err().to_string();
        assert!(error.contains("max_active must be between 1 and 8"));
    }

    #[test]
    fn directed_storyboard_requires_the_audience_contract() {
        let source = format!(
            r#"{PROJECT}
direction: {{}}
storyboard:
  - id: hook
    intent: introduce
    transition: reveal
"#
        );
        let spec: DirectorSpec = serde_yaml::from_str(&source).unwrap();
        let error = spec.validate().unwrap_err().to_string();
        assert!(error.contains("requires a non-empty audience_question"));
    }

    #[test]
    fn declared_motion_grammar_only_accepts_semantic_values() {
        let source = format!(
            "{PROJECT}\ndirection:\n  motion:\n    continuation: random_bounce\n"
        );
        assert!(serde_yaml::from_str::<DirectorSpec>(&source).is_err());
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
