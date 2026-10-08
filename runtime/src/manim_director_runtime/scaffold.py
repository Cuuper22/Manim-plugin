"""The `init` operation: create a project from a packaged template, or add a template's scene
to an existing project.

Templates are ordinary files under `data/templates/<name>/`, each with a runnable
`scenes/main.py`. Files in `data/templates/_shared/` belong to every template that does not
ship its own copy. Only YAML and Markdown files are filled in (`{{name}}`, `{{seed}}`,
`{{theme}}`, `{{scene}}`); scene sources are copied byte for byte.
"""

from __future__ import annotations

import ast
import hashlib
import json
import re
from dataclasses import dataclass
from functools import cache
from importlib import resources
from importlib.resources.abc import Traversable
from pathlib import Path
from typing import TYPE_CHECKING

from .errors import DirectorError, io_error
from .inspection import DIRECTOR_SCENE_BASES
from .model import ArtifactKind, RuntimeArtifact, SceneRef
from .paths import atomic_target, ensure_dir, slug
from .tasks import InitTask
from .themes import themes

if TYPE_CHECKING:
    from .protocol import Context

DEFAULT_TEMPLATE = "explainer"
SCENE_FILE = "scenes/main.py"
SPEC_FILE = "director.yaml"
_SHARED = "_shared"
_FILLED_SUFFIXES = frozenset({".yaml", ".md"})
_GITIGNORE_LINES = (".manim-director/", "__pycache__/")
_PLACEHOLDER = re.compile(r"\{\{(\w+)\}\}")


@dataclass(frozen=True, slots=True)
class InitResult:
    mode: str
    name: str | None
    slug: str | None
    seed: int | None
    template: str | None
    scene_template: str | None
    theme: str | None
    scene: SceneRef
    artifacts: list[RuntimeArtifact]


@cache
def templates() -> tuple[str, ...]:
    """Packaged template names, the default first."""

    names = [node.name for node in _root().iterdir() if node.is_dir() and node.name != _SHARED]
    return tuple(sorted(names, key=lambda name: (name != DEFAULT_TEMPLATE, name)))


def scene_source(template: str) -> str:
    return _files(template)[SCENE_FILE].read_text(encoding="utf-8")


def scene_class(source: str) -> str:
    """The template's scene: its first class built directly on a DirectedScene base."""

    for node in ast.parse(source).body:
        if isinstance(node, ast.ClassDef) and any(
            isinstance(base, ast.Name) and base.id in DIRECTOR_SCENE_BASES for base in node.bases
        ):
            return node.name
    raise AssertionError("every packaged template defines a DirectedScene subclass")


def init(task: InitTask, ctx: Context) -> InitResult:
    if task.mode == "add_scene":
        return _add_scene(task, ctx)
    return _create(task, ctx)


def _create(task: InitTask, ctx: Context) -> InitResult:
    root = ctx.project_root
    template = task.template or DEFAULT_TEMPLATE
    if template not in templates():
        raise _unknown("template", template, list(templates()))
    theme = task.theme or next(iter(themes()))
    if theme not in themes():
        raise _unknown("theme", theme, list(themes()))
    name = (task.name or root.name or "Manim Project").strip()
    project_slug = slug(name, fallback="manim-project")
    seed = task.seed if task.seed is not None else _seed_for(project_slug)
    sources = _files(template)
    scene = scene_class(sources[SCENE_FILE].read_text(encoding="utf-8"))
    values = {"name": name, "seed": seed, "theme": theme, "scene": scene}

    if task.mode == "create":
        existing = sorted(path for path in sources if (root / path).exists())
        if existing:
            raise _occupied(existing)
    written = [
        _write(root / path, _fill(path, source.read_text(encoding="utf-8"), values))
        for path, source in sources.items()
    ]
    written.append(_merge_gitignore(root / ".gitignore"))
    return InitResult(
        mode=task.mode,
        name=name,
        slug=project_slug,
        seed=seed,
        template=template,
        scene_template=None,
        theme=theme,
        scene=SceneRef(name=scene, file=SCENE_FILE),
        artifacts=[ctx.artifact(ArtifactKind.FILE, path) for path in written],
    )


