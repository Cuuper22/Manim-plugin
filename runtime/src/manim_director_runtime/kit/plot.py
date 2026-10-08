"""FunctionPlot and Readout: graphs whose overlays are drawn from the axes, and live numbers."""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass

import numpy as np
from manim import (
    DL,
    DOWN,
    DR,
    LEFT,
    RIGHT,
    UL,
    UP,
    UR,
    Animation,
    Axes,
    DashedLine,
    DecimalNumber,
    Dot,
    GrowFromPoint,
    LaggedStart,
    Line,
    MathTex,
    Mobject,
    Polygon,
    Rectangle,
    Transform,
    VGroup,
    VMobject,
)

from ..errors import CompositionError
from ..layout import Rect
from ..texscan import atoms, is_relation, upright_words
from . import context
from .labels import beside, box, first_free, halo, inside, trace
from .overlay import Live, derived, now, overlay, pacing, reserve, reserved

Function = Callable[[float], float]

LABEL_SCALE = 0.72  # math labels drawn on a component, relative to formulas
NUMBER_SIZE = 22
CURVE_WIDTH = 4.0
DOT_RADIUS = 0.07
_SAMPLES = 400
_GUIDE = {"stroke_width": 2.0, "dashed_ratio": 0.55}


class FunctionPlot(VGroup):
    """Axes with one or more curves, each labeled at its right end in its own color.

    Overlays (`dot`, `guides`, `tangent`, `secant`, `area`, `riemann`) are recomputed from the
    axes every frame, so they stay exact wherever `place()` moves or scales the plot, and they
    are clipped to its window. Curves are clipped to the window too, and split at `breaks`.
    """

    def __init__(
        self,
        *functions: Function,
        x_range: Sequence[float] = (-1, 3),
        y_range: Sequence[float] | None = None,
        size: tuple[float, float] = (6.0, 4.5),
        labels: Sequence[str | None] = (),
        colors: Sequence[str] = ("primary", "secondary", "foreground"),
        numbers: bool = True,
        axis_labels: tuple[str, str] | None = None,
        breaks: Sequence[float] = (),
    ) -> None:
        super().__init__()
        if not functions:
            raise CompositionError(
                "FunctionPlot() needs a function, e.g. FunctionPlot(lambda x: x**2)."
            )
        if len(labels) > len(functions):
            raise CompositionError(
                f"{len(labels)} labels for {len(functions)} curves: give one per curve."
            )
        self.functions = tuple(functions)
        x0, x1, x_step = _range(x_range, "x_range")
        self.x_range = (x0, x1)
        self.breaks = tuple(sorted(float(b) for b in breaks))
        for b in self.breaks:
            if not x0 < b < x1:
                raise CompositionError(f"breaks: {b:g} is outside x_range ({x0:g}, {x1:g}).")
        samples = [self._sample(i) for i in range(len(functions))]
        if y_range is None:
            y0, y1, y_step = _fit(samples)
        else:
            y0, y1, y_step = _range(y_range, "y_range")
        self.y_range = (y0, y1)
        self._size = (float(size[0]), float(size[1]))
        self.colors = [context.color(colors[i % len(colors)]) for i in range(len(functions))]
        muted = context.color("muted")
        self.axes = Axes(
            x_range=[x0, x1, x_step],
            y_range=[y0, y1, y_step],
            x_length=self._size[0],
            y_length=self._size[1],
            tips=False,
            axis_config={"color": muted, "stroke_width": 2, "tick_size": 0.05},
        )
        self._cross = (_crossing(x0, x1), _crossing(y0, y1))
        if numbers:
            self._number_axes(x_step, y_step, muted)
        self.curves = VGroup(*(self._curve(i) for i in range(len(functions))))
        self.labels = VGroup()
        for i, text in enumerate(labels):
            if text is not None:
                self.labels.add(self._curve_label(i, text))
        self.axis_labels = VGroup()
        if axis_labels is not None:
            self._label_axes(*axis_labels, muted)
        self.add(self.axes, self.curves, self.labels, self.axis_labels)
        pacing(self, "plot", chunks=len(functions), read=1.0 + 0.5 * len(self.labels))

    # Coordinates ------------------------------------------------------------------------

    def f(self, x: float, curve: int = 0) -> float:
        return _evaluate(self._function(curve), float(x), curve)

    def point(self, x: Live, curve: int = 0) -> np.ndarray:
        """Where the curve is at `x`, in scene coordinates (follows placement)."""

        at = self._x(x)
        return self.axes.c2p(at, self.f(at, curve))

    # Overlays ---------------------------------------------------------------------------

    def dot(
        self, x: Live, curve: int = 0, *, color: str = "foreground", label: str | None = None
    ) -> VMobject:
        """A point on the curve; `label` (TeX) sits beside it, away from the curve."""

        self._require(x, curve=curve)
        hue = context.color(color)
        tag = None if label is None else halo(context.math(label, color=hue).scale(LABEL_SCALE))
        side = UL if self._slope(self._x(x), curve) >= 0 else UR

        def build() -> VMobject:
            k, p = self._scale(), self.point(x, curve)
            mark = Dot(p, radius=DOT_RADIUS * k, color=hue)
            if tag is None:
                return mark
            return VGroup(mark, beside(tag.copy().scale(k), p, side, 0.1 * k))

        return overlay(derived(build), self, "dot")

    def guides(self, x: Live, curve: int = 0) -> VMobject:
        """Dashed drops from the curve's point at `x` to both axes."""

        self._require(x, curve=curve)
        muted = context.color("muted")

        def build() -> VMobject:
            at, k = self._x(x), self._scale()
            y = self.f(at, curve)
            p = self.axes.c2p(at, y)
            return VGroup(
                _dashed(self.axes.c2p(at, self._cross[1]), p, muted, k),
                _dashed(self.axes.c2p(self._cross[0], y), p, muted, k),
            )

        return overlay(derived(build), self, "guides")

    def tangent(
        self, x: Live, curve: int = 0, *, length: float = 0.45, color: str = "accent"
    ) -> VMobject:
        """The tangent line at `x`, `length` times the plot's width, centered on the point."""

        self._require(x, curve=curve)
        hue = context.color(color)

        def build() -> VMobject:
            at = self._x(x)
            p = self.axes.c2p(at, self.f(at, curve))
            half = length * self._width() / 2
            d = self._direction(at, self._slope(at, curve), curve)
            return self._clipped(p - half * d, p + half * d, hue)

        return overlay(derived(build), self, "tangent")

    def secant(
        self, x: Live, h: Live, curve: int = 0, *, color: str = "accent", legs: bool = True
    ) -> VMobject:
        """The line through the points at `x` and `x + h`, with dashed legs showing the run
        and the rise; shrink `h` and it turns into the tangent."""

        self._require(x, lambda: now(x) + now(h), curve=curve)
        hue, muted, ink = context.color(color), context.color("muted"), context.color("foreground")

        def build() -> VMobject:
            at, step, k = self._x(x), now(h), self._scale()
            fp, fq = self.f(at, curve), self.f(at + step, curve)
            p, q = self.axes.c2p(at, fp), self.axes.c2p(at + step, fq)
            slope = (fq - fp) / step if abs(step) > 1e-9 else self._slope(at, curve)
            d = self._direction(at, slope, curve)
            reach = float(np.dot(q - p, d))
            margin = 0.12 * self._width()
            parts = VGroup()
            if legs:
                corner = self.axes.c2p(at + step, fp)
                parts.add(_dashed(p, corner, muted, k), _dashed(corner, q, muted, k))
            start, end = min(0.0, reach) - margin, max(0.0, reach) + margin
            parts.add(self._clipped(p + start * d, p + end * d, hue))
            radius = DOT_RADIUS * k
            parts.add(Dot(p, radius=radius, color=ink), Dot(q, radius=radius, color=hue))
            return parts

        return overlay(derived(build), self, "secant")

    def area(
        self,
        a: Live,
        b: Live,
        curve: int = 0,
        *,
        under: int | None = None,
        color: str = "primary",
        opacity: float = 0.35,
    ) -> VMobject:
        """The region between the curve and the x-axis (or the curve `under`) from a to b."""

        self._require(a, b, curve=curve)
        if under is not None:
            self._function(under)
        hue = context.color(color)

        def build() -> VMobject:
            lo, hi = sorted((self._x(a), self._x(b)))
            xs = np.linspace(lo, hi, 96)
            top = [self._clamped(x, self.f(x, curve)) for x in xs]
            if under is None:
                bottom = [self._clamped(hi, 0.0), self._clamped(lo, 0.0)]
            else:
                bottom = [self._clamped(x, self.f(x, under)) for x in xs[::-1]]
            return Polygon(*top, *bottom, stroke_width=0, fill_color=hue, fill_opacity=opacity)

        shape = derived(build).set_z_index(-1)
        return overlay(shape, self, "area")

    def riemann(
        self,
        a: float,
        b: float,
        n: int,
        *,
        rule: str = "left",
        curve: int = 0,
        color: str = "primary",
    ) -> VMobject:
        """`n` rectangles under the curve on [a, b], their heights sampled at each bar's left
        edge, middle or right edge (`rule`). `refine(bars)` splits every bar in place."""

        if rule not in _RULES:
            raise CompositionError(f"rule={rule!r}; use 'left', 'mid' or 'right'.")
        if not (isinstance(n, int) and n >= 1):
            raise CompositionError(f"riemann() needs a whole number of bars; got n={n!r}.")
        if not a < b:
            raise CompositionError(f"riemann() needs a < b; got a={a:g}, b={b:g}.")
        self._require(a, b, curve=curve)
        spec = _Riemann(float(a), float(b), n, _RULES[rule], curve, context.color(color))
        bars = derived(lambda: self._bars(spec, spec.n)).set_z_index(-1)
        bars.riemann = spec
        bars.director_entrance = self._rise
        return overlay(bars, self, "riemann")

    def inset(
        self, around: tuple[float, float], radius: float, *, size: tuple[float, float] = (3.2, 3.2)
    ) -> FunctionPlot:
        """A zoomed copy of the plot around a point, with the same slopes as the original.
        Place it (usually beside the plot): its `window` on this plot and the `leaders`
        joining the two enter and leave with it."""

        cx, cy = float(around[0]), float(around[1])
        x0, x1 = self.x_range
        if radius <= 0 or not x0 <= cx - radius < cx + radius <= x1:
            raise CompositionError(
                f"inset(around=({cx:g}, {cy:g}), radius={radius:g}) must lie inside the "
                f"plot's x_range ({x0:g}, {x1:g})."
            )
        (width, height), (y0, y1) = self._size, self.y_range
        aspect = ((y1 - y0) / height) / ((x1 - x0) / width)
        ry = radius * aspect * size[1] / size[0]
        if not y0 <= cy - ry < cy + ry <= y1:
            raise CompositionError(
                f"inset() around y={cy:g} reaches outside the plot's y_range ({y0:g}, {y1:g})."
            )
        zoom = FunctionPlot(
            *self.functions,
            x_range=(cx - radius, cx + radius),
            y_range=(cy - ry, cy + ry),
            size=size,
            colors=self.colors,
            breaks=[b for b in self.breaks if cx - radius < b < cx + radius],
        )
        # Up close the curves often coincide: each lies wider under the next, so all show.
        for i, curve in enumerate(zoom.curves):
            curve.set_stroke(width=CURVE_WIDTH * (1 + 1.2 * (len(zoom.curves) - 1 - i)))
        accent = context.color("accent")
        zoom.border = Rectangle(width=size[0], height=size[1], color=accent, stroke_width=2)
        zoom.border.move_to(zoom.axes.c2p(cx, cy))
        zoom.add(zoom.border)

        def window() -> VMobject:
            corners = [self.axes.c2p(cx + sx * radius, cy + sy * ry) for sx, sy in _CORNERS]
            return Polygon(*corners, color=accent, stroke_width=2, fill_opacity=0)

        def leaders() -> VMobject:
            (left, bottom), (right, top) = (
                self.axes.c2p(cx - radius, cy - ry)[:2],
                self.axes.c2p(cx + radius, cy + ry)[:2],
            )
            pairs = _facing_corners(Rect(left, bottom, right, top), box(zoom.border))
            style = {"color": accent, "stroke_width": 1.5, "stroke_opacity": 0.55}
            return VGroup(*(Line(a, b, **style) for a, b in pairs))

        # The window and its leaders are read as part of the inset, not as more things.
        zoom.window = overlay(derived(window), zoom, "inset window", persist=True)
        zoom.leaders = overlay(derived(leaders), zoom, "inset leaders", persist=True)
        for part in (zoom.window, zoom.leaders):
            pacing(part, part.director_kind, chunks=0, read=0.0)
        zoom.director_companions = (zoom.window, zoom.leaders)
        return pacing(zoom, "inset", chunks=1, read=1.0)

    # Verb -------------------------------------------------------------------------------

    def refine(self, bars: Mobject, factor: int = 2) -> Animation:
        """Split every bar of a `riemann` overlay into `factor` thinner bars, in place: the
        viewer sees the same area get closer to the curve."""

        spec = getattr(bars, "riemann", None)
        if spec is None or getattr(bars, "director_parent", None) is not self:
            raise CompositionError("refine() takes the bars that this plot's riemann() made.")
        if not (isinstance(factor, int) and factor >= 2):
            raise CompositionError(f"refine(factor={factor!r}) needs a whole number >= 2.")
        return _Refine(bars, self._bars(spec, spec.n * factor), spec, factor)

    # Internals --------------------------------------------------------------------------

    def _function(self, curve: int) -> Function:
        if not 0 <= curve < len(self.functions):
            raise CompositionError(
                f"curve={curve}, but the plot has {len(self.functions)} curve(s) (0-based)."
            )
        return self.functions[curve]

    def _require(self, *xs: Live, curve: int) -> None:
        """Check an overlay's inputs when it is made, not when it fails mid-render."""

        for x in xs:
            self.point(x, curve)

    def _x(self, x: Live) -> float:
        at = now(x)
        x0, x1 = self.x_range
        if not x0 - 1e-9 <= at <= x1 + 1e-9:
            raise CompositionError(f"x={at:g} is outside the plot's x_range ({x0:g}, {x1:g}).")
        return at

    def _scale(self) -> float:
        """How much `place()` has scaled the plot since it was built."""

        x0, x1 = self.x_range
        return float(np.linalg.norm(self.axes.c2p(x1, 0) - self.axes.c2p(x0, 0))) / self._size[0]

    def _width(self) -> float:
        return self._size[0] * self._scale()

    def _window(self) -> Rect:
        (x0, x1), (y0, y1) = self.x_range, self.y_range
        left, bottom = self.axes.c2p(x0, y0)[:2]
        right, top = self.axes.c2p(x1, y1)[:2]
        return Rect(float(left), float(bottom), float(right), float(top))

    def _slope(self, x: float, curve: int) -> float:
        x0, x1 = self.x_range
        h = 1e-5 * (x1 - x0)
        lo, hi = max(x0, x - h), min(x1, x + h)
        return (self.f(hi, curve) - self.f(lo, curve)) / (hi - lo)

    def _direction(self, x: float, slope: float, curve: int) -> np.ndarray:
        """The unit vector, in scene coordinates, of a line with `slope` through the curve."""

        y = self.f(x, curve)
        d = self.axes.c2p(x + 1, y + slope) - self.axes.c2p(x, y)
        return d / np.linalg.norm(d)

    def _clamped(self, x: float, y: float) -> np.ndarray:
        y0, y1 = self.y_range
        return self.axes.c2p(x, min(max(y, y0), y1))

    def _clipped(self, a: np.ndarray, b: np.ndarray, hue: str) -> Line:
        ends = _clip(a, b, self._window())
        start, end = ends if ends is not None else (a, a + 1e-4 * RIGHT)
        return Line(start, end, color=hue, stroke_width=CURVE_WIDTH)

    def _sample(self, curve: int) -> tuple[np.ndarray, np.ndarray]:
        """Points of the curve for fitting the y range; values next to a break are skipped,
        and anything else that is not finite is an error."""

        f = self.functions[curve]
        x0, x1 = self.x_range
        xs = np.linspace(x0, x1, _SAMPLES)
        margin = 0.02 * (x1 - x0)
        xs = xs[[all(abs(x - b) > margin for b in self.breaks) for x in xs]]
        return xs, np.array([_evaluate(f, float(x), curve) for x in xs])

    def _curve(self, curve: int) -> VMobject:
        f, hue = self.functions[curve], self.colors[curve]
        (x0, x1), (y0, y1) = self.x_range, self.y_range
        edges = [x0, *self.breaks, x1]
        pieces = VGroup()
        for lo, hi in zip(edges, edges[1:], strict=False):
            gap = 1e-4 * (x1 - x0)
            lo, hi = lo + (gap if lo in self.breaks else 0), hi - (gap if hi in self.breaks else 0)
            for a, b in _visible_runs(f, lo, hi, y0, y1):
                pieces.add(
                    self.axes.plot(
                        f, x_range=[a, b, (b - a) / 160], color=hue, stroke_width=CURVE_WIDTH
                    )
                )
        if not pieces.submobjects:
            raise CompositionError(
                f"Curve {curve} never enters the window y_range ({y0:g}, {y1:g})."
            )
        return pieces[0] if len(pieces) == 1 else pieces

    def _curve_label(self, curve: int, text: str) -> Mobject:
        """The label goes beside the right end of the curve where it leaves the window, or a
        little back along the curve, wherever it is clear of the curves and earlier labels."""

        name = halo(context.math(text, color=self.colors[curve]).scale(LABEL_SCALE))
        x0, x1 = self.x_range
        runs = _visible_runs(self.functions[curve], x0, x1, *self.y_range)
        end = runs[-1][1] if runs else x1
        stops = [self._last_visible(curve, end - share * (x1 - x0)) for share in _LABEL_STOPS]
        candidates = [(p, d) for p in stops if p is not None for d in _LABEL_SIDES]
        numbers = [n for axis in self.axes for n in getattr(axis, "numbers", [])]
        taken = [box(m, 0.08) for m in [*self.labels, *numbers]]
        taken += trace([self.curves, self.axes])
        first_free(name, candidates, gap=0.1, avoid=taken, within=self._window())
        return inside(name, self._window())  # even when no spot was free

    def _last_visible(self, curve: int, x: float) -> np.ndarray | None:
        """The curve's point at `x`, if it is drawn there."""

        y0, y1 = self.y_range
        try:
            y = self.f(x, curve)
        except CompositionError:
            return None
        return self.axes.c2p(x, y) if y0 <= y <= y1 else None

    def _number_axes(self, x_step: float, y_step: float, muted: str) -> None:
        for axis, (lo, hi), step, cross in (
            (self.axes.x_axis, self.x_range, x_step, self._cross[0]),
            (self.axes.y_axis, self.y_range, y_step, self._cross[1]),
        ):
            ticks = [t for t in _ticks(lo, hi, step) if abs(t - cross) > step / 2]
            axis.add_numbers(ticks, font_size=NUMBER_SIZE, num_decimal_places=_places(step))
            axis.numbers.set_color(muted)

    def _label_axes(self, x_name: str, y_name: str, muted: str) -> None:
        x_end = self.axes.c2p(self.x_range[1], self._cross[1])
        y_end = self.axes.c2p(self._cross[0], self.y_range[1])
        x_label = context.math(x_name, color=muted).scale(LABEL_SCALE)
        y_label = context.math(y_name, color=muted).scale(LABEL_SCALE)
        self.axis_labels.add(
            x_label.next_to(x_end, RIGHT, buff=0.15), y_label.next_to(y_end, UP, buff=0.15)
        )

    def _bars(self, spec: _Riemann, n: int) -> VGroup:
        width = (spec.b - spec.a) / n
        stroke = max(0.5, min(2.0, 16 / n))
        bars = VGroup()
        for k in range(n):
            left = spec.a + k * width
            height = self.f(left + spec.sample * width, spec.curve)
            corners = [(left, 0.0), (left + width, 0.0), (left + width, height), (left, height)]
            bars.add(
                Polygon(
                    *(self._clamped(x, y) for x, y in corners),
                    stroke_color=spec.color,
                    stroke_width=stroke,
                    stroke_opacity=0.9,
                    fill_color=spec.color,
                    fill_opacity=0.3,
                )
            )
        return bars

    def _rise(self, bars: Mobject) -> Animation:
        """Bars grow out of the x-axis, left to right."""

        y0, y1 = self.y_range
        base = min(max(0.0, y0), y1)
        return LaggedStart(
            *(
                GrowFromPoint(bar, self.axes.c2p(self.axes.p2c(bar.get_center())[0], base))
                for bar in bars.family_members_with_points()
            ),
            lag_ratio=0.08,
        )


