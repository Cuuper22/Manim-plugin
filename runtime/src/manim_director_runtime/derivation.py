"""Layout of stepwise derivations: lines aligned on their relation, notes beside them."""

from __future__ import annotations

from collections.abc import Sequence

from manim import DOWN, RIGHT, MathTex, Mobject, SingleStringMathTex, VGroup

from .texscan import is_relation

LINE_GAP = 0.32
NOTE_GAP = 0.6


class Derivation(VGroup):
    """The on-stage result of `derive()`; `lines` and `notes` follow the steps in order."""

    def __init__(self, lines: Sequence[MathTex], notes: Sequence[Mobject | None]) -> None:
        super().__init__(*lines, *(note for note in notes if note is not None))
        self.lines = list(lines)
        self.notes = list(notes)


def stack(lines: Sequence[SingleStringMathTex], notes: Sequence[Mobject | None]) -> None:
    """One line per step, relations in one column (lines without one align by their left)."""

    VGroup(*lines).arrange(DOWN, buff=LINE_GAP)
    column = relation_x(lines[0])
    for line in lines[1:]:
        line.shift(RIGHT * (column - relation_x(line)))
    right = max(line.get_right()[0] for line in lines)
    for line, note in zip(lines, notes, strict=True):
        if note is not None:
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
