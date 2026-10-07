//! Views read straight from `director.yaml` and the runtime catalog: spec
//! status, project summary, profiles and themes (HTTP §6.1.1–§6.1.4).

use manim_director_core::{
    is_builtin_profile, Catalog, DirectorSpec, EngineError, MediaFormat, ProjectInventory,
    Renderer, SpecError, SPEC_FILE,
};
use serde::Serialize;
use std::{
    fs, io,
    path::{Component, Path},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpecStatus {
    pub path: &'static str,
    pub valid: bool,
    /// blake3 of the file; `None` when it is missing or unreadable.
    pub revision: Option<String>,
    pub error: Option<SpecProblem>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpecProblem {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// `director.yaml` as the views see it: its status now, and the spec every
/// other section derives from.
#[derive(Debug, Clone)]
pub struct SpecSnapshot {
    pub status: SpecStatus,
    /// The last valid spec this process saw, else the defaults.
    pub spec: Arc<DirectorSpec>,
    /// `false` while no valid spec was ever seen: the project is then named
    /// after its directory.
    pub seen_valid: bool,
}

/// Remembers the last valid spec so an invalid edit never blanks the views.
#[derive(Debug, Default)]
pub struct SpecTracker {
    last_valid: Option<Arc<DirectorSpec>>,
}

impl SpecTracker {
    /// Reads `director.yaml` once. Blocking.
    pub fn load(&mut self, root: &Path) -> SpecSnapshot {
        let path = root.join(SPEC_FILE);
        let bytes = fs::read(&path);
        let revision = bytes
            .as_ref()
            .ok()
            .map(|bytes| blake3::hash(bytes).to_hex().to_string());
        let parsed = bytes
            .and_then(|bytes| {
                String::from_utf8(bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
            })
            .map_err(|source| SpecError::Read { path, source })
            .and_then(|text| DirectorSpec::parse(&text));
        let error = match parsed {
            Ok(spec) => {
                self.last_valid = Some(Arc::new(spec));
                None
            }
            Err(error) => {
                let (line, column) = match &error {
                    SpecError::Parse { line, column, .. } => (*line, *column),
                    _ => (None, None),
                };
                // The same reason a submit that needs the spec reports.
                let message = EngineError::from(error).to_string();
                Some(SpecProblem {
                    message,
                    line,
                    column,
                })
            }
        };
        SpecSnapshot {
            status: SpecStatus {
                path: SPEC_FILE,
                valid: error.is_none(),
                revision,
                error,
            },
            spec: self.current(),
            seen_valid: self.last_valid.is_some(),
        }
    }

    /// The spec the views derive from, without re-reading the file.
    pub fn current(&self) -> Arc<DirectorSpec> {
        self.last_valid
            .clone()
            .unwrap_or_else(|| Arc::new(DirectorSpec::defaults()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectSummary {
    pub root: String,
    pub name: String,
    pub description: Option<String>,
    pub spec_file: &'static str,
    pub source_dir: String,
    pub asset_dir: String,
    pub output_dir: String,
    pub media_dir: String,
    pub theme: Option<String>,
    pub default_profile: String,
    pub duration_seconds: Option<f64>,
    pub counts: FileCounts,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileCounts {
    pub sources: usize,
    pub assets: usize,
    pub outputs: usize,
}

/// Blocking (walks the source, asset and output trees).
pub fn project_summary(
    root: &Path,
    snapshot: &SpecSnapshot,
    catalog: Option<&Catalog>,
) -> ProjectSummary {
    let spec = &snapshot.spec;
    let project = &spec.project;
    let name = match snapshot.seen_valid {
        true => project
            .title
            .clone()
            .unwrap_or_else(|| project.name.clone()),
        false => root.file_name().map_or_else(
            || "project".into(),
            |name| name.to_string_lossy().into_owned(),
        ),
    };
    let inventory = ProjectInventory::scan(root, spec);
    let declared: Option<Vec<f64>> = spec.storyboard.iter().map(|beat| beat.duration).collect();
    let storyboard_total = declared
        .filter(|durations| !durations.is_empty())
        .map(|durations| durations.iter().sum());
    ProjectSummary {
        root: root.to_string_lossy().into_owned(),
        name,
        description: project.description.clone(),
        spec_file: SPEC_FILE,
        source_dir: directory(&project.source_dir),
        asset_dir: directory(&project.asset_dir),
        output_dir: directory(&project.output_dir),
        media_dir: directory(&project.media_dir),
        theme: project_theme(spec, catalog),
        default_profile: spec.render.profile.clone(),
        duration_seconds: spec.brief.duration_seconds.or(storyboard_total),
        counts: FileCounts {
            sources: inventory.source_files.len(),
            assets: inventory.asset_files.len(),
            outputs: inventory.output_files.len(),
        },
    }
}

/// A spec directory as a `ProjectPath`: `"."` for the root, no `./`.
fn directory(value: &str) -> String {
    let parts: Vec<_> = Path::new(value)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect();
    match parts.is_empty() {
        true => ".".into(),
        false => parts.join("/"),
    }
}

/// OPS §1.9: the spec's theme, else the runtime's default (first) theme.
pub fn project_theme(spec: &DirectorSpec, catalog: Option<&Catalog>) -> Option<String> {
    spec.theme_name().map(str::to_owned).or_else(|| {
        catalog
            .and_then(|catalog| catalog.themes.first())
            .map(|theme| theme.name.clone())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileOrigin {
    Builtin,
    Project,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileView {
    pub name: String,
    pub origin: ProfileOrigin,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub renderer: Renderer,
    pub format: MediaFormat,
    pub transparent: bool,
    pub is_default: bool,
}

/// OPS `profiles(spec)` in its order; HTTP never resolves profiles itself.
pub fn profiles(spec: &DirectorSpec) -> Vec<ProfileView> {
    spec.profiles()
        .iter()
        .map(|settings| {
            let name = &settings.profile;
            ProfileView {
                name: name.clone(),
                origin: match is_builtin_profile(name) && !spec.profiles.contains_key(name) {
                    true => ProfileOrigin::Builtin,
                    false => ProfileOrigin::Project,
                },
                width: settings.width,
                height: settings.height,
                fps: settings.fps,
                renderer: settings.renderer,
                format: settings.format,
                transparent: settings.transparent,
                is_default: *name == spec.render.profile,
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeView {
    pub name: String,
    pub tokens: Vec<ThemeToken>,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeToken {
    pub token: String,
    pub color: String,
}

/// The catalog's themes in catalog order; empty until a runtime reported one.
pub fn themes(catalog: Option<&Catalog>, project_theme: Option<&str>) -> Vec<ThemeView> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    catalog
        .themes
        .iter()
        .map(|theme| ThemeView {
            name: theme.name.clone(),
            tokens: theme
                .tokens
                .iter()
                .map(|(token, color)| ThemeToken {
                    token: token.clone(),
                    color: color.clone(),
                })
                .collect(),
            is_default: Some(theme.name.as_str()) == project_theme,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(source: &str) -> (tempfile::TempDir, SpecSnapshot, SpecTracker) {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(SPEC_FILE), source).unwrap();
        let mut tracker = SpecTracker::default();
        let snapshot = tracker.load(directory.path());
        (directory, snapshot, tracker)
    }

    #[test]
    fn an_invalid_spec_keeps_the_last_valid_one_and_reports_its_line() {
        let (directory, first, mut tracker) =
            snapshot("version: 1\nproject:\n  name: Demo\n  title: The Demo\n");
        assert!(first.status.valid);
        fs::write(
            directory.path().join(SPEC_FILE),
            "version: 1\nproject:\n  name: Demo\nrender:\n  fps: fast\n",
        )
        .unwrap();
        let broken = tracker.load(directory.path());
        assert!(!broken.status.valid);
        assert_eq!(broken.status.error.as_ref().unwrap().line, Some(5));
        assert!(broken.status.revision.is_some());
        assert_eq!(broken.spec.project.title.as_deref(), Some("The Demo"));
        assert!(broken.seen_valid);
    }

    #[test]
    fn without_any_valid_spec_the_project_is_named_after_its_directory() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = SpecTracker::default().load(directory.path());
        assert!(!snapshot.status.valid);
        assert_eq!(snapshot.status.revision, None);
        let summary = project_summary(directory.path(), &snapshot, None);
        assert_eq!(
            summary.name,
            directory.path().file_name().unwrap().to_string_lossy()
        );
        assert_eq!(summary.media_dir, ".manim-director/media");
    }

    #[test]
    fn duration_comes_from_the_brief_or_a_fully_timed_storyboard() {
        let (directory, timed, _) = snapshot(
            "version: 1\nproject:\n  name: D\n  source_dir: ./\nstoryboard:\n  - {id: a, duration: 2}\n  - {id: b, duration: 3.5}\n",
        );
        let summary = project_summary(directory.path(), &timed, None);
        assert_eq!(summary.duration_seconds, Some(5.5));
        assert_eq!(summary.source_dir, ".");
        let (directory, partial, _) = snapshot(
            "version: 1\nproject:\n  name: D\nstoryboard:\n  - {id: a, duration: 2}\n  - {id: b}\n",
        );
        assert_eq!(
            project_summary(directory.path(), &partial, None).duration_seconds,
            None
        );
    }

    #[test]
    fn profiles_mark_their_origin_and_the_default() {
        let spec = DirectorSpec::parse(
            "version: 1\nproject:\n  name: D\nrender:\n  profile: square\nprofiles:\n  draft: {fps: 10}\n  square: {resolution: [1080, 1080], fps: 30}\n",
        )
        .unwrap();
        let views = profiles(&spec);
        let names: Vec<_> = views.iter().map(|view| view.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "draft",
                "preview",
                "production",
                "ultra",
                "custom",
                "square"
            ]
        );
        assert_eq!(views[0].origin, ProfileOrigin::Project);
        assert_eq!(views[1].origin, ProfileOrigin::Builtin);
        assert!(views[5].is_default && !views[1].is_default);
    }
}