class Readout(VGroup):
    """`label = value`: a live number set in the label's math font, on its baseline.

    `value` is a number, a ValueTracker or a function read every frame (`grid.det`). The
    number keeps its left edge, and `width` digits are reserved, so nothing around it shifts
    as it changes. The label is TeX: words in it are set upright (`area` reads as a word, not
    a·r·e·a), and a label without a relation gets " =".
    """

    def __init__(
        self,
        label: str,
        value: Live,
        *,
        decimals: int = 2,
        unit: str | None = None,
        color: str = "foreground",
        width: int = 6,
    ) -> None:
        super().__init__()
        label = upright_words(label)
        if not atoms(label) or not is_relation(atoms(label)[-1]):
            label = f"{label} ="
        self.source = value
        self.name = context.math(label)
        # The label typeset with a trailing 0: that 0 shows where digits start and sit.
        probe = context.math(f"{label} 0")
        probe.shift(_first_glyph(self.name).get_center() - _first_glyph(probe).get_center())
        self._slot = probe[-1].set_opacity(0)
        hue = context.color(color)
        self.number = DecimalNumber(
            now(value), num_decimal_places=decimals, color=hue, font_size=self.name.font_size
        )
        self.number.scale(self._slot.height / _digit(self.number).height)
        self.unit = None if unit is None else context.math(rf"\mathrm{{{unit}}}", color=hue)
        advance = self._slot.width * 1.08
        self._room = Rectangle(
            width=advance * width, height=self.name.height, stroke_opacity=0, fill_opacity=0
        )
        self._room.move_to(self._slot.get_left(), aligned_edge=LEFT)
        self.add(self.name, self._slot, self._room, self.number)
        if self.unit is not None:
            self.add(self.unit)
        self._align()
        number = self.number

        def update(m: Mobject) -> None:
            if m is number:
                number.set_value(now(self.source))  # new digits: keep them reserved too
                if reserved(self.name) and not reserved(number):
                    reserve(number)
                self._align()

        update.director = True  # type: ignore[attr-defined]
        self.number.add_updater(update)
        pacing(self, "readout", chunks=1, read=1.0)

    def _align(self) -> None:
        slot = self._slot
        self.number.shift(slot.get_corner(DL) - self.number.get_corner(DL))
        if self.unit is not None:
            self.unit.next_to(self.number, RIGHT, buff=slot.width * 0.4)
            self.unit.shift((slot.get_bottom()[1] - self.unit[0].get_bottom()[1]) * UP)


