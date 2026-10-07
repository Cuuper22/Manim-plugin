//! End-to-end scheduler tests against the bridge v2 stub runtime in
//! `tests/fixtures/stub_runtime.py`.

use super::*;
use manim_director_core::{
    ArtifactKind, CaptionsParams, DiagnoseParams, DoctorParams, FrameParams, InitParams, JobStatus,
    LogStream, ProgressPhase, RenderParams, ARTIFACTS_DIR,
};
use serde_json::Value;
use std::{fs, time::Instant};

const STUB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stub_runtime.py"
);

fn stub(module: &str) -> BridgeConfig {
    BridgeConfig {
        python: STUB.into(),
        module: module.into(),
    }
}

struct Project {
    _directory: tempfile::TempDir,
    root: PathBuf,
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
        fs::create_dir_all(root.join("scenes")).unwrap();
        fs::write(
            root.join("scenes/main.py"),
            "class MainScene(Scene):\n    pass\n",
        )
        .unwrap();
        Self {
            _directory: directory,
            root,
        }
    }

    async fn scheduler(&self, module: &str, workers: usize, queue_capacity: usize) -> Scheduler {
        self.engine(EngineMode::Cli, stub(module), workers, queue_capacity)
            .await
    }

    async fn engine(
        &self,
        mode: EngineMode,
        bridge: BridgeConfig,
        workers: usize,
        queue_capacity: usize,
    ) -> Scheduler {
        Scheduler::open(
            &self.root,
            SchedulerConfig {
                mode,
                workers,
                queue_capacity,
                bridge,
                prune: PrunePolicy {
                    keep_jobs: 500,
                    keep_days: 30,
                },
                prewarm: None,
            },
        )
        .await
        .unwrap()
    }
}

fn diagnose(text: &str) -> OperationRequest {
    OperationRequest::Diagnose(DiagnoseParams {
        job_id: None,
        text: Some(text.into()),
    })
}

async fn run(scheduler: &Scheduler, request: OperationRequest) -> JobRecord {
    let job = scheduler
        .submit(JobOrigin::Cli, request)
        .await
        .unwrap()
        .into_job();
    tokio::time::timeout(Duration::from_secs(30), scheduler.wait(job.id))
        .await
        .expect("the job finishes")
        .unwrap()
}

fn error_code(job: &JobRecord) -> &str {
    job.error.as_ref().map_or("", |error| error.code.as_str())
}

fn error_data(job: &JobRecord) -> &Value {
    job.error
        .as_ref()
        .and_then(|error| error.data.as_ref())
        .unwrap_or(&Value::Null)
}

async fn wait_until_running(scheduler: &Scheduler, id: Uuid) {
    for _ in 0..200 {
        let job = scheduler.store().get_job(id).unwrap().unwrap();
        if job.status == JobStatus::Running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {id} never started");
}

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[tokio::test]
async fn a_job_streams_progress_and_logs_into_a_typed_result() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 2, 8).await;
    let mut events = scheduler.subscribe();
    let job = run(&scheduler, diagnose("hello")).await;
    assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
    let Some(OperationResult::Diagnose(result)) = &job.result else {
        panic!("expected a diagnose result")
    };
    assert_eq!(result.findings[0].message, "hello");
    assert!(job.progress.is_none());

    let logs = scheduler.store().logs(job.id, None, 100).unwrap().items;
    let line = |stream: LogStream, text: &str| {
        logs.iter()
            .any(|record| record.stream == stream && record.message.contains(text))
    };
    assert!(line(LogStream::Engine, "Runtime 2.0.0-stub ready"));
    assert!(line(LogStream::Runtime, "looked at the text"));
    assert!(line(LogStream::Stderr, "a print from user code"));

    // The record turns terminal in the store just before the event goes out.
    let mut kinds = Vec::new();
    while kinds.last() != Some(&"finished") {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("the terminal event follows")
            .unwrap();
        kinds.push(match event {
            EngineEvent::Job(job) => match job.status {
                JobStatus::Queued => "queued",
                JobStatus::Running => "started",
                _ => "finished",
            },
            EngineEvent::Progress { progress, .. } => match progress.phase {
                ProgressPhase::Analyze => "analyze",
                _ => "progress",
            },
        });
    }
    kinds.dedup();
    assert_eq!(kinds.first(), Some(&"queued"));
    assert!(kinds.contains(&"analyze"));
    assert_eq!(kinds.last(), Some(&"finished"));
}

