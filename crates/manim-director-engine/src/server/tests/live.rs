//! HTTP §8 and §14 items 40–54: the event stream, job feeds, the file
//! watcher and the scene index, plus binding a real socket.

use super::*;
use crate::{
    db::testing,
    server::{feed, watch, ServeConfig, Server},
    RuntimeIdentity,
};
use manim_director_core::{
    Catalog, CatalogTheme, DiagnoseParams, DoctorParams, DoctorTask, ErrorBody, JobOrigin,
    JobStatus, OperationRequest, Task,
};
use serde_json::json;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

fn seq(id: &str) -> u64 {
    id.rsplit_once('.').unwrap().1.parse().unwrap()
}

impl Harness {
    fn announce(&self, n: usize) {
        for index in 0..n {
            self.state
                .file_changed(&format!("scenes/{index}.py"), Some(format!("r{index}")));
        }
    }

    fn spawn(&self, tasks: &[&str]) -> CancellationToken {
        let stop = CancellationToken::new();
        for task in tasks {
            let (state, stop) = (self.state.clone(), stop.clone());
            match *task {
                "feed" => {
                    let jobs = state.scheduler.subscribe();
                    tokio::spawn(feed::run(state, jobs, stop));
                }
                "files" => {
                    tokio::spawn(watch::files(state, stop));
                }
                "index" => {
                    tokio::spawn(watch::index(state, stop));
                }
                other => panic!("no task {other}"),
            }
        }
        stop
    }
}

