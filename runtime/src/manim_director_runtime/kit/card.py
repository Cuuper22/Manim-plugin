"""Misconception: a tempting wrong idea, tested, struck and repaired, on one card.

The viewer must first recognize the claim as their own belief, so it starts in neutral ink.
Evidence then refutes it, the struck claim stays in view, and the repair arrives as a small,
boxed change from it: the viewer updates the belief they hold instead of memorizing a new one.
"""

from __future__ import annotations

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
    TransformMatchingTex,
    VGroup,
    VMobject,
)

from ..errors import CompositionError
from ..terms import term_groups
from ..texscan import atoms
from . import context
from .labels import backdrop, label
from .overlay import pacing, reserve, reveal

ROW_GAP = 0.32
MARK_GAP = 0.35
MARK_WIDTH = 6.0
STRUCK_OPACITY = 0.45


class Misconception(VGroup):
    """`tag` over the `claim`, with reserved room for a ✗, `evidence` rows of tests and a
    repair row with a ✓, so nothing reflows as the story plays: `test(...)`, `refute()`,
    `repair(...)`, each an Animation to play."""

    def __init__(self, claim: str | Mobject, *, tag: str = "Tempting", evidence: int = 2) -> None:
        super().__init__()
        if not (isinstance(evidence, int) and evidence >= 1):
            raise CompositionError(
                f"misconception(evidence={evidence!r}) needs a whole number >= 1."
            )
        self.claim = context.math(claim) if isinstance(claim, str) else claim
        self.tag = label(tag).next_to(self.claim, UP, buff=0.22, aligned_edge=LEFT)
        size = 0.55 * self.claim.height
        self.cross = _cross(size).next_to(self.claim, RIGHT, buff=MARK_GAP)
        left, right = self.claim.get_left(), self.claim.get_right()
        self.strike = Line(left + 0.12 * (LEFT + DOWN), right + 0.12 * (RIGHT + UP))
        self.strike.set_stroke(context.color("accent"), width=MARK_WIDTH - 1)
        self.slots = VGroup(
            *(
                Rectangle(width=self.claim.width, height=self.claim.height, stroke_opacity=0)
                for _ in range(evidence + 1)
            )
        ).arrange(DOWN, buff=ROW_GAP)
        self.slots.next_to(self.claim, DOWN, buff=ROW_GAP, aligned_edge=LEFT)
        self.check = _check(size).next_to(self.slots[-1], RIGHT, buff=MARK_GAP)
        self.check.set_x(self.cross.get_x())
        self.rows = VGroup()
        self.fix: Mobject | None = None
        self.diff: VGroup | None = None  # boxes behind what the repair changed
        pacing(self.strike, "strike", chunks=0, read=0.0)  # read as one mark with the ✗
        pacing(self.cross, "verdict", chunks=1, read=0.5)
        pacing(self.check, "verdict", chunks=1, read=0.5)
        for part in (self.cross, self.strike, self.check):
            reserve(part)
        self.add(self.tag, self.claim, self.cross, self.strike, self.slots, self.check, self.rows)
        self._height = self.claim.height
        pacing(self, "card", chunks=2, read=1.0)

    def test(self, *evidence: str | Mobject) -> Animation:
        """Fill the next evidence rows with checks the viewer can compute, one after another."""

        free = len(self.slots) - 1 - len(self.rows)
        if not evidence or len(evidence) > free:
            raise CompositionError(
                f"test() fills {free} more evidence row(s) of this card; got {len(evidence)}. "
                "Ask for more with misconception(..., evidence=n)."
            )
        rows = [self._fit(item, self.slots[len(self.rows) + i]) for i, item in enumerate(evidence)]
        self.rows.add(*(reserve(row) for row in rows))
        entrances = [FadeIn(row, shift=0.15 * DOWN) for row in rows]
        return _Revealing(LaggedStart(*entrances, lag_ratio=0.6), shown=rows)

    def refute(self) -> Animation:
        """Strike the claim and mark it ✗; it fades but stays, to compare with the repair."""

        fade = self.claim.animate.set_opacity(STRUCK_OPACITY)
        return _Revealing(
            Create(self.strike), Create(self.cross), fade, shown=[self.strike, self.cross]
        )

    def repair(self, fix: str | Mobject) -> Animation:
        """The claim, copied, turns into `fix` in the repair row; what changed is boxed in
        `success`, and the row gets a ✓."""

        if self.fix is not None:
            raise CompositionError("This card is already repaired.")
        self.fix = self._fit(fix, self.slots[-1])
        boxes = VGroup(*(backdrop(g, context.color("success"), 0.25) for g in self._changed()))
        boxes.set_z_index(self.fix.z_index - 1)
        self.diff = boxes
        morph = _Morph(self.claim, self.fix)  # copies the fix, then reserves it
        self.add(reserve(pacing(boxes, "diff", chunks=0, read=0.0)), self.fix)
        return Succession(
            morph, _Revealing(FadeIn(boxes), Create(self.check), shown=[boxes, self.check])
        )

    def _fit(self, item: str | Mobject, slot: Mobject) -> Mobject:
        """`item` at the claim's size (as placed), left in `slot`; never wider than the card."""

        row = context.math(item) if isinstance(item, str) else item
        row.scale(self.claim.height / self._height)
        room = self.cross.get_right()[0] - slot.get_left()[0]
        if row.width > room:
            row.scale(room / row.width)
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
        self._shown = shown

    def begin(self) -> None:
        for mobject in self._shown:
            reveal(leaf for leaf in mobject.family_members_with_points())
        super().begin()


class _Morph(TransformMatchingTex):
    """A copy of the claim, at full strength, turns into the fix; then the fix itself takes
    its place, so the card keeps the row."""

    def __init__(self, claim: Mobject, fix: Mobject) -> None:
        self._fix = fix
        shown = fix.copy()
        reserve(fix)
        super().__init__(claim.copy().set_opacity(1), shown)

    def clean_up_from_scene(self, scene: object) -> None:
        super().clean_up_from_scene(scene)
        scene.remove(self.to_add)  # type: ignore[attr-defined]
        reveal(leaf for leaf in self._fix.family_members_with_points())


def _cross(size: float) -> VMobject:
    hue = context.color("accent")
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