#[tokio::test]
async fn runtime_error_frames_fail_jobs_with_contract_codes() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 2, 8).await;
    let failed = run(&scheduler, diagnose("error")).await;
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(error_code(&failed), "render_failed");
    assert_eq!(error_data(&failed)["stage"], "construct");

    let unknown = run(&scheduler, diagnose("unknown-code")).await;
    assert_eq!(error_code(&unknown), "internal");
    assert_eq!(error_data(&unknown)["runtime_code"], "exploded");
}

#[tokio::test]
async fn bridge_violations_fail_jobs_with_engine_codes() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 4, 8).await;
    let crashed = run(&scheduler, diagnose("crash")).await;
    assert_eq!(error_code(&crashed), "runtime_crashed");
    assert_eq!(error_data(&crashed)["exit_code"], 3);
    assert!(error_data(&crashed)["stderr_tail"]
        .as_str()
        .unwrap()
        .contains("stub crashed on purpose"));

    for text in ["wrong-id", "huge"] {
        let job = run(&scheduler, diagnose(text)).await;
        assert_eq!(error_code(&job), "runtime_protocol", "{text}");
    }
    let anonymous = run(&scheduler, diagnose("null-id")).await;
    assert_eq!(error_code(&anonymous), "runtime_protocol");
    assert!(error_data(&anonymous)["detail"]
        .as_str()
        .unwrap()
        .contains("Request is not valid JSON."));

    let silent = project.scheduler("stub_without_ready", 1, 8).await;
    let job = run(&silent, OperationRequest::Doctor(DoctorParams {})).await;
    assert_eq!(error_code(&job), "runtime_unavailable");
    assert!(error_data(&job)["stderr_tail"]
        .as_str()
        .unwrap()
        .contains("unknown option"));

    let outdated = project.scheduler("stub_protocol_1", 1, 8).await;
    let job = run(&outdated, OperationRequest::Doctor(DoctorParams {})).await;
    assert_eq!(error_code(&job), "runtime_protocol");
    assert_eq!(error_data(&job)["got"], 1);

    let missing = project
        .engine(
            EngineMode::Cli,
            BridgeConfig {
                python: "/nonexistent/python3".into(),
                module: "stub".into(),
            },
            1,
            1,
        )
        .await;
    let job = run(&missing, OperationRequest::Doctor(DoctorParams {})).await;
    assert_eq!(error_code(&job), "runtime_unavailable");
}

#[tokio::test]
async fn cancelling_a_running_job_kills_it_and_finishes_once() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 8).await;
    let mut events = scheduler.subscribe();
    let job = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap()
        .into_job();
    wait_until_running(&scheduler, job.id).await;
    let started = Instant::now();
    scheduler.cancel(job.id).await.unwrap();
    let finished = scheduler.wait(job.id).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(finished.status, JobStatus::Cancelled);
    assert_eq!(error_code(&finished), "cancelled");
    assert_eq!(error_data(&finished)["by"], "client");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut finishes = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, EngineEvent::Job(record) if record.id == job.id && record.status.is_terminal())
        {
            finishes += 1;
        }
    }
    assert_eq!(finishes, 1);
    assert_eq!(
        scheduler.cancel(job.id).await.unwrap().status,
        JobStatus::Cancelled
    );
}

