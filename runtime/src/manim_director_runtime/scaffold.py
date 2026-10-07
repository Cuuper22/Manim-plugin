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
from dataclasses import dataclass
from functools import cache
from importlib import resources
from importlib.resources.abc import Traversable
from pathlib import Path
from typing import TYPE_CHECKING

import yaml

from .errors import DirectorError, invalid_params, io_error
from .inspection import DIRECTOR_SCENE_BASES
from .model import ArtifactKind, RuntimeArtifact, SceneRef
from .paths import atomic_target, slug
from .tasks import InitTask
from .themes import themes

if TYPE_CHECKING:
    from .protocol import Context

DEFAULT_TEMPLATE = "explainer"
SCENE_FILE = "scenes/main.py"
_SHARED = "_shared"
_FILLED_SUFFIXES = frozenset({".yaml", ".md"})
_GITIGNORE_LINES = (".manim-director/", "__pycache__/")


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
        raise invalid_params("template", "unknown template", allowed=list(templates()))
    theme = task.theme or next(iter(themes()))
    if theme not in themes():
        raise invalid_params("theme", "unknown theme", allowed=list(themes()))
    name = (task.name or root.name or "Manim Project").strip()
    project_slug = slug(name, fallback="manim-project")
    seed = task.seed if task.seed is not None else _seed_for(project_slug)
    sources = _files(template)
    scene = scene_class(sources[SCENE_FILE].read_text(encoding="utf-8"))
    values = {"name": name, "seed": seed, "theme": theme, "scene": scene}

    if task.mode == "create":
        existing = sorted(path for path in sources if (root / path).exists())
        if existing:
            raise DirectorError(
                "project_not_empty",
                f"{len(existing)} template file(s) already exist; pass force to overwrite them.",
                {"paths": existing},
            )
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
        raise invalid_params("scene_template", "unknown scene template", allowed=list(templates()))
    source = scene_source(task.scene_template)
    source_dir = ctx.require_inside(task.source_dir, "source_dir")
    target = source_dir / f"{task.scene_template}.py"
    try:
        source_dir.mkdir(parents=True, exist_ok=True)
        with target.open("w" if task.force else "x", encoding="utf-8") as handle:
            handle.write(source)
    except FileExistsError:
        raise DirectorError(
            "project_not_empty",
            f"{ctx.relative(target)} already exists; pass force to overwrite it.",
            {"paths": [ctx.relative(target)]},
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
    for key, value in values.items():
        text = text.replace(f"{{{{{key}}}}}", spell(value))
    return text


def _yaml_scalar(value: str | int) -> str:
    plain = str(value)
    return plain if yaml.safe_load(plain) == value else json.dumps(value, ensure_ascii=False)


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