@dataclass(eq=False)
class _Riemann:
    a: float
    b: float
    n: int
    sample: float  # where each bar takes its height: 0 left edge, 0.5 middle, 1 right edge
    curve: int
    color: str


class _Refine(Transform):
    def __init__(self, bars: Mobject, finer: VGroup, spec: _Riemann, factor: int) -> None:
        n = spec.n
        target = VGroup(*(VGroup(*finer[k * factor : (k + 1) * factor]) for k in range(n)))
        super().__init__(bars, target)
        self._spec, self._count = spec, n * factor

    def begin(self) -> None:
        super().begin()
        self._spec.n = self._count  # the bars redraw at the new count once this finishes


_RULES = {"left": 0.0, "mid": 0.5, "right": 1.0}
_LABEL_STOPS = (0.0, 0.08, 0.16, 0.24, 0.32)  # shares of the x range back from the right end
_LABEL_SIDES = (UL, DR, DL, UR, UP, DOWN)
_CORNERS = ((-1, -1), (1, -1), (1, 1), (-1, 1))


def _evaluate(f: Function, x: float, curve: int) -> float:
    try:
        y = float(f(x))
    except (ArithmeticError, ValueError):
        y = float("nan")
    if not np.isfinite(y):
        raise CompositionError(
            f"Curve {curve} is not finite at x={x:g}: narrow x_range, or split the curve "
            f"there with breaks=[{x:g}].",
            x=x,
        )
    return y


