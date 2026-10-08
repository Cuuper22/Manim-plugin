"""Beat timeline (contract §1.4): recorded by DirectedScene during a bridge render.

The render handler activates a recorder for the duration of one render; scenes look it up
with `active()`. Under plain `manim` nothing is active and nothing is recorded.

Version 2 adds what pacing QA reads: every change the viewer sees (`events`), captions and
titles with their time on screen, prediction prompts (`asks`), where symbols are named
(`definitions`) and what the stage holds at each still (`settles`). The v1 fields keep their
shape, and a v1 file loads with these lists empty.
"""

from __future__ import annotations

import json
from collections.abc import Iterator, Mapping
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Any, Literal

from .errors import invalid_source
from .paths import public_path

TIMELINE_VERSION = 2


@dataclass(slots=True)
class TimelineBeat:
    id: str
    start_seconds: float
    end_seconds: float
    file: str
    line: int
    transition: str | None = None
    intent: str | None = None
    question: str | None = None
    takeaway: str | None = None
    aha: bool = False
    hold: float | None = None  # the still at its end
    hold_auto: bool = False  # chosen by the scene from the viewer's budgets (R7)


@dataclass(frozen=True, slots=True)
class Seen:
    """One thing that entered: what a viewer must find and take in."""

    kind: str
    label: str
    chunks: int
    read: float | None = None  # seconds a component declares; None: from words and glyphs
    words: int = 0
    glyphs: int = 0
    symbols: list[str] = field(default_factory=list)  # symbol-colored TeX tokens in it
    conventions: list[str] = field(default_factory=list)  # marks such as "right angle"


@dataclass(frozen=True, slots=True)
class Event:
    """One animation. `source` names the call that played it (transition, play, tracker,
    show, derive, focus, highlight); `file:line` is the statement in the scene."""

    at: float
    seconds: float
    beat: str | None
    source: str
    file: str
    line: int
    entered: list[Seen] = field(default_factory=list)
    targets: int = 0  # independent things animated at once
    spread: float = 0.0  # the widest gap between them, as a fraction of the frame width
    morph_glyphs: int = 0  # glyphs a text or math morph did not carry over
    of: str | None = None  # the one component that everything entering belongs to


@dataclass(slots=True)
class Lane:
    """A caption or title, from when it starts to appear until it starts to leave."""

    at: float
    until: float | None
    text: str
    words: int
    file: str
    line: int


@dataclass(frozen=True, slots=True)
class LaneText:
    """What a caption or title mobject says, and the statement that set it."""

    kind: Literal["caption", "title"]
    text: str
    file: str
    line: int


@dataclass(frozen=True, slots=True)
class Ask:
    at: float  # when the question is on screen
    hold: float
    text: str
    file: str
    line: int


@dataclass(frozen=True, slots=True)
class Definition:
    at: float
    symbol: str
    via: str  # label | annotate | legend | caption


@dataclass(frozen=True, slots=True)
class TextBox:
    label: str
    box: list[float]  # left, top, right, bottom as fractions of the frame from its top left


@dataclass(frozen=True, slots=True)
class Settle:
    """The stage during a wait: what the viewer has in front of them while nothing moves."""

    at: float
    beat: str | None
    still_seconds: float
    visible_chunks: int  # dimmed content excluded
    colors: list[str]  # theme tokens (or #RRGGBB) of lit content
    text_boxes: list[TextBox]


@dataclass(slots=True)
class Timeline:
    version: int
    scene: str
    duration_seconds: float
    beats: list[TimelineBeat]
    events: list[Event] = field(default_factory=list)
    captions: list[Lane] = field(default_factory=list)
    titles: list[Lane] = field(default_factory=list)
    asks: list[Ask] = field(default_factory=list)
    definitions: list[Definition] = field(default_factory=list)
    settles: list[Settle] = field(default_factory=list)

    def beat_at(self, seconds: float) -> TimelineBeat | None:
        """The innermost beat covering `seconds` (nested beats start later)."""

        covering = [b for b in self.beats if b.start_seconds <= seconds < b.end_seconds]
        return max(covering, key=lambda b: (b.start_seconds, -b.end_seconds), default=None)


