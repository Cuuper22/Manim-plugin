from __future__ import annotations

import os
import re
import shutil
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence

from .diagnostics import diagnose_text
from .errors import DirectorError
from .inspection import inspect_file
from .util import Emit, confined_path, noop_emit, project_root, python_executable, run_command


@dataclass(frozen=True, slots=True)
class RenderProfile:
    name: str
    quality: str
    width: int
    height: int
    fps: int


PROFILES = {
    "draft": RenderProfile("draft", "l", 854, 480, 15),
    "preview": RenderProfile("preview", "m", 1280, 720, 30),
    "production": RenderProfile("production", "h", 1920, 1080, 60),
    "ultra": RenderProfile("ultra", "k", 3840, 2160, 60),
}

FORMATS = {"mp4", "mov", "webm", "gif", "png"}
RENDERERS = {"cairo", "opengl"}


def _manim_prefix(params: Mapping[str, Any]) -> list[str]:
    explicit = params.get("manim_executable")
    if explicit:
        if isinstance(explicit, (list, tuple)):
            if not explicit or not all(isinstance(part, str) and part for part in explicit):
                raise DirectorError("invalid_executable", "manim_executable list must contain non-empty strings")
            return list(explicit)
        return [str(explicit)]
    executable = shutil.which("manim")
    return [executable] if executable else [python_executable(), "-m", "manim"]


def _profile(params: Mapping[str, Any], default: str = "preview") -> RenderProfile:
    name = str(params.get("profile", default))
    if name == "custom" or (name not in PROFILES and all(key in params for key in ("width", "height", "fps"))):
        try:
            width, height, fps = int(params["width"]), int(params["height"]), int(params["fps"])
        except (KeyError, TypeError, ValueError) as exc:
            raise DirectorError("invalid_profile", "Custom profile requires integer width, height, and fps") from exc
        if not (16 <= width <= 16384 and 16 <= height <= 16384 and 1 <= fps <= 240):
            raise DirectorError("invalid_profile", "Custom dimensions or fps are outside supported bounds")
        return RenderProfile(name, "", width, height, fps)
    try:
        base = PROFILES[name]
    except KeyError as exc:
        raise DirectorError("invalid_profile", f"Unknown render profile: {name}", {"available": sorted(PROFILES) + ["custom"]}) from exc
    width = int(params.get("width", base.width))
    height = int(params.get("height", base.height))
    fps = int(params.get("fps", base.fps))
    if width != base.width or height != base.height or fps != base.fps:
        return RenderProfile(name, "", width, height, fps)
    return base


def build_render_command(params: Mapping[str, Any], *, mode: str = "render") -> tuple[list[str], dict[str, Any]]:
    root = project_root(params)
    scene_file = confined_path(root, str(params.get("scene_file", params.get("path", "scenes/main.py"))), must_exist=True)
    if scene_file.suffix != ".py":
        raise DirectorError("invalid_scene_file", "Manim scene file must end in .py")
    report = inspect_file(scene_file)
    if not report["valid_python"]:
        raise DirectorError("invalid_python", "Scene file contains a syntax error", report["syntax_error"] or {})
    requested_scenes = params.get("scenes")
    if requested_scenes is None:
        requested_scenes = [params["scene"]] if params.get("scene") else []
    if isinstance(requested_scenes, str):
        requested_scenes = [requested_scenes]
    requested_scenes = [str(x) for x in requested_scenes]
    discovered = {scene["name"] for scene in report["scenes"]}
    missing = sorted(set(requested_scenes) - discovered)
    if missing:
        raise DirectorError("scene_not_found", "Requested scene was not found", {"missing": missing, "available": sorted(discovered)})
    if not requested_scenes and len(discovered) == 1:
        requested_scenes = sorted(discovered)
    if not requested_scenes and not bool(params.get("all_scenes", False)):
        raise DirectorError("scene_required", "Specify a scene or set all_scenes=true", {"available": sorted(discovered)})

    renderer = str(params.get("renderer", "cairo"))
    if renderer not in RENDERERS:
        raise DirectorError("invalid_renderer", f"Unsupported renderer: {renderer}", {"available": sorted(RENDERERS)})
    transparent = bool(params.get("transparent", False))
    fmt = str(params.get("format", "mov" if transparent else "mp4")).lower()
    if fmt not in FORMATS:
        raise DirectorError("invalid_format", f"Unsupported output format: {fmt}", {"available": sorted(FORMATS)})
    if transparent and fmt in {"mp4", "gif"}:
        raise DirectorError("alpha_unsupported", f"{fmt} does not preserve a full alpha channel; use mov or webm")
    profile = _profile(params, "draft" if mode in {"preview", "still"} else "production")
    media_dir = confined_path(root, str(params.get("media_dir", ".manim-director/media")))
    media_dir.mkdir(parents=True, exist_ok=True)
    output_name = str(params.get("output_name", requested_scenes[0] if len(requested_scenes) == 1 else scene_file.stem))
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", output_name):
        raise DirectorError("invalid_output_name", "output_name may contain only letters, numbers, dot, dash, and underscore")
    command = [*_manim_prefix(params), f"-q{profile.quality}"] if profile.quality else _manim_prefix(params)
    command += ["--renderer", renderer, "--format", fmt, "--media_dir", str(media_dir)]
    if len(requested_scenes) == 1 and not bool(params.get("all_scenes", False)):
        command += ["--output_file", output_name]
    if not profile.quality:
        command += ["--resolution", f"{profile.width},{profile.height}", "--fps", str(profile.fps)]
    if transparent:
        command.append("--transparent")
    if mode == "still":
        command.append("--save_last_frame")
    if mode == "section" or params.get("save_sections") or params.get("sections"):
        command.append("--save_sections")
    if bool(params.get("disable_caching", False)):
        command.append("--disable_caching")
    if bool(params.get("flush_cache", False)):
        command.append("--flush_cache")
    if bool(params.get("all_scenes", False)):
        command.append("--write_all")
    command.append(str(scene_file))
    if not bool(params.get("all_scenes", False)):
        command.extend(requested_scenes)
    metadata = {
        "project_root": root, "scene_file": scene_file,
        "scenes": sorted(discovered) if bool(params.get("all_scenes", False)) else requested_scenes,
        "renderer": renderer, "format": "png" if mode == "still" else fmt,
        "transparent": transparent, "profile": profile, "media_dir": media_dir,
        "output_name": output_name,
    }
    return command, metadata


