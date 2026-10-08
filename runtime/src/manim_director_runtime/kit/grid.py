"""VectorGrid: a patch of the plane, its basis vectors, and the linear maps that move them."""

from __future__ import annotations

from collections.abc import Callable, Mapping, Sequence
from typing import Any

import numpy as np
from manim import Animation, Arrow, MathTex, Mobject, NumberPlane, Polygon, Rectangle, VGroup
from numpy.typing import ArrayLike

from ..errors import CompositionError
from ..texscan import paint
from . import context
from .labels import beside, halo
from .overlay import derived, overlay, pacing
from .plot import LABEL_SCALE

ARROW_WIDTH = 5.0
_COLOR_KEYS = ("grid", "i", "j")
_SQUARE = ((0, 0), (1, 0), (1, 1), (0, 1))


class VectorGrid(VGroup):
    """A square patch of grid lines from -extent to extent with î and ĵ.

    `apply(M)` moves the grid and everything drawn on it (`vector`, `unit_square`) by M; a
    faint ghost keeps the original grid in view. `fits` lists the matrices the grid will show
    (cumulative states), and room for all of them is reserved at placement, so the moving grid
    never runs into its neighbors.
    """

    def __init__(
        self,
        extent: int = 3,
        *,
        unit: float = 0.8,
        fits: Sequence[ArrayLike] = (),
        ghost: bool = True,
        basis: bool = True,
        colors: Mapping[str, str] | None = None,
    ) -> None:
        super().__init__()
        if not (isinstance(extent, int) and extent >= 1) or unit <= 0:
            raise CompositionError("VectorGrid needs a whole extent >= 1 and unit > 0.")
        unknown = sorted(set(colors or {}) - set(_COLOR_KEYS))
        if unknown:
            raise CompositionError(f"Unknown VectorGrid colors {unknown}; use grid, i and j.")
        palette = {"grid": "muted", "i": "primary", "j": "secondary", **(colors or {})}
        self.hues = {key: context.color(token) for key, token in palette.items()}
        self.extent, self.unit = extent, float(unit)
        self._matrix = np.eye(2)  # where the grid is going (or rests)
        self._start = np.eye(2)  # where the current move began
        self._t = 1.0
        self._tips: list[np.ndarray] = []  # of the arrows so far, for labels to avoid
        corners = np.array([[x, y] for x in (-extent, extent) for y in (-extent, extent)], float)
        reach = np.vstack([corners, *(corners @ _matrix(m).T for m in fits)]) * self.unit
        self._reach = (reach.min(axis=0), reach.max(axis=0))
        lo, hi = self._reach[0] - 0.35, self._reach[1] + 0.35  # labels at the rim
        self.room = Rectangle(width=hi[0] - lo[0], height=hi[1] - lo[1], stroke_opacity=0)
        self.room.move_to([*(lo + hi) / 2, 0]).set_fill(opacity=0)
        self.ghost = self._plane(self.hues["grid"], self.hues["grid"], 1.0, 0.3 if ghost else 0.0)
        self.plane = self._plane(self.hues["grid"], context.color("foreground"), 2.0, 0.75)
        self.add(self.room, self.ghost, self.plane)
        if basis:
            self.i_hat = self._arrow((1, 0), r"\hat{\imath}", self.hues["i"])
            self.j_hat = self._arrow((0, 1), r"\hat{\jmath}", self.hues["j"])
            self.add(self.i_hat, self.j_hat)
        pacing(self, "grid", chunks=2, read=1.5)

    # State ------------------------------------------------------------------------------

    def current(self) -> np.ndarray:
        """The matrix the grid shows right now, mid-animation included."""

        return (1 - self._t) * self._start + self._t * self._matrix

    def det(self) -> float:
        """The area scale of the current matrix: a live `Readout` source (`grid.det`)."""

        return float(np.linalg.det(self.current()))

    def origin(self) -> np.ndarray:
        return self.ghost.c2p(0, 0)

    def to_point(self, coords: ArrayLike) -> np.ndarray:
        """Scene coordinates of grid coordinates `coords` in the untransformed grid."""

        x, y = np.asarray(coords, dtype=float)
        return self.ghost.c2p(x, y)

    # Overlays ---------------------------------------------------------------------------

    def vector(
        self, coords: ArrayLike, *, label: str | None = None, color: str = "foreground"
    ) -> Mobject:
        """An arrow from the origin to `coords`, carried along by every `apply`."""

        return overlay(self._arrow(coords, label, context.color(color)), self, "vector")

    def unit_square(self, *, color: str = "foreground", opacity: float = 0.15) -> Mobject:
        """The square on î and ĵ; its area is always |det|."""

        hue = context.color(color)

        def build() -> Mobject:
            corners = [self.to_point(self.current() @ np.array(c, float)) for c in _SQUARE]
            return Polygon(
                *corners, stroke_color=hue, stroke_width=2, fill_color=hue, fill_opacity=opacity
            )

        return overlay(derived(build).set_z_index(-1), self, "unit square")

    # Matrices ---------------------------------------------------------------------------

    def matrix(self, M: ArrayLike | None = None) -> MathTex:
        """The matrix as TeX, its columns colored like î and ĵ (where they land). Default:
        the grid's current matrix."""

        m = self.current() if M is None else _matrix(M)
        cells = [[_number(m[r, c]) for c in range(2)] for r in range(2)]
        hues = (self.hues["i"], self.hues["j"])
        rows = [
            " & ".join(paint(cell, [((0, len(cell)), hues[c])]) for c, cell in enumerate(row))
            for row in cells
        ]
        return context.math(r"\begin{bmatrix} " + r" \\ ".join(rows) + r" \end{bmatrix}")

    def apply(self, M: ArrayLike, **kwargs: Any) -> Animation:
        """Move the grid by M (after what it already shows), about its origin."""

        target = _matrix(M) @ self._matrix
        self._require_room(target)
        return _Move(self, lambda: _matrix(M) @ self._matrix, **kwargs)

    def reset(self, **kwargs: Any) -> Animation:
        """Move the grid back to where it started."""

        return _Move(self, lambda: np.eye(2), **kwargs)

    # Internals --------------------------------------------------------------------------

    def _plane(self, hue: str, axes: str, width: float, opacity: float) -> NumberPlane:
        span = [-self.extent, self.extent, 1]
        size = 2 * self.extent * self.unit
        axis = {"stroke_color": axes, "stroke_width": width + 0.5, "stroke_opacity": opacity}
        lines = {"stroke_color": hue, "stroke_width": width, "stroke_opacity": opacity}
        return NumberPlane(
            x_range=span,
            y_range=span,
            x_length=size,
            y_length=size,
            faded_line_ratio=1,
            background_line_style=lines,
            axis_config=axis,
        )

    def _arrow(self, coords: ArrayLike, label: str | None, hue: str) -> Mobject:
        base = np.asarray(coords, dtype=float)
        if base.shape != (2,):
            raise CompositionError(f"A grid vector has two coordinates; got {coords!r}.")
        tag = None if label is None else halo(context.math(label, color=hue).scale(LABEL_SCALE))
        side = self._label_side(base)
        self._tips.append(base)

        def build() -> Mobject:
            k = self._scale()
            tail, tip = self.origin(), self.to_point(self.current() @ base)
            if np.linalg.norm(tip - tail) < 1e-3:  # a collapsing map: keep a sliver
                tip = tail + np.array([1e-3, 0, 0])
            arrow = Arrow(
                tail,
                tip,
                buff=0,
                color=hue,
                stroke_width=ARROW_WIDTH,
                tip_length=0.2 * k,
                max_tip_length_to_length_ratio=0.35,
                max_stroke_width_to_length_ratio=12,
            )
            if tag is None:
                return arrow
            along = (tip - tail) / np.linalg.norm(tip - tail)
            normal = side * np.array([-along[1], along[0], 0.0])
            # Off the tip, on the label's side: clear of the shaft and of the other arrows.
            return VGroup(arrow, beside(tag.copy().scale(k), tip, along + 1.6 * normal, 0.08 * k))

        return derived(build)

    def _label_side(self, base: np.ndarray) -> float:
        """+1 to label on the left of the arrow, -1 on its right: away from the arrows drawn
        before it (î is labeled below, ĵ to its left)."""

        others = [tip for tip in self._tips if not np.allclose(tip, base)]
        if not others:
            return -1.0
        mean = np.mean(others, axis=0)
        return 1.0 if base[0] * mean[1] - base[1] * mean[0] < -1e-9 else -1.0

    def _scale(self) -> float:
        return float(np.linalg.norm(self.ghost.c2p(1, 0) - self.ghost.c2p(0, 0))) / self.unit

    def _require_room(self, target: np.ndarray) -> None:
        corners = np.array([[x, y] for x in (-1, 1) for y in (-1, 1)], float) * self.extent
        reach = corners @ target.T * self.unit
        lo, hi = self._reach
        if (reach < lo - 1e-6).any() or (reach > hi + 1e-6).any():
            raise CompositionError(
                f"This matrix moves the grid to {target.round(3).tolist()}, outside the room "
                "reserved by fits=; add that matrix to fits.",
                matrix=target.tolist(),
            )


