from __future__ import annotations

import contextlib
import json
import os
import select
import signal
import subprocess
import sys
from pathlib import Path

import pytest

from conftest import SRC, RecordingWriter, request, run_bridge
from manim_director_runtime import protocol
from manim_director_runtime.protocol import MAX_REQUEST_BYTES, METHODS, handle_request

CANONICAL = [
    "init",
    "discover",
    "doctor",
    "render",
    "still",
    "frame",
    "contact_sheet",
    "qa",
    "diagnose",
    "validate_math",
    "captions",
    "ingest",
    "export",
]


def test_method_table_has_exactly_the_canonical_operations() -> None:
    assert list(METHODS) == CANONICAL


def test_ready_frame_comes_first_and_idle_eof_exits_quietly(project: Path) -> None:
    frames, completed = run_bridge(project, None)
    assert completed.returncode == 0
    assert len(frames) == 1
    ready = frames[0]
    assert ready["type"] == "ready"
    assert ready["protocol"] == 2
    assert ready["preloaded"] == [] and ready["preload_failed"] == []
    themes = ready["catalog"]["themes"]
    assert themes[0]["name"] == "midnight"
    assert [token for token, _ in themes[0]["tokens"]] == [
        "background",
        "foreground",
        "primary",
        "secondary",
        "accent",
        "muted",
        "success",
    ]
    assert all(color.startswith("#") and color == color.upper() for _, color in themes[0]["tokens"])
    templates = ["explainer", "derivation", "geometry", "graph", "vertical_short"]
    assert ready["catalog"]["project_templates"] == ready["catalog"]["scene_templates"] == templates


def test_the_package_and_catalog_import_without_manim() -> None:
    code = (
        "import sys\n"
        "from manim_director_runtime import Region, Theme, Transition, protocol, catalog\n"
        "catalog.catalog()\n"
        "assert 'manim' not in sys.modules\n"
        "assert 'yaml' not in sys.modules  # doctor must run to report PyYAML missing\n"
    )
    env = {**os.environ, "PYTHONPATH": str(SRC)}
    subprocess.run([sys.executable, "-c", code], env=env, check=True)


def test_preload_reports_what_it_imported(project: Path) -> None:
    frames, completed = run_bridge(project, None, preload=True)
    assert completed.returncode == 0
    ready = frames[0]
    assert {"manim", "numpy", "PIL.Image"} <= set(ready["preloaded"]) | {
        failure["module"] for failure in ready["preload_failed"]
    }
    assert "manim_director_runtime.rendering" in ready["preloaded"]
    assert isinstance(ready["preload_ms"], int)


def test_serves_exactly_one_request(project: Path) -> None:
    first = json.dumps(request("diagnose", project, {"text": "NameError: name 'x' is not defined"}))
    second = json.dumps(request("diagnose", project, {"text": "other"}, request_id="req-2"))
    frames, completed = run_bridge(project, f"{first}\n{second}\n".encode())
    assert completed.returncode == 0
    assert [frame["type"] for frame in frames] == ["ready", "result"]
    assert frames[1]["request_id"] == "req-1"


@pytest.mark.parametrize(
    "line",
    [b'{"protocol":2,"request_id":\n', b"[1, 2]\n", b'{"protocol":2,"request_id":""}\n'],
)
def test_unidentifiable_requests_fail_with_a_null_request_id(project: Path, line: bytes) -> None:
    frames, completed = run_bridge(project, line)
    assert completed.returncode == 0
    error = frames[-1]
    assert (error["type"], error["request_id"]) == ("error", None)
    assert error["error"]["code"] == "invalid_request"
    assert error["error"]["data"]["detail"]


def test_oversize_request_is_rejected(project: Path) -> None:
    frames, _ = run_bridge(project, b"x" * (MAX_REQUEST_BYTES + 10))
    assert frames[-1]["error"]["code"] == "invalid_request"
    assert frames[-1]["request_id"] is None


def test_errors_echo_the_request_id(project: Path) -> None:
    cases = {
        "unknown_method": {**request("debug", project, {})},
        "invalid_request": {**request("doctor", project, {}), "protocol": 1},
        "invalid_params": request("diagnose", project, {"text": 3}),
    }
    for code, payload in cases.items():
        writer = RecordingWriter()
        handle_request(json.dumps(payload).encode(), writer)  # type: ignore[arg-type]
        (frame,) = writer.frames
        assert frame["type"] == "error"
        assert frame["request_id"] == "req-1"
        assert frame["error"]["code"] == code


def test_request_shape_is_strict(project: Path, writer: RecordingWriter) -> None:
    payload = {**request("doctor", project, {}), "id": "legacy"}
    handle_request(json.dumps(payload).encode(), writer)  # type: ignore[arg-type]
    assert writer.frames[0]["error"]["code"] == "invalid_request"
    assert "unknown field id" in writer.frames[0]["error"]["data"]["detail"]