INTERMEDIATE_DIRECTORIES = {
    "partial_movie_files", "partial_movie_file", "partial_files", "cache", "caches",
    "cached_files", "temp", "tmp",
}


def _is_intermediate(path: Path, media_dir: Path) -> bool:
    try:
        parts = path.relative_to(media_dir).parts
    except ValueError:
        return True
    lowered = {part.lower() for part in parts[:-1]}
    return bool(lowered & INTERMEDIATE_DIRECTORIES) or path.name.startswith(".")


def _outputs_since(media_dir: Path, started: float, extensions: set[str]) -> list[Path]:
    files: list[Path] = []
    if media_dir.exists():
        for path in media_dir.rglob("*"):
            try:
                if (
                    path.is_file()
                    and path.suffix.lower() in extensions
                    and path.stat().st_mtime >= started - 1.0
                    and not _is_intermediate(path, media_dir)
                ):
                    files.append(path)
            except OSError:
                continue
    return sorted(files, key=lambda p: p.as_posix())


def _is_section_output(path: Path, media_dir: Path) -> bool:
    try:
        parts = [part.lower() for part in path.relative_to(media_dir).parts[:-1]]
    except ValueError:
        return False
    return "sections" in parts or "section" in parts


def _pick_preferred(candidates: list[Path]) -> Path:
    """Prefer the newest candidate, with a stable path tie-breaker."""
    return sorted(candidates, key=lambda path: (-path.stat().st_mtime_ns, path.as_posix()))[0]


def _select_render_outputs(
    candidates: list[Path], *, media_dir: Path, mode: str, output_name: str,
    scenes: list[str], sections: Sequence[str] | None = None,
) -> list[Path]:
    """Select deliverables and never expose Manim's partial/cache segments."""

    clean = [path for path in candidates if not _is_intermediate(path, media_dir)]
    section_candidates = [path for path in clean if _is_section_output(path, media_dir)]
    if sections:
        normalized = [re.sub(r"\W+", "", str(name)).lower() for name in sections]
        matched = [
            path for path in section_candidates
            if any(token in re.sub(r"\W+", "", path.stem).lower() for token in normalized)
        ]
        return sorted(matched, key=lambda path: path.as_posix())
    if mode == "section":
        return sorted(section_candidates, key=lambda path: path.as_posix())

    finals = [path for path in clean if not _is_section_output(path, media_dir)]
    expected_names = [Path(output_name).stem] if len(scenes) == 1 else list(scenes)
    selected: list[Path] = []
    for name in expected_names:
        exact = [path for path in finals if path.stem == name]
        if exact:
            selected.append(_pick_preferred(exact))
    if selected:
        return selected
    # Compatibility fallback for backends that decorate final filenames. Keep the
    # result bounded to the requested scene count and prefer shallower paths.
    decorated = [
        path for path in finals
        if any(re.sub(r"\W+", "", name).lower() in re.sub(r"\W+", "", path.stem).lower() for name in expected_names)
    ]
    fallback = decorated or finals
    limit = max(1, len(expected_names))
    return sorted(fallback, key=lambda path: (len(path.relative_to(media_dir).parts), path.as_posix()))[:limit]


