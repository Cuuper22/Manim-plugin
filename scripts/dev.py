#!/usr/bin/env python3
"""Run the workbench and Rust API together for local development."""

from __future__ import annotations

from pathlib import Path
import os
import signal
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    env = os.environ.copy()
    env.setdefault("RUST_LOG", "manim_director=info")
    api = subprocess.Popen(
        [
            "cargo",
            "run",
            "-p",
            "manim-director-cli",
            "--bin",
            "manim-director",
            "--",
            "serve",
            "--port",
            "4177",
        ],
        cwd=ROOT,
        env=env,
    )
    ui = subprocess.Popen(["npm", "run", "dev"], cwd=ROOT / "workbench", env=env)

    def stop(*_: object) -> None:
        for child in (ui, api):
            if child.poll() is None:
                child.terminate()

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        code = ui.wait()
        if code:
            raise SystemExit(code)
    finally:
        stop()
        api.wait(timeout=10)


if __name__ == "__main__":
    main()
