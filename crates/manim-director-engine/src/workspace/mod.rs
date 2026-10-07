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
pub use project::{SpecSnapshot, SpecTracker};
pub use scenes::SceneIndex;

use crate::{JobFilter, Store};
use anyhow::Result;
use findings::{FindingInputs, FindingView};
use latest::{scene_latest, Revisions, SceneLatest};
use manim_director_core::{
    Catalog, DoctorResult, JobStatus, Operation, OperationResult, Timestamp,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DoctorSnapshot {
    pub job_id: Uuid,
    pub finished_at: Option<Timestamp>,
    pub report: DoctorResult,
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

/// The newest successful environment report.
fn doctor(store: &Store) -> Result<Option<DoctorSnapshot>> {
    let job = store.newest_job(
        JobFilter {
            operations: &[Operation::Doctor],
            ..JobFilter::default()
        },
        &[JobStatus::Succeeded],
    )?;
    Ok(job.and_then(|job| match job.result {
        Some(OperationResult::Doctor(report)) => Some(DoctorSnapshot {
            job_id: job.id,
            finished_at: job.finished_at,
            report,
        }),
        _ => None,
    }))
}
