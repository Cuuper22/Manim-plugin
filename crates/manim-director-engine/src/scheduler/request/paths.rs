//! The public path rule (OPS §1.2) for every path-typed param, and the CLI's
//! conversion of its path arguments into project-relative form.

use crate::confine::resolve_existing_prefix;
use manim_director_core::{check_project_path, files, EngineError};
use std::{
    io,
    path::{Component, Path, PathBuf},
};

pub(crate) enum PathUse<'a> {
    /// An existing regular file.
    Input {
        allow_artifacts: bool,
        extensions: &'a [&'a str],
    },
    /// A file the runtime writes; never under engine state or Manim's media
    /// cache, never an existing directory.
    Output {
        media_dir: &'a str,
        extensions: &'a [&'a str],
    },
}

/// Applies rules 1–6 to `value` and returns the absolute path for a task.
pub(crate) fn project_path(
    root: &Path,
    field: &str,
    value: &str,
    usage: PathUse<'_>,
) -> Result<PathBuf, EngineError> {
    let allow_artifacts = matches!(
        usage,
        PathUse::Input {
            allow_artifacts: true,
            ..
        }
    );
    check_project_path(field, value, allow_artifacts)?;
    let joined = root.join(value);
    let resolved = resolve_existing_prefix(root, &joined)
        .ok_or_else(|| EngineError::invalid(field, "outside_project"))?;
    match usage {
        PathUse::Input { extensions, .. } => {
            if !resolved.is_file() {
                return Err(EngineError::invalid(field, "missing"));
            }
            require_extension(field, &resolved, extensions)?;
            Ok(resolved)
        }
        PathUse::Output {
            media_dir,
            extensions,
        } => {
            let denied = [root.join(files::STATE_DIR), root.join(media_dir)];
            let under_denied = denied
                .iter()
                .any(|dir| dir != root && resolved.starts_with(dir));
            if under_denied || resolved.is_dir() {
                return Err(EngineError::invalid(field, "denied"));
            }
            require_extension(field, &resolved, extensions)?;
            Ok(resolved)
        }
    }
}

fn require_extension(field: &str, path: &Path, allowed: &[&str]) -> Result<(), EngineError> {
    if allowed.is_empty() || files::has_extension(path, allowed) {
        Ok(())
    } else {
        Err(EngineError::invalid_choice(
            field,
            "extension",
            allowed.iter().copied(),
        ))
    }
}

/// Turns a CLI path argument (relative to the shell's cwd, or absolute) into
/// the project-relative POSIX form every request carries.
pub fn cli_project_path(root: &Path, cwd: &Path, argument: &Path) -> Result<String, EngineError> {
    let absolute = normalize(&cwd.join(argument));
    let relative = absolute
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .or_else(|_| {
            // The cwd may reach the project through a symlink.
            let canonical = canonical_prefix(&absolute)?;
            canonical
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .map_err(|_| io::ErrorKind::NotFound.into())
        })
        .map_err(|_: io::Error| {
            EngineError::invalid(argument.display().to_string(), "outside_project")
        })?;
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        return Err(EngineError::invalid(
            argument.display().to_string(),
            "missing",
        ));
    }
    Ok(text)
}

fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

fn canonical_prefix(path: &Path) -> io::Result<PathBuf> {
    let mut existing = path;
    let mut tail = Vec::new();
    while !existing.exists() {
        tail.push(existing.file_name().ok_or(io::ErrorKind::NotFound)?);
        existing = existing.parent().ok_or(io::ErrorKind::NotFound)?;
    }
    let mut canonical = existing.canonicalize()?;
    canonical.extend(tail.iter().rev());
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn reason(error: EngineError) -> String {
        match error {
            EngineError::InvalidParams { reason, .. } => reason,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn project() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        for path in [
            "scenes/main.py",
            ".manim-director/artifacts/job/clip.mp4",
            "captions/en.vtt",
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        fs::create_dir_all(root.join("media")).unwrap();
        (directory, root)
    }

    #[test]
    fn inputs_must_exist_inside_with_the_right_extension() {
        let (_directory, root) = project();
        let input = |allow_artifacts, extensions| PathUse::Input {
            allow_artifacts,
            extensions,
        };
        assert_eq!(
            project_path(&root, "file", "scenes/main.py", input(false, &["py"])).unwrap(),
            root.join("scenes/main.py")
        );
        assert_eq!(
            reason(
                project_path(&root, "file", "scenes/other.py", input(false, &["py"])).unwrap_err()
            ),
            "missing"
        );
        assert_eq!(
            reason(
                project_path(&root, "file", "scenes/main.py", input(false, &["mp4"])).unwrap_err()
            ),
            "extension"
        );
        project_path(
            &root,
            "source",
            ".manim-director/artifacts/job/clip.mp4",
            input(true, files::VIDEO),
        )
        .unwrap();
    }

    #[test]
    fn outputs_are_denied_in_state_and_media_dirs_and_over_directories() {
        let (_directory, root) = project();
        let output = |extensions| PathUse::Output {
            media_dir: "media",
            extensions,
        };
        assert_eq!(
            project_path(&root, "output", "output/new/film.zip", output(&["zip"])).unwrap(),
            root.join("output/new/film.zip")
        );
        let media_at_root = PathUse::Output {
            media_dir: ".",
            extensions: &["zip"],
        };
        project_path(&root, "output", "film.zip", media_at_root).unwrap();
        for (value, expected) in [
            ("media/a.srt", "denied"),
            ("captions", "denied"),
            ("captions/en.txt", "extension"),
            (".manim-director/a.srt", "hidden"),
        ] {
            assert_eq!(
                reason(
                    project_path(&root, "output", value, output(&["srt", "txt"][..1])).unwrap_err()
                ),
                expected,
                "{value}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_project_are_rejected() {
        let (_directory, root) = project();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
        let use_output = PathUse::Output {
            media_dir: "media",
            extensions: &["zip"],
        };
        assert_eq!(
            reason(project_path(&root, "output", "escape/a.zip", use_output).unwrap_err()),
            "outside_project"
        );
        std::os::unix::fs::symlink("/nonexistent/target", root.join("dangling.zip")).unwrap();
        let use_output = PathUse::Output {
            media_dir: "media",
            extensions: &["zip"],
        };
        assert_eq!(
            reason(project_path(&root, "output", "dangling.zip", use_output).unwrap_err()),
            "outside_project"
        );
    }

    #[test]
    fn cli_paths_become_project_relative() {
        let (_directory, root) = project();
        let scenes = root.join("scenes");
        assert_eq!(
            cli_project_path(&root, &scenes, Path::new("./main.py")).unwrap(),
            "scenes/main.py"
        );
        assert_eq!(
            cli_project_path(&root, &scenes, &root.join("captions/en.vtt")).unwrap(),
            "captions/en.vtt"
        );
        assert_eq!(
            cli_project_path(&root, &scenes, Path::new("../output/new.zip")).unwrap(),
            "output/new.zip"
        );
        assert_eq!(
            reason(cli_project_path(&root, &scenes, Path::new("../../elsewhere.py")).unwrap_err()),
            "outside_project"
        );
    }
}
