"""Pacing QA: a v2 beat timeline judged against the viewer's reading and attention budgets.

A viewer sees only frames and captions and cannot pause to ask. Each rule below names a way a
film outruns them: text that leaves before it is read, a reveal followed at once by the next
change, more new things than working memory holds, notation used before it is named.

Every number comes from `data/budgets.json`, in the column of the viewer's level
(`brief.viewer.level`), overridden key by key by `qa.pacing`. `check` is pure.
"""

from __future__ import annotations

import json
import math
import re
import unicodedata
from collections import Counter
from collections.abc import Callable, Iterator, Mapping, Sequence
from contextlib import suppress
from dataclasses import dataclass, replace
from functools import cache
from importlib import resources
from itertools import combinations
from typing import Any

from .errors import CompositionError
from .model import Finding, Severity, SourceLocation
from .project import StoryBeat, Viewer
from .timeline import Event, Settle, TextBox, Timeline, TimelineBeat

_WARNING, _INFO = Severity.WARNING, Severity.INFO

CODES: dict[str, tuple[Severity, str]] = {
    "caption_too_fast": (
        _WARNING,
        "Hold the beat longer (hold= or self.wait), or shorten or split the caption.",
    ),
    "short_hold": (
        _WARNING,
        "Add self.wait(...) before the next change, or raise the beat's hold= or derive's pause=.",
    ),
    "question_hold_short": (
        _WARNING,
        "Keep everything still while the viewer predicts: leave ask's hold at its default.",
    ),
    "rushed_step": (_WARNING, "Raise run_time= or pause=; give the aha its own slow motion."),
    "crowded_beat": (_WARNING, "Split the beat, or reveal its parts across beats."),
    "crowded_moment": (
        _WARNING,
        "Play them one after the other; use link() for a deliberate pair.",
    ),
    "text_overlap": (_WARNING, "Place them in one call, or in different regions."),
    "long_text": (_INFO, "Split it across beats; let labels on the picture do the naming."),
    "long_beat": (_INFO, "Split it: one takeaway per beat."),
    "unsignaled_reveal": (
        _INFO,
        "Caption it, or focus, highlight or annotate the one that matters.",
    ),
    "too_dense": (
        _INFO,
        "Let old objects leave (drop them from keep=), or focus the one that matters.",
    ),
    "palette_overload": (_INFO, "Give fewer things their own color; context can be muted."),
    "unexplained_notation": (
        _INFO,
        "Name it once where it appears: a label on the picture, a caption or a note.",
    ),
    "viewer_plan": (_INFO, "Fill brief.viewer and the storyboard (see the skill's template)."),
}

SIGNALS = frozenset({"focus", "highlight"})  # direct the eye without adding anything to read
_POINTERS = SIGNALS | {"annotate", "link", "ask"}  # sources that tell the viewer where to look
_RESULT_INTENTS = frozenset({"reveal", "prove", "recap"})
_NEUTRAL_COLORS = frozenset({"foreground", "muted", "background"})
_LISTED = 3  # labels a message names before "and N more"
_CONVENTION_WORDS = {
    "right angle": ("right angle", "90", "perpendicular", "⊥"),
    "equal length": ("equal", "same length", "congruent"),
}


@dataclass(frozen=True, slots=True)
class Settings:
    """One column of `budgets.json`; see its `about` entries."""

    words_per_second: float
    caption_min_seconds: float
    read_base_text: float
    read_base_math: float
    read_per_term_group: float
    glyphs_per_term_group: int
    morph_base_share: float
    read_per_shape: float
    read_shapes_max: float
    read_max: float
    settle_min: float
    beat_end_min: float
    result_end_min: float
    chain_seconds: float
    ask_hold: float
    final_hold: float
    motion_min: float
    step_pause_min: float
    aha_motion_min: float
    rush_glyphs: int
    rush_seconds: float
    max_new_per_beat: int
    max_targets_per_motion: int
    split_fraction: float
    signal_min_chunks: int
    max_visible_chunks: int
    max_colors: int
    caption_max_words: int
    title_max_words: int
    beat_max_seconds: float
    overlap_fraction: float
    plan_min_seconds: float
    frame_slack_seconds: float


@cache
def budgets() -> dict[str, Any]:
    source = resources.files(__package__).joinpath("data", "budgets.json")
    return json.loads(source.read_text(encoding="utf-8"))


