//! The Python bridge (OPS §2). Every request runs in its own runtime process,
//! which exits after answering. Long-lived engines keep one preloaded worker
//! idle so Manim's import cost leaves a job's critical path; direct
//! operations spawn a light worker that imports nothing heavy.

mod failure;
mod lines;
mod prewarm;
mod spawn;
mod worker;

pub use prewarm::{PrewarmPolicy, SpawnKey};
pub(crate) use worker::{ReadyWorker, Undelivered};

use manim_director_core::{
    BridgeRequest, EngineError, ErrorBody, LogFrame, ProgressFrame, ReadyFrame, Task,
    PROTOCOL_VERSION,
};
use prewarm::Prewarm;
use serde_json::Value;
use spawn::Launcher;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

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

/// The request line, newline included, within the 4 MiB limit.
pub fn request_line(
    request_id: &str,
    project_root: &Path,
    task: &Task,
) -> Result<Vec<u8>, EngineError> {
    let mut line = serde_json::to_vec(&BridgeRequest {
        protocol: PROTOCOL_VERSION,
        request_id,
        method: task.operation(),
        project_root,
        params: task,
    })
    .map_err(EngineError::internal)?;
    line.push(b'\n');
    if line.len() > MAX_REQUEST_BYTES {
        return Err(EngineError::RequestTooLarge {
            limit_bytes: MAX_REQUEST_BYTES as u64,
            actual_bytes: Some(line.len() as u64),
        });
    }
    Ok(line)
}

/// Receives every `ready` frame any worker of a bridge sends.
pub type ReadySink = Arc<dyn Fn(&ReadyFrame) + Send + Sync>;

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

pub struct RuntimeBridge {
    launcher: Launcher,
    prewarm: Option<Prewarm>,
}

impl RuntimeBridge {
    pub fn new(config: BridgeConfig) -> Self {
        Self::with_ready_sink(config, Arc::new(|_| {}))
    }

    pub fn with_ready_sink(config: BridgeConfig, on_ready: ReadySink) -> Self {
        Self {
            launcher: Launcher { config, on_ready },
            prewarm: None,
        }
    }

    pub fn config(&self) -> &BridgeConfig {
        &self.launcher.config
    }

    /// From now on keeps one preloaded worker idle in `root`, spawned with
    /// `key` until a job brings another.
    pub fn start_prewarm(&mut self, root: &Path, key: SpawnKey, policy: PrewarmPolicy) {
        self.prewarm = Some(Prewarm::start(
            self.launcher.clone(),
            root.to_path_buf(),
            key,
            policy,
        ));
    }

    /// Retires the idle worker; later jobs spawn their own.
    pub async fn close(&self) {
        if let Some(prewarm) = &self.prewarm {
            prewarm.close().await;
        }
    }

    /// Runs one direct operation in a fresh worker that preloads nothing.
    pub async fn run_direct(
        &self,
        root: &Path,
        task: &Task,
        cancel: &CancellationToken,
        on_event: &mut (impl FnMut(BridgeEvent<'_>) + Send),
    ) -> BridgeOutcome {
        let request_id = Uuid::new_v4().to_string();
        let line = match request_line(&request_id, root, task) {
            Ok(line) => line,
            Err(error) => return BridgeOutcome::Failed(error.body()),
        };
        let worker = match self.launcher.spawn(root, false, None) {
            Ok(worker) => worker,
            Err(error) => return BridgeOutcome::Failed(error),
        };
        let worker = match worker.into_ready(cancel, on_event).await {
            Ok(worker) => worker,
            Err(outcome) => return outcome,
        };
        worker
            .converse(&line, &request_id, cancel, on_event)
            .await
            .unwrap_or_else(|Undelivered(error)| BridgeOutcome::Failed(error))
    }

    /// A preloaded worker past `ready` for a job: the idle one when its
    /// spawn key matches, else a fresh one.
    pub(crate) async fn acquire(
        &self,
        root: &Path,
        key: &SpawnKey,
        cancel: &CancellationToken,
        on_event: &mut (impl FnMut(BridgeEvent<'_>) + Send),
    ) -> Result<ReadyWorker, BridgeOutcome> {
        let prewarmed = match &self.prewarm {
            Some(prewarm) => prewarm.take(key).await,
            None => None,
        };
        let worker = match prewarmed {
            Some(worker) => worker,
            None => self
                .launcher
                .spawn(root, true, key.memory_mb)
                .map_err(BridgeOutcome::Failed)?,
        };
        worker.into_ready(cancel, on_event).await
    }
}

#[cfg(test)]
mod tests;
