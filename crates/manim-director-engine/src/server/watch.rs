//! Keeps the workspace views current (HTTP §6.1.5, §6.1.9, §8.6): the scene
//! index, external edits to indexed files and `director.yaml`, runtime
//! catalog changes, and the doctor report a fresh server needs.

use super::state::{AppState, Shared};
use crate::{file_revision, workspace::Section, JobFilter};
use chrono::{Duration as Age, Utc};
use manim_director_core::{
    python_sources, relative_posix, DoctorParams, JobOrigin, JobStatus, Operation,
    OperationRequest, SPEC_FILE,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

/// Edits arrive in bursts (save, format, save); one scan follows them.
const REINDEX_DEBOUNCE: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_secs(1);
const DOCTOR_FRESH_FOR: Age = Age::hours(24);

/// What a `director.yaml` change can alter.
pub const SPEC_SECTIONS: [Section; 7] = [
    Section::Spec,
    Section::Project,
    Section::Profiles,
    Section::Themes,
    Section::Storyboard,
    Section::Scenes,
    Section::Findings,
];
const INDEX_SECTIONS: [Section; 5] = [
    Section::SceneIndex,
    Section::Scenes,
    Section::Storyboard,
    Section::Latest,
    Section::Findings,
];
const CATALOG_SECTIONS: [Section; 3] = [Section::Themes, Section::Project, Section::Findings];

/// One `discover` at a time; requests during a refresh make one follow-up.
pub async fn index(state: AppState, shutdown: CancellationToken) {
    state.request_reindex();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = state.reindex_wanted() => {}
        }
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(REINDEX_DEBOUNCE) => {}
        }
        if !state.claim_reindex() {
            continue;
        }
        state.index().lock().begin_refresh();
        state.publish_sections(&[Section::SceneIndex]).await;
        let outcome = tokio::select! {
            _ = shutdown.cancelled() => return,
            outcome = state.scheduler.discover() => outcome.map_err(|error| error.body()),
        };
        state.index().lock().finish_refresh(outcome);
        state.publish_sections(&INDEX_SECTIONS).await;
    }
}

type Stamp = Option<(SystemTime, u64)>;

#[derive(Default)]
struct Changes {
    python: bool,
    spec: bool,
}

/// Polls `(mtime, size)` of the indexed Python files and `director.yaml`;
/// a changed revision is announced and refreshes what depends on it.
pub async fn files(state: AppState, shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(POLL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut stamps: HashMap<String, Stamp> = HashMap::new();
    let mut announce = false;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {}
        }
        let scanner = state.clone();
        let scanned = tokio::task::spawn_blocking(move || {
            let changes = scan(&scanner, &mut stamps, announce);
            (stamps, changes)
        })
        .await;
        let changes;
        (stamps, changes) = match scanned {
            Ok(scanned) => scanned,
            Err(error) => {
                tracing::warn!(%error, "the file watcher stopped");
                return;
            }
        };
        announce = true;
        if changes.spec {
            state.publish_sections(&SPEC_SECTIONS).await;
        }
        if changes.spec || changes.python {
            state.request_reindex();
        }
    }
}

/// Blocking. The first scan only records what is there.
fn scan(state: &Shared, stamps: &mut HashMap<String, Stamp>, announce: bool) -> Changes {
    let root = state.root();
    // Later spec changes are loaded by the refresh they trigger.
    let spec = match announce {
        true => state.current_spec(),
        false => state.load_spec().spec,
    };
    let mut watched: Vec<(String, PathBuf)> = python_sources(root, &spec)
        .files
        .into_iter()
        .map(|path| (relative_posix(root, &path), path))
        .collect();
    watched.push((SPEC_FILE.to_owned(), root.join(SPEC_FILE)));
    let mut changes = Changes::default();
    let note = |path: &str, revision: Option<String>, changes: &mut Changes| {
        let changed = match announce {
            true => state.file_changed(path, revision),
            false => {
                state.file_seen(path, revision);
                false
            }
        };
        match (changed, path == SPEC_FILE) {
            (true, true) => changes.spec = true,
            (true, false) => changes.python = true,
            (false, _) => {}
        }
    };
    let mut current = HashMap::with_capacity(watched.len());
    for (path, file) in watched {
        let stamp = stamp(&file);
        if stamps.get(&path) != Some(&stamp) {
            let revision = stamp.and_then(|_| file_revision(&file).ok());
            note(&path, revision, &mut changes);
        }
        current.insert(path, stamp);
    }
    for path in stamps.keys().filter(|path| !current.contains_key(*path)) {
        note(path, None, &mut changes);
    }
    *stamps = current;
    changes
}

fn stamp(file: &Path) -> Stamp {
    let metadata = file.metadata().ok().filter(|metadata| metadata.is_file())?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Pushes `themes`, `project` and `findings` whenever a worker reports a
/// different runtime or catalog.
pub async fn catalog(state: AppState, shutdown: CancellationToken) {
    let mut runtime = state.scheduler.runtime();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            changed = runtime.changed() => if changed.is_err() { return },
        }
        state.publish_sections(&CATALOG_SECTIONS).await;
    }
}

/// Submits a `doctor` job unless one succeeded within a day and after the
/// runtime last changed.
pub async fn doctor(state: AppState) {
    let python = state.scheduler.python().to_path_buf();
    let fresh = state
        .scheduler
        .store()
        .blocking(move |store| {
            let Some(runtime) = store.runtime(&python)? else {
                return Ok(false);
            };
            let newest = store.newest_job(
                JobFilter {
                    operations: &[Operation::Doctor],
                    ..JobFilter::default()
                },
                &[JobStatus::Succeeded],
            )?;
            let finished = newest.and_then(|job| job.finished_at);
            Ok(finished.is_some_and(|finished| {
                finished.as_datetime() > Utc::now() - DOCTOR_FRESH_FOR
                    && finished > runtime.changed_at
            }))
        })
        .await;
    match fresh {
        Ok(true) => {}
        Ok(false) => {
            let request = OperationRequest::Doctor(DoctorParams {});
            if let Err(error) = state.scheduler.submit(JobOrigin::Engine, request).await {
                tracing::warn!(%error, "could not start the environment check");
            }
        }
        Err(error) => tracing::warn!(%error, "could not read the last environment check"),
    }
}
