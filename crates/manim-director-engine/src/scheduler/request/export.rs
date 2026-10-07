//! `export` resolution: the zip entry list (one rule set, computed by the
//! engine) or a single media delivery.

use super::{
    paths::{project_path, PathUse},
    source::{self, Consumer, SelectedSource},
    ProjectContext,
};
use crate::scheduler::artifacts;
use manim_director_core::{
    files, relative_posix, Budget, DirectorSpec, EngineError, ExportEntry, ExportFormat,
    ExportParams, ExportTask, GifSettings, MediaExportFormat, MediaExportTask, Task, ZipExportTask,
    ZipFormat,
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const MAX_ZIP_ENTRIES: usize = 10_000;

pub(super) fn task(
    ctx: &ProjectContext<'_>,
    params: &ExportParams,
    budget_bytes: u64,
) -> Result<(Task, Option<SelectedSource>), EngineError> {
    let source = source::select(
        ctx,
        params.source.as_ref(),
        params.scene.as_deref(),
        params.profile.as_deref(),
        Consumer::Video,
    );
    let format = match params.format {
        ExportFormat::Zip => {
            let source = match source {
                Ok(source) => Some(source),
                Err(EngineError::SourceNotFound { .. }) if params.source.is_none() => None,
                Err(error) => return Err(error),
            };
            return zip(ctx, params, source, budget_bytes);
        }
        ExportFormat::Mp4 => MediaExportFormat::Mp4,
        ExportFormat::Webm => MediaExportFormat::Webm,
        ExportFormat::Gif => MediaExportFormat::Gif,
    };
    let source = source?;
    let stem = source.reference.scene.clone().unwrap_or_else(|| {
        source.absolute.file_stem().map_or_else(
            || "export".into(),
            |stem| stem.to_string_lossy().into_owned(),
        )
    });
    let output = output_path(ctx, params, &format!("{stem}.{}", params.format))?;
    let task = Task::Export(ExportTask::Media(MediaExportTask {
        format,
        output,
        source: source.absolute.clone(),
        alpha: source.media.has_alpha && format == MediaExportFormat::Webm,
        gif: (format == MediaExportFormat::Gif).then(|| GifSettings {
            fps: params.gif_fps.unwrap_or(15),
            width: params.gif_width.unwrap_or(960),
        }),
    }));
    Ok((task, Some(source)))
}

fn zip(
    ctx: &ProjectContext<'_>,
    params: &ExportParams,
    source: Option<SelectedSource>,
    budget_bytes: u64,
) -> Result<(Task, Option<SelectedSource>), EngineError> {
    let spec = ctx.spec()?;
    let output = output_path(ctx, params, &format!("{}.zip", slug(&spec.project.name)))?;
    let mut entries = project_entries(ctx.root, spec);
    if let Some(source) = &source {
        entries.extend(deliverables(ctx.root, source));
    }
    if entries.len() > MAX_ZIP_ENTRIES {
        return Err(EngineError::BudgetExceeded {
            budget: Budget::ExportEntries,
            limit: MAX_ZIP_ENTRIES as u64,
            actual: entries.len() as u64,
        });
    }
    let bytes: u64 = entries
        .iter()
        .filter_map(|entry| fs::metadata(&entry.path).ok())
        .map(|metadata| metadata.len())
        .sum();
    if bytes > budget_bytes {
        return Err(EngineError::BudgetExceeded {
            budget: Budget::ExportBytes,
            limit: budget_bytes,
            actual: bytes,
        });
    }
    let task = Task::Export(ExportTask::Zip(ZipExportTask {
        format: ZipFormat::Zip,
        output,
        project_name: spec.project.name.clone(),
        source_job_id: source.as_ref().and_then(|source| source.reference.job_id),
        entries,
    }));
    Ok((task, source))
}

/// The explicit output, or `<output_dir>/<file_name>` (which needs the spec).
fn output_path(
    ctx: &ProjectContext<'_>,
    params: &ExportParams,
    file_name: &str,
) -> Result<PathBuf, EngineError> {
    let spec = ctx.spec.as_ref().ok();
    let relative = match &params.output {
        Some(output) => output.clone(),
        None => {
            let output_dir = &ctx.spec()?.project.output_dir;
            match output_dir.trim_end_matches('/') {
                "" | "." => file_name.to_owned(),
                dir => format!("{dir}/{file_name}"),
            }
        }
    };
    project_path(
        ctx.root,
        "output",
        &relative,
        PathUse::Output {
            media_dir: spec.map_or(super::DEFAULT_MEDIA_DIR, |spec| &spec.project.media_dir),
            extensions: &[params.format.as_str()],
        },
    )
}

/// Project files a bundle carries, keyed and sorted by archive path.
fn project_entries(root: &Path, spec: &DirectorSpec) -> Vec<ExportEntry> {
    let project = &spec.project;
    let ignore = spec.ignore_set(root);
    let mut paths = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let regular = entry.file_type().is_ok_and(|kind| kind.is_file());
            if regular && is_root_bundle_file(&name) {
                paths.insert(entry.path());
            }
        }
    }
    for dir in [project.source_dir.as_str(), &project.asset_dir, "sources"] {
        paths.extend(ignore.files_under(&root.join(dir)));
    }
    let referenced = spec
        .engine
        .source
        .iter()
        .chain(&spec.inputs.data)
        .chain(
            spec.inputs
                .sources
                .iter()
                .filter_map(|source| source.path.as_ref()),
        )
        .chain(&spec.captions.source)
        .chain(&spec.narration.manifest)
        .chain(&spec.narration.source);
    for value in referenced {
        if let Ok(path) = crate::confine(root, value) {
            paths.insert(path);
        }
    }
    let excluded = [
        root.join(".manim-director"),
        root.join(&project.output_dir),
        root.join(&project.media_dir),
    ];
    paths
        .into_iter()
        .filter(|path| !excluded.iter().any(|dir| path.starts_with(dir)))
        .filter(|path| !files::is_secret_like(path))
        .map(|path| ExportEntry {
            archive_path: relative_posix(root, &path),
            path,
        })
        .collect()
}

