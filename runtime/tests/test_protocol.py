from __future__ import annotations

import json
import os
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
    assert ready["catalog"]["project_templates"] == ["explainer"]
    assert "equation_derivation" in ready["catalog"]["scene_templates"]


def test_the_package_and_catalog_import_without_manim() -> None:
    code = (
        "import sys\n"
        "from manim_director_runtime import Region, Theme, Transition, protocol, catalog\n"
        "catalog.catalog()\n"
        "assert 'manim' not in sys.modules\n"
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
