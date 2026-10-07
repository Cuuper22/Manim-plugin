"""Failure classification shared by `diagnose` and render failures (contract §1.3)."""

from __future__ import annotations

import re
import traceback
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from .model import Finding, RuntimeArtifact, Severity, SourceLocation
from .paths import is_user_source, public_path
from .tasks import DiagnoseTask

if TYPE_CHECKING:
    from .protocol import Context

MAX_FINDINGS = 20
_TEX_TOOLS = r"(?:latex|pdflatex|xelatex|lualatex|dvisvgm)"
_SYNTAX_ERROR = re.compile(r"(?m)^\s*(?:SyntaxError|IndentationError|TabError): ")
_TRACE_FILE = re.compile(r'(?m)^\s*File "(?P<file>[^"]+)", line (?P<line>\d+)')
# TeX prints the input up to the failure point on the line after the "! message" line.
_TEX_CONTEXT = re.compile(r"(?m)^! .*\n(?:l\.\d+ |<[^>]*> )?(?P<context>.*\S)")
_TEX_TOKEN = re.compile(r"\\[A-Za-z@]+\*?|\S+")
_TEX_LOG = re.compile(r"the log file: (?P<log>\S+\.log)")


@dataclass(frozen=True, slots=True)
class _Rule:
    code: str
    pattern: re.Pattern[str]
    hint: str
    message: Callable[[re.Match[str]], str]


def _group(name: str) -> Callable[[re.Match[str]], str]:
    return lambda match: match.group(name).strip()


def _fixed(text: str) -> Callable[[re.Match[str]], str]:
    return lambda _match: text


_RULES = (
    _Rule(
        "python_syntax",
        re.compile(r"(?:SyntaxError|IndentationError|TabError): (?P<message>.+)"),
        "Fix the syntax error before rendering.",
        _group("message"),
    ),
    _Rule(
        "python_import",
        re.compile(
            r"ModuleNotFoundError: (?P<message>No module named '[^']+')"
            r"|ImportError: (?P<import>cannot import name .+)"
        ),
        "Install the package in the runtime environment, or fix the import.",
        lambda match: (match.group("message") or match.group("import")).strip(),
    ),
    _Rule(
        "python_name",
        re.compile(r"NameError: (?P<message>.+)"),
        "Define or import the referenced name.",
        _group("message"),
    ),
    _Rule(
        "python_attribute",
        re.compile(r"AttributeError: (?P<message>.+)"),
        "Check that the attribute exists in the installed Manim version.",
        _group("message"),
    ),
    _Rule(
        "api_signature",
        re.compile(
            r"TypeError: (?P<message>.*(?:unexpected keyword argument|required positional argument"
            r"|positional arguments? but|got multiple values).*)"
        ),
        "The call does not match the installed signature; check the Manim docs for this version.",
        _group("message"),
    ),
    _Rule(
        "latex_missing",
        re.compile(
            rf"No such file or directory: '{_TEX_TOOLS}'|\b{_TEX_TOOLS}: (?:command )?not found"
        ),
        "Install a TeX distribution with dvisvgm, or use Text instead of Tex/MathTex.",
        _fixed("LaTeX or dvisvgm is not installed."),
    ),
    _Rule(
        "latex_package",
        re.compile(r"LaTeX Error: File `(?P<package>[^']+)' not found"),
        "Install the missing TeX package (for example with tlmgr) or remove it from the template.",
        lambda match: f"TeX file {match.group('package')} is not installed.",
    ),
    _Rule(
        "latex_error",
        re.compile(r"(?m)^! (?!LaTeX Error: File `)(?P<message>.+)$"),
        "Correct the TeX in the expression.",
        _group("message"),
    ),
    _Rule(
        "latex_error",
        re.compile(rf"{_TEX_TOOLS} error converting to (?:dvi|xdv|pdf)"),
        "Correct the TeX in the expression.",
        _fixed("LaTeX could not compile the expression."),
    ),
    _Rule(
        "ffmpeg_encoder",
        re.compile(r"Unknown encoder ['\"]?(?P<encoder>[\w-]+)"),
        "Install an FFmpeg build with this encoder, or render to another format.",
        lambda match: f"FFmpeg has no encoder {match.group('encoder')}.",
    ),
    _Rule(
        "ffmpeg_mux",
        re.compile(
            r"Could not write header.*|Invalid argument.*\b(?:mp4|webm|mov)\b", re.IGNORECASE
        ),
        "Use a codec and container that fit together, for example h264 in mp4.",
        lambda match: match.group(0).strip(),
    ),
    _Rule(
        "font_missing",
        re.compile(r"[Ff]ont(?: family)? ['\"]?(?P<font>[^'\"\n]+?)['\"]? (?:not found|is not in)"),
        "Install the font or choose one that is available.",
        lambda match: f"Font {match.group('font')} is not available.",
    ),
    _Rule(
        "opengl_context",
        re.compile(
            r"(?:OpenGL|GLX|EGL).*(?:context|display).*(?:fail|error|unavailable)", re.IGNORECASE
        ),
        "Use the cairo renderer, or provide a working OpenGL context.",
        lambda match: match.group(0).strip(),
    ),
    _Rule(
        "asset_missing",
        re.compile(
            r"FileNotFoundError: \[Errno 2\] No such file or directory: "
            rf"'(?!{_TEX_TOOLS}')(?P<path>[^']+)'"
            r"|could not find (?P<asset>\S+) at either of these locations"
        ),
        "Restore the file or fix its path; asset paths resolve from the project root.",
        lambda match: f"File {match.group('path') or match.group('asset')} does not exist.",
    ),
)


