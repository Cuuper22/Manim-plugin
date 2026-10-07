//! Per-scene latest artifacts (HTTP §6.1.7) from the one OPS `latest` rule,
//! with the beat and section marks and the caption files of the latest video.

use super::{
    artifacts::{artifact_view, ArtifactView},
    scenes::Scene,
};
use crate::{confine, file_revision, latest, Store};
use anyhow::Result;
use manim_director_core::{
    Artifact, ArtifactKind, JobRecord, Operation, OperationResult, SheetFrame, Timeline, Timestamp,
};
use serde::Serialize;
use std::{collections::HashMap, fs, path::Path};
use uuid::Uuid;

/// Timelines larger than this are not read for marks.
const MAX_TIMELINE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestBase {
    pub job_id: Uuid,
    pub finished_at: Option<Timestamp>,
    pub artifact: ArtifactView,
    /// The scene file changed since the job (OPS §1.8).
    pub outdated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestVideo {
    #[serde(flatten)]
    pub base: LatestBase,
    pub profile: Option<String>,
    pub timeline: Vec<TimelineMark>,
    /// The render's caption files (Manim subcaptions) still on disk.
    pub captions: Vec<ArtifactView>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestStill {
    #[serde(flatten)]
    pub base: LatestBase,
    pub operation: Operation,
    /// `None` for a `still` (the last frame).
    pub at_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestContactSheet {
    #[serde(flatten)]
    pub base: LatestBase,
    pub frames: Vec<SheetFrame>,
    pub source_job_id: Option<Uuid>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SceneLatest {
    pub video: Option<LatestVideo>,
    pub still: Option<LatestStill>,
    pub contact_sheet: Option<LatestContactSheet>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkKind {
    Beat,
    Section,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimelineMark {
    pub kind: MarkKind,
    pub name: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Current revisions of scene files, hashed once per snapshot.
pub struct Revisions<'a> {
    root: &'a Path,
    known: HashMap<String, Option<String>>,
}

impl<'a> Revisions<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self {
            root,
            known: HashMap::new(),
        }
    }

    /// OPS §1.8: the job recorded a revision and its scene file no longer has
    /// it (a deleted file counts as changed).
    pub fn outdated(&mut self, job: &JobRecord) -> bool {
        let (Some(recorded), Some(file)) = (&job.scene_revision, &job.scene_file) else {
            return false;
        };
        let root = self.root;
        let current = self.known.entry(file.clone()).or_insert_with(|| {
            confine(root, file)
                .ok()
                .and_then(|path| file_revision(&path).ok())
        });
        current.as_ref() != Some(recorded)
    }
}

/// Blocking (database and file stats).
pub fn scene_latest(
    store: &Store,
    root: &Path,
    scene: &Scene,
    revisions: &mut Revisions<'_>,
) -> Result<SceneLatest> {
    let found = latest(store, root, &scene.class_name, &scene.file)?;
    let mut base = |job: &JobRecord, kind| {
        let artifact = job.result.as_ref()?.artifact(kind)?;
        Some(LatestBase {
            job_id: job.id,
            finished_at: job.finished_at,
            artifact: artifact_view(root, artifact, job.scene_id.as_deref())?,
            outdated: revisions.outdated(job),
        })
    };
    let video = found.video.as_ref().and_then(|job| {
        Some(LatestVideo {
            base: base(job, ArtifactKind::Video)?,
            profile: job.profile.clone(),
            timeline: marks(root, job, scene),
            captions: job
                .result
                .iter()
                .flat_map(OperationResult::artifacts)
                .filter(|artifact| artifact.kind == ArtifactKind::Captions)
                .filter_map(|artifact| artifact_view(root, artifact, job.scene_id.as_deref()))
                .collect(),
        })
    });
    let still = found.still.as_ref().and_then(|job| {
        Some(LatestStill {
            base: base(job, ArtifactKind::Image)?,
            operation: job.operation,
            at_seconds: match &job.result {
                Some(OperationResult::Frame(result)) => Some(result.at_seconds),
                _ => None,
            },
        })
    });
    let contact_sheet = found.contact_sheet.as_ref().and_then(|job| {
        Some(LatestContactSheet {
            base: base(job, ArtifactKind::ContactSheet)?,
            frames: match &job.result {
                Some(OperationResult::ContactSheet(result)) => result.frames.clone(),
                _ => Vec::new(),
            },
            source_job_id: job.source_job_id,
        })
    });
    Ok(SceneLatest {
        video,
        still,
        contact_sheet,
    })
}

/// Beat marks from the render's own timeline; section marks laid end to end
/// from the section videos' durations.
fn marks(root: &Path, job: &JobRecord, scene: &Scene) -> Vec<TimelineMark> {
    let Some(result) = &job.result else {
        return Vec::new();
    };
    let artifacts = result.artifacts();
    let beats = artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::Timeline)
        .and_then(|artifact| read_timeline(root, artifact))
        .map(|timeline| timeline.beats)
        .unwrap_or_default();
    let mut marks: Vec<TimelineMark> = beats
        .iter()
        .map(|beat| TimelineMark {
            kind: MarkKind::Beat,
            name: beat.id.clone(),
            start_seconds: beat.start_seconds,
            end_seconds: beat.end_seconds,
            file: relative(&beat.file),
            line: Some(beat.line),
        })
        .collect();
    let mut start = 0.0;
    for section in artifacts
        .iter()
        .filter(|artifact| artifact.kind == ArtifactKind::Section)
    {
        let Some(duration) = section
            .media
            .as_ref()
            .and_then(|media| media.duration_seconds)
        else {
            break;
        };
        let name = section.label.clone().unwrap_or_default();
        let (file, line) = match beats.iter().find(|beat| beat.id == name) {
            Some(beat) => (relative(&beat.file), Some(beat.line)),
            None => match scene
                .sections
                .iter()
                .find(|mark| mark.name.as_deref() == Some(name.as_str()))
            {
                Some(mark) => (Some(scene.file.clone()), Some(mark.line)),
                None => (None, None),
            },
        };
        marks.push(TimelineMark {
            kind: MarkKind::Section,
            name,
            start_seconds: start,
            end_seconds: start + duration,
            file,
            line,
        });
        start += duration;
    }
    marks.sort_by(|a, b| {
        a.start_seconds
            .total_cmp(&b.start_seconds)
            .then((a.kind == MarkKind::Section).cmp(&(b.kind == MarkKind::Section)))
    });
    marks
}

fn read_timeline(root: &Path, artifact: &Artifact) -> Option<Timeline> {
    let path = confine(root, &artifact.path).ok()?;
    if path.metadata().ok()?.len() > MAX_TIMELINE_BYTES {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// A timeline `file` is a jump target only when it is project-relative.
fn relative(file: &str) -> Option<String> {
    (!Path::new(file).is_absolute()).then(|| file.to_owned())
}

#[cfg(test)]
mod tests {
    use super::super::{sections, SceneIndex, Section, SpecTracker, ViewInputs};
    use super::*;
    use crate::{db::testing, NewJob};
    use manim_director_core::{
        DiscoverResult, OperationRequest, RenderParams, RenderResult, RenderTask, SceneRef, Task,
    };

    #[test]
    fn the_latest_video_carries_its_renders_caption_files() {
        let (_db, store) = testing::store();
        let project = tempfile::tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        fs::write(
            root.join("director.yaml"),
            "version: 1\nproject:\n  name: D\n",
        )
        .unwrap();
        let out = ".manim-director/artifacts/r";
        fs::create_dir_all(root.join(out)).unwrap();
        for file in ["Intro.mp4", "Intro.srt"] {
            fs::write(root.join(out).join(file), "1\n").unwrap();
        }
        let mut index = SceneIndex::default();
        index.finish_refresh(Ok(DiscoverResult {
            files: 1,
            truncated: false,
            scenes: vec![testing::scene("Intro", "scenes/main.py", 1)],
            findings: vec![],
            artifacts: vec![],
        }));

        let request = OperationRequest::Render(RenderParams::default());
        let task = Task::Render(RenderTask {
            scene: Some("Intro".into()),
            files: vec![root.join("scenes/main.py")],
            settings: testing::draft(),
            media_dir: root.join("media"),
            out_dir: root.join(out),
            sections: false,
            fresh: false,
        });
        let id = Uuid::new_v4();
        store
            .insert_job(&NewJob {
                scene_class: Some("Intro"),
                scene_file: Some("scenes/main.py"),
                ..testing::new_job(id, &request, &task)
            })
            .unwrap();
        store.set_running(id).unwrap();
        let artifact = |kind, name: &str| Artifact {
            kind,
            path: format!("{out}/{name}"),
            label: None,
            bytes: 2,
            media: None,
        };
        let result = OperationResult::Render(RenderResult {
            scene: SceneRef {
                name: "Intro".into(),
                file: "scenes/main.py".into(),
            },
            duration_seconds: 1.0,
            animations: 1,
            artifacts: vec![
                artifact(ArtifactKind::Video, "Intro.mp4"),
                artifact(ArtifactKind::Captions, "Intro.srt"),
                artifact(ArtifactKind::Captions, "Removed.vtt"),
            ],
        });
        store.finish_success(id, &result, None).unwrap();

        let spec = SpecTracker::default().load(&root);
        let inputs = ViewInputs {
            root: &root,
            spec: &spec,
            index: &index,
            catalog: None,
            store: &store,
        };
        let latest = sections(&inputs, &[Section::Latest])
            .unwrap()
            .latest
            .unwrap();
        let video = serde_json::to_value(&latest["scenes/main.py#Intro"].video).unwrap();
        let captions = video["captions"].as_array().unwrap();
        assert_eq!(captions.len(), 1, "{captions:?}");
        assert_eq!(captions[0]["kind"], "captions");
        assert_eq!(captions[0]["path"], format!("{out}/Intro.srt"));
        assert_eq!(captions[0]["content_type"], "text/plain; charset=utf-8");
        let url = captions[0]["url"].as_str().unwrap();
        assert!(
            url.starts_with(&format!("/api/files/{out}/Intro.srt?v=")),
            "{url}"
        );
    }
}
