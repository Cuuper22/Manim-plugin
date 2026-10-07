"""Result types shared by several operations (contract §1.1)."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class Severity(StrEnum):
    ERROR = "error"
    WARNING = "warning"
    INFO = "info"


class ArtifactKind(StrEnum):
    VIDEO = "video"
    SECTION = "section"
    IMAGE = "image"
    CONTACT_SHEET = "contact_sheet"
    CAPTIONS = "captions"
    TIMELINE = "timeline"
    ARCHIVE = "archive"
    FILE = "file"


@dataclass(frozen=True, slots=True)
class SourceLocation:
    file: str
    line: int
    column: int | None = None


@dataclass(frozen=True, slots=True)
class Finding:
    code: str
    severity: Severity
    message: str
    hint: str | None = None
    location: SourceLocation | None = None
    at_seconds: float | None = None
    beat: str | None = None
    frame: str | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "message", _clip(self.message, 500))
        if self.hint is not None:
            object.__setattr__(self, "hint", _clip(self.hint, 300))


@dataclass(frozen=True, slots=True)
class SceneRef:
    name: str
    file: str


@dataclass(frozen=True, slots=True)
class RuntimeArtifact:
    kind: ArtifactKind
    path: str
    label: str | None = None


def _clip(text: str, limit: int) -> str:
    return text if len(text) <= limit else text[: limit - 1].rstrip() + "…"
