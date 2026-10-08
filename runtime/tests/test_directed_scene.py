"""DirectedScene rendered by real Manim (cairo, tiny frames, last frame only)."""

from __future__ import annotations

import importlib.util
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import numpy as np
import pytest

from conftest import requires_latex, requires_manim

pytestmark = requires_manim

manim = pytest.importorskip("manim")
from manim import (  # noqa: E402
    BLACK,
    DEGREES,
    DL,
    RED,
    RIGHT,
    YELLOW,
    Circle,
    Create,
    DecimalNumber,
    Dot,
    FadeIn,
    Group,
    Indicate,
    Line,
    MathTex,
    Rectangle,
    Restore,
    Square,
    SurroundingRectangle,
    Text,
    ThreeDAxes,
    Triangle,
    ValueTracker,
    VGroup,
    ZoomedScene,
    always_redraw,
    tempconfig,
)

from manim_director_runtime import (  # noqa: E402
    CompositionError,
    Directed,
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


def drawn(scene: Any) -> set[int]:
    return {id(m) for m in scene.get_mobject_family_members()}


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


def test_theme_applies_to_the_camera_and_plain_manim_objects(
    render: Render, tmp_path: Path
) -> None:
    seen: dict[str, str] = {}

    class Plain(DirectedScene):
        theme = "paper"

        def construct(self):
            seen["text"] = Text("x")[0].get_color().to_hex()
            seen["primary"] = self.theme.primary
            # Classes that hard-code white or yellow, which vanish on a light theme.
            for name, mobject in [("line", Line()), ("dot", Dot()), ("square", Square())]:
                seen[name] = mobject.get_color().to_hex()
            seen["box"] = SurroundingRectangle(Dot()).get_color().to_hex()
            seen["indicate"] = str(Indicate(Dot()).color).upper()
            backing = Square().add_background_rectangle().background_rectangle
            seen["backing"] = backing.get_fill_color().to_hex()
            self.add(Dot())

    scene = render(Plain)
    assert scene.camera.background_color == PAPER.background
    ink = PAPER.foreground
    assert seen == {
        "text": ink,
        "primary": PAPER.primary,
        "line": ink,
        "dot": ink,
        "square": ink,
        "box": PAPER.accent,
        "indicate": PAPER.accent,
        "backing": PAPER.background,
    }
    with tempconfig({"media_dir": str(tmp_path / "media")}):  # Text caches its SVG there
        assert Text("x")[0].get_color().to_hex() == "#FFFFFF"  # defaults restored after render
        assert Dot().get_color().to_hex() == "#FFFFFF"
        assert Square().add_background_rectangle().background_rectangle.get_fill_color() == BLACK


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


def test_title_and_caption_outlast_beats_that_swap_the_content(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Lanes(DirectedScene):
        def construct(self):
            with self.beat("one"):
                title = self.title("Title")
                self.place(Square())
            with self.beat("two"):
                caption = self.caption("Caption")
                self.place(Circle())  # the square leaves while circle and caption arrive
            with self.beat("three"):
                self.place(Triangle())
            seen["lanes"] = [id(m) in drawn(self) for m in (title, caption)]
            seen["groups"] = [type(m).__name__ for m in on_stage(self) if type(m) is Group]

    render(Lanes)
    assert seen == {"lanes": [True, True], "groups": []}


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


def test_a_new_group_glides_what_is_on_stage_and_brings_in_the_rest(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Gather(DirectedScene):
        def construct(self):
            square, circle, dot = Square(), Circle(), Dot()
            with self.beat("one"):
                self.place(square)
            self.play(FadeIn(circle.next_to(square, RIGHT)))  # plain Manim, never placed
            pair = VGroup(square, circle)
            with self.beat("two"):
                self.place(pair, region=Region.LEFT)
            seen["glided"] = inside(pair, self.region("left")) and {
                id(square),
                id(circle),
            } <= drawn(self)
            with self.beat("three"):
                self.place(VGroup(pair, dot), region=Region.RIGHT)
            seen["joined"] = inside(dot, self.region("right")) and {id(square), id(dot)} <= drawn(
                self
            )

    render(Gather)
    assert seen == {"glided": True, "joined": True}


def test_keeping_part_of_a_group_retires_the_rest(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Partial(DirectedScene):
        def construct(self):
            row = VGroup(Square(), Circle(), Triangle()).arrange(RIGHT)
            with self.beat("one"):
                self.place(row)
            with self.beat("two", keep=[row[-1]]):
                self.place(Dot(), region=Region.TOP)
            seen["kept"] = [id(part) in drawn(self) for part in row]

    render(Partial)
    assert seen["kept"] == [False, False, True]


def test_a_frame_the_pixels_would_squash_is_refused(tex_dir: Path, tmp_path: Path) -> None:
    class Anything(DirectedScene):
        def construct(self):
            self.add(Dot())

    portrait_pixels = {"pixel_width": 180, "pixel_height": 320, "media_dir": str(tmp_path)}
    with tempconfig(portrait_pixels), pytest.raises(CompositionError, match="squashed"):
        Anything().render()


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

    class Bare(DirectedScene):  # the call style of v1
        def construct(self):
            self.beat("hook")
            self.wait()

    with pytest.raises(CompositionError, match="write `with self.beat\\('hook'\\):`"):
        render(Bare)

    class Redrawn(DirectedScene):
        def construct(self):
            self.place(always_redraw(lambda: Circle()), region=Region.LEFT)

    with pytest.raises(CompositionError, match="positions itself every frame"):
        render(Redrawn)


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
    # A closing wait after the last beat still maps to it.
    assert recorder.timeline("Timed", 4.0).beats[-1].end_seconds == 4.0


def test_the_timeline_marks_stage_and_camera_moves_as_transitions(
    render: Render, tmp_path: Path
) -> None:
    class Moves(DirectedMovingCameraScene):
        def construct(self):
            with self.beat("hook", hold=0.5):
                self.place(Square())
            self.play(Circle().animate.shift(RIGHT), run_time=0.4)  # content, not a move
            self.camera.frame.save_state()
            self.play(self.camera.frame.animate.scale(0.5), run_time=0.6)
            self.play(Restore(self.camera.frame), run_time=0.3)

    with timeline.recording(tmp_path) as recorder:
        render(Moves)
    spans = recorder.timeline("Moves", 3.0).transitions
    # The beat's entrance, then the zoom and its way back as one.
    assert [t for span in spans for t in (span.start_seconds, span.end_seconds)] == pytest.approx(
        [0.0, 0.8, 1.7, 2.6]
    )


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
def test_highlight_boxes_each_occurrence_and_moves_and_leaves_with_its_equation(
    render: Render,
) -> None:
    seen: dict[str, Any] = {}

    class Boxes(DirectedScene):
        def construct(self):
            with self.beat("one"):
                eq = self.math(r"x^2 + 2x + 1")
                self.place(eq)
                marked = self.highlight(eq, "x", box=True)
                self.place(eq, region=Region.LEFT)  # glides in the same beat
            boxes = marked.boxes
            seen["count"] = len(boxes)
            others = [g for g in eq.family_members_with_points() if g not in marked]
            seen["clear"] = not any(
                b.get_left()[0] < g.get_x() < b.get_right()[0] for b in boxes for g in others
            )
            seen["on glyphs"] = all(
                np.allclose(b.get_center(), VGroup(g).get_center())
                for b, g in zip(boxes, marked, strict=True)
            )
            with self.beat("two", keep=[eq]):
                self.place(Square(), region=Region.RIGHT)
            seen["kept"] = boxes in self.mobjects
            with self.beat("three"):
                self.place(Circle())
            seen["gone"] = boxes not in self.mobjects

    render(Boxes)
    assert seen == {"count": 2, "clear": True, "on glyphs": True, "kept": True, "gone": True}

    class BoxOnly(DirectedScene):
        symbols = {"b": "secondary"}

        def construct(self):
            formula = self.math(r"b^2 - 4ac")
            self.place(formula)
            seen["colors"] = set(colors(self.highlight(formula, "b^2", color=None, box=True)))
            with pytest.raises(CompositionError, match="only boxes"):
                self.highlight(formula, "b^2", color=None)

    render(BoxOnly)
    assert seen["colors"] == {MIDNIGHT.secondary, MIDNIGHT.foreground}  # b and its 2 keep theirs


@requires_latex
def test_colored_operators_keep_their_typesetting_and_alignment(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Operators(DirectedScene):
        symbols = {"=": "muted", r"\sum": "accent", r"\times": "primary"}

        def construct(self):
            source = r"\sum_{k=1}^{n} k \times 2 = n(n+1)"
            colored = self.math(source)
            plain = MathTex(source, font_size=colored.font_size)
            seen["layout"] = [
                [g.get_center() - m.get_corner(DL) for g in m.family_members_with_points()]
                for m in (colored, plain)
            ]
            proof = self.derive(r"a = b + b", r"= 2b")
            seen["aligned"] = relation_x(proof.lines[0]) - relation_x(proof.lines[1])
            seen["term"] = len(self.term(proof.lines[0], "b + b"))

    render(Operators)
    colored, plain = seen["layout"]
    assert np.allclose(colored, plain, atol=1e-3)  # limits above and below, relation spacing
    assert seen["aligned"] == pytest.approx(0, abs=1e-6) and seen["term"] == 3


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
def test_derive_puts_notes_under_their_lines_when_the_region_is_tall(tmp_path, tex_dir) -> None:
    seen: dict[str, Any] = {}

    class Portrait(DirectedScene):
        symbols = {"a": "primary", "b": "secondary", "c": "accent"}

        def construct(self):
            steps = self.derive(
                r"ax^2 + bx + c = 0",
                (r"x^2 + \frac{b}{a}x = -\frac{c}{a}", "divide by $a$"),
                (
                    r"\left(x + \frac{b}{2a}\right)^2 = \frac{b^2 - 4ac}{4a^2}",
                    "complete the square",
                ),
            )
            (_, line, after), note = steps.lines, steps.notes[1]
            seen["under"] = line.get_bottom()[1] > note.get_top()[1] > after.get_top()[1]
            seen["left"] = note.get_left()[0] - line.get_left()[0]
            seen["math note"] = MIDNIGHT.primary in colors(note)
            seen["scale"] = line.font_size / 44
            with pytest.raises(CompositionError, match="notes='left'"):
                self.derive("a = b", notes="left")

    portrait = {"pixel_width": 180, "pixel_height": 320, "frame_width": 4.5, "frame_height": 8.0}
    settings = {"media_dir": str(tmp_path / "media"), "tex_dir": str(tex_dir), **portrait}
    with tempconfig({**settings, "save_last_frame": True, "write_to_movie": False}):
        Portrait().render()  # notes beside the lines would need 0.46x, below the 0.5x minimum
    assert seen["under"] and seen["left"] == pytest.approx(0)
    assert seen["math note"] and seen["scale"] > 0.6


@requires_latex
def test_math_in_a_note_matches_plain_notes_in_size_and_baseline(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Notes(DirectedScene):
        def construct(self):
            steps = self.derive(
                r"\lambda^{n+2} = p\lambda^{n+1} + q\lambda^n",
                (r"\lambda^2 = p\lambda + q", r"divide by $\lambda^n$"),
                (r"\lambda_\pm = \frac{p \pm \sqrt{p^2 + 4q}}{2}", "quadratic formula"),
                pause=0,
            )
            mixed, plain = steps.notes[1:]
            seen["heights"] = mixed.height, plain.height
            # "divide by" is eight glyphs, the first i on the baseline; then lambda and n.
            seen["baseline"] = mixed[8].get_bottom()[1] - mixed[1].get_bottom()[1]
            seen["colors"] = set(colors(mixed))
            with pytest.raises(CompositionError, match="unpaired or empty"):
                self.derive("a = b", ("b = a", "costs $5"))

    render(Notes)
    mixed, plain = seen["heights"]
    assert mixed == pytest.approx(plain, rel=0.08)  # a TeX note was 0.7 of the height
    assert seen["baseline"] == pytest.approx(0, abs=0.02 * plain)  # lambda overshoots a little
    assert seen["colors"] == {MIDNIGHT.muted}  # the math is in the note's color too


@requires_latex
def test_derive_aligns_latex_style_steps_on_their_relation(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Aligned(DirectedScene):
        def construct(self):
            steps = self.derive(r"(a+b)^2 &= (a+b)(a+b)", r"&= a^2 + 2ab + b^2", pause=0)
            columns = [relation_x(line) for line in steps.lines]
            seen["aligned"] = columns[0] - columns[1]
            seen["atoms"] = len(steps.lines[0].submobjects) > 1  # still matchable terms

    render(Aligned)
    assert seen["aligned"] == pytest.approx(0, abs=1e-6) and seen["atoms"]


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
        symbols = {"x": "secondary", "1": RED}  # a Manim color constant works too

        def construct(self):
            eq = self.math(r"x^2 + 2x + 1 = (x + 1)^2")
            self.place(eq)
            seen["manim color"] = set(colors(self.highlight(eq, "=", color=YELLOW)))
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

            ghost = eq.copy()  # a copy keeps the authored source, not the painted TeX
            seen["copy"] = len(self.term(ghost, "x^2"))
            with pytest.raises(CompositionError) as raised:
                self.place(self.math(r"x^2 + " * 40 + "x"))
            seen["named"] = str(raised.value).startswith("MathTex('x^2 + x^2")

    render(Terms)
    assert seen["counts"] == [3, 2, 1, 6]
    assert seen["copy"] == 2 and seen["named"]
    assert seen["manim color"] == {YELLOW.to_hex()}
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
            self.focus(first)  # its tag stays lit with it
            seen["lit"] = [tag.family_members_with_points()[0].get_fill_opacity() for tag in tags]

    render(Tags)
    lit = seen.pop("lit")
    assert seen == {"labels": ["(1)", r"(\star)"], "edge": True, "rows": True}
    assert lit[0] == 1.0 and lit[1] < 1.0


@requires_latex
def test_tags_follow_their_equation_into_its_region(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Moving(DirectedScene):
        def construct(self):
            with self.beat("one"):
                eq = self.math("a^2 + b^2 = c^2")
                self.place(eq)
                mark = self.tag(eq)
            with self.beat("two", keep=[eq]):
                self.place(eq, region=Region.LEFT)
                self.place(Square(), region=Region.RIGHT)  # the tag is not in the way
            seen["edge"] = mark.get_right()[0] - self.region("left").right
            seen["row"] = mark.get_y() - eq.get_y()
            with self.beat("three"):
                # Long enough to reach the region's right edge in any fallback font.
                note = "a note so long that the derivation spans the whole width " * 3
                steps = self.derive("x = 1", ("y = 2", note), notes="right")
                with pytest.raises(CompositionError, match="notes='below'"):
                    self.tag(steps.lines[1])

    render(Moving)
    assert seen["edge"] == pytest.approx(0) and seen["row"] == pytest.approx(0)


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


@requires_latex
def test_three_d_derivations_and_highlights_stay_in_screen_space(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Tilted(DirectedThreeDScene):
        def construct(self):
            camera = self.renderer.camera
            self.set_camera_orientation(phi=60 * DEGREES, theta=30 * DEGREES)
            axes = ThreeDAxes()
            world = {id(m) for m in axes.get_family()}
            projected: set[int] = set()
            project = camera.transform_points_pre_display

            def record(mobject, points):
                if mobject not in camera.fixed_in_frame_mobjects:
                    projected.add(id(mobject))
                return project(mobject, points)

            with self.beat("world"):
                self.play(Create(axes))
            camera.transform_points_pre_display = record
            with self.beat("algebra"):  # axes leave, still as 3D world objects
                steps = self.derive(r"z = x^2 + y^2", r"z = r^2")
                self.highlight(steps.lines[-1], "r^2", box=True)
            seen["overlays projected"] = projected - world
            seen["axes pinned"] = axes in camera.fixed_in_frame_mobjects

    render(Tilted)
    assert seen == {"overlays projected": set(), "axes pinned": False}


def test_moving_camera_scenes_keep_title_and_caption_on_screen(render: Render) -> None:
    seen: dict[str, Any] = {}

    def on_screen(mobject: Any, frame: Any) -> tuple[float, float, float]:
        x, y = (mobject.get_center() - frame.get_center())[:2] / frame.width
        return (round(x, 4), round(y, 4), round(mobject.width / frame.width, 4))

    class Zoom(DirectedMovingCameraScene):
        def construct(self):
            square, frame = Square(), self.camera.frame
            with self.beat("zoom", focus=square):
                title = self.title("Zoom")
                self.place(square, Circle(), direction=RIGHT)
            seen["before"] = (on_screen(title, frame), square.width / frame.width)
            self.play(frame.animate.scale(0.5).move_to(square))
            seen["after"] = (on_screen(title, frame), square.width / frame.width)

    scene = render(Zoom)
    assert scene.camera.background_color == MIDNIGHT.background
    assert seen["after"][0] == seen["before"][0]
    assert seen["after"][1] == pytest.approx(2 * seen["before"][1])


def test_beats_leave_cameras_zoom_displays_and_trackers_alone(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Machinery(DirectedMovingCameraScene):
        def construct(self):
            frame, tracker = self.camera.frame, ValueTracker(1.0)
            readout = DecimalNumber(1.0).add_updater(lambda d: d.set_value(tracker.get_value()))
            self.add(tracker)
            with self.beat("pan"):
                self.place(readout)
            self.play(frame.animate.shift(RIGHT))
            centers: list[float] = []
            frame.add_updater(lambda m: centers.append(float(m.get_x())))
            with self.beat("contrast", transition="contrast", keep=[readout]):
                self.place(Square(), region=Region.RIGHT)
            seen["frame"] = (set(centers), frame in self.mobjects)
            seen["tracker"] = (tracker.get_value(), tracker in self.mobjects)

    class Zoomed(Directed, ZoomedScene):
        def construct(self):
            with self.beat("one"):
                self.place(Square())
            self.activate_zooming(animate=False)
            with self.beat("two"):
                self.place(Circle())
            seen["zoom"] = self.zoomed_display in self.mobjects

    render(Machinery)
    render(Zoomed)
    assert seen["frame"] == ({1.0}, True)  # the camera did not pan with the departures
    assert seen["tracker"] == (1.0, True)
    assert seen["zoom"] is True
