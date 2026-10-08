//! The SSE event stream (HTTP §8): one sequenced ring of events per engine
//! process. Each stream reads the ring from its own cursor, so replay after a
//! reconnect and lag detection are the same mechanism, and no event is ever
//! dropped without a `resync`.

use super::{
    error::{ApiError, ApiQuery, ApiResult},
    state::AppState,
};
use crate::workspace::{JobView, Sections};
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap},
    response::Response,
};
use manim_director_core::Progress;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const RING_CAPACITY: usize = 4096;
const MAX_STREAMS: usize = 32;
const HEARTBEAT: Duration = Duration::from_secs(15);
/// Frames buffered per stream before it is treated as slow.
const STREAM_BUFFER: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Job {
        job: Box<JobView>,
    },
    Progress {
        job_id: Uuid,
        progress: Progress,
    },
    Workspace {
        sections: Box<Sections>,
    },
    File {
        path: String,
        revision: Option<String>,
    },
    Resync {
        reason: ResyncReason,
    },
}

impl ServerEvent {
    fn name(&self) -> &'static str {
        match self {
            Self::Job { .. } => "job",
            Self::Progress { .. } => "progress",
            Self::Workspace { .. } => "workspace",
            Self::File { .. } => "file",
            Self::Resync { .. } => "resync",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResyncReason {
    UnknownCursor,
    Expired,
    Lagged,
}

/// One encoded SSE frame; every stream sends the same bytes.
#[derive(Clone)]
struct Frame {
    seq: u64,
    bytes: Bytes,
}

struct Ring {
    frames: VecDeque<Frame>,
    last: u64,
}

pub struct EventHub {
    instance: Uuid,
    capacity: usize,
    ring: Mutex<Ring>,
    published: watch::Sender<u64>,
    streams: Arc<Semaphore>,
}

impl EventHub {
    pub fn new(instance: Uuid, capacity: usize) -> Self {
        Self {
            instance,
            capacity,
            ring: Mutex::new(Ring {
                frames: VecDeque::with_capacity(capacity),
                last: 0,
            }),
            published: watch::channel(0).0,
            streams: Arc::new(Semaphore::new(MAX_STREAMS)),
        }
    }

    /// Callers publish only after the change is committed and visible.
    pub fn publish(&self, event: &ServerEvent) {
        let data = match serde_json::to_string(event) {
            Ok(data) => data,
            Err(error) => {
                tracing::error!(%error, "could not encode an event");
                return;
            }
        };
        let seq = {
            let mut ring = self.ring.lock();
            ring.last += 1;
            let seq = ring.last;
            let bytes = encode(&self.id(seq), event.name(), &data);
            if ring.frames.len() == self.capacity {
                ring.frames.pop_front();
            }
            ring.frames.push_back(Frame { seq, bytes });
            seq
        };
        self.published.send_replace(seq);
    }

    /// The id of the newest event (`<instance>.0` before any).
    pub fn cursor(&self) -> String {
        self.id(self.ring.lock().last)
    }

    fn id(&self, seq: u64) -> String {
        format!("{}.{seq}", self.instance)
    }

    /// Where a stream starts: replay after an honourable cursor, else a
    /// `resync` and live events only.
    fn start(&self, cursor: Option<&str>) -> (u64, Option<ResyncReason>) {
        let ring = self.ring.lock();
        let live = ring.last + 1;
        let Some(cursor) = cursor else {
            return (live, None);
        };
        let seq = cursor
            .split_once('.')
            .filter(|(instance, _)| instance.parse::<Uuid>().ok() == Some(self.instance))
            .and_then(|(_, seq)| seq.parse::<u64>().ok())
            .filter(|seq| *seq <= ring.last);
        match seq {
            None => (live, Some(ResyncReason::UnknownCursor)),
            Some(seq) if seq == ring.last => (live, None),
            Some(seq)
                if ring
                    .frames
                    .front()
                    .is_some_and(|front| front.seq <= seq + 1) =>
            {
                (seq + 1, None)
            }
            Some(_) => (live, Some(ResyncReason::Expired)),
        }
    }

    /// Frames from `next` on; `None` when some of them already left the ring.
    fn read(&self, next: u64) -> Option<Vec<Frame>> {
        let ring = self.ring.lock();
        match ring.frames.front() {
            Some(front) if front.seq > next => None,
            Some(front) => Some(
                ring.frames
                    .iter()
                    .skip((next - front.seq) as usize)
                    .cloned()
                    .collect(),
            ),
            None => Some(Vec::new()),
        }
    }

    fn resync(&self, reason: ResyncReason) -> (Bytes, u64) {
        let last = self.ring.lock().last;
        let event = ServerEvent::Resync { reason };
        let data = serde_json::to_string(&event).expect("a resync encodes");
        (encode(&self.id(last), event.name(), &data), last + 1)
    }
}

fn encode(id: &str, name: &str, data: &str) -> Bytes {
    Bytes::from(format!("id: {id}\nevent: {name}\ndata: {data}\n\n"))
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    after: Option<String>,
}

/// `GET /api/events`: `Last-Event-ID` wins over `?after=`.
pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiQuery(query): ApiQuery<EventsQuery>,
) -> ApiResult<Response> {
    let hub = state.hub.clone();
    let permit = hub
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::too_many_streams(MAX_STREAMS))?;
    let cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or(query.after);
    let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
    tokio::spawn(pump(hub, cursor, sender, permit, state.closing.clone()));
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(ReceiverStream::new(receiver)))
        .expect("static headers are valid"))
}

