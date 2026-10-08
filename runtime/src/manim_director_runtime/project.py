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


VIEWER_PROTOCOL = ("question", "wrong_guess", "aha")  # what a brief must say about the viewer


@dataclass(frozen=True, slots=True)
class Viewer:
    """`brief.viewer`: who the film is for. `level` picks the pacing budgets; `knows` items
    written `$...$` are notation the viewer already reads."""

    level: str = "general"
    knows: tuple[str, ...] = ()
    missing: tuple[str, ...] = ()  # VIEWER_PROTOCOL fields the brief leaves out


@dataclass(frozen=True, slots=True)
class StoryBeat:
    id: str
    intent: str | None = None
    question: str | None = None  # `audience_question`
    takeaway: str | None = None
    aha: bool = False


@dataclass(frozen=True, slots=True)
class ProjectStyle:
    theme: str | None = None
    safe_area: SafeArea = SAFE_AREA_DEFAULT
    symbols: dict[str, str] = field(default_factory=dict)  # TeX symbol -> token or #RRGGBB
    viewer: Viewer | None = None
    storyboard: tuple[StoryBeat, ...] = ()
    storyboard_scene: str | None = None  # `engine.main_scene`: the scene the storyboard plans
    pacing: dict[str, float] = field(default_factory=dict)  # `qa.pacing` budget overrides


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
        viewer=_viewer(spec.get("brief"), path),
        storyboard=_storyboard(spec.get("storyboard"), path),
        storyboard_scene=_main_scene(spec.get("engine")),
        pacing=_pacing(spec.get("qa"), path),
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


def _viewer(brief: Any, path: Path) -> Viewer | None:
    viewer = brief.get("viewer") if isinstance(brief, Mapping) else None
    if viewer is None:
        return None
    if not isinstance(viewer, Mapping):
        raise CompositionError(f"{path}: brief.viewer must be a mapping.", path=str(path))
    level, knows = viewer.get("level", "general"), viewer.get("knows", [])
    if not isinstance(level, str):
        raise CompositionError(f"{path}: brief.viewer.level must be a name.", path=str(path))
    if not isinstance(knows, list) or not all(isinstance(item, str) for item in knows):
        raise CompositionError(f"{path}: brief.viewer.knows must be a list.", path=str(path))
    missing = tuple(key for key in VIEWER_PROTOCOL if not str(viewer.get(key) or "").strip())
    return Viewer(level=level, knows=tuple(knows), missing=missing)


def _storyboard(storyboard: Any, path: Path) -> tuple[StoryBeat, ...]:
    if storyboard is None:
        return ()
    if not isinstance(storyboard, list) or not all(
        isinstance(beat, Mapping) and isinstance(beat.get("id"), str) for beat in storyboard
    ):
        raise CompositionError(
            f"{path}: storyboard must be a list of beats, each with an id.", path=str(path)
        )
    return tuple(
        StoryBeat(
            id=beat["id"],
            intent=_text(beat.get("intent")),
            question=_text(beat.get("audience_question")),
            takeaway=_text(beat.get("takeaway")),
            aha=beat.get("aha") is True,
        )
        for beat in storyboard
    )


def _main_scene(engine: Any) -> str | None:
    return _text(engine.get("main_scene")) if isinstance(engine, Mapping) else None


def _pacing(qa: Any, path: Path) -> dict[str, float]:
    pacing = qa.get("pacing") if isinstance(qa, Mapping) else None
    if pacing is None:
        return {}
    if not isinstance(pacing, Mapping) or not all(
        isinstance(key, str) and isinstance(value, int | float) and not isinstance(value, bool)
        for key, value in pacing.items()
    ):
        raise CompositionError(
            f"{path}: qa.pacing must map budget names to numbers.", path=str(path)
        )
    return dict(pacing)


def _text(value: Any) -> str | None:
    text = "" if value is None else str(value).strip()
    return text or None
