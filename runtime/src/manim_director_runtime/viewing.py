"""What the viewer is shown, recorded animation by animation into timeline v2 (R8).

DirectedScene wraps every animation in `_shown` and every wait in `_still`. Both do nothing
unless a render handler is recording, so plain `manim` renders pay nothing. A record says what
entered (and how much there is to take in), what moved, and the statement in the scene that
played it; pacing QA judges the records afterwards.
"""

from __future__ import annotations

import math
import re
import sys
from collections import Counter, deque
from collections.abc import Collection, Iterable, Iterator, Sequence
from contextlib import contextmanager
from dataclasses import replace
from itertools import combinations
from typing import Any

from manim import (
    AnimationGroup,
    DecimalNumber,
    MarkupText,
    Mobject,
    ReplacementTransform,
    SingleStringMathTex,
    Tex,
    Text,
    ValueTracker,
    VMobject,
    Wait,
    config,
)
from manim.animation.transform_matching_parts import TransformMatchingAbstractBase

from . import pacing, timeline
from .kit.overlay import is_overlay
from .layout import LANES, Rect
from .motion import DIM_OPACITY
from .staging import bounds, within
from .texscan import occurrences
from .timeline import Ask, Definition, Event, LaneText, Seen, Settle, TextBox

_MOTIONS = frozenset({"play", "tracker", "show"})  # sources whose targets count as motions
_LABEL_CHARS = 48
_MATH_SPAN = re.compile(r"\$[^$]*\$")  # counts as one word of a text-mode Tex
_MARKUP = re.compile(r"<[^>]*>")


