//! `manim-director mcp` stops cleanly on SIGTERM, as it does at end of input.
#![cfg(unix)]

use manim_director_core::JobStatus;
use manim_director_engine::{state_db_path, Store};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
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

#[test]
fn sigterm_cancels_the_servers_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    fs::write(
        root.join("director.yaml"),
        "version: 1\nproject:\n  name: Demo\n",
    )
    .unwrap();
    let mut mcp = Command::new(env!("CARGO_BIN_EXE_manim-director"))
        .arg("--project")
        .arg(&root)
        .arg("mcp")
        .env("MANIM_DIRECTOR_PYTHON", STUB)
        .env("MANIM_DIRECTOR_PREWARM", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "submit",
        "arguments": {"operation": "diagnose", "text": "sleep", "wait_seconds": 0}}});
    writeln!(mcp.stdin.as_mut().unwrap(), "{call}").unwrap();
    let mut line = String::new();
    BufReader::new(mcp.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let answer: Value = serde_json::from_str(&line).unwrap();
    let id: Uuid = answer["result"]["structuredContent"]["job"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let store = Store::open(state_db_path(&root)).unwrap();
    let status = || store.get_job(id).unwrap().unwrap().status;
    until("the job starts", || status() == JobStatus::Running);

    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(mcp.id().to_string())
        .status()
        .unwrap();
    assert!(killed.success());
    until("the server exits", || mcp.try_wait().unwrap().is_some());
    let job = store.get_job(id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Cancelled);
    assert_eq!(job.error.unwrap().data.unwrap()["by"], "shutdown");
}
