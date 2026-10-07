//! Direct operations (`init`, `discover`): run synchronously through a
//! worker that preloads nothing, with no job row and their own timeouts.

use super::{accept_result, Inner};
use crate::{cache, BridgeConfig, BridgeEvent, BridgeOutcome, RuntimeBridge};
use manim_director_core::{
    python_sources, CancelledBy, DirectorSpec, DiscoverResult, DiscoverTask, EngineError,
    ErrorBody, Finding, InitMode, InitParams, InitResult, InitTask, Operation, OperationRequest,
    OperationResult, Task,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const INIT_TIMEOUT: Duration = Duration::from_secs(120);
const DISCOVER_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TEMPLATE: &str = "explainer";
const MAX_LISTED_ENTRIES: usize = 20;

/// Runs one direct task to completion or its timeout.
async fn run(
    bridge: &RuntimeBridge,
    root: &Path,
    task: &Task,
    timeout: Duration,
) -> Result<OperationResult, EngineError> {
    let cancel = CancellationToken::new();
    let mut ignore = |_: BridgeEvent<'_>| {};
    let run = bridge.run_direct(root, task, &cancel, &mut ignore);
    tokio::pin!(run);
    let outcome = tokio::select! {
        outcome = &mut run => outcome,
        _ = tokio::time::sleep(timeout) => {
            cancel.cancel();
            run.await;
            return Err(EngineError::Operation(ErrorBody::timeout(timeout.as_secs())));
        }
    };
    let value = match outcome {
        BridgeOutcome::Succeeded(value) => value,
        BridgeOutcome::Failed(error) => return Err(EngineError::Operation(error)),
        BridgeOutcome::Cancelled => {
            return Err(EngineError::Operation(ErrorBody::cancelled(
                CancelledBy::Shutdown,
            )))
        }
    };
    let root = root.to_path_buf();
    let task = task.clone();
    tokio::task::spawn_blocking(move || accept_result(&root, &task, value, None, u64::MAX))
        .await
        .map_err(EngineError::internal)?
        .map_err(EngineError::Operation)
}

struct PreparedDiscover {
    task: Task,
    spec: DirectorSpec,
    /// The scanned files' hashes from before the scan.
    inputs: BTreeMap<String, String>,
    cached: Option<DiscoverResult>,
    files: u32,
    truncated: bool,
    oversized: Vec<String>,
}

/// Scans the project's Python sources, answering from the cache when the
/// files and the runtime identity are unchanged.
pub(super) async fn discover(inner: &Arc<Inner>) -> Result<DiscoverResult, EngineError> {
    let prepared = {
        let inner = inner.clone();
        tokio::task::spawn_blocking(move || prepare_discover(&inner))
            .await
            .map_err(EngineError::internal)??
    };
    let mut result = match prepared.cached {
        Some(result) => result,
        None => {
            let outcome = run(&inner.bridge, &inner.root, &prepared.task, DISCOVER_TIMEOUT).await?;
            let OperationResult::Discover(result) = outcome else {
                return Err(EngineError::internal("discover returned another result"));
            };
            let (inner, task, spec) = (inner.clone(), prepared.task, prepared.spec);
            let (inputs, cached) = (prepared.inputs, OperationResult::Discover(result.clone()));
            tokio::task::spawn_blocking(move || {
                cache_discover(&inner, &spec, &task, inputs, &cached)
            })
            .await
            .map_err(EngineError::internal)??;
            result
        }
    };
    result.files = prepared.files;
    result.truncated |= prepared.truncated;
    for path in prepared.oversized {
        result.findings.push(Finding::warning(
            "file_too_large",
            format!("{path} is over 2 MiB and was not scanned."),
        ));
    }
    if prepared.truncated {
        result.findings.push(Finding::warning(
            "index_truncated",
            "Only the first 500 Python files were scanned.",
        ));
    }
    Ok(result)
}

fn prepare_discover(inner: &Inner) -> Result<PreparedDiscover, EngineError> {
    let spec = inner
        .load_spec()
        .ok()
        .or_else(|| inner.last_valid_spec.lock().clone())
        .unwrap_or_else(DirectorSpec::defaults);
    let sources = python_sources(&inner.root, &spec);
    let truncated = sources.truncated();
    let files = sources.files.len() as u32;
    let task = Task::Discover(DiscoverTask {
        files: sources.files,
    });
    let inputs = cache::input_hashes(&inner.root, &spec, &task).map_err(EngineError::internal)?;
    let cached = match inner.known_runtime()? {
        Some(runtime) => {
            let fingerprint = cache::Fingerprint::new(&runtime, &task, inputs.clone())
                .map_err(EngineError::internal)?;
            let entry = inner
                .store
                .cache_get(&fingerprint.value)
                .map_err(EngineError::internal)?;
            match entry.map(|entry| entry.result) {
                Some(OperationResult::Discover(result)) => Some(result),
                _ => None,
            }
        }
        None => None,
    };
    Ok(PreparedDiscover {
        task,
        spec,
        inputs,
        cached,
        files,
        truncated,
        oversized: sources.oversized,
    })
}

/// Caches a scan under the identity of the worker that just ran it, unless a
/// scanned file changed meanwhile: the scan may then describe either version,
/// so the next discover must look again.
fn cache_discover(
    inner: &Inner,
    spec: &DirectorSpec,
    task: &Task,
    inputs: BTreeMap<String, String>,
    result: &OperationResult,
) -> Result<(), EngineError> {
    let Some(runtime) = inner.known_runtime()? else {
        return Ok(());
    };
    if cache::input_hashes(&inner.root, spec, task).ok().as_ref() != Some(&inputs) {
        return Ok(());
    }
    let fingerprint =
        cache::Fingerprint::new(&runtime, task, inputs).map_err(EngineError::internal)?;
    inner
        .store
        .cache_put(&fingerprint.value, None, result, Operation::Discover)
        .map_err(EngineError::internal)
}

/// Creates a project in `target` from a template, or adds one scene template
/// to the project there.
pub async fn init_project(
    bridge: &BridgeConfig,
    target: &Path,
    params: InitParams,
) -> Result<InitResult, EngineError> {
    OperationRequest::Init(params.clone()).validate()?;
    let target = target.to_path_buf();
    let (root, task) = tokio::task::spawn_blocking(move || preflight(&target, &params))
        .await
        .map_err(EngineError::internal)??;
    let runtime = RuntimeBridge::new(bridge.clone());
    match run(&runtime, &root, &Task::Init(task), INIT_TIMEOUT).await? {
        OperationResult::Init(result) => Ok(result),
        _ => Err(EngineError::internal("init returned another result")),
    }
}

/// Creates the target directory and builds the task: add-scene mode needs a
/// valid spec, create mode an empty directory unless `force`.
fn preflight(target: &Path, params: &InitParams) -> Result<(PathBuf, InitTask), EngineError> {
    fs::create_dir_all(target).map_err(EngineError::internal)?;
    let root = target.canonicalize().map_err(EngineError::internal)?;
    let task = match &params.scene_template {
        Some(scene_template) => {
            let spec = DirectorSpec::load(&root).map_err(EngineError::from)?;
            InitTask {
                mode: InitMode::AddScene,
                name: None,
                template: None,
                scene_template: Some(scene_template.clone()),
                theme: None,
                seed: None,
                source_dir: Some(root.join(&spec.project.source_dir)),
                force: params.force,
            }
        }
        None => {
            let entries = project_entries(&root).map_err(EngineError::internal)?;
            if !entries.is_empty() && !params.force {
                return Err(EngineError::ProjectNotEmpty { entries });
            }
            let directory_name = root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            InitTask {
                mode: if params.force {
                    InitMode::Overwrite
                } else {
                    InitMode::Create
                },
                name: params.name.clone().or(directory_name),
                template: Some(
                    params
                        .template
                        .clone()
                        .unwrap_or_else(|| DEFAULT_TEMPLATE.into()),
                ),
                scene_template: None,
                theme: params.theme.clone(),
                seed: params.seed,
                source_dir: None,
                force: false,
            }
        }
    };
    Ok((root, task))
}

/// Entries that make a directory a non-empty project: everything except
/// `.git` and engine state that holds nothing but the database and artifacts.
fn project_entries(root: &Path) -> std::io::Result<Vec<String>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let engine_state_only = name == ".manim-director" && holds_only_state(&entry.path())?;
        if name != ".git" && !engine_state_only {
            entries.push(name);
        }
    }
    entries.sort();
    entries.truncate(MAX_LISTED_ENTRIES);
    Ok(entries)
}