class Viewing:
    """Recording hooks, mixed into Directed."""

    @contextmanager
    def _shown(
        self: Any, source: str, animations: Sequence[Any], shown: Sequence[Mobject] | None = None
    ) -> Iterator[None]:
        """Describe the animation played inside as one event. `shown` is what enters, when the
        animations do not say it themselves (a reveal of reserved parts). The scene keeps the
        events for its automatic holds; a recording render also writes them to the timeline."""

        at = self._clock()
        played = list(_flat(animations))
        introduced = _introduced(played)
        content = [m for m in introduced if self._stage.region_of(m) not in LANES]
        morphed = [m for a in played if (m := _morph_target(a)) is not None]
        # A transition plays what earlier statements staged: point at the first of them.
        staged = [
            m.director_statement
            for m in content + morphed + introduced
            if hasattr(m, "director_statement")
        ]
        file, line = staged[0] if staged and source == "transition" else self._statement()
        if shown is None:
            shown = content
        symbols = list(self._symbol_colors)
        entered = [seen for m in shown if (seen := describe(m, symbols)).chunks or seen.read]
        # What a morph brings in costs its changed glyphs (morph_glyphs), but its notation
        # is new to the viewer all the same.
        entered += [
            replace(seen, chunks=0, read=None, words=0, glyphs=0)
            for m in morphed
            if (seen := describe(m, symbols)).symbols or seen.conventions
        ]
        moved = [a.mobject for a in played if getattr(a, "mobject", None) is not None]
        units, tracked = self._targets(list(shown) + moved) if source in _MOTIONS else ([], False)
        if source == "play" and tracked and not units:
            source = "tracker"
        unit = self._unit_of(shown)
        yield
        recorder = timeline.active()
        event = Event(
            at=at,
            seconds=round(self._clock() - at, 6),
            beat=self._beat.id if self._beat is not None else None,
            source=source,
            file=file if recorder is None else recorder.where(file),
            line=line,
            entered=entered,
            targets=len(units) + tracked,
            spread=_spread(units),
            morph_glyphs=sum(_changed_glyphs(a) for a in played),
            of=unit,
        )
        self._events.append(event)
        lanes = self._lane_texts()
        self._lanes_since = {
            key: self._lanes_since.get(key, (at, text)) for key, text in lanes.items()
        }
        if recorder is not None:
            recorder.events.append(event)
            recorder.lanes(lanes, at)

    @contextmanager
    def _still(self: Any) -> Iterator[None]:
        """Record the stage during the wait inside."""

        recorder = timeline.active()
        if recorder is None:
            yield
            return
        at = recorder.clock(self.renderer.time)
        chunks, colors, boxes = self._stage_view()
        yield
        recorder.settles.append(
            Settle(
                at=at,
                beat=self._beat.id if self._beat is not None else None,
                still_seconds=recorder.since(at, self.renderer.time),
                visible_chunks=chunks,
                colors=colors,
                text_boxes=boxes,
            )
        )
        recorder.lanes(self._lane_texts(), at)

    def _watched(self, animations: Sequence[Any]) -> Any:
        """`_still` for a wait, else `_shown` for a play of the scene's own code."""

        if animations and all(isinstance(a, Wait) for a in animations):
            return self._still()
        return self._shown("play", animations)

    def _placed_by(self, mobject: Mobject) -> None:
        """Remember the statement that placed `mobject`, which enters later."""

        mobject.director_statement = self._statement()

    def _lane(self, mobject: Mobject, kind: str, text: str) -> None:
        """Mark a caption or title, so the timeline can tell how long it stays on screen."""

        mobject.director_lane = LaneText(kind, text, *self._statement())  # type: ignore[arg-type]

    def _asked(self: Any, question: str, hold: float) -> None:
        """Record a prediction prompt that is on screen now (for `ask`)."""

        recorder = timeline.active()
        if recorder is not None:
            file, line = self._statement()
            at = recorder.clock(self.renderer.time)
            recorder.asks.append(Ask(at, hold, question, recorder.where(file), line))

    def _named(self: Any, symbol: str, via: str) -> None:
        """Record that `symbol` is named on screen now (by an annotation or a legend row)."""

        recorder = timeline.active()
        if recorder is not None:
            at = recorder.clock(self.renderer.time)
            recorder.definitions.append(Definition(at, symbol, via))

    # Internals ------------------------------------------------------------------------------

    def _start_viewing(self) -> None:
        self._events: list[Event] = []  # every change shown so far, recorded or not
        self._lanes_since: dict[int, tuple[float, LaneText]] = {}  # captions/titles on screen
        self._units: dict[int, str] = {}

    def _clock(self: Any) -> float:
        """Seconds into the video: the recorder's count of frames written when recording."""

        recorder = timeline.active()
        seconds = self.renderer.time
        return round(seconds if recorder is None else recorder.clock(seconds), 6)

    def _still_for(self: Any, seconds: float) -> None:
        """Hold still for at least `seconds`, in whole frames. Manim repeats a still frame
        int(duration / frame time) times, so a hold computed to the budget could round down
        a frame short of it."""

        rate = config.frame_rate
        frames = math.ceil(seconds * rate - 1e-6)
        if frames <= 0:
            return
        duration = frames / rate
        while int(duration / (1 / rate)) < frames:
            duration = math.nextafter(duration, math.inf)
        self.wait(duration)

    def _unit_of(self, shown: Sequence[Mobject]) -> str | None:
        """A name, stable within this render (`plot-1`, `dots-2`), for the one component that
        everything in `shown` belongs to, if there is one."""

        owners, _ = self._targets(shown)
        kind = getattr(owners[0], "director_kind", None) if len(owners) == 1 else None
        if kind is None:
            return None
        return self._units.setdefault(id(owners[0]), f"{kind}-{len(self._units) + 1}")

    def _settle(self: Any, shown: Sequence[Mobject], seconds: float) -> None:
        """Before a device plays: hold still until the viewer has taken in the beat's last
        change, unless the device continues that same reveal (R7). Content staged for this
        beat enters with the device instead, so nothing is held for it."""

        if self._stage.pending():
            return
        beat = self._beat.id if self._beat is not None else None
        events = [e for e in self._events if e.beat == beat]
        rest = pacing.owed(
            events, self._budgets, now=self._clock(), of=self._unit_of(shown), seconds=seconds
        )
        self._still_for(rest)

    def _statement(self) -> tuple[str, int]:
        """The innermost line of the scene's own file on the stack: the statement that caused
        what is being recorded, else the scene's construct."""

        code = type(self).construct.__code__
        frame = sys._getframe(1)
        while frame is not None:
            if frame.f_code.co_filename == code.co_filename:
                return code.co_filename, frame.f_lineno
            frame = frame.f_back
        return code.co_filename, code.co_firstlineno

    def _unit(self: Any, mobject: Mobject) -> Mobject:
        """What a viewer sees as one thing: the component a part or overlay belongs to, else
        the placed object around it, else the mobject itself."""

        drawn_on = (o for o in self._stage.overlays.values() if within(mobject, [o]))
        mobject = next(drawn_on, mobject)  # a part of an overlay counts as the overlay
        while is_overlay(mobject):
            mobject = mobject.director_parent
        leaves = mobject.family_members_with_points()
        probe = leaves[0] if leaves else mobject
        root = self._placed_root(probe)
        if root is probe:
            return mobject
        components = (m for m in root.get_family() if hasattr(m, "director_kind"))
        return next((m for m in components if within(probe, [m])), root)

    def _targets(self, mobjects: Iterable[Mobject]) -> tuple[list[Mobject], bool]:
        """The distinct units among `mobjects`, and whether a value tracker is among them (a
        tracker and everything it drives count as one target)."""

        units: dict[int, Mobject] = {}
        tracked = False
        for mobject in mobjects:
            if isinstance(mobject, ValueTracker):
                tracked = True
            elif mobject.family_members_with_points():  # a Wait animates an empty Mobject
                unit = self._unit(mobject)
                units.setdefault(id(unit), unit)
        return list(units.values()), tracked

    def _lane_texts(self: Any) -> dict[int, LaneText]:
        return {
            id(m): m.director_lane
            for m in self.get_mobject_family_members()
            if hasattr(m, "director_lane")
        }

    def _stage_view(self: Any) -> tuple[int, list[str], list[TextBox]]:
        """Lit content chunks, the colors of lit content, and the visible text boxes."""

        names = {value: token for token, value in self.theme.tokens()}
        chunks, colors = 0, set()
        counted: set[int] = set()  # a placed group may be on stage as several pieces
        for top in self.mobjects:
            if self._backstage(top):
                continue
            base = self._stage.dimmed.get(id(top), (top, {}))[1]
            lit = [leaf for leaf in _drawn(top) if _lit(leaf, base)]
            if not lit:
                continue
            colors |= {names.get(color, color) for leaf in lit for color in _leaf_colors(leaf)}
            # Tags and highlight boxes belong to what they are attached to.
            follower = id(top) in self._stage.attached and id(top) not in self._stage.overlays
            unit = top if is_overlay(top) else self._unit(top)
            if self._stage.region_of(top) not in LANES and not follower and id(unit) not in counted:
                counted.add(id(unit))
                chunks += describe(unit, ()).chunks
        boxes = [
            TextBox(_label(unit), _frame_box(bounds(unit)))
            for top in self.mobjects
            if not self._backstage(top)
            for unit in _text_units(top)
            if _drawn(unit)
        ]
        return chunks, sorted(colors), boxes


