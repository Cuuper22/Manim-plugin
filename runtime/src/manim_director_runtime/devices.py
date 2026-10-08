"""Scene methods that reveal kit content, mixed into Directed.

`place()` positions top-level content; `show()` reveals what belongs to placed content:
overlays (`plot.tangent(1)`) and reserved parts (`dots.select(...)` of a `DotArray` built with
`shown=`).
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from manim import AnimationGroup, Dot, LaggedStart, Mobject

from . import motion
from .errors import CompositionError
from .kit.overlay import is_overlay, reserved, reveal
from .staging import describe

if TYPE_CHECKING:
    from manim import Animation


class Devices:
    def show(
        self: Any, *mobjects: Mobject, run_time: float | None = None, lag: float = 0.15
    ) -> None:
        """Reveal overlays and reserved parts of placed content, each with the entrance that
        suits it (dots grow, strokes are drawn, short math is written), one after the other.
        Content placed in this beat enters first."""

        if not mobjects:
            raise CompositionError("show() needs at least one mobject.")
        self._flush()
        visible = self._visible_ids()
        entrances: list[Animation] = []
        for mobject in mobjects:
            if is_overlay(mobject):
                parent = mobject.director_parent
                if not self._holds(parent):
                    raise CompositionError(
                        f"{describe(mobject)} is drawn on {describe(parent)}, which is not on "
                        "stage: place() it first."
                    )
                if self._visible(mobject):
                    raise CompositionError(f"{describe(mobject)} is already on stage.")
                self.add(mobject)  # adopted now; its entrance starts from nothing
                entrances.append(motion.entrance(mobject, lag))
                continue
            if not self._holds(mobject):
                raise CompositionError(
                    f"{describe(mobject)} is not placed: place() it first. show() reveals "
                    "overlays and reserved parts of placed content."
                )
            hidden = reserved(mobject)
            if not hidden:
                raise CompositionError(f"{describe(mobject)} is already visible.")
            whole = id(mobject) in visible and len(hidden) == len(
                mobject.family_members_with_points()
            )
            reveal(hidden)
            if whole:
                entrances.append(motion.entrance(mobject, lag))
                continue
            # A group made on the fly (a selection) is not in the scene: an entrance played on
            # it would add it, splitting the component its leaves belong to.
            parts = [motion.entrance(leaf) for leaf in hidden]
            dots = all(isinstance(leaf, Dot) for leaf in hidden)
            entrances.append(LaggedStart(*parts, lag_ratio=lag) if dots else AnimationGroup(*parts))
        animation = entrances[0] if len(entrances) == 1 else LaggedStart(*entrances, lag_ratio=lag)
        self._perform([animation], motion.SHOW_SECONDS if run_time is None else run_time)
