//! Turns a running job's bridge events into progress events and stored log
//! records. The bridge calls back synchronously, so writes go to a per-job
//! task that batches them (OPS §1.8: every 250 ms or 100 records, one
//! transaction) and persists progress at most twice a second.

use crate::{BridgeEvent, NewLog, Store};
use manim_director_core::{
    EngineEvent, LogLevel, LogStream, Progress, ProgressPhase, RuntimeLogLevel, Timestamp,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
    time::Instant,
};
use uuid::Uuid;

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const FLUSH_RECORDS: usize = 100;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);
const MAX_MESSAGE_CHARS: usize = 2000;

enum Write {
    Log(NewLog),
    Progress(Progress),
}

pub(super) struct Recorder {
    id: Uuid,
    events: broadcast::Sender<EngineEvent>,
    writes: mpsc::UnboundedSender<Write>,
    writer: JoinHandle<()>,
}

impl Recorder {
    pub(super) fn start(
        store: Arc<Store>,
        events: broadcast::Sender<EngineEvent>,
        id: Uuid,
    ) -> Self {
        let (writes, receiver) = mpsc::unbounded_channel();
        Self {
            id,
            events,
            writes,
            writer: tokio::spawn(write_batches(store, id, receiver)),
        }
    }

    pub(super) fn record(&self, event: BridgeEvent<'_>) {
        match event {
            BridgeEvent::Ready(ready) => {
                self.log(
                    LogStream::Engine,
                    LogLevel::Info,
                    &format!(
                        "Runtime {} ready (Python {}, Manim {}).",
                        ready.runtime_version,
                        ready.python,
                        ready.manim.as_deref().unwrap_or("not installed")
                    ),
                );
                for failure in &ready.preload_failed {
                    self.log(
                        LogStream::Engine,
                        LogLevel::Warning,
                        &format!("Preloading {} failed: {}", failure.module, failure.message),
                    );
                }
            }
            BridgeEvent::Progress(frame) => self.progress(Progress {
                phase: frame.phase,
                current: frame.current,
                total: frame.total,
                scene_seconds: frame.scene_seconds,
                message: frame.message,
                updated_at: Timestamp::now(),
            }),
            BridgeEvent::Log(frame) => {
                let level = match frame.level {
                    RuntimeLogLevel::Info => LogLevel::Info,
                    RuntimeLogLevel::Warning => LogLevel::Warning,
                };
                self.log(LogStream::Runtime, level, &frame.message);
            }
            BridgeEvent::Stderr(line) => self.log(LogStream::Stderr, LogLevel::Info, &line),
        }
    }

    /// A milestone of the engine's own (stream `engine`).
    pub(super) fn engine_log(&self, level: LogLevel, message: &str) {
        self.log(LogStream::Engine, level, message);
    }

    /// Progress the engine itself reports (`starting`, `validate`).
    pub(super) fn engine_phase(
        &self,
        phase: ProgressPhase,
        total: Option<u64>,
        message: Option<&str>,
    ) {
        self.progress(Progress {
            phase,
            current: 0,
            total,
            scene_seconds: None,
            message: message.map(str::to_owned),
            updated_at: Timestamp::now(),
        });
    }

    fn progress(&self, progress: Progress) {
        let _ = self.events.send(EngineEvent::JobProgress {
            job_id: self.id,
            progress: progress.clone(),
        });
        let _ = self.writes.send(Write::Progress(progress));
    }

    /// One record per ≤ 2000-character piece of `message`.
    fn log(&self, stream: LogStream, level: LogLevel, message: &str) {
        let chars: Vec<char> = message.chars().collect();
        for piece in chars.chunks(MAX_MESSAGE_CHARS) {
            let piece: String = piece.iter().collect();
            let _ = self
                .writes
                .send(Write::Log(NewLog::now(stream, level, piece)));
        }
    }

    /// Flushes every pending record; the job's terminal transition comes
    /// after, so readers of a finished job see its whole log.
    pub(super) async fn close(self) {
        drop(self.writes);
        if let Err(error) = self.writer.await {
            tracing::warn!(id = %self.id, %error, "the log writer stopped");
        }
    }
}

async fn write_batches(store: Arc<Store>, id: Uuid, mut receiver: mpsc::UnboundedReceiver<Write>) {
    let mut logs = Vec::new();
    let mut progress: Option<Progress> = None;
    let mut progress_saved: Option<Instant> = None;
    let mut due: Option<Instant> = None;
    loop {
        let flush_now = tokio::select! {
            write = receiver.recv() => match write {
                Some(Write::Log(record)) => {
                    logs.push(record);
                    due.get_or_insert_with(|| Instant::now() + FLUSH_INTERVAL);
                    logs.len() >= FLUSH_RECORDS
                }
                Some(Write::Progress(latest)) => {
                    progress = Some(latest);
                    due.get_or_insert_with(|| Instant::now() + FLUSH_INTERVAL);
                    false
                }
                None => {
                    // The job is ending and its progress becomes null.
                    flush(&store, id, std::mem::take(&mut logs), None).await;
                    return;
                }
            },
            _ = tokio::time::sleep_until(due.unwrap_or_else(Instant::now)), if due.is_some() => true,
        };
        if !flush_now {
            continue;
        }
        let save_progress =
            progress.is_some() && progress_saved.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL);
        let saved = save_progress.then(|| progress.take()).flatten();
        if saved.is_some() {
            progress_saved = Some(Instant::now());
        }
        due = progress.is_some().then(|| Instant::now() + FLUSH_INTERVAL);
        flush(&store, id, std::mem::take(&mut logs), saved).await;
    }
}

async fn flush(store: &Arc<Store>, id: Uuid, logs: Vec<NewLog>, progress: Option<Progress>) {
    if logs.is_empty() && progress.is_none() {
        return;
    }
    let written = store
        .blocking(move |store| {
            store.append_logs(id, &logs)?;
            if let Some(progress) = &progress {
                store.set_progress(id, progress)?;
            }
            Ok(())
        })
        .await;
    if let Err(error) = written {
        tracing::warn!(%id, %error, "could not store job logs or progress");
    }
}
