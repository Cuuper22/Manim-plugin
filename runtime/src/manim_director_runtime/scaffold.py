"""The `init` operation: create a project from a template or add one scene template."""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path

from . import catalog
from .errors import DirectorError, invalid_params, io_error
from .model import ArtifactKind, RuntimeArtifact, SceneRef
from .paths import atomic_target, slug
from .protocol import Context
from .tasks import InitTask
from .templates import SCENE_TEMPLATES

_GITIGNORE_LINES = (".manim-director/", "__pycache__/")
_MAIN_SCENE = "MainScene"


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


def init(task: InitTask, ctx: Context) -> InitResult:
    if task.mode == "add_scene":
        return _add_scene(task, ctx)
    return _create(task, ctx)


def _create(task: InitTask, ctx: Context) -> InitResult:
    root = ctx.project_root
    template = task.template or catalog.PROJECT_TEMPLATES[0]
    if template not in catalog.PROJECT_TEMPLATES:
        raise invalid_params(
            "template", "unknown template", allowed=list(catalog.PROJECT_TEMPLATES)
        )
    theme = task.theme or catalog.default_theme()
    if theme not in catalog.theme_names():
        raise invalid_params("theme", "unknown theme", allowed=catalog.theme_names())
    name = (task.name or root.name or "Manim Project").strip()
    project_slug = slug(name, fallback="manim-project")
    seed = task.seed if task.seed is not None else _seed_for(project_slug)

    files = {
        "director.yaml": _director_yaml(name, seed, theme),
        "manim.cfg": "[CLI]\nmedia_dir = .manim-director/media\n",
        "requirements.txt": "manim>=0.21,<0.22\n",
        "README.md": _readme(name),
        "scenes/main.py": _starter_scene(name),
    }
    if task.mode == "create":
        existing = sorted(path for path in files if (root / path).exists())
        if existing:
            raise DirectorError(
                "project_not_empty",
                f"{len(existing)} template file(s) already exist; pass force to overwrite them.",
                {"paths": existing},
            )
    written = [_write(root / path, content) for path, content in files.items()]
    written.append(_merge_gitignore(root / ".gitignore"))
    return InitResult(
        mode=task.mode,
        name=name,
        slug=project_slug,
        seed=seed,
        template=template,
        scene_template=None,
        theme=theme,
        scene=SceneRef(name=_MAIN_SCENE, file="scenes/main.py"),
        artifacts=[ctx.artifact(ArtifactKind.FILE, path) for path in written],
    )


def _add_scene(task: InitTask, ctx: Context) -> InitResult:
    assert task.scene_template is not None and task.source_dir is not None
    entry = SCENE_TEMPLATES.get(task.scene_template)
    if entry is None:
        raise invalid_params(
            "scene_template", "unknown scene template", allowed=list(SCENE_TEMPLATES)
        )
    scene_class, source = entry
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
        scene=SceneRef(name=scene_class, file=ctx.relative(target)),
        artifacts=[ctx.artifact(ArtifactKind.FILE, target)],
    )


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


def _director_yaml(name: str, seed: int, theme: str) -> str:
    return f"""version: 1
project:
  name: {json.dumps(name)}
  seed: {seed}
  source_dir: scenes
  asset_dir: assets
  output_dir: output
  media_dir: .manim-director/media
engine:
  source: scenes/main.py
  main_scene: {_MAIN_SCENE}
render:
  profile: preview
  renderer: cairo
  format: mp4
  transparent: false
  width: 1920
  height: 1080
  fps: 60
theme: {theme}
safe_area:
  top: 0.05
  right: 0.05
  bottom: 0.08
  left: 0.05
budgets:
  render_seconds: 900
"""


def _readme(name: str) -> str:
    return f"""# {name}

Render a quick draft, then look at it:

```bash
manim-director render --scene {_MAIN_SCENE} --profile draft
manim-director contact-sheet --scene {_MAIN_SCENE}
```

Production render:

```bash
manim-director render --scene {_MAIN_SCENE} --profile production
```

Scenes are ordinary Manim Python, so `manim -ql scenes/main.py {_MAIN_SCENE}` works too
from the environment where Manim Director is installed.
"""


def _starter_scene(name: str) -> str:
    return f'''"""{name}: one idea, one beat at a time."""

from manim import *
from manim_director_runtime import DirectedScene


class {_MAIN_SCENE}(DirectedScene):
    symbols = {{"a": "primary", "b": "secondary"}}

    def construct(self):
        question = self.math(r"(a + b)^2 = ?")
        with self.beat("question", transition="reveal"):
            self.title({name!r})
            self.place(question)
            self.caption("What happens when a sum is squared?")

        with self.beat("expand"):
            self.derive(
                r"(a + b)^2",
                (r"= (a + b)(a + b)", "definition"),
                (r"= a^2 + 2ab + b^2", "distribute and collect"),
                replaces=question,
            )
        self.wait()
'''
