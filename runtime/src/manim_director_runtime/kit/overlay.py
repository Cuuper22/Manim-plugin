"""How components draw on themselves. DirectedScene reads three protocols from here:

- An *overlay* (`director_parent`) is drawn in its component's coordinates. The scene adopts it
  when it enters: it follows the component, stays lit when the component is focused, leaves
  with it, and is beat-local unless `director_persist` (R3).
- A *derived* mobject recomputes its points from the component every frame. The updater keeps
  no state and leaves copies alone (Manim also updates the copies inside animations), so it
  stays exact however `place()` scales, moves or glides the component (R4).
- A *reserved* leaf (`director_hidden`) is laid out and counted in bounds, but invisible until
  `show()` reveals it (R5).

Every component and overlay also carries pacing metadata: `director_kind`, `director_chunks`
(new things a viewer must identify) and `director_read` (seconds to take it in).
"""

from __future__ import annotations

from collections.abc import Callable, Iterable
from typing import TypeVar

import numpy as np
from manim import Animation, AnimationGroup, Mobject, ValueTracker, VMobject

from ..errors import CompositionError

M = TypeVar("M", bound=Mobject)
Live = float | ValueTracker | Callable[[], float]
"""A number, a ValueTracker, or a function read every frame (e.g. `grid.det`)."""


def now(live: Live) -> float:
    """The value of a live number at this moment."""

    if isinstance(live, ValueTracker):
        return float(live.get_value())
    number = float(live()) if callable(live) else float(live)
    if not np.isfinite(number):
        raise CompositionError(f"A live value is {number}; it must be a finite number.")
    return number


def pacing(mobject: M, kind: str, *, chunks: int = 1, read: float | None = 0.5) -> M:
    """Declare what a viewer must take in when `mobject` enters: `chunks` new things, read in
    `read` seconds (None: measured from its words and glyphs)."""

    mobject.director_kind = kind
    mobject.director_chunks = chunks
    mobject.director_read = read
    return mobject


def overlay(mobject: M, parent: Mobject, kind: str, *, persist: bool = False) -> M:
    mobject.director_parent = parent
    mobject.director_persist = persist
    return pacing(mobject, kind)


def is_overlay(mobject: Mobject) -> bool:
    return getattr(mobject, "director_parent", None) is not None


def derived(build: Callable[[], VMobject]) -> VMobject:
    """`build()` now and again every frame, like `always_redraw`, except that the mobject keeps
    its identity and style (focus dims, recolors) and only its points follow."""

    mobject = build()

    def redraw(m: Mobject) -> None:
        if m is mobject:
            retrace(m, build())

    redraw.director = True  # type: ignore[attr-defined]
    mobject.add_updater(redraw)
    return mobject


def retrace(target: Mobject, source: Mobject) -> None:
    """Give `target` the points of `source`, keeping `target`'s style where the shapes match
    leaf for leaf (a dashed line can gain dashes as it grows: then it becomes `source`)."""

    old, new = target.family_members_with_points(), source.family_members_with_points()
    if len(old) != len(new):
        target.become(source)
        return
    for leaf, shape in zip(old, new, strict=True):
        leaf.set_points(shape.points)


def reserve(mobject: M) -> M:
    """Make every leaf of `mobject` invisible but keep it in the layout until `show()`."""

    for leaf in _leaves(mobject):
        if not getattr(leaf, "director_hidden", False):
            leaf.director_opacity = (leaf.get_fill_opacity(), leaf.get_stroke_opacity())
            leaf.director_hidden = True
            leaf.set_fill(opacity=0, family=False)
            leaf.set_stroke(opacity=0, family=False)
    return mobject


def reserved(mobject: Mobject) -> list[VMobject]:
    return [leaf for leaf in _leaves(mobject) if getattr(leaf, "director_hidden", False)]


def reveal(leaves: Iterable[VMobject]) -> None:
    """Give reserved leaves back their opacities (without animating)."""

    for leaf in leaves:
        fill, stroke = leaf.director_opacity
        leaf.set_fill(opacity=fill, family=False)
        leaf.set_stroke(opacity=stroke, family=False)
        leaf.director_hidden = False


def hide(*mobjects: Mobject, run_time: float = 0.8) -> Animation:
    """Fade parts or overlays back to reserved, for a replay: `show()` brings them in again."""

    leaves = [
        leaf for m in mobjects for leaf in _leaves(m) if not getattr(leaf, "director_hidden", 0)
    ]
    if not leaves:
        raise CompositionError("Nothing to hide: every part is already reserved.")
    for leaf in leaves:
        leaf.director_opacity = (leaf.get_fill_opacity(), leaf.get_stroke_opacity())
        leaf.director_hidden = True
    return AnimationGroup(*(leaf.animate.set_opacity(0) for leaf in leaves), run_time=run_time)


def _leaves(mobject: Mobject) -> list[VMobject]:
    return [m for m in mobject.family_members_with_points() if isinstance(m, VMobject)]
