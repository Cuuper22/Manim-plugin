"""Layout of stepwise derivations: lines aligned on their relation, notes beside or under them."""

from __future__ import annotations

from collections.abc import Sequence

from manim import DOWN, LEFT, RIGHT, MathTex, Mobject, SingleStringMathTex, VGroup

from .texscan import is_relation

LINE_GAP = 0.32
NOTE_GAP = 0.6  # beside the lines
NOTE_BELOW = 0.12  # under its line, much closer than the next line so it reads as its own


class Derivation(VGroup):
    """The on-stage result of `derive()`; `lines` and `notes` follow the steps in order.
    `stacked` (not in place) and `below` (notes under their lines) say how it was laid out,
    so `derive(continues=...)` can add lines the same way."""

    def __init__(
        self,
        lines: Sequence[MathTex],
        notes: Sequence[Mobject | None],
        *,
        stacked: bool = True,
        below: bool = False,
    ) -> None:
        super().__init__(*lines, *(note for note in notes if note is not None))
        self.lines = list(lines)
        self.notes = list(notes)
        self.stacked, self.below = stacked, below
        self.natural_width = float(lines[0].width)  # before placement scaled it

    @property
    def size(self) -> float:
        """How much placement has scaled the derivation since it was built."""

        return float(self.lines[0].width) / self.natural_width

    def extend(self, lines: Sequence[MathTex], notes: Sequence[Mobject | None]) -> None:
        self.add(*lines, *(note for note in notes if note is not None))
        self.lines += lines
        self.notes += notes


def stack(
    lines: Sequence[SingleStringMathTex], notes: Sequence[Mobject | None], *, below: bool = False
) -> None:
    """One line per step, relations in one column (lines without one align by their left);
    notes in a column beside the lines, or each under its own line."""

    top = 0.0
    for line, note in zip(lines, notes, strict=True):
        line.move_to((0, top - line.height / 2, 0))
        top -= line.height + LINE_GAP
        if below and note is not None:
            note.move_to((0, top + LINE_GAP - NOTE_BELOW - note.height / 2, 0))
            top -= note.height + NOTE_BELOW
    column = relation_x(lines[0])
    for line in lines[1:]:
        line.shift(RIGHT * (column - relation_x(line)))
    right = max(line.get_right()[0] for line in lines)
    for line, note in zip(lines, notes, strict=True):
        if note is not None and below:
            note.align_to(line, LEFT)
        elif note is not None:
            note.move_to((right + NOTE_GAP + note.width / 2, line.get_center()[1], 0))


def overlay(lines: Sequence[SingleStringMathTex], notes: Sequence[Mobject | None]) -> None:
    """Every step in the same place; its note centered below it."""

    for line, note in zip(lines, notes, strict=True):
        line.move_to((0, 0, 0))
        if note is not None:
            note.next_to(line, DOWN, buff=NOTE_GAP)


def relation_x(line: SingleStringMathTex) -> float:
    """The left edge of the line's first top-level relation, else of the line itself."""

    for part in line.submobjects:
        if is_relation(getattr(part, "tex_string", "")) and part.submobjects:
            return float(part.get_left()[0])
    return float(line.get_left()[0])
