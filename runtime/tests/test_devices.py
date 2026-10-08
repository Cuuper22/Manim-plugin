"""Devices that direct the viewer: ask, annotate, link, misconception, and replays."""

from __future__ import annotations

from collections.abc import Callable
from itertools import combinations
from pathlib import Path
from typing import Any

import numpy as np
import pytest

from conftest import requires_latex, requires_manim

pytestmark = [requires_manim, requires_latex]

manim = pytest.importorskip("manim")
from manim import RIGHT, Brace, Dot, Line, Mobject, Square  # noqa: E402

from manim_director_runtime import CompositionError, DirectedScene, pacing, timeline  # noqa: E402
from manim_director_runtime.kit import DotArray, Figure, VectorGrid, hide  # noqa: E402
from manim_director_runtime.layout import Rect  # noqa: E402
from manim_director_runtime.themes import theme  # noqa: E402

MIDNIGHT = theme("midnight")
HERE = Path(__file__).resolve().parent
Render = Callable[..., Any]


def box(mobject: Mobject) -> Rect:
    x, y = mobject.get_center()[:2]
    return Rect.around((float(x), float(y)), mobject.width, mobject.height)


def drawn(mobject: Mobject) -> bool:
    return any(
        leaf.get_fill_opacity() > 0 or leaf.get_stroke_opacity() > 0
        for leaf in mobject.family_members_with_points()
    )