class _Move(Animation):
    """Moves the grid from the matrix it shows to `target()` (read when the move starts),
    through the straight-line blend of the two; every derived overlay follows. Points come
    from the ghost (the untransformed grid), so a collapsing map can be undone."""

    def __init__(self, grid: VectorGrid, target: Callable[[], np.ndarray], **kwargs: Any) -> None:
        super().__init__(grid.plane, **kwargs)
        self.grid, self._target = grid, target

    def begin(self) -> None:
        grid = self.grid
        grid._start, grid._matrix, grid._t = grid.current(), self._target(), 0.0
        super().begin()

    def interpolate_mobject(self, alpha: float) -> None:
        grid = self.grid
        grid._t = self.rate_func(alpha)
        origin, lift = grid.origin(), np.eye(3)
        lift[:2, :2] = grid.current()
        leaves = grid.plane.family_members_with_points()
        for leaf, reference in zip(leaves, grid.ghost.family_members_with_points(), strict=True):
            leaf.set_points(origin + (reference.points - origin) @ lift.T)

    def finish(self) -> None:
        super().finish()
        self.grid._t = 1.0


def _matrix(M: ArrayLike) -> np.ndarray:
    m = np.asarray(M, dtype=float)
    if m.shape != (2, 2) or not np.isfinite(m).all():
        raise CompositionError(f"A grid map is a finite 2×2 matrix; got {M!r}.")
    return m


def _number(x: float) -> str:
    rounded = round(float(x), 2)
    if rounded == int(rounded):
        return str(int(rounded))
    return f"{rounded:g}"
