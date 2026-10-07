"""Beat timeline (contract §1.4): recorded by DirectedScene during a bridge render.

The render handler activates a recorder for the duration of one render; scenes look it up
with `active()`. Under plain `manim` nothing is active and nothing is recorded.
"""

from __future__ import annotations

import json
from collections.abc import Iterator
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
class Timeline:
    version: int
    scene: str
    duration_seconds: float
    beats: list[TimelineBeat]

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
    _beats: list[_OpenBeat] = field(default_factory=list)

    def attach(self) -> None:
        """Called by a scene that records beats, so its render gets a timeline."""

        self.attached = True

    def enter(self, beat_id: str | None, file: str, line: int, at_seconds: float) -> int:
        resolved = beat_id or f"beat-{len(self._beats) + 1}"
        self._beats.append(
            _OpenBeat(resolved, at_seconds, public_path(file, self.project_root), line)
        )
        return len(self._beats) - 1

    def exit(self, handle: int, at_seconds: float) -> None:
        self._beats[handle].end_seconds = at_seconds

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
        return Timeline(TIMELINE_VERSION, scene, duration_seconds, beats)


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
        return Timeline(raw["version"], raw["scene"], raw["duration_seconds"], beats)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        raise invalid_source(path, f"it is not a beat timeline ({exc})") from exc
