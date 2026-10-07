"""DirectedScene rendered by real Manim (cairo, tiny frames, last frame only)."""

from __future__ import annotations

import importlib.util
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

from conftest import requires_latex, requires_manim

pytestmark = requires_manim

manim = pytest.importorskip("manim")
from manim import (  # noqa: E402
    DEGREES,
    RIGHT,
    Circle,
    Dot,
    Line,
    Rectangle,
    Square,
    Text,
    ThreeDAxes,
    Triangle,
    tempconfig,
)

from manim_director_runtime import (  # noqa: E402
    CompositionError,
    DirectedMovingCameraScene,
    DirectedScene,
    DirectedThreeDScene,
    Region,
    timeline,
)
from manim_director_runtime.derivation import relation_x  # noqa: E402
from manim_director_runtime.themes import theme  # noqa: E402

MIDNIGHT, PAPER = theme("midnight"), theme("paper")
Render = Callable[[type], Any]


@pytest.fixture(scope="session")
def tex_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    return tmp_path_factory.mktemp("tex")


@pytest.fixture
def render(tmp_path: Path, tex_dir: Path) -> Render:
    def run(scene_class: type) -> Any:
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
            scene = scene_class()
            scene.render()
        return scene

    return run


def on_stage(scene: Any) -> list[Any]:
    """The scene's visible mobjects (Manim's wait() leaves empty Mobjects behind)."""

    return [m for m in scene.mobjects if m.family_members_with_points()]


def colors(mobject: Any) -> list[str]:
    return [leaf.get_color().to_hex() for leaf in mobject.family_members_with_points()]


def inside(mobject: Any, area: Any) -> bool:
    eps = 1e-6
    return (
        mobject.get_left()[0] >= area.left - eps
        and mobject.get_right()[0] <= area.right + eps
        and mobject.get_bottom()[1] >= area.bottom - eps
        and mobject.get_top()[1] <= area.top + eps
    )


def test_theme_applies_to_the_camera_and_plain_manim_objects(render: Render) -> None:
    seen: dict[str, str] = {}

    class Plain(DirectedScene):
        theme = "paper"

        def construct(self):
            seen["text"] = Text("x")[0].get_color().to_hex()
            seen["line"] = Line().get_color().to_hex()
            seen["primary"] = self.theme.primary
            self.add(Dot())

    scene = render(Plain)
    assert scene.camera.background_color == PAPER.background
    assert seen == {"text": PAPER.foreground, "line": PAPER.foreground, "primary": PAPER.primary}
    assert Text("x")[0].get_color().to_hex() == "#FFFFFF"  # defaults restored after the render


def test_project_settings_come_from_the_nearest_director_yaml(render: Render, tmp_path: Path):
    project = tmp_path / "project"
    (project / "scenes").mkdir(parents=True)
    (project / "director.yaml").write_text("theme: chalkboard\nsafe_area: {left: 0.2}\n")
    source = project / "scenes" / "demo.py"
    source.write_text(
        "from manim_director_runtime import DirectedScene\n"
        "class Demo(DirectedScene):\n"
        "    def construct(self):\n"
        "        self.seen = (self.theme.name, self.region('safe').left)\n"
    )
    spec = importlib.util.spec_from_file_location("director_demo_scene", source)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module  # as `manim` and the bridge import scene files
    spec.loader.exec_module(module)
    scene = render(module.Demo)
    width = scene.camera.frame_width
    assert scene.seen == ("chalkboard", pytest.approx(-width / 2 + 0.2 * width))

    (project / "director.yaml").write_text("theme: sepia\n")
    with pytest.raises(CompositionError, match="Unknown theme 'sepia'"):
        render(module.Demo)