fn holds_only_state(path: &Path) -> std::io::Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let name = entry?.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("state.db") || name == "artifacts") {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_state_and_git_do_not_make_a_project_occupied() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join(".manim-director/artifacts")).unwrap();
        fs::write(root.join(".manim-director/state.db"), "").unwrap();
        fs::write(root.join(".manim-director/state.db-wal"), "").unwrap();
        assert!(project_entries(root).unwrap().is_empty());
        fs::write(root.join(".manim-director/undo"), "").unwrap();
        fs::write(root.join("notes.txt"), "mine").unwrap();
        assert_eq!(
            project_entries(root).unwrap(),
            [".manim-director", "notes.txt"]
        );
    }

    #[test]
    fn force_reaches_the_runtime_as_overwrite_or_as_the_add_scene_flag() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let forced = |scene_template: Option<&str>| InitParams {
            scene_template: scene_template.map(str::to_owned),
            force: true,
            ..InitParams::default()
        };
        let (_, created) = preflight(root, &forced(None)).unwrap();
        assert_eq!((created.mode, created.force), (InitMode::Overwrite, false));
        fs::write(
            root.join("director.yaml"),
            "version: 1\nproject:\n  name: Demo\n",
        )
        .unwrap();
        let (root, added) = preflight(root, &forced(Some("graph"))).unwrap();
        assert_eq!((added.mode, added.force), (InitMode::AddScene, true));
        assert_eq!(added.source_dir, Some(root.join("scenes")));
    }

    #[tokio::test]
    async fn init_refuses_an_occupied_directory_before_spawning() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("notes.txt"), "mine").unwrap();
        let unreachable = BridgeConfig {
            python: "/nonexistent/python".into(),
            module: "missing".into(),
        };
        let error = init_project(&unreachable, directory.path(), InitParams::default())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            EngineError::ProjectNotEmpty {
                entries: vec!["notes.txt".into()]
            }
        );
    }
}
