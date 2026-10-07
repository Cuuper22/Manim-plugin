//! HTTP job projections (HTTP §5 `JobSummary` / `Job`): the OPS record minus
//! engine internals, plus artifacts with URLs.

use super::artifacts::{artifact_view, ArtifactView};
use manim_director_core::{
    ArtifactKind, ErrorBody, JobOrigin, JobRecord, JobStatus, Operation, OperationRequest,
    OperationResult, Progress, Timestamp,
};
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

/// Artifacts a summary carries; `artifacts_total` counts them all.
const SUMMARY_ARTIFACTS: usize = 20;

/// The order a summary keeps artifacts in: what a viewer wants first.
const KIND_PRIORITY: [ArtifactKind; 8] = [
    ArtifactKind::Video,
    ArtifactKind::ContactSheet,
    ArtifactKind::Image,
    ArtifactKind::Section,
    ArtifactKind::Captions,
    ArtifactKind::Timeline,
    ArtifactKind::Archive,
    ArtifactKind::File,
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobView {
    pub id: Uuid,
    pub sequence: i64,
    pub operation: Operation,
    pub status: JobStatus,
    pub origin: JobOrigin,
    pub cached: bool,
    pub cached_from: Option<Uuid>,
    pub source_job_id: Option<Uuid>,
    pub cancel_requested: bool,
    pub request: OperationRequest,
    pub scene_id: Option<String>,
    pub profile: Option<String>,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    pub progress: Option<Progress>,
    pub error: Option<ErrorBody>,
    pub artifacts: Vec<ArtifactView>,
    pub artifacts_total: usize,
}

/// One job with its whole result and every artifact still on disk.
#[derive(Debug, Clone, Serialize)]
pub struct JobDetail {
    #[serde(flatten)]
    pub job: JobView,
    pub result: Option<OperationResult>,
}

/// A list entry: at most 20 artifacts, by kind priority. Blocking (stats files).
pub fn job_summary(root: &Path, job: &JobRecord) -> JobView {
    let mut artifacts = existing_artifacts(root, job);
    let total = artifacts.len();
    artifacts.sort_by_key(|artifact| KIND_PRIORITY.iter().position(|kind| *kind == artifact.kind));
    artifacts.truncate(SUMMARY_ARTIFACTS);
    view(job, artifacts, total)
}

/// Blocking (stats files).
pub fn job_detail(root: &Path, job: JobRecord) -> JobDetail {
    let artifacts = existing_artifacts(root, &job);
    let total = artifacts.len();
    JobDetail {
        job: view(&job, artifacts, total),
        result: job.result,
    }
}

fn existing_artifacts(root: &Path, job: &JobRecord) -> Vec<ArtifactView> {
    match (&job.result, job.status) {
        (Some(result), JobStatus::Succeeded) => result
            .artifacts()
            .iter()
            .filter_map(|artifact| artifact_view(root, artifact, job.scene_id.as_deref()))
            .collect(),
        _ => Vec::new(),
    }
}

fn view(job: &JobRecord, artifacts: Vec<ArtifactView>, artifacts_total: usize) -> JobView {
    JobView {
        id: job.id,
        sequence: job.sequence,
        operation: job.operation,
        status: job.status,
        origin: job.origin,
        cached: job.cached,
        cached_from: job.cached_from,
        source_job_id: job.source_job_id,
        cancel_requested: job.cancel_requested,
        request: job.request.clone(),
        scene_id: job.scene_id.clone(),
        profile: job.profile.clone(),
        created_at: job.created_at,
        started_at: job.started_at,
        finished_at: job.finished_at,
        progress: job
            .progress
            .clone()
            .filter(|_| job.status == JobStatus::Running),
        error: job.error.clone(),
        artifacts,
        artifacts_total,
    }
}