def _range(spec: Sequence[float], name: str) -> tuple[float, float, float]:
    if len(spec) not in (2, 3):
        raise CompositionError(f"{name} is (min, max) or (min, max, step); got {tuple(spec)}.")
    lo, hi = float(spec[0]), float(spec[1])
    if not lo < hi:
        raise CompositionError(f"{name} needs min < max; got {tuple(spec)}.")
    step = float(spec[2]) if len(spec) == 3 else _nice(hi - lo)
    if step <= 0:
        raise CompositionError(f"{name} needs a positive step; got {step:g}.")
    return lo, hi, step


def _fit(samples: list[tuple[np.ndarray, np.ndarray]]) -> tuple[float, float, float]:
    ys = np.concatenate([ys for _, ys in samples])
    lo, hi = min(0.0, float(ys.min())), max(0.0, float(ys.max()))
    if hi - lo < 1e-9:
        lo, hi = lo - 1, hi + 1
    pad = 0.08 * (hi - lo)
    return lo - (pad if lo < 0 else 0), hi + pad, _nice(hi - lo)


def _nice(span: float) -> float:
    """A 1, 2 or 5 step that gives about five ticks."""

    raw = span / 5
    magnitude = 10 ** np.floor(np.log10(raw))
    return float(next(m * magnitude for m in (1, 2, 5, 10) if raw <= m * magnitude))


