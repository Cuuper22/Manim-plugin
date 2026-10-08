"""The explainer kit rendered by real Manim: components, overlays and reserved parts."""

from __future__ import annotations

from collections.abc import Callable
from itertools import combinations
from typing import Any

import numpy as np
import pytest

from conftest import requires_latex, requires_manim

pytestmark = [requires_manim, requires_latex]

manim = pytest.importorskip("manim")
from manim import (  # noqa: E402
    RIGHT,
    AnimationGroup,
    Circle,
    FadeToColor,
    Group,
    Indicate,
    Mobject,
    Square,
    ValueTracker,
)

from manim_director_runtime import CompositionError, DirectedScene  # noqa: E402
from manim_director_runtime.kit import (  # noqa: E402
    DotArray,
    Figure,
    FunctionPlot,
    Readout,
    VectorGrid,
    label,
    reserve,
)
from manim_director_runtime.layout import Rect  # noqa: E402
from manim_director_runtime.themes import theme  # noqa: E402

MIDNIGHT, PAPER = theme("midnight"), theme("paper")
SHEAR = [[1, 1], [0, 1]]
STRETCH = [[2, 0], [0, 1]]
Render = Callable[..., Any]


def hex_of(mobject: Mobject) -> str:
    """The color of the first drawn leaf (groups and TeX carry no color of their own)."""

    return mobject.family_members_with_points()[0].get_color().to_hex()


def box(mobject: Mobject) -> Rect:
    x, y = mobject.get_center()[:2]
    return Rect.around((float(x), float(y)), mobject.width, mobject.height)


def close(a: Any, b: Any, tol: float = 1e-6) -> bool:
    return bool(np.allclose(np.asarray(a, float), np.asarray(b, float), atol=tol))


def cross(a: np.ndarray, b: np.ndarray) -> float:
    return float(a[0] * b[1] - a[1] * b[0])


def visible(mobject: Mobject) -> bool:
    return any(
        leaf.get_fill_opacity() > 0 or leaf.get_stroke_opacity() > 0
        for leaf in mobject.family_members_with_points()
    )


def test_components_take_the_theme_of_the_scene_they_are_built_in(render: Render) -> None:
    seen: dict[str, str] = {}

    class Paper(DirectedScene):
        theme = "paper"

        def construct(self):
            plot = FunctionPlot(lambda x: x, lambda x: -x, numbers=False)
            grid = VectorGrid(1)
            seen.update(
                curve=hex_of(plot.curves[0]),
                second=hex_of(plot.curves[1]),
                axes=hex_of(plot.axes.x_axis),
                dots=hex_of(DotArray(2)[0]),
                i_hat=hex_of(grid.i_hat),
                j_hat=hex_of(grid.j_hat),
                figure=hex_of(Figure({"A": (0, 0), "B": (1, 0)}).segment("AB")),
            )

    render(Paper)
    assert seen == {
        "curve": PAPER.primary,
        "second": PAPER.secondary,
        "axes": PAPER.muted,
        "dots": PAPER.muted,
        "i_hat": PAPER.primary,
        "j_hat": PAPER.secondary,
        "figure": PAPER.foreground,
    }
    # Outside a render, components fall back to the default theme.
    assert hex_of(FunctionPlot(lambda x: x, numbers=False).curves[0]) == MIDNIGHT.primary


def test_plot_overlays_stay_exact_through_a_glide_and_a_rescale(render: Render) -> None:
    errors: list[float] = []
    seen: dict[str, Any] = {}

    def square(x: float) -> float:
        return x * x

    class Glide(DirectedScene):
        def construct(self):
            plot = FunctionPlot(square, x_range=(-0.5, 2.5), size=(9, 4), labels=["x^2"])
            h = ValueTracker(0.8)
            with self.beat("plot"):
                self.place(plot, region="content")
            overlays = {
                "dot": plot.dot(1.5, label="P"),
                "tangent": plot.tangent(1),
                "secant": plot.secant(0.5, h),
                "area": plot.area(0.2, 1.4),
                "bars": plot.riemann(0, 2, 4),
                "guides": plot.guides(2),
            }
            with self.beat("marks", keep=[plot]):
                self.show(*overlays.values())

            def check(_: Mobject) -> None:
                errors.append(misfit(plot, overlays, h.get_value()))

            probe = Mobject().add_updater(check)
            self.add(probe)
            with self.beat("move", keep=[plot, *overlays.values()]):
                self.place(plot, region="right")
                self.play(h.animate.set_value(0.3))
            seen["scale"] = plot._scale()
            seen["final"] = misfit(plot, overlays, h.get_value())

    render(Glide, every_frame=True)
    assert seen["scale"] < 0.8  # the right region shrank it
    assert seen["final"] < 1e-6
    assert len(errors) > 10 and max(errors) < 1e-6  # every frame of the glide


