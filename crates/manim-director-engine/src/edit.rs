use anyhow::{anyhow, bail, Context, Result};
use blake3::Hasher;
use chrono::Utc;
use manim_director_core::{DirectorSpec, SPEC_FILE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;
use uuid::Uuid;

const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_READ_LINES: usize = 2_000;

#[derive(Debug, Clone, Deserialize)]
pub struct SourceReadQuery {
    pub path: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceView {
    pub path: String,
    pub revision: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceMutation {
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
    #[serde(default)]
    pub replacement: Option<String>,
    #[serde(default)]
    pub merge_patch: Option<Value>,
    #[serde(default)]
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceMutationResult {
    pub path: String,
    pub previous_revision: Option<String>,
    pub revision: String,
    pub bytes: usize,
    pub undo_path: Option<String>,
    pub affected_scenes: Vec<String>,
}

pub fn read_source(root: &Path, query: &SourceReadQuery) -> Result<SourceView> {
    let path = safe_source_path(root, &query.path, true)?;
    let metadata = fs::metadata(&path)?;
    if metadata.len() > MAX_SOURCE_BYTES {
        bail!("source exceeds {} bytes", MAX_SOURCE_BYTES);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let lines = split_lines(&content);
    let total = lines.len();
    let start = query.start_line.unwrap_or(1).clamp(1, total.max(1));
    let requested_end = query.end_line.unwrap_or_else(|| start.saturating_add(399));
    let end = requested_end
        .min(total)
        .min(start.saturating_add(MAX_READ_LINES - 1));
    let selected = if total == 0 || end < start {
        String::new()
    } else {
        lines[start - 1..end].join("\n")
    };
    Ok(SourceView {
        path: relative_string(root, &path),
        revision: revision(content.as_bytes()),
        language: language_for(&path).to_owned(),
        start_line: start,
        end_line: end,
        total_lines: total,
        content: selected,
    })
}

pub async fn apply_source_mutation(
    root: &Path,
    mutation: SourceMutation,
) -> Result<SourceMutationResult> {
    let path = safe_source_path(root, &mutation.path, false)?;
    let existing = if path.exists() {
        Some(fs::read_to_string(&path)?)
    } else {
        None
    };
    let previous_revision = existing.as_ref().map(|value| revision(value.as_bytes()));
    if let Some(expected) = mutation.expected_revision.as_deref() {
        if previous_revision.as_deref() != Some(expected) {
            bail!("revision conflict: source changed since it was read");
        }
    }

    let modes = usize::from(mutation.content.is_some())
        + usize::from(
            mutation.replacement.is_some()
                || mutation.start_line.is_some()
                || mutation.end_line.is_some(),
        )
        + usize::from(mutation.merge_patch.is_some());
    if modes != 1 {
        bail!("provide exactly one of content, a line edit, or merge_patch");
    }

    let mut next = if let Some(content) = mutation.content {
        content
    } else if let Some(patch) = mutation.merge_patch {
        if mutation.path != SPEC_FILE {
            bail!("merge_patch is only supported for {SPEC_FILE}");
        }
        let source = existing
            .as_deref()
            .ok_or_else(|| anyhow!("{SPEC_FILE} does not exist"))?;
        let mut document: Value = serde_yaml::from_str(source)?;
        json_merge_patch(&mut document, patch);
        serde_yaml::to_string(&document)?
    } else {
        apply_line_edit(
            existing.as_deref().unwrap_or_default(),
            mutation
                .start_line
                .ok_or_else(|| anyhow!("line edit requires start_line"))?,
            mutation
                .end_line
                .ok_or_else(|| anyhow!("line edit requires end_line"))?,
            mutation.replacement.as_deref().unwrap_or_default(),
        )?
    };
    if next.len() as u64 > MAX_SOURCE_BYTES {
        bail!("edited source exceeds {} bytes", MAX_SOURCE_BYTES);
    }
    if existing
        .as_deref()
        .is_some_and(|value| value.ends_with('\n'))
        && !next.ends_with('\n')
    {
        next.push('\n');
    }

    validate_source(&path, &next).await?;
    let affected_scenes = affected_scenes(root, &mutation.path);
    let undo_path = snapshot(root, &path, existing.as_deref())?;
    atomic_write(&path, next.as_bytes())?;
    Ok(SourceMutationResult {
        path: relative_string(root, &path),
        previous_revision,
        revision: revision(next.as_bytes()),
        bytes: next.len(),
        undo_path,
        affected_scenes,
    })
}

fn safe_source_path(root: &Path, relative: &str, must_exist: bool) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("source path must be project-relative and cannot contain ..");
    }
    if relative_path.starts_with(".manim-director") {
        bail!("working-state files cannot be edited through the source API");
    }
    let allowed = relative == SPEC_FILE
        || matches!(
            relative_path.extension().and_then(|v| v.to_str()),
            Some(
                "py" | "json"
                    | "yaml"
                    | "yml"
                    | "toml"
                    | "md"
                    | "tex"
                    | "typ"
                    | "vtt"
                    | "srt"
                    | "txt"
            )
        );
    if !allowed {
        bail!("unsupported source extension");
    }
    let candidate = root.join(relative_path);
    if must_exist && !candidate.is_file() {
        bail!("source file not found: {relative}");
    }
    if candidate.exists() {
        let canonical = candidate.canonicalize()?;
        if !canonical.starts_with(&root) {
            bail!("source path escapes the project");
        }
    } else {
        let parent = candidate
            .parent()
            .ok_or_else(|| anyhow!("source has no parent directory"))?;
        fs::create_dir_all(parent)?;
        if !parent.canonicalize()?.starts_with(&root) {
            bail!("source parent escapes the project");
        }
    }
    Ok(candidate)
}

async fn validate_source(path: &Path, source: &str) -> Result<()> {
    match path.extension().and_then(|value| value.to_str()) {
        Some("py") => {
            let python =
                std::env::var("MANIM_DIRECTOR_PYTHON").unwrap_or_else(|_| "python3".into());
            let mut child = Command::new(python)
                .args([
                    "-c",
                    "import ast,sys; ast.parse(sys.stdin.read(), filename=sys.argv[1])",
                    &path.to_string_lossy(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;
            use tokio::io::AsyncWriteExt;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(source.as_bytes())
                .await?;
            child.stdin.take();
            let output = child.wait_with_output().await?;
            if !output.status.success() {
                bail!(
                    "Python syntax invalid: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
        }
        Some("yaml" | "yml")
            if path.file_name().and_then(|value| value.to_str()) == Some(SPEC_FILE) =>
        {
            let spec: DirectorSpec = serde_yaml::from_str(source)?;
            spec.validate().map_err(|error| anyhow!(error))?;
        }
        Some("json") => {
            serde_json::from_str::<Value>(source).context("invalid JSON")?;
        }
        _ => {}
    }
    Ok(())
}

fn apply_line_edit(source: &str, start: usize, end: usize, replacement: &str) -> Result<String> {
    let mut lines = split_lines(source);
    if start == 0 || start > lines.len() + 1 {
        bail!("start_line is outside the file");
    }
    if end + 1 < start || end > lines.len() {
        bail!("end_line is outside the file");
    }
    let replacements = if replacement.is_empty() {
        Vec::new()
    } else {
        replacement.lines().map(str::to_owned).collect()
    };
    lines.splice(start - 1..end, replacements);
    Ok(lines.join("\n"))
}

fn split_lines(source: &str) -> Vec<String> {
    if source.is_empty() {
        Vec::new()
    } else {
        source.lines().map(str::to_owned).collect()
    }
}

fn json_merge_patch(target: &mut Value, patch: Value) {
    match patch {
        Value::Object(patch) => {
            if !target.is_object() {
                *target = Value::Object(Default::default());
            }
            let object = target.as_object_mut().unwrap();
            for (key, value) in patch {
                if value.is_null() {
                    object.remove(&key);
                } else {
                    json_merge_patch(object.entry(key).or_insert(Value::Null), value);
                }
            }
        }
        value => *target = value,
    }
}

fn snapshot(root: &Path, path: &Path, source: Option<&str>) -> Result<Option<String>> {
    let Some(source) = source else {
        return Ok(None);
    };
    let relative = path.strip_prefix(root)?;
    let snapshot_root = root.join(".manim-director/undo").join(format!(
        "{}-{}",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        Uuid::new_v4()
    ));
    let snapshot = snapshot_root.join(relative);
    fs::create_dir_all(snapshot.parent().unwrap())?;
    fs::write(&snapshot, source)?;
    Ok(Some(relative_string(root, &snapshot)))
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("source has no parent"))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("source"),
        Uuid::new_v4()
    ));
    fs::write(&temp, content)?;
    let file = fs::OpenOptions::new().read(true).open(&temp)?;
    file.sync_all()?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

fn affected_scenes(root: &Path, relative: &str) -> Vec<String> {
    let Ok(spec) = DirectorSpec::load(root) else {
        return Vec::new();
    };
    if relative == SPEC_FILE {
        return spec
            .scenes
            .into_iter()
            .map(|scene| scene.id)
            .filter(|id| !id.is_empty())
            .collect();
    }
    let normalized = relative.replace('\\', "/");
    spec.scenes
        .into_iter()
        .filter(|scene| {
            scene
                .file
                .as_deref()
                .map(|file| file.replace('\\', "/") == normalized)
                .unwrap_or(false)
        })
        .map(|scene| scene.id)
        .collect()
}

fn revision(content: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(content);
    hasher.finalize().to_hex().to_string()
}
fn relative_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
fn language_for(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("py") => "python",
        Some("json") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("md") => "markdown",
        Some("tex") => "latex",
        Some("typ") => "typst",
        Some("vtt" | "srt") => "captions",
        _ => "text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_replacement_and_merge_patch_work() {
        assert_eq!(
            apply_line_edit("a\nb\nc\n", 2, 2, "x\ny").unwrap(),
            "a\nx\ny\nc"
        );
        let mut value = serde_json::json!({"a":1,"nested":{"b":2}});
        json_merge_patch(&mut value, serde_json::json!({"a":null,"nested":{"c":3}}));
        assert_eq!(value, serde_json::json!({"nested":{"b":2,"c":3}}));
    }
}