def test_ask_holds_the_stage_still_and_records_the_prompt(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Predict(DirectedScene):
        def construct(self):
            with self.beat("setup"):
                self.place(Square())
            with self.beat("predict", keep=[]):
                seen["prompt"] = self.ask("Bigger or smaller?")
            with self.beat("reveal"):
                self.place(Square(2))

    with timeline.recording(HERE) as recorder:
        scene = render(Predict, every_frame=True)
    film = recorder.timeline("Predict", scene.renderer.time)
    (ask,) = film.asks
    assert (ask.text, ask.hold) == ("Bigger or smaller?", 3.0)
    assert [lane.text for lane in film.captions] == ["Bigger or smaller?"]
    following = next(e for e in film.events if e.at > ask.at)
    assert following.at - ask.at >= 3.0 - 1e-6
    assert not [f for f in pacing.check(film, pacing.settings()) if f.code == "question_hold_short"]
    mark, words = seen["prompt"]
    assert mark.get_right()[0] < words.get_left()[0]  # the "?" leads the question
    assert scene.region("caption").contains(box(seen["prompt"]))


def test_notes_find_free_room_follow_their_target_and_settle_to_muted(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Notes(DirectedScene):
        symbols = {"b": "secondary"}

        def construct(self):
            eq = self.math(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}")
            with self.beat("parts"):
                self.place(eq)
                first = self.annotate(eq, "how far apart the roots are", term=r"b^2 - 4ac")
                second = self.annotate(eq, "always halve", term="2a", style="brace")
                named = self.annotate(eq, "the slope at 0", term="b", occurrence=0)
            seen["notes"] = (first, second, named)
            seen["first color"] = first.label.family_members_with_points()[0].get_color()
            seen["first box"] = first.box.get_fill_opacity()
            leaves = [leaf for m in self.mobjects for leaf in m.family_members_with_points()]
            seen["clear"] = all(
                not box(note.label).overlaps(box(leaf))
                for note in (first, second, named)
                for leaf in leaves
                if leaf not in note.get_family() and leaf not in eq.get_family()
            )
            seen["inside"] = all(
                self.region("content").contains(box(n)) for n in (first, second, named)
            )
            offset = second.label.get_center() - eq.get_center()
            with self.beat("move", keep=[eq, second]):
                self.place(eq, region="left")
            seen["followed"] = np.allclose(
                second.label.get_center() - eq.get_center(), offset * eq.width / seen["width"]
            )

        def place(self, *args, **kwargs):
            placed = super().place(*args, **kwargs)
            seen.setdefault("width", placed.width)
            return placed

    with timeline.recording(HERE) as recorder:
        scene = render(Notes, every_frame=True)
    film = recorder.timeline("Notes", scene.renderer.time)
    first, second, named = seen["notes"]
    assert isinstance(second.pointer, Brace) and named.box is not None
    assert seen["first color"].to_hex() == MIDNIGHT.muted and seen["first box"] == 0
    assert seen["clear"] and seen["inside"] and seen["followed"]
    assert [(d.symbol, d.via) for d in film.definitions] == [("b", "annotate")]
    assert [e.source for e in film.events].count("annotate") == 3


def test_a_boxed_in_note_raises_with_the_fix(render: Render) -> None:
    class Boxed(DirectedScene):
        def construct(self):
            wide = Square(20).set_fill(opacity=0.5)
            dot = Dot()
            self.add(wide, dot)
            with pytest.raises(CompositionError, match="No free space"):
                self.annotate(dot, "a note that cannot fit anywhere at all on this stage")

    render(Boxed)


def test_link_marks_both_ends_in_one_motion_and_leaves_with_the_beat(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Linked(DirectedScene):
        def construct(self):
            dots = DotArray(2, 3, color="primary")
            eq = self.math("2 + 2 + 2 = 6")
            with self.beat("pair"):
                self.place(dots, eq, direction=RIGHT)
                seen["marks"] = self.link(self.term(eq, "6"), dots)
                seen["lit"] = all(drawn(m) for m in seen["marks"])
            with self.beat("next", keep=[dots, eq]):
                self.play(dots.paint(1, "accent"))
            seen["after"] = any(m in self.mobjects for m in seen["marks"])

    with timeline.recording(HERE) as recorder:
        scene = render(Linked, every_frame=True)
    film = recorder.timeline("Linked", scene.renderer.time)
    glyph_box, halos = seen["marks"]
    assert len(halos.family_members_with_points()) == 6  # one halo per dot
    assert glyph_box.get_fill_color().to_hex() == MIDNIGHT.accent
    assert seen["lit"] and not seen["after"]
    (event,) = [e for e in film.events if e.source == "link"]
    assert (event.entered, event.targets) == ([], 0)
    with pytest.raises(CompositionError, match="pairs two or more"):
        DirectedScene.link(scene, Square())


def test_misconception_keeps_the_struck_claim_beside_its_repair(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Card(DirectedScene):
        symbols = {"a": "primary", "b": "secondary"}

        def construct(self):
            card = self.misconception(r"\sqrt{a + b} = \sqrt{a} + \sqrt{b}")
            with self.beat("elicit"):
                self.caption("Tempting?")
            seen["claim color"] = card.claim[1].family_members_with_points()[0].get_color()
            seen["hidden"] = [drawn(m) for m in (card.cross, card.strike, card.check)]
            with self.beat("test", keep=[card]):
                self.play(card.test(r"\sqrt{9 + 16} = 5", r"\sqrt{9} + \sqrt{16} = 7"))
                with pytest.raises(CompositionError, match="evidence row"):
                    card.test("1 = 1")
            with self.beat("refute", keep=[card]):
                self.play(card.refute())
            with self.beat("repair", keep=[card]):
                self.play(card.repair(r"\sqrt{a + b} \le \sqrt{a} + \sqrt{b}"))
            seen["card"] = card
            seen["safe"] = self.region("safe").contains(box(card))

    render(Card, every_frame=True)
    card = seen["card"]
    assert seen["claim color"].to_hex() == MIDNIGHT.foreground  # not red: it is their belief
    assert seen["hidden"] == [False, False, False]
    assert all(drawn(m) for m in (*card.rows, card.cross, card.strike, card.fix, card.check))
    opacities = [g.get_fill_opacity() for g in card.claim.family_members_with_points()]
    assert opacities == pytest.approx([0.45] * len(opacities))  # struck, and still there
    (changed,) = card.diff  # only "≤" is new
    assert drawn(changed) and changed.get_fill_color().to_hex() == MIDNIGHT.success
    assert seen["safe"]
    rows = [card.claim, *card.rows, card.fix]
    assert all(not box(a).overlaps(box(b)) for a, b in combinations(rows, 2))


def test_hide_and_show_replay_an_overlay(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Replay(DirectedScene):
        def construct(self):
            triangle = Figure({"A": (0, 0), "B": (3, 0), "C": (0, 2)}, labels=False)
            triangle.polygon("ABC")
            self.place(triangle)
            side = triangle.length("BC", "c")
            self.show(side)
            self.play(hide(side))
            seen["hidden"] = drawn(side)
            self.show(side, run_time=0.5)
            seen["shown"] = drawn(side)
            with pytest.raises(CompositionError, match="already visible"):
                self.show(side)

    render(Replay, every_frame=True)
    assert (seen["hidden"], seen["shown"]) == (False, True)


def test_a_lined_up_selection_reads_as_one_group(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Gather(DirectedScene):
        def construct(self):
            crowd = DotArray(4, 5)
            with self.beat("crowd"):
                self.place(crowd, region="left")
            picked = [crowd.at(0, 1), crowd.at(2, 3), crowd.at(3, 0)]
            with self.beat("gather", keep=[crowd]):
                row = self.place(*picked, region="right", direction=RIGHT)
            seen["row"] = row
            seen["unit"] = self._unit(picked[1])
            seen["in crowd"] = any(dot in crowd.get_family() for dot in picked)
            seen["chunks"] = self._stage_view()[0]
            with self.beat("next", keep=[crowd, row]):
                self.place(Square(0.5), region="top")
            seen["kept"] = all(dot in self.get_mobject_family_members() for dot in picked)

    render(Gather, every_frame=True)
    assert seen["unit"] is seen["row"] and not seen["in crowd"]
    assert seen["chunks"] == 2  # the crowd, and the row
    assert seen["kept"]


def test_grid_notes_may_cross_grid_lines(render: Render) -> None:
    seen: dict[str, Any] = {}

    class OnGrid(DirectedScene):
        def construct(self):
            grid = VectorGrid(2, unit=0.6)
            self.place(grid)
            height = Line(grid.to_point((-1, 0)), grid.to_point((-1, 1)))
            self.play(manim.Create(height))
            note = self.annotate(height, "height 1", style="brace")
            seen["side"] = note.pointer.get_center()[0] - height.get_center()[0]
            seen["halo"] = note.label.family_members_with_points()[0].get_stroke_width(
                background=True
            )

    render(OnGrid, every_frame=True)
    assert abs(seen["side"]) > 0.05 and seen["halo"] > 0  # beside the line, over the grid


def test_braces_follow_the_long_side_of_their_term(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Sides(DirectedScene):
        def construct(self):
            dots = DotArray(3, 3)
            self.place(dots)
            column = dots.select(lambda r, c: c == 2)
            row = dots.select(lambda r, c: r == 2)
            seen["column"] = self.annotate(column, "3", style="brace").pointer
            seen["row"] = self.annotate(row, "3", style="brace").pointer
            seen["dots"] = (column.get_center(), row.get_center())

    render(Sides, every_frame=True)
    column, row = seen["dots"]
    assert abs(seen["column"].get_center()[0] - column[0]) > 0.1  # beside the column
    assert abs(seen["row"].get_center()[1] - row[1]) > 0.1  # above or below the row