def _places(step: float) -> int:
    return next(p for p in range(6) if abs(round(step, p) - step) < 1e-9)


def _ticks(lo: float, hi: float, step: float) -> list[float]:
    first = np.ceil(lo / step - 1e-9) * step
    return [round(t, 9) for t in np.arange(first, hi + step * 1e-6, step)]


def _crossing(lo: float, hi: float) -> float:
    """Where the other axis crosses this range (as Manim's Axes places it)."""

    return lo if lo > 0 else hi if hi < 0 else 0.0


def _visible_runs(f: Function, lo: float, hi: float, y0: float, y1: float) -> list:
    """The x intervals of [lo, hi] where f stays in [y0, y1], ends found by bisection."""

    def inside(x: float) -> bool:
        try:
            y = float(f(x))
        except (ArithmeticError, ValueError):
            return False
        return bool(np.isfinite(y)) and y0 <= y <= y1

    def edge(outside: float, within: float) -> float:
        for _ in range(40):
            middle = (outside + within) / 2
            outside, within = (outside, middle) if inside(middle) else (middle, within)
        return within

    xs = np.linspace(lo, hi, 800)
    flags = [inside(float(x)) for x in xs]
    runs, start = [], None
    for i, flag in enumerate([*flags, False]):
        if flag and start is None:
            start = i
        elif not flag and start is not None:
            a = float(xs[start]) if start == 0 else edge(float(xs[start - 1]), float(xs[start]))
            last = i - 1
            b = float(xs[last]) if last == len(xs) - 1 else edge(float(xs[i]), float(xs[last]))
            if b - a > 1e-9:
                runs.append((a, b))
            start = None
    return runs


