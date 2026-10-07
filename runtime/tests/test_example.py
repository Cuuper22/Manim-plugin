"""The flagship example: its files agree with each other and every scene renders."""

from __future__ import annotations

import csv
import importlib.util
import json
import sys
from pathlib import Path

import pytest
import yaml

from conftest import requires_latex, requires_manim
from manim_director_runtime.captions import parse
from manim_director_runtime.inspection import discover
from manim_director_runtime.tasks import DiscoverTask
from manim_director_runtime.themes import COLOR_TOKENS, themes

EXAMPLE = Path(__file__).resolve().parents[2] / "examples" / "generalized-fibonacci"


def load(name: str) -> str:
    return (EXAMPLE / name).read_text(encoding="utf-8")


def test_spec_scenes_and_beats_match_the_source(ctx) -> None:
    spec = yaml.safe_load(load("director.yaml"))
    scenes = {s.name: s for s in discover(DiscoverTask([EXAMPLE / "scenes.py"]), ctx).scenes}
    assert [entry["class"] for entry in spec["scenes"]] == list(scenes)
    assert spec["engine"]["main_scene"] == "GeneralizedFibonacci"
    beats = [beat.id for beat in scenes["GeneralizedFibonacci"].beats]
    assert [beat["id"] for beat in spec["storyboard"]] == beats
    assert json.loads(load("expected/outputs.json"))["beats"] == beats
    assert spec["theme"] in themes()
    assert set(spec["direction"]["symbols"].values()) <= set(COLOR_TOKENS)


def test_narration_and_captions_share_beats_and_times() -> None:
    cues = json.loads(load("narration.json"))["cues"]
    captions = parse(load("captions.vtt"), vtt=True)
    assert [c.identifier for c in captions] == [cue["beat"] for cue in cues]
    assert [(c.start, c.end) for c in captions] == [
        (cue["start_seconds"], cue["end_seconds"]) for cue in cues
    ]
    assert [" ".join(c.text.split("\n")) for c in captions] == [cue["text"] for cue in cues]
    spec = yaml.safe_load(load("director.yaml"))
    assert cues[-1]["end_seconds"] == spec["brief"]["duration_seconds"]


def test_recorded_sequences_follow_their_recurrence() -> None:
    rows = list(csv.DictReader(load("data/sequences.csv").splitlines()))
    by_name: dict[str, list[dict[str, str]]] = {}
    for row in rows:
        by_name.setdefault(row["sequence"], []).append(row)
    assert set(by_name) == {"fibonacci", "lucas", "pell", "oscillator"}
    for series in by_name.values():
        first = series[0]
        p, q = float(first["p"]), float(first["q"])
        values = [float(row["value"]) for row in sorted(series, key=lambda r: int(r["n"]))]
        assert values[:2] == [float(first["x0"]), float(first["x1"])]
        triples = zip(values, values[1:], values[2:], strict=False)
        assert all(c == p * b + q * a for a, b, c in triples)


@pytest.fixture(scope="module")
def tex_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Shared by the scenes so common TeX is compiled once."""

    return tmp_path_factory.mktemp("tex")


@requires_manim
@requires_latex
@pytest.mark.parametrize(
    "scene", [entry["class"] for entry in yaml.safe_load(load("director.yaml"))["scenes"]]
)
def test_every_scene_renders_under_plain_manim(
    scene: str, tmp_path: Path, tex_dir: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from manim import tempconfig

    monkeypatch.setattr(sys, "dont_write_bytecode", True)  # keep the example tree clean
    spec = importlib.util.spec_from_file_location(f"_example_{scene}", EXAMPLE / "scenes.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    settings = {
        "media_dir": str(tmp_path / "media"),
        "tex_dir": str(tex_dir),
        "pixel_width": 320,
        "pixel_height": 180,
        "frame_rate": 10,
        "save_last_frame": True,
        "write_to_movie": False,
        "disable_caching": True,
        "progress_bar": "none",
        "verbosity": "ERROR",
    }
    with tempconfig(settings):
        rendered = getattr(module, scene)()
        rendered.render()
    luminance = rendered.renderer.get_frame()[..., :3].mean(axis=2)
    assert luminance.max() - luminance.min() > 100, "the last frame is blank"