#[tokio::test]
async fn queued_jobs_cancel_without_running_and_a_full_queue_creates_no_row() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 1).await;
    let mut events = scheduler.subscribe();
    let busy = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap()
        .into_job()
        .id;
    wait_until_running(&scheduler, busy).await;
    let mut accepted = vec![busy];
    let mut rejection = None;
    for _ in 0..6 {
        match scheduler.submit(JobOrigin::Cli, diagnose("sleep")).await {
            Ok(submission) => accepted.push(submission.into_job().id),
            Err(error) => {
                rejection = Some(error);
                break;
            }
        }
    }
    assert_eq!(rejection, Some(EngineError::QueueFull { capacity: 1 }));
    let rows = scheduler.store().jobs(None, 50).unwrap().items.len();
    assert_eq!(rows, accepted.len());

    let last = *accepted.last().unwrap();
    let cancelled = scheduler.cancel(last).await.unwrap();
    assert_eq!(
        cancelled.status,
        JobStatus::Cancelled,
        "while the worker is busy"
    );
    assert!(cancelled.started_at.is_none(), "a queued job never spawns");
    assert_eq!(
        scheduler
            .store()
            .get_job(accepted[0])
            .unwrap()
            .unwrap()
            .status,
        JobStatus::Running
    );
    for id in &accepted {
        scheduler.cancel(*id).await.unwrap();
    }
    for id in &accepted {
        let job = scheduler.wait(*id).await.unwrap();
        assert_eq!(job.status, JobStatus::Cancelled);
    }
    let queued = scheduler.store().get_job(last).unwrap().unwrap();
    assert!(queued.started_at.is_none());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut finishes = HashMap::new();
    while let Ok(event) = events.try_recv() {
        match event {
            EngineEvent::Job(job) if job.status.is_terminal() => {
                *finishes.entry(job.id).or_insert(0) += 1;
            }
            _ => {}
        }
    }
    for id in &accepted {
        assert_eq!(finishes.get(id), Some(&1), "{id} finishes exactly once");
    }
}

#[tokio::test]
async fn a_job_over_its_timeout_fails_with_timeout() {
    let project = Project::new("budgets:\n  render_seconds: 10\n");
    let scheduler = project.scheduler("stub", 1, 8).await;
    let job = run(&scheduler, diagnose("sleep")).await;
    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(error_code(&job), "timeout");
    assert_eq!(error_data(&job)["timeout_seconds"], 10);
}

#[tokio::test]
async fn captions_jobs_publish_their_validated_output() {
    let project = Project::new("");
    fs::create_dir_all(project.root.join("captions")).unwrap();
    fs::write(
        project.root.join("captions/en.vtt"),
        "WEBVTT\n\n00:00.000 --> 00:01.000\nHi\n",
    )
    .unwrap();
    let scheduler = project.scheduler("stub", 1, 8).await;
    let job = run(
        &scheduler,
        OperationRequest::Captions(CaptionsParams {
            path: "captions/en.vtt".into(),
            shift_seconds: 0.0,
            scale: 1.0,
            output: Some("captions/out/en.srt".into()),
        }),
    )
    .await;
    assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
    let artifact = &job.result.as_ref().unwrap().artifacts()[0];
    assert_eq!(artifact.path, "captions/out/en.srt");
    assert!(artifact.bytes > 0);
}