def describe(mobject: Mobject, symbols: Collection[str]) -> Seen:
    """What a viewer must take in when `mobject` enters. Components and overlays declare their
    own load (`director_chunks`, `director_read`); plain content counts as one chunk whose
    reading time comes from its words and math glyphs."""

    family = mobject.get_family()
    found = [s for s in symbols if any(_has_symbol(m, s) for m in family)]
    conventions = list(
        dict.fromkeys(c for m in family for c in getattr(m, "director_conventions", ()))
    )
    if hasattr(mobject, "director_chunks"):
        held_back = _reserved(mobject)  # it counts when show() reveals it
        return Seen(
            kind=mobject.director_kind,
            label=_label(mobject),
            chunks=0 if held_back else mobject.director_chunks,
            read=0.0 if held_back else mobject.director_read,
            symbols=found,
            conventions=conventions,
        )
    declared: list[Mobject] = []
    words = glyphs = 0
    shapes = False
    pending = deque([mobject])
    while pending:
        m = pending.popleft()
        if m is not mobject and hasattr(m, "director_chunks"):
            if not _reserved(m):
                declared.append(m)
        elif isinstance(m, Tex):
            words += len(_MATH_SPAN.sub(" x ", getattr(m, "authored_tex", m.tex_string)).split())
        elif isinstance(m, SingleStringMathTex | DecimalNumber):  # reserved glyphs come later
            glyphs += sum(not _hidden(g) for g in m.family_members_with_points())
        elif isinstance(m, Text):  # `.text` has its spaces removed
            words += len(m.original_text.split())
        elif isinstance(m, MarkupText):
            words += len(_MARKUP.sub(" ", m.original_text).split())
        else:
            shapes = shapes or bool(_drawn(m, family=False))
            pending += m.submobjects
    plain = bool(words or glyphs or shapes)
    kinds = [m.director_kind for m in declared]
    return Seen(
        kind=_kind(words, glyphs, kinds),
        label=_label(mobject, declared),
        chunks=sum(m.director_chunks for m in declared) + int(plain),
        read=sum(m.director_read for m in declared) if declared else None,
        words=words,
        glyphs=glyphs,
        symbols=found,
        conventions=conventions,
    )


def _kind(words: int, glyphs: int, declared: list[str]) -> str:
    if declared and not (words or glyphs):
        return declared[0] if len(set(declared)) == 1 else "group"
    if words and glyphs:
        return "label"
    return "text" if words else "math" if glyphs else "shape"


def _has_symbol(mobject: Mobject, symbol: str) -> bool:
    tex = getattr(mobject, "authored_tex", None)
    return tex is not None and bool(occurrences(tex, symbol, math_only=isinstance(mobject, Tex)))


