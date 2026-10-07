//! What a signal stops: an `mcp` server shuts down cleanly, and an
//! interrupted command cancels only a job it started.
#![cfg(unix)]

use manim_director_core::JobStatus;
use manim_director_engine::{state_db_path, Store};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

const STUB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../manim-director-engine/tests/fixtures/stub_runtime.py"
);

fn until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(10), "{what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn project() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("scenes")).unwrap();
    fs::write(
        directory.path().join("director.yaml"),
        "version: 1\nproject:\n  name: Demo\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("scenes/main.py"),
        "class SlowScene(Scene):\n    pass\n",
    )
    .unwrap();
    directory
}

fn engine(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_manim-director"));
    command
        .arg("--project")
        .arg(root)
        .args(args)
        .env("MANIM_DIRECTOR_PYTHON", STUB)
        .env("MANIM_DIRECTOR_PREWARM", "0")
        .stderr(Stdio::null());
    command
}

/// Starts `mcp` and submits one job through it without waiting.
fn mcp_job(root: &Path, tool: &str, arguments: Value) -> (Child, Uuid) {
    let mut mcp = engine(root, &["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": arguments}});
    writeln!(mcp.stdin.as_mut().unwrap(), "{call}").unwrap();
    let mut line = String::new();
    BufReader::new(mcp.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let answer: Value = serde_json::from_str(&line).unwrap();
    let id = answer["result"]["structuredContent"]["job"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    (mcp, id)
}

fn signal(child: &Child, name: &str) {
    let sent = Command::new("kill")
        .arg(format!("-{name}"))
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(sent.success());
}

#[test]
fn sigterm_cancels_the_servers_jobs() {
    let directory = project();
    let root = directory.path().canonicalize().unwrap();
    let arguments = json!({"operation": "diagnose", "text": "sleep", "wait_seconds": 0});
    let (mut mcp, id) = mcp_job(&root, "submit", arguments);
    let store = Store::open(state_db_path(&root)).unwrap();
    let status = || store.get_job(id).unwrap().unwrap().status;
    until("the job starts", || status() == JobStatus::Running);

    signal(&mcp, "TERM");
    until("the server exits", || mcp.try_wait().unwrap().is_some());
    let job = store.get_job(id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Cancelled);
    assert_eq!(job.error.unwrap().data.unwrap()["by"], "shutdown");
}

#[cfg(target_os = "linux")]
#[test]
fn interrupting_a_command_that_joined_another_clients_job_leaves_it_running() {
    let directory = project();
    let root = directory.path().canonicalize().unwrap();
    let (mut mcp, id) = mcp_job(
        &root,
        "render",
        json!({"scene": "SlowScene", "wait_seconds": 0}),
    );
    let store = Store::open(state_db_path(&root)).unwrap();
    let job = || store.get_job(id).unwrap().unwrap();
    // Its start fingerprint means the runtime identity is known to others.
    until("the render starts", || job().fingerprint.is_some());

    let mut cli = engine(&root, &["render", "--scene", "SlowScene"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let status = format!("/proc/{}/status", cli.id());
    let catches_sigint = || {
        let status = fs::read_to_string(&status).unwrap_or_default();
        let caught = status.lines().find_map(|line| line.strip_prefix("SigCgt:"));
        caught.is_some_and(|mask| u64::from_str_radix(mask.trim(), 16).unwrap() & 2 != 0)
    };
    until("the command waits", catches_sigint);
    signal(&cli, "INT");
    until("the command exits", || cli.try_wait().unwrap().is_some());
    assert_eq!(cli.wait().unwrap().code(), Some(130));
    assert_eq!(job().status, JobStatus::Running);
    assert!(!job().cancel_requested);

    drop(mcp.stdin.take());
    mcp.wait().unwrap();
}
