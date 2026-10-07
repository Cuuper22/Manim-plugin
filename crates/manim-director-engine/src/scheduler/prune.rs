//! Bounded history (OPS §1.8): old terminal jobs, cache rows whose artifacts
//! are gone, and surplus undo snapshots. Blocking; long-lived engines run it
//! at start and every ten minutes.

use super::{artifacts, latest};
use crate::{JobLinks, Store, UNDO_DIR};
use anyhow::Result;
use chrono::{TimeDelta, Utc};
use manim_director_core::Timestamp;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use walkdir::WalkDir;

const KEEP_DISCOVER_RESULTS: usize = 50;
const KEEP_SNAPSHOTS_PER_FILE: usize = 20;

#[derive(Debug, Clone, Copy)]
pub struct PrunePolicy {
    pub keep_jobs: usize,
    pub keep_days: u32,
}

impl PrunePolicy {
    pub fn from_env() -> Self {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.trim().parse::<u64>().ok())
        };
        Self {
            keep_jobs: read("MANIM_DIRECTOR_KEEP_JOBS")
                .map_or(500, |value| value.clamp(50, 100_000)) as usize,
            keep_days: read("MANIM_DIRECTOR_KEEP_DAYS").map_or(30, |value| value.clamp(1, 3650))
                as u32,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pruned {
    pub jobs: usize,
    pub cache_rows: usize,
    pub snapshots: usize,
}

pub fn prune(store: &Store, root: &Path, policy: PrunePolicy) -> Result<Pruned> {
    let cutoff = Timestamp::from(Utc::now() - TimeDelta::days(policy.keep_days.into()));
    Ok(Pruned {
        jobs: prune_jobs(store, root, policy.keep_jobs, cutoff)?,
        cache_rows: prune_cache(store, root)?,
        snapshots: prune_snapshots(&root.join(UNDO_DIR), KEEP_SNAPSHOTS_PER_FILE)?,
    })
}

/// Deletes candidates except what views still show: every scene's `latest`
/// jobs, and (transitively) every job a kept job was cached from or read.
fn prune_jobs(store: &Store, root: &Path, keep: usize, cutoff: Timestamp) -> Result<usize> {
    let candidates = store.prune_candidates(keep, cutoff)?;
    if candidates.is_empty() {
        return Ok(0);
    }
    let candidate_set: HashSet<Uuid> = candidates.iter().copied().collect();
    let links = store.job_links()?;
    let links: HashMap<Uuid, JobLinks> = links.into_iter().map(|link| (link.id, link)).collect();
    let mut kept: HashSet<Uuid> = links
        .keys()
        .filter(|id| !candidate_set.contains(id))
        .copied()
        .collect();
    for (class, file) in store.scene_pairs()? {
        let found = latest(store, root, &class, &file)?;
        kept.extend(
            [found.video, found.still, found.contact_sheet]
                .into_iter()
                .flatten()
                .map(|job| job.id),
        );
    }
    let mut frontier: Vec<Uuid> = kept.iter().copied().collect();
    while let Some(id) = frontier.pop() {
        let Some(link) = links.get(&id) else {
            continue;
        };
        for referenced in [link.cached_from, link.source_job_id].into_iter().flatten() {
            if kept.insert(referenced) {
                frontier.push(referenced);
            }
        }
    }
    let doomed: Vec<Uuid> = candidates
        .into_iter()
        .filter(|id| !kept.contains(id))
        .collect();
    let deleted = store.delete_jobs(&doomed)?;
    for id in &doomed {
        if let Err(error) = artifacts::remove_out_dir(root, *id) {
            tracing::warn!(%id, %error, "could not remove a pruned job's artifacts");
        }
    }
    Ok(deleted)
}

fn prune_cache(store: &Store, root: &Path) -> Result<usize> {
    let dead: Vec<String> = store
        .artifact_cache_rows()?
        .into_iter()
        .filter(|(_, result)| {
            !result
                .as_ref()
                .is_some_and(|result| artifacts::intact(root, result))
        })
        .map(|(fingerprint, _)| fingerprint)
        .collect();
    Ok(store.delete_cache_rows(&dead)? + store.trim_discover_cache(KEEP_DISCOVER_RESULTS)?)
}

/// Snapshot directories are named `<UTC timestamp>-<uuid>`, so name order is
/// age order. Never follows symlinks.
fn prune_snapshots(undo: &Path, keep: usize) -> Result<usize> {
    if !fs::symlink_metadata(undo).is_ok_and(|metadata| metadata.is_dir()) {
        return Ok(0);
    }
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(undo)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            snapshots.push(entry.path());
        }
    }
    let mut versions: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for snapshot in &snapshots {
        for entry in WalkDir::new(snapshot).follow_links(false) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let relative = entry.path().strip_prefix(snapshot)?.to_path_buf();
                versions
                    .entry(relative)
                    .or_default()
                    .push(entry.path().to_path_buf());
            }
        }
    }
    let mut removed = 0;
    for paths in versions.values_mut() {
        if paths.len() <= keep {
            continue;
        }
        paths.sort_by(|a, b| b.cmp(a));
        for path in &paths[keep..] {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    for snapshot in &snapshots {
        for entry in WalkDir::new(snapshot)
            .contents_first(true)
            .follow_links(false)
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                // Fails while the directory still holds a kept version.
                let _ = fs::remove_dir(entry.path());
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::testing::queued_job, Finish, NewJob};
    use manim_director_core::{
        Artifact, ArtifactKind, ErrorBody, JobOrigin, JobStatus, Limits, MediaFormat, Operation,
        OperationRequest, OperationResult, RenderParams, RenderResult, RenderSettings, RenderTask,
        Renderer, SceneRef, Task, ARTIFACTS_DIR,
    };

    fn finished(store: &Store, root: &Path, status: JobStatus) -> Uuid {
        let id = Uuid::new_v4();
        queued_job(store, id, Uuid::new_v4());
        store.set_running(id).unwrap();
        let out_dir = root.join(ARTIFACTS_DIR).join(id.to_string());
        artifacts::create_out_dir(root, &out_dir).unwrap();
        let finish = match status {
            JobStatus::Succeeded => store
                .finish_success(id, &crate::db::testing::diagnosis(), None)
                .unwrap(),
            _ => store
                .finish_error(id, status, &ErrorBody::internal("x"), None)
                .unwrap(),
        };
        assert!(matches!(finish, Finish::Ended(_)));
        id
    }

    fn project() -> (tempfile::TempDir, PathBuf, Store) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let store = Store::open(root.join(".manim-director/state.db")).unwrap();
        (dir, root, store)
    }

    #[test]
    fn old_jobs_go_with_their_artifacts_but_active_and_recent_ones_stay() {
        let (_dir, root, store) = project();
        let old: Vec<Uuid> = (0..3)
            .map(|_| finished(&store, &root, JobStatus::Failed))
            .collect();
        let active = Uuid::new_v4();
        queued_job(&store, active, Uuid::new_v4());
        let recent = finished(&store, &root, JobStatus::Succeeded);
        let policy = PrunePolicy {
            keep_jobs: 1,
            keep_days: 30,
        };
        let pruned = prune(&store, &root, policy).unwrap();
        assert_eq!(pruned.jobs, 3);
        for id in &old {
            assert!(store.get_job(*id).unwrap().is_none());
            assert!(!root.join(ARTIFACTS_DIR).join(id.to_string()).exists());
        }
        assert!(store.get_job(active).unwrap().is_some());
        assert!(store.get_job(recent).unwrap().is_some());
        assert!(root.join(ARTIFACTS_DIR).join(recent.to_string()).exists());
    }

    fn render(store: &Store, root: &Path, cached_from: Option<Uuid>) -> Uuid {
        let id = Uuid::new_v4();
        let out_dir = root.join(ARTIFACTS_DIR).join(id.to_string());
        let request = OperationRequest::Render(RenderParams::default());
        let task = Task::Render(RenderTask {
            scene: Some("Intro".into()),
            files: vec![],
            settings: RenderSettings {
                profile: "draft".into(),
                width: 854,
                height: 480,
                fps: 15,
                renderer: Renderer::Cairo,
                format: MediaFormat::Mp4,
                transparent: false,
            },
            media_dir: root.join("media"),
            out_dir: out_dir.clone(),
            sections: false,
            fresh: false,
        });
        let origin = cached_from.unwrap_or(id);
        let video = format!("{ARTIFACTS_DIR}/{origin}/Intro.mp4");
        let result = OperationResult::Render(RenderResult {
            scene: SceneRef {
                name: "Intro".into(),
                file: "scenes/main.py".into(),
            },
            duration_seconds: 1.0,
            animations: 1,
            artifacts: vec![Artifact {
                kind: ArtifactKind::Video,
                path: video.clone(),
                label: None,
                bytes: 5,
                media: None,
            }],
        });
        let job = NewJob {
            id,
            origin: JobOrigin::Cli,
            owner: Uuid::new_v4(),
            request: &request,
            task: &task,
            limits: Limits {
                timeout_seconds: 60,
                memory_mb: None,
            },
            fingerprint: None,
            source_job_id: None,
            scene_class: Some("Intro"),
            scene_file: Some("scenes/main.py"),
            scene_revision: None,
            profile: Some("draft"),
        };
        match cached_from {
            Some(origin) => {
                store
                    .insert_cached_job(&job, Some(origin), &result)
                    .unwrap();
            }
            None => {
                store.insert_job(&job).unwrap();
                store.set_running(id).unwrap();
                fs::create_dir_all(&out_dir).unwrap();
                fs::write(root.join(&video), b"video").unwrap();
                store.finish_success(id, &result, None).unwrap();
            }
        }
        id
    }

    #[test]
    fn a_scenes_latest_render_and_its_cache_origin_survive() {
        let (_dir, root, store) = project();
        let original = render(&store, &root, None);
        let cached = render(&store, &root, Some(original));
        for _ in 0..3 {
            finished(&store, &root, JobStatus::Failed);
        }
        let policy = PrunePolicy {
            keep_jobs: 1,
            keep_days: 30,
        };
        assert_eq!(prune(&store, &root, policy).unwrap().jobs, 2);
        assert!(store.get_job(cached).unwrap().is_some(), "latest is kept");
        assert!(
            store.get_job(original).unwrap().is_some(),
            "the job a kept cache hit came from is kept"
        );
        assert!(root.join(ARTIFACTS_DIR).join(original.to_string()).is_dir());
    }

    #[test]
    fn cache_rows_without_their_files_are_dropped() {
        let (_dir, root, store) = project();
        let missing = OperationResult::Render(RenderResult {
            scene: SceneRef {
                name: "Intro".into(),
                file: "scenes/main.py".into(),
            },
            duration_seconds: 1.0,
            animations: 1,
            artifacts: vec![Artifact {
                kind: ArtifactKind::Video,
                path: "gone.mp4".into(),
                label: None,
                bytes: 5,
                media: None,
            }],
        });
        store
            .cache_put("dead", None, &missing, Operation::Render)
            .unwrap();
        store
            .cache_put(
                "alive",
                None,
                &crate::db::testing::diagnosis(),
                Operation::Diagnose,
            )
            .unwrap();
        assert_eq!(prune_cache(&store, &root).unwrap(), 1);
        assert!(store.cache_get("dead").unwrap().is_none());
        assert!(store.cache_get("alive").unwrap().is_some());
    }

    #[test]
    fn only_the_newest_snapshots_per_file_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let undo = dir.path().join("undo");
        for index in 0..5 {
            let snapshot = undo.join(format!("20261007T1000{index:02}.000Z-x"));
            fs::create_dir_all(snapshot.join("scenes")).unwrap();
            fs::write(snapshot.join("scenes/main.py"), index.to_string()).unwrap();
        }
        let other = undo.join("20261007T090000.000Z-y");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("director.yaml"), "x").unwrap();
        assert_eq!(prune_snapshots(&undo, 2).unwrap(), 3);
        let mut left: Vec<_> = fs::read_dir(&undo)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "20261007T090000.000Z-y",
                "20261007T100003.000Z-x",
                "20261007T100004.000Z-x"
            ]
        );
    }
}
