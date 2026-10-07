"""Project settings a DirectedScene reads from the nearest `director.yaml` (contract §1.9).

The lookup is identical under the bridge and under plain `manim`: walk up from the scene
file's directory and read the first `director.yaml` found.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

from .errors import CompositionError
from .tasks import SAFE_AREA_DEFAULT, InvalidField, SafeArea

SPEC_NAME = "director.yaml"


@dataclass(frozen=True, slots=True)
class ProjectStyle:
    theme: str | None = None
    safe_area: SafeArea = SAFE_AREA_DEFAULT
    symbols: dict[str, str] = field(default_factory=dict)  # TeX symbol -> token or #RRGGBB


def find_spec(start: Path) -> Path | None:
    for directory in (start, *start.parents):
        candidate = directory / SPEC_NAME
        if candidate.is_file():
            return candidate
    return None


def load_style(start: Path) -> ProjectStyle:
    path = find_spec(start)
    if path is None:
        return ProjectStyle()
    try:
        spec = yaml.safe_load(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, yaml.YAMLError) as exc:
        raise CompositionError(f"Cannot read {path}: {exc}", path=str(path)) from exc
    if spec is None:
        return ProjectStyle()
    if not isinstance(spec, Mapping):
        raise CompositionError(f"{path} must be a YAML mapping.", path=str(path))
    return ProjectStyle(
        theme=_theme_name(spec.get("theme"), path),
        safe_area=_safe_area(spec.get("safe_area"), path),
        symbols=_symbols(spec.get("direction"), path),
    )


def _theme_name(value: Any, path: Path) -> str | None:
    if isinstance(value, Mapping):  # the legacy `{preset: name, ...}` form
        value = value.get("preset")
    if value is None or isinstance(value, str):
        return value
    raise CompositionError(f"{path}: theme must be a theme name.", path=str(path))


def _safe_area(value: Any, path: Path) -> SafeArea:
    if value is None:
        return SAFE_AREA_DEFAULT
    if not isinstance(value, Mapping):
        raise CompositionError(f"{path}: safe_area must be a mapping.", path=str(path))
    sides = {}
    for side in ("top", "right", "bottom", "left"):
        raw = value.get(side, getattr(SAFE_AREA_DEFAULT, side))
        if isinstance(raw, bool) or not isinstance(raw, int | float):
            raise CompositionError(f"{path}: safe_area.{side} must be a number.", path=str(path))
        sides[side] = float(raw)
    try:
        return SafeArea(**sides)
    except InvalidField as exc:
        raise CompositionError(
            f"{path}: safe_area.{exc.field} {exc.reason}.", path=str(path)
        ) from None


def _symbols(direction: Any, path: Path) -> dict[str, str]:
    symbols = direction.get("symbols") if isinstance(direction, Mapping) else None
    if symbols is None:
        return {}
    if not isinstance(symbols, Mapping) or not all(
        isinstance(key, str) and isinstance(value, str) for key, value in symbols.items()
    ):
        raise CompositionError(
            f"{path}: direction.symbols must map TeX symbols to color tokens.", path=str(path)
        )
    return dict(symbols)