def settings(level: str = "general", overrides: Mapping[str, float] | None = None) -> Settings:
    """The budgets for a viewer `level`, with `qa.pacing` overrides applied."""

    data = budgets()
    if level not in data["levels"]:
        raise CompositionError(
            f"brief.viewer.level is {level!r}; use one of {', '.join(data['levels'])}.",
            level=level,
        )
    values = {key: entry[level] for key, entry in data["budgets"].items()}
    unknown = sorted(set(overrides or {}) - set(values))
    if unknown:
        raise CompositionError(
            f"qa.pacing has unknown budgets {', '.join(unknown)}; "
            f"the budgets are {', '.join(values)}.",
            unknown=unknown,
        )
    return Settings(**{**values, **(overrides or {})})


def reading_seconds(event: Event, s: Settings) -> float:
    """How long a viewer needs to take in what `event` brought in."""

    total, shapes = 0.0, 0
    for seen in event.entered:
        if not seen.chunks:  # a morph's target: its changed glyphs count below
            continue
        part = (seen.read or 0.0) + _text_read(seen.words, s) + _math_read(seen.glyphs, s)
        if part:
            total += part
        else:
            shapes += 1
    total += min(shapes * s.read_per_shape, s.read_shapes_max)
    total += _math_read(event.morph_glyphs, s, s.read_base_math * s.morph_base_share)
    return min(total, s.read_max)


def check(
    timeline: Timeline,
    s: Settings,
    viewer: Viewer | None = None,
    storyboard: Sequence[StoryBeat] = (),
) -> list[Finding]:
    """Pacing findings for a v2 timeline, earliest first; a v1 timeline has none."""

    if timeline.version < 2:
        return []
    film = _Film(timeline, s, viewer, tuple(storyboard))
    findings = [finding for rule in _RULES for finding in rule(film)]
    findings.sort(key=lambda f: -1.0 if f.at_seconds is None else f.at_seconds)
    return _once_per_statement(findings)


def _once_per_statement(findings: list[Finding]) -> list[Finding]:
    """A loop repeats its finding at every turn: keep the first, counting the rest."""

    first: dict[tuple[str, str | None, SourceLocation], int] = {}
    repeats: Counter[int] = Counter()
    kept: list[Finding] = []
    for finding in findings:
        key = (finding.code, finding.beat, finding.location)
        if finding.location is not None and key in first:
            repeats[first[key]] += 1
            continue
        if finding.location is not None:
            first[key] = len(kept)
        kept.append(finding)
    for index, more in repeats.items():
        times = "time" if more == 1 else "times"
        message = f"{kept[index].message} The same line does it {more} more {times}."
        kept[index] = replace(kept[index], message=message)
    return kept


@dataclass(frozen=True, slots=True)
class PlannedBeat:
    """A played beat with what its storyboard entry adds."""

    beat: TimelineBeat
    question: str | None
    takeaway: str | None
    aha: bool
    result: bool  # its point must land: a longer still at its end


def planned(timeline: Timeline, storyboard: Sequence[StoryBeat] = ()) -> list[PlannedBeat]:
    """The played beats in order, each merged with its storyboard entry; the beat's own
    `question=`, `takeaway=`, `intent=` and `aha=` win."""

    story = {beat.id: beat for beat in storyboard}
    merged = []
    for beat in sorted(timeline.beats, key=lambda b: b.start_seconds):
        plan = story.get(beat.id, StoryBeat(beat.id))
        aha = beat.aha or plan.aha
        merged.append(
            PlannedBeat(
                beat=beat,
                question=beat.question or plan.question,
                takeaway=beat.takeaway or plan.takeaway,
                aha=aha,
                result=aha or (beat.intent or plan.intent) in _RESULT_INTENTS,
            )
        )
    return merged


class _Film:
    def __init__(
        self, t: Timeline, s: Settings, viewer: Viewer | None, storyboard: tuple[StoryBeat, ...]
    ) -> None:
        self.t, self.s, self.viewer, self.storyboard = t, s, viewer, storyboard
        self.events = sorted(t.events, key=lambda e: e.at)
        self.beats = {plan.beat.id: plan for plan in planned(t, storyboard)}

    def in_beat(self, beat_id: str) -> list[Event]:
        return [e for e in self.events if e.beat == beat_id]

    def latest(self, at: float) -> Event | None:
        """The last change that started by `at`."""

        return next((e for e in reversed(self.events) if e.at <= at), None)


