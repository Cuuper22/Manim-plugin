//! Bridge tests against the stub runtime in `tests/fixtures/stub_runtime.py`.

use super::*;
use manim_director_core::{DiagnoseTask, DiscoverTask, DoctorTask};
use std::{
    fs,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::time::Instant;

const STUB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stub_runtime.py"
);

struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    readies: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        fs::create_dir_all(root.join(".manim-director")).unwrap();
        Self {
            _directory: directory,
            root,
            readies: Arc::default(),
        }
    }

    fn bridge(&self, module: &str) -> RuntimeBridge {
        let readies = self.readies.clone();
        RuntimeBridge::with_ready_sink(
            BridgeConfig {
                python: STUB.into(),
                module: module.into(),
            },
            Arc::new(move |_| {
                readies.fetch_add(1, Ordering::SeqCst);
            }),
        )
    }

    /// `(pid, detail)` per line of a stub record file.
    fn records(&self, name: &str) -> Vec<(u32, String)> {
        fs::read_to_string(self.root.join(".manim-director").join(name))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let (pid, detail) = line.split_once(' ').unwrap();
                (pid.parse().unwrap(), detail.to_owned())
            })
            .collect()
    }

    async fn until(&self, what: &str, done: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(self) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

fn quick_policy() -> PrewarmPolicy {
    PrewarmPolicy {
        max_idle: Duration::from_secs(600),
        backoff: [Duration::from_millis(50); 3],
    }
}

fn diagnose(text: &str) -> Task {
    Task::Diagnose(DiagnoseTask { text: text.into() })
}

/// Runs `task` on an acquired worker; returns the outcome and its events.
async fn run_job(
    bridge: &RuntimeBridge,
    root: &Path,
    key: &SpawnKey,
    task: &Task,
) -> (BridgeOutcome, Vec<String>) {
    let cancel = CancellationToken::new();
    let mut events = Vec::new();
    let mut on_event = |event: BridgeEvent<'_>| {
        events.push(match event {
            BridgeEvent::Ready(frame) => format!("ready {:?}", frame.preloaded),
            BridgeEvent::Progress(frame) => format!("progress {:?}", frame.phase),
            BridgeEvent::Log(frame) => format!("log {}", frame.message),
            BridgeEvent::Stderr(line) => format!("stderr {line}"),
        })
    };
    let request_id = Uuid::new_v4().to_string();
    let line = request_line(&request_id, root, task).unwrap();
    let outcome = match bridge.acquire(root, key, &cancel, &mut on_event).await {
        Ok(worker) => match worker
            .converse(&line, &request_id, &cancel, &mut on_event)
            .await
        {
            Ok(outcome) => outcome,
            Err(Undelivered(error)) => BridgeOutcome::Failed(error),
        },
        Err(outcome) => outcome,
    };
    (outcome, events)
}

fn message(outcome: &BridgeOutcome) -> &str {
    match outcome {
        BridgeOutcome::Succeeded(value) => value["findings"][0]["message"].as_str().unwrap(),
        other => panic!("expected success, got {other:?}"),
    }
}

#[test]
fn oversized_requests_are_refused_before_any_worker() {
    let task = Task::Discover(DiscoverTask {
        files: vec![PathBuf::from("x".repeat(1024)); 4200],
    });
    let error = request_line("id", Path::new("/p"), &task).unwrap_err();
    assert_eq!(error.code(), "request_too_large");
    let line = request_line("id", Path::new("/p"), &Task::Doctor(DoctorTask {})).unwrap();
    assert_eq!(line.last(), Some(&b'\n'));
}

