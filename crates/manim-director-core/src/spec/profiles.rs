//! Render profiles are owned by the engine: built-ins, `director.yaml`
//! overrides and custom profiles resolve here, once, when the spec loads. The
//! runtime only ever sees the resolved [`RenderSettings`].

use super::SpecError;
use crate::{names::named_enum, EngineError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

named_enum! {
    pub enum Renderer {
        Cairo = "cairo",
        Opengl = "opengl",
    }
}

named_enum! {
    /// Output container. `png` is only produced by `still`.
    pub enum MediaFormat {
        Mp4 = "mp4",
        Mov = "mov",
        Webm = "webm",
        Gif = "gif",
        Png = "png",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSettings {
    pub profile: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub renderer: Renderer,
    pub format: MediaFormat,
    pub transparent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderSpec {
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default = "default_renderer")]
    pub renderer: Renderer,
    #[serde(default = "default_format")]
    pub format: MediaFormat,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
}

impl Default for RenderSpec {
    fn default() -> Self {
        Self {
            profile: default_profile(),
            renderer: default_renderer(),
            format: default_format(),
            transparent: false,
            width: default_width(),
            height: default_height(),
            fps: default_fps(),
        }
    }
}

fn default_profile() -> String {
    "preview".into()
}
fn default_renderer() -> Renderer {
    Renderer::Cairo
}
fn default_format() -> MediaFormat {
    MediaFormat::Mp4
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

/// A `profiles.<name>` entry. `layout` and `scenes` are accepted and ignored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileSpec {
    #[serde(default)]
    pub resolution: Option<[u32; 2]>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub renderer: Option<Renderer>,
    #[serde(default)]
    pub format: Option<MediaFormat>,
    #[serde(default)]
    pub alpha: Option<bool>,
}

const BUILT_IN: [(&str, u32, u32, u32); 4] = [
    ("draft", 854, 480, 15),
    ("preview", 1280, 720, 30),
    ("production", 1920, 1080, 60),
    ("ultra", 3840, 2160, 60),
];

/// Manim's quality names without the `_quality` suffix.
const QUALITY: [(&str, u32, u32, u32); 5] = [
    ("low", 854, 480, 15),
    ("medium", 1280, 720, 30),
    ("high", 1920, 1080, 60),
    ("production", 2560, 1440, 60),
    ("fourk", 3840, 2160, 60),
];

/// The engine's own profile names, `custom` included; any other name comes
/// from `director.yaml`.
pub fn is_builtin_profile(name: &str) -> bool {
    name == "custom" || BUILT_IN.iter().any(|(builtin, ..)| *builtin == name)
}

pub(super) fn resolve_profiles(
    render: &RenderSpec,
    entries: &BTreeMap<String, ProfileSpec>,
) -> Result<Vec<RenderSettings>, SpecError> {
    let custom = ("custom", render.width, render.height, render.fps);
    let built_in = BUILT_IN.iter().copied().chain([custom]);
    let mut resolved = Vec::with_capacity(BUILT_IN.len() + 1 + entries.len());
    for (name, width, height, fps) in built_in {
        resolved.push(resolve_one(
            name,
            Some((width, height, fps)),
            entries.get(name),
            render,
        )?);
    }
    for (name, entry) in entries {
        if !resolved.iter().any(|profile| &profile.profile == name) {
            resolved.push(resolve_one(name, None, Some(entry), render)?);
        }
    }
    Ok(resolved)
}

fn resolve_one(
    name: &str,
    base: Option<(u32, u32, u32)>,
    entry: Option<&ProfileSpec>,
    render: &RenderSpec,
) -> Result<RenderSettings, SpecError> {
    let invalid = |reason: String| SpecError::Invalid(format!("profile {name}: {reason}"));
    let (mut width, mut height, mut fps) = match base {
        Some((width, height, fps)) => (Some(width), Some(height), Some(fps)),
        None => (None, None, None),
    };
    let entry = entry.cloned().unwrap_or_default();
    if let Some(word) = &entry.quality {
        let (_, w, h, f) = QUALITY
            .iter()
            .find(|(quality, ..)| quality == word)
            .ok_or_else(|| {
                let allowed: Vec<_> = QUALITY.iter().map(|(quality, ..)| *quality).collect();
                invalid(format!(
                    "unknown quality {word:?}; use one of {}",
                    allowed.join(", ")
                ))
            })?;
        (width, height, fps) = (Some(*w), Some(*h), Some(*f));
    }
    if let Some([w, h]) = entry.resolution {
        (width, height) = (Some(w), Some(h));
    }
    if let Some(f) = entry.fps {
        fps = Some(f);
    }
    let (Some(width), Some(height), Some(fps)) = (width, height, fps) else {
        return Err(invalid("needs resolution and fps, or quality".into()));
    };
    for (axis, value) in [("width", width), ("height", height)] {
        if !(16..=8192).contains(&value) || value % 2 != 0 {
            return Err(invalid(format!(
                "{axis} {value} must be an even number between 16 and 8192"
            )));
        }
    }
    if !(1..=240).contains(&fps) {
        return Err(invalid(format!("fps {fps} must be between 1 and 240")));
    }
    let format = entry.format.unwrap_or(render.format);
    if format == MediaFormat::Png {
        return Err(invalid("format png is reserved for stills".into()));
    }
    let transparent = entry.alpha.unwrap_or(render.transparent);
    if transparent && !matches!(format, MediaFormat::Mov | MediaFormat::Webm) {
        return Err(invalid(format!(
            "transparency needs format mov or webm, not {format}"
        )));
    }
    Ok(RenderSettings {
        profile: name.to_owned(),
        width,
        height,
        fps,
        renderer: entry.renderer.unwrap_or(render.renderer),
        format,
        transparent,
    })
}

impl super::DirectorSpec {
    /// Picks the profile for a render or still. An unknown requested name is a
    /// bad parameter; an unknown `render.profile` default is a bad spec.
    pub fn select_profile(&self, requested: Option<&str>) -> Result<&RenderSettings, EngineError> {
        let name = requested.unwrap_or(&self.render.profile);
        if let Some(profile) = self.profile(name) {
            return Ok(profile);
        }
        Err(match requested {
            Some(_) => EngineError::invalid_choice(
                "profile",
                format!("unknown profile {name:?}"),
                self.profiles()
                    .iter()
                    .map(|profile| profile.profile.clone()),
            ),
            None => EngineError::InvalidSpec {
                reason: format!("render.profile {name:?} names no profile"),
                line: None,
                column: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{DirectorSpec, EngineError, MediaFormat, Renderer};

    fn spec(extra: &str) -> Result<DirectorSpec, crate::SpecError> {
        DirectorSpec::parse(&format!("version: 1\nproject:\n  name: Demo\n{extra}"))
    }

    fn dims(spec: &DirectorSpec, name: &str) -> (u32, u32, u32) {
        let profile = spec.profile(name).unwrap();
        (profile.width, profile.height, profile.fps)
    }

    #[test]
    fn built_ins_resolve_in_table_order_with_custom_from_render() {
        let spec = spec("render:\n  width: 640\n  height: 360\n  fps: 24\n").unwrap();
        let names: Vec<_> = spec.profiles().iter().map(|p| p.profile.as_str()).collect();
        assert_eq!(names, ["draft", "preview", "production", "ultra", "custom"]);
        assert_eq!(dims(&spec, "draft"), (854, 480, 15));
        assert_eq!(dims(&spec, "preview"), (1280, 720, 30));
        assert_eq!(dims(&spec, "production"), (1920, 1080, 60));
        assert_eq!(dims(&spec, "ultra"), (3840, 2160, 60));
        assert_eq!(dims(&spec, "custom"), (640, 360, 24));
        let preview = spec.profile("preview").unwrap();
        assert_eq!(preview.renderer, Renderer::Cairo);
        assert_eq!(preview.format, MediaFormat::Mp4);
        assert!(!preview.transparent);
    }

    #[test]
    fn overrides_replace_built_ins_in_place_and_extras_sort_after() {
        let spec = spec(
            "profiles:\n  zeta: {quality: low}\n  alpha: {resolution: [100, 50], fps: 12}\n  preview: {fps: 24, format: webm, alpha: true}\n",
        )
        .unwrap();
        let names: Vec<_> = spec.profiles().iter().map(|p| p.profile.as_str()).collect();
        assert_eq!(
            names,
            [
                "draft",
                "preview",
                "production",
                "ultra",
                "custom",
                "alpha",
                "zeta"
            ]
        );
        assert_eq!(dims(&spec, "preview"), (1280, 720, 24));
        assert!(spec.profile("preview").unwrap().transparent);
        assert_eq!(dims(&spec, "alpha"), (100, 50, 12));
        assert_eq!(dims(&spec, "zeta"), (854, 480, 15));
    }

    #[test]
    fn quality_words_are_manim_names_and_resolution_wins_over_them() {
        let spec = spec("profiles:\n  big: {quality: fourk}\n  wide: {quality: production, resolution: [2000, 1000]}\n").unwrap();
        assert_eq!(dims(&spec, "big"), (3840, 2160, 60));
        assert_eq!(dims(&spec, "wide"), (2000, 1000, 60));
    }

    #[test]
    fn invalid_profile_entries_make_the_spec_invalid() {
        for (entry, needle) in [
            ("profiles:\n  x: {quality: ultra_hd}\n", "unknown quality"),
            (
                "profiles:\n  x: {resolution: [640, 360]}\n",
                "needs resolution and fps",
            ),
            (
                "profiles:\n  x: {resolution: [641, 360], fps: 30}\n",
                "even number",
            ),
            (
                "profiles:\n  x: {quality: low, fps: 300}\n",
                "between 1 and 240",
            ),
            (
                "profiles:\n  x: {quality: low, alpha: true}\n",
                "mov or webm",
            ),
            (
                "profiles:\n  x: {quality: low, format: png}\n",
                "reserved for stills",
            ),
            ("render:\n  width: 10\n", "profile custom"),
        ] {
            let error = spec(entry).unwrap_err().to_string();
            assert!(error.contains(needle), "{entry}: {error}");
        }
    }

    #[test]
    fn selection_distinguishes_bad_params_from_a_bad_default() {
        let spec = spec("render:\n  profile: cinema\n").unwrap();
        assert_eq!(spec.select_profile(Some("draft")).unwrap().width, 854);
        match spec.select_profile(Some("cinema")).unwrap_err() {
            EngineError::InvalidParams { field, allowed, .. } => {
                assert_eq!(field.as_deref(), Some("profile"));
                assert!(allowed.contains(&"custom".to_owned()));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            spec.select_profile(None).unwrap_err().code(),
            "invalid_spec"
        );
    }
}