def _label(mobject: Mobject, parts: Sequence[Mobject] = ()) -> str:
    """The words or TeX of `mobject`, else the labels of its `parts`, else its kind."""

    text = getattr(mobject, "authored_text", None) or getattr(mobject, "authored_tex", None)
    if text is None and isinstance(mobject, Text | MarkupText):
        text = _MARKUP.sub("", mobject.original_text)
    elif text is None and isinstance(mobject, SingleStringMathTex):
        text = mobject.tex_string
    elif text is None and isinstance(mobject, DecimalNumber):
        text = f"{mobject.number:.{mobject.num_decimal_places}f}"
    lines = getattr(mobject, "lines", None)  # a Derivation
    if text is None and lines:
        text = getattr(lines[0], "authored_tex", None)
    counts = Counter(_label(part) for part in parts)
    text = (
        text
        or ", ".join(name if n == 1 else f"{n} × {name}" for name, n in counts.items())
        or getattr(mobject, "director_kind", None)
        or type(mobject).__name__
    )
    text = " ".join(str(text).split())
    return text if len(text) <= _LABEL_CHARS else text[: _LABEL_CHARS - 1] + "…"


def _flat(animations: Iterable[Any]) -> Iterator[Any]:
    """Members of animation groups; a matching morph stays whole (it is a group inside)."""

    for animation in animations:
        if isinstance(animation, AnimationGroup) and not isinstance(
            animation, TransformMatchingAbstractBase
        ):
            yield from _flat(animation.animations)
        else:
            yield animation


def _introduced(animations: Iterable[Any]) -> list[Mobject]:
    return [
        a.mobject
        for a in animations
        if callable(getattr(a, "is_introducer", None)) and a.is_introducer()
    ]


def _morph_target(animation: Any) -> Mobject | None:
    """What a morph turns its mobject into (and puts on stage in its place)."""

    if isinstance(animation, TransformMatchingAbstractBase):
        return animation.to_add
    if isinstance(animation, ReplacementTransform):
        return animation.target_mobject
    return None


def _changed_glyphs(animation: Any) -> int:
    """Glyphs of the target that a matching morph fades in rather than carries over."""

    if isinstance(animation, TransformMatchingAbstractBase):
        return len(animation.to_remove[1].family_members_with_points())
    return 0


def _spread(units: Sequence[Mobject]) -> float:
    rects = [bounds(unit) for unit in units if unit.family_members_with_points()]
    gaps = (_gap(a, b) for a, b in combinations(rects, 2))
    return round(max(gaps, default=0.0) / config.frame_width, 4)


def _gap(a: Rect, b: Rect) -> float:
    dx = max(0.0, a.left - b.right, b.left - a.right)
    dy = max(0.0, a.bottom - b.top, b.bottom - a.top)
    return float((dx * dx + dy * dy) ** 0.5)


def _drawn(mobject: Mobject, *, family: bool = True) -> list[VMobject]:
    """The leaves of `mobject` that put ink on screen (reserved parts are not drawn yet)."""

    leaves = mobject.family_members_with_points() if family else [mobject]
    return [
        leaf
        for leaf in leaves
        if isinstance(leaf, VMobject)
        and len(leaf.points) > 1
        and not _hidden(leaf)
        and _opacity(leaf) > 0
    ]


def _hidden(leaf: Mobject) -> bool:
    return getattr(leaf, "director_hidden", False)


def _reserved(mobject: Mobject) -> bool:
    """Laid out, but held back until show() reveals it: nothing of it is drawn yet."""

    leaves = mobject.family_members_with_points()
    return any(map(_hidden, leaves)) and not _drawn(mobject)


def _opacity(leaf: VMobject) -> float:
    stroke = leaf.get_stroke_opacity() if leaf.get_stroke_width() > 0 else 0.0
    return float(max(leaf.get_fill_opacity(), stroke))


def _lit(leaf: VMobject, base: dict[int, tuple[float, float]]) -> bool:
    """Closer to its full opacity than to the level focus dims it to."""

    if id(leaf) not in base:
        return True
    return _opacity(leaf) * 2 > max(base[id(leaf)]) * (1 + DIM_OPACITY)


def _leaf_colors(leaf: VMobject) -> list[str]:
    colors = []
    if leaf.get_fill_opacity() > 0:
        colors.append(leaf.get_fill_color().to_hex().upper())
    if leaf.get_stroke_width() > 0 and leaf.get_stroke_opacity() > 0:
        colors.append(leaf.get_stroke_color().to_hex().upper())
    return colors


def _text_units(mobject: Mobject) -> Iterator[Mobject]:
    if isinstance(mobject, Text | MarkupText | SingleStringMathTex | DecimalNumber) or hasattr(
        mobject, "authored_text"
    ):
        yield mobject
        return
    for sub in mobject.submobjects:
        yield from _text_units(sub)


def _frame_box(rect: Rect) -> list[float]:
    width, height = config.frame_width, config.frame_height
    return [
        round((rect.left + width / 2) / width, 4),
        round((height / 2 - rect.top) / height, 4),
        round((rect.right + width / 2) / width, 4),
        round((height / 2 - rect.bottom) / height, 4),
    ]
