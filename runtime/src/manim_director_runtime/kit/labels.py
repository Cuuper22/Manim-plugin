"""Labels: words and math on one baseline (R9), and the free-space search that places them."""

from __future__ import annotations

from collections.abc import Iterable, Sequence
from functools import cache
from statistics import median
from typing import TypeVar

import numpy as np
from manim import MathTex, Mobject, RoundedRectangle, Text, VGroup, VMobject

from ..errors import CompositionError, parse_choice
from ..layout import Rect
from ..themes import TEXT_STYLES, Role
from . import context

M = TypeVar("M", bound=Mobject)
_DESCENDERS = frozenset("gjpqyQ,;()[]{}/|@$_")
HALO_WIDTH = 5.0


def label(text: str, role: Role | str = Role.LABEL, color: str | None = None) -> VGroup:
    """`text` with each `$...$` span as math (in its symbol colors) and the rest in the theme
    font, on one baseline and at one x-height. `color` (a theme token or #RRGGBB) replaces the
    role's color for the words and is the default color of the math."""

    pieces = text.split("$")
    if len(pieces) % 2 == 0:
        raise CompositionError(f"Unbalanced $ in the label {text!r}: math goes between two $.")
    role = parse_choice(Role, role)
    hue = context.color(TEXT_STYLES[role].color if color is None else color)
    space = _space(context.theme().font, role)
    row, cursor = VGroup(), 0.0
    for i, piece in enumerate(pieces):
        is_math = i % 2 == 1
        body = piece if is_math else piece.strip()
        if not body:
            continue
        if row.submobjects and _spaced(pieces, i):
            cursor += space
        mobject, baseline = _math(body, role, hue) if is_math else _words(body, role, hue)
        mobject.shift((cursor - mobject.get_left()[0], -baseline, 0))
        cursor = mobject.get_right()[0]
        row.add(mobject)
    if not row.submobjects:
        raise CompositionError("A label needs some text.")
    row.authored_text = text
    return row


def halo(mobject: Mobject) -> Mobject:
    """Outline `mobject` in the background color so it reads over grid lines and curves."""

    for leaf in mobject.family_members_with_points():
        if isinstance(leaf, VMobject):
            leaf.set_stroke(context.color("background"), width=HALO_WIDTH, background=True)
    return mobject


def backdrop(glyphs: Mobject, hue: str, opacity: float = 0.16) -> RoundedRectangle:
    """A soft box in `hue` to put behind glyphs, like a highlighter pen: the glyphs keep their
    own colors."""

    # Tight sideways so neighbouring operators keep their space; taller like a marker.
    pad_x, pad_y = 0.05, 0.05 + 0.12 * glyphs.height
    return RoundedRectangle(
        width=glyphs.width + 2 * pad_x,
        height=glyphs.height + 2 * pad_y,
        corner_radius=0.08,
        stroke_width=0,
        fill_color=hue,
        fill_opacity=opacity,
    ).move_to(glyphs)


def beside(mobject: M, anchor: Sequence[float], direction: Sequence[float], gap: float) -> M:
    """Move `mobject` so that its near edge sits `gap` beyond `anchor` along `direction`."""

    unit = _unit(direction)
    reach = gap + abs(unit[0]) * mobject.width / 2 + abs(unit[1]) * mobject.height / 2
    x, y = float(anchor[0]) + reach * unit[0], float(anchor[1]) + reach * unit[1]
    return mobject.move_to((x, y, 0.0))


def first_free(
    mobject: Mobject,
    candidates: Iterable[tuple[Sequence[float], Sequence[float]]],
    *,
    gap: float,
    avoid: Sequence[Rect] = (),
    within: Rect | None = None,
) -> tuple[np.ndarray, np.ndarray]:
    """The first `(anchor, direction)` where `mobject`, set beside the anchor, overlaps nothing
    in `avoid` and stays `within`; the first candidate if none is free. Leaves `mobject` there.
    """

    tried = []
    for anchor, direction in candidates:
        spot = (np.asarray(anchor, dtype=float), _unit(direction))
        tried.append(spot)
        place = box(beside(mobject, *spot, gap))
        if (within is None or within.contains(place)) and not any(place.overlaps(r) for r in avoid):
            return spot
    if not tried:
        raise CompositionError("No candidate spots for a label.")
    beside(mobject, *tried[0], gap)
    return tried[0]


