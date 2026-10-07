//! The compact project summary shown by CLI and MCP `inspect`.

use crate::{
    workspace::{self, project_theme, SceneIndex, Section, SpecTracker, ViewInputs},
    Scheduler,
};
use manim_director_core::{DirectorSpec, EngineError, Finding, JobStatus, Operation, Timestamp};
use serde::Serialize;
use std::path::PathBuf;
use uuid::Uuid;

const MAX_FINDINGS: usize = 20;
const RECENT_JOBS: usize = 10;

#[derive(Debug, Clone, Serialize)]
pub struct Inspect {
    pub name: String,
    pub root: PathBuf,
    pub main_scene: Option<String>,
    pub default_profile: String,
    pub theme: Option<String>,
    pub profiles: Vec<String>,
    pub scenes: Vec<InspectScene>,
    pub findings: Vec<Finding>,
    pub latest: Vec<InspectLatest>,
    pub recent_jobs: Vec<RecentJob>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectScene {
    pub scene_id: String,
    pub name: String,
    pub file: String,
    pub line: u32,
    pub sections: usize,
    pub beats: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectLatest {
    pub scene_id: String,
    pub video: Option<String>,
    pub still: Option<String>,
    pub contact_sheet: Option<String>,
    pub video_job_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentJob {
    pub id: Uuid,
    pub operation: Operation,
    pub status: JobStatus,
    pub created_at: Timestamp,
}

/// The project, its scenes, latest artifacts and findings as the workbench
/// derives them (declared scenes first; spec, index, render, QA and doctor
/// findings), compacted for a terminal or an agent.
pub async fn inspect(scheduler: &Scheduler) -> Result<Inspect, EngineError> {
    let root = scheduler.root().to_path_buf();
    let spec = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || DirectorSpec::load(&root))
            .await
            .map_err(EngineError::internal)?
            .map_err(EngineError::from)?
    };
    let discovered = scheduler.discover().await.map_err(|error| error.body());
    // After discover, whose worker has reported the runtime's catalog.
    let catalog = scheduler
        .runtime()
        .borrow()
        .as_ref()
        .map(|identity| identity.catalog.clone());
    let store = scheduler.store().clone();
    let (views, recent_jobs) = {
        let (root, catalog) = (root.clone(), catalog.clone());
        tokio::task::spawn_blocking(move || {
            let mut index = SceneIndex::default();
            index.finish_refresh(discovered);
            let inputs = ViewInputs {
                root: &root,
                spec: &SpecTracker::default().load(&root),
                index: &index,
                catalog: catalog.as_ref(),
                store: &store,
            };
            let views = workspace::sections(
                &inputs,
                &[Section::Scenes, Section::Latest, Section::Findings],
            )?;
            let recent = store.jobs(None, RECENT_JOBS)?.items;
            anyhow::Ok((views, recent))
        })
        .await
        .map_err(EngineError::internal)?
        .map_err(EngineError::internal)?
    };
    let mut latest = views.latest.unwrap_or_default();
    let scenes = views.scenes.unwrap_or_default();
    Ok(Inspect {
        name: spec.project.name.clone(),
        root,
        main_scene: spec.engine.main_scene.clone(),
        default_profile: spec.render.profile.clone(),
        theme: project_theme(&spec, catalog.as_ref()),
        profiles: spec
            .profiles()
            .iter()
            .map(|profile| profile.profile.clone())
            .collect(),
        latest: scenes
            .iter()
            .map(|scene| {
                let found = latest.remove(&scene.id).unwrap_or_default();
                InspectLatest {
                    scene_id: scene.id.clone(),
                    video_job_id: found.video.as_ref().map(|video| video.base.job_id),
                    video: found.video.map(|video| video.base.artifact.path),
                    still: found.still.map(|still| still.base.artifact.path),
                    contact_sheet: found.contact_sheet.map(|sheet| sheet.base.artifact.path),
                }
            })
            .collect(),
        scenes: scenes
            .into_iter()
            .map(|scene| InspectScene {
                scene_id: scene.id,
                name: scene.class_name,
                file: scene.file,
                line: scene.span.start,
                sections: scene.sections.len(),
                beats: scene.beats.len(),
            })
            .collect(),
        findings: views
            .findings
            .unwrap_or_default()
            .into_iter()
            .take(MAX_FINDINGS)
            .map(|view| view.finding)
            .collect(),
        recent_jobs: recent_jobs
            .into_iter()
            .map(|job| RecentJob {
                id: job.id,
                operation: job.operation,
                status: job.status,
                created_at: job.created_at,
            })
            .collect(),
    })
}
