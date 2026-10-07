//! The single "latest artifact" rule (OPS §1.10).

use super::artifacts;
use crate::{JobFilter, Store};
use anyhow::Result;
use manim_director_core::{ArtifactKind, JobRecord, Operation};
use std::path::Path;

/// Newest succeeded render matching every given filter whose video is still
/// on disk. Cached jobs count: their artifacts live in the original job's dir.
pub fn latest_render(
    store: &Store,
    root: &Path,
    class: Option<&str>,
    file: Option<&str>,
    profile: Option<&str>,
) -> Result<Option<JobRecord>> {
    newest_with(
        store,
        root,
        JobFilter {
            operations: &[Operation::Render],
            scene_class: class,
            scene_file: file,
            profile,
        },
        ArtifactKind::Video,
    )
}

#[derive(Debug, Clone, Default)]
pub struct Latest {
    pub video: Option<JobRecord>,
    pub still: Option<JobRecord>,
    pub contact_sheet: Option<JobRecord>,
}

pub fn latest(store: &Store, root: &Path, class: &str, file: &str) -> Result<Latest> {
    let filter = |operations| JobFilter {
        operations,
        scene_class: Some(class),
        scene_file: Some(file),
        profile: None,
    };
    Ok(Latest {
        video: latest_render(store, root, Some(class), Some(file), None)?,
        still: newest_with(
            store,
            root,
            filter(&[Operation::Still, Operation::Frame]),
            ArtifactKind::Image,
        )?,
        contact_sheet: newest_with(
            store,
            root,
            filter(&[Operation::ContactSheet]),
            ArtifactKind::ContactSheet,
        )?,
    })
}

fn newest_with(
    store: &Store,
    root: &Path,
    filter: JobFilter<'_>,
    kind: ArtifactKind,
) -> Result<Option<JobRecord>> {
    store.find_succeeded(filter, |job| {
        job.result
            .as_ref()
            .and_then(|result| result.artifact(kind))
            .and_then(|artifact| artifacts::existing(root, artifact))
            .is_some()
    })
}
