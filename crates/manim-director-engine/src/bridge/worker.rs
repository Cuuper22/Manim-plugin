//! One runtime process speaking bridge v2 (OPS §2.2–§2.4): read until
//! `ready`, handed exactly one request line, then drained until it exits.
//! Every read is cancel-safe, so an idle worker can wait inside a `select!`
//! and still be handed to a job halfway through its startup.

use super::{
    failure::{
        describe_exit, protocol_mismatch, runtime_crashed, runtime_protocol, runtime_unavailable,
        truncate,
    },
    lines::{Line, LineReader, StderrTail},
    spawn::{close_gracefully, terminate_process_tree},
    BridgeEvent, BridgeOutcome, ReadySink,
};
use manim_director_core::{ErrorBody, ReadyFrame, RuntimeFrame, PROTOCOL_VERSION};
use serde_json::Value;
use std::{collections::VecDeque, path::PathBuf, process::ExitStatus, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, ChildStdin},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_STDERR_LINE_BYTES: usize = 64 * 1024;
const MAX_NOISE_LINES: usize = 64;
/// Output kept for the job that takes an idle worker; the oldest goes first.
const MAX_PENDING_BYTES: usize = 2 * 1024 * 1024;
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const EXIT_GRACE: Duration = Duration::from_secs(5);

pub(crate) struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: LineReader,
    stderr: LineReader,
    tail: StderrTail,
    python: PathBuf,
    on_ready: ReadySink,
    ready: Option<ReadyFrame>,
    noise_lines: usize,
    spawned_at: Instant,
    /// Lines not yet handed to a job's log.
    pending: Pending,
}

pub(super) enum Step {
    /// A line was read; nothing was decided.
    Continue,
    /// The worker can no longer serve a request and must be discarded.
    Failed(ErrorBody),
}

/// How a conversation stopped before its outcome was settled.
enum Ended {
    /// A result or error frame arrived; the process exits by itself.
    Terminal(BridgeOutcome),
    /// stdout closed without a terminal frame.
    Closed,
    /// Cancelled, or the runtime broke the protocol.
    Abort(BridgeOutcome),
}

impl Worker {
    pub(super) fn new(mut child: Child, python: PathBuf, on_ready: ReadySink) -> Self {
        Self {
            stdin: child.stdin.take(),
            stdout: LineReader::new(child.stdout.take(), MAX_FRAME_BYTES),
            stderr: LineReader::new(child.stderr.take(), MAX_STDERR_LINE_BYTES),
            child,
            tail: StderrTail::default(),
            python,
            on_ready,
            ready: None,
            noise_lines: 0,
            spawned_at: Instant::now(),
            pending: Pending::default(),
        }
    }

    pub(super) fn spawned_at(&self) -> Instant {
        self.spawned_at
    }

    /// Still running with its protocol pipe open.
    pub(super) fn is_alive(&mut self) -> bool {
        self.stdout.open && matches!(self.child.try_wait(), Ok(None))
    }

    /// Reads one line from either pipe and acts on it.
    pub(super) async fn step(&mut self) -> Step {
        if !self.stdout.open {
            return Step::Failed(self.exited().await);
        }
        let ready_deadline = self.spawned_at + READY_TIMEOUT;
        let starting = self.ready.is_none();
        tokio::select! {
            line = self.stderr.next(), if self.stderr.open => {
                if let Some(line) = line {
                    self.stderr_line(line.bytes());
                }
                Step::Continue
            }
            line = self.stdout.next() => match line {
                None => Step::Failed(self.exited().await),
                Some(Line::Overflow(_)) => Step::Failed(self.protocol_error("a frame exceeds 1 MiB")),
                Some(Line::Complete(bytes)) => self.startup_line(&bytes),
            },
            _ = tokio::time::sleep_until(ready_deadline), if starting => Step::Failed(
                runtime_unavailable(&self.python, "no ready frame within 120 s", &self.tail.text),
            ),
        }
    }