def _clip(a: np.ndarray, b: np.ndarray, rect: Rect) -> tuple[np.ndarray, np.ndarray] | None:
    """The part of segment ab inside `rect` (Liang-Barsky), or None."""

    d = b - a
    t0, t1 = 0.0, 1.0
    for p, q in (
        (-d[0], a[0] - rect.left),
        (d[0], rect.right - a[0]),
        (-d[1], a[1] - rect.bottom),
        (d[1], rect.top - a[1]),
    ):
        if abs(p) < 1e-12:
            if q < 0:
                return None
            continue
        t = q / p
        if p < 0:
            t0 = max(t0, t)
        else:
            t1 = min(t1, t)
    return None if t0 > t1 else (a + t0 * d, a + t1 * d)


def _dashed(a: np.ndarray, b: np.ndarray, hue: str, k: float) -> VMobject:
    if np.linalg.norm(b - a) < 1e-3:
        return Line(a, a + 1e-4 * RIGHT, color=hue, stroke_width=_GUIDE["stroke_width"])
    return DashedLine(a, b, color=hue, dash_length=0.08 * k, **_GUIDE)


def _facing_corners(near: Rect, far: Rect) -> list[tuple[np.ndarray, np.ndarray]]:
    """The two corner pairs that join the facing sides of two boxes (a magnifier's lines)."""

    def corner(r: Rect, x: str, y: str) -> np.ndarray:
        return np.array([getattr(r, x), getattr(r, y), 0.0])

    dx = far.center[0] - near.center[0]
    dy = far.center[1] - near.center[1]
    if abs(dx) >= abs(dy):
        a, b = ("right", "left") if dx > 0 else ("left", "right")
        return [(corner(near, a, y), corner(far, b, y)) for y in ("top", "bottom")]
    a, b = ("top", "bottom") if dy > 0 else ("bottom", "top")
    return [(corner(near, x, a), corner(far, x, b)) for x in ("left", "right")]


def _first_glyph(tex: MathTex) -> Mobject:
    return tex.family_members_with_points()[0]


def _digit(number: DecimalNumber) -> Mobject:
    digits = [m for m in number.submobjects if m.height > 0.5 * number.height]
    return digits[0] if digits else number