def _caption_pace(film: _Film) -> Iterator[Finding]:
    s = film.s
    for lane in film.t.captions:
        shown = (lane.until if lane.until is not None else film.t.duration_seconds) - lane.at
        need = max(s.caption_min_seconds, _text_read(lane.words, s))
        if shown + s.frame_slack_seconds < need:
            yield _finding(
                "caption_too_fast",
                f"The caption {_quote(lane.text)} is on screen for {shown:.1f} s; "
                f"{lane.words} words need {need:.1f} s.",
                film,
                at=lane.at,
                where=(lane.file, lane.line),
            )


def _long_text(film: _Film) -> Iterator[Finding]:
    s = film.s
    for kind, lanes, limit in (
        ("caption", film.t.captions, s.caption_max_words),
        ("title", film.t.titles, s.title_max_words),
    ):
        for lane in lanes:
            if lane.words > limit:
                yield _finding(
                    "long_text",
                    f"The {kind} {_quote(lane.text)} has {lane.words} words; "
                    f"a {kind} reads at a glance up to {limit}.",
                    film,
                    at=lane.at,
                    where=(lane.file, lane.line),
                )


def _short_holds(film: _Film) -> Iterator[Finding]:
    """A reveal needs a still to be taken in before the next change: its reading time
    mid-beat, and more at a beat's end. Focus and highlight do not end a still."""

    s = film.s
    changes = [e for e in film.events if e.source not in SIGNALS]
    i = 0
    while i < len(changes):
        chain = [changes[i]]
        while i + 1 < len(changes) and _chained(chain[-1], changes[i + 1], s):
            i += 1
            chain.append(changes[i])
        first, last = chain[0], chain[-1]
        following = changes[i + 1] if i + 1 < len(changes) else None
        i += 1
        read = min(sum(reading_seconds(e, s) for e in chain), s.read_max)
        beat_end = following is None or following.beat != last.beat
        if not (read or beat_end):
            continue
        still = max(
            (film.t.duration_seconds if following is None else following.at) - _end(last), 0
        )
        if beat_end:
            beat = film.beats.get(last.beat or "")
            floor = s.result_end_min if beat is not None and beat.result else s.beat_end_min
            need = max(floor, read)
        else:
            need = min(max(read, s.settle_min), s.read_max)
        if still + s.frame_slack_seconds >= need:
            continue
        subject = f"{_what(chain)} (in at {first.at:.1f} s)" if read else "The last change"
        if beat_end:
            then = f"beat {last.beat!r} ends" if last.beat else "the video ends"
            needs = "the end of a beat needs"
        else:
            then, needs = f"line {following.line} moves on", "taking it in needs"
        yield _finding(
            "short_hold",
            f"{subject} is still for {still:.1f} s before {then}; {needs} {need:.1f} s.",
            film,
            event=first,
        )


def _chained(previous: Event, event: Event, s: Settings) -> bool:
    """Back-to-back short reveals of one component read as one reveal."""

    return (
        previous.of is not None
        and event.of == previous.of
        and event.beat == previous.beat
        and max(previous.seconds, event.seconds) <= s.chain_seconds
        and event.at - _end(previous) <= s.frame_slack_seconds
    )


def _question_holds(film: _Film) -> Iterator[Finding]:
    s = film.s
    for ask in film.t.asks:
        following = next((e for e in film.events if e.at >= ask.at - s.frame_slack_seconds), None)
        still = (film.t.duration_seconds if following is None else following.at) - ask.at
        if still + s.frame_slack_seconds < s.ask_hold:
            yield _finding(
                "question_hold_short",
                f"The question {_quote(ask.text)} gets {still:.1f} s before the stage changes; "
                f"a prediction needs {s.ask_hold:.1f} s.",
                film,
                at=ask.at,
                where=(ask.file, ask.line),
            )


