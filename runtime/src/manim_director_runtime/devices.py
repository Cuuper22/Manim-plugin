"""Scene methods that reveal and explain kit content, mixed into Directed.

`place()` positions top-level content; `show()` reveals what belongs to placed content:
overlays (`plot.tangent(1)`) and reserved parts (`dots.select(...)` of a `DotArray` built with
`shown=`). `annotate`, `link` and `ask` direct the viewer: a note where the thing is, a term
paired with its part of the picture, a prediction prompt with time to think.
"""

from __future__ import annotations

from collections.abc import Callable, Sequence
from typing import TYPE_CHECKING, Any, Literal

import numpy as np
from manim import (
    DL,
    DOWN,
    DR,
    LEFT,
    RIGHT,
    UL,
    UP,
    UR,
    AnimationGroup,
    Arrow,
    Brace,
    Circle,
    Dot,
    FadeIn,
    FadeToColor,
    GrowArrow,
    GrowFromCenter,
    LaggedStart,
    Mobject,
    VGroup,
    VMobject,
)

from . import motion
from .beats import Transition
from .errors import CompositionError
from .kit.card import Misconception
from .kit.labels import backdrop, halo, label, trace
from .kit.overlay import derived, is_overlay, pacing, reserved, reveal
from .layout import LANES, Rect, Region
from .staging import bounds, describe
from .themes import Role

if TYPE_CHECKING:
    from manim import Animation

NOTE_SECONDS = 0.9
LINK_SECONDS = 1.0
_ARROW_GAPS = (0.45, 0.75, 1.1, 1.6, 2.2)  # shorter arrows shrink to bare tips
_LABEL_GAPS = (0.12, 0.25, 0.45, 0.75)
_SIDES = {"up": UP, "down": DOWN, "right": RIGHT, "left": LEFT}
_SEARCH = (UP, DOWN, RIGHT, LEFT, UR, DR, UL, DL)
_CLEARANCE = 0.06  # between a note and anything already drawn
Style = Literal["arrow", "brace", "label"]


