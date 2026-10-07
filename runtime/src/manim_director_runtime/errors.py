from __future__ import annotations

from pathlib import Path
from typing import Any


class DirectorError(Exception):
    """An error with a stable code that crosses the bridge as an `error` frame."""

    def __init__(self, code: str, message: str, data: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.data = data

    def as_dict(self) -> dict[str, Any]:
        return {"code": self.code, "message": self.message, "data": self.data}


def invalid_params(
    field: str | None, reason: str, *, allowed: list[str] | None = None
) -> DirectorError:
    data: dict[str, Any] = {"field": field, "reason": reason}
    if allowed is not None:
        data["allowed"] = allowed
    where = f" {field}" if field else ""
    return DirectorError("invalid_params", f"Invalid task field{where}: {reason}.", data)


def io_error(path: Path, error: OSError) -> DirectorError:
    detail = error.strerror or str(error)
    return DirectorError(
        "io_error", f"Cannot access {path}: {detail}.", {"path": str(path), "error": detail}
    )


def dependency_missing(dependency: str, hint: str) -> DirectorError:
    return DirectorError(
        "dependency_missing",
        f"{dependency} is not available in the runtime environment.",
        {"dependency": dependency, "hint": hint},
    )
