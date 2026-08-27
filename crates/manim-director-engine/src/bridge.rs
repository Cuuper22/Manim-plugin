use anyhow::{anyhow, bail, Context, Result};
use manim_director_core::{BridgeMessage, BridgeRequest, Operation, ProtocolError};
use serde_json::{json, Map, Value};
use std::{fs, path::Path, process::Stdio};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_BRIDGE_REQUEST_BYTES: usize = 4 * 1024 * 1024;
const MAX_BRIDGE_STDOUT_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_BRIDGE_STDERR_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub python: String,
    pub module: String,
    pub memory_mb: u64,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            python: default_python(),
            module: std::env::var("MANIM_DIRECTOR_RUNTIME_MODULE")
                .unwrap_or_else(|_| "manim_director_runtime".into()),
            memory_mb: std::env::var("MANIM_DIRECTOR_MEMORY_MB")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(8192)
                .clamp(128, 262_144),
        }
    }
}

fn default_python() -> String {
    if let Ok(configured) = std::env::var("MANIM_DIRECTOR_PYTHON") {
        if !configured.trim().is_empty() {
            return configured;
        }
    }
    // scripts/install.py puts the binary in <prefix>/bin and the isolated
    // runtime in <prefix>/share/manim-director/venv. Discovering it relative
    // to the executable keeps Windows and virtual-environment installs pinned
    // without a shell wrapper.
    if let Ok(executable) = std::env::current_exe() {
        if let Some(prefix) = executable.parent().and_then(Path::parent) {
            let candidate = if cfg!(windows) {
                prefix.join("share/manim-director/venv/Scripts/python.exe")
            } else {
                prefix.join("share/manim-director/venv/bin/python")
            };
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }
    if cfg!(windows) {
        "python".into()
    } else {
        "python3".into()
    }
}

#[derive(Debug)]
pub enum BridgeOutcome {
    Result(Value),
    Error(ProtocolError),
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

    pub async fn execute<F>(
        &self,
        request_id: &str,
        operation: Operation,
        project_root: &Path,
        params: Value,
        cancellation: CancellationToken,
        mut on_event: F,
    ) -> Result<BridgeOutcome>
    where
        F: FnMut(&str, Value) + Send,
    {
        let params = inject_project_root(params, project_root);
        let request = BridgeRequest {
            request_id: request_id.to_owned(),
            method: operation.runtime_method().to_owned(),
            params,
        };
        let mut command = Command::new(&self.config.python);
        command
            .args(["-m", &self.config.module, "bridge"])
            .current_dir(project_root)
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            command.process_group(0);
            let memory_bytes = self.config.memory_mb.saturating_mul(1024 * 1024) as libc::rlim_t;
            unsafe {
                command.pre_exec(move || {
                    let limit = libc::rlimit {
                        rlim_cur: memory_bytes,
                        rlim_max: memory_bytes,
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        command.creation_flags(0x0000_0200);
        let mut child = command.spawn().with_context(|| {
            format!(
                "starting {} -m {} bridge",
                self.config.python, self.config.module
            )
        })?;

        let mut stdin = child
            .stdin
            .take()
            .context("runtime bridge stdin unavailable")?;
        let encoded = serde_json::to_vec(&request)?;
        if encoded.len() > MAX_BRIDGE_REQUEST_BYTES {
            terminate_process_tree(&mut child).await;
            bail!(
                "runtime request is {} bytes; maximum is {} bytes",
                encoded.len(),
                MAX_BRIDGE_REQUEST_BYTES
            );
        }
        stdin.write_all(&encoded).await?;
        stdin.write_all(b"\n").await?;
        stdin.shutdown().await?;
        drop(stdin);

        let mut stdout = BufReader::new(
            child
                .stdout
                .take()
                .context("runtime bridge stdout unavailable")?,
        );
        let mut stderr = BufReader::new(
            child
                .stderr
                .take()
                .context("runtime bridge stderr unavailable")?,
        );
        let mut terminal: Option<BridgeOutcome> = None;
        let mut stdout_open = true;
        let mut stderr_open = true;

        while terminal.is_none() && (stdout_open || stderr_open) {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    terminate_process_tree(&mut child).await;
                    terminal = Some(BridgeOutcome::Cancelled);
                }
                line = read_bounded_line(&mut stdout, MAX_BRIDGE_STDOUT_LINE_BYTES), if stdout_open => {
                    let line = match line {
                        Ok(line) => line,
                        Err(error) => {
                            terminate_process_tree(&mut child).await;
                            return Err(error.context("reading bounded runtime stdout"));
                        }
                    };
                    match line {
                        Some(line) if !line.trim().is_empty() => {
                            let message: BridgeMessage = match serde_json::from_str(&line)
                                .with_context(|| format!("invalid JSONL from runtime: {}", truncate(&line, 240))) {
                                    Ok(message) => message,
                                    Err(error) => {
                                        terminate_process_tree(&mut child).await;
                                        return Err(error);
                                    }
                                };
                            if message.id() != request_id {
                                terminate_process_tree(&mut child).await;
                                return Err(anyhow!("runtime response id {} did not match {request_id}", message.id()));
                            }
                            match message {
                                BridgeMessage::Event { event, data, .. } => on_event(&event, data),
                                BridgeMessage::Success { result, .. } => terminal = Some(BridgeOutcome::Result(result)),
                                BridgeMessage::Failure { error, .. } => terminal = Some(BridgeOutcome::Error(error)),
                            }
                        }
                        Some(_) => {}
                        None => stdout_open = false,
                    }
                }
                line = read_bounded_line(&mut stderr, MAX_BRIDGE_STDERR_LINE_BYTES), if stderr_open => {
                    let line = match line {
                        Ok(line) => line,
                        Err(error) => {
                            terminate_process_tree(&mut child).await;
                            return Err(error.context("reading bounded runtime stderr"));
                        }
                    };
                    match line {
                        Some(line) if !line.trim().is_empty() => on_event("runtime_stderr", json!({"message": truncate(&line, 2000)})),
                        Some(_) => {}
                        None => stderr_open = false,
                    }
                }
            }
        }

        if matches!(terminal, Some(BridgeOutcome::Cancelled)) {
            let _ = child.wait().await;
            return Ok(BridgeOutcome::Cancelled);
        }
        let status = child.wait().await?;
        match terminal {
            Some(BridgeOutcome::Result(value)) if status.success() => {
                Ok(BridgeOutcome::Result(value))
            }
            Some(BridgeOutcome::Result(_)) => Err(anyhow!(
                "runtime returned a result but exited with {status}"
            )),
            Some(BridgeOutcome::Error(error)) => Ok(BridgeOutcome::Error(error)),
            Some(BridgeOutcome::Cancelled) => Ok(BridgeOutcome::Cancelled),
            None => Err(anyhow!(
                "runtime exited with {status} without a terminal JSONL message"
            )),
        }
    }
}

/// Prepare scaffold parameters without allowing an occupied directory to be
/// mutated accidentally. Git metadata and the scheduler's own empty state
/// database are infrastructure rather than project content, so those entries
/// select the runtime's non-destructive merge mode automatically.
pub fn scaffold_params(
    project_root: &Path,
    name: &str,
    force: bool,
    seed: Option<u32>,
) -> Result<Value> {
    fs::create_dir_all(project_root)
        .with_context(|| format!("creating scaffold destination {}", project_root.display()))?;
    if !project_root.is_dir() {
        bail!(
            "scaffold destination is not a directory: {}",
            project_root.display()
        );
    }
    let mut metadata_only = false;
    let mut occupied = Vec::new();
    for entry in fs::read_dir(project_root)
        .with_context(|| format!("reading scaffold destination {}", project_root.display()))?
    {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        match name.as_ref() {
            ".git" => metadata_only = true,
            ".manim-director" if scheduler_state_only(&entry.path())? => metadata_only = true,
            _ => occupied.push(name.into_owned()),
        }
    }
    if !occupied.is_empty() && !force {
        occupied.sort();
        let count = occupied.len();
        occupied.truncate(20);
        bail!(
            "scaffold destination is not empty ({} project entries: {}); pass --force explicitly",
            count,
            occupied.join(", ")
        );
    }
    if name.trim().is_empty() {
        bail!("project name cannot be empty");
    }
    if seed.is_some_and(|value| value > 0x7fff_ffff) {
        bail!("project seed must be between 0 and 2147483647");
    }
    let mut params = Map::new();
    params.insert("name".into(), Value::String(name.to_owned()));
    params.insert("force".into(), Value::Bool(force));
    if metadata_only && !force {
        params.insert("merge".into(), Value::Bool(true));
    }
    if let Some(seed) = seed {
        params.insert("seed".into(), Value::from(seed));
    }
    Ok(Value::Object(params))
}

/// Run the canonical Python scaffold directly. This deliberately avoids
/// opening the per-project SQLite store before the runtime's empty-directory
/// preflight has completed.
pub async fn scaffold_project(project_root: &Path, params: Value) -> Result<Value> {
    let root = project_root.canonicalize().with_context(|| {
        format!(
            "canonicalizing scaffold destination {}",
            project_root.display()
        )
    })?;
    let request_id = format!("scaffold-{}", Uuid::new_v4());
    match RuntimeBridge::new(BridgeConfig::default())
        .execute(
            &request_id,
            Operation::Scaffold,
            &root,
            params,
            CancellationToken::new(),
            |_event, _data| {},
        )
        .await?
    {
        BridgeOutcome::Result(result) => Ok(result),
        BridgeOutcome::Error(error) => {
            let detail = error
                .data
                .map(|value| format!(" ({value})"))
                .unwrap_or_default();
            bail!("{}: {}{}", error.code, error.message, detail)
        }
        BridgeOutcome::Cancelled => bail!("scaffold was cancelled"),
    }
}

fn scheduler_state_only(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !matches!(name.as_ref(), "state.db" | "state.db-wal" | "state.db-shm") {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn read_bounded_line<R>(reader: &mut R, max_bytes: usize) -> Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::with_capacity(max_bytes.min(8 * 1024));
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let (take, complete) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(index) => (index + 1, true),
            None => (buffer.len(), false),
        };
        if bytes.len().saturating_add(take) > max_bytes {
            bail!("JSONL line exceeds {max_bytes} bytes");
        }
        bytes.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if complete {
            break;
        }
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    Ok(Some(
        String::from_utf8(bytes).context("runtime output was not UTF-8")?,
    ))
}

async fn terminate_process_tree(child: &mut tokio::process::Child) {
    let Some(pid) = child.id() else {
        return;
    };
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        if tokio::time::timeout(std::time::Duration::from_secs(2), child.wait())
            .await
            .is_err()
        {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
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
        let _ = child.wait().await;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

fn inject_project_root(params: Value, root: &Path) -> Value {
    let mut object = match params {
        Value::Object(object) => object,
        Value::Null => Map::new(),
        other => {
            let mut object = Map::new();
            object.insert("input".into(), other);
            object
        }
    };
    object.insert(
        "project_root".into(),
        Value::String(root.to_string_lossy().into_owned()),
    );
    Value::Object(object)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scaffold_preflight_rejects_project_content_but_allows_engine_metadata() {
        let occupied = tempdir().unwrap();
        fs::write(occupied.path().join("notes.txt"), "mine").unwrap();
        let error = scaffold_params(occupied.path(), "Demo", false, None).unwrap_err();
        assert!(error.to_string().contains("not empty"));

        let metadata = tempdir().unwrap();
        fs::create_dir(metadata.path().join(".git")).unwrap();
        fs::create_dir(metadata.path().join(".manim-director")).unwrap();
        fs::write(metadata.path().join(".manim-director/state.db"), b"").unwrap();
        let params = scaffold_params(metadata.path(), "Demo", false, Some(73)).unwrap();
        assert_eq!(params["merge"], true);
        assert_eq!(params["seed"], 73);
    }

    #[tokio::test]
    async fn bounded_line_reader_rejects_oversize_input() {
        let source = vec![b'x'; 33];
        let mut reader = BufReader::new(source.as_slice());
        assert!(read_bounded_line(&mut reader, 32).await.is_err());
    }
}