fn is_root_bundle_file(name: &str) -> bool {
    matches!(name, "director.yaml" | "manim.cfg" | "pyproject.toml")
        || (name.starts_with("requirements") && name.ends_with(".txt"))
        || name.starts_with("README")
        || name.starts_with("LICENSE")
}

/// Every artifact of the source job as `deliverables/<file name>`.
fn deliverables(root: &Path, source: &SelectedSource) -> Vec<ExportEntry> {
    let job_artifacts: Vec<PathBuf> = match source.job.as_ref().and_then(|job| job.result.as_ref())
    {
        Some(result) => result
            .artifacts()
            .iter()
            .filter_map(|artifact| artifacts::existing(root, artifact))
            .collect(),
        None => vec![source.absolute.clone()],
    };
    let prefix: String = source
        .reference
        .job_id
        .map(|id| id.to_string().chars().take(8).collect())
        .unwrap_or_else(|| "source".into());
    let mut taken = BTreeSet::new();
    let mut entries = Vec::new();
    for path in job_artifacts {
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let mut archive_name = name.clone();
        let mut attempt = 1;
        while taken.contains(&archive_name) {
            archive_name = if attempt == 1 {
                format!("{prefix}-{name}")
            } else {
                format!("{prefix}-{attempt}-{name}")
            };
            attempt += 1;
        }
        taken.insert(archive_name.clone());
        entries.push(ExportEntry {
            path,
            archive_path: format!("deliverables/{archive_name}"),
        });
    }
    entries
}

fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "project".into()
    } else {
        slug.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_lowercase_dashed_and_never_empty() {
        assert_eq!(slug("My Film: Part 2"), "my-film-part-2");
        assert_eq!(slug("¿?"), "project");
    }

    #[test]
    fn bundle_root_files_follow_the_contract_patterns() {
        for name in [
            "director.yaml",
            "manim.cfg",
            "pyproject.toml",
            "requirements.txt",
            "requirements-dev.txt",
            "README.md",
            "LICENSE",
        ] {
            assert!(is_root_bundle_file(name), "{name}");
        }
        for name in ["requirements.in", "notes.md", "setup.py"] {
            assert!(!is_root_bundle_file(name), "{name}");
        }
    }

    #[test]
    fn project_entries_skip_outputs_state_ignored_dirs_and_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        for path in [
            "director.yaml",
            "README.md",
            "notes.txt",
            "scenes/main.py",
            "scenes/.env",
            "scenes/__pycache__/main.pyc",
            "assets/logo.svg",
            "sources/paper.pdf",
            "data/values.csv",
            "output/old.mp4",
            ".manim-director/state.db",
        ] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        fs::write(
            root.join("director.yaml"),
            "version: 1\nproject:\n  name: Demo\ninputs:\n  data: [data/values.csv, missing.csv]\n",
        )
        .unwrap();
        let spec = DirectorSpec::load(&root).unwrap();
        let archive: Vec<_> = project_entries(&root, &spec)
            .into_iter()
            .map(|entry| entry.archive_path)
            .collect();
        assert_eq!(
            archive,
            [
                "README.md",
                "assets/logo.svg",
                "data/values.csv",
                "director.yaml",
                "scenes/main.py",
                "sources/paper.pdf"
            ]
        );
    }
}
