from __future__ import annotations

import pytest

from manim_director_runtime.errors import CompositionError
from manim_director_runtime.layout import Rect, Region, fit_scale, frame_regions
from manim_director_runtime.staging import plan
from manim_director_runtime.tasks import SAFE_AREA_DEFAULT, SafeArea

WIDE = frame_regions(14.222, 8.0, SAFE_AREA_DEFAULT)


class Box:
    def __init__(self, width: float, height: float) -> None:
        self.width, self.height = width, height

    def __repr__(self) -> str:
        return f"Box({self.width}, {self.height})"


@pytest.mark.parametrize("size", [(14.222, 8.0), (4.5, 8.0), (8.0, 8.0)])
def test_regions_nest_inside_the_safe_area_without_overlapping(size) -> None:
    regions = frame_regions(*size, SAFE_AREA_DEFAULT)
    safe = regions[Region.SAFE]
    assert all(safe.contains(rect) for rect in regions.values())
    stacked = [Region.HEADER, Region.CONTENT, Region.CAPTION]
    for upper, lower in zip(stacked, stacked[1:], strict=False):
        assert regions[upper].bottom > regions[lower].top
    assert not regions[Region.LEFT].overlaps(regions[Region.RIGHT])
    assert not regions[Region.TOP].overlaps(regions[Region.BOTTOM])
    for half in (Region.LEFT, Region.RIGHT, Region.TOP, Region.BOTTOM):
        assert regions[Region.CONTENT].contains(regions[half])


def test_safe_area_fractions_set_the_margins() -> None:
    safe = frame_regions(10.0, 8.0, SafeArea(top=0.25, right=0.1, bottom=0.0, left=0.0))[
        Region.SAFE
    ]
    assert (safe.left, safe.right, safe.bottom, safe.top) == pytest.approx((-5, 4, -4, 2))


def test_rect_geometry() -> None:
    a = Rect(0, 0, 2, 1)
    assert (a.width, a.height, a.center) == (2, 1, (1.0, 0.5, 0.0))
    assert a.overlaps(Rect(1.9, 0.9, 3, 2)) and not a.overlaps(Rect(2, 0, 3, 1))
    assert Rect.around((1, 0.5), 2, 1) == a


def test_fit_scale_never_enlarges_and_ignores_zero_sizes() -> None:
    area = Rect(0, 0, 4, 2)
    assert fit_scale(1, 1, area) == 1.0
    assert fit_scale(8, 1, area) == 0.5
    assert fit_scale(8, 0, area) == 0.5


def test_plan_arranges_along_the_axis_and_centers_the_block() -> None:
    area = WIDE[Region.CONTENT]
    layout = plan(
        [Box(2, 1), Box(4, 1)],
        Region.CONTENT,
        area,
        anchor=(0, 0),
        axis=(0, -1),
        buff=0.5,
        min_scale=0.5,
    )
    assert layout.scale == 1.0
    (x1, y1, _), (x2, y2, _) = layout.centers
    assert x1 == x2 == pytest.approx(area.center[0])
    assert y1 - y2 == pytest.approx(1.5)
    assert layout.bounds.center[:2] == pytest.approx(area.center[:2])
    assert (layout.bounds.width, layout.bounds.height) == pytest.approx((4, 2.5))


def test_plan_anchors_to_region_edges() -> None:
    area = WIDE[Region.CONTENT]
    layout = plan(
        [Box(1, 1), Box(1, 1)],
        Region.CONTENT,
        area,
        anchor=(1, 1),
        axis=(1, 0),
        buff=0.0,
        min_scale=0.5,
    )
    assert layout.bounds.right == pytest.approx(area.right)
    assert layout.bounds.top == pytest.approx(area.top)
    assert layout.centers[0][0] < layout.centers[1][0]


def test_plan_scales_down_to_fit_and_refuses_unreadable_sizes() -> None:
    area = WIDE[Region.CAPTION]
    layout = plan(
        [Box(area.width * 1.25, 0.1)],
        Region.CAPTION,
        area,
        anchor=(0, 0),
        axis=(0, -1),
        buff=0,
        min_scale=0.5,
    )
    assert layout.scale == pytest.approx(0.8)
    with pytest.raises(CompositionError, match=r"needs 0\.25x to fit the caption region") as raised:
        plan(
            [Box(area.width * 4, 0.1)],
            Region.CAPTION,
            area,
            anchor=(0, 0),
            axis=(0, -1),
            buff=0,
            min_scale=0.5,
        )
    assert raised.value.code == "composition"
    assert raised.value.data["region"] == "caption"
