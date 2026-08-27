from __future__ import annotations

import json
import sys
import traceback
from collections.abc import Mapping
from typing import Any, Callable, TextIO

from .errors import DirectorError
from .util import jsonable


Writer = Callable[[dict[str, Any]], None]


def _dispatch(method: str, params: Mapping[str, Any], emit: Callable[[str, dict[str, Any]], None]) -> Any:
    """Lazy imports keep the bridge small when an operation does not need heavy tools."""

    aliases = {"validate_math": "math_validate", "debug": "diagnose", "export_bundle": "export"}
    method = aliases.get(method, method)
    if method == "doctor":
        from .doctor import doctor
        return doctor(params)
    if method == "scaffold":
        from .scaffold import scaffold
        return scaffold(params)
    if method in {"discover", "inspect"}:
        from .inspection import discover, inspect
        return discover(params) if method == "discover" else inspect(params)
    if method in {"render", "preview", "still", "section"}:
        from .rendering import preview, render, section, still
        return {"render": render, "preview": preview, "still": still, "section": section}[method](params, emit)
    if method == "contact_sheet":
        from .media import contact_sheet
        return contact_sheet(params, emit)
    if method == "media":
        from pathlib import Path
        from .media import mix_audio, normalize_audio, probe_media, representative_frames
        from .util import confined_path, project_root
        root = project_root(params)
        operation = str(params.get("operation", "probe"))
        if operation == "probe":
            return probe_media(confined_path(root, str(params.get("source")), must_exist=True))
        if operation == "frames":
            return {"frames": representative_frames(confined_path(root, str(params.get("source")), must_exist=True), confined_path(root, str(params.get("output", "output/frames"))), count=int(params.get("count", 6)), emit=emit)}
        if operation == "normalize_audio":
            return normalize_audio(confined_path(root, str(params.get("source")), must_exist=True), confined_path(root, str(params.get("output", "output/audio-normalized.wav"))), target_lufs=float(params.get("target_lufs", -16)), emit=emit)
        if operation == "mix_audio":
            tracks = [{**track, "path": str(confined_path(root, str(track["path"]), must_exist=True))} for track in params.get("tracks", [])]
            return mix_audio(tracks, confined_path(root, str(params.get("output", "output/audio-mix.wav"))), emit=emit)
        raise DirectorError("invalid_media_operation", f"Unknown media operation: {operation}")
    if method == "qa":
        from .qa import qa
        return qa(params, emit)
    if method == "diagnose":
        from .diagnostics import diagnose
        return diagnose(params)
    if method == "math_validate":
        from .math_validation import math_validate
        return math_validate(params)
    if method == "captions":
        from .captions import captions
        return captions(params)
    if method == "assets":
        from .assets import assets
        return assets(params, emit)
    if method == "ingest":
        from .ingest import ingest
        return ingest(params, emit)
    if method == "export":
        from .exporting import export_bundle
        return export_bundle(params, emit)
    if method == "sample":
        from .sample import generate_sample
        return generate_sample(params)
    if method == "themes":
        from .themes import list_themes
        return {"themes": list_themes()}
    if method == "templates":
        from .templates import templates
        return templates(params)
    if method == "capabilities":
        return {
            "protocol_version": 1,
            "methods": ["doctor", "scaffold", "ingest", "discover", "inspect", "render", "preview", "still", "section", "contact_sheet", "media", "qa", "diagnose", "math_validate", "captions", "assets", "export", "sample", "themes", "templates", "capabilities"],
        }
    raise DirectorError("method_not_found", f"Unknown bridge method: {method}")


def handle_request(request: Any, write: Writer) -> None:
    request_id = request.get("request_id", request.get("id")) if isinstance(request, Mapping) else None
    if not isinstance(request, Mapping):
        write({"request_id": request_id, "type": "error", "error": DirectorError("invalid_request", "A request must be a JSON object").as_dict()})
        return
    method = request.get("method")
    params = request.get("params", {})
    if not isinstance(method, str) or not method:
        write({"request_id": request_id, "type": "error", "error": DirectorError("invalid_request", "method must be a non-empty string").as_dict()})
        return
    if not isinstance(params, Mapping):
        write({"request_id": request_id, "type": "error", "error": DirectorError("invalid_request", "params must be an object").as_dict()})
        return

    def emit(event: str, data: dict[str, Any]) -> None:
        write({"request_id": request_id, "type": "event", "event": event, "data": jsonable(data)})

    try:
        result = _dispatch(method, params, emit)
        write({"request_id": request_id, "type": "result", "result": jsonable(result)})
    except DirectorError as exc:
        write({"request_id": request_id, "type": "error", "error": exc.as_dict()})
    except Exception as exc:
        # Keep the public error stable; the traceback goes to stderr for local diagnosis.
        traceback.print_exc(file=sys.stderr)
        write({"request_id": request_id, "type": "error", "error": {"code": "internal_error", "message": str(exc) or type(exc).__name__}})


def serve(input_stream: TextIO = sys.stdin, output_stream: TextIO = sys.stdout) -> int:
    def write(message: dict[str, Any]) -> None:
        output_stream.write(json.dumps(jsonable(message), ensure_ascii=False, separators=(",", ":"), allow_nan=False) + "\n")
        output_stream.flush()

    for raw in input_stream:
        if not raw.strip():
            continue
        try:
            request = json.loads(raw)
        except json.JSONDecodeError as exc:
            write({"request_id": None, "type": "error", "error": {"code": "invalid_json", "message": str(exc), "data": {"line": exc.lineno, "column": exc.colno}}})
            continue
        try:
            handle_request(request, write)
        except BrokenPipeError:
            return 0
    return 0
