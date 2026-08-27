from __future__ import annotations

import importlib.metadata
import importlib.util
import platform
import shutil
import sys
from pathlib import Path
from typing import Any, Mapping

from .util import executable_version, project_root


def _version_args(name: str) -> tuple[str, ...]:
    # FFmpeg tools treat GNU-style --version as an invalid option and return a
    # failure code even though they print a version banner.
    return ("-version",) if name in {"ffmpeg", "ffprobe"} else ("--version",)


def _package(name: str) -> dict[str, Any]:
    available = importlib.util.find_spec(name) is not None
    version = None
    if available:
        try:
            version = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            pass
    return {"available": available, "version": version}


def doctor(params: Mapping[str, Any]) -> dict[str, Any]:
    root = project_root(params)
    executables = {
        name: executable_version(name, _version_args(name))
        for name in ("manim", "ffmpeg", "ffprobe", "latex", "pdflatex", "xelatex", "lualatex", "dvisvgm", "typst")
    }
    packages = {name: _package(name) for name in ("manim", "PIL", "numpy", "sympy")}
    manim_ready = bool(executables["manim"]["available"] or packages["manim"]["available"])
    ffmpeg_ready = bool(executables["ffmpeg"]["available"] and executables["ffprobe"]["available"])
    latex_ready = any(executables[x]["available"] for x in ("latex", "pdflatex", "xelatex", "lualatex"))
    renderers = ["cairo"] if manim_ready else []
    if manim_ready:
        try:
            import importlib

            module = importlib.import_module("manim")
            if getattr(module, "OpenGLRenderer", None) is not None:
                renderers.append("opengl")
        except Exception:
            # Importing Manim may touch optional system libraries. Its CLI remains usable.
            pass
    issues: list[dict[str, str]] = []
    if not manim_ready:
        issues.append({"code": "manim_missing", "message": "Install Manim Community Edition to render scenes."})
    if not ffmpeg_ready:
        issues.append({"code": "ffmpeg_missing", "message": "Install FFmpeg and ffprobe for video inspection and post-processing."})
    if not latex_ready:
        issues.append({"code": "latex_missing", "message": "Install a LaTeX engine and dvisvgm to render Tex/MathTex."})
    if not executables["typst"]["available"]:
        issues.append({"code": "typst_missing", "message": "Typst content is unavailable until the typst executable is installed."})
    disk = shutil.disk_usage(root)
    if disk.free < 512 * 1024 * 1024:
        issues.append({"code": "low_disk", "message": "Less than 512 MiB is free under the project filesystem."})
    return {
        "ok": manim_ready,
        "project_root": str(root),
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "python_executable": sys.executable,
        },
        "executables": executables,
        "packages": packages,
        "capabilities": {
            "render": manim_ready,
            "renderers": sorted(set(renderers)),
            "video_processing": ffmpeg_ready,
            "latex": latex_ready and bool(executables["dvisvgm"]["available"]),
            "typst": bool(executables["typst"]["available"]),
            "visual_qa": packages["PIL"]["available"],
            "vectorized_qa": packages["numpy"]["available"],
            "symbolic_math": packages["sympy"]["available"],
        },
        "disk": {"free_bytes": disk.free, "total_bytes": disk.total},
        "issues": issues,
    }
