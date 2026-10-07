//! Runs one bridge v2 request in a fresh runtime process (OPS §2): spawn,
//! wait for `ready`, write the single request line, stream frames until the
//! terminal frame, then let the process exit.

use manim_director_core::{
    BridgeRequest, ErrorBody, LogFrame, ProgressFrame, ReadyFrame, RuntimeFrame, Task,
    PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::{Child, Command},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_STDERR_LINE_BYTES: usize = 64 * 1024;
const STDERR_TAIL_BYTES: usize = 64 * 1024;
const MAX_NOISE_LINES: usize = 64;
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const EXIT_GRACE: Duration = Duration::from_secs(5);
const KILL_GRACE: Duration = Duration::from_secs(2);

const RUNTIME_ENV: [(&str, &str); 5] = [
    ("PYTHONUNBUFFERED", "1"),
    ("PYTHONIOENCODING", "utf-8"),
    ("PYTHONSAFEPATH", "1"),
    ("NO_COLOR", "1"),
    ("MPLBACKEND", "Agg"),
];

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub python: PathBuf,
    pub module: String,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            python: python_interpreter(),
            module: std::env::var("MANIM_DIRECTOR_RUNTIME_MODULE")
                .unwrap_or_else(|_| "manim_director_runtime".into()),
        }
    }
}

/// The single interpreter lookup: `MANIM_DIRECTOR_PYTHON`, then the venv an
/// install places beside the binary, then `python3`.
pub fn python_interpreter() -> PathBuf {
    if let Some(configured) = std::env::var_os("MANIM_DIRECTOR_PYTHON") {
        if !configured.is_empty() {
            return configured.into();
        }
    }
    if let Some(prefix) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
    {
        let venv = if cfg!(windows) {
            "share/manim-director/venv/Scripts/python.exe"
        } else {
            "share/manim-director/venv/bin/python"
        };
        let candidate = prefix.join(venv);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
}

/// The request line without its trailing newline.
pub fn encode_request(
    request_id: &str,
    project_root: &Path,
    task: &Task,
) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&BridgeRequest {
        protocol: PROTOCOL_VERSION,
        request_id,
        method: task.operation(),
        project_root,
        params: task,
    })
}

pub struct Invocation<'a> {
    pub request_id: &'a str,
    /// Canonical project root; also the process cwd.
    pub project_root: &'a Path,
    pub task: &'a Task,
    pub preload: bool,
    pub memory_mb: Option<u64>,
}

#[derive(Debug)]
pub enum BridgeEvent<'a> {
    Ready(&'a ReadyFrame),
    Progress(ProgressFrame),
    Log(LogFrame),
    Stderr(String),
}