def misfit(plot: FunctionPlot, overlays: dict[str, Mobject], h: float) -> float:
    """How far the overlays are from where the plot's coordinates put them."""

    c2p = plot.axes.c2p
    dot, tangent, secant = overlays["dot"], overlays["tangent"], overlays["secant"]
    p = c2p(1, 1)
    along = tangent.get_end() - tangent.get_start()
    slope = c2p(2, 3) - p  # slope 2 at x = 1
    bar = overlays["bars"].family_members_with_points()[1]  # [0.5, 1], height f(0.5)
    corners = [c2p(0.5, 0), c2p(1, 0), c2p(1, 0.25), c2p(0.5, 0.25)]
    area = overlays["area"].get_vertices()
    deviations = [
        np.linalg.norm(dot[0].get_center() - c2p(1.5, 2.25)),
        abs(cross(along, p - tangent.get_start())),  # the point is on the line
        abs(cross(along, slope)) / np.linalg.norm(slope),  # with the curve's slope
        np.linalg.norm(secant[-2].get_center() - c2p(0.5, 0.25)),
        np.linalg.norm(secant[-1].get_center() - c2p(0.5 + h, (0.5 + h) ** 2)),
        np.abs(np.asarray(bar.get_vertices()) - np.asarray(corners)).max(),
        np.linalg.norm(area[0] - c2p(0.2, 0.04)),
        np.linalg.norm(area[-1] - c2p(0.2, 0)),
        np.linalg.norm(overlays["guides"][0].get_end() - c2p(2, 4)),
    ]
    return float(max(deviations))