    /// A stdout line before the request: noise or the `ready` frame.
    fn startup_line(&mut self, line: &[u8]) -> Step {
        if line.iter().all(u8::is_ascii_whitespace) {
            return Step::Continue;
        }
        if self.ready.is_some() {
            return Step::Failed(
                self.protocol_error("it wrote a frame before receiving a request"),
            );
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            if self.noise_lines < MAX_NOISE_LINES {
                self.noise_lines += 1;
                self.pending
                    .push(String::from_utf8_lossy(line).into_owned());
            }
            return Step::Continue;
        };
        match serde_json::from_value::<RuntimeFrame>(value) {
            Ok(RuntimeFrame::Ready(frame)) if frame.protocol == PROTOCOL_VERSION => {
                (self.on_ready)(&frame);
                self.ready = Some(frame);
                Step::Continue
            }
            Ok(RuntimeFrame::Ready(frame)) => {
                Step::Failed(protocol_mismatch(frame.protocol, &self.tail.text))
            }
            _ => Step::Failed(self.protocol_error("the first frame was not a valid ready frame")),
        }
    }

    /// Waits for `ready`, handing everything the worker printed so far to
    /// `on_event`.
    pub(super) async fn into_ready(
        mut self,
        cancel: &CancellationToken,
        on_event: &mut (impl FnMut(BridgeEvent<'_>) + Send),
    ) -> Result<ReadyWorker, BridgeOutcome> {
        loop {
            self.pending.flush(on_event);
            if let Some(frame) = self.ready.clone() {
                on_event(BridgeEvent::Ready(&frame));
                return Ok(ReadyWorker {
                    frame,
                    worker: self,
                });
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    self.discard().await;
                    return Err(BridgeOutcome::Cancelled);
                }
                step = self.step() => if let Step::Failed(error) = step {
                    terminate_process_tree(&mut self.child).await;
                    self.pending.flush(on_event);
                    return Err(BridgeOutcome::Failed(error));
                },
            }
        }
    }

    fn stderr_line(&mut self, line: &[u8]) {
        let text = String::from_utf8_lossy(line).into_owned();
        self.tail.push(&text);
        self.pending.push(text);
    }

    /// Interprets one frame after the request: an event to forward, nothing,
    /// or the end of the conversation.
    fn frame(&self, line: &[u8], request_id: &str) -> Result<Option<BridgeEvent<'static>>, Ended> {
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        let frame: RuntimeFrame = serde_json::from_slice(line).map_err(|error| {
            self.abort_protocol(&format!(
                "malformed frame ({error}): {}",
                truncate(&String::from_utf8_lossy(line), 240)
            ))
        })?;
        match (&frame, frame.request_id()) {
            (RuntimeFrame::Ready(_), _) => {
                return Err(self.abort_protocol("a second ready frame arrived"))
            }
            (RuntimeFrame::Error(error), None) => {
                return Err(self.abort_protocol(&format!(
                    "the runtime rejected the request: {}",
                    error.error.message
                )))
            }
            (_, Some(id)) if id != request_id => {
                return Err(self
                    .abort_protocol(&format!("request_id {id:?} does not match {request_id:?}")))
            }
            _ => {}
        }
        Ok(match frame {
            RuntimeFrame::Progress(progress) => Some(BridgeEvent::Progress(progress)),
            RuntimeFrame::Log(log) => Some(BridgeEvent::Log(log)),
            RuntimeFrame::Result(result) => {
                return Err(Ended::Terminal(BridgeOutcome::Succeeded(result.result)))
            }
            RuntimeFrame::Error(error) => {
                return Err(Ended::Terminal(BridgeOutcome::Failed(
                    ErrorBody::from_runtime(error.error),
                )))
            }
            RuntimeFrame::Ready(_) => None,
        })
    }

    fn protocol_error(&self, detail: &str) -> ErrorBody {
        runtime_protocol(detail, &self.tail.text)
    }

    fn abort_protocol(&self, detail: &str) -> Ended {
        Ended::Abort(BridgeOutcome::Failed(self.protocol_error(detail)))
    }

    /// The failure of a worker whose stdout closed: it never got ready, or
    /// it died before answering.
    async fn exited(&mut self) -> ErrorBody {
        let status = self.drain_until_exit().await;
        match self.ready {
            None => runtime_unavailable(
                &self.python,
                &format!("exited before ready ({})", describe_exit(status)),
                &self.tail.text,
            ),
            Some(_) => runtime_crashed(status, &self.tail.text),
        }
    }