@dataclass(frozen=True, slots=True)
class DiagnoseResult:
    recognized: bool
    findings: list[Finding]
    artifacts: list[RuntimeArtifact]


def diagnose(task: DiagnoseTask, ctx: Context) -> DiagnoseResult:
    found = classify(task.text, location_from_text(task.text, ctx.project_root))
    return DiagnoseResult(
        recognized=any(f.code != "unclassified" for f in found), findings=found, artifacts=[]
    )


def classify(text: str, location: SourceLocation | None) -> list[Finding]:
    findings: list[Finding] = []
    seen: set[str] = set()
    for rule in _RULES:
        match = rule.pattern.search(text)
        if match is None or rule.code in seen:
            continue
        seen.add(rule.code)
        findings.append(_finding(rule.code, rule.message(match), _hint(rule, text), location))
    if not findings and text.strip():
        last_line = next(line.strip() for line in reversed(text.splitlines()) if line.strip())
        hint = "Read the full log around this line."
        findings.append(_finding("unclassified", last_line, hint, location))
    return findings[:MAX_FINDINGS]


def exception_findings(exc: BaseException, root: Path) -> tuple[list[Finding], str]:
    """Findings and the formatted traceback for an exception raised by user code."""

    trace = "".join(traceback.format_exception(exc))
    text = trace + _tex_log_excerpt(str(exc))
    return classify(text, location_from_exception(exc, root)), trace


def location_from_exception(exc: BaseException, root: Path) -> SourceLocation | None:
    if isinstance(exc, SyntaxError) and exc.filename and not exc.filename.startswith("<"):
        return SourceLocation(public_path(exc.filename, root), exc.lineno or 1, exc.offset)
    frames = [(f.filename, f.lineno or 1) for f in traceback.extract_tb(exc.__traceback__)]
    return _innermost(frames, root)


def location_from_text(text: str, root: Path) -> SourceLocation | None:
    frames = [
        (m.group("file"), int(m.group("line")), m.start()) for m in _TRACE_FILE.finditer(text)
    ]
    syntax = list(_SYNTAX_ERROR.finditer(text))
    if syntax:
        # The header directly above the caret block names the offending file and line.
        before = [frame for frame in frames if frame[2] < syntax[-1].start()]
        if before:
            file, line, _ = before[-1]
            return SourceLocation(public_path(root / file, root), line)
    return _innermost([(file, line) for file, line, _ in frames], root)


def _innermost(frames: list[tuple[str, int]], root: Path) -> SourceLocation | None:
    resolved = [(root / file, line) for file, line in frames if not file.startswith("<")]
    if not resolved:
        return None
    file, line = next(
        ((f, n) for f, n in reversed(resolved) if is_user_source(f, root)), resolved[-1]
    )
    return SourceLocation(public_path(file, root), line)


def _hint(rule: _Rule, text: str) -> str:
    if rule.code == "latex_error" and (context := _TEX_CONTEXT.search(text)):
        return f"Check the TeX near {_TEX_TOKEN.findall(context.group('context'))[-1]}."
    return rule.hint


def _finding(code: str, message: str, hint: str, location: SourceLocation | None) -> Finding:
    return Finding(
        code=code, severity=Severity.ERROR, message=message, hint=hint, location=location
    )


def _tex_log_excerpt(message: str) -> str:
    """Error lines of the .log that Manim names when a TeX compile fails."""

    match = _TEX_LOG.search(message)
    if match is None:
        return ""
    try:
        lines = Path(match.group("log")).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    excerpt: list[str] = []
    for index, line in enumerate(lines):
        if line.startswith("!"):
            block = lines[index : index + 12]
            end = next((i for i, item in enumerate(block) if item.startswith("l.")), len(block) - 1)
            excerpt.extend(block[: end + 1])
    return "\n" + "\n".join(excerpt)[:4096]
