from __future__ import annotations

from dataclasses import replace
from pathlib import Path

import pytest
import yaml

from conftest import as_json
from manim_director_runtime.errors import DirectorError
from manim_director_runtime.inspection import discover
from manim_director_runtime.scaffold import init
from manim_director_runtime.tasks import DiscoverTask, InitTask


def create(name: str | None = "Sequence Lab", **fields) -> InitTask:
    return InitTask(
        mode=fields.get("mode", "create"),
        name=name,
        template=fields.get("template"),
        scene_template=None,
        theme=fields.get("theme"),
        seed=fields.get("seed"),
        source_dir=None,
    )


def test_create_writes_a_renderable_project(project: Path, ctx) -> None:
    result = as_json(init(create(), ctx))
    assert {key: result[key] for key in ("mode", "name", "slug", "template", "theme")} == {
        "mode": "create",
        "name": "Sequence Lab",
        "slug": "sequence-lab",
        "template": "explainer",
        "theme": "midnight",
    }
    assert result["seed"] == as_json(init(create(mode="overwrite"), ctx))["seed"]
    assert result["scene"] == {"name": "MainScene", "file": "scenes/main.py"}
    paths = [artifact["path"] for artifact in result["artifacts"]]
    assert paths == [
        "director.yaml",
        "manim.cfg",
        "requirements.txt",
        "README.md",
        "scenes/main.py",
        ".gitignore",
    ]
    spec = yaml.safe_load((project / "director.yaml").read_text())
    assert spec["theme"] == "midnight" and spec["project"]["name"] == "Sequence Lab"
    assert "memory_mb" not in spec.get("budgets", {})
    assert not (project / "assets/manifest.json").exists()
    scenes = discover(DiscoverTask(files=[project / "scenes/main.py"]), ctx).scenes
    assert [scene.name for scene in scenes] == ["MainScene"]


def test_create_refuses_to_overwrite_and_overwrite_merges_gitignore(project: Path, ctx) -> None:
    (project / "README.md").write_text("mine\n")
    (project / ".gitignore").write_text("build/\n__pycache__/")
    with pytest.raises(DirectorError) as raised:
        init(create(), ctx)
    assert raised.value.code == "project_not_empty"
    assert raised.value.data == {"paths": ["README.md"]}
    assert not (project / "director.yaml").exists()
    init(create(mode="overwrite", theme="paper", seed=42), ctx)
    assert (project / ".gitignore").read_text() == "build/\n__pycache__/\n.manim-director/\n"
    assert "# Sequence Lab" in (project / "README.md").read_text()
    assert "theme: paper" in (project / "director.yaml").read_text()


def test_unknown_template_or_theme_lists_the_choices(ctx) -> None:
    with pytest.raises(DirectorError) as raised:
        init(create(theme="sepia"), ctx)
    assert raised.value.data["field"] == "theme"
    assert raised.value.data["allowed"][0] == "midnight"
    with pytest.raises(DirectorError) as raised:
        init(create(template="slides"), ctx)
    assert raised.value.data == {
        "field": "template",
        "reason": "unknown template",
        "allowed": ["explainer"],
    }


def test_add_scene_writes_one_file_exclusively(project: Path, ctx) -> None:
    task = InitTask(
        mode="add_scene",
        name=None,
        template=None,
        scene_template="equation_derivation",
        theme=None,
        seed=None,
        source_dir=project / "scenes",
    )
    result = as_json(init(task, ctx))
    assert result["scene"] == {
        "name": "EquationDerivationScene",
        "file": "scenes/equation_derivation.py",
    }
    assert all(result[key] is None for key in ("name", "slug", "seed", "template", "theme"))
    assert [path.name for path in project.iterdir()] == ["scenes"]
    compile((project / "scenes/equation_derivation.py").read_text(), "scene.py", "exec")
    with pytest.raises(DirectorError) as raised:
        init(task, ctx)
    assert raised.value.data == {"paths": ["scenes/equation_derivation.py"]}
    (project / "scenes/equation_derivation.py").write_text("edited\n")
    init(replace(task, force=True), ctx)
    assert "EquationDerivationScene" in (project / "scenes/equation_derivation.py").read_text()
