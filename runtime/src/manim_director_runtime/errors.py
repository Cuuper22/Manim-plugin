from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(slots=True)
class DirectorError(Exception):
    """A stable, serializable error crossing the JSONL boundary."""

    code: str
    message: str
    data: dict[str, Any] = field(default_factory=dict)

    def __str__(self) -> str:
        return self.message

    def as_dict(self) -> dict[str, Any]:
        value: dict[str, Any] = {"code": self.code, "message": self.message}
        if self.data:
            value["data"] = self.data
        return value
