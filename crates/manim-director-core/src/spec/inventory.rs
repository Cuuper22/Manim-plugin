use super::{DirectorSpec, SpecError, SPEC_FILE};
use crate::files::{self, IgnoreSet};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const MAX_PYTHON_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PYTHON_SOURCES: usize = 500;

/// Walks up from `start` to the nearest directory holding `director.yaml`,
/// never into one anyone may write to (`/tmp`): a spec planted there by
/// another user must not become this user's project.
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
            .filter(|parent| !world_writable(parent))
            .ok_or_else(|| SpecError::NotFound(start.to_path_buf()))?;
    }
}

#[cfg(unix)]
fn world_writable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.permissions().mode() & 0o002 != 0)
}

#[cfg(not(unix))]
fn world_writable(_: &Path) -> bool {
    false
}

/// Project-relative file lists of the source, asset and output trees.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectInventory {
    pub source_files: Vec<PathBuf>,
    pub asset_files: Vec<PathBuf>,
    pub output_files: Vec<PathBuf>,
}

impl ProjectInventory {
    pub fn scan(root: &Path, spec: &DirectorSpec) -> Self {
        let project = &spec.project;
        let relative = |files: Vec<PathBuf>| -> Vec<PathBuf> {
            files
                .into_iter()
                .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
                .collect()
        };
        let sources = IgnoreSet::new(
            root,
            &[&project.asset_dir, &project.output_dir, &project.media_dir],
        );
        let plain = IgnoreSet::new(root, &[]);
        Self {
            source_files: relative(sources.files_under(&root.join(&project.source_dir))),
            asset_files: relative(plain.files_under(&root.join(&project.asset_dir))),
            output_files: relative(plain.files_under(&root.join(&project.output_dir))),
        }
    }
}

/// The Python files a scene may live in (OPS §1.2 `python_sources`).
#[derive(Debug, Clone, Default)]
pub struct PythonSources {
    /// Absolute paths, sorted by project-relative path.
    pub files: Vec<PathBuf>,
    /// Project-relative paths skipped for exceeding [`MAX_PYTHON_SOURCE_BYTES`].
    pub oversized: Vec<String>,
    /// Count before the [`MAX_PYTHON_SOURCES`] cap.
    pub total: usize,
}

impl PythonSources {
    pub fn truncated(&self) -> bool {
        self.total > self.files.len()
    }
}

pub fn python_sources(root: &Path, spec: &DirectorSpec) -> PythonSources {
    let mut candidates = spec
        .ignore_set(root)
        .files_under(&root.join(&spec.project.source_dir));
    candidates.retain(|path| files::has_extension(path, &["py"]));
    if let Some(source) = &spec.engine.source {
        let path = root.join(source);
        if path.is_file() && !candidates.contains(&path) {
            candidates.push(path);
        }
    }
    candidates.sort_by_key(|path| relative_posix(root, path));
    let mut sources = PythonSources::default();
    for path in candidates {
        let bytes = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        if bytes > MAX_PYTHON_SOURCE_BYTES {
            sources.oversized.push(relative_posix(root, &path));
        } else {
            sources.total += 1;
            if sources.files.len() < MAX_PYTHON_SOURCES {
                sources.files.push(path);
            }
        }
    }
    sources
}

/// `path` relative to `root` with `/` separators (unchanged when outside).
pub fn relative_posix(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project(spec: &str, files: &[&str]) -> (tempfile::TempDir, DirectorSpec) {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(SPEC_FILE), spec).unwrap();
        for path in files {
            let path = directory.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let spec = DirectorSpec::load(directory.path()).unwrap();
        (directory, spec)
    }

    #[test]
    fn root_source_inventory_excludes_generated_and_separately_classified_trees() {
        let (directory, spec) = project(
            "version: 1\nproject:\n  name: Demo\n  source_dir: .\n  output_dir: dist-out\n  media_dir: media\n",
            &[
                "scenes.py",
                "data/values.csv",
                "assets/logo.svg",
                "dist-out/final.mp4",
                ".manim-director/state.db",
                "media/Tex/cache.svg",
                "__pycache__/scenes.pyc",
            ],
        );
        let inventory = ProjectInventory::scan(directory.path(), &spec);
        let sources: Vec<_> = inventory
            .source_files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(sources, ["data/values.csv", SPEC_FILE, "scenes.py"]);
        assert_eq!(inventory.asset_files, [PathBuf::from("assets/logo.svg")]);
        assert_eq!(
            inventory.output_files,
            [PathBuf::from("dist-out/final.mp4")]
        );
    }

    #[test]
    fn python_sources_add_engine_source_skip_ignored_dirs_and_report_oversize() {
        let (directory, spec) = project(
            "version: 1\nproject:\n  name: Demo\nengine:\n  source: tools/main.py\n",
            &[
                "scenes/b.py",
                "scenes/a.py",
                "scenes/notes.md",
                "scenes/venv/lib.py",
                "tools/main.py",
                "output/old.py",
            ],
        );
        let root = directory.path();
        fs::write(
            root.join("scenes/huge.py"),
            vec![b'#'; MAX_PYTHON_SOURCE_BYTES as usize + 1],
        )
        .unwrap();
        let sources = python_sources(root, &spec);
        let relative: Vec<_> = sources
            .files
            .iter()
            .map(|path| relative_posix(root, path))
            .collect();
        assert_eq!(relative, ["scenes/a.py", "scenes/b.py", "tools/main.py"]);
        assert_eq!(sources.oversized, ["scenes/huge.py"]);
        assert!(!sources.truncated());
    }

    #[test]
    fn find_project_walks_up_from_nested_files() {
        let (directory, _) = project("version: 1\nproject:\n  name: Demo\n", &["scenes/a.py"]);
        let found = find_project(directory.path().join("scenes/a.py")).unwrap();
        assert_eq!(found, directory.path().canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn find_project_never_climbs_into_a_world_writable_directory() {
        use std::os::unix::fs::PermissionsExt;
        let (directory, _) = project("version: 1\nproject:\n  name: Planted\n", &[]);
        let shared = directory.path();
        fs::set_permissions(shared, fs::Permissions::from_mode(0o1777)).unwrap();
        fs::create_dir(shared.join("work")).unwrap();
        assert!(matches!(
            find_project(shared.join("work")),
            Err(SpecError::NotFound(_))
        ));
        assert!(find_project(shared).is_ok(), "asked for by name");
    }
}
