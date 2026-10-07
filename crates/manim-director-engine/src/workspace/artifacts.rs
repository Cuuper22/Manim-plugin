//! Served-file identity (HTTP §7): versions, content types and the one place
//! artifact URLs are minted.

use crate::confine;
use manim_director_core::{files, Artifact, ArtifactKind, MediaInfo};
use serde::Serialize;
use std::{fs, path::Path, time::UNIX_EPOCH};

/// `<size hex>-<mtime ns hex>`; any rewrite of the file changes it.
pub fn file_version(metadata: &fs::Metadata) -> String {
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    format!("{:x}-{:x}", metadata.len(), mtime)
}

/// `/api/files/<path>?v=<version>`, each segment encoded like
/// `encodeURIComponent`.
pub fn file_url(path: &str, version: &str) -> String {
    let segments: Vec<String> = path.split('/').map(encode_component).collect();
    format!("/api/files/{}?v={version}", segments.join("/"))
}

fn encode_component(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// The exact `Content-Type` of a served file (HTTP §7.2). Text formats are
/// served as plain text so nothing a project contains renders as markup.
pub fn content_type(path: &str) -> &'static str {
    match files::extension(path).as_deref() {
        Some("vtt") => "text/vtt; charset=utf-8",
        Some("srt" | "csv" | "txt" | "md" | "py" | "tex" | "typ" | "yaml" | "yml") => {
            "text/plain; charset=utf-8"
        }
        Some(extension) => mime_guess::from_ext(extension)
            .first_raw()
            .unwrap_or("application/octet-stream"),
        None => "application/octet-stream",
    }
}

/// An OPS artifact as the workbench fetches it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArtifactView {
    pub kind: ArtifactKind,
    pub path: String,
    pub label: Option<String>,
    /// Measured now, not when the job ended.
    pub bytes: u64,
    pub media: Option<MediaInfo>,
    pub url: String,
    pub content_type: &'static str,
    pub version: String,
    pub scene_id: Option<String>,
}

/// The artifact with a URL for its current file; `None` once the file is gone.
pub fn artifact_view(
    root: &Path,
    artifact: &Artifact,
    scene_id: Option<&str>,
) -> Option<ArtifactView> {
    let file = confine(root, &artifact.path).ok()?;
    let metadata = file.metadata().ok()?;
    let version = file_version(&metadata);
    Some(ArtifactView {
        kind: artifact.kind,
        path: artifact.path.clone(),
        label: artifact.label.clone(),
        bytes: metadata.len(),
        media: artifact.media.clone(),
        url: file_url(&artifact.path, &version),
        content_type: content_type(&artifact.path),
        version,
        scene_id: scene_id.map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_encode_each_segment_like_encode_uri_component() {
        assert_eq!(
            file_url(".manim-director/artifacts/7f/Re cur#rence.mp4", "a-1"),
            "/api/files/.manim-director/artifacts/7f/Re%20cur%23rence.mp4?v=a-1"
        );
        assert_eq!(encode_component("é(1)!"), "%C3%A9(1)!");
    }

    #[test]
    fn content_types_match_the_contract_table() {
        for (path, expected) in [
            ("a.mp4", "video/mp4"),
            ("a.mov", "video/quicktime"),
            ("a.webm", "video/webm"),
            ("a.gif", "image/gif"),
            ("a.png", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.webp", "image/webp"),
            ("a.svg", "image/svg+xml"),
            ("a.wav", "audio/wav"),
            ("a.mp3", "audio/mpeg"),
            ("a.ogg", "audio/ogg"),
            ("a.vtt", "text/vtt; charset=utf-8"),
            ("a.zip", "application/zip"),
            ("a.pdf", "application/pdf"),
            ("a.json", "application/json"),
        ] {
            assert_eq!(content_type(path), expected, "{path}");
        }
        for text in ["srt", "csv", "txt", "md", "py", "tex", "typ", "yaml", "yml"] {
            assert_eq!(
                content_type(&format!("a.{text}")),
                "text/plain; charset=utf-8"
            );
        }
        for served in files::DOWNLOADABLE {
            assert_ne!(
                content_type(&format!("a.{served}")),
                "application/octet-stream"
            );
        }
    }
}
