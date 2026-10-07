//! Workspace findings (HTTP §6.1.8): spec, index, render, qa and doctor
//! findings with stable ids, sorted errors first and capped.

use super::{
    artifacts::{file_url, file_version},
    latest::{Revisions, SceneLatest},
    project::SpecSnapshot,
    scenes::{Scene, SceneIndex},
};
use crate::{confine, JobFilter, Store};
use anyhow::Result;
use manim_director_core::{
    named_enum, Catalog, ErrorBody, Finding, JobRecord, JobStatus, Operation, OperationResult,
    Severity, SourceLocation, ThemeSetting, SPEC_FILE,
};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};
use uuid::Uuid;

const MAX_FINDINGS: usize = 200;

named_enum! {
    pub enum FindingSource {
        Spec = "spec",
        Index = "index",
        Render = "render",
        Qa = "qa",
        Doctor = "doctor",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindingView {
    #[serde(flatten)]
    pub finding: Finding,
    /// `<source>:<job id | spec | index>:<n>`, stable within a snapshot.
    pub id: String,
    pub source: FindingSource,
    pub scene_id: Option<String>,
    pub job_id: Option<Uuid>,
    pub outdated: bool,
    pub frame_url: Option<String>,
}

pub struct FindingInputs<'a> {
    pub root: &'a Path,
    pub spec: &'a SpecSnapshot,
    pub catalog: Option<&'a Catalog>,
    pub index: &'a SceneIndex,
    pub scenes: &'a [Scene],
    pub latest: &'a BTreeMap<String, SceneLatest>,
    pub store: &'a Store,
}

/// Blocking (database and file stats).
pub fn findings(
    inputs: &FindingInputs<'_>,
    revisions: &mut Revisions<'_>,
) -> Result<Vec<FindingView>> {
    let mut all = Collector {
        root: inputs.root,
        views: Vec::new(),
    };
    all.add(
        FindingSource::Spec,
        "spec",
        None,
        None,
        false,
        spec_findings(inputs),
    );
    let mut index = inputs.index.findings().to_vec();
    if let Some(error) = inputs.index.failure() {
        index.push(from_error_code("index_failed", error));
    }
    all.add(FindingSource::Index, "index", None, None, false, index);
    for scene in inputs.scenes {
        let filter = |operations| JobFilter {
            operations,
            scene_class: Some(&scene.class_name),
            scene_file: Some(&scene.file),
            profile: None,
        };
        let rendered = inputs.store.newest_job(
            filter(&[Operation::Render, Operation::Still]),
            &[
                JobStatus::Queued,
                JobStatus::Running,
                JobStatus::Succeeded,
                JobStatus::Failed,
            ],
        )?;
        if let Some(job) = rendered.filter(|job| job.status == JobStatus::Failed) {
            let outdated = revisions.outdated(&job);
            all.add_job(
                FindingSource::Render,
                &job,
                outdated,
                failure_findings(&job),
            );
        }
        let checked = inputs.store.newest_job(
            filter(&[Operation::Qa]),
            &[JobStatus::Succeeded, JobStatus::Failed],
        )?;
        if let Some(job) = checked {
            let latest = inputs.latest.get(&scene.id);
            let shown = [
                latest.and_then(|latest| latest.video.as_ref().map(|video| video.base.job_id)),
                latest.and_then(|latest| latest.still.as_ref().map(|still| still.base.job_id)),
            ];
            let stale_source = job
                .source_job_id
                .is_none_or(|source| !shown.contains(&Some(source)));
            let outdated = stale_source || revisions.outdated(&job);
            let found = match &job.result {
                Some(OperationResult::Qa(result)) => result
                    .findings
                    .iter()
                    .cloned()
                    .map(|mut finding| {
                        finding.location.get_or_insert_with(|| SourceLocation {
                            file: scene.file.clone(),
                            line: scene.span.start,
                            column: None,
                        });
                        finding
                    })
                    .collect(),
                _ => failure_findings(&job),
            };
            all.add_job(FindingSource::Qa, &job, outdated, found);
        }
    }
    let doctor = inputs.store.newest_job(
        JobFilter {
            operations: &[Operation::Doctor],
            ..JobFilter::default()
        },
        &[JobStatus::Succeeded, JobStatus::Failed],
    )?;
    if let Some(job) = doctor {
        let found = match &job.result {
            Some(OperationResult::Doctor(result)) => result.findings.clone(),
            _ => failure_findings(&job),
        };
        all.add_job(FindingSource::Doctor, &job, false, found);
    }
    let mut views = all.views;
    views.sort_by(|a, b| order_key(&a.finding).cmp(&order_key(&b.finding)));
    views.truncate(MAX_FINDINGS);
    Ok(views)
}

fn spec_findings(inputs: &FindingInputs<'_>) -> Vec<Finding> {
    let snapshot = inputs.spec;
    let spec = &snapshot.spec;
    let mut found = Vec::new();
    if let Some(problem) = &snapshot.status.error {
        let mut finding = Finding::warning("invalid_spec", problem.message.clone());
        finding.severity = Severity::Error;
        finding.location = problem.line.map(|line| SourceLocation {
            file: SPEC_FILE.into(),
            line,
            column: problem.column,
        });
        found.push(finding);
    }
    let default = &spec.render.profile;
    if spec.profile(default).is_none() {
        let mut finding = Finding::warning(
            "unknown_default_profile",
            format!("render.profile {default:?} names no profile."),
        );
        let names: Vec<_> = spec.profiles().iter().map(|p| p.profile.as_str()).collect();
        finding.hint = Some(format!("Use one of {}.", names.join(", ")));
        found.push(finding);
    }
    if matches!(spec.theme, Some(ThemeSetting::Legacy { .. })) {
        let mut finding = Finding::warning(
            "legacy_theme_mapping",
            "The theme mapping form is deprecated; only its preset is read.",
        );
        finding.severity = Severity::Info;
        finding.hint = Some("Write `theme: <name>`.".into());
        found.push(finding);
    }
    if let (Some(theme), Some(catalog)) = (spec.theme_name(), inputs.catalog) {
        if !catalog.themes.iter().any(|known| known.name == theme) {
            let mut finding = Finding::warning(
                "unknown_theme",
                format!("Theme {theme:?} is not installed."),
            );
            let names: Vec<_> = catalog.themes.iter().map(|t| t.name.as_str()).collect();
            finding.hint = Some(format!("Use one of {}.", names.join(", ")));
            found.push(finding);
        }
    }
    found
}

/// What a failed job contributes: a render's own findings, else its error.
fn failure_findings(job: &JobRecord) -> Vec<Finding> {
    let Some(error) = &job.error else {
        return Vec::new();
    };
    if error.code == "render_failed" {
        let reported = error
            .data
            .as_ref()
            .and_then(|data| data.get("findings"))
            .and_then(|findings| serde_json::from_value::<Vec<Finding>>(findings.clone()).ok());
        if let Some(findings) = reported.filter(|findings| !findings.is_empty()) {
            return findings;
        }
    }
    vec![from_error_code(&error.code, error)]
}

fn from_error_code(code: &str, error: &ErrorBody) -> Finding {
    let mut finding = Finding::warning(code, error.message.clone());
    finding.severity = Severity::Error;
    finding
}

/// Errors, warnings, info; then by file and line, location-less last.
fn order_key(finding: &Finding) -> (Severity, bool, &str, u32) {
    let severity = finding.severity;
    match &finding.location {
        Some(location) => (severity, false, location.file.as_str(), location.line),
        None => (severity, true, "", 0),
    }
}

struct Collector<'a> {
    root: &'a Path,
    views: Vec<FindingView>,
}