#[tokio::test]
async fn identical_renders_coalesce_once_the_runtime_identity_is_known() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 8).await;
    let render = || {
        OperationRequest::Render(RenderParams {
            scene: Some("SlowScene".into()),
            profile: Some("draft".into()),
            ..Default::default()
        })
    };
    let first = scheduler.submit(JobOrigin::Cli, render()).await.unwrap();
    let Submission::Queued(first) = first else {
        panic!("expected a new job")
    };
    assert!(
        first.fingerprint.is_none(),
        "no submit fingerprint before any worker reported its identity"
    );
    let mut started = None;
    for _ in 0..200 {
        started = scheduler
            .store()
            .get_job(first.id)
            .unwrap()
            .unwrap()
            .fingerprint;
        if started.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(started.is_some(), "the start fingerprint is recorded");
    let identity = scheduler.runtime().borrow().clone().unwrap();
    assert_eq!(identity.runtime_version, "2.0.0-stub");
    assert_eq!(identity.catalog.themes[0].name, "midnight");

    let second = scheduler.submit(JobOrigin::Http, render()).await.unwrap();
    let Submission::Coalesced(second) = second else {
        panic!("expected coalescing")
    };
    assert_eq!(second.id, first.id);
    let fresh = scheduler
        .submit(
            JobOrigin::Cli,
            OperationRequest::Render(RenderParams {
                scene: Some("SlowScene".into()),
                profile: Some("draft".into()),
                fresh: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert!(matches!(fresh, Submission::Queued(_)));
    for id in [first.id, fresh.job().id] {
        scheduler.cancel(id).await.unwrap();
        scheduler.wait(id).await.unwrap();
    }
    let out_dir = project.root.join(ARTIFACTS_DIR).join(first.id.to_string());
    assert!(!out_dir.exists(), "a cancelled job leaves no artifacts");
}

#[tokio::test]
async fn renders_are_validated_cached_and_feed_default_sources() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg is not installed");
        return;
    }
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 2, 8).await;
    let render = || {
        OperationRequest::Render(RenderParams {
            scene: Some("MainScene".into()),
            profile: Some("draft".into()),
            ..Default::default()
        })
    };
    let first = run(&scheduler, render()).await;
    assert_eq!(first.status, JobStatus::Succeeded, "{:?}", first.error);
    assert_eq!(first.scene_id.as_deref(), Some("scenes/main.py#MainScene"));
    assert!(first.scene_revision.is_some());
    let result = first.result.as_ref().unwrap();
    let video = result.artifact(ArtifactKind::Video).unwrap();
    let media = video.media.as_ref().unwrap();
    assert_eq!(
        (media.width, media.height, media.container.as_str()),
        (854, 480, "mp4")
    );
    assert!(result.artifact(ArtifactKind::Timeline).unwrap().bytes > 0);

    let cached = scheduler.submit(JobOrigin::Cli, render()).await.unwrap();
    let Submission::Cached(cached) = cached else {
        panic!("expected a cache hit")
    };
    assert!(cached.cached);
    assert_eq!(cached.cached_from, Some(first.id));
    assert_eq!(cached.scene_id, first.scene_id);

    let frame = run(
        &scheduler,
        OperationRequest::Frame(FrameParams {
            at_seconds: 0.5,
            source: None,
            scene: Some("MainScene".into()),
            profile: None,
        }),
    )
    .await;
    assert_eq!(frame.status, JobStatus::Succeeded, "{:?}", frame.error);
    assert_eq!(frame.source_job_id, Some(cached.id));
    assert_eq!(frame.scene_id, first.scene_id);
    let Some(OperationResult::Frame(grabbed)) = &frame.result else {
        panic!("expected a frame result")
    };
    let image = grabbed.artifacts[0].media.as_ref().unwrap();
    assert_eq!((image.width, image.height), (854, 480));
    assert_eq!(
        grabbed.source.as_ref().unwrap().scene.as_deref(),
        Some("MainScene")
    );

    fs::remove_file(project.root.join(&video.path)).unwrap();
    let rerun = scheduler.submit(JobOrigin::Cli, render()).await.unwrap();
    assert!(
        matches!(rerun, Submission::Queued(_)),
        "a stale cache entry is evicted"
    );
    scheduler.wait(rerun.job().id).await.unwrap();
}

#[tokio::test]
async fn discover_is_cached_and_reports_engine_findings() {
    let project = Project::new("");
    fs::write(
        project.root.join("scenes/huge.py"),
        vec![b'#'; 2 * 1024 * 1024 + 1],
    )
    .unwrap();
    let scheduler = project.scheduler("stub", 1, 8).await;
    let index = scheduler.discover().await.unwrap();
    assert_eq!(index.files, 1);
    assert_eq!(index.scenes[0].name, "MainScene");
    assert_eq!(index.findings[0].code, "file_too_large");

    let again = scheduler.discover().await.unwrap();
    assert_eq!(again.scenes, index.scenes);
    assert_eq!(
        again.findings.len(),
        1,
        "engine findings are not cached twice"
    );
    let calls = fs::read_to_string(project.root.join("discover-calls.txt")).unwrap();
    assert_eq!(calls.lines().count(), 1, "the second scan is a cache hit");

    fs::write(
        project.root.join("scenes/extra.py"),
        "class Extra(Scene):\n    pass\n",
    )
    .unwrap();
    let changed = scheduler.discover().await.unwrap();
    assert_eq!(changed.scenes.len(), 2);
}

#[tokio::test]
async fn a_scan_that_raced_an_edit_is_not_cached() {
    let project = Project::new("");
    fs::write(
        project.root.join("scenes/main.py"),
        "# edited during the scan\nclass MainScene(Scene):\n    pass\n",
    )
    .unwrap();
    let scheduler = project.scheduler("stub", 1, 8).await;
    let names = |index: DiscoverResult| -> Vec<String> {
        index.scenes.into_iter().map(|scene| scene.name).collect()
    };
    assert_eq!(names(scheduler.discover().await.unwrap()), ["MainScene"]);
    assert_eq!(
        names(scheduler.discover().await.unwrap()),
        ["MainScene", "Late"],
        "the edit is scanned, not hidden behind the older scan"
    );
}

