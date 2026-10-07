"""The `doctor` operation: what this Python environment can do (contract §1.3 doctor)."""

from __future__ import annotations

import importlib
import importlib.metadata
import importlib.util
import platform
import shutil
import sys
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from . import __version__, process
from .model import Finding, RuntimeArtifact, Severity
from .protocol import PROTOCOL
from .tasks import DoctorTask

if TYPE_CHECKING:
    from .protocol import Context

LOW_DISK_BYTES = 512 * 1024 * 1024
# Check name -> (module that must import, distribution name), in report order.
PACKAGES = {
    "manim": ("manim", "manim"),
    "numpy": ("numpy", "numpy"),
    "PIL": ("PIL.Image", "Pillow"),
    "av": ("av", "av"),
    "yaml": ("yaml", "PyYAML"),
    "sympy": ("sympy", "sympy"),
    "pypdf": ("pypdf", "pypdf"),
    "moderngl": ("moderngl", "moderngl"),
}
EXECUTABLES = ("ffmpeg", "ffprobe", "latex", "pdflatex", "xelatex", "lualatex", "dvisvgm")
TEX_ENGINES = ("latex", "pdflatex", "xelatex", "lualatex")


@dataclass(frozen=True, slots=True)
class RuntimeInfo:
    version: str
    protocol: int
    python: str
    executable: str
    platform: str


@dataclass(frozen=True, slots=True)
class Check:
    name: str
    kind: Literal["package", "executable"]
    available: bool
    version: str | None
    path: str | None


@dataclass(frozen=True, slots=True)
class Capabilities:
    render: bool
    renderers: list[str]
    latex: bool
    video_tools: bool
    visual_qa: bool
    symbolic_math: bool
    pdf_ingest: bool


@dataclass(frozen=True, slots=True)
class Disk:
    free_bytes: int
    total_bytes: int


@dataclass(frozen=True, slots=True)
class DoctorResult:
    ok: bool
    runtime: RuntimeInfo
    checks: list[Check]
    capabilities: Capabilities
    disk: Disk
    findings: list[Finding]
    artifacts: list[RuntimeArtifact]


def doctor(task: DoctorTask, ctx: Context) -> DoctorResult:
    packages = {name: _package(name, *spec) for name, spec in PACKAGES.items()}
    broken = {name: error for name, (_, error) in packages.items() if error}
    checks = [check for check, _ in packages.values()]
    checks += [_executable(name) for name in EXECUTABLES]
    have = {check.name: check.available for check in checks}
    render = all(have[name] for name in ("manim", "numpy", "PIL", "av", "yaml"))
    opengl_error = _opengl_error() if have["moderngl"] else "moderngl is not installed"
    capabilities = Capabilities(
        render=render,
        renderers=(["cairo"] + ([] if opengl_error else ["opengl"])) if render else [],
        latex=any(have[name] for name in TEX_ENGINES) and have["dvisvgm"],
        video_tools=have["ffmpeg"] and have["ffprobe"],
        visual_qa=have["PIL"] and have["numpy"],
        symbolic_math=have["sympy"],
        pdf_ingest=have["pypdf"],
    )
    usage = shutil.disk_usage(ctx.project_root)
    return DoctorResult(
        ok=capabilities.render and capabilities.video_tools,
        runtime=RuntimeInfo(
            version=__version__,
            protocol=PROTOCOL,
            python=platform.python_version(),
            executable=sys.executable,
            platform=platform.platform(),
        ),
        checks=checks,
        capabilities=capabilities,
        disk=Disk(free_bytes=usage.free, total_bytes=usage.total),
        findings=_findings(have, broken, capabilities, opengl_error, usage.free),
        artifacts=[],
    )


def _package(name: str, module: str, distribution: str) -> tuple[Check, str | None]:
    """The check, plus the import error of a package that is installed but broken."""

    if importlib.util.find_spec(module.partition(".")[0]) is None:
        return Check(name=name, kind="package", available=False, version=None, path=None), None
    try:
        version = importlib.metadata.version(distribution)
    except importlib.metadata.PackageNotFoundError:
        version = None  # importable without distribution metadata, e.g. a source checkout
    try:
        importlib.import_module(module)
    except Exception as exc:  # present but unusable, e.g. a missing system library
        error = f"{type(exc).__name__}: {exc}".splitlines()[0][:200]
    else:
        error = None
    check = Check(name=name, kind="package", available=error is None, version=version, path=None)
    return check, error


def _executable(name: str) -> Check:
    path = shutil.which(name)
    # FFmpeg tools reject GNU-style --version.
    flag = "-version" if name in ("ffmpeg", "ffprobe") else "--version"
    version = process.version_line(path, flag) if path else None
    return Check(
        name=name, kind="executable", available=path is not None, version=version, path=path
    )


def _opengl_error() -> str | None:
    try:
        import moderngl

        context = moderngl.create_standalone_context()
    except Exception as exc:  # any backend failure means no headless OpenGL here
        return str(exc).strip().splitlines()[0] if str(exc).strip() else type(exc).__name__
    context.release()
    return None


def _findings(
    have: dict[str, bool],
    broken: dict[str, str],
    capabilities: Capabilities,
    opengl_error: str | None,
    free: int,
) -> list[Finding]:
    findings = []

    def add(code: str, severity: Severity, message: str, hint: str) -> None:
        findings.append(Finding(code=code, severity=severity, message=message, hint=hint))

    def absent(name: str, label: str) -> str:
        if name in broken:
            return f"{label} is installed but fails to import ({broken[name]})"
        return f"{label} is not installed"

    required = {
        "manim": ("manim_missing", "Manim Community Edition", "pip install 'manim>=0.21,<0.22'"),
        "numpy": ("numpy_missing", "NumPy", "pip install numpy"),
        "PIL": ("pillow_missing", "Pillow", "pip install Pillow"),
        "av": ("av_missing", "PyAV", "pip install av"),
        "yaml": ("yaml_missing", "PyYAML", "pip install PyYAML"),
    }
    for name, (code, label, command) in required.items():
        if not have[name]:
            add(
                code,
                Severity.ERROR,
                f"{absent(name, label)}; scenes cannot render.",
                f"Run {command}.",
            )
    if not capabilities.video_tools:
        add(
            "ffmpeg_missing",
            Severity.ERROR,
            "ffmpeg or ffprobe is not on PATH.",
            "Install FFmpeg; it provides both tools.",
        )
    if not capabilities.latex:
        add(
            "latex_missing",
            Severity.WARNING,
            "No TeX engine with dvisvgm was found; Tex and MathTex cannot render.",
            "Install a TeX distribution that includes dvisvgm (for example TeX Live).",
        )
    if free < LOW_DISK_BYTES:
        add(
            "low_disk",
            Severity.WARNING,
            f"Only {free // (1024 * 1024)} MiB of disk is free.",
            "Free disk space before rendering.",
        )
    if not have["sympy"]:
        add(
            "sympy_missing",
            Severity.INFO,
            f"{absent('sympy', 'SymPy')}; validate_math checks numerically only.",
            "Run pip install sympy.",
        )
    if not have["pypdf"]:
        add(
            "pypdf_missing",
            Severity.INFO,
            f"{absent('pypdf', 'pypdf')}; PDF sources cannot be ingested.",
            "Run pip install pypdf.",
        )
    if opengl_error:
        add(
            "opengl_unavailable",
            Severity.INFO,
            f"OpenGL rendering is unavailable: {opengl_error}.",
            "Use the cairo renderer.",
        )
    return findings
