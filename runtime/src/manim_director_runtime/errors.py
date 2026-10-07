from __future__ import annotations

from enum import StrEnum
from pathlib import Path
from typing import Any, TypeVar

Choice = TypeVar("Choice", bound=StrEnum)


class DirectorError(Exception):
    """An error with a stable code that crosses the bridge as an `error` frame."""

    def __init__(self, code: str, message: str, data: dict[str, Any] | None = None) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.data = data

    def as_dict(self) -> dict[str, Any]:
        return {"code": self.code, "message": self.message, "data": self.data}


class CompositionError(DirectorError):
    """An authoring call that cannot be honored: layout, beats, themes or TeX terms."""

    def __init__(self, message: str, **data: Any) -> None:
        super().__init__("composition", message, data or None)


def parse_choice(kind: type[Choice], value: object) -> Choice:
    try:
        return kind(value)
    except ValueError:
        choices = [member.value for member in kind]
        raise CompositionError(
            f"Unknown {kind.__name__.lower()} {value!r}; choose one of {', '.join(choices)}.",
            value=str(value),
            choices=choices,
        ) from None


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