class Note(VGroup):
    """What `annotate` drew: its `label`, the `pointer` to the target (an arrow or a brace;
    None for a label set right beside it) and the soft `box` behind the term (or None)."""

    def __init__(self, label: Mobject, pointer: Mobject | None, box: Mobject | None) -> None:
        super().__init__(*(part for part in (box, pointer, label) if part is not None))
        self.label, self.pointer, self.box = label, pointer, box


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
        seconds = motion.SHOW_SECONDS if run_time is None else run_time
        self._settle(mobjects, seconds)
        visible = self._visible_ids()
        entrances: list[Animation] = []
        for mobject in mobjects:
            if is_overlay(mobject) and not self._visible(mobject):
                parent = mobject.director_parent
                if not self._holds(parent):
                    raise CompositionError(
                        f"{describe(mobject)} is drawn on {describe(parent)}, which is not on "
                        "stage: place() it first."
                    )
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
        self._perform([animation], seconds, "show", shown=mobjects)

    def annotate(
        self: Any,
        target: Mobject,
        note: str,
        *,
        term: str | None = None,
        occurrence: int | None = None,
        style: Style = "arrow",
        side: str | Sequence[float] = "auto",
        color: str = "accent",
        mark: bool = True,
        persist: bool = False,
        run_time: float | None = None,
    ) -> Note:
        """Explain one thing where it is: `note` beside `target`, or beside the `term` of a
        formula (which gets a soft box behind it, its glyphs keeping their colors), joined to
        it by an arrow, a brace, or nothing (`style="label"`: a name set right beside a part
        of a picture). The note takes the first free side, clear of everything on stage
        (`side="up"` or a direction picks one), and follows the target. Words and `$math$`
        mix as in a label. In a beat the newest note is lit and earlier ones turn muted. A
        note on a single symbol names it for the viewer. Beat-local unless `persist`."""

        if style not in ("arrow", "brace", "label"):
            raise CompositionError(f"style={style!r}; use 'arrow', 'brace' or 'label'.")
        self._flush()
        if not self._visible(target):
            raise CompositionError(
                f"annotate() explains what is on stage; {describe(target)} is not."
            )
        glyphs = target if term is None else self.term(target, term, occurrence=occurrence)
        seconds = NOTE_SECONDS if run_time is None else run_time
        self._settle([target], seconds)
        hue = self.theme.color(color)
        words = halo(label(note, color=color))
        # A brace hugs its term, closer than notes keep to anything else.
        avoid = self._obstacles(reserved=persist, besides=glyphs if style == "brace" else None)
        spot = _free_side(
            glyphs,
            words,
            style,
            _directions(side, style, wide=glyphs.width >= glyphs.height),
            avoid,
            self.region("content"),
        )
        if spot is None:
            raise CompositionError(
                f"No free space beside {describe(target)} for the note {note!r}: shorten it, "
                "give the target more room, or choose another side=.",
                note=note,
            )
        boxed = mark and term is not None
        drawn = derived(_note_builder(glyphs, words, spot, style, hue, boxed))
        drawn.director_parent = self._placed_root(glyphs.family_members_with_points()[0])
        drawn.director_persist = persist
        muted = self.theme.muted
        quieted: list[Animation] = []
        beat = self._beat.id if self._beat is not None else None
        for older in [n for b, n in self._notes if b == beat and self._visible(n)]:
            parts = (older.label, older.pointer)
            quieted += [FadeToColor(part, muted) for part in parts if part is not None]
            if older.box is not None:
                quieted.append(older.box.animate.set_fill(opacity=0))
        self._notes = [(b, n) for b, n in self._notes if b == beat] + [(beat, drawn)]
        direction = spot[0]
        entrance = [] if drawn.box is None else [FadeIn(drawn.box)]
        if isinstance(drawn.pointer, Arrow):
            entrance.append(GrowArrow(drawn.pointer))
        elif drawn.pointer is not None:
            entrance.append(GrowFromCenter(drawn.pointer))
        entrance.append(FadeIn(drawn.label, shift=0.15 * direction / np.linalg.norm(direction)))
        self._draw(drawn, [AnimationGroup(*entrance, lag_ratio=0.3), *quieted], seconds, "annotate")
        symbol = term if term is not None else getattr(target, "authored_tex", None)
        if symbol is not None and symbol.strip() in self._symbol_colors:
            self._named(symbol.strip(), "annotate")
        return drawn

    def link(
        self: Any,
        *targets: Mobject,
        color: str = "accent",
        run_time: float | None = None,
        persist: bool = False,
    ) -> VGroup:
        """Show that things on stage are one thing, typically a term and its part of the
        picture (`self.link(self.term(eq, "5"), ring)`): each is marked in `color` at the
        same moment, with one pulse (a soft box behind glyphs, a halo behind dots, a glow
        behind lines and shapes). Nothing is recolored. Beat-local unless `persist`."""

        if len(targets) < 2:
            raise CompositionError(
                "link() pairs two or more things on stage, e.g. "
                "self.link(self.term(eq, 'n'), side)."
            )
        self._flush()
        missing = [describe(t) for t in targets if not self._visible(t)]
        if missing:
            raise CompositionError(f"Cannot link {', '.join(missing)}: not on stage.")
        seconds = LINK_SECONDS if run_time is None else run_time
        self._settle(targets, seconds)
        hue = self.theme.color(color)
        marks = []
        for target in targets:
            leaves = _inked(target)
            drawn = derived(lambda leaves=leaves: _link_mark(leaves, hue))
            drawn.director_parent = self._placed_root(leaves[0])
            drawn.director_persist = persist
            marks.append(pacing(drawn, "link", chunks=0, read=0.0))  # points at, adds nothing
        group = VGroup(*marks)
        self._draw(group, [FadeIn(m, scale=1.25) for m in marks], seconds, "link")
        return group

    def ask(self: Any, question: str, *, hold: float | None = None) -> Mobject:
        """Ask the viewer to predict before you show: `question` replaces the caption, behind
        a "?" mark, and nothing moves for `hold` seconds (by default the viewer's
        `ask_hold`, 3 s) while they commit to a guess. Ask in its own beat, right before the
        reveal it sets up."""

        seconds = self._budgets.ask_hold if hold is None else hold
        if seconds < 0:
            raise CompositionError("ask(hold=...) cannot be negative.")
        self._settle([], motion.TRANSITION_SECONDS[Transition.CONTINUE])
        prompt = self._set_caption(question, self._ask_mark())
        self._flush(source="ask")
        self._asked(question, seconds)
        self._still_for(seconds)
        return prompt

    def misconception(
        self: Any,
        claim: str | Mobject,
        *,
        tag: str = "Tempting",
        region: Region | str = Region.LEFT,
        evidence: int = 2,
    ) -> Misconception:
        """Stage a tempting wrong idea as a card, in neutral ink so the viewer recognizes it as
        their own, with room kept for `evidence` tests, a ✗ and a repair row. Then play its
        story, a beat each: `card.test(...)`, `card.refute()`, `card.repair(...)`. The struck
        claim stays in view to compare with its repair. It enters like placed content."""

        card = Misconception(claim, tag=tag, evidence=evidence)
        self.place(card, region=region)
        return card

    def pause(self: Any) -> None:
        """Hold still until the viewer has taken in the last change (its reading time at the
        viewer's level) before a plain `play`: read, then watch. The devices and `derive`
        pause like this by themselves."""

        self._flush()
        self._settle([], 0.0)

    # Internals ------------------------------------------------------------------------------

    def _draw(
        self: Any, drawn: Mobject, animations: list[Animation], seconds: float, source: str
    ) -> None:
        """Bring in derived overlays: their own redraw waits while their entrance plays."""

        parts = drawn.submobjects if source == "link" else [drawn]
        for part in parts:
            self.add(part)
            part.suspend_updating()
        try:
            self._perform(animations, seconds, source, shown=parts)
        finally:
            for part in parts:
                part.resume_updating()

    def _obstacles(self: Any, *, reserved: bool, besides: Mobject | None = None) -> list[Rect]:
        """What a note keeps clear of: the content drawn on stage, lines as small boxes along
        their path (a bounding box would wall off a whole diagonal), and with `reserved` the
        parts laid out for later (a beat-local note is gone before they appear). Ground, such
        as grid lines (`director_ground`), may be written over: notes have a halo. So may
        `besides`, the term a brace hugs."""

        rects: list[Rect] = []
        for top in self.mobjects:
            if self._backstage(top) or self._stage.region_of(top) in LANES:
                continue
            ground = {
                id(leaf)
                for part in top.get_family()
                if getattr(part, "director_ground", False)
                for leaf in part.get_family()
            }
            if besides is not None:
                ground |= {id(leaf) for leaf in besides.get_family()}
            for leaf in top.family_members_with_points():
                if id(leaf) in ground:
                    continue
                if not (_ink(leaf) or (reserved and getattr(leaf, "director_hidden", False))):
                    continue
                if isinstance(leaf, VMobject) and leaf.get_fill_opacity() == 0:
                    rects += trace([leaf], pad=0.05)
                else:
                    rects.append(bounds(leaf))
        return rects

    def _ask_mark(self: Any) -> VGroup:
        ring = Circle(radius=0.2, stroke_color=self.theme.accent, stroke_width=2.5)
        glyph = self.text("?", Role.CAPTION, weight="BOLD", color=self.theme.accent)
        return VGroup(ring, glyph.scale_to_fit_height(0.22).move_to(ring))


