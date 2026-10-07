from __future__ import annotations

import dataclasses
import json
import math
from collections.abc import Mapping
from enum import Enum
from pathlib import Path
from typing import Any

from .paths import atomic_target


def jsonable(value: Any) -> Any:
    """Convert result dataclasses to plain JSON values; non-finite floats become null."""

    if value is None or isinstance(value, (bool, int, str)):
        return value.value if isinstance(value, Enum) else value
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        return {f.name: jsonable(getattr(value, f.name)) for f in dataclasses.fields(value)}
    if isinstance(value, Mapping):
        return {str(key): jsonable(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [jsonable(item) for item in value]
    raise TypeError(f"{type(value).__name__} is not JSON serializable")


def dumps(value: Any) -> str:
    return json.dumps(jsonable(value), ensure_ascii=False, allow_nan=False, separators=(",", ":"))


def write_json(path: Path, value: Any) -> None:
    text = json.dumps(jsonable(value), ensure_ascii=False, allow_nan=False, indent=2) + "\n"
    with atomic_target(path) as temp:
        temp.write_text(text, encoding="utf-8")
