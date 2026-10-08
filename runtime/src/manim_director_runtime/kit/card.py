"""Misconception: a tempting wrong idea, tested, struck and repaired, on one card.

The viewer must first recognize the claim as their own guess, so it starts in neutral ink under
"Your guess". Evidence then refutes it, the struck claim stays in view, and the repair arrives
as a small, boxed change from it: the viewer updates the belief they hold instead of memorizing
a new one.
"""

from __future__ import annotations

import re
from collections.abc import Sequence

from manim import (
    DOWN,
    LEFT,
    RIGHT,
    UP,
    Animation,
    AnimationGroup,
    Create,
    FadeIn,
    LaggedStart,
    Line,
    Mobject,
    Rectangle,
    SingleStringMathTex,
    Succession,
    Transform,
    VGroup,
    VMobject,
)

from .. import motion
from ..errors import CompositionError
from ..terms import term_groups
from ..texscan import atoms
from ..themes import Role
from . import context
from .labels import backdrop, label
from .overlay import pacing, reserve, reveal

ROW_GAP = 0.32
MARK_GAP = 0.35
MARK_WIDTH = 6.0
STRUCK_OPACITY = 0.45
Item = str | Mobject


class Misconception(VGroup):
    """`tag` over the `claim`, with reserved room for a ✗, the evidence rows and a repair row
    with a ✓, so nothing reflows as the story plays: `test()`, `refute()`, `repair()`, each an
    Animation to play. Give `evidence` (a list) and `fix` up front and each row gets room of its
    own height; a number of rows gets room at the claim's height. A text with `$math$` in it
    is set in the theme's font, like a label; other text is math."""

    def __init__(
        self,
        claim: Item,
        *,
        tag: str = "Your guess",
        evidence: int | Sequence[Item] = 2,
        fix: Item | None = None,
        refuted: str | None = "Tempting",
    ) -> None:
        super().__init__()
        declared = [] if isinstance(evidence, int) else [_typeset(item) for item in evidence]
        count = evidence if isinstance(evidence, int) else len(declared)
        if not (isinstance(count, int) and count >= 1):
            raise CompositionError(
                f"misconception(evidence={evidence!r}) needs a whole number >= 1, or the rows."
            )
        self.claim = _typeset(claim)
        self.tag = label(tag).next_to(self.claim, UP, buff=0.22, aligned_edge=LEFT)
        self._refuted = None if refuted in (None, tag) else label(refuted)
        left, right = self.claim.get_left(), self.claim.get_right()
        self.strike = Line(left + 0.12 * (LEFT + DOWN), right + 0.12 * (RIGHT + UP))
        self.strike.set_stroke(context.color("highlight"), width=MARK_WIDTH - 1)
        self.fix = None if fix is None else _typeset(fix)
        known = [self.claim, *declared, *([self.fix] if self.fix is not None else [])]
        heights = [row.height for row in declared] or [self.claim.height] * count
        heights.append(self.claim.height if self.fix is None else self.fix.height)
        width = max(row.width for row in known)
        self.slots = VGroup(
            *(Rectangle(width=width, height=h, stroke_opacity=0) for h in heights)
        ).arrange(DOWN, buff=ROW_GAP)
        self.slots.next_to(self.claim, DOWN, buff=ROW_GAP, aligned_edge=LEFT)
        # The marks stand in one column, right of the widest row.
        size = 0.55 * min(self.claim.height, 0.6)
        column = left[0] + width + MARK_GAP + size / 2
        self.cross = _cross(size).move_to((column, self.claim.get_y(), 0))
        self.check = _check(size).move_to((column, self.slots[-1].get_y(), 0))
        self.rows = VGroup()
        self._height = self.claim.height
        self._declared = [
            reserve(self._fit(row, slot)) for row, slot in zip(declared, self.slots, strict=False)
        ]
        if self.fix is not None:
            reserve(self._fit(self.fix, self.slots[-1]))
        self.diff: VGroup | None = None  # boxes behind what the repair changed
        pacing(self.strike, "strike", chunks=0, read=0.0)  # read as one mark with the ✗
        pacing(self.cross, "verdict", chunks=1, read=0.5)
        pacing(self.check, "verdict", chunks=1, read=0.5)
        for part in (self.cross, self.strike, self.check):
            reserve(part)
        self.add(self.tag, self.claim, self.cross, self.strike, self.slots, self.check, self.rows)
        self.add(*self._declared, *([self.fix] if self.fix is not None else []))
        pacing(self, "card", chunks=2, read=None)  # the claim and its tag, read in full

    def test(self, *evidence: Item) -> Animation:
        """Fill the next evidence rows with checks the viewer can compute, one after another:
        the rows given here, or with none, every row declared in `misconception(evidence=[…])`
        that is not shown yet."""

        if not evidence and self._declared:
            rows = [row for row in self._declared if row not in self.rows]
            if not rows:
                raise CompositionError("Every declared evidence row is shown already.")
        else:
            free = len(self.slots) - 1 - len(self.rows)
            if not evidence or len(evidence) > free or self._declared:
                raise CompositionError(
                    f"test() fills {free} more evidence row(s) of this card; got "
                    f"{len(evidence)}. Declare the rows up front instead: "
                    "misconception(..., evidence=[...]), then call test()."
                )
            slots = self.slots[len(self.rows) : len(self.rows) + len(evidence)]
            rows = [
                reserve(self._fit(_typeset(item), slot))
                for item, slot in zip(evidence, slots, strict=True)
            ]
        self.rows.add(*rows)
        entrances = [FadeIn(row, shift=0.15 * DOWN) for row in rows]
        return _Revealing(LaggedStart(*entrances, lag_ratio=0.6), shown=rows)

    def refute(self) -> Animation:
        """Strike the claim and mark it ✗; it fades but stays, to compare with the repair. The
        tag turns into `refuted` ("Tempting"): the guess was natural, and wrong."""

        fade = self.claim.animate.set_opacity(STRUCK_OPACITY)
        animations: list[Animation] = [Create(self.strike), Create(self.cross), fade]
        if self._refuted is not None:  # the tag keeps its identity: the card stays one group
            animations.append(Transform(self.tag, self._refuted.move_to(self.tag, LEFT)))
            self._refuted = None
        return _Revealing(*animations, shown=[self.strike, self.cross])

    def repair(self, fix: Item | None = None) -> Animation:
        """A copy of the claim appears in the repair row and turns into the fix (`fix`, or the
        one given to `misconception`); what changed is boxed in `success`, and the row gets a
        ✓."""

        if self.diff is not None:
            raise CompositionError("This card is already repaired.")
        if (fix is None) == (self.fix is None):
            raise CompositionError(
                "repair() needs the fix once: here, or as misconception(..., fix=...)."
            )
        if self.fix is None:
            self.fix = reserve(self._fit(_typeset(fix), self.slots[-1]))
            self.add(self.fix)
        if self.fix.height > self.slots[-1].height + ROW_GAP / 2:
            raise _taller("repair")
        boxes = VGroup(*(backdrop(g, context.color("success"), 0.25) for g in self._changed()))
        boxes.set_z_index(self.fix.z_index - 1)
        self.diff = boxes
        self.add(reserve(pacing(boxes, "diff", chunks=0, read=0.0)))
        start = self.claim.copy().set_opacity(1).move_to(self.slots[-1], aligned_edge=LEFT)
        self.add(pacing(start, "claim", chunks=0, read=0.0))  # the claim again, part of the card
        return Succession(
            FadeIn(start, shift=0.15 * DOWN, run_time=0.5),
            _Morph(self, start, run_time=1.5),
            _Revealing(FadeIn(boxes), Create(self.check), shown=[boxes, self.check]),
        )

    def _fit(self, row: Mobject, slot: Mobject) -> Mobject:
        """`row` at the card's size (as placed), left in `slot`; never wider than the card."""

        row.scale(self.claim.height / self._height)
        room = self.cross.get_left()[0] - MARK_GAP - slot.get_left()[0]
        if row.width > room:
            row.scale(room / row.width)
        if row.height > slot.height + ROW_GAP / 2:
            raise _taller("evidence")
        return row.move_to(slot, aligned_edge=LEFT)

    def _changed(self) -> list[VGroup]:
        """The glyphs of the fix's atoms that the claim does not have."""

        fix, claim = self.fix, self.claim
        sources = [getattr(m, "authored_tex", None) for m in (fix, claim)]
        if not all(isinstance(m, SingleStringMathTex) for m in (fix, claim)) or None in sources:
            return []
        old = set(atoms(sources[1]))
        new = dict.fromkeys(atom for atom in atoms(sources[0]) if atom not in old)
        return [group for atom in new for group in term_groups(fix, sources[0], atom)]