def _directions(side: str | Sequence[float], style: Style, *, wide: bool) -> list[np.ndarray]:
    if isinstance(side, str):
        if side == "auto" and style == "brace":  # along the term's long side
            return [DOWN, UP] if wide else [RIGHT, LEFT]
        if side == "auto":
            return list(_SEARCH)
        if side not in _SIDES:
            raise CompositionError(f"side={side!r}; use 'auto', {', '.join(map(repr, _SIDES))}.")
        return [_SIDES[side]]
    direction = np.array([float(side[0]), float(side[1]), 0.0])
    if not np.any(direction):
        raise CompositionError("side= needs a direction such as UP or DR.")
    if style == "brace" and direction[0] and direction[1]:
        raise CompositionError("A brace goes up, down, left or right of its term.")
    return [direction]


def _free_side(
    glyphs: Mobject,
    words: Mobject,
    style: Style,
    directions: Sequence[np.ndarray],
    avoid: Sequence[Rect],
    area: Rect,
) -> tuple[np.ndarray, np.ndarray] | None:
    """The first (direction, label offset from the target's edge there) where the label,
    and a brace, sit inside `area` and clear of `avoid`; nearer spots first."""

    def free(rect: Rect) -> bool:
        return area.contains(rect) and not any(rect.overlaps(r, _CLEARANCE) for r in avoid)

    if style == "brace":
        for d in directions:
            brace = Brace(glyphs, direction=d, buff=0.08)
            words.next_to(brace, d, buff=0.1)
            if free(bounds(brace)) and free(bounds(words)):
                return d, words.get_center() - glyphs.get_critical_point(d)
        return None
    target = bounds(glyphs)
    for gap in _LABEL_GAPS if style == "label" else _ARROW_GAPS:
        for d in directions:
            x = target.center[0] + d[0] * (target.width / 2 + words.width / 2 + gap)
            y = target.center[1] + d[1] * (target.height / 2 + words.height / 2 + gap)
            if free(Rect.around((x, y), words.width, words.height)):
                center = np.array([x, y, 0.0])
                return d, center - glyphs.get_critical_point(d)
    return None