def render(params: Mapping[str, Any], emit: Emit = noop_emit, *, mode: str = "render") -> dict[str, Any]:
    mutable = dict(params)
    if mode == "preview":
        mutable.setdefault("profile", "preview")
    elif mode == "still":
        mutable.setdefault("profile", "draft")
        mutable["format"] = "png"
    command, metadata = build_render_command(mutable, mode=mode)
    started_epoch = time.time()
    timeout = float(mutable.get("timeout", 1800))
    result = run_command(command, cwd=metadata["project_root"], timeout=timeout, emit=emit)
    if result.returncode != 0:
        raise DirectorError(
            "render_failed", f"Manim render failed with exit code {result.returncode}",
            {"command": command, "diagnostics": diagnose_text(result.output), "tail": result.output[-12000:]},
        )
    suffixes = {".png"} if mode == "still" else {f".{metadata['format']}"}
    candidates = _outputs_since(metadata["media_dir"], started_epoch, suffixes)
    section_filter = mutable.get("sections")
    if isinstance(section_filter, str):
        section_filter = [section_filter]
    outputs = _select_render_outputs(
        candidates, media_dir=metadata["media_dir"], mode=mode,
        output_name=metadata["output_name"], scenes=metadata["scenes"], sections=section_filter,
    )
    if not outputs:
        if section_filter or mode == "section":
            raise DirectorError(
                "section_not_found", "Render completed but none of the requested section outputs were found",
                {"requested": list(section_filter or []), "available_final_files": [path.name for path in candidates[:50]]},
            )
        raise DirectorError(
            "render_output_missing", "Manim exited successfully but no requested media was produced",
            {"media_dir": str(metadata["media_dir"]), "format": metadata["format"], "command": command},
        )
    copied: list[Path] = []
    if mutable.get("output"):
        root: Path = metadata["project_root"]
        destination = confined_path(root, str(mutable["output"]))
        if len(outputs) == 1:
            if destination.suffix.lower() != outputs[0].suffix.lower():
                destination = destination.with_suffix(outputs[0].suffix)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(outputs[0], destination)
            copied.append(destination)
        else:
            destination.mkdir(parents=True, exist_ok=True)
            for source in outputs:
                target = destination / source.name
                shutil.copy2(source, target)
                copied.append(target)
    artifacts = copied or outputs
    from .media import compact_media_summary, probe_media

    probes = []
    for path in artifacts:
        try:
            probes.append(compact_media_summary(probe_media(path)))
        except DirectorError:
            probes.append({"path": str(path), "probe": "unavailable"})
    return {
        "job_id": uuid.uuid4().hex,
        "mode": mode,
        "command": command,
        "elapsed_seconds": round(result.elapsed_seconds, 4),
        "profile": {"name": metadata["profile"].name, "width": metadata["profile"].width, "height": metadata["profile"].height, "fps": metadata["profile"].fps},
        "renderer": metadata["renderer"], "format": metadata["format"],
        "artifacts": [str(p) for p in artifacts], "media": probes,
    }


def preview(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    result = render(params, emit, mode="preview")
    if bool(params.get("contact_sheet", False)):
        from .media import contact_sheet

        artifact = Path(result["artifacts"][0])
        if artifact.suffix.lower() in {".mp4", ".mov", ".webm", ".gif"}:
            output = params.get("contact_sheet_output", f"output/{artifact.stem}-contact-sheet.png")
            sheet = contact_sheet({
                "project_root": str(result_path_root(params)), "source": str(artifact), "output": output,
                "count": int(params.get("contact_sheet_frames", 6)), "columns": int(params.get("contact_sheet_columns", 3)),
            }, emit)
            result["contact_sheet"] = {
                key: sheet[key] for key in ("path", "width", "height", "images", "columns", "rows") if key in sheet
            }
    return result


def still(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    return render(params, emit, mode="still")


def section(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    return render(params, emit, mode="section")


def result_path_root(params: Mapping[str, Any]) -> Path:
    """Kept separate so composed operations use the same validated project root."""
    return project_root(params)