impl Collector<'_> {
    fn add_job(
        &mut self,
        source: FindingSource,
        job: &JobRecord,
        outdated: bool,
        found: Vec<Finding>,
    ) {
        let key = job.id.to_string();
        self.add(
            source,
            &key,
            job.scene_id.clone(),
            Some(job.id),
            outdated,
            found,
        );
    }

    fn add(
        &mut self,
        source: FindingSource,
        key: &str,
        scene_id: Option<String>,
        job_id: Option<Uuid>,
        outdated: bool,
        found: Vec<Finding>,
    ) {
        for (n, finding) in found.into_iter().enumerate() {
            let frame_url = finding.frame.as_deref().and_then(|frame| {
                let metadata = confine(self.root, frame).ok()?.metadata().ok()?;
                Some(file_url(frame, &file_version(&metadata)))
            });
            self.views.push(FindingView {
                finding,
                id: format!("{source}:{key}:{n}"),
                source,
                scene_id: scene_id.clone(),
                job_id,
                outdated,
                frame_url,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{sections, Section, ViewInputs};
    use super::*;
    use crate::{db::testing, workspace::SpecTracker, NewJob};
    use manim_director_core::{
        DiscoverResult, DiscoveredScene, DoctorParams, DoctorTask, JobOrigin, Limits, MediaFormat,
        OperationRequest, QaParams, QaResult, QaStatus, QaTask, RenderParams, RenderSettings,
        RenderTask, Renderer, SafeArea, SourceKind, Task,
    };
    use serde_json::{json, Value};
    use std::fs;

    fn insert(store: &Store, request: OperationRequest, task: Task, source: Option<Uuid>) -> Uuid {
        let id = Uuid::new_v4();
        store
            .insert_job(&NewJob {
                id,
                origin: JobOrigin::Http,
                owner: Uuid::new_v4(),
                request: &request,
                task: &task,
                limits: Limits {
                    timeout_seconds: 60,
                    memory_mb: None,
                },
                fingerprint: None,
                source_job_id: source,
                scene_class: Some("Intro"),
                scene_file: Some("scenes/main.py"),
                scene_revision: None,
                profile: None,
            })
            .unwrap();
        store.set_running(id).unwrap();
        id
    }

    fn located(file: &str, line: u32) -> Option<SourceLocation> {
        Some(SourceLocation {
            file: file.into(),
            line,
            column: None,
        })
    }

    #[test]
    fn job_findings_carry_locations_ids_and_staleness() {
        let (_db, store) = testing::store();
        let project = tempfile::tempdir().unwrap();
        let root = project.path().canonicalize().unwrap();
        fs::write(
            root.join("director.yaml"),
            "version: 1\nproject:\n  name: D\n",
        )
        .unwrap();
        let mut index = SceneIndex::default();
        index.finish_refresh(Ok(DiscoverResult {
            files: 1,
            truncated: false,
            scenes: vec![DiscoveredScene {
                name: "Intro".into(),
                file: "scenes/main.py".into(),
                line: 4,
                end_line: 20,
                construct_line: Some(5),
                bases: vec!["Scene".into()],
                doc: None,
                theme: None,
                sections: vec![],
                beats: vec![],
            }],
            findings: vec![],
            artifacts: vec![],
        }));

        let render = Task::Render(RenderTask {
            scene: Some("Intro".into()),
            files: vec![root.join("scenes/main.py")],
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
            out_dir: root.join("out"),
            sections: false,
            fresh: false,
        });
        let request = OperationRequest::Render(RenderParams::default());
        let failed = insert(&store, request, render, None);
        let mut traced = Finding::warning("python_name", "name 'rowz' is not defined");
        traced.severity = Severity::Error;
        traced.location = located("scenes/main.py", 9);
        let error = ErrorBody::new(
            "render_failed",
            "Intro.construct raised NameError.",
            Some(json!({"stage": "construct", "exception": "NameError",
                        "findings": [traced], "traceback": "…"})),
        );
        store
            .finish_error(failed, JobStatus::Failed, &error, None)
            .unwrap();

        let qa_task = Task::Qa(QaTask {
            source: root.join("x.mp4"),
            source_kind: SourceKind::Video,
            frames: 1,
            safe_area: SafeArea::default(),
            timeline: None,
            out_dir: root.join("qa"),
        });
        let checked = insert(
            &store,
            OperationRequest::Qa(QaParams::default()),
            qa_task,
            Some(Uuid::new_v4()),
        );
        let qa = OperationResult::Qa(QaResult {
            status: QaStatus::Warn,
            frames: vec![],
            findings: vec![Finding::warning("low_contrast", "Text is hard to read.")],
            source: None,
            artifacts: vec![],
        });
        store.finish_success(checked, &qa, None).unwrap();

        let doctor = insert(
            &store,
            OperationRequest::Doctor(DoctorParams {}),
            Task::Doctor(DoctorTask {}),
            None,
        );
        let unavailable = ErrorBody::new("runtime_unavailable", "Python was not found.", None);
        store
            .finish_error(doctor, JobStatus::Failed, &unavailable, None)
            .unwrap();

        let spec = SpecTracker::default().load(&root);
        let derived = sections(
            &ViewInputs {
                root: &root,
                spec: &spec,
                index: &index,
                catalog: None,
                store: &store,
            },
            &[Section::Findings],
        )
        .unwrap();
        let found = serde_json::to_value(derived.findings.unwrap()).unwrap();
        let summary: Vec<_> = found
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| {
                (
                    finding["id"].as_str().unwrap().to_owned(),
                    finding["location"]["line"].clone(),
                    finding["outdated"].clone(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (format!("render:{failed}:0"), json!(9), json!(false)),
                (format!("doctor:{doctor}:0"), Value::Null, json!(false)),
                (format!("qa:{checked}:0"), json!(4), json!(true)),
            ]
        );
        assert_eq!(found[0]["scene_id"], "scenes/main.py#Intro");
        assert_eq!(found[1]["code"], "runtime_unavailable");
        assert_eq!(found[2]["source"], "qa");
        assert_eq!(found[2]["location"]["file"], "scenes/main.py");
    }
}
