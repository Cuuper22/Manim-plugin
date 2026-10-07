"""Beats: named, timed stage changes. Pure data; DirectedScene performs them."""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import StrEnum
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from .scene import Directed


class Transition(StrEnum):
    CONTINUE = "continue"  # carried objects glide or morph; everything else fades out
    CONTRAST = "contrast"  # the old slides out as the new slides in
    REVEAL = "reveal"  # new objects are drawn
    CHAPTER = "chapter"  # the stage clears, title and caption included


class Intent(StrEnum):
    INTRODUCE = "introduce"
    EXPLAIN = "explain"
    COMPARE = "compare"
    REVEAL = "reveal"
    PROVE = "prove"
    RECAP = "recap"


@dataclass(eq=False)
class Beat:
    """Returned by `DirectedScene.beat()`; use it as `with self.beat("hook"):`."""

    id: str | None
    transition: Transition
    focus: Any
    keep: tuple[Any, ...]
    hold: float
    run_time: float | None
    intent: Intent | None = None
    question: str | None = None
    takeaway: str | None = None
    scene: Directed | None = field(default=None, repr=False)
    file: str = field(default="", repr=False)  # where `with self.beat(...)` is written
    line: int = field(default=0, repr=False)
    # Set while the beat runs.
    before: list[Any] = field(default_factory=list, repr=False)
    carried: set[int] = field(default_factory=set, repr=False)
    transitioned: bool = field(default=False, repr=False)
    focused: bool = field(default=False, repr=False)
    record: int | None = field(default=None, repr=False)

    def __enter__(self) -> Beat:
        assert self.scene is not None
        self.scene._enter_beat(self)
        return self

    def __exit__(self, exc_type: type[BaseException] | None, *_: object) -> None:
        assert self.scene is not None
        self.scene._exit_beat(self, completed=exc_type is None)