#[tokio::test]
async fn direct_operations_are_not_jobs() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 8).await;
    let error = scheduler
        .submit(
            JobOrigin::Http,
            OperationRequest::Init(InitParams::default()),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        EngineError::OperationNotAllowed {
            operation: Operation::Init,
            ..
        }
    ));
}

#[tokio::test]
async fn init_creates_a_project_through_the_runtime() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("film");
    let result = init_project(
        &stub("stub"),
        &target,
        InitParams {
            name: Some("Film".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(result.scene.file, "scenes/main.py");
    assert!(result.artifacts.iter().all(|artifact| artifact.bytes > 0));
    assert!(target.join("director.yaml").is_file());
}

/// `(pid, detail)` per line of a stub record file.
fn stub_records(project: &Project, name: &str) -> Vec<(u32, String)> {
    fs::read_to_string(project.root.join(".manim-director").join(name))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let (pid, detail) = line.split_once(' ').unwrap();
            (pid.parse().unwrap(), detail.to_owned())
        })
        .collect()
}

fn job_logs(scheduler: &Scheduler, id: Uuid) -> Vec<String> {
    let records = scheduler.store().logs(id, None, 100).unwrap().items;
    records.into_iter().map(|record| record.message).collect()
}

#[cfg(unix)]
#[tokio::test]
async fn long_lived_engines_serve_jobs_from_a_prewarmed_worker() {
    let project = Project::new("");
    let mut config = SchedulerConfig::new(EngineMode::Serve);
    config.bridge = stub("stub");
    config.prewarm = Some(PrewarmPolicy::default());
    let scheduler = Scheduler::open(&project.root, config).await.unwrap();
    let mut runtime = scheduler.runtime();
    tokio::time::timeout(Duration::from_secs(10), runtime.wait_for(Option::is_some))
        .await
        .expect("the idle worker reports ready")
        .unwrap();
    let idle = stub_records(&project, "stub-spawns.txt");
    assert_eq!(idle.len(), 1);

    let job = run(&scheduler, diagnose("warm")).await;
    assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
    let served = stub_records(&project, "stub-requests.txt");
    assert_eq!(served[0].0, idle[0].0, "the idle worker served the job");
    let logs = job_logs(&scheduler, job.id);
    assert!(logs
        .iter()
        .any(|line| line.contains("Runtime 2.0.0-stub ready")));
    assert!(logs
        .iter()
        .any(|line| line.contains("Preloading moderngl failed")));
    let stored = scheduler
        .store()
        .runtime(scheduler.python())
        .unwrap()
        .expect("the identity is recorded");
    assert_eq!(stored.identity.catalog.project_templates, ["explainer"]);

    for _ in 0..200 {
        if stub_records(&project, "stub-spawns.txt").len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let replacement = stub_records(&project, "stub-spawns.txt")[1].0;
    scheduler.shutdown().await;
    // SAFETY: signal 0 only checks whether the process exists.
    let alive = unsafe { libc::kill(replacement as i32, 0) } == 0;
    assert!(!alive, "shutdown retires the idle worker");
}

#[tokio::test]
async fn renders_of_one_scene_wait_for_each_other() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 2, 8).await;
    let render = || {
        OperationRequest::Render(RenderParams {
            scene: Some("SlowScene".into()),
            profile: Some("draft".into()),
            fresh: true,
            ..Default::default()
        })
    };
    let first = scheduler
        .submit(JobOrigin::Cli, render())
        .await
        .unwrap()
        .into_job();
    wait_until_running(&scheduler, first.id).await;
    let second = scheduler
        .submit(JobOrigin::Http, render())
        .await
        .unwrap()
        .into_job();
    wait_until_running(&scheduler, second.id).await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let waiting = scheduler.store().get_job(second.id).unwrap().unwrap();
    let progress = waiting.progress.expect("progress while waiting");
    assert_eq!(progress.phase, ProgressPhase::Starting);
    assert_eq!(
        progress.message.as_deref(),
        Some("Waiting for another render of SlowScene")
    );
    assert_eq!(
        stub_records(&project, "stub-requests.txt").len(),
        1,
        "the second render has not reached the runtime"
    );

    scheduler.cancel(first.id).await.unwrap();
    for _ in 0..200 {
        if stub_records(&project, "stub-requests.txt").len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(stub_records(&project, "stub-requests.txt").len(), 2);
    scheduler.cancel(second.id).await.unwrap();
    assert_eq!(
        scheduler.wait(second.id).await.unwrap().status,
        JobStatus::Cancelled
    );
}