def test_place_fits_arranges_and_validates_before_moving(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Layout(DirectedScene):
        def construct(self):
            content = self.region(Region.CONTENT)
            wide = Rectangle(width=20, height=1)
            self.place(wide, region=Region.BOTTOM)
            seen["fits"] = inside(wide, content) and wide.width == pytest.approx(content.width)
            square = Square(1).move_to((5, 3, 0))
            with pytest.raises(CompositionError, match="would overlap Rectangle"):
                self.place(square, region=Region.BOTTOM)
            with pytest.raises(CompositionError, match="needs 0.13x"):
                self.place(Rectangle(width=100, height=1), region=Region.TOP)
            seen["untouched"] = tuple(square.get_center()[:2])
            pair = self.place(Square(1), Circle(), region=Region.TOP, direction=RIGHT)
            seen["pair"] = (inside(pair, self.region("top")), pair[0].get_x() < pair[1].get_x())

    scene = render(Layout)
    assert seen == {"fits": True, "untouched": (5.0, 3.0), "pair": (True, True)}
    assert len(on_stage(scene)) == 3  # staged objects still land on stage by the end


def test_beats_retire_what_they_do_not_keep(render: Render) -> None:
    stages: list[set[str]] = []

    class Beats(DirectedScene):
        def construct(self):
            square, circle, triangle = Square(), Circle(), Triangle()
            for mob, name in ((square, "square"), (circle, "circle"), (triangle, "triangle")):
                mob.name = name

            def snapshot():
                stages.append({m.name for m in on_stage(self)})

            with self.beat("one"):
                self.title("Title").name = "title"
                self.place(square, circle, direction=RIGHT)
            snapshot()
            with self.beat("two", keep=[circle]):
                self.place(triangle, region=Region.LEFT)
            snapshot()
            with self.beat("three", transition="chapter"):
                self.place(Dot())
            snapshot()

    render(Beats)
    assert stages == [
        {"title", "square", "circle"},
        {"title", "circle", "triangle"},
        {"Dot"},
    ]


def test_replacing_morphs_and_re_placing_glides(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Moves(DirectedScene):
        def construct(self):
            first, second = Square(), Circle()
            with self.beat("one"):
                self.place(first)
            with self.beat("two"):
                self.place(first, region=Region.TOP)
            seen["glided"] = inside(first, self.region("top")) and first in self.mobjects
            with self.beat("three"):
                self.place(second, replaces=first)
            seen["morphed"] = (first in self.mobjects, second in self.mobjects)

    render(Moves)
    assert seen == {"glided": True, "morphed": (False, True)}


def test_focus_dims_the_rest_until_the_next_beat(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Focus(DirectedScene):
        def construct(self):
            other, star = Square(), Circle()
            with self.beat("one", focus=star):
                self.title("Lit")
                self.place(other, star, direction=RIGHT)
            seen["during"] = (
                other.get_stroke_opacity(),
                star.get_stroke_opacity(),
                self.mobjects[0].family_members_with_points()[0].get_fill_opacity(),
            )
            with self.beat("two", keep=[other, star]):
                pass
            seen["after"] = (other.get_stroke_opacity(), star.get_stroke_opacity())

    render(Focus)
    assert seen["during"] == (pytest.approx(0.28), 1.0, 1.0)
    assert seen["after"] == (1.0, 1.0)


def test_beat_misuse_fails_with_a_clear_message(render: Render) -> None:
    class Nested(DirectedScene):
        def construct(self):
            with self.beat("outer"), self.beat("inner"):
                pass

    with pytest.raises(CompositionError, match="starts inside beat 'outer'"):
        render(Nested)

    class Unseen(DirectedScene):
        def construct(self):
            with self.beat("hook", focus=Square()):
                pass

    with pytest.raises(CompositionError, match="never reached the stage"):
        render(Unseen)


def test_run_time_zero_lands_on_the_end_state_instantly(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Instant(DirectedScene):
        def construct(self):
            square = Square()
            with self.beat("instant", run_time=0, hold=0, focus=square):
                self.place(square, Circle(), direction=RIGHT)
            seen["state"] = (self.renderer.time, square in self.mobjects, len(on_stage(self)))

    render(Instant)
    assert seen["state"] == (0.0, True, 2)


def test_beats_record_the_timeline_under_the_bridge(render: Render, tmp_path: Path) -> None:
    class Timed(DirectedScene):
        def construct(self):
            with self.beat("hook", hold=0.5):
                self.place(Square())
            with self.beat():
                self.play(Circle().animate.shift(RIGHT), run_time=0.4)

    with timeline.recording(tmp_path) as recorder:
        scene = render(Timed)
    record = recorder.timeline("Timed", scene.renderer.time)
    assert recorder.attached
    assert [(b.id, b.file, b.line) for b in record.beats] == [
        ("hook", __file__, Timed.construct.__code__.co_firstlineno + 1),
        ("beat-2", __file__, Timed.construct.__code__.co_firstlineno + 3),
    ]
    hook, second = record.beats
    assert (hook.start_seconds, hook.end_seconds) == pytest.approx((0.0, 1.3))
    assert (second.start_seconds, second.end_seconds) == pytest.approx((1.3, 3.5))


@requires_latex
def test_math_colors_symbols_and_splits_into_matchable_atoms(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Symbols(DirectedScene):
        symbols = {"x": "primary"}

        def construct(self):
            eq = self.math(r"x^2 + \max(x, 1)")
            seen["colors"] = colors(eq)
            seen["parts"] = [part.tex_string for part in eq.submobjects][-3:]

    render(Symbols)
    assert seen["colors"].count(MIDNIGHT.primary) == 2  # never the x inside \max
    assert set(seen["colors"]) == {MIDNIGHT.primary, MIDNIGHT.foreground}
    assert seen["parts"] == [",", "1", ")"]


@requires_latex
def test_derive_aligns_relations_and_leaves_one_block_on_stage(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Derive(DirectedScene):
        def construct(self):
            with self.beat("proof", hold=0):
                proof = self.derive(
                    r"x^2 + 2x + 1 = 0",
                    (r"(x + 1)^2 = 0", "factor"),
                    (r"x = -1", "take roots"),
                )
            columns = [relation_x(line) for line in proof.lines]
            notes = [note for note in proof.notes if note is not None]
            seen["aligned"] = max(columns) - min(columns)
            seen["notes_right"] = min(n.get_left()[0] for n in notes) > max(
                line.get_right()[0] for line in proof.lines
            )
            seen["stage"] = on_stage(self) == [proof]
            seen["inside"] = inside(proof, self.region("content"))
            seen["plays"] = self.renderer.num_plays

    render(Derive)
    assert seen["aligned"] == pytest.approx(0, abs=1e-6)
    assert seen["notes_right"] and seen["stage"] and seen["inside"]
    assert seen["plays"] == 5  # first line, then (pause + step) for each further step


@requires_latex
def test_derive_in_place_ends_on_the_last_step(render: Render) -> None:
    seen: dict[str, Any] = {}

    class InPlace(DirectedScene):
        def construct(self):
            proof = self.derive("a = b", ("b = a", "symmetry"), in_place=True, pause=0)
            seen["stage"] = [m.submobjects for m in on_stage(self)]
            seen["last"] = [proof.lines[-1], proof.notes[-1]]

    render(InPlace)
    assert seen["stage"] == [seen["last"]]


@requires_latex
def test_terms_and_highlights_find_sub_expressions(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Terms(DirectedScene):
        symbols = {"x": "secondary"}

        def construct(self):
            eq = self.math(r"x^2 + 2x + 1 = (x + 1)^2")
            self.place(eq)
            seen["counts"] = [
                len(self.term(eq, "x")),
                len(self.term(eq, "2x")),
                len(self.term(eq, "x", occurrence=2)),
                len(self.term(eq, "(x + 1)^2")),
            ]
            glyphs = self.highlight(eq, "2x", box=True)
            seen["highlighted"] = set(colors(glyphs))
            seen["box"] = len(on_stage(self))
            with pytest.raises(CompositionError, match="does not occur"):
                self.term(eq, "y")
            with pytest.raises(CompositionError, match="occurs 3 time"):
                self.term(eq, "x", occurrence=3)
            with pytest.raises(CompositionError, match="looks inside MathTex or Tex"):
                self.term(Square(), "x")

    render(Terms)
    assert seen["counts"] == [3, 2, 1, 6]
    assert seen["highlighted"] == {MIDNIGHT.accent}
    assert seen["box"] == 2


@requires_latex
def test_tags_number_equations_at_the_right_edge(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Tags(DirectedScene):
        def construct(self):
            first, second = self.math("a = b"), self.math("b = c")
            self.place(first, second)
            tags = [self.tag(first), self.tag(second, r"(\star)")]
            self.wait(0.1)
            right = self.region("content").right
            seen["labels"] = [tag.tex_string for tag in tags]
            seen["edge"] = all(tag.get_right()[0] == pytest.approx(right) for tag in tags)
            seen["rows"] = tags[0].get_y() == pytest.approx(first.get_y())

    render(Tags)
    assert seen == {"labels": ["(1)", r"(\star)"], "edge": True, "rows": True}


def test_a_still_shows_the_theme_and_the_content(render: Render, tmp_path: Path) -> None:
    from PIL import Image

    class Still(DirectedScene):
        theme = "paper"

        def construct(self):
            self.place(Square(3, fill_color=self.theme.primary, fill_opacity=1))

    render(Still)
    (image_path,) = (tmp_path / "media").rglob("Still*.png")
    with Image.open(image_path) as image:
        rgb = image.convert("RGB")
        corner, center = rgb.getpixel((2, 2)), rgb.getpixel((160, 90))
    assert "#{:02X}{:02X}{:02X}".format(*corner) == PAPER.background
    assert "#{:02X}{:02X}{:02X}".format(*center) == PAPER.primary


def test_three_d_scenes_keep_placed_objects_in_screen_space(render: Render) -> None:
    class Orbit(DirectedThreeDScene):
        def construct(self):
            self.title("Orbit")
            self.add(ThreeDAxes())
            self.move_camera(phi=60 * DEGREES, theta=30 * DEGREES, run_time=0.2)

    scene = render(Orbit)
    (title,) = [m for m in scene.mobjects if isinstance(m, Text)]
    assert title in scene.renderer.camera.fixed_in_frame_mobjects
    assert inside(title, scene.region("header"))


def test_moving_camera_scenes_are_directed_too(render: Render) -> None:
    class Zoom(DirectedMovingCameraScene):
        def construct(self):
            square = Square()
            with self.beat("zoom", focus=square):
                self.place(square, Circle(), direction=RIGHT)
            self.play(self.camera.frame.animate.scale(0.5).move_to(square))

    scene = render(Zoom)
    assert scene.camera.background_color == MIDNIGHT.background
