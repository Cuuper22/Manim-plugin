from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path
from typing import Any, Mapping

from .errors import DirectorError
from .sample import generalized_fibonacci_source
from .themes import get_theme
from .util import atomic_write


def _slug(value: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-")
    return slug or "manim-project"


def _deterministic_seed(slug: str) -> int:
    digest = hashlib.sha256(f"manim-director:{slug}".encode("utf-8")).digest()
    return int.from_bytes(digest[:4], "big") & 0x7FFF_FFFF


def _starter_scene_source(name: str, theme: Mapping[str, Any]) -> str:
    return f'''"""A compact starter that uses Manim Director's composition grammar."""
from manim import *
from manim_director_runtime import Beat, DesignSystem, DirectedScene


class MainScene(DirectedScene):
    design = DesignSystem.from_mapping({{"theme": {dict(theme)!r}}})

    def construct(self):
        title = self.styled_text({name!r}, role="title")
        core = Circle(
            radius=0.72,
            color=self.design.color("primary"),
            fill_color=self.design.color("primary"),
            fill_opacity=0.16,
            **self.design.stroke("primary", width=1.15),
        )
        echo = Circle(
            radius=1.12,
            color=self.design.color("secondary"),
            stroke_opacity=0.55,
            stroke_width=self.design.stroke_width * 0.7,
        )
        visual = VGroup(echo, core)
        self.beat(
            Beat(
                intent="introduce",
                audience_question="What single idea should the audience see first?",
                takeaway="Build one visual relationship before adding detail.",
                focus="visual",
                visual_metaphor="A clear signal and its echo",
                transition="reveal",
            ),
            title,
            visual,
            keys=("title", "visual"),
            flow="column",
        )
        self.caption("One beat, one focus, one clean visual relationship.")
        self.wait(1)
'''


def scaffold(params: Mapping[str, Any]) -> dict[str, Any]:
    raw_root = Path(str(params.get("project_root", "."))).expanduser().resolve()
    raw_root.mkdir(parents=True, exist_ok=True)
    if not raw_root.is_dir():
        raise DirectorError("invalid_project_root", f"Cannot create project at: {raw_root}")
    name = str(params.get("name", raw_root.name or "Manim Project"))
    if not name.strip():
        raise DirectorError("invalid_project_name", "Project name cannot be empty")
    slug = _slug(name)
    theme_name = str(params.get("theme", "midnight"))
    theme = get_theme(theme_name)
    sample = bool(params.get("sample", False))
    force = bool(params.get("force", False))
    merge = bool(params.get("merge", False))
    try:
        seed = int(params.get("seed", _deterministic_seed(slug)))
    except (TypeError, ValueError) as exc:
        raise DirectorError("invalid_seed", "Project seed must be an integer") from exc
    if not (0 <= seed <= 0x7FFF_FFFF):
        raise DirectorError("invalid_seed", "Project seed must be between 0 and 2147483647")
    existing_entries = sorted(path.name for path in raw_root.iterdir())
    if existing_entries and not force and not merge:
        raise DirectorError(
            "project_not_empty", "Scaffold destination is not empty; use force or merge explicitly",
            {"entries": existing_entries[:100], "entry_count": len(existing_entries)},
        )
    scene_name = "GeneralizedFibonacciScene" if sample else "MainScene"
    files: dict[str, str] = {
        "director.yaml": f'''version: 1
schema: manim-director/v1
project:
  name: {json.dumps(name)}
  seed: {seed}
  source_dir: scenes
  asset_dir: assets
  output_dir: output
  media_dir: .manim-director/media
engine:
  backend: manim-ce
  source: scenes/main.py
  main_scene: {scene_name}
  compatible: ">=0.21,<0.22"
render:
  renderer: cairo
  profile: preview
  format: mp4
  transparent: false
  width: 1920
  height: 1080
  fps: 60
theme:
  preset: {theme_name}
  background: {json.dumps(theme["background"])}
  foreground: {json.dumps(theme["foreground"])}
  accent: {json.dumps(theme["accent"])}
  font: {json.dumps(theme["font"])}
safe_area:
  top: 0.05
  right: 0.05
  bottom: 0.08
  left: 0.05
direction:
  composition:
    density: spacious
    max_active: 4
    caption_lane: true
  typography:
    scale:
      hero: 64
      title: 44
      section: 36
      body: 30
      math: 48
      label: 24
      caption: 25
      micro: 18
  motion:
    continuation: morph
    contrast: lateral
    reveal: draw
    chapter: reset
  narrative:
    audience: curious general audience
    principle: one-idea-per-beat
captions:
  format: vtt
  burn_in: false
''',
        "manim.cfg": f'''[CLI]
media_dir = .manim-director/media
background_color = {theme["background"]}
progress_bar = display
''',
        "requirements.txt": "manim>=0.21,<0.22\n",
        "scenes/__init__.py": "\"\"\"Directed Manim scenes.\"\"\"\n",
        "assets/manifest.json": '{"version":1,"assets":[]}\n',
        ".gitignore": ".manim-director/media/\n.manim-director/tmp/\n__pycache__/\n*.pyc\n",
        "README.md": f'''# {name}

Preview the included scene in the plugin-managed runtime:

```bash
manim-director preview --scene {scene_name} --contact-sheet
```

Production render:

```bash
manim-director render --scene {scene_name} --profile production
```

The generated source remains ordinary Manim Python. When invoking Manim
directly, use the same environment in which Manim Director is installed so
`manim_director_runtime` is importable.
''',
    }
    files["scenes/main.py"] = generalized_fibonacci_source(theme=theme_name) if sample else _starter_scene_source(name, theme)
    directories = ("scenes", "assets", "output", ".manim-director/media", ".manim-director/tmp")
    conflicts = []
    for directory in directories:
        target = raw_root / directory
        if target.exists() and not target.is_dir():
            conflicts.append(str(target))
    for relative in files:
        target = raw_root / relative
        if target.exists() and not target.is_file():
            conflicts.append(str(target))
        parent = target.parent
        while parent != raw_root:
            if parent.exists() and not parent.is_dir():
                conflicts.append(str(parent))
                break
            parent = parent.parent
    if conflicts:
        raise DirectorError("scaffold_path_conflict", "Scaffold paths conflict with non-file or non-directory entries", {"paths": sorted(set(conflicts))})
    for directory in directories:
        (raw_root / directory).mkdir(parents=True, exist_ok=True)
    written: list[str] = []
    preserved: list[str] = []
    for rel, content in files.items():
        target = raw_root / rel
        if target.exists() and merge and not force:
            preserved.append(str(target))
            continue
        if rel == ".gitignore" and target.exists() and force:
            current = target.read_text(encoding="utf-8")
            missing = [line for line in content.splitlines() if line and line not in current.splitlines()]
            content = current.rstrip("\n") + ("\n" if current else "") + "\n".join(missing) + ("\n" if missing else "")
        atomic_write(target, content)
        written.append(str(target))
    return {
        "project_root": str(raw_root),
        "name": name,
        "slug": slug,
        "seed": seed,
        "theme": theme,
        "files": [str(raw_root / rel) for rel in sorted(files)],
        "written_files": written,
        "preserved_files": preserved,
        "sample_scene": scene_name,
    }
