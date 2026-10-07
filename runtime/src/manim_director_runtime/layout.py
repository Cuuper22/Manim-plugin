"""Frame regions in Manim scene units. Pure geometry: no Manim import."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from .tasks import SafeArea


class Region(StrEnum):
    SAFE = "safe"  # the whole safe area: one hero visual
    HEADER = "header"  # title lane; persists across beats until a chapter
    CONTENT = "content"  # between the header and caption lanes
    LEFT = "left"
    RIGHT = "right"
    TOP = "top"
    BOTTOM = "bottom"
    CAPTION = "caption"  # caption lane; persists across beats until a chapter


LANES = frozenset({Region.HEADER, Region.CAPTION})

# Lane heights and the gutter between regions, as shares of the safe-area height.
_HEADER_SHARE = 0.12
_CAPTION_SHARE = 0.12
_GUTTER_SHARE = 0.035


@dataclass(frozen=True, slots=True)
class Rect:
    left: float
    bottom: float
    right: float
    top: float

    @property
    def width(self) -> float:
        return self.right - self.left

    @property
    def height(self) -> float:
        return self.top - self.bottom

    @property
    def center(self) -> tuple[float, float, float]:
        return ((self.left + self.right) / 2, (self.bottom + self.top) / 2, 0.0)

    def overlaps(self, other: Rect, gap: float = 0.0) -> bool:
        return (
            self.left < other.right + gap
            and other.left < self.right + gap
            and self.bottom < other.top + gap
            and other.bottom < self.top + gap
        )

    def contains(self, other: Rect, tolerance: float = 1e-6) -> bool:
        return (
            other.left >= self.left - tolerance
            and other.right <= self.right + tolerance
            and other.bottom >= self.bottom - tolerance
            and other.top <= self.top + tolerance
        )

    @classmethod
    def around(cls, center: tuple[float, float], width: float, height: float) -> Rect:
        x, y = center
        return cls(x - width / 2, y - height / 2, x + width / 2, y + height / 2)


def frame_regions(frame_width: float, frame_height: float, safe: SafeArea) -> dict[Region, Rect]:
    half_w, half_h = frame_width / 2, frame_height / 2
    outer = Rect(
        -half_w + safe.left * frame_width,
        -half_h + safe.bottom * frame_height,
        half_w - safe.right * frame_width,
        half_h - safe.top * frame_height,
    )
    unit = outer.height
    gutter = _GUTTER_SHARE * unit
    header = Rect(outer.left, outer.top - _HEADER_SHARE * unit, outer.right, outer.top)
    caption = Rect(outer.left, outer.bottom, outer.right, outer.bottom + _CAPTION_SHARE * unit)
    content = Rect(outer.left, caption.top + gutter, outer.right, header.bottom - gutter)
    mid_x, mid_y = content.center[:2]
    return {
        Region.SAFE: outer,
        Region.HEADER: header,
        Region.CONTENT: content,
        Region.LEFT: Rect(content.left, content.bottom, mid_x - gutter, content.top),
        Region.RIGHT: Rect(mid_x + gutter, content.bottom, content.right, content.top),
        Region.TOP: Rect(content.left, mid_y + gutter / 2, content.right, content.top),
        Region.BOTTOM: Rect(content.left, content.bottom, content.right, mid_y - gutter / 2),
        Region.CAPTION: caption,
    }


def fit_scale(width: float, height: float, region: Rect) -> float:
    """The largest scale <= 1 at which a box fits the region (zero sizes never constrain)."""

    scale = 1.0
    if width > 0:
        scale = min(scale, region.width / width)
    if height > 0:
        scale = min(scale, region.height / height)
    return scale
