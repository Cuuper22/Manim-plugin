"""Figure: named points and the marks of geometry, with labels that find room by themselves."""

from __future__ import annotations

from collections.abc import Callable, Mapping, Sequence

import numpy as np
from manim import UP, Arc, Line, Mobject, Polygon, VectorizedPoint, VGroup, VMobject

from ..errors import CompositionError
from ..layout import Rect
from . import context
from .labels import beside, box, fan, first_free, halo, trace
from .overlay import derived, overlay, pacing
from .plot import LABEL_SCALE

STROKE = 3.0
MARK_STROKE = 2.0
_GAP = 0.12  # between a label and what it names


class Figure(VGroup):
    """Named points (`p("A")`) and the marks drawn on them.

    Marks made before `place()` are parts of the figure; marks made after are persistent
    overlays for `self.show(...)`. Point names go outward, away from the segments that meet
    there; segment labels go on the side away from the figure's middle (`side=±1` picks the
    left or right of a→b); angle labels sit on the bisector. Labels never overlap. `color` is
    the color of the point names. Right-angle and tick marks are notation a newcomer may not
    know: name them once.
    """

    def __init__(
        self,
        points: Mapping[str, Sequence[float]],
        *,
        unit: float = 1.0,
        labels: bool | Sequence[str] = True,
        color: str = "foreground",
    ) -> None:
        super().__init__()
        if not points:
            raise CompositionError("A Figure needs at least one named point.")
        if unit <= 0:
            raise CompositionError("Figure(unit=...) must be positive.")
        self.hue = context.color(color)
        self._anchors = {
            name: VectorizedPoint(np.array([float(xy[0]), float(xy[1]), 0.0]) * unit)
            for name, xy in points.items()
        }
        first = next(iter(self._anchors.values()))
        self._ruler = VectorizedPoint(first.get_center() + np.array([unit, 0.0, 0.0]))
        self._unit = float(unit)
        self.add(*self._anchors.values(), self._ruler)
        self.center()
        named = list(points) if labels is True else list(labels or ())
        for name in named:
            self._anchor(name)
        self._edges: list[tuple[str, str]] = []
        self._texts: list[Mobject] = []  # labels other labels keep clear of
        self._drawn: list[Mobject] = []  # every mark, parts and overlays
        self.marks = VGroup()
        self.point_labels = VGroup(
            *(halo(context.math(name, color=self.hue).scale(LABEL_SCALE)) for name in named)
        )
        self._named = named
        self.add(self.marks, self.point_labels)
        self._place_point_labels()
        pacing(self, "figure", chunks=1 + len(named), read=1.0 + 0.3 * len(named))

    def p(self, name: str) -> np.ndarray:
        """Where point `name` is now, in scene coordinates (follows placement)."""

        return self._anchor(name).get_center()

    # Marks ------------------------------------------------------------------------------

    def segment(
        self, ab: str, *, color: str = "foreground", label: str | None = None, side: int = 0
    ) -> VMobject:
        a, b = self._names(ab, 2)
        hue = context.color(color)
        self._edges.append((a, b))
        tag = self._tag(label, None)
        spot = None if tag is None else self._segment_spot(a, b, tag, side)

        def build() -> VMobject:
            line = Line(self.p(a), self.p(b), color=hue, stroke_width=STROKE)
            if tag is None:
                return line
            return VGroup(line, self._beside(tag, (self.p(a) + self.p(b)) / 2, spot))

        return self._mark("segment", build, labeled=tag is not None)

    def polygon(
        self,
        names: str | Sequence[str],
        *,
        fill: str = "muted",
        opacity: float = 0.25,
        color: str = "foreground",
    ) -> VMobject:
        corners = self._names(names)
        if len(corners) < 3:
            raise CompositionError(f"A polygon needs at least 3 points; got {corners}.")
        stroke, inside = context.color(color), context.color(fill)
        self._edges += list(zip(corners, corners[1:] + corners[:1], strict=True))

        def build() -> VMobject:
            return Polygon(
                *(self.p(n) for n in corners),
                stroke_color=stroke,
                stroke_width=STROKE,
                fill_color=inside,
                fill_opacity=opacity,
            ).set_z_index(-1)

        return self._mark("polygon", build)

    def angle(
        self,
        abc: str,
        *,
        label: str | None = None,
        radius: float = 0.45,
        color: str = "secondary",
    ) -> VMobject:
        """The arc of angle abc (at b, the smaller side), its label on the bisector."""

        a, b, c = self._names(abc, 3)
        hue = context.color(color)
        tag = self._tag(label, hue)

        def build() -> VMobject:
            k = self._scale()
            start, sweep, bisector = _sweep(self.p(b), self.p(a), self.p(c))
            arc = Arc(
                radius=radius * k,
                start_angle=start,
                angle=sweep,
                arc_center=self.p(b),
                color=hue,
                stroke_width=MARK_STROKE + 0.5,
            )
            if tag is None:
                return arc
            return VGroup(arc, self._beside(tag, self.p(b) + radius * k * bisector, bisector))

        return self._mark("angle", build, labeled=tag is not None)

    def right_angle(self, abc: str, *, size: float = 0.25, color: str = "muted") -> VMobject:
        a, b, c = self._names(abc, 3)
        hue = context.color(color)

        def build() -> VMobject:
            s = size * self._scale()
            corner = self.p(b)
            u, v = _toward(corner, self.p(a)), _toward(corner, self.p(c))
            path = [corner + s * u, corner + s * (u + v), corner + s * v]
            return VMobject(color=hue, stroke_width=MARK_STROKE).set_points_as_corners(path)

        return self._mark("right angle", build, conventions=("right angle",))

    def ticks(self, *segments: str, count: int = 1, color: str = "muted") -> VMobject:
        """`count` short ticks across the middle of each segment: these lengths are equal."""

        if not segments or count < 1:
            raise CompositionError("ticks() needs segments, e.g. ticks('AB', 'AC').")
        ends = [self._names(ab, 2) for ab in segments]
        hue = context.color(color)

        def build() -> VMobject:
            k, marks = self._scale(), VGroup()
            for a, b in ends:
                along = _toward(self.p(a), self.p(b))
                across = np.array([-along[1], along[0], 0.0]) * 0.12 * k
                middle = (self.p(a) + self.p(b)) / 2
                for i in range(count):
                    at = middle + along * 0.09 * k * (i - (count - 1) / 2)
                    marks.add(Line(at - across, at + across, color=hue, stroke_width=MARK_STROKE))
            return marks

        return self._mark("ticks", build, conventions=("equal length",))

    def length(self, ab: str, label: str, *, side: int = 0, color: str | None = None) -> VMobject:
        """A label for the length of ab, outside the figure at the segment's middle. With
        `color=None` its symbols keep their colors (`a` in its `symbols` color)."""

        a, b = self._names(ab, 2)
        tag = self._tag(label, None if color is None else context.color(color))
        spot = self._segment_spot(a, b, tag, side)

        def build() -> VMobject:
            return self._beside(tag, (self.p(a) + self.p(b)) / 2, spot)

        return self._mark("length", build, labeled=tag is not None)

    # Internals --------------------------------------------------------------------------

    def _mark(
        self,
        kind: str,
        build: Callable[[], VMobject],
        *,
        labeled: bool = False,
        conventions: Sequence[str] = (),
    ) -> VMobject:
        if self._placed():
            mark = overlay(derived(build), self, kind, persist=True)
        else:
            mark = pacing(build(), kind)
            self.marks.add(mark)
        self._drawn.append(mark)
        if labeled:  # the label is the mark, or its last part
            self._texts.append(mark[-1] if isinstance(mark, VGroup) else mark)
        mark.director_conventions = tuple(conventions)
        if not self._placed():
            self._place_point_labels()
        return mark

    def _placed(self) -> bool:
        scene = context.scene()
        return scene is not None and scene._holds(self)

    def _anchor(self, name: str) -> VectorizedPoint:
        try:
            return self._anchors[name]
        except KeyError:
            raise CompositionError(
                f"No point {name!r} in this figure; it has {', '.join(self._anchors)}."
            ) from None

    def _names(self, spec: str | Sequence[str], count: int | None = None) -> list[str]:
        """`"ABC"` (one-letter names), `"P1 P2"` or `["P1", "P2"]` as a list of names."""

        if isinstance(spec, str):
            names = spec.split() if " " in spec else list(spec)
            if " " not in spec and spec in self._anchors:
                names = [spec]
        else:
            names = list(spec)
        for name in names:
            self._anchor(name)
        if count is not None and len(names) != count:
            raise CompositionError(f"{spec!r} names {len(names)} points; this mark needs {count}.")
        return names

    def _scale(self) -> float:
        anchor = next(iter(self._anchors.values()))
        return float(np.linalg.norm(self._ruler.get_center() - anchor.get_center())) / self._unit

    def _centroid(self) -> np.ndarray:
        return np.mean([anchor.get_center() for anchor in self._anchors.values()], axis=0)

    def _tag(self, label: str | None, hue: str | None) -> Mobject | None:
        """A label at the figure's unplaced scale; marks draw scaled copies of it."""

        if label is None:
            return None
        options = {} if hue is None else {"color": hue}
        return halo(context.math(label, **options).scale(LABEL_SCALE))

    def _beside(self, tag: Mobject, anchor: np.ndarray, direction: np.ndarray) -> Mobject:
        """A copy of `tag` at the figure's scale, set beside `anchor`. Labels keep the side
        they chose when they were made, so they never jump as the figure moves."""

        k = self._scale()
        return beside(tag.copy().scale(k), anchor, direction, _GAP * k)

    def _segment_spot(self, a: str, b: str, tag: Mobject, side: int) -> np.ndarray:
        along = _toward(self.p(a), self.p(b))
        left = np.array([-along[1], along[0], 0.0])
        middle = (self.p(a) + self.p(b)) / 2
        if side not in (-1, 0, 1):
            raise CompositionError(f"side={side!r}; use 1 (left of a→b), -1 (right) or 0.")
        if side == 0:
            outward = middle - self._centroid()
            side = 1 if np.dot(outward, left) >= 0 else -1
        normal = side * left
        _, direction = first_free(
            tag.copy().scale(self._scale()),
            [(middle, d) for d in fan(normal, step_degrees=20, count=4)],
            gap=_GAP * self._scale(),
            avoid=self._obstacles(),
        )
        return direction

    def _place_point_labels(self) -> None:
        taken: list[Rect] = []
        strokes = self._strokes()
        for name, tag in zip(self._named, self.point_labels, strict=True):
            first_free(
                tag,
                [(self.p(name), d) for d in fan(self._outward(name), step_degrees=25, count=10)],
                gap=_GAP * self._scale(),
                avoid=[*taken, *strokes, *(box(t, 0.04) for t in self._texts)],
            )
            taken.append(box(tag, 0.04))

    def _outward(self, name: str) -> np.ndarray:
        """Away from the segments that meet at the point, else away from the middle."""

        here = self.p(name)
        spokes = [
            _toward(here, self.p(b if a == name else a))
            for a, b in self._edges
            if name in (a, b) and a != b
        ]
        mean = np.sum(spokes, axis=0) if spokes else np.zeros(3)
        if np.linalg.norm(mean) > 0.2:
            return -mean / np.linalg.norm(mean)
        away = here - self._centroid()
        if spokes:  # on a straight run: go to the side away from the middle
            across = np.array([-spokes[0][1], spokes[0][0], 0.0])
            return across if np.dot(across, away) >= 0 else -across
        return away / np.linalg.norm(away) if np.linalg.norm(away) > 1e-9 else UP

    def _strokes(self) -> list[Rect]:
        """Boxes along every drawn line, for labels to keep clear of."""

        texts = {id(m) for t in self._texts for m in t.get_family()}
        lines = [
            leaf
            for mark in self._drawn
            for leaf in mark.family_members_with_points()
            if id(leaf) not in texts
        ]
        return trace(lines, pad=0.06 * self._scale())

    def _obstacles(self) -> list[Rect]:
        return [
            *self._strokes(),
            *(box(t, 0.04) for t in self._texts),
            *(box(t, 0.04) for t in self.point_labels),
        ]


def _toward(start: np.ndarray, end: np.ndarray) -> np.ndarray:
    d = end - start
    norm = np.linalg.norm(d)
    if norm < 1e-9:
        raise CompositionError("Two points of this mark coincide.")
    return d / norm


def _sweep(vertex: np.ndarray, a: np.ndarray, c: np.ndarray) -> tuple[float, float, np.ndarray]:
    """Start angle and sweep (< π) of the arc from ray va to ray vc, and the unit bisector."""

    u, w = _toward(vertex, a), _toward(vertex, c)
    start = float(np.arctan2(u[1], u[0]))
    sweep = float(np.arctan2(w[1], w[0])) - start
    sweep = (sweep + np.pi) % (2 * np.pi) - np.pi
    middle = start + sweep / 2
    return start, sweep, np.array([np.cos(middle), np.sin(middle), 0.0])
