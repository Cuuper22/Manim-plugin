"""Bridge protocol v2: one ready frame, one request, frames until a terminal frame, exit."""

from __future__ import annotations

import importlib
import importlib.metadata
import json
import os
import platform
import signal
import sys
import threading
import time
import traceback
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any, BinaryIO, Literal

from . import __version__, catalog
from .errors import DirectorError
from .jsonio import dumps
from .model import ArtifactKind, RuntimeArtifact
from .paths import is_within, public_path
from .tasks import (
    CaptionsTask,
    ContactSheetTask,
    DiagnoseTask,
    DiscoverTask,
    DoctorTask,
    ExportTask,
    FrameTask,
    IngestTask,
    InitTask,
    QaTask,
    RenderTask,
    StillTask,
    ValidateMathTask,
    parse_task,
)

PROTOCOL = 2
MAX_REQUEST_BYTES = 4 * 1024 * 1024
MAX_FRAME_BYTES = 1024 * 1024
_REQUEST_KEYS = ("protocol", "request_id", "method", "project_root", "params")
_PROGRESS_INTERVAL_SECONDS = 0.1
_PARENT_POLL_SECONDS = 0.5
_MAX_LOG_FRAMES = 1000
_HEAVY_MODULES = ("manim", "numpy", "PIL.Image")


@dataclass(frozen=True, slots=True)
class Method:
    handler: str  # "module:function" inside this package
    task: Any
    preload: bool = True


METHODS: dict[str, Method] = {
    "init": Method("scaffold:init", InitTask, preload=False),
    "discover": Method("inspection:discover", DiscoverTask, preload=False),
    "doctor": Method("doctor:doctor", DoctorTask),
    "render": Method("rendering:render", RenderTask),
    "still": Method("rendering:still", StillTask),
    "frame": Method("media:frame", FrameTask),
    "contact_sheet": Method("media:contact_sheet", ContactSheetTask),
    "qa": Method("qa:qa", QaTask),
    "diagnose": Method("diagnostics:diagnose", DiagnoseTask),
    "validate_math": Method("math_validation:validate_math", ValidateMathTask),
    "captions": Method("captions:captions", CaptionsTask),
    "ingest": Method("ingest:ingest", IngestTask),
    "export": Method("exporting:export", ExportTask),
}

Phase = Literal[
    "import", "animate", "encode", "extract", "analyze", "package", "transcode", "ingest"
]


def encode_frame(frame: dict[str, Any]) -> bytes:
    return (dumps(frame) + "\n").encode("utf-8")


class FrameWriter:
    def __init__(self, fd: int) -> None:
        self._fd = fd
        self._lock = threading.Lock()

    def write(self, frame: dict[str, Any]) -> None:
        self.send(encode_frame(frame))

    def send(self, data: bytes) -> None:
        view = memoryview(data)
        with self._lock:
            while view:
                view = view[os.write(self._fd, view) :]


class Context:
    """What a handler may use besides its task: the project root and frame emitters."""

    def __init__(self, project_root: Path, request_id: str, writer: FrameWriter) -> None:
        self.project_root = project_root
        self.request_id = request_id
        self._writer = writer
        self._last_progress = float("-inf")
        self._pending: dict[str, Any] | None = None
        self._logs = 0

    def progress(
        self,
        phase: Phase,
        current: int,
        total: int | None = None,
        scene_seconds: float | None = None,
        message: str | None = None,
    ) -> None:
        frame = {
            "type": "progress",
            "request_id": self.request_id,
            "phase": phase,
            "current": current,
            "total": total,
            "scene_seconds": scene_seconds,
            "message": None if message is None else message[:200],
        }
        now = time.monotonic()
        if now - self._last_progress < _PROGRESS_INTERVAL_SECONDS:
            self._pending = frame
            return
        self._last_progress = now
        self._pending = None
        self._writer.write(frame)

    def log(self, level: Literal["info", "warning"], message: str) -> None:
        if self._logs >= _MAX_LOG_FRAMES:
            return
        self._logs += 1
        frame = {"type": "log", "request_id": self.request_id, "level": level}
        self._writer.write({**frame, "message": message[:2000]})

    def flush(self) -> None:
        if self._pending is not None:
            self._writer.write(self._pending)
            self._pending = None

    def relative(self, path: Path | str) -> str:
        return public_path(path, self.project_root)

    def artifact(self, kind: ArtifactKind, path: Path, label: str | None = None) -> RuntimeArtifact:
        return RuntimeArtifact(kind=kind, path=self.relative(path), label=label)

    def require_inside(self, path: Path, field: str) -> Path:
        """Write targets come from the engine; refuse any that would leave the project."""

        if not is_within(Path(os.path.abspath(path)), self.project_root):
            raise DirectorError(
                "invalid_params",
                f"Invalid task field {field}: {path} is outside the project.",
                {"field": field, "reason": "outside_project"},
            )
        return path


def run_bridge(proto_fd: int, *, preload: bool, stdin: BinaryIO) -> None:
    """Serve exactly one request, then end the process without interpreter teardown."""

    _watch_parent()
    writer = FrameWriter(proto_fd)
    writer.write(ready_frame(preload))
    line = stdin.readline(MAX_REQUEST_BYTES + 1)
    if line.endswith(b"\n"):
        handle_request(line, writer)
    elif len(line) > MAX_REQUEST_BYTES:
        _write_error(writer, None, _invalid_request(f"Request exceeds {MAX_REQUEST_BYTES} bytes."))
    sys.stdout.flush()
    sys.stderr.flush()
    # Skipping teardown keeps GL/Cairo destructors from turning a finished request into a crash.
    os._exit(0)


