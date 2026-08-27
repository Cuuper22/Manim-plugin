from __future__ import annotations

import ast
from pathlib import Path
from typing import Any, Mapping

from .errors import DirectorError
from .util import confined_path, project_root


SCENE_BASES = {
    "Scene", "MovingCameraScene", "ThreeDScene", "SpecialThreeDScene", "VectorScene",
    "LinearTransformationScene", "ZoomedScene", "VoiceoverScene", "Slide",
}


def _name(node: ast.expr) -> str:
    if isinstance(node, ast.Name):
        return node.id
    if isinstance(node, ast.Attribute):
        return f"{_name(node.value)}.{node.attr}"
    if isinstance(node, ast.Subscript):
        return _name(node.value)
    return ""


def _literal(node: ast.AST | None) -> Any:
    if node is None:
        return None
    try:
        return ast.literal_eval(node)
    except (ValueError, TypeError):
        return None


def _call_info(node: ast.Call) -> dict[str, Any]:
    return {
        "name": _name(node.func),
        "line": node.lineno,
        "end_line": getattr(node, "end_lineno", node.lineno),
        "positional_count": len(node.args),
        "keywords": {kw.arg or "**": _literal(kw.value) for kw in node.keywords},
    }


def _scene_info(node: ast.ClassDef, source_path: Path) -> dict[str, Any]:
    construct = next((n for n in node.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef)) and n.name == "construct"), None)
    calls = [_call_info(n) for n in ast.walk(construct or node) if isinstance(n, ast.Call)]
    assignments: list[dict[str, Any]] = []
    for item in ast.walk(construct or node):
        if isinstance(item, (ast.Assign, ast.AnnAssign)):
            targets = item.targets if isinstance(item, ast.Assign) else [item.target]
            value = item.value
            for target in targets:
                if isinstance(target, ast.Name):
                    assignments.append({
                        "name": target.id,
                        "line": item.lineno,
                        "constructor": _name(value.func) if isinstance(value, ast.Call) else None,
                    })
    sections: list[dict[str, Any]] = []
    for call in calls:
        if call["name"].endswith("next_section"):
            original = next((n for n in ast.walk(construct or node) if isinstance(n, ast.Call) and n.lineno == call["line"]), None)
            section_name = _literal(original.args[0]) if original and original.args else None
            sections.append({"name": section_name, "line": call["line"]})
    return {
        "name": node.name,
        "file": str(source_path),
        "line": node.lineno,
        "end_line": getattr(node, "end_lineno", node.lineno),
        "bases": [_name(base) for base in node.bases],
        "docstring": ast.get_docstring(node),
        "has_construct": construct is not None,
        "construct_line": construct.lineno if construct else None,
        "play_calls": [c for c in calls if c["name"].endswith(".play") or c["name"] == "play"],
        "wait_calls": [c for c in calls if c["name"].endswith(".wait") or c["name"] == "wait"],
        "sections": sections,
        "objects": assignments,
        "call_count": len(calls),
    }


def inspect_file(path: Path) -> dict[str, Any]:
    try:
        source = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        raise DirectorError("source_encoding", f"Scene file is not valid UTF-8: {path}") from exc
    try:
        tree = ast.parse(source, filename=str(path))
    except SyntaxError as exc:
        return {
            "file": str(path), "valid_python": False, "scenes": [],
            "syntax_error": {"message": exc.msg, "line": exc.lineno, "column": exc.offset, "text": exc.text},
        }
    imports: list[dict[str, Any]] = []
    for node in tree.body:
        if isinstance(node, ast.Import):
            imports.extend({"module": alias.name, "name": None, "alias": alias.asname, "line": node.lineno} for alias in node.names)
        elif isinstance(node, ast.ImportFrom):
            imports.extend({"module": node.module, "name": alias.name, "alias": alias.asname, "line": node.lineno} for alias in node.names)
    classes = [n for n in tree.body if isinstance(n, ast.ClassDef)]
    scene_names: set[str] = set()
    changed = True
    while changed:
        changed = False
        for cls in classes:
            bases = {_name(base).split(".")[-1] for base in cls.bases}
            if cls.name not in scene_names and (bases & SCENE_BASES or bases & scene_names):
                scene_names.add(cls.name)
                changed = True
    scenes = [_scene_info(cls, path) for cls in classes if cls.name in scene_names]
    return {
        "file": str(path), "valid_python": True, "syntax_error": None,
        "scenes": scenes, "imports": imports, "line_count": source.count("\n") + 1,
    }


def discover(params: Mapping[str, Any]) -> dict[str, Any]:
    root = project_root(params)
    source_dir = confined_path(root, str(params.get("source_dir", "scenes")))
    if not source_dir.exists():
        return {"project_root": str(root), "source_dir": str(source_dir), "files": [], "scenes": [], "errors": []}
    pattern = str(params.get("pattern", "*.py"))
    recursive = bool(params.get("recursive", True))
    paths = sorted(source_dir.rglob(pattern) if recursive else source_dir.glob(pattern))
    reports = [inspect_file(path) for path in paths if path.is_file()]
    return {
        "project_root": str(root), "source_dir": str(source_dir),
        "files": [str(p) for p in paths if p.is_file()],
        "scenes": [scene for report in reports for scene in report["scenes"]],
        "errors": [{"file": report["file"], **report["syntax_error"]} for report in reports if report["syntax_error"]],
    }


def inspect(params: Mapping[str, Any]) -> dict[str, Any]:
    root = project_root(params)
    path = confined_path(root, str(params.get("path", params.get("scene_file", "scenes/main.py"))), must_exist=True)
    if path.suffix != ".py":
        raise DirectorError("invalid_scene_file", "Scene inspection requires a Python file")
    report = inspect_file(path)
    scene_filter = params.get("scene")
    if scene_filter:
        report["scenes"] = [scene for scene in report["scenes"] if scene["name"] == scene_filter]
        if not report["scenes"]:
            raise DirectorError("scene_not_found", f"Scene {scene_filter!r} was not found in {path}")
    return report