#[tokio::test]
async fn streams_replay_from_a_cursor_and_resync_when_they_cannot() {
    let harness = Harness::with(
        "",
        Options {
            ring: 8,
            ..Options::default()
        },
    )
    .await;
    harness.announce(3);
    let cursor = harness.get("/api/state").await.json()["event_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(seq(&cursor), 3);
    let instance = cursor.split_once('.').unwrap().0.to_owned();

    let mut replay = harness.events(&format!("?after={instance}.1"), None).await;
    let first = replay.next().await;
    assert_eq!(first.id.as_deref(), Some(format!("{instance}.2").as_str()));
    assert_eq!(first.name, "file");
    assert_eq!(
        first.data,
        json!({"type": "file", "path": "scenes/1.py", "revision": "r1"})
    );
    assert_eq!(seq(&replay.next().await.id.unwrap()), 3);
    harness.announce(1);
    assert!(
        replay
            .next_within(Duration::from_millis(300))
            .await
            .is_none(),
        "same revision"
    );
    harness.state.file_changed("scenes/0.py", None);
    assert_eq!(seq(&replay.next().await.id.unwrap()), 4);

    let header_wins = harness
        .events(
            &format!("?after={instance}.0"),
            Some(&format!("{instance}.3")),
        )
        .await
        .next()
        .await;
    assert_eq!(seq(&header_wins.id.unwrap()), 4);

    let mut stranger = harness
        .events(&format!("?after={}.2", uuid::Uuid::new_v4()), None)
        .await;
    let resync = stranger.next().await;
    assert_eq!(resync.name, "resync");
    assert_eq!(resync.data["reason"], "unknown_cursor");
    assert_eq!(resync.id.as_deref(), Some(format!("{instance}.4").as_str()));

    for index in 10..20 {
        harness
            .state
            .file_changed(&format!("scenes/{index}.py"), None);
    }
    let mut late = harness.events(&format!("?after={instance}.2"), None).await;
    let expired = late.next().await;
    assert_eq!(expired.data["reason"], "expired");
    assert_eq!(seq(&expired.id.unwrap()), 14);
    assert!(late.next_within(Duration::from_millis(200)).await.is_none());
}

#[tokio::test]
async fn at_most_32_streams_are_open_at_once() {
    let harness = Harness::new("").await;
    let mut open = Vec::new();
    for _ in 0..32 {
        open.push(harness.events("", None).await);
    }
    let refused = harness.get("/api/events").await;
    assert_eq!(
        refused.error(StatusCode::TOO_MANY_REQUESTS),
        "too_many_streams"
    );
    assert_eq!(refused.json()["error"]["data"]["limit"], 32);
    drop(open.pop());
    tokio::time::sleep(Duration::from_millis(50)).await;
    open.push(harness.events("", None).await);
}

#[tokio::test]
async fn job_events_follow_the_lifecycle_then_patch_the_workspace() {
    let harness = Harness::new("").await;
    let stop = harness.spawn(&["feed"]);
    let cursor = harness.get("/api/state").await.json()["event_cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut events = harness.events(&format!("?after={cursor}"), None).await;
    let job = harness
        .json(Method::POST, "/api/jobs", &json!({"operation": "doctor"}))
        .await
        .json();
    let mut statuses = Vec::new();
    let mut last = seq(&cursor);
    while statuses.last().map(String::as_str) != Some("succeeded") {
        let event = events.next().await;
        let id = seq(event.id.as_deref().unwrap());
        assert!(id > last, "ids strictly increase");
        last = id;
        if event.name == "job" {
            assert_eq!(event.data["job"]["id"], job["id"]);
            statuses.push(event.data["job"]["status"].as_str().unwrap().to_owned());
        }
    }
    assert_eq!(statuses, ["queued", "running", "succeeded"]);
    let patch = events.until(|event| event.name == "workspace").await;
    let sections = &patch.data["sections"];
    assert_eq!(sections["doctor"]["job_id"], job["id"]);
    assert_eq!(sections["doctor"]["report"]["ok"], true);
    assert!(sections.get("findings").is_some());
    assert!(sections.get("project").is_none());
    stop.cancel();
}

#[tokio::test]
async fn other_engines_jobs_reach_the_stream() {
    let harness = Harness::new("").await;
    let stop = harness.spawn(&["feed"]);
    let mut events = harness.events("", None).await;
    let other = Scheduler::open(
        &harness.root,
        SchedulerConfig {
            mode: EngineMode::Mcp,
            workers: 1,
            queue_capacity: 4,
            bridge: BridgeConfig {
                python: STUB.into(),
                module: "stub".into(),
            },
            prune: PrunePolicy {
                keep_jobs: 500,
                keep_days: 30,
            },
            prewarm: None,
        },
    )
    .await
    .unwrap();
    let request = OperationRequest::Diagnose(DiagnoseParams {
        job_id: None,
        text: Some("hello".into()),
    });
    let job = other
        .submit(JobOrigin::Mcp, request)
        .await
        .unwrap()
        .into_job();
    other.wait(job.id).await.unwrap();
    let finished = events
        .until(|event| event.name == "job" && event.data["job"]["status"] == "succeeded")
        .await;
    assert_eq!(finished.data["job"]["id"], job.id.to_string());
    assert_eq!(finished.data["job"]["origin"], "mcp");
    other.shutdown().await;
    stop.cancel();
}

#[tokio::test]
async fn external_edits_reach_files_and_scenes_within_two_seconds() {
    let harness = Harness::new("").await;
    let stop = harness.spawn(&["files", "index"]);
    let indexed = |state: &Value| state["scene_index"]["state"] == "ready";
    for _ in 0..200 {
        if indexed(&harness.get("/api/state").await.json()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // Give the watcher its first, silent look.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let mut events = harness.events("", None).await;
    let edited = std::time::Instant::now();
    fs::write(
        harness.root.join("scenes/extra.py"),
        "class Extra(Scene):\n    pass\n",
    )
    .unwrap();
    let file = events.until(|event| event.name == "file").await;
    assert_eq!(file.data["path"], "scenes/extra.py");
    assert!(file.data["revision"].is_string());
    let patch = events
        .until(|event| event.name == "workspace" && event.data["sections"].get("scenes").is_some())
        .await;
    let scenes = patch.data["sections"]["scenes"].as_array().unwrap();
    assert!(scenes
        .iter()
        .any(|scene| scene["id"] == "scenes/extra.py#Extra"));
    assert!(
        edited.elapsed() < Duration::from_secs(2),
        "{:?}",
        edited.elapsed()
    );
    stop.cancel();
}

#[tokio::test]
async fn render_results_appear_as_latest_with_marks_and_go_stale() {
    if std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("skipping: ffmpeg is not installed");
        return;
    }
    let harness = Harness::new("").await;
    let outcome = harness
        .state
        .scheduler
        .discover()
        .await
        .map_err(|error| error.body());
    harness.state.index().lock().finish_refresh(outcome);
    let job = harness
        .json(
            Method::POST,
            "/api/jobs",
            &json!({"operation": "render", "scene": "MainScene", "file": "scenes/main.py", "profile": "draft"}),
        )
        .await;
    assert_eq!(job.status, StatusCode::ACCEPTED);
    let id = job.json()["id"].as_str().unwrap().to_owned();
    let finished = harness.finished(&id).await;
    assert_eq!(finished["status"], "succeeded", "{finished}");
    assert_eq!(finished["artifacts"][0]["kind"], "video");

    let state = harness.get("/api/state").await.json();
    let video = &state["latest"]["scenes/main.py#MainScene"]["video"];
    assert_eq!(video["job_id"], id.as_str());
    assert_eq!(video["profile"], "draft");
    assert_eq!(video["outdated"], false);
    assert_eq!(
        video["timeline"],
        json!([{"kind": "beat", "name": "hook", "start_seconds": 0.0, "end_seconds": 1.0,
                "file": "scenes/main.py", "line": 3}])
    );
    let url = video["artifact"]["url"].as_str().unwrap();
    let streamed = harness.get(url).await;
    assert_eq!(streamed.status, StatusCode::OK);
    assert_eq!(streamed.header(header::CONTENT_TYPE), "video/mp4");

    fs::write(
        harness.root.join("scenes/main.py"),
        "class MainScene(Scene):\n    pass  # edited\n",
    )
    .unwrap();
    let state = harness.get("/api/state").await.json();
    assert_eq!(
        state["latest"]["scenes/main.py#MainScene"]["video"]["outdated"],
        true
    );
    assert_eq!(state["jobs"][0]["id"], id.as_str());
}

#[tokio::test]
async fn a_bound_server_answers_real_sockets_on_its_tokenized_url() {
    let harness = Harness::new("").await;
    // 127.0.0.2 is loopback, but no Host it could send would be accepted.
    for ip in [[0, 0, 0, 0], [127, 0, 0, 2]] {
        let remote = ServeConfig {
            address: SocketAddr::from((ip, 0)),
            workbench_dir: None,
            allow_remote: false,
        };
        let refused = Server::bind(remote, harness.state.scheduler.clone()).await;
        assert!(refused
            .err()
            .unwrap()
            .to_string()
            .contains("--allow-remote"));
    }

    let local = ServeConfig {
        address: SocketAddr::from(([127, 0, 0, 1], 0)),
        workbench_dir: None,
        allow_remote: false,
    };
    let server = Server::bind(local, harness.state.scheduler.clone())
        .await
        .unwrap();
    let address = server.address();
    let url = server.url();
    let token = url
        .strip_prefix(&format!("http://{address}/?token="))
        .unwrap()
        .to_owned();
    assert_eq!(token.len(), 43);
    let taken = ServeConfig {
        address,
        workbench_dir: None,
        allow_remote: false,
    };
    let refused = Server::bind(taken, harness.state.scheduler.clone()).await;
    assert_eq!(
        refused.err().unwrap().to_string(),
        format!(
            "port {} is in use, probably by a running engine: open the workbench link it printed, or pass --port",
            address.port()
        )
    );
    let serving = tokio::spawn(server.run());
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET /api/health HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"api_version\":2"));
    serving.abort();
}

#[tokio::test]
async fn only_a_fresh_passing_check_is_not_repeated_at_start() {
    let harness = Harness::new("").await;
    let doctors = || async {
        harness.get("/api/jobs").await.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|job| job["operation"] == "doctor")
            .map(|job| job["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    // The identity the stub reports, so its `ready` frames leave `changed_at` alone.
    let store = harness.state.scheduler.store().clone();
    store
        .record_runtime(&RuntimeIdentity {
            python: STUB.into(),
            runtime_version: "2.0.0-stub".into(),
            manim: None,
            catalog: Catalog {
                themes: vec![CatalogTheme {
                    name: "midnight".into(),
                    tokens: vec![("background".into(), "#0B1020".into())],
                }],
                project_templates: vec!["explainer".into()],
                scene_templates: vec![],
            },
        })
        .unwrap();
    watch::doctor(harness.state.clone()).await;
    let first = doctors().await;
    assert_eq!(first.len(), 1, "no report exists yet");
    assert_eq!(harness.finished(&first[0]).await["origin"], "engine");
    watch::doctor(harness.state.clone()).await;
    assert_eq!(doctors().await, first);

    // A check that failed since is never fresh: the runtime may work again.
    let failed = uuid::Uuid::new_v4();
    let (request, task) = (
        OperationRequest::Doctor(DoctorParams {}),
        Task::Doctor(DoctorTask {}),
    );
    store
        .insert_job(&testing::new_job(failed, &request, &task))
        .unwrap();
    let unavailable = ErrorBody::new("runtime_unavailable", "Python was not found.", None);
    store
        .finish_error(failed, JobStatus::Failed, &unavailable, None)
        .unwrap();
    watch::doctor(harness.state.clone()).await;
    let after = doctors().await;
    assert_eq!(after.len(), 3);
    assert_eq!(after[1..], [failed.to_string(), first[0].clone()]);
}
