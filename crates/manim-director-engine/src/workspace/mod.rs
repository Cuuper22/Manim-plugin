//! The workbench-shaped views of a project (HTTP §5, §6.1): pure
//! derivations from the spec, the scene index, the runtime catalog, the job
//! store and the filesystem, unit-testable without HTTP.

mod artifacts;
mod findings;
mod jobs;
mod latest;
mod project;
mod scenes;

pub use artifacts::{content_type, file_version};
pub use jobs::{job_detail, job_summary, JobDetail, JobView};
pub use project::{project_theme, SpecSnapshot, SpecTracker};
pub use scenes::SceneIndex;

use crate::{JobFilter, Store};
use anyhow::Result;
use findings::{FindingInputs, FindingView};
use latest::{scene_latest, Revisions, SceneLatest};
use manim_director_core::{
    Catalog, DoctorResult, ErrorBody, JobRecord, JobStatus, Operation, OperationResult, Timestamp,
};
use project::{ProfileView, ProjectSummary, SpecStatus, ThemeView};
use scenes::{Scene, SceneIndexStatus, StoryboardBeatView};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;

/// A top-level section a `workspace` event can replace wholesale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Project,
    Spec,
    Profiles,
    Themes,
    SceneIndex,
    Scenes,
    Storyboard,
    Latest,
    Findings,
    Doctor,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Self::Project,
        Self::Spec,
        Self::Profiles,
        Self::Themes,
        Self::SceneIndex,
        Self::Scenes,
        Self::Storyboard,
        Self::Latest,
        Self::Findings,
        Self::Doctor,
    ];
}

/// Some or all sections; each present section is complete.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Sections {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<SpecStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Vec<ProfileView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub themes: Option<Vec<ThemeView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_index: Option<SceneIndexStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenes: Option<Vec<Scene>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storyboard: Option<Vec<StoryboardBeatView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<BTreeMap<String, SceneLatest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings: Option<Vec<FindingView>>,
    /// `Some(None)` is a present section whose value is `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doctor: Option<Option<DoctorSnapshot>>,
}

/// The newest environment check, whatever it found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DoctorSnapshot {
    pub job_id: Uuid,
    pub finished_at: Option<Timestamp>,
    /// Non-null iff the check succeeded.
    pub report: Option<DoctorResult>,
    /// Non-null iff it failed, e.g. `runtime_unavailable`.
    pub error: Option<ErrorBody>,
}

/// Everything the views derive from, read once by the caller.
pub struct ViewInputs<'a> {
    pub root: &'a Path,
    pub spec: &'a SpecSnapshot,
    pub index: &'a SceneIndex,
    pub catalog: Option<&'a Catalog>,
    pub store: &'a Store,
}

/// Derives the `wanted` sections. Blocking (database, stats, hashing).
pub fn sections(inputs: &ViewInputs<'_>, wanted: &[Section]) -> Result<Sections> {
    let wants = |section| wanted.contains(&section);
    let (root, spec, catalog) = (inputs.root, &inputs.spec.spec, inputs.catalog);
    let theme = project::project_theme(spec, catalog);
    let mut out = Sections {
        project: wants(Section::Project)
            .then(|| project::project_summary(root, inputs.spec, catalog)),
        spec: wants(Section::Spec).then(|| inputs.spec.status.clone()),
        profiles: wants(Section::Profiles).then(|| project::profiles(spec)),
        themes: wants(Section::Themes).then(|| project::themes(catalog, theme.as_deref())),
        scene_index: wants(Section::SceneIndex).then(|| inputs.index.status()),
        ..Sections::default()
    };
    let scene_sections = [
        Section::Scenes,
        Section::Storyboard,
        Section::Latest,
        Section::Findings,
    ];
    if scene_sections.into_iter().any(wants) {
        let scenes = scenes::scenes(spec, inputs.index);
        let mut revisions = Revisions::new(root);
        let mut latest = BTreeMap::new();
        if wants(Section::Latest) || wants(Section::Findings) {
            for scene in &scenes {
                let found = scene_latest(inputs.store, root, scene, &mut revisions)?;
                latest.insert(scene.id.clone(), found);
            }
        }
        if wants(Section::Findings) {
            let found = findings::findings(
                &FindingInputs {
                    root,
                    spec: inputs.spec,
                    catalog,
                    index: inputs.index,
                    scenes: &scenes,
                    latest: &latest,
                    store: inputs.store,
                },
                &mut revisions,
            )?;
            out.findings = Some(found);
        }
        out.storyboard = wants(Section::Storyboard).then(|| scenes::storyboard(spec, &scenes));
        out.latest = wants(Section::Latest).then_some(latest);
        out.scenes = wants(Section::Scenes).then_some(scenes);
    }
    if wants(Section::Doctor) {
        out.doctor = Some(doctor(inputs.store)?);
    }
    Ok(out)
}

