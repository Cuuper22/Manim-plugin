//! The source API shared by HTTP, MCP and CLI `edit` (HTTP §6.3): exact
//! line-model pages and revision-checked atomic writes. Blocking; callers on
//! the async runtime use `spawn_blocking`.

use crate::{
    confine::{resolve_existing_prefix, state_dir},
    process::wait_bounded,
};
use chrono::Utc;
use manim_director_core::{
    files, named_enum, path_rule_violation, relative_posix, scene_key, DirectorSpec,
    DiscoverResult, EngineError, Resource, SpecError, SPEC_FILE,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_yaml::{Mapping, Value as Yaml};
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use uuid::Uuid;

/// Where previous versions are kept as manual recovery (pruned to 20 per file).
pub const UNDO_DIR: &str = ".manim-director/undo";
pub(crate) const MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_PAGE_LINES: u64 = 400;
pub(crate) const MAX_PAGE_LINES: u64 = 2000;
const SYNTAX_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

/// Prints `{message, line, column}` for the first syntax error, nothing when
/// the source parses. `-I` keeps the project's own modules off `sys.path`.
const SYNTAX_CHECK: &str = "\
import ast, json, sys, warnings
warnings.simplefilter('ignore')
source = sys.stdin.buffer.read()
try:
    ast.parse(source, filename=sys.argv[1])
except (SyntaxError, ValueError) as error:
    print(json.dumps({'message': getattr(error, 'msg', None) or str(error),
                      'line': getattr(error, 'lineno', None),
                      'column': getattr(error, 'offset', None)}))
";

named_enum! {
    pub enum SourceLanguage {
        Python = "python",
        Json = "json",
        Yaml = "yaml",
        Toml = "toml",
        Markdown = "markdown",
        Latex = "latex",
        Typst = "typst",
        Captions = "captions",
        Text = "text",
    }
}

impl SourceLanguage {
    fn of(path: &str) -> Self {
        match files::extension(path).as_deref() {
            Some("py") => Self::Python,
            Some("json") => Self::Json,
            Some("yaml" | "yml") => Self::Yaml,
            Some("toml") => Self::Toml,
            Some("md") => Self::Markdown,
            Some("tex") => Self::Latex,
            Some("typ") => Self::Typst,
            Some("vtt" | "srt") => Self::Captions,
            _ => Self::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Eol {
    Lf,
    Crlf,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourcePage {
    pub path: String,
    pub revision: String,
    pub language: SourceLanguage,
    pub eol: Eol,
    pub final_newline: bool,
    pub bytes: u64,
    pub total_lines: u64,
    pub start_line: u64,
    /// Inclusive; `start_line - 1` for an empty page.
    pub end_line: u64,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceEdit {
    ReplaceAll {
        content: String,
    },
    ReplaceLines {
        start_line: u64,
        end_line: u64,
        replacement: String,
    },
    MergePatch {
        patch: Map<String, Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceWrite {
    pub path: String,
    /// Required, and `null` means "create; the file must not exist".
    #[serde(deserialize_with = "Option::deserialize")]
    pub expected_revision: Option<String>,
    pub edit: SourceEdit,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceWriteResult {
    pub path: String,
    pub previous_revision: Option<String>,
    pub revision: String,
    pub bytes: u64,
    pub total_lines: u64,
    /// Scene ids in this file per the scene index at write time.
    pub affected_scenes: Vec<String>,
}

/// Lines split on `"\n"` only; a `"\r"` stays part of its line so CRLF files
/// round-trip byte for byte.
struct Lines<'a> {
    lines: Vec<&'a str>,
    eol: Eol,
    final_newline: bool,
}

impl<'a> Lines<'a> {
    fn of(content: &'a str) -> Self {
        let final_newline = content.ends_with('\n');
        let body = content.strip_suffix('\n').unwrap_or(content);
        let lines = match content.is_empty() {
            true => Vec::new(),
            false => body.split('\n').collect(),
        };
        let newlines = content.matches('\n').count();
        let eol = match newlines > 0 && content.matches("\r\n").count() == newlines {
            true => Eol::Crlf,
            false => Eol::Lf,
        };
        Self {
            lines,
            eol,
            final_newline,
        }
    }

    fn total(&self) -> u64 {
        self.lines.len() as u64
    }
}

pub fn read_source(
    root: &Path,
    path: &str,
    start_line: Option<u64>,
    end_line: Option<u64>,
) -> Result<SourcePage, EngineError> {
    let target = source_path(root, path)?;
    let content = read_current(&target, path)?.ok_or_else(|| missing(path))?;
    let lines = Lines::of(&content);
    let total = lines.total();
    let start = start_line.unwrap_or(1);
    let requested_end = end_line.unwrap_or(start.saturating_add(DEFAULT_PAGE_LINES - 1));
    let end = requested_end
        .min(total)
        .min(start.saturating_add(MAX_PAGE_LINES - 1));
    if start < 1 || (total > 0 && start > total) || end + 1 < start {
        return Err(EngineError::LineOutOfRange {
            start_line: start,
            end_line: requested_end,
            total_lines: total,
        });
    }
    let page = match end >= start {
        true => lines.lines[(start - 1) as usize..end as usize].join("\n"),
        false => String::new(),
    };
    Ok(SourcePage {
        path: path.to_owned(),
        revision: revision(content.as_bytes()),
        language: SourceLanguage::of(path),
        eol: lines.eol,
        final_newline: lines.final_newline,
        bytes: content.len() as u64,
        total_lines: total,
        start_line: start,
        end_line: end,
        content: page,
    })
}

/// The file's current revision, or `None` when it does not exist.
pub fn current_revision(root: &Path, path: &str) -> Result<Option<String>, EngineError> {
    let target = source_path(root, path)?;
    Ok(read_current(&target, path)?.map(|content| revision(content.as_bytes())))
}

/// Applies a revision-checked edit. The edit is computed and validated
/// against the revision the caller saw; the write then happens under a
/// per-file lock only if the file still has exactly that content, so two
/// concurrent writers can never both succeed from the same revision.
pub fn write_source(
    root: &Path,
    python: &Path,
    write: &SourceWrite,
    index: Option<&DiscoverResult>,
) -> Result<SourceWriteResult, EngineError> {
    let path = write.path.as_str();
    let target = source_path(root, path)?;
    let current = read_current(&target, path)?;
    let previous_revision = current
        .as_deref()
        .map(|content| revision(content.as_bytes()));
    if write.expected_revision != previous_revision {
        return Err(conflict(write, previous_revision));
    }
    let next = apply(path, current.as_deref(), &write.edit)?;
    if next.len() as u64 > MAX_SOURCE_BYTES {
        return Err(EngineError::FileTooLarge {
            path: path.to_owned(),
            bytes: next.len() as u64,
            limit_bytes: MAX_SOURCE_BYTES,
        });
    }
    validate(python, path, &next)?;

    // Keyed by the resolved file, so a symlink and its target share one lock.
    let _lock = lock(root, &relative_posix(root, &target))?;
    let on_disk = disk_revision(&target)?;
    if on_disk != previous_revision {
        return Err(conflict(write, on_disk));
    }
    create_parent(root, &target, path)?;
    if let Some(previous) = &current {
        snapshot(root, path, previous)?;
    }
    atomic_write(&target, next.as_bytes())?;
    Ok(SourceWriteResult {
        path: path.to_owned(),
        previous_revision,
        revision: revision(next.as_bytes()),
        bytes: next.len() as u64,
        total_lines: Lines::of(&next).total(),
        affected_scenes: index
            .map(|index| {
                index
                    .scenes
                    .iter()
                    .filter(|scene| scene.file == path)
                    .map(|scene| scene_key(&scene.file, &scene.name))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// The filesystem path a source path names: the lexical rules, the extension
/// allowlist, then confinement of the deepest existing ancestor, all before
/// anything is created. A symlink is followed only to a path that passes the
/// same rules, so it cannot reach engine state or `.git`.
fn source_path(root: &Path, path: &str) -> Result<PathBuf, EngineError> {
    let invalid = |reason| EngineError::InvalidPath {
        path: path.to_owned(),
        reason,
    };
    if let Some(reason) = path_rule_violation(path, false) {
        return Err(invalid(reason));
    }
    let extension = files::extension(path).unwrap_or_default();
    if !files::EDITABLE.contains(&extension.as_str()) {
        return Err(EngineError::UnsupportedFileType {
            path: path.to_owned(),
            extension,
        });
    }
    let target =
        resolve_existing_prefix(root, &root.join(path)).ok_or(invalid("outside_project"))?;
    let resolved = relative_posix(root, &target);
    if let Some(reason) = path_rule_violation(&resolved, false) {
        return Err(invalid(reason));
    }
    if target.is_dir() || !files::has_extension(&resolved, files::EDITABLE) {
        return Err(invalid("denied"));
    }
    Ok(target)
}

fn read_current(target: &Path, path: &str) -> Result<Option<String>, EngineError> {
    let bytes = match fs::metadata(target) {
        Ok(metadata) if metadata.len() > MAX_SOURCE_BYTES => {
            return Err(EngineError::FileTooLarge {
                path: path.to_owned(),
                bytes: metadata.len(),
                limit_bytes: MAX_SOURCE_BYTES,
            })
        }
        Ok(_) => fs::read(target).map_err(EngineError::internal)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(EngineError::internal(error)),
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| EngineError::NotUtf8 {
            path: path.to_owned(),
        })
}

fn disk_revision(target: &Path) -> Result<Option<String>, EngineError> {
    match fs::read(target) {
        Ok(bytes) => Ok(Some(revision(&bytes))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(EngineError::internal(error)),
    }
}

fn apply(path: &str, current: Option<&str>, edit: &SourceEdit) -> Result<String, EngineError> {
    match edit {
        SourceEdit::ReplaceAll { content } => Ok(content.clone()),
        SourceEdit::ReplaceLines {
            start_line,
            end_line,
            replacement,
        } => replace_lines(current.unwrap_or(""), *start_line, *end_line, replacement),
        SourceEdit::MergePatch { patch } => {
            if path != SPEC_FILE {
                return Err(EngineError::invalid(
                    "edit.kind",
                    format!("merge_patch applies only to {SPEC_FILE}"),
                ));
            }
            let mut document = match current {
                Some(text) => serde_yaml::from_str(text).map_err(|error| {
                    let location = error.location();
                    source_invalid(
                        path,
                        error.to_string(),
                        location.as_ref().map(|at| at.line() as u32),
                        location.as_ref().map(|at| at.column() as u32),
                    )
                })?,
                None => Yaml::Null,
            };
            let patch = serde_yaml::to_value(patch).map_err(EngineError::internal)?;
            merge_patch(&mut document, patch);
            serde_yaml::to_string(&document).map_err(EngineError::internal)
        }
    }
}

/// Replaces lines `start..=end` (`end = start - 1` inserts before `start`),
/// joining with the file's own line ending and keeping its final newline.
fn replace_lines(
    content: &str,
    start: u64,
    end: u64,
    replacement: &str,
) -> Result<String, EngineError> {
    let lines = Lines::of(content);
    let total = lines.total();
    if start < 1 || start > total + 1 || end + 1 < start || end > total {
        return Err(EngineError::LineOutOfRange {
            start_line: start,
            end_line: end,
            total_lines: total,
        });
    }
    let crlf = lines.eol == Eol::Crlf;
    let mut result: Vec<&str> = lines.lines.iter().map(|line| bare(line, crlf)).collect();
    let inserted: Vec<&str> = match replacement.is_empty() {
        true => Vec::new(),
        false => replacement
            .strip_suffix('\n')
            .unwrap_or(replacement)
            .split('\n')
            .map(|line| bare(line, crlf))
            .collect(),
    };
    result.splice((start - 1) as usize..end as usize, inserted);
    let eol = if crlf { "\r\n" } else { "\n" };
    let mut text = result.join(eol);
    if lines.final_newline && !result.is_empty() {
        text.push_str(eol);
    }
    Ok(text)
}

/// A CRLF file's lines without their `"\r"`, so they can be rejoined with
/// `"\r\n"` without doubling it.
fn bare(line: &str, crlf: bool) -> &str {
    match crlf {
        true => line.strip_suffix('\r').unwrap_or(line),
        false => line,
    }
}

/// RFC 7386, applied to the YAML itself so the file keeps its key order.
fn merge_patch(target: &mut Yaml, patch: Yaml) {
    let Yaml::Mapping(patch) = patch else {
        *target = patch;
        return;
    };
    if !target.is_mapping() {
        *target = Yaml::Mapping(Mapping::new());
    }
    if let Yaml::Mapping(mapping) = target {
        for (key, value) in patch {
            if value.is_null() {
                mapping.shift_remove(&key);
            } else {
                merge_patch(mapping.entry(key).or_insert(Yaml::Null), value);
            }
        }
    }
}

fn validate(python: &Path, path: &str, content: &str) -> Result<(), EngineError> {
    match SourceLanguage::of(path) {
        SourceLanguage::Python => python_syntax(python, path, content),
        SourceLanguage::Yaml if path == SPEC_FILE => match DirectorSpec::parse(content) {
            Ok(_) => Ok(()),
            Err(SpecError::Parse {
                message,
                line,
                column,
            }) => Err(source_invalid(path, message, line, column)),
            Err(error) => Err(source_invalid(path, error.to_string(), None, None)),
        },
        SourceLanguage::Json => serde_json::from_str::<serde::de::IgnoredAny>(content)
            .map(drop)
            .map_err(|error| {
                source_invalid(
                    path,
                    error.to_string(),
                    Some(error.line() as u32),
                    Some(error.column() as u32),
                )
            }),
        _ => Ok(()),
    }
}

/// `ast.parse` in the interpreter the bridge runs, so the check matches the
/// Python that will import the file.
fn python_syntax(python: &Path, path: &str, content: &str) -> Result<(), EngineError> {
    let unavailable = |detail: String| {
        EngineError::internal(format!(
            "could not check Python syntax with {}: {detail}",
            python.display()
        ))
    };
    let mut child = Command::new(python)
        .args(["-I", "-c", SYNTAX_CHECK, path])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| unavailable(error.to_string()))?;
    // The script reads all of stdin before writing, so this cannot deadlock.
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(content.as_bytes()));
    let status = wait_bounded(&mut child, SYNTAX_CHECK_TIMEOUT)
        .map_err(|error| unavailable(error.to_string()))?
        .ok_or_else(|| unavailable("timed out".into()))?;
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut output);
    }
    if output.trim().is_empty() {
        return match (status.success(), written) {
            (true, Some(Ok(()))) => Ok(()),
            _ => Err(unavailable(format!("it exited with {status}"))),
        };
    }
    #[derive(Deserialize)]
    struct Failure {
        message: String,
        line: Option<u32>,
        column: Option<u32>,
    }
    let failure: Failure = serde_json::from_str(output.trim())
        .map_err(|error| unavailable(format!("unreadable output ({error})")))?;
    Err(source_invalid(
        path,
        failure.message,
        failure.line,
        failure.column,
    ))
}

/// An exclusive advisory lock per file, shared with every engine process of
/// the project; released when the returned handle drops.
fn lock(root: &Path, path: &str) -> Result<File, EngineError> {
    let locks = state_dir(root, "locks", true).map_err(EngineError::internal)?;
    let name = format!("{}.lock", &revision(path.as_bytes())[..32]);
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(locks.join(name))
        .map_err(EngineError::internal)?;
    file.lock().map_err(EngineError::internal)?;
    Ok(file)
}

/// Creates missing parent directories, then re-checks confinement in case an
/// ancestor was swapped for a symlink since `source_path` looked.
fn create_parent(root: &Path, target: &Path, path: &str) -> Result<(), EngineError> {
    let Some(parent) = target.parent() else {
        return Err(EngineError::internal(format!("{path} has no parent")));
    };
    fs::create_dir_all(parent).map_err(EngineError::internal)?;
    let inside = parent
        .canonicalize()
        .map_err(EngineError::internal)?
        .starts_with(root);
    match inside {
        true => Ok(()),
        false => Err(EngineError::InvalidPath {
            path: path.to_owned(),
            reason: "outside_project",
        }),
    }
}

fn snapshot(root: &Path, path: &str, previous: &str) -> Result<(), EngineError> {
    let undo = state_dir(root, "undo", true).map_err(EngineError::internal)?;
    let file = undo
        .join(format!(
            "{}-{}",
            Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
            Uuid::new_v4()
        ))
        .join(path);
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(EngineError::internal)?;
    }
    fs::write(file, previous).map_err(EngineError::internal)
}

/// Temp file in the same directory, fsync, rename. Keeps an existing file's
/// permissions.
fn atomic_write(target: &Path, content: &[u8]) -> Result<(), EngineError> {
    let parent = target.parent().unwrap_or(target);
    let name = target
        .file_name()
        .map_or_else(|| "source".into(), |name| name.to_string_lossy());
    let temp = parent.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
    let written = (|| {
        let mut file = File::create(&temp)?;
        file.write_all(content)?;
        if let Ok(metadata) = fs::metadata(target) {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()?;
        fs::rename(&temp, target)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written.map_err(EngineError::internal)
}

fn conflict(write: &SourceWrite, current: Option<String>) -> EngineError {
    EngineError::RevisionConflict {
        path: write.path.clone(),
        expected_revision: write.expected_revision.clone(),
        current_revision: current,
    }
}

fn missing(path: &str) -> EngineError {
    EngineError::NotFound {
        resource: Resource::File,
        key: path.to_owned(),
    }
}

fn source_invalid(
    path: &str,
    message: String,
    line: Option<u32>,
    column: Option<u32>,
) -> EngineError {
    EngineError::SourceInvalid {
        path: path.to_owned(),
        language: SourceLanguage::of(path).as_str().to_owned(),
        message,
        line,
        column,
    }
}

fn revision(content: &[u8]) -> String {
    blake3::hash(content).to_hex().to_string()
}

#[cfg(test)]
mod tests;