def _note_builder(
    glyphs: Mobject,
    words: Mobject,
    spot: tuple[np.ndarray, np.ndarray],
    style: Style,
    hue: str,
    boxed: bool,
) -> Callable[[], Note]:
    """Draws the note from where its glyphs are now, scaled with them (R4: no state)."""

    direction, offset = spot
    size = _size(glyphs)
    depth = min(leaf.z_index for leaf in glyphs.family_members_with_points()) - 1

    def build() -> Note:
        k = _size(glyphs) / size
        anchor = glyphs.get_critical_point(direction)
        text = words.copy().scale(k).move_to(anchor + k * offset)
        pointer: Mobject | None = None
        if style == "arrow":
            pointer = Arrow(
                text.get_critical_point(-direction),
                anchor,
                buff=0.08 * k,
                color=hue,
                stroke_width=3,
                tip_length=0.16 * k,
                max_tip_length_to_length_ratio=0.35,
                max_stroke_width_to_length_ratio=12,
            )
        elif style == "brace":
            pointer = Brace(glyphs, direction=direction, buff=0.08 * k, color=hue)
        box = backdrop(glyphs, hue).set_z_index(depth) if boxed else None
        return Note(text, pointer, box)

    return build


def _link_mark(leaves: Sequence[VMobject], hue: str) -> VMobject:
    """A soft halo behind each dot, a box behind glyphs, a glow behind lines and shapes."""

    depth = min(leaf.z_index for leaf in leaves) - 1
    if all(isinstance(leaf, Dot) for leaf in leaves):
        mark = VGroup(
            *(
                Dot(leaf.get_center(), radius=0.95 * leaf.width, color=hue, fill_opacity=0.4)
                for leaf in leaves
            )
        )
    elif all(leaf.get_fill_opacity() > 0 and leaf.get_stroke_width() == 0 for leaf in leaves):
        mark = backdrop(VGroup(*leaves), hue, opacity=0.3)  # as strong as the dots' halos
    else:
        mark = VGroup(
            *(
                leaf.copy().set_fill(opacity=0).set_stroke(hue, width=10, opacity=0.45)
                for leaf in leaves
            )
        )
    return mark.set_z_index(depth)


def _inked(mobject: Mobject) -> list[VMobject]:
    leaves = [leaf for leaf in mobject.family_members_with_points() if _ink(leaf)]
    if not leaves:
        raise CompositionError(f"{describe(mobject)} has nothing drawn to point at.")
    return leaves


def _ink(leaf: Mobject) -> bool:
    if not isinstance(leaf, VMobject) or getattr(leaf, "director_hidden", False):
        return False
    stroke = leaf.get_stroke_width() > 0 and leaf.get_stroke_opacity() > 0
    return leaf.get_fill_opacity() > 0 or stroke


def _size(mobject: Mobject) -> float:
    return max(mobject.width, mobject.height, 1e-6)