class _Revealing(AnimationGroup):
    """Reserved parts become visible only as their entrance begins, never before (a beat's
    transition may play first)."""

    def __init__(self, *animations: Animation, shown: Sequence[Mobject]) -> None:
        super().__init__(*animations)
        self.director_shown = shown  # the timeline counts them as entering

    def begin(self) -> None:
        for mobject in self.director_shown:
            reveal(leaf for leaf in mobject.family_members_with_points())
        super().begin()


class _Morph(AnimationGroup):
    """`start`, a part of the card, turns into a copy of the card's reserved fix; then the fix
    itself takes the copy's place, so the card keeps the row."""

    def __init__(self, card: Misconception, start: Mobject, run_time: float) -> None:
        self._card, self._start = card, start
        self._shown = card.fix.copy()
        reveal(leaf for leaf in self._shown.family_members_with_points())
        super().__init__(motion.morph(start, self._shown), run_time=run_time)

    def clean_up_from_scene(self, scene: object) -> None:
        self._card.remove(self._start)  # gone with the morph, not split out of the card
        super().clean_up_from_scene(scene)
        scene.remove(self._shown)  # type: ignore[attr-defined]
        reveal(leaf for leaf in self._card.fix.family_members_with_points())


def _typeset(item: Item) -> Mobject:
    """TeX as math; words (with `$math$` in them, or no TeX at all) as a label in the theme's
    font, at body size."""

    if not isinstance(item, str):
        return item
    words = "$" in item or ("\\" not in item and re.search(r"[A-Za-z]{2}", item))
    return label(item, Role.BODY) if words else context.math(item)


def _taller(row: str) -> CompositionError:
    return CompositionError(
        f"This {row} row is taller than the room the card kept for it. Declare the rows up "
        "front, misconception(claim, evidence=[...], fix=...), so each gets its own height."
    )


def _cross(size: float) -> VMobject:
    hue = context.color("highlight")
    half = size / 2
    return VGroup(
        Line(half * (LEFT + UP), half * (RIGHT + DOWN)),
        Line(half * (LEFT + DOWN), half * (RIGHT + UP)),
    ).set_stroke(hue, width=MARK_WIDTH)


def _check(size: float) -> VMobject:
    hue = context.color("success")
    tick = VMobject().set_points_as_corners(
        [
            size * (-0.5 * RIGHT + 0.05 * UP),
            size * (-0.15 * RIGHT - 0.4 * UP),
            size * (0.55 * RIGHT + 0.5 * UP),
        ]
    )
    return tick.set_stroke(hue, width=MARK_WIDTH)
