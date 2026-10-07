//! Direct operations (`init`, `discover`): run synchronously through a
//! non-preloaded worker, with no job row and their own timeouts.

use super::accept_result;
use crate::{BridgeConfig, BridgeOutcome, Invocation, RuntimeBridge};
use manim_director_core::{
    DirectorSpec, EngineError, ErrorBody, InitMode, InitParams, InitResult, InitTask,
    OperationRequest, OperationResult, Task,
};
use std::{fs, path::Path, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const INIT_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_TEMPLATE: &str = "explainer";
const MAX_LISTED_ENTRIES: usize = 20;

/// Runs one direct task to completion or its timeout.
pub(crate) async fn run(
    bridge: &RuntimeBridge,
    root: &Path,
    task: &Task,
    timeout: Duration,
) -> Result<OperationResult, EngineError> {
    let request_id = Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    let run = bridge.run(
        Invocation {
            request_id: &request_id,
            project_root: root,
            task,
            preload: false,
            memory_mb: None,
        },
        cancel.clone(),
        |_| {},
    );
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
                manim_director_core::CancelledBy::Shutdown,
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

/// Creates a project in `target` from a template, or adds one scene template
/// to the project there.
pub async fn init_project(
    bridge: &BridgeConfig,
    target: &Path,
    params: InitParams,
) -> Result<InitResult, EngineError> {
    OperationRequest::Init(params.clone()).validate()?;
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
            }
        }
    };
    let runtime = RuntimeBridge::new(bridge.clone());
    match run(&runtime, &root, &Task::Init(task), INIT_TIMEOUT).await? {
        OperationResult::Init(result) => Ok(result),
        _ => Err(EngineError::internal("init returned another result")),
    }
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
