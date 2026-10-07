"""The `discover` operation: an AST-only scan of scene classes, sections and beats."""

from __future__ import annotations

import ast
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

from .errors import io_error
from .model import Finding, RuntimeArtifact, Severity, SourceLocation
from .tasks import DiscoverTask

if TYPE_CHECKING:
    from .protocol import Context

MANIM_SCENE_BASES = frozenset(
    {
        "Scene",
        "MovingCameraScene",
        "ThreeDScene",
        "SpecialThreeDScene",
        "VectorScene",
        "LinearTransformationScene",
        "ZoomedScene",
        "VoiceoverScene",
        "Slide",
    }
)
# Scene classes exported by manim_director_runtime; kept literal so discover never imports Manim.
DIRECTOR_SCENE_BASES = frozenset(
    {"DirectedScene", "DirectedMovingCameraScene", "DirectedThreeDScene"}
)
MAX_SCENES = 2000
MAX_FINDINGS = 200


@dataclass(frozen=True, slots=True)
class SectionInfo:
    name: str | None
    line: int


@dataclass(frozen=True, slots=True)
class BeatSpan:
    id: str | None
    line: int
    end_line: int


@dataclass(frozen=True, slots=True)
class SceneInfo:
    name: str
    file: str
    line: int
    end_line: int
    construct_line: int | None
    bases: list[str]
    doc: str | None
    theme: str | None
    sections: list[SectionInfo]
    beats: list[BeatSpan]


@dataclass(frozen=True, slots=True)
class DiscoverResult:
    truncated: bool  # the engine adds `files` and may also set this when it capped the file list
    scenes: list[SceneInfo]
    findings: list[Finding]
    artifacts: list[RuntimeArtifact]


@dataclass(frozen=True, slots=True)
class ParsedFile:
    path: Path
    classes: list[ast.ClassDef]


def discover(task: DiscoverTask, ctx: Context) -> DiscoverResult:
    parsed: list[ParsedFile] = []
    findings: list[Finding] = []
    for path in task.files:
        outcome = parse_module(path, ctx.relative(path))
        if isinstance(outcome, Finding):
            findings.append(outcome)
        else:
            parsed.append(
                ParsedFile(path, [n for n in outcome.body if isinstance(n, ast.ClassDef)])
            )
    scene_names = scene_class_names(parsed)
    scenes = [
        _scene_info(node, ctx.relative(item.path))
        for item in parsed
        for node in item.classes
        if node.name in scene_names
    ]
    truncated = len(scenes) > MAX_SCENES or len(findings) > MAX_FINDINGS
    return DiscoverResult(
        truncated=truncated,
        scenes=scenes[:MAX_SCENES],
        findings=findings[:MAX_FINDINGS],
        artifacts=[],
    )


def parse_module(path: Path, file: str) -> ast.Module | Finding:
    """Parse one file, or describe why it cannot be parsed; `file` is its public path."""

    try:
        source = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return Finding(
            code="source_encoding",
            severity=Severity.ERROR,
            message="The file is not valid UTF-8.",
            hint="Save the file as UTF-8.",
            location=SourceLocation(file=file, line=1),
        )
    except OSError as exc:
        raise io_error(path, exc) from exc
    try:
        return ast.parse(source, filename=str(path))
    except SyntaxError as exc:
        return Finding(
            code="python_syntax",
            severity=Severity.ERROR,
            message=exc.msg,
            hint="Fix the syntax error before rendering.",
            location=SourceLocation(file=file, line=exc.lineno or 1, column=exc.offset),
        )


def scene_class_names(parsed: list[ParsedFile]) -> set[str]:
    """Classes deriving, directly or through other listed classes, from a known scene base."""

    known = set(MANIM_SCENE_BASES | DIRECTOR_SCENE_BASES)
    found: set[str] = set()
    classes = [node for item in parsed for node in item.classes]
    changed = True
    while changed:
        changed = False
        for node in classes:
            if node.name not in found and {_final_name(b) for b in node.bases} & (known | found):
                found.add(node.name)
                changed = True
    return found


def _scene_info(node: ast.ClassDef, file: str) -> SceneInfo:
    construct = next(
        (
            item
            for item in node.body
            if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef))
            and item.name == "construct"
        ),
        None,
    )
    doc = ast.get_docstring(node)
    return SceneInfo(
        name=node.name,
        file=file,
        line=node.lineno,
        end_line=node.end_lineno or node.lineno,
        construct_line=construct.lineno if construct else None,
        bases=[_dotted_name(base) for base in node.bases],
        doc=doc.strip().splitlines()[0][:200] if doc and doc.strip() else None,
        theme=_theme_literal(node),
        sections=_sections(node),
        beats=_beats(node),
    )


def _theme_literal(node: ast.ClassDef) -> str | None:
    for item in node.body:
        if isinstance(item, ast.Assign):
            targets, value = item.targets, item.value
        elif isinstance(item, ast.AnnAssign) and item.value is not None:
            targets, value = [item.target], item.value
        else:
            continue
        named = any(isinstance(t, ast.Name) and t.id == "theme" for t in targets)
        if named and isinstance(value, ast.Constant) and isinstance(value.value, str):
            return value.value
    return None


def _sections(node: ast.ClassDef) -> list[SectionInfo]:
    calls = [
        call
        for call in ast.walk(node)
        if isinstance(call, ast.Call) and _is_self_method(call.func, "next_section")
    ]
    calls.sort(key=lambda call: (call.lineno, call.col_offset))
    return [SectionInfo(name=_label(call, "name"), line=call.lineno) for call in calls]


def _beats(node: ast.ClassDef) -> list[BeatSpan]:
    spans = []
    for statement in ast.walk(node):
        if not isinstance(statement, ast.With):
            continue
        for item in statement.items:
            call = item.context_expr
            if isinstance(call, ast.Call) and _is_self_method(call.func, "beat"):
                spans.append(
                    BeatSpan(
                        id=_label(call, "id"),
                        line=statement.lineno,
                        end_line=statement.end_lineno or statement.lineno,
                    )
                )
    spans.sort(key=lambda span: span.line)
    return spans


def _is_self_method(func: ast.expr, name: str) -> bool:
    return (
        isinstance(func, ast.Attribute)
        and func.attr == name
        and isinstance(func.value, ast.Name)
        and func.value.id == "self"
    )


def _label(call: ast.Call, keyword: str) -> str | None:
    """The literal first positional argument or `keyword=` string of a call, if any."""

    candidates = [*call.args[:1], *(kw.value for kw in call.keywords if kw.arg == keyword)]
    for value in candidates:
        if isinstance(value, ast.Constant) and isinstance(value.value, str):
            return value.value
    return None


def _final_name(node: ast.expr) -> str:
    return _dotted_name(node).rsplit(".", 1)[-1]


def _dotted_name(node: ast.expr) -> str:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return f"{_dotted_name(node.value)}.{node.attr}"
    if isinstance(node, ast.Subscript):
        return _dotted_name(node.value)
    return ""
