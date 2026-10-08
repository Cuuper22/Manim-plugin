"""Beat timeline (contract §1.4): recorded by DirectedScene during a bridge render.

The render handler activates a recorder for the duration of one render; scenes look it up
with `active()`. Under plain `manim` nothing is active and nothing is recorded.
"""

from __future__ import annotations

import json
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import dataclass, field
from pathlib import Path

from .errors import invalid_source
from .paths import public_path

TIMELINE_VERSION = 1


@dataclass(slots=True)
class TimelineBeat:
    id: str
    start_seconds: float
    end_seconds: float
    file: str
    line: int


@dataclass(slots=True)
class Span:
    start_seconds: float
    end_seconds: float


@dataclass(slots=True)
class Timeline:
    version: int
    scene: str
    duration_seconds: float
    beats: list[TimelineBeat]
    transitions: list[Span] = field(default_factory=list)
    """When the stage is between two states (beat transitions, derivation steps, focus,
    camera moves), in order and disjoint: frames there are not content to judge."""

    def beat_at(self, seconds: float) -> TimelineBeat | None:
        """The innermost beat covering `seconds` (nested beats start later)."""

        covering = [b for b in self.beats if b.start_seconds <= seconds < b.end_seconds]
        return max(covering, key=lambda b: (b.start_seconds, -b.end_seconds), default=None)


@dataclass(slots=True)
class _OpenBeat:
    id: str
    start_seconds: float
    file: str
    line: int
    end_seconds: float | None = None


@dataclass(slots=True)
class BeatRecorder:
    project_root: Path
    attached: bool = False
    # Seconds of video written so far, kept by the render handler: Manim's own clock also
    # counts skipped sections and runs off the frame grid on cached plays.
    now: float | None = None
    _beats: list[_OpenBeat] = field(default_factory=list)
    _transitions: list[Span] = field(default_factory=list)

    def attach(self) -> None:
        """Called by a scene that records beats, so its render gets a timeline."""

        self.attached = True

    def enter(self, beat_id: str | None, file: str, line: int, at_seconds: float) -> int:
        resolved = beat_id or f"beat-{len(self._beats) + 1}"
        start = self._time(at_seconds)
        self._beats.append(_OpenBeat(resolved, start, public_path(file, self.project_root), line))
        return len(self._beats) - 1

    def exit(self, handle: int, at_seconds: float) -> None:
        self._beats[handle].end_seconds = self._time(at_seconds)

    @contextmanager
    def transition(self, clock: Callable[[], float]) -> Iterator[None]:
        """Record what plays inside as a transition; `clock` reads Manim's time."""

        start = self._time(clock())
        yield
        end = self._time(clock())
        if end <= start:
            return
        if self._transitions and start <= self._transitions[-1].end_seconds:
            self._transitions[-1].end_seconds = end  # back to back: one transition
        else:
            self._transitions.append(Span(start, end))

    def _time(self, at_seconds: float) -> float:
        return at_seconds if self.now is None else self.now

    def timeline(self, scene: str, duration_seconds: float) -> Timeline:
        beats = [
            TimelineBeat(
                id=b.id,
                start_seconds=b.start_seconds,
                end_seconds=duration_seconds if b.end_seconds is None else b.end_seconds,
                file=b.file,
                line=b.line,
            )
            for b in self._beats
        ]
        beats.sort(key=lambda b: b.start_seconds)
        if beats:
            # What plays after the last beat (a closing highlight, a final wait) continues its
            # stage, so frames there still map to a beat and a line.
            beats[-1].end_seconds = max(beats[-1].end_seconds, duration_seconds)
        transitions = [Span(span.start_seconds, span.end_seconds) for span in self._transitions]
        return Timeline(TIMELINE_VERSION, scene, duration_seconds, beats, transitions)


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
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
        beats = [TimelineBeat(**beat) for beat in raw["beats"]]
        transitions = [Span(**span) for span in raw.get("transitions", [])]
        return Timeline(raw["version"], raw["scene"], raw["duration_seconds"], beats, transitions)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        raise invalid_source(path, f"it is not a beat timeline ({exc})") from exc
