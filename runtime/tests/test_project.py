from __future__ import annotations

from pathlib import Path

import pytest

from manim_director_runtime.errors import CompositionError
from manim_director_runtime.project import (
    ProjectStyle,
    StoryBeat,
    Viewer,
    find_spec,
    load_style,
)
from manim_director_runtime.tasks import SAFE_AREA_DEFAULT, SafeArea


def write(path: Path, text: str) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def test_the_nearest_director_yaml_wins(tmp_path: Path) -> None:
    write(tmp_path / "director.yaml", "theme: paper\n")
    inner = write(tmp_path / "chapter" / "director.yaml", "theme: chalkboard\n")
    scenes = tmp_path / "chapter" / "scenes" / "deep"
    scenes.mkdir(parents=True)
    assert find_spec(scenes) == inner
    assert load_style(scenes).theme == "chalkboard"
    assert load_style(tmp_path).theme == "paper"


def test_settings_are_read_with_defaults(tmp_path: Path) -> None:
    write(
        tmp_path / "director.yaml",
        "theme: {preset: contrast, background: '#000'}\n"
        "safe_area: {top: 0.1}\n"
        "direction:\n  symbols: {'\\phi': accent, x: '#123456'}\n",
    )
    style = load_style(tmp_path)
    assert style.theme == "contrast"  # legacy mapping: only `preset` counts
    assert style.safe_area == SafeArea(top=0.1, right=0.05, bottom=0.08, left=0.05)
    assert style.symbols == {r"\phi": "accent", "x": "#123456"}


def test_the_viewer_model_storyboard_and_pacing_budgets_are_read(tmp_path: Path) -> None:
    write(
        tmp_path / "director.yaml",
        """engine: {main_scene: OddSquares}
brief:
  viewer:
    who: a curious 16-year-old
    level: intro
    knows: [odd numbers, $n^2$]
    question: Why do sums of odd numbers land on squares?
storyboard:
  - id: grow
    intent: explain
    audience_question: What shape comes next?
    takeaway: Adding 7 turns 3x3 into 4x4.
    aha: true
  - id: recap
qa:
  pacing: {max_new_per_beat: 4}
""",
    )
    style = load_style(tmp_path)
    assert style.viewer == Viewer(
        level="intro",
        knows=("odd numbers", "$n^2$"),
        question="Why do sums of odd numbers land on squares?",
        missing=("wrong_guess", "aha"),
    )
    assert style.storyboard == (
        StoryBeat(
            "grow", "explain", "What shape comes next?", "Adding 7 turns 3x3 into 4x4.", True
        ),
        StoryBeat("recap"),
    )
    assert style.storyboard_scene == "OddSquares"
    assert style.pacing == {"max_new_per_beat": 4}


def test_no_project_means_defaults(tmp_path: Path) -> None:
    write(tmp_path / "director.yaml", "")
    assert load_style(tmp_path) == ProjectStyle()
    assert ProjectStyle().safe_area == SAFE_AREA_DEFAULT


@pytest.mark.parametrize(
    ("text", "message"),
    [
        ("theme: [1, 2\n", "Cannot read"),
        ("- a list\n", "must be a YAML mapping"),
        ("theme: 3\n", "theme must be a theme name"),
        ("safe_area: {left: 0.6}\n", "safe_area.left must be between 0 and 0.45"),
        ("safe_area: {left: yes}\n", "safe_area.left must be a number"),
        ("direction: {symbols: [x]}\n", "direction.symbols must map"),
        ("brief: {viewer: [x]}\n", "brief.viewer must be a mapping"),
        ("brief: {viewer: {level: 2}}\n", "brief.viewer.level must be a name"),
        ("brief: {viewer: {knows: algebra}}\n", "brief.viewer.knows must be a list"),
        ("storyboard: [{intent: explain}]\n", "storyboard must be a list of beats"),
        ("qa: {pacing: {words_per_second: fast}}\n", "qa.pacing must map budget names"),
    ],
)
def test_invalid_settings_name_the_file_and_key(tmp_path: Path, text: str, message: str) -> None:
    write(tmp_path / "director.yaml", text)
    with pytest.raises(CompositionError, match=message) as raised:
        load_style(tmp_path)
    assert raised.value.data["path"] == str(tmp_path / "director.yaml")
