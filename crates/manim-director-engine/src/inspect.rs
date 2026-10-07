//! The compact project summary shown by CLI and MCP `inspect`.

use crate::{latest, Scheduler};
use manim_director_core::{
    ArtifactKind, DirectorSpec, EngineError, Finding, JobRecord, JobStatus, Operation, Severity,
    Timestamp,
};
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

pub async fn inspect(scheduler: &Scheduler) -> Result<Inspect, EngineError> {
    let root = scheduler.root().to_path_buf();
    let spec = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || DirectorSpec::load(&root))
            .await
            .map_err(EngineError::internal)?
            .map_err(EngineError::from)?
    };
    let (scenes, findings) = match scheduler.discover().await {
        Ok(index) => {
            let scenes: Vec<_> = index
                .scenes
                .iter()
                .map(|scene| InspectScene {
                    scene_id: format!("{}#{}", scene.file, scene.name),
                    name: scene.name.clone(),
                    file: scene.file.clone(),
                    line: scene.line,
                    sections: scene.sections.len(),
                    beats: scene.beats.len(),
                })
                .collect();
            (scenes, index.findings)
        }
        Err(error) => {
            let mut finding = Finding::warning("index_failed", error.to_string());
            finding.severity = Severity::Error;
            (Vec::new(), vec![finding])
        }
    };
    let store = scheduler.store().clone();
    let latest_root = root.clone();
    let scene_keys: Vec<_> = scenes
        .iter()
        .map(|scene| {
            (
                scene.scene_id.clone(),
                scene.name.clone(),
                scene.file.clone(),
            )
        })
        .collect();
    let (latest, recent_jobs) = tokio::task::spawn_blocking(move || {
        let latest = scene_keys
            .into_iter()
            .map(|(scene_id, class, file)| {
                let found = latest(&store, &latest_root, &class, &file)?;
                let path = |job: &Option<JobRecord>, kind| {
                    job.as_ref()
                        .and_then(|job| job.result.as_ref())
                        .and_then(|result| result.artifact(kind))
                        .map(|artifact| artifact.path.clone())
                };
                Ok(InspectLatest {
                    scene_id,
                    video: path(&found.video, ArtifactKind::Video),
                    still: path(&found.still, ArtifactKind::Image),
                    contact_sheet: path(&found.contact_sheet, ArtifactKind::ContactSheet),
                    video_job_id: found.video.as_ref().map(|job| job.id),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let recent = store
            .jobs(None, RECENT_JOBS)?
            .items
            .into_iter()
            .map(|job| RecentJob {
                id: job.id,
                operation: job.operation,
                status: job.status,
                created_at: job.created_at,
            })
            .collect();
        anyhow::Ok((latest, recent))
    })
    .await
    .map_err(EngineError::internal)?
    .map_err(EngineError::internal)?;
    Ok(Inspect {
        name: spec.project.name.clone(),
        root,
        main_scene: spec.engine.main_scene.clone(),
        default_profile: spec.render.profile.clone(),
        theme: spec.theme_name().map(str::to_owned),
        profiles: spec
            .profiles()
            .iter()
            .map(|profile| profile.profile.clone())
            .collect(),
        scenes,
        findings: findings.into_iter().take(MAX_FINDINGS).collect(),
        latest,
        recent_jobs,
    })
}
