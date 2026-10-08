"""The gallery films, packaged as `init` templates: each one scaffolds into a project that plans
for its viewer, renders, and passes pacing QA."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest
import yaml

from conftest import requires_latex, requires_manim
from manim_director_runtime import pacing, timeline
from manim_director_runtime.inspection import discover
from manim_director_runtime.project import load_style
from manim_director_runtime.protocol import Context
from manim_director_runtime.scaffold import init
from manim_director_runtime.tasks import DiscoverTask, InitTask

FILMS = ("concrete_first", "contrast", "misconception", "picture_to_formula", "zoom_detail")
SHORTEST, LONGEST = 20.0, 75.0  # seconds: one question, one aha, read at the viewer's pace


def scaffold(name: str, ctx: Context) -> dict:
    """Create the film's project in `ctx.project_root`; its director.yaml."""

    init(InitTask("create", name.replace("_", " "), name, None, None, None, None), ctx)
    return yaml.safe_load((ctx.project_root / "director.yaml").read_text(encoding="utf-8"))


@pytest.mark.parametrize("name", FILMS)
def test_each_film_plans_for_its_viewer(name: str, ctx: Context) -> None:
    project = scaffold(name, ctx)
    viewer = project["brief"]["viewer"]
    for key in ("who", "level", "knows", "new", "question", "wrong_guess", "aha", "payoff"):
        assert viewer.get(key), f"{name}: brief.viewer.{key} is missing"
    source = ctx.project_root / project["engine"]["source"]
    (scene,) = discover(DiscoverTask([source]), ctx).scenes
    assert scene.name == project["engine"]["main_scene"]
    assert [beat["id"] for beat in project["storyboard"]] == [beat.id for beat in scene.beats]
    assert sum(beat.get("aha") is True for beat in project["storyboard"]) == 1


@pytest.fixture(scope="module")
def tex_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Shared by the films so common TeX is compiled once."""

    return tmp_path_factory.mktemp("tex")


@requires_manim
@requires_latex
@pytest.mark.parametrize("name", FILMS)
def test_each_film_renders_a_frame_and_passes_pacing_qa(
    name: str, ctx: Context, tmp_path: Path, tex_dir: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from manim import tempconfig

    project, root = scaffold(name, ctx), ctx.project_root
    loaded = importlib.util.spec_from_file_location(f"_film_{name}", root / "scenes/main.py")
    assert loaded is not None and loaded.loader is not None
    module = importlib.util.module_from_spec(loaded)
    monkeypatch.setitem(sys.modules, loaded.name, module)  # the scene finds its director.yaml
    loaded.loader.exec_module(module)
    settings = {
        "media_dir": str(tmp_path / "media"),
        "tex_dir": str(tex_dir),
        "pixel_width": 854,  # Pango lays text out on a canvas of the frame's pixels
        "pixel_height": 480,
        "frame_rate": 15,
        "save_last_frame": True,
        "write_to_movie": False,
        "disable_caching": True,
        "progress_bar": "none",
        "verbosity": "ERROR",
    }
    scene_class = getattr(module, project["engine"]["main_scene"])
    with timeline.recording(root) as recorder, tempconfig(settings):
        scene = scene_class()
        scene.render()
    luminance = scene.renderer.get_frame()[..., :3].mean(axis=2)
    assert luminance.max() - luminance.min() > 100, "the last frame is blank"
    film = recorder.timeline(scene_class.__name__, scene.renderer.time)
    assert SHORTEST <= film.duration_seconds <= LONGEST
    style = load_style(root)
    assert style.viewer is not None
    budgets = pacing.settings(style.viewer.level, style.pacing)
    findings = pacing.check(film, budgets, style.viewer, style.storyboard_of(film.scene))
    assert [(f.code, f.beat, f.message) for f in findings] == []
