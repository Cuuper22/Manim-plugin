//! `serve` on a port that is already taken says what to do instead.

use std::{fs, net::TcpListener, process::Command};

const STUB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../manim-director-engine/tests/fixtures/stub_runtime.py"
);

#[test]
fn a_taken_port_points_at_the_engine_holding_it_or_another_port() {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join("director.yaml"),
        "version: 1\nproject:\n  name: Demo\n",
    )
    .unwrap();
    let taken = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let output = Command::new(env!("CARGO_BIN_EXE_manim-director"))
        .arg("--project")
        .arg(project.path())
        .args(["serve", "--port", &port.to_string()])
        .env("MANIM_DIRECTOR_PYTHON", STUB)
        .env("MANIM_DIRECTOR_PREWARM", "0")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = format!(
        "error: port {port} is in use, probably by a running engine: open the workbench link it printed, or pass --port\n"
    );
    assert!(stderr.ends_with(&expected), "{stderr}");
}