#[tokio::test]
async fn a_job_waiting_for_a_scene_lock_leaves_its_worker_to_other_jobs() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 2, 8).await;
    let render = || {
        OperationRequest::Render(RenderParams {
            scene: Some("SlowScene".into()),
            fresh: true,
            ..Default::default()
        })
    };
    let mut renders = Vec::new();
    for _ in 0..2 {
        let job = scheduler.submit(JobOrigin::Cli, render()).await.unwrap();
        wait_until_running(&scheduler, job.job().id).await;
        renders.push(job.into_job().id);
    }
    let other = run(&scheduler, diagnose("x")).await;
    assert_eq!(other.status, JobStatus::Succeeded, "{:?}", other.error);
    for id in renders {
        scheduler.cancel(id).await.unwrap();
        scheduler.wait(id).await.unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn the_memory_ceiling_comes_from_the_budget_only_when_set() {
    let limited = Project::new("budgets:\n  memory_mb: 4096\n");
    let scheduler = limited.scheduler("stub", 1, 8).await;
    let job = run(&scheduler, diagnose("rlimit")).await;
    assert_eq!(job.limits.memory_mb, Some(4096));
    let Some(OperationResult::Diagnose(result)) = &job.result else {
        panic!("expected a diagnose result: {:?}", job.error)
    };
    assert_eq!(result.findings[0].message, (4096_u64 << 20).to_string());

    let open = Project::new("");
    let scheduler = open.scheduler("stub", 1, 8).await;
    let job = run(&scheduler, diagnose("rlimit")).await;
    assert_eq!(job.limits.memory_mb, None);
    let Some(OperationResult::Diagnose(result)) = &job.result else {
        panic!("expected a diagnose result: {:?}", job.error)
    };
    assert_eq!(result.findings[0].message, "unlimited");
}

#[tokio::test]
async fn a_request_the_worker_cannot_take_is_retried_once_on_a_fresh_worker() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub_dies_idle", 1, 8).await;
    let job = run(&scheduler, diagnose("x")).await;
    assert_eq!(error_code(&job), "runtime_crashed");
    assert_eq!(stub_records(&project, "stub-spawns.txt").len(), 2);
    assert!(job_logs(&scheduler, job.id)
        .iter()
        .any(|line| line.contains("retrying with a fresh one")));
}

fn finishes(events: &mut broadcast::Receiver<EngineEvent>) -> HashMap<Uuid, Vec<Arc<JobRecord>>> {
    let mut finished: HashMap<Uuid, Vec<Arc<JobRecord>>> = HashMap::new();
    while let Ok(event) = events.try_recv() {
        match event {
            EngineEvent::Job(job) if job.status.is_terminal() => {
                finished.entry(job.id).or_default().push(job);
            }
            _ => {}
        }
    }
    finished
}

#[tokio::test]
async fn logs_are_complete_when_a_job_finishes() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 8).await;
    let job = run(&scheduler, diagnose("chatty")).await;
    assert_eq!(job.status, JobStatus::Succeeded, "{:?}", job.error);
    let mut chatter = 0;
    let mut after = None;
    loop {
        let page = scheduler.store().logs(job.id, after, 500).unwrap();
        chatter += page
            .items
            .iter()
            .filter(|record| record.message.starts_with("chatter "))
            .count();
        match page.next_cursor {
            Some(cursor) => after = Some(cursor.parse().unwrap()),
            None => break,
        }
    }
    assert_eq!(
        chatter, 500,
        "every stderr line is stored before the job ends"
    );
}

