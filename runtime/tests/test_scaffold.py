from __future__ import annotations

from dataclasses import replace
from pathlib import Path

import pytest
import yaml

from conftest import as_json
from manim_director_runtime.errors import DirectorError
from manim_director_runtime.inspection import discover
from manim_director_runtime.scaffold import init, scene_class, scene_source, templates
from manim_director_runtime.tasks import DiscoverTask, InitTask

TEMPLATES = ("explainer", "derivation", "geometry", "graph", "vertical_short")


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


def add_scene(project: Path, template: str) -> InitTask:
    return InitTask(
        mode="add_scene",
        name=None,
        template=None,
        scene_template=template,
        theme=None,
        seed=None,
        source_dir=project / "scenes",
    )


def test_templates_are_listed_with_the_default_first() -> None:
    assert templates() == TEMPLATES


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
    assert result["scene"] == {"name": "GeometricSeries", "file": "scenes/main.py"}
    paths = [artifact["path"] for artifact in result["artifacts"]]
    assert paths == ["README.md", "director.yaml", "manim.cfg", "scenes/main.py", ".gitignore"]
    spec = yaml.safe_load((project / "director.yaml").read_text())
    assert spec["theme"] == "midnight" and spec["project"]["name"] == "Sequence Lab"
    assert spec["project"]["seed"] == result["seed"]
    assert "budgets" not in spec
    assert not (project / "assets/manifest.json").exists()
    assert "`GeometricSeries`" in (project / "README.md").read_text()
    scenes = discover(DiscoverTask(files=[project / "scenes/main.py"]), ctx).scenes
    assert [scene.name for scene in scenes] == ["GeometricSeries"]


@pytest.mark.parametrize("template", TEMPLATES)
def test_each_template_fills_a_consistent_project(template: str, project: Path, ctx) -> None:
    result = init(create(template=template, theme="paper", seed=11), ctx)
    spec = yaml.safe_load((project / "director.yaml").read_text())
    assert spec["version"] == 1 and spec["theme"] == "paper" and spec["project"]["seed"] == 11
    assert spec["engine"] == {"source": "scenes/main.py", "main_scene": result.scene.name}
    for path in ("director.yaml", "README.md"):
        assert "{{" not in (project / path).read_text(), path
    (scene,) = discover(DiscoverTask(files=[project / "scenes/main.py"]), ctx).scenes
    assert scene.name == result.scene.name and scene.theme is None
    assert [beat.id for beat in scene.beats] == [beat["id"] for beat in spec["storyboard"]]


def test_values_that_yaml_would_misread_are_quoted(project: Path, ctx) -> None:
    for name in (
        "Sequences: a tour",
        "yes",
        "1729",
        "{{theme}} notes",
        "@handle [draft",
        "Plain Title",
    ):
        init(create(name, mode="overwrite"), ctx)
        assert yaml.safe_load((project / "director.yaml").read_text())["project"]["name"] == name
    assert "name: Plain Title\n" in (project / "director.yaml").read_text()


def test_a_template_file_replaces_the_shared_one(project: Path, ctx) -> None:
    init(create(template="vertical_short"), ctx)
    assert "pixel_height = 1920" in (project / "manim.cfg").read_text()
    readme = (project / "README.md").read_text()
    assert "manim -r 540,960 scenes/main.py TriangularNumbers" in readme


def test_create_refuses_to_overwrite_and_overwrite_merges_gitignore(project: Path, ctx) -> None:
    (project / "README.md").write_text("mine\n")
    (project / ".gitignore").write_text("build/\n__pycache__/")
    with pytest.raises(DirectorError) as raised:
        init(create(), ctx)
    assert raised.value.code == "project_not_empty"
    assert raised.value.data == {"paths": ["README.md"]}
    assert raised.value.message == (
        "This directory already has files the template writes; "
        "force (--force) would replace README.md, and keeps no copy."
    )
    assert not (project / "director.yaml").exists()
    init(create(mode="overwrite", theme="paper", seed=42), ctx)
    assert (project / ".gitignore").read_text() == "build/\n__pycache__/\n.manim-director/\n"
    assert "# Sequence Lab" in (project / "README.md").read_text()
    assert "theme: paper" in (project / "director.yaml").read_text()


def test_create_in_a_project_points_at_adding_a_scene(project: Path, ctx) -> None:
    init(create(), ctx)
    with pytest.raises(DirectorError) as raised:
        init(create(), ctx)
    assert raised.value.message == (
        "This directory is already a project; add a scene with scene_template "
        "(--scene-template). force (--force) would replace README.md, director.yaml, "
        "manim.cfg and scenes/main.py, and keeps no copy."
    )


def test_unknown_template_or_theme_lists_the_choices(project: Path, ctx) -> None:
    with pytest.raises(DirectorError) as raised:
        init(create(theme="sepia"), ctx)
    assert raised.value.data["field"] == "theme"
    assert raised.value.data["allowed"][0] == "midnight"
    with pytest.raises(DirectorError) as raised:
        init(create(template="slides"), ctx)
    assert raised.value.data == {
        "field": "template",
        "reason": "unknown template",
        "allowed": list(TEMPLATES),
    }
    # The message names the choices too: the CLI's human output shows no error data.
    assert (
        raised.value.message == f"Unknown template 'slides'; choose one of {', '.join(TEMPLATES)}."
    )
    with pytest.raises(DirectorError) as raised:
        init(add_scene(project, "equation_derivation"), ctx)
    assert raised.value.data["field"] == "scene_template"


def test_add_scene_writes_one_file_exclusively(project: Path, ctx) -> None:
    task = add_scene(project, "derivation")
    result = as_json(init(task, ctx))
    assert result["scene"] == {"name": "QuadraticFormula", "file": "scenes/derivation.py"}
    assert all(result[key] is None for key in ("name", "slug", "seed", "template", "theme"))
    assert [path.name for path in project.iterdir()] == ["scenes"]
    assert (project / "scenes/derivation.py").read_text() == scene_source("derivation")
    with pytest.raises(DirectorError) as raised:
        init(task, ctx)
    assert raised.value.data == {"paths": ["scenes/derivation.py"]}
    assert raised.value.message == (
        "scenes/derivation.py already exists; force (--force) would replace it with the "
        "template's scene, and keeps no copy."
    )
    (project / "scenes/derivation.py").write_text("edited\n")
    init(replace(task, force=True), ctx)
    assert scene_class((project / "scenes/derivation.py").read_text()) == "QuadraticFormula"


def test_add_scene_never_writes_through_a_symlink(project: Path, ctx) -> None:
    outside = project.parent / "profile"
    outside.write_text("mine\n")
    target = project / "scenes/derivation.py"
    target.parent.mkdir()
    for link in (outside, project.parent / "missing.py"):
        target.symlink_to(link)
        with pytest.raises(DirectorError) as raised:
            init(add_scene(project, "derivation"), ctx)
        assert raised.value.code == "project_not_empty"
        target.unlink()
    assert not (project.parent / "missing.py").exists()
    target.symlink_to(outside)
    init(replace(add_scene(project, "derivation"), force=True), ctx)
    assert not target.is_symlink() and target.read_text() == scene_source("derivation")
    assert outside.read_text() == "mine\n"


def test_add_scene_refuses_a_source_dir_linked_outside(project: Path, ctx) -> None:
    outside = project.parent / "elsewhere"
    outside.mkdir()
    (project / "scenes").symlink_to(outside)
    with pytest.raises(DirectorError) as raised:
        init(add_scene(project, "graph"), ctx)
    assert raised.value.data == {"field": "source_dir", "reason": "outside_project"}
    assert not any(outside.iterdir())
