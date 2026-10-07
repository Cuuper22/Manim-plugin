//! The single source for file-type allowlists and the directories every
//! project walk skips.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Directory names skipped at any depth by every project walk.
pub const IGNORED_DIR_NAMES: &[&str] = &[
    ".git",
    ".manim-director",
    "__pycache__",
    ".venv",
    "venv",
    "node_modules",
    "target",
    "dist",
    "build",
];

pub const VIDEO: &[&str] = &["mp4", "mov", "webm", "gif"];
pub const IMAGE: &[&str] = &["png", "jpg", "jpeg", "webp"];
pub const CAPTIONS: &[&str] = &["vtt", "srt"];

/// Project files whose content can change a render (cache fingerprint).
pub const RENDER_INPUTS: &[&str] = &[
    "py", "svg", "png", "jpg", "jpeg", "webp", "csv", "json", "tex", "typ", "md", "wav", "mp3",
    "ogg", "ttf", "otf", "cfg", "toml", "txt", "yaml", "yml",
];

/// Text files the source API reads and writes.
pub const EDITABLE: &[&str] = &[
    "py", "json", "yaml", "yml", "toml", "md", "tex", "typ", "vtt", "srt", "txt",
];

/// Files the HTTP server may stream to the workbench.
pub const DOWNLOADABLE: &[&str] = &[
    "mp4", "mov", "webm", "gif", "png", "jpg", "jpeg", "webp", "svg", "wav", "mp3", "ogg", "vtt",
    "srt", "zip", "json", "yaml", "yml", "py", "tex", "typ", "md", "csv", "txt", "pdf",
];

/// Lowercase extension without the dot.
pub fn extension(path: impl AsRef<Path>) -> Option<String> {
    path.as_ref()
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
}

pub fn has_extension(path: impl AsRef<Path>, allowed: &[&str]) -> bool {
    extension(path).is_some_and(|extension| allowed.contains(&extension.as_str()))
}

/// Ingest classification: (kind, extensions, stored under the asset dir).
pub const INGEST_KINDS: &[(&str, &[&str], bool)] = &[
    ("markdown", &["md", "markdown"], false),
    ("latex", &["tex", "latex"], false),
    ("typst", &["typ"], false),
    ("text", &["txt"], false),
    ("csv", &["csv", "tsv"], false),
    ("json", &["json"], false),
    ("python", &["py"], false),
    ("notebook", &["ipynb"], false),
    ("pdf", &["pdf"], false),
    ("svg", &["svg"], true),
    (
        "image",
        &["png", "jpg", "jpeg", "webp", "bmp", "tiff"],
        true,
    ),
    (
        "audio",
        &["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus"],
        true,
    ),
    ("video", &["mp4", "mov", "webm", "mkv"], true),
];

/// Returns the ingest kind of a file and whether it belongs in the asset dir.
pub fn ingest_kind(path: impl AsRef<Path>) -> (&'static str, bool) {
    let extension = extension(path);
    INGEST_KINDS
        .iter()
        .find(|(_, extensions, _)| {
            extension
                .as_deref()
                .is_some_and(|extension| extensions.contains(&extension))
        })
        .map_or(("other", false), |(kind, _, asset)| (*kind, *asset))
}

/// File names that look like credentials; never ingested or exported.
pub fn is_secret_like(path: impl AsRef<Path>) -> bool {
    let Some(name) = path.as_ref().file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    if name == ".env" {
        return true;
    }
    if let Some(suffix) = name.strip_prefix(".env.") {
        return !matches!(suffix, "example" | "sample" | "template");
    }
    has_extension(&name, &["pem", "key", "p12", "pfx"])
        || ["id_rsa", "id_ed25519", "id_ecdsa"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || matches!(name.as_str(), ".npmrc" | ".pypirc" | ".netrc")
}

/// The ignored-directory set for one project: the shared names plus the
/// project's own media and output trees and any caller-specified directories.
#[derive(Debug, Clone)]
pub struct IgnoreSet {
    excluded: Vec<PathBuf>,
}

impl IgnoreSet {
    pub fn new(root: &Path, project_dirs: &[&str]) -> Self {
        Self {
            excluded: project_dirs
                .iter()
                .filter(|dir| !matches!(**dir, "" | "."))
                .map(|dir| root.join(dir))
                .collect(),
        }
    }

    /// Sorted regular files under `base`. `base` itself is always walked, even
    /// when it is one of the excluded directories.
    pub fn files_under(&self, base: &Path) -> Vec<PathBuf> {
        if !base.is_dir() {
            return Vec::new();
        }
        let mut files: Vec<PathBuf> = WalkDir::new(base)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| entry.depth() == 0 || !self.skips_dir(entry))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(walkdir::DirEntry::into_path)
            .collect();
        files.sort();
        files
    }

    fn skips_dir(&self, entry: &walkdir::DirEntry) -> bool {
        entry.file_type().is_dir()
            && (entry
                .file_name()
                .to_str()
                .is_some_and(|name| IGNORED_DIR_NAMES.contains(&name))
                || self.excluded.iter().any(|path| entry.path() == path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn walks_skip_shared_names_and_project_dirs_but_not_the_base() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for path in [
            "scenes/main.py",
            "scenes/__pycache__/main.pyc",
            "render/clip.mp4",
            "node_modules/x/index.js",
            "notes.md",
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let ignore = IgnoreSet::new(root, &["render"]);
        let relative: Vec<_> = ignore
            .files_under(root)
            .into_iter()
            .map(|path| path.strip_prefix(root).unwrap().to_path_buf())
            .collect();
        assert_eq!(
            relative,
            [PathBuf::from("notes.md"), PathBuf::from("scenes/main.py")]
        );
        assert_eq!(ignore.files_under(&root.join("render")).len(), 1);
    }

    #[test]
    fn secret_predicate_matches_the_contract_list() {
        for secret in [
            ".env",
            ".env.local",
            "server.pem",
            "tls.KEY",
            "cert.p12",
            "id_rsa.pub",
            "id_ed25519",
            ".npmrc",
            ".netrc",
        ] {
            assert!(is_secret_like(secret), "{secret}");
        }
        for plain in [".env.example", ".env.sample", "keynote.md", "main.py"] {
            assert!(!is_secret_like(plain), "{plain}");
        }
    }

    #[test]
    fn ingest_kinds_pick_destinations_by_extension() {
        assert_eq!(ingest_kind("notes.MD"), ("markdown", false));
        assert_eq!(ingest_kind("figure.svg"), ("svg", true));
        assert_eq!(ingest_kind("take.flac"), ("audio", true));
        assert_eq!(ingest_kind("archive.7z"), ("other", false));
    }
}