def fan(direction: Sequence[float], *, step_degrees: float = 30, count: int = 12) -> list:
    """`direction`, then directions turned away from it by ±step, ±2·step, ..."""

    angle = np.arctan2(direction[1], direction[0])
    turns = [0.0] + [sign * k * step_degrees for k in range(1, count // 2 + 1) for sign in (1, -1)]
    return [(np.cos(angle + np.radians(turn)), np.sin(angle + np.radians(turn))) for turn in turns]


def inside(mobject: Mobject, area: Rect) -> Mobject:
    """Shift `mobject` the least distance that puts it inside `area`."""

    dx = min(0.0, area.right - mobject.get_right()[0]) + max(0.0, area.left - mobject.get_left()[0])
    dy = min(0.0, area.top - mobject.get_top()[1]) + max(0.0, area.bottom - mobject.get_bottom()[1])
    return mobject.shift((dx, dy, 0))


def trace(mobjects: Iterable[Mobject], pad: float = 0.05, spacing: float = 0.12) -> list[Rect]:
    """Small boxes every `spacing` along the drawn paths of `mobjects`, for labels to keep
    clear of lines and curves (a bounding box would wall off a whole diagonal)."""

    boxes = []
    for mobject in mobjects:
        for leaf in mobject.family_members_with_points():
            points = leaf.points
            ends = [*(points[::4] if len(points) >= 4 else points), points[-1]]
            for p, q in zip(ends, ends[1:], strict=False):
                steps = max(1, int(np.linalg.norm(q - p) / spacing))
                for i in range(steps + 1):
                    x, y = (p + (q - p) * i / steps)[:2]
                    boxes.append(Rect.around((float(x), float(y)), pad, pad))
    return boxes


def box(mobject: Mobject, pad: float = 0.0) -> Rect:
    x, y = mobject.get_center()[:2]
    return Rect.around((float(x), float(y)), mobject.width + 2 * pad, mobject.height + 2 * pad)


def _unit(direction: Sequence[float]) -> np.ndarray:
    unit = np.array([direction[0], direction[1]], dtype=float)
    return unit / np.linalg.norm(unit)


def _spaced(pieces: list[str], i: int) -> bool:
    """Whether the source has whitespace between piece `i` and the piece before it."""

    return pieces[i - 1][-1:].isspace() if i % 2 == 1 else pieces[i][:1].isspace()


def _words(body: str, role: Role, hue: str) -> tuple[Text, float]:
    words = context.text(body, role, color=hue)
    pairs = zip(words.submobjects, words.text, strict=False)
    bottoms = [g.get_bottom()[1] for g, ch in pairs if ch not in _DESCENDERS]
    return words, float(median(bottoms or [words.get_bottom()[1]]))


def _math(body: str, role: Role, hue: str) -> tuple[Mobject, float]:
    # A roman x typeset alongside gives the baseline; it is then dropped.
    probe = context.math(
        r"\mathrm{x}", body, font_size=_math_size(context.theme().font, role), color=hue
    )
    reference, formula = probe.submobjects
    return formula, float(reference.get_bottom()[1])


@cache
def _math_size(font: str, role: Role) -> float:
    """The math font size whose x-height matches the role's text."""

    size = TEXT_STYLES[role].font_size
    words = Text("x", font=font, font_size=size, warn_missing_font=False)
    return size * words.height / MathTex(r"\mathrm{x}", font_size=size).height


@cache
def _space(font: str, role: Role) -> float:
    size = TEXT_STYLES[role].font_size

    def width(text: str) -> float:
        return Text(text, font=font, font_size=size, warn_missing_font=False).width

    return width("x x") - width("xx")