@dataclass(slots=True)
class BeatRecorder:
    project_root: Path
    attached: bool = False
    # Seconds of video written so far, kept by the render handler: Manim's own clock also
    # counts skipped sections and runs off the frame grid on cached plays.
    now: float | None = None
    events: list[Event] = field(default_factory=list)
    captions: list[Lane] = field(default_factory=list)
    titles: list[Lane] = field(default_factory=list)
    asks: list[Ask] = field(default_factory=list)
    definitions: list[Definition] = field(default_factory=list)
    settles: list[Settle] = field(default_factory=list)
    _beats: list[tuple[TimelineBeat, bool]] = field(default_factory=list)  # (beat, exited)
    _lanes: dict[int, Lane] = field(default_factory=dict)  # on screen, by mobject id

    def attach(self) -> None:
        """Called by a scene that records beats, so its render gets a timeline."""

        self.attached = True

    def clock(self, renderer_seconds: float) -> float:
        return round(renderer_seconds if self.now is None else self.now, 6)

    def since(self, at: float, renderer_seconds: float) -> float:
        return round(self.clock(renderer_seconds) - at, 6)

    def where(self, file: str) -> str:
        return public_path(file, self.project_root)

    def enter(
        self, beat_id: str | None, file: str, line: int, at_seconds: float, **notes: Any
    ) -> int:
        """Open a beat; `notes` are the v2 beat fields (transition, intent, question, ...)."""

        resolved = beat_id or f"beat-{len(self._beats) + 1}"
        start = self.clock(at_seconds)
        self._beats.append(
            (TimelineBeat(resolved, start, start, self.where(file), line, **notes), False)
        )
        return len(self._beats) - 1

    def exit(self, handle: int, at_seconds: float, hold: float, *, auto: bool) -> None:
        beat, _ = self._beats[handle]
        beat.end_seconds = self.clock(at_seconds)
        beat.hold, beat.hold_auto = round(hold, 6), auto
        self._beats[handle] = (beat, True)

    def lanes(self, shown: Mapping[int, LaneText], at: float) -> None:
        """Open the captions and titles in `shown` (by mobject id) that were not on screen,
        and close the open ones that are gone; `at` is when the change began."""

        for key in [key for key in self._lanes if key not in shown]:
            self._lanes.pop(key).until = at
        for key, text in shown.items():
            if key not in self._lanes:
                words = len(text.text.split())
                lane = Lane(at, None, text.text, words, self.where(text.file), text.line)
                (self.titles if text.kind == "title" else self.captions).append(lane)
                self._lanes[key] = lane

    def timeline(self, scene: str, duration_seconds: float) -> Timeline:
        beats = [
            replace(beat) if exited else replace(beat, end_seconds=duration_seconds)
            for beat, exited in self._beats
        ]
        beats.sort(key=lambda b: b.start_seconds)
        if beats:
            # What plays after the last beat (a closing highlight, a final wait) continues its
            # stage, so frames there still map to a beat and a line.
            beats[-1].end_seconds = max(beats[-1].end_seconds, duration_seconds)

        def closed(lanes: list[Lane]) -> list[Lane]:
            return [
                replace(lane, until=duration_seconds if lane.until is None else lane.until)
                for lane in lanes
            ]

        return Timeline(
            TIMELINE_VERSION,
            scene,
            duration_seconds,
            beats,
            events=list(self.events),
            captions=closed(self.captions),
            titles=closed(self.titles),
            asks=list(self.asks),
            definitions=list(self.definitions),
            settles=list(self.settles),
        )


_ACTIVE: ContextVar[BeatRecorder | None] = ContextVar("manim_director_beat_recorder", default=None)


def active() -> BeatRecorder | None:
    return _ACTIVE.get()


@contextmanager
def recording(project_root: Path) -> Iterator[BeatRecorder]:
    recorder = BeatRecorder(project_root)
    token = _ACTIVE.set(recorder)
    try:
        yield recorder
    finally:
        _ACTIVE.reset(token)


def load(path: Path) -> Timeline:
    """A v1 or v2 timeline; a v1 file has no pacing records."""

    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
        return Timeline(
            raw["version"],
            raw["scene"],
            raw["duration_seconds"],
            [TimelineBeat(**beat) for beat in raw["beats"]],
            events=[
                Event(**{**event, "entered": [Seen(**seen) for seen in event["entered"]]})
                for event in raw.get("events", [])
            ],
            captions=[Lane(**lane) for lane in raw.get("captions", [])],
            titles=[Lane(**lane) for lane in raw.get("titles", [])],
            asks=[Ask(**ask) for ask in raw.get("asks", [])],
            definitions=[Definition(**d) for d in raw.get("definitions", [])],
            settles=[
                Settle(**{**settle, "text_boxes": [TextBox(**t) for t in settle["text_boxes"]]})
                for settle in raw.get("settles", [])
            ],
        )
    except (OSError, ValueError, KeyError, TypeError) as exc:
        raise invalid_source(path, f"it is not a beat timeline ({exc})") from exc