def _rushed_steps(film: _Film) -> Iterator[Finding]:
    s = film.s
    previous: Event | None = None
    for event in film.events:
        morph = event.source == "derive" or event.morph_glyphs > 0
        reason = None
        if morph and event.seconds + s.frame_slack_seconds < s.motion_min:
            what = "derive step" if event.source == "derive" else "morph"
            reason = f"This {what} runs {event.seconds:.1f} s; the eye needs {s.motion_min:.1f} s."
        elif event.morph_glyphs > s.rush_glyphs and event.seconds < s.rush_seconds:
            reason = (
                f"{event.morph_glyphs} glyphs change in {event.seconds:.1f} s; more than "
                f"{s.rush_glyphs} need {s.rush_seconds:.1f} s."
            )
        elif (
            previous is not None
            and event.source == previous.source == "derive"
            and (event.file, event.line) == (previous.file, previous.line)
            and event.at - _end(previous) + s.frame_slack_seconds < s.step_pause_min
            and not reading_seconds(previous, s)  # else short_hold judges the pause
        ):
            pause = event.at - _end(previous)
            reason = (
                f"Derive steps follow each other after {pause:.1f} s; "
                f"a step needs {s.step_pause_min:.1f} s to sink in."
            )
        if reason is not None:
            yield _finding("rushed_step", reason, film, event=event)
        previous = event
    for beat in film.beats.values():
        if not beat.aha:
            continue
        motions = film.in_beat(beat.beat.id)
        longest = max(motions, key=lambda e: e.seconds, default=None)
        if longest is None or longest.seconds + s.frame_slack_seconds < s.aha_motion_min:
            seconds = 0.0 if longest is None else longest.seconds
            yield _finding(
                "rushed_step",
                f"The aha beat {beat.beat.id!r} moves for at most {seconds:.1f} s; the change "
                f"the viewer should get needs one motion of {s.aha_motion_min:.1f} s or more.",
                film,
                event=longest,
                beat=beat.beat,
            )


def _crowded_beats(film: _Film) -> Iterator[Finding]:
    s = film.s
    for beat in film.beats.values():
        total, names = 0, []
        for event in film.in_beat(beat.beat.id):
            total += sum(seen.chunks for seen in event.entered)
            names += [seen.label for seen in event.entered if seen.chunks]
            if total > s.max_new_per_beat:
                yield _finding(
                    "crowded_beat",
                    f"Beat {beat.beat.id!r} brings in {total} new things by here "
                    f"({_list(names)}); a viewer takes in {s.max_new_per_beat} per beat.",
                    film,
                    event=event,
                )
                break


def _crowded_moments(film: _Film) -> Iterator[Finding]:
    s = film.s
    for event in film.events:
        split = event.targets > 1 and event.spread > s.split_fraction
        if event.targets > s.max_targets_per_motion or split:
            where = f", {event.spread:.0%} of the frame apart" if split else ""
            yield _finding(
                "crowded_moment",
                f"One animation moves {event.targets} things at once{where}; "
                "the eye follows one motion at a time.",
                film,
                event=event,
            )


def _text_overlaps(film: _Film) -> Iterator[Finding]:
    s, reported = film.s, set()
    for settle in _settled(film):
        for a, b in combinations(settle.text_boxes, 2):
            key = (settle.beat, a.label, b.label)
            if key in reported or _overlap_share(a, b) <= s.overlap_fraction:
                continue
            reported.add(key)
            yield _finding(
                "text_overlap",
                f"{_quote(a.label)} and {_quote(b.label)} overlap by "
                f"{_overlap_share(a, b):.0%} of the smaller one.",
                film,
                event=film.latest(settle.at),
                at=settle.at,
            )


def _long_beats(film: _Film) -> Iterator[Finding]:
    for beat in film.beats.values():
        seconds = beat.beat.end_seconds - beat.beat.start_seconds
        if seconds > film.s.beat_max_seconds:
            yield _finding(
                "long_beat",
                f"Beat {beat.beat.id!r} lasts {seconds:.1f} s; past "
                f"{film.s.beat_max_seconds:g} s a beat usually carries two takeaways.",
                film,
                beat=beat.beat,
            )


