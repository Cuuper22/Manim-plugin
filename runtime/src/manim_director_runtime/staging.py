"""What DirectedScene tracks between animations, and placement planning.

Planning only reads sizes, so a placement is fully validated before anything moves.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field
from typing import Any

from .errors import CompositionError
from .layout import Rect, Region, fit_scale

Mobject = Any  # duck-typed: anything with width, height and get_center()


@dataclass(frozen=True, slots=True)
class Plan:
    scale: float
    centers: list[tuple[float, float, float]]
    bounds: Rect


@dataclass(eq=False)
class Stage:
    """Objects DirectedScene placed, and changes waiting for the next animation."""

    placed: dict[int, tuple[Mobject, Region]] = field(default_factory=dict)
    entering: list[Mobject] = field(default_factory=list)
    leaving: list[Mobject] = field(default_factory=list)
    morphs: list[tuple[Mobject, Mobject]] = field(default_factory=list)
    glides: dict[int, tuple[Mobject, Mobject]] = field(default_factory=dict)  # id -> (now, before)
    dimmed: dict[int, tuple[Mobject, dict[int, tuple[float, float]]]] = field(default_factory=dict)
    attached: dict[int, Mobject] = field(default_factory=dict)  # child id -> parent

    def place(self, mobject: Mobject, region: Region) -> None:
        self.placed[id(mobject)] = (mobject, region)

    def region_of(self, mobject: Mobject) -> Region | None:
        entry = self.placed.get(id(mobject))
        return entry[1] if entry else None

    def occupants(self, region: Region) -> list[Mobject]:
        return [mobject for mobject, placed_in in self.placed.values() if placed_in is region]

    def pending(self) -> bool:
        return bool(self.entering or self.leaving or self.morphs or self.glides)

    def prune(self, alive: set[int]) -> None:
        """Forget placements that left the scene by any means (plain Manim included)."""

        waiting = {id(m) for m in self.entering} | {id(new) for _, new in self.morphs}
        self.placed = {k: v for k, v in self.placed.items() if k in alive or k in waiting}
        self.dimmed = {k: v for k, v in self.dimmed.items() if k in alive}


def bounds(mobject: Mobject) -> Rect:
    x, y = mobject.get_center()[:2]
    return Rect.around((float(x), float(y)), float(mobject.width), float(mobject.height))


def plan(
    mobjects: Sequence[Mobject],
    region: Region,
    area: Rect,
    *,
    anchor: tuple[float, float],
    axis: tuple[int, int],
    buff: float,
    min_scale: float,
) -> Plan:
    """Scale and centers that arrange `mobjects` along `axis` and fit them into `area`."""

    sizes = [(float(m.width), float(m.height)) for m in mobjects]
    along = 0 if axis[0] else 1
    extent = sum(size[along] for size in sizes) + buff * (len(sizes) - 1)
    across = max(size[1 - along] for size in sizes)
    width, height = (extent, across) if along == 0 else (across, extent)
    scale = fit_scale(width, height, area)
    if scale < min_scale:
        what = describe(mobjects[0]) if len(mobjects) == 1 else f"{len(mobjects)} objects"
        raise CompositionError(
            f"{what} needs {scale:.2f}x to fit the {region} region "
            f"({area.width:.1f} x {area.height:.1f}), below the readable minimum "
            f"{min_scale:.2f}x. Shorten or split it, or use a larger region.",
            region=region.value,
            scale=round(scale, 3),
            min_scale=min_scale,
        )
    width, height = width * scale, height * scale
    cx = area.center[0] + anchor[0] * (area.width - width) / 2
    cy = area.center[1] + anchor[1] * (area.height - height) / 2
    centers = []
    sign = axis[along]
    cursor = (cx, cy)[along] - sign * (extent * scale) / 2
    for size in sizes:
        middle = cursor + sign * size[along] * scale / 2
        centers.append((middle, cy, 0.0) if along == 0 else (cx, middle, 0.0))
        cursor += sign * (size[along] + buff) * scale
    return Plan(scale, centers, Rect.around((cx, cy), width, height))


def describe(mobject: Mobject) -> str:
    text = repr(mobject)
    return text if len(text) <= 60 else text[:57] + "..."