    /// Keeps draining both pipes until the process exits, killing its group
    /// after the grace period. stdout after a terminal frame is only logged.
    async fn drain_until_exit(&mut self) -> Option<ExitStatus> {
        let deadline = Instant::now() + EXIT_GRACE;
        while self.stdout.open || self.stderr.open {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    terminate_process_tree(&mut self.child).await;
                    return self.child.wait().await.ok();
                }
                line = self.stderr.next(), if self.stderr.open => {
                    if let Some(line) = line {
                        self.stderr_line(line.bytes());
                    }
                }
                line = self.stdout.next(), if self.stdout.open => {
                    if let Some(line) = line {
                        self.pending.push(String::from_utf8_lossy(line.bytes()).into_owned());
                    }
                }
            }
        }
        match tokio::time::timeout_at(deadline, self.child.wait()).await {
            Ok(status) => status.ok(),
            Err(_) => {
                terminate_process_tree(&mut self.child).await;
                self.child.wait().await.ok()
            }
        }
    }

    /// Kills the worker's process group.
    pub(super) async fn discard(mut self) {
        terminate_process_tree(&mut self.child).await;
    }

    /// Lets an idle worker go by closing its stdin.
    pub(super) async fn retire(mut self) {
        drop(self.stdin.take());
        close_gracefully(&mut self.child).await;
    }
}

/// A worker past `ready`, about to receive its one request.
pub(crate) struct ReadyWorker {
    frame: ReadyFrame,
    worker: Worker,
}

/// The request never reached the runtime: the worker died while idle.
pub(crate) struct Undelivered(pub ErrorBody);

impl ReadyWorker {
    pub(crate) fn frame(&self) -> &ReadyFrame {
        &self.frame
    }

    /// Writes the request line and closes stdin, streams frames until the
    /// terminal one, then drains the process until it exits.
    pub(crate) async fn converse(
        self,
        request: &[u8],
        request_id: &str,
        cancel: &CancellationToken,
        on_event: &mut (impl FnMut(BridgeEvent<'_>) + Send),
    ) -> Result<BridgeOutcome, Undelivered> {
        let mut worker = self.worker;
        let Some(mut stdin) = worker.stdin.take() else {
            worker.discard().await;
            return Err(Undelivered(ErrorBody::internal(
                "the runtime worker has no stdin",
            )));
        };
        let write = async move {
            stdin.write_all(request).await?;
            stdin.shutdown().await
        };
        tokio::pin!(write);
        let mut writing = true;
        let ended = loop {
            worker.pending.flush(on_event);
            tokio::select! {
                _ = cancel.cancelled() => break Ended::Abort(BridgeOutcome::Cancelled),
                written = &mut write, if writing => {
                    writing = false;
                    if written.is_err() {
                        let status = worker.drain_until_exit().await;
                        worker.pending.flush(on_event);
                        return Err(Undelivered(runtime_crashed(status, &worker.tail.text)));
                    }
                }
                line = worker.stderr.next(), if worker.stderr.open => {
                    if let Some(line) = line {
                        worker.stderr_line(line.bytes());
                    }
                }
                line = worker.stdout.next(), if worker.stdout.open => match line {
                    None => break Ended::Closed,
                    Some(Line::Overflow(_)) => break worker.abort_protocol("a frame exceeds 1 MiB"),
                    Some(Line::Complete(bytes)) => match worker.frame(&bytes, request_id) {
                        Ok(Some(event)) => on_event(event),
                        Ok(None) => {}
                        Err(ended) => break ended,
                    },
                },
            }
        };
        let outcome = match ended {
            Ended::Terminal(outcome) => {
                worker.drain_until_exit().await;
                outcome
            }
            Ended::Closed => {
                let status = worker.drain_until_exit().await;
                BridgeOutcome::Failed(runtime_crashed(status, &worker.tail.text))
            }
            Ended::Abort(outcome) => {
                terminate_process_tree(&mut worker.child).await;
                outcome
            }
        };
        worker.pending.flush(on_event);
        Ok(outcome)
    }
}

#[derive(Default)]
struct Pending {
    lines: VecDeque<String>,
    bytes: usize,
}

impl Pending {
    fn push(&mut self, line: String) {
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.bytes > MAX_PENDING_BYTES {
            match self.lines.pop_front() {
                Some(dropped) => self.bytes -= dropped.len(),
                None => break,
            }
        }
    }

    fn flush(&mut self, on_event: &mut impl FnMut(BridgeEvent<'_>)) {
        self.bytes = 0;
        for line in self.lines.drain(..) {
            on_event(BridgeEvent::Stderr(line));
        }
    }
}