#[tokio::test]
async fn a_second_engine_leaves_running_jobs_alone_and_cancels_across_processes() {
    let project = Project::new("");
    let first = project.scheduler("stub", 1, 8).await;
    let job = first
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap()
        .into_job();
    wait_until_running(&first, job.id).await;

    let second = project.engine(EngineMode::Serve, stub("stub"), 1, 8).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let still = second.store().get_job(job.id).unwrap().unwrap();
    assert_eq!(still.status, JobStatus::Running, "{:?}", still.error);
    assert_eq!(still.owner, first.instance_id());

    let started = Instant::now();
    let flagged = second.cancel(job.id).await.unwrap();
    assert!(flagged.cancel_requested);
    let finished = second.wait(job.id).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(finished.status, JobStatus::Cancelled);
    assert_eq!(error_data(&finished)["by"], "client");
}

#[tokio::test]
async fn jobs_of_a_vanished_engine_fail_as_engine_lost_and_are_published() {
    let project = Project::new("");
    let store = Store::open(project.root.join(".manim-director/state.db")).unwrap();
    let before = Uuid::new_v4();
    crate::db::testing::queued_job(&store, before, Uuid::new_v4());

    let engine = project.engine(EngineMode::Mcp, stub("stub"), 1, 8).await;
    let reaped = engine.store().get_job(before).unwrap().unwrap();
    assert_eq!(reaped.status, JobStatus::Failed, "reaped at start");
    assert_eq!(error_code(&reaped), "engine_lost");

    let mut events = engine.subscribe();
    let later = Uuid::new_v4();
    let crashed = Uuid::new_v4();
    store
        .renew_lease(crashed, EngineMode::Cli, now_millis())
        .unwrap();
    crate::db::testing::queued_job(&store, later, crashed);
    store.set_running(later).unwrap();
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        store.get_job(later).unwrap().unwrap().status,
        JobStatus::Running,
        "a live lease protects the job"
    );
    store
        .renew_lease(
            crashed,
            EngineMode::Cli,
            now_millis() - crate::LEASE_STALE_MILLIS - 1,
        )
        .unwrap();
    let job = tokio::time::timeout(Duration::from_secs(5), engine.wait(later))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(error_code(&job), "engine_lost");
    assert_eq!(error_data(&job)["owner"], crashed.to_string());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(finishes(&mut events).get(&later).map(Vec::len), Some(1));
}