def _watch_parent() -> None:
    """End this worker once its engine is gone, instead of rendering on as an orphan.

    The engine starts each worker as the leader of its own process group, so the group's
    ffmpeg and LaTeX children go with it. A worker that shares its starter's group (a shell,
    a test) ends only itself, so it cannot take its starter's other processes down. POSIX only.
    """

    if os.name != "posix":
        return
    parent = os.getppid()
    leader = os.getpgrp() == os.getpid()

    def watch() -> None:
        while os.getppid() == parent:
            time.sleep(_PARENT_POLL_SECONDS)
        if leader:
            os.killpg(0, signal.SIGKILL)
        else:
            os.kill(os.getpid(), signal.SIGKILL)

    threading.Thread(target=watch, name="parent-watch", daemon=True).start()


def handle_request(line: bytes, writer: FrameWriter) -> None:
    try:
        request = json.loads(line)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        _write_error(writer, None, _invalid_request("Request is not valid JSON.", detail=str(exc)))
        return
    request_id = request.get("request_id") if isinstance(request, dict) else None
    if not isinstance(request_id, str) or not request_id:
        detail = "request_id must be a non-empty string"
        _write_error(writer, None, _invalid_request("Request has no request_id.", detail=detail))
        return
    ctx: Context | None = None
    try:
        method, project_root, params = _validate_request(request)
        task = parse_task(method.task, params)
        ctx = Context(project_root, request_id, writer)
        result = _resolve(method.handler)(task, ctx)
        ctx.flush()
        _write_result(writer, request_id, result)
    except DirectorError as error:
        if ctx is not None:
            ctx.flush()
        _write_error(writer, request_id, error)
    except Exception as exc:
        trace = traceback.format_exc()
        sys.stderr.write(trace)
        message = f"{type(exc).__name__}: {exc}"[:500]
        _write_error(
            writer, request_id, DirectorError("internal", message, {"stderr_tail": trace[-16384:]})
        )


def _validate_request(request: dict[str, Any]) -> tuple[Method, Path, object]:
    unknown = sorted(set(request) - set(_REQUEST_KEYS))
    missing = [key for key in _REQUEST_KEYS if key not in request]
    if unknown or missing:
        problems = [f"unknown field {key}" for key in unknown] + [
            f"missing {key}" for key in missing
        ]
        raise _invalid_request("Request does not match protocol v2.", detail="; ".join(problems))
    if request["protocol"] != PROTOCOL:
        raise DirectorError(
            "invalid_request",
            f"Request protocol {request['protocol']!r} is not supported; expected {PROTOCOL}.",
            {"detail": "protocol mismatch", "expected": PROTOCOL, "got": request["protocol"]},
        )
    name = request["method"]
    method = METHODS.get(name) if isinstance(name, str) else None
    if method is None:
        raise DirectorError("unknown_method", f"Unknown bridge method {name!r}.", {"method": name})
    raw_root = request["project_root"]
    if not isinstance(raw_root, str) or not Path(raw_root).is_absolute():
        raise _invalid_request("project_root must be an absolute path.", detail=repr(raw_root))
    if not Path(raw_root).is_dir():
        raise _invalid_request("project_root is not a directory.", detail=raw_root)
    return method, Path(raw_root), request["params"]


def _resolve(handler: str) -> Callable[[Any, Context], Any]:
    module_name, function = handler.split(":")
    return getattr(importlib.import_module(f"{__package__}.{module_name}"), function)


def ready_frame(preload: bool) -> dict[str, Any]:
    started = time.monotonic()
    preloaded: list[str] = []
    failed: list[dict[str, str]] = []
    if preload:
        handlers = [
            f"{__package__}.{m.handler.split(':')[0]}" for m in METHODS.values() if m.preload
        ]
        for module in dict.fromkeys([*_HEAVY_MODULES, *handlers]):
            try:
                importlib.import_module(module)
            except Exception as exc:  # best effort: the job that needs it reports the real error
                failed.append({"module": module, "message": f"{type(exc).__name__}: {exc}"[:500]})
            else:
                preloaded.append(module)
    return {
        "type": "ready",
        "protocol": PROTOCOL,
        "runtime_version": __version__,
        "python": platform.python_version(),
        "manim": _distribution_version("manim"),
        "preloaded": preloaded,
        "preload_failed": failed,
        "preload_ms": round((time.monotonic() - started) * 1000),
        "catalog": catalog.catalog(),
    }


def _distribution_version(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def _invalid_request(message: str, *, detail: str | None = None) -> DirectorError:
    return DirectorError("invalid_request", message, {"detail": detail or message})


def _write_result(writer: FrameWriter, request_id: str, result: Any) -> None:
    data = encode_frame({"type": "result", "request_id": request_id, "result": result})
    if len(data) > MAX_FRAME_BYTES:
        error = DirectorError("internal", "The result exceeds the 1 MiB frame limit.", None)
        _write_error(writer, request_id, error)
        return
    writer.send(data)


def _write_error(writer: FrameWriter, request_id: str | None, error: DirectorError) -> None:
    writer.write({"type": "error", "request_id": request_id, "error": error.as_dict()})
