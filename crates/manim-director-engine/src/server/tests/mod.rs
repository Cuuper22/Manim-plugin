//! HTTP v2 through the real router, against the bridge v2 stub runtime in
//! `tests/fixtures/stub_runtime.py`.

mod api;
mod auth;
mod files;
mod live;

use super::{
    auth::Session,
    events::{EventHub, RING_CAPACITY},
    router,
    state::AppState,
    workbench::Workbench,
};
use crate::{BridgeConfig, EngineMode, PrunePolicy, Scheduler, SchedulerConfig};
use axum::{
    body::{Body, Bytes},
    http::{header, request::Builder, HeaderMap, Method, Request, StatusCode},
    Router,
};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use tokio_stream::StreamExt;
use tower::ServiceExt;

const STUB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/stub_runtime.py"
);
const PORT: u16 = 4177;
const HOST: &str = "127.0.0.1:4177";

pub(super) struct Harness {
    _directory: tempfile::TempDir,
    pub root: PathBuf,
    pub state: AppState,
    router: Router,
}

pub(super) struct Options {
    pub allow_remote: bool,
    pub ring: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            allow_remote: false,
            ring: RING_CAPACITY,
        }
    }
}

impl Harness {
    /// A project whose `director.yaml` is `version: 1`, `project.name: Demo`
    /// and then `spec`, with `scenes/main.py` defining `MainScene`.
    pub async fn new(spec: &str) -> Self {
        Self::with(spec, Options::default()).await
    }

    pub async fn with(spec: &str, options: Options) -> Self {
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
        let scheduler = Scheduler::open(
            &root,
            SchedulerConfig {
                mode: EngineMode::Cli,
                workers: 2,
                queue_capacity: 8,
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
        let hub = Arc::new(EventHub::new(scheduler.instance_id(), options.ring));
        let state = AppState::new(
            scheduler,
            Session::new(PORT).unwrap(),
            PORT,
            options.allow_remote,
            Workbench::Embedded,
            hub,
        );
        Self {
            _directory: directory,
            router: router(state.clone()),
            root,
            state,
        }
    }

    pub fn token(&self) -> &str {
        self.state.session.token()
    }

    /// A request with the loopback Host and the Bearer token.
    pub fn request(&self, method: Method, path: &str) -> Builder {
        Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token()))
    }

    pub async fn send(&self, request: Request<Body>) -> Reply {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        Reply {
            status,
            headers,
            body,
        }
    }

    pub async fn get(&self, path: &str) -> Reply {
        self.send(self.request(Method::GET, path).body(Body::empty()).unwrap())
            .await
    }

    pub async fn json(&self, method: Method, path: &str, body: &Value) -> Reply {
        let request = self
            .request(method, path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        self.send(request).await
    }

    /// An open `GET /api/events` stream.
    pub async fn events(&self, query: &str, last_event_id: Option<&str>) -> Events {
        let mut request = self.request(Method::GET, &format!("/api/events{query}"));
        if let Some(id) = last_event_id {
            request = request.header("last-event-id", id);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        Events {
            stream: Box::pin(response.into_body().into_data_stream()),
            buffer: String::new(),
        }
    }

    /// Waits for a job this test submitted to end.
    pub async fn finished(&self, id: &str) -> Value {
        for _ in 0..400 {
            let job = self.get(&format!("/api/jobs/{id}")).await.json();
            if matches!(
                job["status"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("job {id} never finished");
    }
}

pub(super) struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|error| panic!("{error}: {:?}", String::from_utf8_lossy(&self.body)))
    }

    /// The envelope's `error.code`, checked against the status.
    pub fn error(&self, status: StatusCode) -> String {
        assert_eq!(
            self.status,
            status,
            "{}",
            String::from_utf8_lossy(&self.body)
        );
        self.json()["error"]["code"].as_str().unwrap().to_owned()
    }

    pub fn header(&self, name: impl header::AsHeaderName) -> &str {
        self.headers.get(name).unwrap().to_str().unwrap()
    }
}

pub(super) struct Event {
    pub id: Option<String>,
    pub name: String,
    pub data: Value,
}

pub(super) struct Events {
    stream: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<Bytes, axum::Error>> + Send>>,
    buffer: String,
}

impl Events {
    /// The next event frame (comments and `retry:` skipped), or `None` when
    /// nothing arrives within `wait`.
    pub async fn next_within(&mut self, wait: Duration) -> Option<Event> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            while let Some(end) = self.buffer.find("\n\n") {
                let frame: String = self.buffer.drain(..end + 2).collect();
                if let Some(event) = parse_frame(&frame) {
                    return Some(event);
                }
            }
            let chunk = tokio::time::timeout_at(deadline, self.stream.next()).await;
            match chunk {
                Ok(Some(Ok(bytes))) => self.buffer.push_str(std::str::from_utf8(&bytes).unwrap()),
                _ => return None,
            }
        }
    }

    pub async fn next(&mut self) -> Event {
        self.next_within(Duration::from_secs(10))
            .await
            .expect("an event within 10 s")
    }

    /// Skips events until one satisfies `wanted`.
    pub async fn until(&mut self, mut wanted: impl FnMut(&Event) -> bool) -> Event {
        loop {
            let event = self.next().await;
            if wanted(&event) {
                return event;
            }
        }
    }
}

fn parse_frame(frame: &str) -> Option<Event> {
    let (mut id, mut name, mut data) = (None, None, None);
    for line in frame.lines() {
        match line.split_once(": ") {
            Some(("id", value)) => id = Some(value.to_owned()),
            Some(("event", value)) => name = Some(value.to_owned()),
            Some(("data", value)) => data = Some(serde_json::from_str(value).unwrap()),
            _ => {}
        }
    }
    Some(Event {
        id,
        name: name?,
        data: data?,
    })
}