/// The newest `doctor` job that succeeded or failed; a cancelled one found
/// nothing. Blocking.
pub fn newest_check(store: &Store) -> Result<Option<JobRecord>> {
    store.newest_job(
        JobFilter {
            operations: &[Operation::Doctor],
            ..JobFilter::default()
        },
        &[JobStatus::Succeeded, JobStatus::Failed],
    )
}

/// A failed check outranks an older report: the runtime may have broken since.
fn doctor(store: &Store) -> Result<Option<DoctorSnapshot>> {
    Ok(newest_check(store)?.map(|job| DoctorSnapshot {
        job_id: job.id,
        finished_at: job.finished_at,
        report: match job.result {
            Some(OperationResult::Doctor(report)) => Some(report),
            _ => None,
        },
        error: job.error,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing;
    use manim_director_core::{DoctorParams, DoctorTask, OperationRequest, Task};
    use serde_json::json;

    #[test]
    fn the_doctor_section_is_the_newest_check_whatever_it_found() {
        let (_db, store) = testing::store();
        let (request, task) = (
            OperationRequest::Doctor(DoctorParams {}),
            Task::Doctor(DoctorTask {}),
        );
        let check = || {
            let id = Uuid::new_v4();
            store
                .insert_job(&testing::new_job(id, &request, &task))
                .unwrap();
            store.set_running(id).unwrap();
            id
        };
        assert_eq!(doctor(&store).unwrap(), None);

        let passed = check();
        let report: DoctorResult = serde_json::from_value(json!({
            "ok": true,
            "runtime": {"version": "2.0.0", "protocol": 2, "python": "3.12.3",
                        "executable": "/usr/bin/python3", "platform": "linux"},
            "checks": [],
            "capabilities": {"render": true, "renderers": ["cairo"], "latex": true,
                             "video_tools": true, "visual_qa": true, "symbolic_math": true,
                             "pdf_ingest": true},
            "disk": {"free_bytes": 1, "total_bytes": 2},
            "findings": [],
            "artifacts": [],
        }))
        .unwrap();
        let result = OperationResult::Doctor(report.clone());
        store.finish_success(passed, &result, None).unwrap();
        let snapshot = doctor(&store).unwrap().unwrap();
        assert_eq!(
            (snapshot.job_id, snapshot.report, snapshot.error),
            (passed, Some(report), None)
        );

        // The runtime broke since; a cancelled check found nothing either way.
        let broken = check();
        let unavailable = ErrorBody::new("runtime_unavailable", "Python was not found.", None);
        store
            .finish_error(broken, JobStatus::Failed, &unavailable, None)
            .unwrap();
        let cancelled = ErrorBody::new("cancelled", "Cancelled.", None);
        store
            .finish_error(check(), JobStatus::Cancelled, &cancelled, None)
            .unwrap();
        let snapshot = serde_json::to_value(doctor(&store).unwrap()).unwrap();
        assert_eq!(snapshot["job_id"], json!(broken));
        assert_eq!(snapshot["report"], json!(null));
        assert_eq!(snapshot["error"]["code"], "runtime_unavailable");
    }
}
