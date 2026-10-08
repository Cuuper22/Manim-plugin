"""DotArray: countable dots for natural frequencies and counting arguments."""

from __future__ import annotations

from collections.abc import Callable

from manim import Animation, Circle, Dot, FadeToColor, LaggedStart, VGroup, VMobject

from ..errors import CompositionError
from . import context
from .overlay import derived, hide, overlay, pacing, reserve

Where = Callable[[int, int], bool]


class DotArray(VGroup):
    """`rows` × `cols` dots, row by row from the top left: `at(r, c)`.

    Dots that `shown` leaves out are reserved: laid out, invisible, and revealed with
    `self.show(dots.select(...))`, so the picture never reflows; with `ghost` their places
    are faint rings meanwhile, so the shape to be filled shows from the start. A selection is
    the dots themselves: show it, `place()` it elsewhere (the dots glide out of the array),
    annotate it, `paint()` it, or `ring()` it.
    """

    def __init__(
        self,
        rows: int,
        cols: int | None = None,
        *,
        shown: bool | Where = True,
        radius: float = 0.1,
        gap: float = 0.2,
        color: str = "muted",
        ghost: bool = True,
    ) -> None:
        super().__init__()
        cols = rows if cols is None else cols
        if not all(isinstance(n, int) and n >= 1 for n in (rows, cols)):
            raise CompositionError(f"DotArray({rows!r}, {cols!r}) needs whole numbers >= 1.")
        if radius <= 0 or gap < 0:
            raise CompositionError("DotArray needs radius > 0 and gap >= 0.")
        self.rows, self.cols = rows, cols
        hue = context.color(color)
        pitch = 2 * radius + gap
        self._grid: dict[tuple[int, int], Dot] = {}
        for r in range(rows):
            for c in range(cols):
                dot = Dot((c * pitch, -r * pitch, 0), radius=radius, color=hue)
                self._grid[(r, c)] = dot
                self.add(dot)
        self.center()
        visible = shown if callable(shown) else (lambda r, c: bool(shown))
        hidden = [dot for (r, c), dot in self._grid.items() if not visible(r, c)]
        self.ghosts = VGroup()
        if ghost and hidden:
            muted = context.color("muted")
            self.ghosts.add(
                *(
                    Circle(radius=radius * 0.85, stroke_color=muted, stroke_width=1.5)
                    .set_stroke(opacity=0.5)
                    .move_to(dot)
                    for dot in hidden
                )
            )
            self.ghosts.director_ground = True  # notes may sit over the rings
            self.add_to_back(self.ghosts)
        for dot in hidden:
            reserve(dot)
        pacing(self, "dots", chunks=1, read=0.5)

    def at(self, r: int, c: int) -> Dot:
        if (r, c) not in self._grid:
            raise CompositionError(
                f"at({r}, {c}) is outside the {self.rows} × {self.cols} array (0-based)."
            )
        return self._grid[(r, c)]

    def select(self, where: Where | int, *, color: str | None = None) -> VGroup:
        """The dots where `where(r, c)` holds, or the first `where` dots row by row. `color`
        recolors them at once (before they are shown); `paint` animates a recolor."""

        dots = self._where(where)
        if color is not None:
            hue = context.color(color)
            for dot in dots:
                dot.set_color(hue)
        return pacing(VGroup(*dots), "dots", chunks=1, read=0.5)

    def count(self, where: Where) -> int:
        return len(self._where(where))

    def paint(self, where: Where | int, color: str) -> Animation:
        """Recolor the selected dots one after another; reserved ones take the color at once,
        ready for when they are shown."""

        hue = context.color(color)
        dots = self._where(where)
        visible = [d for d in dots if not getattr(d, "director_hidden", False)]
        for dot in dots:
            if dot not in visible:
                dot.set_color(hue)
        if not visible:
            raise CompositionError("paint() selected no visible dots; show() them first.")
        return LaggedStart(*(FadeToColor(dot, hue) for dot in visible), lag_ratio=0.1)

    def ring(self, where: Where | int, color: str = "foreground") -> VMobject:
        """Rings around the selected dots, to `show()`: a second fact about each one (tested
        positive) that leaves its color for the first (sick). The rings follow the dots."""

        dots, hue = self._where(where), context.color(color)

        def build() -> VMobject:
            return VGroup(
                *(
                    Circle(radius=0.85 * dot.width, stroke_color=hue, stroke_width=3).move_to(dot)
                    for dot in dots
                )
            )

        return overlay(derived(build), self, "rings")

    def hide(self, where: Where | None = None) -> Animation:
        """Fade the selected (default: all) visible dots back to reserved, for a replay."""

        return hide(*self._where(where if where is not None else (lambda r, c: True)))

    def _where(self, where: Where | int) -> list[Dot]:
        if isinstance(where, int):
            total = self.rows * self.cols
            if not 0 <= where <= total:
                raise CompositionError(f"select({where}) needs 0 to {total} dots.")
            return list(self._grid.values())[:where]
        if not callable(where):
            raise CompositionError(
                "Select dots with a count or a predicate, e.g. lambda r, c: r == 0."
            )
        dots = [dot for (r, c), dot in self._grid.items() if where(r, c)]
        if not dots:
            raise CompositionError("The predicate selected no dots.")
        return dots
