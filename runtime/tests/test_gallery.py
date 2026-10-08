"""The gallery: every scene renders a frame, keeps to its plan, and passes pacing QA."""

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
from manim_director_runtime.tasks import DiscoverTask

GALLERY = Path(__file__).resolve().parents[2] / "examples" / "gallery"
PROJECTS = sorted(path.parent.name for path in GALLERY.glob("*/director.yaml"))
SHORTEST, LONGEST = 20.0, 45.0  # seconds: one question, one aha


def spec(name: str) -> dict:
    return yaml.safe_load((GALLERY / name / "director.yaml").read_text(encoding="utf-8"))


def test_the_gallery_has_one_project_per_pattern() -> None:
    assert PROJECTS == [
        "concrete_first",
        "contrast",
        "misconception",
        "picture_to_formula",
        "zoom_detail",
    ]


@pytest.mark.parametrize("name", PROJECTS)
def test_each_project_plans_for_its_viewer(name: str, ctx) -> None:
    project = spec(name)
    viewer = project["brief"]["viewer"]
    for key in ("who", "level", "knows", "new", "question", "wrong_guess", "aha", "payoff"):
        assert viewer.get(key), f"{name}: brief.viewer.{key} is missing"
    source = GALLERY / name / project["engine"]["source"]
    (scene,) = discover(DiscoverTask([source]), ctx).scenes
    assert scene.name == project["engine"]["main_scene"]
    assert [beat["id"] for beat in project["storyboard"]] == [beat.id for beat in scene.beats]
    assert sum(beat.get("aha") is True for beat in project["storyboard"]) == 1


@pytest.fixture(scope="module")
def tex_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Shared by the scenes so common TeX is compiled once."""

    return tmp_path_factory.mktemp("tex")


@requires_manim
@requires_latex
@pytest.mark.parametrize("name", PROJECTS)
def test_each_scene_renders_a_frame_and_passes_pacing_qa(
    name: str, tmp_path: Path, tex_dir: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from manim import tempconfig

    monkeypatch.setattr(sys, "dont_write_bytecode", True)  # keep the gallery tree clean
    project = spec(name)
    root = GALLERY / name
    loaded = importlib.util.spec_from_file_location(f"_gallery_{name}", root / "scenes/main.py")
    assert loaded is not None and loaded.loader is not None
    module = importlib.util.module_from_spec(loaded)
    monkeypatch.setitem(sys.modules, loaded.name, module)  # the scene finds its director.yaml
    loaded.loader.exec_module(module)
    settings = {
        "media_dir": str(tmp_path / "media"),
        "tex_dir": str(tex_dir),
        "pixel_width": 320,
        "pixel_height": 180,
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
