//! Turning project-relative paths into filesystem paths that provably stay
//! inside the canonical project root, symlinks included.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

const STATE_DIR: &str = ".manim-director";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confinement {
    /// Not a plain project-relative path.
    Lexical,
    Missing,
    /// Resolves (through a symlink) outside the project.
    Outside,
    NotFile,
}

/// An existing regular file inside the canonical `root`.
pub fn confine(root: &Path, relative: &str) -> Result<PathBuf, Confinement> {
    let path = Path::new(relative);
    if relative.is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(Confinement::Lexical);
    }
    let canonical = root
        .join(path)
        .canonicalize()
        .map_err(|_| Confinement::Missing)?;
    if !canonical.starts_with(root) {
        return Err(Confinement::Outside);
    }
    if !canonical.is_file() {
        return Err(Confinement::NotFile);
    }
    Ok(canonical)
}

/// Canonicalizes the deepest existing ancestor of `path` (symlinks included)
/// and re-appends the missing tail; `None` when that lands outside `root`.
/// Checking this before creating directories means a symlinked ancestor can
/// never make the engine create anything outside the project.
pub fn resolve_existing_prefix(root: &Path, path: &Path) -> Option<PathBuf> {
    let mut existing = path;
    let mut tail = Vec::new();
    while fs::symlink_metadata(existing).is_err() {
        tail.push(existing.file_name()?);
        existing = existing.parent()?;
    }
    let mut resolved = existing.canonicalize().ok()?;
    if !resolved.starts_with(root) {
        return None;
    }
    resolved.extend(tail.iter().rev());
    Some(resolved)
}

/// `<root>/.manim-director/<name>`, created when `create` is set. Either level
/// being anything but a real directory (a symlink, say) is refused, so engine
/// state never lands outside the project.
pub fn state_dir(root: &Path, name: &str, create: bool) -> io::Result<PathBuf> {
    let state = root.join(STATE_DIR);
    let dir = state.join(name);
    for level in [&state, &dir] {
        if create {
            match fs::create_dir(level) {
                Ok(()) => continue,
                // Also when a concurrent writer created it first.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        if !fs::symlink_metadata(level)?.is_dir() {
            return Err(io::Error::other(format!(
                "{} is not a real directory",
                level.display()
            )));
        }
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confinement_rejects_traversal_symlink_escapes_and_directories() {
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.mp4"), "x").unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        fs::create_dir(root.join("dir")).unwrap();
        fs::write(root.join("dir/a.mp4"), "x").unwrap();
        assert_eq!(confine(&root, "dir/a.mp4").unwrap(), root.join("dir/a.mp4"));
        assert_eq!(confine(&root, "../a.mp4"), Err(Confinement::Lexical));
        assert_eq!(confine(&root, "/etc/passwd"), Err(Confinement::Lexical));
        assert_eq!(confine(&root, "dir/b.mp4"), Err(Confinement::Missing));
        assert_eq!(confine(&root, "dir"), Err(Confinement::NotFile));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.join("link")).unwrap();
            assert_eq!(confine(&root, "link/secret.mp4"), Err(Confinement::Outside));
        }
    }

    #[test]
    fn missing_tails_resolve_against_their_deepest_existing_ancestor() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        fs::create_dir(root.join("scenes")).unwrap();
        assert_eq!(
            resolve_existing_prefix(&root, &root.join("scenes/new/a.py")),
            Some(root.join("scenes/new/a.py"))
        );
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
            assert_eq!(
                resolve_existing_prefix(&root, &root.join("escape/new/a.py")),
                None
            );
            assert!(!outside.path().join("new").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn state_dirs_are_never_symlinks() {
        let project = tempfile::tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        assert_eq!(
            state_dir(&root, "undo", true).unwrap(),
            root.join(".manim-director/undo")
        );
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join(".manim-director/locks")).unwrap();
        assert!(state_dir(&root, "locks", true).is_err());
        assert!(state_dir(&root, "missing", false).is_err());
    }
}
