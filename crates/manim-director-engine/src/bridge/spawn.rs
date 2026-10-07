//! Starting a runtime process in its own process group, and ending the whole
//! group (OPS §2.1, §2.7): the runtime never kills its own children.

use super::{failure::runtime_unavailable, worker::Worker, BridgeConfig, ReadySink};
use manim_director_core::ErrorBody;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::process::{Child, Command};

const KILL_GRACE: Duration = Duration::from_secs(2);

const RUNTIME_ENV: [(&str, &str); 5] = [
    ("PYTHONUNBUFFERED", "1"),
    ("PYTHONIOENCODING", "utf-8"),
    ("PYTHONSAFEPATH", "1"),
    ("NO_COLOR", "1"),
    ("MPLBACKEND", "Agg"),
];

/// Spawns workers for one interpreter and runtime module, reporting every
/// `ready` frame they send.
#[derive(Clone)]
pub(super) struct Launcher {
    pub config: BridgeConfig,
    pub on_ready: ReadySink,
}

impl Launcher {
    /// `cwd` is the canonical project root. `memory_mb` sets `RLIMIT_AS`,
    /// which is opt-in because it breaks numpy, Cairo and OpenGL.
    pub(super) fn spawn(
        &self,
        root: &Path,
        preload: bool,
        memory_mb: Option<u64>,
    ) -> Result<Worker, ErrorBody> {
        let python = &self.config.python;
        let mut command = Command::new(python);
        command
            .args(["-P", "-m", &self.config.module, "bridge"])
            .args(preload.then_some("--preload"))
            .current_dir(root)
            .envs(RUNTIME_ENV)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            command.process_group(0);
            if let Some(megabytes) = memory_mb.filter(|megabytes| *megabytes > 0) {
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
        #[cfg(not(unix))]
        let _ = memory_mb;
        #[cfg(windows)]
        {
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
        }
        let child = command.spawn().map_err(|error| {
            runtime_unavailable(python, &format!("could not start it: {error}"), "")
        })?;
        Ok(Worker::new(child, python.clone(), self.on_ready.clone()))
    }
}

/// SIGTERM to the process group, SIGKILL after the grace period.
pub(super) async fn terminate_process_tree(child: &mut Child) {
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

/// Closing stdin ends an idle worker (EOF before a request exits 0); the
/// group is killed if it lingers.
pub(super) async fn close_gracefully(child: &mut Child) {
    if tokio::time::timeout(KILL_GRACE, child.wait())
        .await
        .is_err()
    {
        terminate_process_tree(child).await;
    }
}