def test_protocol_mismatch_names_the_expected_version(
    project: Path, writer: RecordingWriter
) -> None:
    handle_request(json.dumps({**request("doctor", project, {}), "protocol": 1}).encode(), writer)  # type: ignore[arg-type]
    assert writer.frames[0]["error"]["data"]["expected"] == 2


def test_project_root_must_be_an_existing_absolute_directory(
    project: Path, writer: RecordingWriter
) -> None:
    for root in ("relative/path", str(project / "missing")):
        handle_request(
            json.dumps({**request("doctor", project, {}), "project_root": root}).encode(), writer
        )  # type: ignore[arg-type]
    assert [frame["error"]["code"] for frame in writer.frames] == ["invalid_request"] * 2


def test_invalid_params_name_the_dotted_field(project: Path, writer: RecordingWriter) -> None:
    params = {
        "scene": None,
        "files": [str(project / "a.py")],
        "settings": {
            "profile": "p",
            "width": "wide",
            "height": 1,
            "fps": 1,
            "renderer": "cairo",
            "format": "mp4",
            "transparent": False,
        },
        "media_dir": str(project),
        "out_dir": str(project),
        "sections": False,
        "fresh": False,
    }
    handle_request(json.dumps(request("render", project, params)).encode(), writer)  # type: ignore[arg-type]
    error = writer.frames[0]["error"]
    assert error["code"] == "invalid_params"
    assert error["data"] == {"field": "settings.width", "reason": "must be an integer"}


# A stand-in engine: starts a worker (as its own group's leader when argv[4] is "True", as the
# engine does), waits for ready, starts a stand-in for ffmpeg in the worker's group, then dies.
# Each process holds the write end of a pipe the test watches, so its exit shows as EOF.
ORPHANING_ENGINE = """
import subprocess, sys
request, worker_alive, child_alive = map(int, sys.argv[1:4])
own_group = sys.argv[4] == "True"
bridge = [sys.executable, "-P", "-m", "manim_director_runtime", "bridge"]
worker = subprocess.Popen(
    bridge,
    stdin=request,
    stdout=subprocess.PIPE,
    pass_fds=[worker_alive],
    process_group=0 if own_group else None,
)
worker.stdout.readline()
group = worker.pid if own_group else None
subprocess.Popen(["sleep", "60"], pass_fds=[child_alive], process_group=group)
"""


def closed_within(fd: int, seconds: float) -> bool:
    """True once every process holding the write end of the pipe behind `fd` has exited."""

    readable, _, _ = select.select([fd], [], [], seconds)
    return bool(readable) and os.read(fd, 1) == b""


@pytest.mark.skipif(os.name != "posix", reason="process groups are POSIX")
@pytest.mark.parametrize("own_group", [True, False], ids=["own-group", "shared-group"])
def test_a_worker_ends_when_its_engine_dies(project: Path, own_group: bool) -> None:
    request_read, request_write = os.pipe()  # kept open: the worker waits for a request
    worker_read, worker_write = os.pipe()
    child_read, child_write = os.pipe()
    passed = (request_read, worker_write, child_write)
    args = [sys.executable, "-c", ORPHANING_ENGINE, *map(str, passed), str(own_group)]
    env = {**os.environ, "PYTHONPATH": str(SRC)}
    # A group of its own: a worker that wrongly ended its shared group cannot reach pytest.
    engine = subprocess.Popen(args, pass_fds=passed, cwd=project, env=env, process_group=0)
    for fd in passed:
        os.close(fd)
    try:
        assert engine.wait(timeout=60) == 0
        assert closed_within(worker_read, 10)
        # Leading its group, the worker takes its children along; otherwise it ends alone.
        assert closed_within(child_read, 10 if own_group else 1) is own_group
    finally:
        with contextlib.suppress(ProcessLookupError):
            os.killpg(engine.pid, signal.SIGKILL)
        for fd in (request_write, worker_read, child_read):
            os.close(fd)


def test_unexpected_exceptions_become_internal_errors(
    project: Path, writer: RecordingWriter, monkeypatch: pytest.MonkeyPatch, capsys
) -> None:
    def broken(_task, _ctx):
        raise RuntimeError("boom")

    monkeypatch.setattr(protocol, "_resolve", lambda _handler: broken)
    handle_request(json.dumps(request("doctor", project, {})).encode(), writer)  # type: ignore[arg-type]
    error = writer.frames[0]["error"]
    assert error["code"] == "internal"
    assert error["message"] == "RuntimeError: boom"
    assert "RuntimeError: boom" in error["data"]["stderr_tail"]
    assert "Traceback" in capsys.readouterr().err


def test_progress_is_coalesced_and_the_last_state_is_flushed(ctx, writer: RecordingWriter) -> None:
    for current in range(1, 50):
        ctx.progress("extract", current, 49)
    ctx.flush()
    progress = [frame for frame in writer.frames if frame["type"] == "progress"]
    assert progress[0]["current"] == 1
    assert progress[-1]["current"] == 49
    assert len(progress) < 10
