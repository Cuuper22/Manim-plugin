//! Media source selection for frame, contact_sheet, qa and media export.

use super::{ProjectContext, SceneLookup};
use crate::scheduler::{artifacts, latest};
use manim_director_core::{
    files, ArtifactKind, EngineError, JobRecord, JobStatus, MediaInfo, MediaSource, Operation,
    SourceKind, SourceRef, ARTIFACTS_DIR,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Consumer {
    Video,
    /// `qa` also inspects stills and grabbed frames.
    VideoOrImage,
}

pub(crate) struct SelectedSource {
    pub absolute: PathBuf,
    pub kind: SourceKind,
    pub media: MediaInfo,
    /// The job that produced the file, when known; later jobs inherit its
    /// scene association.
    pub job: Option<JobRecord>,
    pub timeline: Option<PathBuf>,
    /// The (engine) `source` field of the result.
    pub reference: MediaSource,
}

pub(crate) fn select(
    ctx: &ProjectContext<'_>,
    source: Option<&SourceRef>,
    scene: Option<&str>,
    profile: Option<&str>,
    consumer: Consumer,
) -> Result<SelectedSource, EngineError> {
    match source {
        Some(SourceRef::JobId(id)) => from_job(ctx, *id, consumer),
        Some(SourceRef::Path(path)) => from_path(ctx, path, consumer),
        None => {
            let spec = ctx.spec()?;
            let lookup = SceneLookup::new(spec, scene);
            let file = lookup.spec_file().and_then(|(_, file)| {
                crate::confine(ctx.root, file)
                    .ok()
                    .map(|path| relative(ctx.root, &path))
            });
            let job = latest::latest_render(
                ctx.store,
                ctx.root,
                lookup.class.as_deref(),
                file.as_deref(),
                profile,
            )
            .map_err(EngineError::internal)?
            .ok_or_else(|| EngineError::SourceNotFound {
                scene: lookup.class.clone().or(scene.map(str::to_owned)),
                profile: profile.map(str::to_owned),
            })?;
            from_record(ctx, job, Consumer::Video)
        }
    }
}

fn from_job(
    ctx: &ProjectContext<'_>,
    id: Uuid,
    consumer: Consumer,
) -> Result<SelectedSource, EngineError> {
    let job = ctx
        .store
        .get_job(id)
        .map_err(EngineError::internal)?
        .ok_or_else(|| EngineError::job_not_found(id))?;
    if job.status != JobStatus::Succeeded {
        return Err(EngineError::invalid(
            "source",
            format!("job {id} is {}, not succeeded", job.status),
        ));
    }
    from_record(ctx, job, consumer)
}

fn from_record(
    ctx: &ProjectContext<'_>,
    job: JobRecord,
    consumer: Consumer,
) -> Result<SelectedSource, EngineError> {
    let mut kinds = Vec::new();
    if matches!(job.operation, Operation::Render | Operation::Export) {
        kinds.push((ArtifactKind::Video, SourceKind::Video));
    }
    if consumer == Consumer::VideoOrImage
        && matches!(job.operation, Operation::Still | Operation::Frame)
    {
        kinds.push((ArtifactKind::Image, SourceKind::Image));
    }
    let result = job.result.as_ref();
    let found = kinds.iter().find_map(|(artifact_kind, source_kind)| {
        let artifact = result?.artifact(*artifact_kind)?;
        let path = artifacts::existing(ctx.root, artifact)?;
        Some((artifact.clone(), path, *source_kind))
    });
    let Some((artifact, absolute, kind)) = found else {
        return Err(EngineError::invalid(
            "source",
            format!("job {} has no usable media artifact on disk", job.id),
        ));
    };
    let media = match artifact.media {
        Some(media) => media,
        None => probe(&absolute)?,
    };
    let timeline = result
        .and_then(|result| result.artifact(ArtifactKind::Timeline))
        .and_then(|timeline| artifacts::existing(ctx.root, timeline));
    Ok(SelectedSource {
        reference: MediaSource {
            path: artifact.path,
            job_id: Some(job.id),
            scene: job.scene_class.clone(),
        },
        absolute,
        kind,
        media,
        timeline,
        job: Some(job),
    })
}

fn from_path(
    ctx: &ProjectContext<'_>,
    value: &str,
    consumer: Consumer,
) -> Result<SelectedSource, EngineError> {
    let extensions: Vec<&str> = match consumer {
        Consumer::Video => files::VIDEO.to_vec(),
        Consumer::VideoOrImage => [files::VIDEO, files::IMAGE].concat(),
    };
    let absolute = super::paths::project_path(
        ctx.root,
        "source",
        value,
        super::paths::PathUse::Input {
            allow_artifacts: true,
            extensions: &extensions,
        },
    )?;
    let media = probe(&absolute)?;
    let kind = if files::has_extension(&absolute, files::IMAGE) {
        SourceKind::Image
    } else {
        SourceKind::Video
    };
    let job = producing_job(ctx, value)?;
    let timeline = job
        .as_ref()
        .and_then(|job| job.result.as_ref())
        .and_then(|result| result.artifact(ArtifactKind::Timeline))
        .and_then(|timeline| artifacts::existing(ctx.root, timeline));
    Ok(SelectedSource {
        reference: MediaSource {
            path: relative(ctx.root, &absolute),
            job_id: job.as_ref().map(|job| job.id),
            scene: job.as_ref().and_then(|job| job.scene_class.clone()),
        },
        absolute,
        kind,
        media,
        job,
        timeline,
    })
}

/// A path under `.manim-director/artifacts/<id>/` belongs to job `<id>`.
fn producing_job(ctx: &ProjectContext<'_>, value: &str) -> Result<Option<JobRecord>, EngineError> {
    let Some(id) = value
        .strip_prefix(ARTIFACTS_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.split('/').next())
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        return Ok(None);
    };
    ctx.store.get_job(id).map_err(EngineError::internal)
}

fn probe(path: &Path) -> Result<MediaInfo, EngineError> {
    artifacts::probe_media(path)
        .map_err(|_| EngineError::invalid("source", "not a readable media file"))
}

fn relative(root: &Path, path: &Path) -> String {
    manim_director_core::relative_posix(root, path)
}