#[tokio::test]
async fn a_job_takes_the_prewarmed_worker_and_a_replacement_starts() {
    let fixture = Fixture::new();
    let mut bridge = fixture.bridge("stub");
    let key = SpawnKey::default();
    bridge.start_prewarm(&fixture.root, key.clone(), quick_policy());
    fixture
        .until("the idle worker", |fixture| {
            fixture.readies.load(Ordering::SeqCst) == 1
        })
        .await;
    let idle = fixture.records("stub-spawns.txt");
    assert_eq!(idle.len(), 1);
    assert_eq!(idle[0].1, "preload");

    let (outcome, events) = run_job(&bridge, &fixture.root, &key, &diagnose("hello")).await;
    assert_eq!(message(&outcome), "hello");
    assert_eq!(
        events.first().map(String::as_str),
        Some("ready [\"numpy\"]")
    );
    assert!(events.contains(&"progress Analyze".to_owned()));
    assert!(events.contains(&"stderr a print from user code".to_owned()));
    let served = fixture.records("stub-requests.txt");
    assert_eq!(
        served[0].0, idle[0].0,
        "the pre-warmed process served the job"
    );

    fixture
        .until("the replacement", |fixture| {
            fixture.readies.load(Ordering::SeqCst) == 2
        })
        .await;
    let (outcome, _) = run_job(&bridge, &fixture.root, &key, &diagnose("again")).await;
    assert_eq!(message(&outcome), "again");
    let spawns = fixture.records("stub-spawns.txt");
    assert_eq!(fixture.records("stub-requests.txt")[1].0, spawns[1].0);
    bridge.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_job_with_another_key_gets_a_worker_spawned_with_that_key() {
    let fixture = Fixture::new();
    let mut bridge = fixture.bridge("stub");
    bridge.start_prewarm(&fixture.root, SpawnKey::default(), quick_policy());
    let (unlimited, _) = run_job(
        &bridge,
        &fixture.root,
        &SpawnKey::default(),
        &diagnose("rlimit"),
    )
    .await;
    assert_eq!(message(&unlimited), "unlimited", "RLIMIT_AS is opt-in");

    let limited = SpawnKey {
        memory_mb: Some(4096),
        manim_cfg: None,
    };
    let (outcome, _) = run_job(&bridge, &fixture.root, &limited, &diagnose("rlimit")).await;
    assert_eq!(message(&outcome), (4096_u64 << 20).to_string());
    let (outcome, _) = run_job(&bridge, &fixture.root, &limited, &diagnose("rlimit")).await;
    assert_eq!(
        message(&outcome),
        (4096_u64 << 20).to_string(),
        "later idle workers carry the new key"
    );
    bridge.close().await;
}

#[tokio::test]
async fn idle_workers_that_die_are_respawned_until_prewarming_gives_up() {
    let fixture = Fixture::new();
    let mut bridge = fixture.bridge("stub_dies_idle");
    bridge.start_prewarm(&fixture.root, SpawnKey::default(), quick_policy());
    fixture
        .until("four attempts", |fixture| {
            fixture.records("stub-spawns.txt").len() == 4
        })
        .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        fixture.records("stub-spawns.txt").len(),
        4,
        "pre-warming stopped after the backoff ran out"
    );

    let (outcome, _) = run_job(&bridge, &fixture.root, &SpawnKey::default(), &diagnose("x")).await;
    let BridgeOutcome::Failed(error) = outcome else {
        panic!("expected a failure")
    };
    assert_eq!(error.code, "runtime_crashed");
    assert_eq!(
        fixture.records("stub-spawns.txt").len(),
        5,
        "spawned on demand"
    );

    let other = SpawnKey {
        memory_mb: None,
        manim_cfg: Some("changed".into()),
    };
    run_job(&bridge, &fixture.root, &other, &diagnose("x")).await;
    fixture
        .until("pre-warming to resume", |fixture| {
            fixture.records("stub-spawns.txt").len() >= 7
        })
        .await;
    bridge.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn closing_the_bridge_retires_the_idle_worker() {
    let fixture = Fixture::new();
    let mut bridge = fixture.bridge("stub");
    bridge.start_prewarm(&fixture.root, SpawnKey::default(), quick_policy());
    fixture
        .until("the idle worker", |fixture| {
            fixture.readies.load(Ordering::SeqCst) == 1
        })
        .await;
    let pid = fixture.records("stub-spawns.txt")[0].0;
    bridge.close().await;
    // SAFETY: signal 0 only checks whether the process exists.
    let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
    assert!(!alive, "the idle worker exited");
    assert_eq!(fixture.records("stub-spawns.txt").len(), 1);
}

#[tokio::test]
async fn direct_operations_use_a_light_worker_and_cancel_kills_it() {
    let fixture = Fixture::new();
    let mut bridge = fixture.bridge("stub");
    bridge.start_prewarm(&fixture.root, SpawnKey::default(), quick_policy());
    let cancel = CancellationToken::new();
    let mut ignore = |_: BridgeEvent<'_>| {};
    let outcome = bridge
        .run_direct(&fixture.root, &diagnose("direct"), &cancel, &mut ignore)
        .await;
    assert_eq!(message(&outcome), "direct");
    let spawns = fixture.records("stub-spawns.txt");
    let served = fixture.records("stub-requests.txt");
    let server = spawns.iter().find(|(pid, _)| *pid == served[0].0).unwrap();
    assert_eq!(server.1, "plain", "never the pre-warmed worker");

    let started = Instant::now();
    let sleep = diagnose("sleep");
    let sleeper = bridge.run_direct(&fixture.root, &sleep, &cancel, &mut ignore);
    tokio::pin!(sleeper);
    tokio::select! {
        _ = &mut sleeper => panic!("the sleeper finished"),
        _ = tokio::time::sleep(Duration::from_millis(500)) => cancel.cancel(),
    }
    assert!(matches!(sleeper.await, BridgeOutcome::Cancelled));
    assert!(started.elapsed() < Duration::from_secs(5));
    bridge.close().await;
}