#[derive(Debug)]
pub enum BridgeOutcome {
    Succeeded(Value),
    Failed(ErrorBody),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct RuntimeBridge {
    config: BridgeConfig,
}

impl RuntimeBridge {
    pub fn new(config: BridgeConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &BridgeConfig {
        &self.config
    }

    pub async fn run(
        &self,
        invocation: Invocation<'_>,
        cancel: CancellationToken,
        mut on_event: impl FnMut(BridgeEvent<'_>) + Send,
    ) -> BridgeOutcome {
        let request = match encode_request(
            invocation.request_id,
            invocation.project_root,
            invocation.task,
        ) {
            Ok(mut line) if line.len() <= MAX_REQUEST_BYTES => {
                line.push(b'\n');
                line
            }
            Ok(line) => {
                return BridgeOutcome::Failed(ErrorBody::new(
                    "request_too_large",
                    "The runtime request exceeds 4 MiB.",
                    Some(json!({"limit_bytes": MAX_REQUEST_BYTES, "actual_bytes": line.len()})),
                ))
            }
            Err(error) => return BridgeOutcome::Failed(ErrorBody::internal(error.to_string())),
        };
        let mut child = match self.spawn(&invocation) {
            Ok(child) => child,
            Err(error) => {
                return BridgeOutcome::Failed(runtime_unavailable(
                    &self.config.python,
                    &format!("could not start it: {error}"),
                    "",
                ))
            }
        };
        let mut session = Session {
            stdout: LineReader::new(child.stdout.take(), MAX_FRAME_BYTES),
            stderr: LineReader::new(child.stderr.take(), MAX_STDERR_LINE_BYTES),
            stdin: child.stdin.take(),
            tail: StderrTail::default(),
            request_id: invocation.request_id,
            python: &self.config.python,
        };
        let outcome = session
            .converse(&mut child, request, &cancel, &mut on_event)
            .await;
        match outcome {
            Ended::Terminal(outcome) => {
                session.drain_until_exit(&mut child, &mut on_event).await;
                outcome
            }
            Ended::Closed => {
                let status = session.drain_until_exit(&mut child, &mut on_event).await;
                BridgeOutcome::Failed(runtime_crashed(status, &session.tail.text))
            }
            Ended::Abort(outcome) => {
                terminate_process_tree(&mut child).await;
                outcome
            }
        }
    }

    fn spawn(&self, invocation: &Invocation<'_>) -> std::io::Result<Child> {
        let mut command = Command::new(&self.config.python);
        command
            .args(["-P", "-m", &self.config.module, "bridge"])
            .args(invocation.preload.then_some("--preload"))
            .current_dir(invocation.project_root)
            .envs(RUNTIME_ENV)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            command.process_group(0);
            if let Some(megabytes) = invocation.memory_mb.filter(|megabytes| *megabytes > 0) {
                let bytes = megabytes.saturating_mul(1024 * 1024) as libc::rlim_t;
                // SAFETY: setrlimit is async-signal-safe and touches only the child.
                unsafe {
                    command.pre_exec(move || {
                        let limit = libc::rlimit {
                            rlim_cur: bytes,
                            rlim_max: bytes,
                        };
                        if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
        }
        #[cfg(windows)]
        {
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        command.spawn()
    }
}

enum Ended {
    /// A result or error frame arrived; the process should now exit by itself.
    Terminal(BridgeOutcome),
    /// stdout closed without a terminal frame.
    Closed,
    /// Cancelled, timed out waiting for `ready`, or broke the protocol.
    Abort(BridgeOutcome),
}

struct Session<'a> {
    stdout: LineReader,
    stderr: LineReader,
    stdin: Option<tokio::process::ChildStdin>,
    tail: StderrTail,
    request_id: &'a str,
    python: &'a Path,
}

impl Session<'_> {
    async fn converse(
        &mut self,
        child: &mut Child,
        request: Vec<u8>,
        cancel: &CancellationToken,
        on_event: &mut (impl FnMut(BridgeEvent<'_>) + Send),
    ) -> Ended {
        let ready_deadline = Instant::now() + READY_TIMEOUT;
        let mut ready = false;
        let mut noise_lines = 0;
        let mut request = Some(request);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return Ended::Abort(BridgeOutcome::Cancelled),
                _ = tokio::time::sleep_until(ready_deadline), if !ready => {
                    return Ended::Abort(BridgeOutcome::Failed(runtime_unavailable(
                        self.python,
                        "no ready frame within 120 s",
                        &self.tail.text,
                    )));
                }
                line = self.stderr.next(), if self.stderr.open => {
                    if let Some(line) = line {
                        let text = String::from_utf8_lossy(line.bytes()).into_owned();
                        self.tail.push(&text);
                        on_event(BridgeEvent::Stderr(text));
                    }
                }
                line = self.stdout.next(), if self.stdout.open => {
                    let line = match line {
                        None => {
                            if ready {
                                return Ended::Closed;
                            }
                            let status = child.wait().await.ok();
                            return Ended::Abort(BridgeOutcome::Failed(runtime_unavailable(
                                self.python,
                                &format!("exited before ready ({})", describe_exit(status)),
                                &self.tail.text,
                            )));
                        }
                        Some(Line::Overflow(_)) => {
                            return self.protocol_error("a frame exceeds 1 MiB");
                        }
                        Some(Line::Complete(bytes)) => bytes,
                    };
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    if !ready {
                        match serde_json::from_slice::<Value>(&line) {
                            Err(_) => {
                                if noise_lines < MAX_NOISE_LINES {
                                    noise_lines += 1;
                                    on_event(BridgeEvent::Stderr(String::from_utf8_lossy(&line).into_owned()));
                                }
                                continue;
                            }
                            Ok(value) => match serde_json::from_value::<RuntimeFrame>(value) {
                                Ok(RuntimeFrame::Ready(frame)) if frame.protocol == PROTOCOL_VERSION => {
                                    on_event(BridgeEvent::Ready(&frame));
                                    ready = true;
                                    if let (Some(stdin), Some(request)) = (self.stdin.take(), request.take()) {
                                        tokio::spawn(write_request(stdin, request));
                                    }
                                }
                                Ok(RuntimeFrame::Ready(frame)) => {
                                    return Ended::Abort(BridgeOutcome::Failed(protocol_mismatch(
                                        frame.protocol,
                                        &self.tail.text,
                                    )));
                                }
                                _ => return self.protocol_error("the first frame was not a valid ready frame"),
                            },
                        }
                        continue;
                    }
                    match self.frame(&line) {
                        Ok(Some(event)) => on_event(event),
                        Ok(None) => {}
                        Err(ended) => return ended,
                    }
                }
            }
        }
    }

    /// Interprets one post-`ready` frame: an event to forward, nothing, or
    /// the end of the conversation.
    fn frame(&self, line: &[u8]) -> Result<Option<BridgeEvent<'static>>, Ended> {
        let frame: RuntimeFrame = serde_json::from_slice(line).map_err(|error| {
            self.protocol_error(&format!(
                "malformed frame ({error}): {}",
                truncate(&String::from_utf8_lossy(line), 240)
            ))
        })?;
        match (&frame, frame.request_id()) {
            (RuntimeFrame::Ready(_), _) => {
                return Err(self.protocol_error("a second ready frame arrived"))
            }
            (RuntimeFrame::Error(error), None) => {
                return Err(self.protocol_error(&format!(
                    "the runtime rejected the request: {}",
                    error.error.message
                )))
            }
            (_, Some(id)) if id != self.request_id => {
                return Err(self.protocol_error(&format!(
                    "request_id {id:?} does not match {:?}",
                    self.request_id
                )))
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

    fn protocol_error(&self, detail: &str) -> Ended {
        Ended::Abort(BridgeOutcome::Failed(ErrorBody::new(
            "runtime_protocol",
            format!("The runtime broke the bridge protocol: {detail}."),
            Some(json!({"detail": detail, "stderr_tail": self.tail.text})),
        )))
    }

    /// Keeps draining both pipes until the process exits, killing its group
    /// after the grace period. stdout after a terminal frame is only logged.
    async fn drain_until_exit(
        &mut self,
        child: &mut Child,
        on_event: &mut (impl FnMut(BridgeEvent<'_>) + Send),
    ) -> Option<ExitStatus> {
        let deadline = Instant::now() + EXIT_GRACE;
        loop {
            if !self.stdout.open && !self.stderr.open {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    terminate_process_tree(child).await;
                    return child.wait().await.ok();
                }
                line = self.stderr.next(), if self.stderr.open => {
                    if let Some(line) = line {
                        let text = String::from_utf8_lossy(line.bytes()).into_owned();
                        self.tail.push(&text);
                        on_event(BridgeEvent::Stderr(text));
                    }
                }
                line = self.stdout.next(), if self.stdout.open => {
                    if let Some(line) = line {
                        on_event(BridgeEvent::Stderr(String::from_utf8_lossy(line.bytes()).into_owned()));
                    }
                }
            }
        }
        match tokio::time::timeout_at(deadline, child.wait()).await {
            Ok(status) => status.ok(),
            Err(_) => {
                terminate_process_tree(child).await;
                child.wait().await.ok()
            }
        }
    }
}

async fn write_request(mut stdin: tokio::process::ChildStdin, request: Vec<u8>) {
    // A failed write means the worker died; the reader then sees stdout close
    // and reports the crash with the stderr tail.
    if stdin.write_all(&request).await.is_ok() {
        let _ = stdin.shutdown().await;
    }
}

enum Line {
    Complete(Vec<u8>),
    /// `max` bytes without a newline; the rest of the line follows.
    Overflow(Vec<u8>),
}

impl Line {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Complete(bytes) | Self::Overflow(bytes) => bytes,
        }
    }
}

/// A cancel-safe bounded line reader: a partially read line survives a
/// dropped `next()` future because it lives in `partial`.
struct LineReader {
    reader: Option<BufReader<Box<dyn AsyncRead + Send + Unpin>>>,
    partial: Vec<u8>,
    max: usize,
    open: bool,
}

impl LineReader {
    fn new(pipe: Option<impl AsyncRead + Send + Unpin + 'static>, max: usize) -> Self {
        let reader =
            pipe.map(|pipe| BufReader::new(Box::new(pipe) as Box<dyn AsyncRead + Send + Unpin>));
        Self {
            open: reader.is_some(),
            reader,
            partial: Vec::new(),
            max,
        }
    }

    /// The next line without its terminator, or `None` at end of stream.
    async fn next(&mut self) -> Option<Line> {
        let reader = self.reader.as_mut()?;
        loop {
            let buffer = match reader.fill_buf().await {
                Ok(buffer) => buffer,
                Err(_) => &[],
            };
            if buffer.is_empty() {
                self.open = false;
                return (!self.partial.is_empty())
                    .then(|| Line::Complete(std::mem::take(&mut self.partial)));
            }
            let room = self.max - self.partial.len();
            match buffer.iter().position(|byte| *byte == b'\n') {
                Some(index) if index <= room => {
                    self.partial.extend_from_slice(&buffer[..index]);
                    reader.consume(index + 1);
                    let mut line = std::mem::take(&mut self.partial);
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    return Some(Line::Complete(line));
                }
                _ if buffer.len() > room => {
                    self.partial.extend_from_slice(&buffer[..room]);
                    reader.consume(room);
                    return Some(Line::Overflow(std::mem::take(&mut self.partial)));
                }
                _ => {
                    let count = buffer.len();
                    self.partial.extend_from_slice(buffer);
                    reader.consume(count);
                }
            }
        }
    }
}

#[derive(Default)]
struct StderrTail {
    text: String,
}

impl StderrTail {
    fn push(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
        if self.text.len() > STDERR_TAIL_BYTES {
            let mut cut = self.text.len() - STDERR_TAIL_BYTES;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
        }
    }
}

fn runtime_unavailable(python: &Path, detail: &str, stderr_tail: &str) -> ErrorBody {
    ErrorBody::new(
        "runtime_unavailable",
        format!(
            "The Python runtime at {} is unavailable: {detail}.",
            python.display()
        ),
        Some(json!({"python": python, "stderr_tail": stderr_tail})),
    )
}

fn protocol_mismatch(got: u32, stderr_tail: &str) -> ErrorBody {
    ErrorBody::new(
        "runtime_protocol",
        format!("The runtime speaks bridge protocol {got}; this engine needs {PROTOCOL_VERSION}."),
        Some(json!({
            "detail": "protocol mismatch",
            "expected": PROTOCOL_VERSION,
            "got": got,
            "hint": format!("reinstall the runtime matching engine {}", env!("CARGO_PKG_VERSION")),
            "stderr_tail": stderr_tail,
        })),
    )
}

fn runtime_crashed(status: Option<ExitStatus>, stderr_tail: &str) -> ErrorBody {
    let exit_code = status.and_then(|status| status.code());
    #[cfg(unix)]
    let signal = status.and_then(|status| std::os::unix::process::ExitStatusExt::signal(&status));
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    ErrorBody::new(
        "runtime_crashed",
        format!(
            "The runtime exited without a result ({}).",
            describe_exit(status)
        ),
        Some(json!({"exit_code": exit_code, "signal": signal, "stderr_tail": stderr_tail})),
    )
}

fn describe_exit(status: Option<ExitStatus>) -> String {
    status.map_or_else(|| "exit status unknown".into(), |status| status.to_string())
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

/// SIGTERM to the process group, SIGKILL after the grace period.
async fn terminate_process_tree(child: &mut Child) {
    let Some(pid) = child.id() else {
        return;
    };
    #[cfg(unix)]
    {
        // SAFETY: signalling our own child's process group.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        if tokio::time::timeout(KILL_GRACE, child.wait())
            .await
            .is_err()
        {
            // SAFETY: as above.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            let _ = child.kill().await;
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        let _ = child.kill().await;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn line_reader_splits_overlong_lines_without_losing_bytes() {
        let source: &'static [u8] = b"abcdefghij\nxy\nlast";
        let mut reader = LineReader::new(Some(source), 4);
        let mut seen = Vec::new();
        while let Some(line) = reader.next().await {
            let tag = match line {
                Line::Complete(_) => "line",
                Line::Overflow(_) => "chunk",
            };
            seen.push((tag, String::from_utf8(line.bytes().to_vec()).unwrap()));
        }
        assert_eq!(
            seen,
            [
                ("chunk", "abcd".to_owned()),
                ("chunk", "efgh".to_owned()),
                ("line", "ij".to_owned()),
                ("line", "xy".to_owned()),
                ("line", "last".to_owned()),
            ]
        );
    }

    #[test]
    fn stderr_tail_keeps_the_last_64_kib() {
        let mut tail = StderrTail::default();
        for index in 0..2_000 {
            tail.push(&format!("{index:05} {}", "x".repeat(60)));
        }
        assert!(tail.text.len() <= STDERR_TAIL_BYTES);
        assert!(tail.text.ends_with(&format!("01999 {}\n", "x".repeat(60))));
    }
}