type Sender = mpsc::Sender<Result<Bytes, Infallible>>;

/// Feeds one stream until its client goes away.
async fn pump(
    hub: Arc<EventHub>,
    cursor: Option<String>,
    sender: Sender,
    _permit: OwnedSemaphorePermit,
    closing: CancellationToken,
) {
    let mut published = hub.published.subscribe();
    let (mut next, resync) = hub.start(cursor.as_deref());
    if sender
        .send(Ok(Bytes::from_static(b"retry: 2000\n\n")))
        .await
        .is_err()
    {
        return;
    }
    if let Some(reason) = resync {
        let (frame, after) = hub.resync(reason);
        next = after;
        if sender.send(Ok(frame)).await.is_err() {
            return;
        }
    }
    loop {
        published.mark_unchanged();
        let Some(frames) = hub.read(next) else {
            let (frame, after) = hub.resync(ResyncReason::Lagged);
            next = after;
            if sender.send(Ok(frame)).await.is_err() {
                return;
            }
            continue;
        };
        for frame in frames {
            next = frame.seq + 1;
            if sender.send(Ok(frame.bytes)).await.is_err() {
                return;
            }
        }
        tokio::select! {
            changed = published.changed() => if changed.is_err() { return },
            _ = tokio::time::sleep(HEARTBEAT) => {
                if sender.send(Ok(Bytes::from_static(b": ping\n\n"))).await.is_err() {
                    return;
                }
            }
            _ = sender.closed() => return,
            _ = closing.cancelled() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(n: u64) -> ServerEvent {
        ServerEvent::File {
            path: format!("scenes/{n}.py"),
            revision: None,
        }
    }

    fn seqs(frames: &[Frame]) -> Vec<u64> {
        frames.iter().map(|frame| frame.seq).collect()
    }

    #[test]
    fn cursors_replay_inside_the_ring_and_resync_outside_it() {
        let instance = Uuid::new_v4();
        let hub = EventHub::new(instance, 4);
        assert_eq!(hub.cursor(), format!("{instance}.0"));
        assert_eq!(hub.start(Some(&format!("{instance}.0"))), (1, None));
        for n in 1..=6 {
            hub.publish(&file(n));
        }
        assert_eq!(hub.cursor(), format!("{instance}.6"));
        assert_eq!(hub.start(None), (7, None));
        assert_eq!(hub.start(Some(&format!("{instance}.3"))), (4, None));
        assert_eq!(seqs(&hub.read(4).unwrap()), [4, 5, 6]);
        assert_eq!(hub.start(Some(&format!("{instance}.6"))), (7, None));
        assert_eq!(
            hub.start(Some(&format!("{instance}.1"))),
            (7, Some(ResyncReason::Expired))
        );
        for cursor in [
            format!("{}.3", Uuid::new_v4()),
            format!("{instance}.99"),
            "garbage".to_owned(),
        ] {
            assert_eq!(
                hub.start(Some(&cursor)),
                (7, Some(ResyncReason::UnknownCursor)),
                "{cursor}"
            );
        }
        assert!(hub.read(2).is_none(), "seq 2 left the ring");
    }

    #[test]
    fn frames_carry_instance_ids_names_and_one_line_of_json() {
        let instance = Uuid::new_v4();
        let hub = EventHub::new(instance, 8);
        hub.publish(&ServerEvent::File {
            path: "a\nb.py".into(),
            revision: Some("r".into()),
        });
        let frame = &hub.read(1).unwrap()[0];
        assert_eq!(
            std::str::from_utf8(&frame.bytes).unwrap(),
            format!(
                "id: {instance}.1\nevent: file\ndata: {{\"type\":\"file\",\"path\":\"a\\nb.py\",\"revision\":\"r\"}}\n\n"
            )
        );
    }

    #[tokio::test]
    async fn a_slow_stream_gets_a_lagged_resync_instead_of_a_gap() {
        let hub = Arc::new(EventHub::new(Uuid::new_v4(), 4));
        let (sender, mut receiver) = mpsc::channel(1);
        let permit = hub.streams.clone().try_acquire_owned().unwrap();
        tokio::spawn(pump(
            hub.clone(),
            None,
            sender,
            permit,
            CancellationToken::new(),
        ));
        let text = |bytes: Bytes| String::from_utf8(bytes.to_vec()).unwrap();
        assert_eq!(
            text(receiver.recv().await.unwrap().unwrap()),
            "retry: 2000\n\n"
        );
        hub.publish(&file(1));
        assert!(text(receiver.recv().await.unwrap().unwrap()).contains("event: file"));
        // The stream holds at most one frame while ten more are published.
        for n in 2..=11 {
            hub.publish(&file(n));
        }
        let mut received = Vec::new();
        while let Ok(Some(Ok(frame))) =
            tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await
        {
            received.push(text(frame));
        }
        let resync = received
            .iter()
            .position(|frame| frame.contains("\"reason\":\"lagged\""))
            .expect("a lagged resync");
        assert!(received[resync].contains(&format!(".{}\n", 11)));
        assert!(received[resync + 1..]
            .iter()
            .all(|frame| !frame.contains("event: file")));
    }
}
