use super::*;
use crate::NewJob;
use manim_director_core::{
    Artifact, ArtifactKind, CaptionsParams, ContactSheetParams, DiagnoseParams, DiscoverResult,
    DiscoveredScene, DoctorParams, ErrorBody, ExportFormat, ExportParams, ExportTask, FrameParams,
    JobOrigin, JobStatus, LogLevel, MediaInfo, OperationResult, QaParams, RenderParams,
    RenderResult, Resource, SceneRef, SourceKind, SourceRef, StillParams, StillResult,
    ValidateMathParams, ARTIFACTS_DIR,
};
use std::fs;

struct Project {
    _directory: tempfile::TempDir,
    root: PathBuf,
    store: Store,
}

impl Project {
    fn new(spec: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        fs::write(
            root.join("director.yaml"),
            format!("version: 1\nproject:\n  name: Demo\n{spec}"),
        )
        .unwrap();
        let store = Store::open(root.join(".manim-director/state.db")).unwrap();
        Self {
            _directory: directory,
            root,
            store,
        }
    }

    fn write(&self, path: &str, content: &str) -> PathBuf {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    fn resolve(&self, request: OperationRequest) -> Result<ResolvedJob, EngineError> {
        request.validate()?;
        let spec = DirectorSpec::load(&self.root).map_err(EngineError::from);
        let context = ProjectContext {
            root: &self.root,
            spec: &spec,
            store: &self.store,
        };
        resolve(&context, Uuid::new_v4(), &request)
    }

    /// Records a succeeded job with `result`, as a render or still would.
    fn finished_job(
        &self,
        result: OperationResult,
        class: Option<&str>,
        file: Option<&str>,
        profile: Option<&str>,
    ) -> Uuid {
        let settings = DirectorSpec::defaults().profiles()[0].clone();
        let (request, task) = match &result {
            OperationResult::Render(_) => (
                OperationRequest::Render(RenderParams::default()),
                Task::Render(RenderTask {
                    scene: None,
                    files: vec![],
                    settings,
                    media_dir: self.root.clone(),
                    out_dir: self.root.clone(),
                    sections: false,
                    fresh: false,
                }),
            ),
            OperationResult::Still(_) => (
                OperationRequest::Still(StillParams::default()),
                Task::Still(StillTask {
                    scene: None,
                    files: vec![],
                    settings,
                    media_dir: self.root.clone(),
                    out_dir: self.root.clone(),
                    fresh: false,
                }),
            ),
            _ => unreachable!("only media producers are recorded here"),
        };
        let id = Uuid::new_v4();
        self.store
            .insert_job(&NewJob {
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
                scene_class: class,
                scene_file: file,
                scene_revision: Some("rev"),
                profile,
            })
            .unwrap();
        self.store.set_running(id).unwrap();
        self.store.finish_success(id, &result, None).unwrap();
        id
    }

    fn render_job(&self, video: &str, class: &str, profile: &str) -> Uuid {
        self.write(video, "video");
        let result = OperationResult::Render(RenderResult {
            scene: SceneRef {
                name: class.into(),
                file: "scenes/main.py".into(),
            },
            duration_seconds: 2.0,
            animations: 1,
            artifacts: vec![media_artifact(ArtifactKind::Video, video)],
        });
        self.finished_job(result, Some(class), Some("scenes/main.py"), Some(profile))
    }
}

fn media_artifact(kind: ArtifactKind, path: &str) -> Artifact {
    Artifact {
        kind,
        path: path.into(),
        label: None,
        bytes: 5,
        media: Some(MediaInfo {
            container: "mp4".into(),
            codec: None,
            width: 640,
            height: 360,
            fps: Some(30.0),
            duration_seconds: Some(2.0),
            has_alpha: false,
        }),
    }
}

fn field_and_reason(error: EngineError) -> (Option<String>, String) {
    match error {
        EngineError::InvalidParams { field, reason, .. } => (field, reason),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn render_resolves_profile_files_and_engine_dirs() {
    let project = Project::new("engine:\n  main_scene: Intro\n  source: scenes/main.py\n");
    project.write("scenes/main.py", "class Intro(Scene): pass\n");
    let resolved = project
        .resolve(OperationRequest::Render(RenderParams {
            profile: Some("draft".into()),
            ..Default::default()
        }))
        .unwrap();
    let Task::Render(task) = &resolved.task else {
        panic!("expected a render task")
    };
    assert_eq!(task.scene.as_deref(), Some("Intro"));
    assert_eq!(task.files, [project.root.join("scenes/main.py")]);
    assert_eq!(
        (task.settings.width, task.settings.height, task.settings.fps),
        (854, 480, 15)
    );
    assert_eq!(task.media_dir, project.root.join(".manim-director/media"));
    assert!(task.out_dir.starts_with(project.root.join(ARTIFACTS_DIR)));
    assert_eq!(resolved.scene_file.as_deref(), Some("scenes/main.py"));
    assert_eq!(resolved.profile.as_deref(), Some("draft"));
    assert_eq!(resolved.limits.timeout_seconds, 1800);
    assert_eq!(resolved.limits.memory_mb, None);
}

#[test]
fn stills_render_png_with_the_profile_size() {
    let project = Project::new("");
    project.write("scenes/a.py", "class A(Scene): pass\n");
    let resolved = project
        .resolve(OperationRequest::Still(StillParams {
            scene: Some("A".into()),
            ..Default::default()
        }))
        .unwrap();
    let Task::Still(task) = resolved.task else {
        panic!("expected a still task")
    };
    assert_eq!(task.settings.format, MediaFormat::Png);
    assert_eq!(task.settings.profile, "preview");
    assert_eq!(task.files, [project.root.join("scenes/a.py")]);
}

#[test]
fn profile_errors_distinguish_params_from_spec() {
    let project = Project::new("render:\n  profile: cinema\n");
    project.write("scenes/a.py", "");
    let request = |profile: Option<&str>| {
        OperationRequest::Render(RenderParams {
            profile: profile.map(str::to_owned),
            ..Default::default()
        })
    };
    let (field, _) = field_and_reason(project.resolve(request(Some("cinema"))).err().unwrap());
    assert_eq!(field.as_deref(), Some("profile"));
    assert_eq!(
        project.resolve(request(None)).err().unwrap().code(),
        "invalid_spec"
    );
    assert!(project.resolve(request(Some("ultra"))).is_ok());
}

#[test]
fn scene_ids_and_classes_map_to_declared_files() {
    let project = Project::new(
        "scenes:\n  - {id: intro, class: IntroScene, file: scenes/intro.py}\n  - {id: outro, file: scenes/missing.py}\n",
    );
    project.write("scenes/intro.py", "");
    for name in ["intro", "IntroScene"] {
        let resolved = project
            .resolve(OperationRequest::Render(RenderParams {
                scene: Some(name.into()),
                ..Default::default()
            }))
            .unwrap();
        let Task::Render(task) = resolved.task else {
            panic!("expected a render task")
        };
        assert_eq!(task.scene.as_deref(), Some("IntroScene"));
        assert_eq!(task.files, [project.root.join("scenes/intro.py")]);
    }
    let error = project
        .resolve(OperationRequest::Render(RenderParams {
            scene: Some("outro".into()),
            ..Default::default()
        }))
        .err()
        .unwrap();
    assert_eq!(error.code(), "invalid_spec");
    assert!(error.to_string().contains("scenes[1].file"), "{error}");
}

#[test]
fn explicit_files_follow_the_public_path_rule() {
    let project = Project::new("");
    project.write("scenes/a.py", "");
    let request = |file: &str| {
        OperationRequest::Render(RenderParams {
            file: Some(file.into()),
            ..Default::default()
        })
    };
    assert!(project.resolve(request("scenes/a.py")).is_ok());
    let (field, reason) = field_and_reason(project.resolve(request("scenes/b.py")).err().unwrap());
    assert_eq!(
        (field.as_deref(), reason.as_str()),
        (Some("file"), "missing")
    );
    let (_, reason) = field_and_reason(project.resolve(request("../a.py")).err().unwrap());
    assert_eq!(reason, "traversal");
}

#[test]
fn the_discover_index_only_associates_a_job_with_a_file() {
    let project = Project::new("");
    project.write("scenes/a.py", "");
    project.write("scenes/b.py", "");
    let index = DiscoverResult {
        files: 2,
        truncated: false,
        scenes: vec![DiscoveredScene {
            name: "Wave".into(),
            file: "scenes/b.py".into(),
            line: 1,
            end_line: 2,
            construct_line: None,
            bases: vec![],
            doc: None,
            theme: None,
            sections: vec![],
            beats: vec![],
        }],
        findings: vec![],
        artifacts: vec![],
    };
    project
        .store
        .cache_put(
            "index",
            None,
            &OperationResult::Discover(index),
            Operation::Discover,
        )
        .unwrap();
    let resolved = project
        .resolve(OperationRequest::Render(RenderParams {
            scene: Some("Wave".into()),
            ..Default::default()
        }))
        .unwrap();
    assert_eq!(resolved.scene_file.as_deref(), Some("scenes/b.py"));
    let Task::Render(task) = resolved.task else {
        panic!("expected a render task")
    };
    assert_eq!(task.files.len(), 2);
}

#[test]
fn too_many_python_sources_fail_instead_of_rendering_from_a_partial_set() {
    let project = Project::new("");
    for index in 0..=MAX_PYTHON_SOURCES {
        project.write(&format!("scenes/s{index:03}.py"), "");
    }
    let error = project
        .resolve(OperationRequest::Render(RenderParams::default()))
        .err()
        .unwrap();
    assert_eq!(
        error,
        EngineError::BudgetExceeded {
            budget: Budget::PythonSources,
            limit: 500,
            actual: 501
        }
    );
}

#[test]
fn spec_budgets_set_limits_and_the_artifact_budget() {
    let project =
        Project::new("budgets:\n  render_seconds: 5\n  output_mb: 3\n  memory_mb: 4096\n");
    let resolved = project
        .resolve(OperationRequest::Doctor(DoctorParams {}))
        .unwrap();
    assert_eq!(resolved.limits.timeout_seconds, 10);
    assert_eq!(resolved.limits.memory_mb, Some(4096));
    assert_eq!(resolved.artifact_budget, 3 * 1024 * 1024);
}

#[test]
fn default_source_is_the_newest_render_whose_video_still_exists() {
    let project = Project::new("engine:\n  main_scene: Intro\n  source: scenes/main.py\n");
    project.write("scenes/main.py", "");
    let frame = |profile: Option<&str>| {
        OperationRequest::Frame(FrameParams {
            at_seconds: 1.0,
            source: None,
            scene: None,
            profile: profile.map(str::to_owned),
        })
    };
    match project.resolve(frame(Some("draft"))).err().unwrap() {
        EngineError::SourceNotFound { scene, profile } => {
            assert_eq!(scene.as_deref(), Some("Intro"));
            assert_eq!(profile.as_deref(), Some("draft"));
        }
        other => panic!("unexpected {other:?}"),
    }
    let older = project.render_job("out/old.mp4", "Intro", "draft");
    let newer = project.render_job("out/new.mp4", "Intro", "draft");
    let resolved = project.resolve(frame(Some("draft"))).unwrap();
    let source = resolved.source.as_ref().unwrap();
    assert_eq!(source.reference.job_id, Some(newer));
    assert_eq!(resolved.scene_class.as_deref(), Some("Intro"));
    assert_eq!(resolved.scene_file.as_deref(), Some("scenes/main.py"));
    assert_eq!(resolved.scene_revision.as_deref(), Some("rev"));

    fs::remove_file(project.root.join("out/new.mp4")).unwrap();
    let resolved = project.resolve(frame(None)).unwrap();
    assert_eq!(resolved.source.unwrap().reference.job_id, Some(older));
    assert!(project.resolve(frame(Some("production"))).is_err());

    let (field, _) = field_and_reason(
        project
            .resolve(OperationRequest::Frame(FrameParams {
                at_seconds: 9.0,
                source: Some(SourceRef::JobId(older)),
                scene: None,
                profile: None,
            }))
            .err()
            .unwrap(),
    );
    assert_eq!(field.as_deref(), Some("at_seconds"));
}

#[test]
fn job_sources_must_be_succeeded_jobs_with_usable_media() {
    let project = Project::new("");
    let missing = Uuid::new_v4();
    let error = project
        .resolve(OperationRequest::ContactSheet(ContactSheetParams {
            source: Some(SourceRef::JobId(missing)),
            ..Default::default()
        }))
        .err()
        .unwrap();
    assert_eq!(
        error,
        EngineError::NotFound {
            resource: Resource::Job,
            key: missing.to_string()
        }
    );
    project.write("out/a.png", "png");
    let still = project.finished_job(
        OperationResult::Still(StillResult {
            scene: SceneRef {
                name: "A".into(),
                file: "scenes/a.py".into(),
            },
            artifacts: vec![media_artifact(ArtifactKind::Image, "out/a.png")],
        }),
        Some("A"),
        None,
        None,
    );
    let (field, _) = field_and_reason(
        project
            .resolve(OperationRequest::ContactSheet(ContactSheetParams {
                source: Some(SourceRef::JobId(still)),
                ..Default::default()
            }))
            .err()
            .unwrap(),
    );
    assert_eq!(field.as_deref(), Some("source"));
    let qa = project
        .resolve(OperationRequest::Qa(QaParams {
            source: Some(SourceRef::JobId(still)),
            ..Default::default()
        }))
        .unwrap();
    let Task::Qa(task) = qa.task else {
        panic!("expected a qa task")
    };
    assert_eq!(task.source_kind, SourceKind::Image);
    assert_eq!(task.safe_area.bottom, 0.08);
}

#[test]
fn diagnosing_a_job_reads_its_error_and_logs() {
    let project = Project::new("");
    let succeeded = project.render_job("out/a.mp4", "A", "draft");
    let (field, _) = field_and_reason(
        project
            .resolve(OperationRequest::Diagnose(DiagnoseParams {
                job_id: Some(succeeded),
                text: None,
            }))
            .err()
            .unwrap(),
    );
    assert_eq!(field.as_deref(), Some("job_id"));

    let failed = Uuid::new_v4();
    let request = OperationRequest::Diagnose(DiagnoseParams {
        job_id: None,
        text: Some("x".into()),
    });
    let task = Task::Diagnose(DiagnoseTask { text: "x".into() });
    project
        .store
        .insert_job(&NewJob {
            id: failed,
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
            scene_class: None,
            scene_file: None,
            scene_revision: None,
            profile: None,
        })
        .unwrap();
    project
        .store
        .append_logs(
            failed,
            &[crate::NewLog::now(
                LogStream::Stderr,
                LogLevel::Info,
                "NameError: rowz",
            )],
        )
        .unwrap();
    project
        .store
        .finish_error(
            failed,
            JobStatus::Failed,
            &ErrorBody::new(
                "render_failed",
                "Scene raised.",
                Some(serde_json::json!({"traceback": "File \"scenes/main.py\", line 3"})),
            ),
            None,
        )
        .unwrap();
    let resolved = project
        .resolve(OperationRequest::Diagnose(DiagnoseParams {
            job_id: Some(failed),
            text: None,
        }))
        .unwrap();
    let Task::Diagnose(task) = resolved.task else {
        panic!("expected a diagnose task")
    };
    assert!(task.text.starts_with("render_failed: Scene raised.\n"));
    assert!(task.text.contains("line 3"));
    assert!(task.text.ends_with("NameError: rowz\n"));
}

#[test]
fn long_diagnose_text_keeps_its_tail() {
    let text = format!("{}é{}", "a".repeat(70_000), "tail");
    let kept = keep_tail(&text, MAX_DIAGNOSE_TEXT_BYTES);
    assert!(kept.len() <= MAX_DIAGNOSE_TEXT_BYTES);
    assert!(kept.ends_with("tail"));
}

#[test]
fn validate_math_seeds_from_params_then_spec_then_default() {
    let seeded = Project::new("  seed: 73\n");
    let unseeded = Project::new("");
    let request = |seed| {
        OperationRequest::ValidateMath(ValidateMathParams {
            steps: vec!["a".into(), "a".into()],
            ranges: Default::default(),
            samples: 10,
            tolerance: 1e-9,
            seed,
        })
    };
    let seed_of = |project: &Project, seed| match project.resolve(request(seed)).unwrap().task {
        Task::ValidateMath(task) => task.seed,
        _ => unreachable!(),
    };
    assert_eq!(seed_of(&seeded, Some(5)), 5);
    assert_eq!(seed_of(&seeded, None), 73);
    assert_eq!(seed_of(&unseeded, None), 1729);
}

#[test]
fn captions_outputs_are_denied_inside_the_media_cache() {
    let project = Project::new("  media_dir: cache\n");
    project.write("captions/en.vtt", "WEBVTT\n");
    let request = |output: &str| {
        OperationRequest::Captions(CaptionsParams {
            path: "captions/en.vtt".into(),
            shift_seconds: 0.0,
            scale: 1.0,
            output: Some(output.into()),
        })
    };
    assert!(project.resolve(request("captions/en.srt")).is_ok());
    let (_, reason) = field_and_reason(project.resolve(request("cache/en.srt")).err().unwrap());
    assert_eq!(reason, "denied");
}

#[test]
fn zip_exports_bundle_sources_and_the_source_jobs_artifacts() {
    let project = Project::new("");
    project.write("scenes/main.py", "");
    let job = project.render_job(".manim-director/artifacts/r1/Intro.mp4", "Intro", "draft");
    let resolved = project
        .resolve(OperationRequest::Export(ExportParams {
            source: Some(SourceRef::JobId(job)),
            ..Default::default()
        }))
        .unwrap();
    let Task::Export(ExportTask::Zip(task)) = resolved.task else {
        panic!("expected a zip export")
    };
    assert_eq!(task.output, project.root.join("output/demo.zip"));
    assert_eq!(task.source_job_id, Some(job));
    let archive: Vec<_> = task
        .entries
        .iter()
        .map(|entry| entry.archive_path.as_str())
        .collect();
    assert_eq!(
        archive,
        ["director.yaml", "scenes/main.py", "deliverables/Intro.mp4"]
    );

    let unsourced = project
        .resolve(OperationRequest::Export(ExportParams {
            scene: Some("Nothing".into()),
            ..Default::default()
        }))
        .unwrap();
    let Task::Export(ExportTask::Zip(task)) = unsourced.task else {
        panic!("expected a zip export")
    };
    assert_eq!(task.source_job_id, None);

    let tight = Project::new("budgets:\n  output_mb: 0\n");
    tight.write("scenes/main.py", "print()");
    let error = tight
        .resolve(OperationRequest::Export(ExportParams::default()))
        .err()
        .unwrap();
    assert!(matches!(
        error,
        EngineError::BudgetExceeded {
            budget: Budget::ExportBytes,
            ..
        }
    ));
}

#[test]
fn media_exports_default_to_the_scene_name_in_the_output_dir() {
    let project = Project::new("");
    let job = project.render_job("out/Intro.mp4", "Intro", "draft");
    let resolved = project
        .resolve(OperationRequest::Export(ExportParams {
            format: ExportFormat::Gif,
            source: Some(SourceRef::JobId(job)),
            gif_fps: Some(12),
            ..Default::default()
        }))
        .unwrap();
    let Task::Export(ExportTask::Media(task)) = resolved.task else {
        panic!("expected a media export")
    };
    assert_eq!(task.output, project.root.join("output/Intro.gif"));
    assert_eq!(task.gif.map(|gif| (gif.fps, gif.width)), Some((12, 960)));
    assert!(!task.alpha);
}

#[test]
fn frontends_reject_operations_they_do_not_serve() {
    let body = |operation: &str| serde_json::json!({ "operation": operation });
    for operation in ["init", "discover", "ingest"] {
        match parse_request(Frontend::Http, body(operation)).unwrap_err() {
            EngineError::OperationNotAllowed { allowed, .. } => {
                assert_eq!(allowed.len(), 10);
                assert!(!allowed.contains(&Operation::Ingest));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(parse_request(Frontend::Http, body("doctor")).is_ok());
    assert!(parse_request(
        Frontend::McpSubmit,
        serde_json::json!({"operation": "ingest", "sources": [{"path": "/n.md"}]})
    )
    .is_ok());
    assert!(parse_request(Frontend::McpSubmit, body("init")).is_err());
}
