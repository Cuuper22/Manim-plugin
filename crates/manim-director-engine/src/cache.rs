use anyhow::{Context, Result};
use blake3::Hasher;
use manim_director_core::{DirectorSpec, JobRequest, SPEC_FILE};
use serde_json::Value;
use std::{fs::File, io::Read, path::Path};
use walkdir::{DirEntry, WalkDir};

const CACHE_SCHEMA: &[u8] = b"manim-director-cache-v2\0";

pub fn project_fingerprint(root: &Path, request: &JobRequest) -> Result<String> {
    let mut hasher = Hasher::new();
    hasher.update(CACHE_SCHEMA);
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    for variable in [
        "MANIM_DIRECTOR_RUNTIME_VERSION",
        "MANIM_DIRECTOR_PYTHON",
        "MANIM_DIRECTOR_RUNTIME_MODULE",
        "MANIM_VERSION",
        "FFMPEG_VERSION",
    ] {
        hasher.update(variable.as_bytes());
        if let Ok(value) = std::env::var(variable) {
            hasher.update(value.as_bytes());
        }
    }
    hasher.update(request.operation.runtime_method().as_bytes());
    let params = canonical_json(&request.params);
    hasher.update(serde_json::to_string(&params)?.as_bytes());

    let selected_scene = request
        .params
        .get("scene_file")
        .and_then(Value::as_str)
        .map(normalize_relative);
    let known_scene_files = DirectorSpec::load(root)
        .map(|spec| {
            spec.scenes
                .into_iter()
                .filter_map(|scene| scene.file)
                .map(|path| normalize_relative(&path))
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();

    let mut files = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(relevant_entry)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            if entry.path().extension().and_then(|value| value.to_str()) != Some("py") {
                return true;
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            match &selected_scene {
                Some(selected) if known_scene_files.contains(&relative) => &relative == selected,
                _ => true,
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.path().cmp(b.path()));

    for entry in files {
        let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
        hasher.update(relative.to_string_lossy().as_bytes());
        let mut file = File::open(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn normalize_relative(value: &str) -> String {
    value.trim_start_matches("./").replace('\\', "/")
}

fn relevant_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    if entry.file_type().is_dir() {
        return !matches!(
            name.as_ref(),
            ".git"
                | ".manim-director"
                | "media"
                | "output"
                | "dist"
                | "target"
                | "__pycache__"
                | ".venv"
                | "venv"
        );
    }
    name == SPEC_FILE
        || matches!(
            entry.path().extension().and_then(|value| value.to_str()),
            Some(
                "py" | "svg"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "webp"
                    | "csv"
                    | "json"
                    | "tex"
                    | "typ"
                    | "md"
                    | "wav"
                    | "mp3"
                    | "ogg"
                    | "ttf"
                    | "otf"
                    | "cfg"
                    | "toml"
                    | "lock"
                    | "txt"
                    | "yaml"
                    | "yml"
            )
        )
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical_json(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use manim_director_core::Operation;
    use serde_json::json;
    use std::fs;

    #[test]
    fn fingerprint_changes_for_source_but_not_output() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("scenes")).unwrap();
        fs::create_dir_all(dir.path().join("output")).unwrap();
        fs::write(dir.path().join("scenes/a.py"), "x=1").unwrap();
        let request = JobRequest {
            operation: Operation::Render,
            params: json!({"scene":"A"}),
            priority: 0,
        };
        let first = project_fingerprint(dir.path(), &request).unwrap();
        fs::write(dir.path().join("output/a.mp4"), "ignored").unwrap();
        assert_eq!(first, project_fingerprint(dir.path(), &request).unwrap());
        fs::write(dir.path().join("scenes/a.py"), "x=2").unwrap();
        assert_ne!(first, project_fingerprint(dir.path(), &request).unwrap());
    }

    #[test]
    fn fingerprint_includes_config_and_dependency_locks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("manim.cfg"), "frame_rate = 30").unwrap();
        fs::write(dir.path().join("uv.lock"), "v1").unwrap();
        let request = JobRequest {
            operation: Operation::Inspect,
            params: json!({}),
            priority: 0,
        };
        let first = project_fingerprint(dir.path(), &request).unwrap();
        fs::write(dir.path().join("manim.cfg"), "frame_rate = 60").unwrap();
        assert_ne!(first, project_fingerprint(dir.path(), &request).unwrap());
    }
}