#[tokio::test]
async fn a_one_off_engine_takes_a_scene_over_from_an_engine_that_died_holding_it() {
    let project = Project::new("");
    let store = Store::open(project.root.join(".manim-director/state.db")).unwrap();
    let (dead, held) = (Uuid::new_v4(), Uuid::new_v4());
    // Fresh when the next command starts, stale soon after: a SIGKILL.
    let heartbeat = now_millis() - crate::LEASE_STALE_MILLIS + 2_000;
    store.renew_lease(dead, EngineMode::Cli, heartbeat).unwrap();
    crate::db::testing::queued_job(&store, held, dead);
    store.set_running(held).unwrap();
    assert!(store.try_lock_scene("SlowScene", held, dead).unwrap());

    let scheduler = project.scheduler("stub", 1, 8).await;
    let render = OperationRequest::Render(RenderParams {
        scene: Some("SlowScene".into()),
        fresh: true,
        ..Default::default()
    });
    let job = scheduler
        .submit(JobOrigin::Cli, render)
        .await
        .unwrap()
        .into_job();
    let started = Instant::now();
    while stub_records(&project, "stub-requests.txt").is_empty() {
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "still locked out"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let lost = store.get_job(held).unwrap().unwrap();
    assert_eq!(error_code(&lost), "engine_lost");
    scheduler.cancel(job.id).await.unwrap();
    scheduler.wait(job.id).await.unwrap();
}

#[tokio::test]
async fn reaping_a_copied_projects_jobs_never_touches_the_original_artifacts() {
    let original = Project::new("");
    let copy = Project::new("");
    let store = Store::open(copy.root.join(".manim-director/state.db")).unwrap();
    let id = Uuid::new_v4();
    let foreign_dir = artifacts::job_dir(&original.root, id);
    let task = manim_director_core::Task::Frame(manim_director_core::FrameTask {
        video: original.root.join("clip.mp4"),
        at_seconds: 0.0,
        out_dir: foreign_dir.clone(),
    });
    let request = OperationRequest::Frame(FrameParams {
        at_seconds: 0.0,
        source: None,
        scene: None,
        profile: None,
    });
    store
        .insert_job(&crate::NewJob {
            id,
            origin: JobOrigin::Cli,
            owner: Uuid::new_v4(),
            request: &request,
            task: &task,
            limits: manim_director_core::Limits {
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
    for dir in [&foreign_dir, &artifacts::job_dir(&copy.root, id)] {
        artifacts::create_out_dir(dir.ancestors().nth(3).unwrap(), dir).unwrap();
        fs::write(dir.join("frame.png"), "x").unwrap();
    }

    let engine = copy.engine(EngineMode::Mcp, stub("stub"), 1, 8).await;
    assert_eq!(
        error_code(&engine.store().get_job(id).unwrap().unwrap()),
        "engine_lost"
    );
    assert!(
        foreign_dir.join("frame.png").is_file(),
        "the original keeps its files"
    );
    assert!(!artifacts::job_dir(&copy.root, id).exists());
}

#[tokio::test]
async fn a_full_queue_is_a_typed_error_that_leaks_no_handle_and_frees_on_cancel() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 2).await;
    let busy = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap()
        .into_job()
        .id;
    wait_until_running(&scheduler, busy).await;
    let mut queued = Vec::new();
    for _ in 0..2 {
        queued.push(
            scheduler
                .submit(JobOrigin::Cli, diagnose("sleep"))
                .await
                .unwrap()
                .into_job()
                .id,
        );
    }
    let full = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap_err();
    assert_eq!(full, EngineError::QueueFull { capacity: 2 });
    assert_eq!(full.status(), 429);
    assert_eq!(scheduler.active_jobs(), 3, "the rejected job holds nothing");
    assert_eq!(scheduler.store().jobs(None, 50).unwrap().items.len(), 3);

    scheduler.cancel(queued[0]).await.unwrap();
    assert_eq!(scheduler.active_jobs(), 2);
    let replacement = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .expect("a cancelled job gives its queue slot back");
    for id in [busy, queued[1], replacement.job().id] {
        scheduler.cancel(id).await.unwrap();
        scheduler.wait(id).await.unwrap();
    }
}

#[tokio::test]
async fn a_finish_that_loses_to_another_engine_publishes_the_winner_once() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 8).await;
    let mut events = scheduler.subscribe();
    let job = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap()
        .into_job();
    wait_until_running(&scheduler, job.id).await;
    let lost = ErrorBody::new("engine_lost", "reaped elsewhere", None);
    scheduler
        .store()
        .finish_error(job.id, JobStatus::Failed, &lost, None)
        .unwrap();
    scheduler
        .inner
        .cancel_local(job.id, CancelledBy::Client)
        .await
        .unwrap();
    for _ in 0..200 {
        if scheduler.active_jobs() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let finished = finishes(&mut events);
    let published = &finished[&job.id];
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].status, JobStatus::Failed);
    assert_eq!(
        error_code(&scheduler.store().get_job(job.id).unwrap().unwrap()),
        "engine_lost"
    );
}

#[tokio::test]
async fn shutdown_cancels_own_jobs_and_releases_the_lease() {
    let project = Project::new("");
    let scheduler = project.scheduler("stub", 1, 8).await;
    let running = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap()
        .into_job();
    let queued = scheduler
        .submit(JobOrigin::Cli, diagnose("sleep"))
        .await
        .unwrap()
        .into_job();
    wait_until_running(&scheduler, running.id).await;
    scheduler.shutdown().await;
    for id in [running.id, queued.id] {
        let job = scheduler.store().get_job(id).unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Cancelled);
        assert_eq!(error_data(&job)["by"], "shutdown");
    }
    assert_eq!(scheduler.active_jobs(), 0);
    let error = scheduler
        .submit(JobOrigin::Cli, diagnose("late"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "internal");

    // The lease is gone, so a later engine reaps nothing of ours and an
    // orphan row with our id would count as abandoned.
    let other = Uuid::new_v4();
    let owned = Uuid::new_v4();
    crate::db::testing::queued_job(scheduler.store(), owned, scheduler.instance_id());
    assert_eq!(
        scheduler.store().reap(other, now_millis()).unwrap()[0].id,
        owned
    );
}
