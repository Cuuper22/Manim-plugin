from __future__ import annotations

import importlib.util
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

from manim_director_runtime.jsonio import jsonable
from manim_director_runtime.protocol import Context

SRC = Path(__file__).resolve().parents[1] / "src"

requires_manim = pytest.mark.skipif(
    importlib.util.find_spec("manim") is None, reason="manim is not installed"
)
requires_ffmpeg = pytest.mark.skipif(
    shutil.which("ffmpeg") is None or shutil.which("ffprobe") is None,
    reason="ffmpeg is not installed",
)
requires_latex = pytest.mark.skipif(
    shutil.which("latex") is None or shutil.which("dvisvgm") is None,
    reason="LaTeX is not installed",
)


class RecordingWriter:
    """Stands in for the protocol fd: keeps every frame a handler emits."""

    def __init__(self) -> None:
        self.frames: list[dict[str, Any]] = []

    def write(self, frame: dict[str, Any]) -> None:
        self.frames.append(json.loads(json.dumps(jsonable(frame))))

    def send(self, data: bytes) -> None:
        self.frames.append(json.loads(data))


@pytest.fixture
def project(tmp_path: Path) -> Path:
    root = tmp_path / "project"
    root.mkdir()
    return root.resolve()


@pytest.fixture
def writer() -> RecordingWriter:
    return RecordingWriter()


@pytest.fixture
def ctx(project: Path, writer: RecordingWriter) -> Context:
    return Context(project, "test-request", writer)  # type: ignore[arg-type]


def as_json(value: Any) -> Any:
    """A result exactly as the engine receives it."""

    return json.loads(json.dumps(jsonable(value)))


def run_bridge(
    project: Path, request: dict[str, Any] | bytes | None, *, preload: bool = False
) -> tuple[list[dict[str, Any]], subprocess.CompletedProcess[bytes]]:
    """Spawn a real worker the way the engine does and return its frames."""

    payload = (json.dumps(request) + "\n").encode() if isinstance(request, dict) else request or b""
    env = {**os.environ, "PYTHONPATH": str(SRC), "PYTHONUNBUFFERED": "1", "NO_COLOR": "1"}
    args = [sys.executable, "-P", "-m", "manim_director_runtime", "bridge"]
    completed = subprocess.run(
        [*args, *(["--preload"] if preload else [])],
        input=payload,
        capture_output=True,
        cwd=project,
        env=env,
        check=False,
        timeout=300,
    )
    frames = [json.loads(line) for line in completed.stdout.decode().splitlines()]
    return frames, completed


def request(method: str, project: Path, params: dict[str, Any], request_id: str = "req-1") -> dict:
    return {
        "protocol": 2,
        "request_id": request_id,
        "method": method,
        "project_root": str(project),
        "params": params,
    }


def make_video(path: Path, *, seconds: float = 2.0, size: str = "320x180", fps: int = 10) -> Path:
    """A small test-pattern video made with ffmpeg."""

    path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "ffmpeg",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            f"testsrc=duration={seconds}:size={size}:rate={fps}",
            "-pix_fmt",
            "yuv420p",
            str(path),
        ],
        check=True,
    )
    return path