def _unsignaled_reveals(film: _Film) -> Iterator[Finding]:
    s = film.s
    for beat in film.beats.values():
        start, end = beat.beat.start_seconds, beat.beat.end_seconds
        events = film.in_beat(beat.beat.id)
        chunks = sum(seen.chunks for event in events for seen in event.entered)
        signaled = (
            any(event.source in _POINTERS for event in events)
            or any(start <= lane.at < end for lane in film.t.captions)
            or any(start <= ask.at < end for ask in film.t.asks)
            or any(start <= d.at < end and d.via != "label" for d in film.t.definitions)
        )
        if chunks >= s.signal_min_chunks and not signaled:
            first = next(event for event in events if event.entered)
            yield _finding(
                "unsignaled_reveal",
                f"Beat {beat.beat.id!r} brings in {chunks} things and nothing says which one "
                "to look at.",
                film,
                event=first,
            )


def _density(film: _Film) -> Iterator[Finding]:
    s = film.s
    dense: set[str | None] = set()
    colorful: set[str | None] = set()
    for settle in _settled(film):
        event = film.latest(settle.at)
        if settle.visible_chunks > s.max_visible_chunks and settle.beat not in dense:
            dense.add(settle.beat)
            yield _finding(
                "too_dense",
                f"{settle.visible_chunks} things are lit at once; a viewer tracks "
                f"{s.max_visible_chunks}.",
                film,
                event=event,
                at=settle.at,
            )
        colors = [c for c in settle.colors if c not in _NEUTRAL_COLORS]
        if len(colors) > s.max_colors and settle.beat not in colorful:
            colorful.add(settle.beat)
            yield _finding(
                "palette_overload",
                f"{len(colors)} colors carry meaning at once ({', '.join(colors)}); "
                f"keep it to {s.max_colors}.",
                film,
                event=event,
                at=settle.at,
            )


def _unexplained_notation(film: _Film) -> Iterator[Finding]:
    known = {
        _compact(token)
        for item in (film.viewer.knows if film.viewer is not None else ())
        for token in re.findall(r"\$([^$]+)\$", item)
    }
    named: dict[str, float] = {}  # symbol or convention -> first time it is named
    for definition in film.t.definitions:
        named[definition.symbol] = min(named.get(definition.symbol, math.inf), definition.at)
    for event in film.events:
        for seen in event.entered:
            for symbol in seen.symbols:
                if _compact(seen.label.replace("$", "")) == _compact(symbol):
                    named.setdefault(symbol, event.at)  # a label that is the symbol names it
    reported: set[str] = set()
    for event in film.events:
        until = _end(event)
        for seen in event.entered:
            marks = [(symbol, _symbol_names(symbol)) for symbol in seen.symbols]
            marks += [(c, _CONVENTION_WORDS.get(c, (c,))) for c in seen.conventions]
            for mark, words in marks:
                if mark in reported or _compact(mark) in known:
                    continue
                said = any(
                    lane.at <= until and _mentions(lane.text, words) for lane in film.t.captions
                )
                if said or named.get(mark, math.inf) <= until:
                    continue
                reported.add(mark)
                yield _finding(
                    "unexplained_notation",
                    f"{_quote(mark)} appears (in {_quote(seen.label)}) before anything on "
                    "screen says what it means.",
                    film,
                    event=event,
                )


def _viewer_plan(film: _Film) -> Iterator[Finding]:
    s, viewer = film.s, film.viewer
    if viewer is None:
        yield _finding(
            "viewer_plan",
            "director.yaml has no brief.viewer: who watches, what they know, their question, "
            "their wrong guess and the aha.",
            film,
        )
    elif viewer.missing:
        yield _finding(
            "viewer_plan",
            f"brief.viewer does not say {', '.join(viewer.missing)}.",
            film,
        )
    played = list(film.beats)
    planned = [beat.id for beat in film.storyboard]
    if planned and planned != played:
        yield _finding(
            "viewer_plan",
            f"The storyboard plans {_list(planned)} but the scene plays {_list(played)}.",
            film,
        )
    vague = [b for b in film.beats.values() if not (b.question and b.takeaway)]
    if vague:
        yield _finding(
            "viewer_plan",
            f"Beats without an audience question or takeaway: {_list([b.beat.id for b in vague])}.",
            film,
            beat=vague[0].beat,
        )
    if film.t.duration_seconds > s.plan_min_seconds:
        if not film.t.asks:
            yield _finding(
                "viewer_plan",
                "The film never asks the viewer to predict (self.ask) before its aha.",
                film,
            )
        ahas = [b.beat.id for b in film.beats.values() if b.aha]
        if len(ahas) != 1:
            which = _list(ahas) if ahas else "none"
            yield _finding(
                "viewer_plan",
                f"A film needs exactly one aha beat (beat(aha=True)); it has {which}.",
                film,
            )