def _add_scene(task: InitTask, ctx: Context) -> InitResult:
    assert task.scene_template is not None and task.source_dir is not None
    if task.scene_template not in templates():
        raise _unknown("scene_template", task.scene_template, list(templates()))
    source = scene_source(task.scene_template)
    source_dir = ctx.require_inside(task.source_dir, "source_dir")
    target = source_dir / f"{task.scene_template}.py"
    # Neither write follows a symlink at `target`: exclusive creation refuses one, and the
    # rename behind `_write` replaces the link itself.
    try:
        if task.force:
            _write(target, source)
        else:
            ensure_dir(source_dir)
            with target.open("x", encoding="utf-8") as handle:
                handle.write(source)
    except FileExistsError:
        existing = ctx.relative(target)
        raise DirectorError(
            "project_not_empty",
            f"{existing} already exists; force (--force) would replace it with the template's "
            "scene, and keeps no copy.",
            {"paths": [existing]},
        ) from None
    except OSError as exc:
        raise io_error(target, exc) from exc
    return InitResult(
        mode=task.mode,
        name=None,
        slug=None,
        seed=None,
        template=None,
        scene_template=task.scene_template,
        theme=None,
        scene=SceneRef(name=scene_class(source), file=ctx.relative(target)),
        artifacts=[ctx.artifact(ArtifactKind.FILE, target)],
    )


def _occupied(existing: list[str]) -> DirectorError:
    """Name what force would replace and, in a project, how to add a scene instead."""

    replaced = f"force (--force) would replace {_listing(existing)}, and keeps no copy."
    if SPEC_FILE in existing:
        add = "add a scene with scene_template (--scene-template)"
        message = f"This directory is already a project; {add}. {replaced}"
    else:
        message = f"This directory already has files the template writes; {replaced}"
    return DirectorError("project_not_empty", message, {"paths": existing})


def _listing(items: list[str]) -> str:
    return f"{', '.join(items[:-1])} and {items[-1]}" if len(items) > 1 else items[0]


def _unknown(field: str, value: str, allowed: list[str]) -> DirectorError:
    """invalid_params whose message itself names the choices (CLI output shows no data)."""

    what = field.replace("_", " ")
    return DirectorError(
        "invalid_params",
        f"Unknown {what} {value!r}; choose one of {', '.join(allowed)}.",
        {"field": field, "reason": f"unknown {what}", "allowed": allowed},
    )


def _root() -> Traversable:
    return resources.files(__package__).joinpath("data", "templates")


def _files(template: str) -> dict[str, Traversable]:
    """Project-relative path -> packaged file, sorted by path."""

    root = _root()
    return dict(sorted({**_walk(root / _SHARED), **_walk(root / template)}.items()))


def _walk(directory: Traversable, prefix: str = "") -> dict[str, Traversable]:
    found: dict[str, Traversable] = {}
    for node in directory.iterdir():
        path = f"{prefix}{node.name}"
        if node.is_dir():
            if node.name != "__pycache__":
                found.update(_walk(node, f"{path}/"))
        else:
            found[path] = node
    return found


def _fill(path: str, text: str, values: dict[str, str | int]) -> str:
    if Path(path).suffix not in _FILLED_SUFFIXES:
        return text
    spell = _yaml_scalar if path.endswith(".yaml") else str
    # One pass, so a value that itself reads like a placeholder is written as given.
    return _PLACEHOLDER.sub(
        lambda match: spell(values[match[1]]) if match[1] in values else match[0], text
    )


def _yaml_scalar(value: str | int) -> str:
    import yaml  # here, not at import: the bridge must start without PyYAML for doctor to say so

    plain = str(value)
    try:
        round_trips = yaml.safe_load(plain) == value
    except yaml.YAMLError:
        round_trips = False
    return plain if round_trips else json.dumps(value, ensure_ascii=False)


def _seed_for(project_slug: str) -> int:
    digest = hashlib.sha256(f"manim-director:{project_slug}".encode()).digest()
    return int.from_bytes(digest[:4], "big") & 0x7FFF_FFFF


def _write(path: Path, content: str) -> Path:
    with atomic_target(path) as temp:
        temp.write_text(content, encoding="utf-8")
    return path


def _merge_gitignore(path: Path) -> Path:
    current = path.read_text(encoding="utf-8") if path.exists() else ""
    present = {line.strip() for line in current.splitlines()}
    missing = [line for line in _GITIGNORE_LINES if line not in present]
    if missing:
        separator = "" if not current or current.endswith("\n") else "\n"
        _write(path, current + separator + "\n".join(missing) + "\n")
    return path