def test_refine_splits_each_bar_in_place_and_approaches_the_integral(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Bars(DirectedScene):
        def construct(self):
            plot = FunctionPlot(lambda x: x * x, x_range=(0, 2))
            self.place(plot)
            bars = plot.riemann(0, 2, 4, rule="mid")
            self.show(bars)
            for _ in range(4):
                self.play(plot.refine(bars), run_time=0.3)
            seen["on stage"] = bars in self.mobjects
            seen["bars"] = bars.family_members_with_points()
            seen["area"] = sum(bar.width * bar.height for bar in seen["bars"]) / (
                plot.axes.x_axis.get_unit_size() * plot.axes.y_axis.get_unit_size()
            )
            with pytest.raises(CompositionError, match="riemann\\(\\) made"):
                plot.refine(Square())

    render(Bars)
    assert seen["on stage"] and len(seen["bars"]) == 64
    assert seen["area"] == pytest.approx(8 / 3, rel=0.02)


def test_overlays_are_beat_local_and_leave_with_their_component(render: Render) -> None:
    stages: list[set[str]] = []

    class Local(DirectedScene):
        def construct(self):
            plot = FunctionPlot(lambda x: x, numbers=False)
            tangent, dot = plot.tangent(1), plot.dot(1)
            tangent.name, dot.name, plot.name = "tangent", "dot", "plot"

            def snapshot():
                names = {getattr(m, "name", "") for m in self.mobjects}
                stages.append(names & {"tangent", "dot", "plot", "zoom", "window"})

            with self.beat("one"):
                self.place(plot, region="left")
                self.show(tangent, dot)
            snapshot()
            with self.beat("two", keep=[plot, tangent]):
                zoom = plot.inset((1, 1), 0.5)
                zoom.name, zoom.window.name = "zoom", "window"
                self.place(zoom, region="right")
            snapshot()
            with self.beat("three", keep=[plot, zoom]):  # the tangent is not kept
                pass
            snapshot()
            with self.beat("four", keep=[tangent, zoom]):  # nor is the plot it is drawn on
                pass
            snapshot()
            with self.beat("five", keep=[zoom]):
                self.place(Circle(), replaces=zoom)
            snapshot()

    render(Local)
    assert stages == [
        {"plot", "tangent", "dot"},
        {"plot", "tangent", "zoom", "window"},  # the window entered with its inset
        {"plot", "zoom", "window"},  # beat-local; the window persists with its inset
        {"zoom", "window"},
        set(),  # morphing the inset away takes its window along
    ]


def test_show_reveals_parts_that_were_laid_out_from_the_start(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Reveal(DirectedScene):
        def construct(self):
            dots = DotArray(4, 4, shown=lambda r, c: max(r, c) < 2)
            seen["reserved width"] = dots.width
            with self.beat("one"):
                self.place(dots, region="left")
            ring = dots.select(lambda r, c: max(r, c) == 2, color="accent")
            seen["before"] = [visible(d) for d in ring]
            width = dots.width
            self.show(ring)
            seen["after"] = [visible(d) for d in ring]
            seen["same layout"] = dots.width == pytest.approx(width)
            seen["corner hidden"] = not visible(dots.at(3, 3))
            seen["colored"] = {hex_of(d) for d in ring}
            with pytest.raises(CompositionError, match="already visible"):
                self.show(ring)
            with pytest.raises(CompositionError, match="place\\(\\) it first"):
                self.show(Square())
            with pytest.raises(CompositionError, match="not on stage: place\\(\\) it first"):
                self.show(FunctionPlot(lambda x: x, numbers=False).dot(1))
            self.play(dots.hide(lambda r, c: max(r, c) == 2))
            seen["hidden again"] = not any(visible(d) for d in ring)
            self.show(ring)  # a replay
            seen["replayed"] = all(visible(d) for d in ring)

    render(Reveal)
    pitch = 2 * 0.1 + 0.2
    assert seen["reserved width"] == pytest.approx(3 * pitch + 0.2)
    assert seen["before"] == [False] * 5 and seen["after"] == [True] * 5
    assert seen["same layout"] and seen["corner hidden"]
    assert seen["colored"] == {MIDNIGHT.accent}
    assert seen["hidden again"] and seen["replayed"]


def test_a_placed_selection_glides_out_of_its_array(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Gather(DirectedScene):
        def construct(self):
            dots = DotArray(1, 6)
            with self.beat("one"):
                self.place(dots, region="content")
            moving = dots.select(lambda r, c: c >= 3)
            path = []
            probe = Mobject().add_updater(lambda _: path.append(moving[0].get_x()))
            self.add(probe)
            with self.beat("two", keep=[dots]):
                self.place(*moving, region="right", direction=RIGHT)
            seen["left the array"] = not any(d in dots.submobjects for d in moving)
            seen["array width"] = dots.width
            seen["path"] = path
            seen["in right"] = all(d.get_x() > self.region("right").left for d in moving)
            seen["opacity"] = [d.get_fill_opacity() for d in moving]

    render(Gather, every_frame=True)
    assert seen["left the array"] and seen["in right"]
    assert seen["array width"] == pytest.approx(3 * 0.4 - 0.2)  # three dots remain
    assert seen["opacity"] == [1.0, 1.0, 1.0]
    steps = np.diff(seen["path"])
    assert (steps >= -1e-9).all() and (steps > 1e-3).sum() > 3  # a glide, not a jump


def test_play_over_parts_of_a_component_keeps_it_whole(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Parts(DirectedScene):
        def construct(self):
            dots = DotArray(2, 3)
            self.place(dots)
            self.play(
                AnimationGroup(Indicate(dots.at(0, 0)), FadeToColor(dots.at(1, 2), "#FF0000"))
            )
            self.play(dots.paint(lambda r, c: r == 0, "accent"))
            seen["top level"] = dots in self.mobjects
            seen["wrappers"] = [m for m in self.mobjects if type(m) is Group]
            seen["whole"] = len(dots.submobjects) == 6
            seen["painted"] = {hex_of(dots.at(0, c)) for c in range(3)}

    render(Parts)
    assert seen["top level"] and seen["whole"] and seen["wrappers"] == []
    assert seen["painted"] == {MIDNIGHT.accent}


def test_plot_misuse_names_the_fix() -> None:
    with pytest.raises(CompositionError, match=r"not finite at x=0.*breaks=\[0\]"):
        FunctionPlot(np.log, x_range=(0, 2), numbers=False)
    plot = FunctionPlot(lambda x: x, x_range=(-0.5, 2.5), numbers=False)
    with pytest.raises(
        CompositionError, match=r"x=3.2 is outside the plot's x_range \(-0.5, 2.5\)"
    ):
        plot.dot(3.2)
    with pytest.raises(CompositionError, match="curve=1, but the plot has 1 curve"):
        plot.tangent(1, curve=1)
    with pytest.raises(CompositionError, match="rule='top'"):
        plot.riemann(0, 1, 4, rule="top")
    with pytest.raises(CompositionError, match="2 labels for 1 curves"):
        FunctionPlot(lambda x: x, labels=["a", "b"])
    with pytest.raises(CompositionError, match="must lie inside"):
        plot.inset((2.4, 2), 0.5)


def test_curves_are_clipped_to_the_window_and_split_at_breaks() -> None:
    plot = FunctionPlot(
        lambda x: 1 / x, x_range=(-2, 2), y_range=(-4, 4), breaks=[0], numbers=False
    )
    window = plot._window()
    points = plot.curves[0].family_members_with_points()
    assert len(points) == 2  # one piece each side of the pole
    for piece in points:
        assert window.contains(box(piece), tolerance=1e-3)
    assert plot.curves[0].get_top()[1] == pytest.approx(window.top, abs=1e-3)
    fitted = FunctionPlot(lambda x: x * x - 1, x_range=(1, 3), numbers=False)
    assert fitted.y_range[0] <= 0 <= fitted.y_range[1]  # the fit includes 0


def test_readout_sets_its_number_on_the_label_baseline_and_follows_the_value(
    render: Render,
) -> None:
    seen: dict[str, Any] = {}

    class Live(DirectedScene):
        def construct(self):
            h = ValueTracker(1.0)
            readout = Readout("h", h)
            area = Readout("area", lambda: h.get_value() ** 4)  # 1.00, then 25.63
            seen["tex"] = (readout.name.authored_tex, area.name.authored_tex)
            widths = [area.width]
            self.place(readout, area, region="right")
            self.play(h.animate.set_value(2.25))
            widths.append(area.width)
            number, slot = readout.number, readout._slot
            seen.update(
                value=readout.number.get_value(),
                area=area.number.get_value(),
                widths=widths,
                bottom=number.get_bottom()[1] - slot.get_bottom()[1],
                left=number.get_left()[0] - slot.get_left()[0],
                size=number[0].height / slot.height,
            )

    render(Live)
    assert seen["tex"] == ("h =", r"\text{area} =")
    assert seen["value"] == pytest.approx(2.25) and seen["area"] == pytest.approx(25.63, abs=0.01)
    assert seen["widths"][0] == pytest.approx(seen["widths"][1])  # room for the digits
    assert abs(seen["bottom"]) < 1e-6 and abs(seen["left"]) < 1e-6
    assert seen["size"] == pytest.approx(1.0, abs=0.08)  # not larger than its label


def test_dot_arrays_select_count_and_refuse_what_is_not_there() -> None:
    dots = DotArray(3, 4)
    assert dots.count(lambda r, c: r == 0) == 4
    assert list(dots.select(5)) == [
        dots.at(0, 0),
        *(dots.at(0, c) for c in (1, 2, 3)),
        dots.at(1, 0),
    ]
    assert dots.at(1, 0).get_y() > dots.at(2, 0).get_y()  # rows go down
    with pytest.raises(CompositionError, match="outside the 3 × 4 array"):
        dots.at(3, 0)
    with pytest.raises(CompositionError, match="selected no dots"):
        dots.select(lambda r, c: r > 5)
    hidden = DotArray(2, shown=False)
    with pytest.raises(CompositionError, match="no visible dots; show\\(\\) them first"):
        hidden.paint(lambda r, c: True, "accent")


def test_vector_grids_reserve_room_and_move_everything_drawn_on_them(render: Render) -> None:
    seen: dict[str, Any] = {}
    dets: list[float] = []

    class Shear(DirectedScene):
        def construct(self):
            grid = VectorGrid(2, fits=[SHEAR, STRETCH])
            self.place(grid, region="left")
            room = box(grid.room)
            v, square = grid.vector((1, 1), label=r"\vec v"), grid.unit_square()
            self.show(v, square)
            self.add(Mobject().add_updater(lambda _: dets.append(grid.det())))
            self.play(grid.apply(STRETCH), run_time=1)
            seen["stretched"] = box(grid.plane)
            seen["tip"] = v[0].get_end() - grid.to_point((2, 1))
            corners = [grid.to_point(p) for p in ((0, 0), (2, 0), (2, 1), (0, 1))]
            seen["square"] = np.abs(np.asarray(square.get_vertices()) - corners).max()
            seen["room"] = room
            with pytest.raises(CompositionError, match="add that matrix to fits"):
                grid.apply([[3, 0], [0, 1]])
            self.play(grid.reset(), run_time=0.5)
            planes = zip(
                grid.plane.family_members_with_points(),
                grid.ghost.family_members_with_points(),
                strict=True,
            )
            seen["reset"] = max(np.abs(a.points - b.points).max() for a, b in planes)
            seen["det"] = grid.det()

    render(Shear, every_frame=True)
    assert seen["room"].contains(seen["stretched"], tolerance=1e-6)
    assert close(seen["tip"], 0) and seen["square"] < 1e-6
    middle = [d for d in dets if 1.05 < d < 1.95]
    assert middle and max(dets) == pytest.approx(2.0)  # live mid-animation
    assert seen["reset"] < 1e-6 and seen["det"] == pytest.approx(1.0)


def test_matrix_columns_take_the_basis_colors(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Columns(DirectedScene):
        def construct(self):
            grid = VectorGrid(1)
            matrix = grid.matrix(SHEAR)
            middle = matrix.get_center()[0]
            entries = [
                g for g in matrix.family_members_with_points() if g.height < 0.6 * matrix.height
            ]
            seen["left"] = {hex_of(g) for g in entries if g.get_center()[0] < middle}
            seen["right"] = {hex_of(g) for g in entries if g.get_center()[0] > middle}

    render(Columns)
    assert seen == {"left": {MIDNIGHT.primary}, "right": {MIDNIGHT.secondary}}


def test_figure_labels_find_room_and_later_marks_are_overlays(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Triangle(DirectedScene):
        symbols = {"a": "primary"}

        def construct(self):
            tri = Figure({"A": (0, 0), "B": (3, 0), "C": (0, 2)})
            tri.polygon("ABC")
            corner = tri.right_angle("BAC")
            tri.length("AC", "a")
            tri.length("AB", "b")
            self.place(tri, region="left")
            hypotenuse = tri.length("BC", "c", color="accent")
            seen["overlay"] = (hypotenuse.director_parent is tri, hypotenuse.director_persist)
            self.show(hypotenuse)
            a_label = tri.point_labels[0]
            seen["A outward"] = (a_label.get_x() < tri.p("A")[0], a_label.get_y() < tri.p("A")[1])
            texts = [*tri.point_labels, *tri._texts]
            seen["overlaps"] = [
                (i, j)
                for (i, s), (j, t) in combinations(enumerate(texts), 2)
                if box(s).overlaps(box(t))
            ]
            seen["convention"] = corner.director_conventions
            seen["a color"] = hex_of(tri._texts[0])
            with pytest.raises(CompositionError, match="No point 'D'"):
                tri.segment("AD")

    render(Triangle)
    assert seen["overlay"] == (True, True)
    assert seen["A outward"] == (True, True)
    assert seen["overlaps"] == []
    assert seen["convention"] == ("right angle",)
    assert seen["a color"] == MIDNIGHT.primary


def test_labels_set_words_and_math_on_one_baseline(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Note(DirectedScene):
        symbols = {"a": "primary"}

        def construct(self):
            note = label("always halve by $a$")
            words, math = note
            word_bottom = words[0].get_bottom()[1]  # "a" of "always" sits on the baseline
            seen.update(
                colors=(hex_of(words), hex_of(math)),
                baseline=abs(math.get_bottom()[1] - word_bottom),
                order=words.get_right()[0] < math.get_left()[0],
            )
            with pytest.raises(CompositionError, match="Unbalanced \\$"):
                label("half of $a")

    render(Note)
    assert seen["colors"] == (MIDNIGHT.muted, MIDNIGHT.primary)
    assert seen["baseline"] < 0.02 and seen["order"]


def test_grid_lines_reach_the_edges_of_the_grid() -> None:
    grid = VectorGrid(1, unit=1.0)
    xs = {
        round(float(line.get_x()), 3)
        for line in grid.plane.background_lines
        if line.width < 1e-6  # vertical lines
    }
    assert {-1.0, 1.0} <= xs


def test_curves_are_labeled_where_they_leave_the_window() -> None:
    plot = FunctionPlot(
        np.sin, lambda x: x, x_range=(-1, 3), y_range=(-1.2, 1.5), labels=["s", "y"]
    )
    window = plot._window()
    label = plot.labels[1]
    assert window.contains(box(label))
    assert label.get_x() < plot.axes.c2p(2, 0)[0]  # near x = 1.5, where y = x leaves the top


def test_an_inset_reads_as_one_thing_and_keeps_coinciding_curves_visible() -> None:
    plot = FunctionPlot(np.sin, lambda x: x, x_range=(-1, 3), y_range=(-1.2, 1.6))
    zoom = plot.inset(around=(0, 0), radius=0.25)
    assert (zoom.window.director_chunks, zoom.leaders.director_chunks) == (0, 0)
    under, over = (curve.get_stroke_width() for curve in zoom.curves)
    assert under > over  # where they coincide, the first shows around the second


def test_a_reserved_readout_stays_hidden_while_its_value_changes(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Hidden(DirectedScene):
        def construct(self):
            t = ValueTracker(1)
            readout = reserve(Readout("t", t))
            self.place(readout)
            self.play(t.animate.set_value(2.5))
            seen["while reserved"] = visible(readout)
            self.show(readout)
            self.play(t.animate.set_value(3))
            seen["shown"] = visible(readout.number) and visible(readout.name)

    render(Hidden, every_frame=True)
    assert seen == {"while reserved": False, "shown": True}


def test_a_slope_triangle_climbs_the_slope_times_its_run(render: Render) -> None:
    seen: dict[str, Any] = {}

    class Climb(DirectedScene):
        def construct(self):
            plot = FunctionPlot(np.exp, x_range=(-1, 2), labels=["y = e^x"])
            x = ValueTracker(0.0)
            self.place(plot)
            triangle = plot.slope_triangle(x)
            guides = plot.guides(x, label="height", color="primary")
            self.show(triangle, guides)
            self.play(x.animate.set_value(1.0))
            seen["parts"] = triangle.submobjects
            seen["guides"] = guides.submobjects
            seen["plot"] = plot

    render(Climb, every_frame=True)
    run, climb, slope, across, rise = seen["parts"]
    plot = seen["plot"]
    assert close(climb.get_end(), plot.axes.c2p(2, np.e + np.e))  # rise e over a run of 1
    assert rise.get_left()[0] > climb.get_x() and across.get_top()[1] < run.get_y()
    *drops, name = seen["guides"]
    assert name.get_left()[0] > drops[0].get_x()  # beside the drop, under the rising curve
    assert hex_of(drops[0]) == MIDNIGHT.primary


def test_secant_legs_carry_their_names_until_they_vanish() -> None:
    plot = FunctionPlot(np.exp, x_range=(-1, 2))
    h = ValueTracker(1.0)
    secant = plot.secant(0, h, labels=("h", "e^h - 1"))
    run, rise = secant.submobjects[2:4]  # after the two legs
    assert run.get_top()[1] < plot.point(0)[1] and rise.get_left()[0] > plot.point(1)[0]
    h.set_value(0.001)
    secant.update(0)
    assert max(part.width for part in secant.submobjects[2:4]) < 0.05
    with pytest.raises(CompositionError, match="two labels"):
        plot.secant(0, h, labels=("h",))


def test_equal_scale_draws_slope_one_at_45_degrees() -> None:
    plot = FunctionPlot(lambda x: x, x_range=(0, 4), y_range=(0, 2), equal_scale=True)
    corner, top = plot.axes.c2p(0, 0), plot.axes.c2p(1, 1)
    assert top[0] - corner[0] == pytest.approx(top[1] - corner[1])
    assert all(n.director_ticks for axis in plot.axes for n in axis.numbers)


def test_dots_to_come_show_as_rings_and_rings_mark_a_second_fact() -> None:
    dots = DotArray(3, shown=lambda r, c: r == 0)
    assert len(dots.ghosts) == 6 and dots.ghosts.director_ground
    assert len(DotArray(2).ghosts) == 0
    rings = dots.ring(lambda r, c: r == 0, color="secondary")
    assert len(rings.submobjects) == 3 and rings.director_parent is dots
    assert close(rings[0].get_center(), dots.at(0, 0).get_center())


def test_theme_text_keeps_its_size_and_the_fonts_spacing() -> None:
    from manim_director_runtime.kit import context

    caption = context.text("Odd numbers are the Ls of a growing square.", "caption")
    assert caption.font_size == pytest.approx(26)