_RULES: tuple[Callable[[_Film], Iterator[Finding]], ...] = (
    _caption_pace,
    _long_text,
    _short_holds,
    _question_holds,
    _rushed_steps,
    _crowded_beats,
    _crowded_moments,
    _text_overlaps,
    _long_beats,
    _unsignaled_reveals,
    _density,
    _unexplained_notation,
    _viewer_plan,
)


def _finding(
    code: str,
    message: str,
    film: _Film,
    *,
    event: Event | None = None,
    beat: TimelineBeat | None = None,
    at: float | None = None,
    where: tuple[str, int] | None = None,
) -> Finding:
    """A finding at `event` (its statement and start), else at `beat` (its header)."""

    severity, hint = CODES[code]
    if event is not None:
        at = event.at if at is None else at
        where = where or (event.file, event.line)
    if beat is not None:
        at = beat.start_seconds if at is None else at
        where = where or (beat.file, beat.line)
    beat_id = beat.id if beat is not None else event.beat if event is not None else None
    if beat_id is None and at is not None:
        covering = film.t.beat_at(at)
        beat_id = covering.id if covering is not None else None
    return Finding(
        code=code,
        severity=severity,
        message=message,
        hint=hint,
        location=SourceLocation(*where) if where and where[0] else None,
        at_seconds=at,
        beat=beat_id,
    )


def _end(event: Event) -> float:
    return event.at + event.seconds


def _text_read(words: int, s: Settings) -> float:
    return s.read_base_text + words / s.words_per_second if words else 0.0


def _math_read(glyphs: int, s: Settings, base: float | None = None) -> float:
    if not glyphs:
        return 0.0
    groups = math.ceil(glyphs / s.glyphs_per_term_group)
    return (s.read_base_math if base is None else base) + s.read_per_term_group * groups


def _settled(film: _Film) -> Iterator[Settle]:
    """Stills long enough for the viewer to notice what is on screen."""

    for settle in film.t.settles:
        if settle.still_seconds + film.s.frame_slack_seconds >= film.s.settle_min:
            yield settle


def _overlap_share(a: TextBox, b: TextBox) -> float:
    (al, at, ar, ab), (bl, bt, br, bb) = a.box, b.box
    width, height = min(ar, br) - max(al, bl), min(ab, bb) - max(at, bt)
    smaller = min((ar - al) * (ab - at), (br - bl) * (bb - bt))
    return width * height / smaller if width > 0 and height > 0 and smaller > 0 else 0.0


def _what(events: Sequence[Event]) -> str:
    labels = [seen.label for event in events for seen in event.entered]
    if labels:
        return _list(labels)
    return "The derive step" if events[0].source == "derive" else "The morph"


def _list(items: Sequence[str]) -> str:
    quoted = [
        _quote(item) if count == 1 else f"{count} × {_quote(item)}"
        for item, count in Counter(items).items()
    ]
    shown = ", ".join(quoted[:_LISTED])
    more = len(quoted) - _LISTED
    return shown + (f" and {more} more" if more > 0 else "")


def _quote(text: str) -> str:
    return f"'{text}'"


def _compact(text: str) -> str:
    return re.sub(r"\s+", "", text).replace("−", "-")


def _symbol_names(symbol: str) -> tuple[str, ...]:
    """How a caption can name a TeX symbol: as written (`\\lambda` as lambda) or in Unicode."""

    plain = symbol.replace("\\", "").strip()
    names = [plain]
    if plain.isalpha():
        case = "CAPITAL" if plain[0].isupper() else "SMALL"
        letter = plain.upper().replace("LAMBDA", "LAMDA")  # Unicode's spelling
        with suppress(KeyError):
            names.append(unicodedata.lookup(f"GREEK {case} LETTER {letter}"))
    return tuple(names)


def _mentions(text: str, words: Sequence[str]) -> bool:
    compact = _compact(text)
    for word in words:
        if word.isalpha():  # a whole word; one letter keeps its case (n is not N)
            flags = re.IGNORECASE if len(word) > 1 else 0
            if re.search(rf"(?<![^\W\d_]){re.escape(word)}(?![^\W\d_])", text, flags):
                return True
        elif _compact(word) in compact:
            return True
    return False
