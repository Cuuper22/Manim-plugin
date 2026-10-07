"""Resolved task types the engine sends as request `params`, and their strict parser.

Every task is parsed exactly once at the bridge boundary. Unknown keys, missing keys and
wrong types fail with `invalid_params` naming the dotted field (`settings.width`).
"""

from __future__ import annotations

import dataclasses
import functools
import math
import types
import typing
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

from .errors import invalid_params


class InvalidField(Exception):
    def __init__(self, field: str, reason: str) -> None:
        super().__init__(reason)
        self.field = field
        self.reason = reason


def check(condition: bool, field: str, reason: str) -> None:
    if not condition:
        raise InvalidField(field, reason)


@dataclass(frozen=True, slots=True)
class RenderSettings:
    profile: str
    width: int
    height: int
    fps: int
    renderer: Literal["cairo", "opengl"]
    format: Literal["mp4", "mov", "webm", "gif", "png"]
    transparent: bool

    def __post_init__(self) -> None:
        check(self.width > 0, "width", "must be positive")
        check(self.height > 0, "height", "must be positive")
        check(self.fps > 0, "fps", "must be positive")
        check(
            not self.transparent or self.format in ("mov", "webm", "png"),
            "transparent",
            f"{self.format} cannot carry an alpha channel",
        )


@dataclass(frozen=True, slots=True)
class SafeArea:
    top: float
    right: float
    bottom: float
    left: float

    def __post_init__(self) -> None:
        for side in ("top", "right", "bottom", "left"):
            check(0 <= getattr(self, side) <= 0.45, side, "must be between 0 and 0.45")


SAFE_AREA_DEFAULT = SafeArea(top=0.05, right=0.05, bottom=0.08, left=0.05)


@dataclass(frozen=True, slots=True)
class InitTask:
    mode: Literal["create", "overwrite", "add_scene"]
    name: str | None
    template: str | None
    scene_template: str | None
    theme: str | None
    seed: int | None
    source_dir: Path | None
    # Not in the contract's InitTask: add_scene has no other way to express `force`.
    force: bool = False

    def __post_init__(self) -> None:
        adding = self.mode == "add_scene"
        check(
            (self.scene_template is not None) == adding,
            "scene_template",
            "is required for add_scene and only allowed there",
        )
        check((self.source_dir is not None) == adding, "source_dir", "is set iff mode is add_scene")
        if adding:
            for name in ("name", "template", "theme", "seed"):
                check(getattr(self, name) is None, name, "is only allowed when creating a project")
        if self.seed is not None:
            check(0 <= self.seed <= 0x7FFF_FFFF, "seed", "must be between 0 and 2147483647")


@dataclass(frozen=True, slots=True)
class DiscoverTask:
    files: list[Path]


@dataclass(frozen=True, slots=True)
class DoctorTask:
    pass


@dataclass(frozen=True, slots=True)
class StillTask:
    scene: str | None
    files: list[Path]
    settings: RenderSettings
    media_dir: Path
    out_dir: Path
    fresh: bool

    def __post_init__(self) -> None:
        check(bool(self.files), "files", "must not be empty")
        check(self.settings.format == "png", "settings.format", "a still is always png")


@dataclass(frozen=True, slots=True)
class RenderTask:
    scene: str | None
    files: list[Path]
    settings: RenderSettings
    media_dir: Path
    out_dir: Path
    sections: bool
    fresh: bool

    def __post_init__(self) -> None:
        check(bool(self.files), "files", "must not be empty")
        check(self.settings.format != "png", "settings.format", "png is only for still")


@dataclass(frozen=True, slots=True)
class FrameTask:
    video: Path
    at_seconds: float
    out_dir: Path

    def __post_init__(self) -> None:
        check(self.at_seconds >= 0, "at_seconds", "must not be negative")


@dataclass(frozen=True, slots=True)
class ContactSheetTask:
    video: Path
    count: int
    columns: int
    timeline: Path | None
    out_dir: Path

    def __post_init__(self) -> None:
        check(self.count >= 1, "count", "must be at least 1")
        check(self.columns >= 1, "columns", "must be at least 1")


@dataclass(frozen=True, slots=True)
class QaTask:
    source: Path
    source_kind: Literal["video", "image"]
    frames: int
    safe_area: SafeArea
    timeline: Path | None
    out_dir: Path

    def __post_init__(self) -> None:
        check(self.frames >= 1, "frames", "must be at least 1")


@dataclass(frozen=True, slots=True)
class DiagnoseTask:
    text: str


@dataclass(frozen=True, slots=True)
class ValidateMathTask:
    steps: list[str]
    ranges: dict[str, tuple[float, float]]
    samples: int
    tolerance: float
    seed: int

    def __post_init__(self) -> None:
        check(len(self.steps) >= 2, "steps", "a derivation needs at least two steps")
        for index, step in enumerate(self.steps):
            check(bool(step.strip()), f"steps[{index}]", "must not be empty")
        for name, (low, high) in self.ranges.items():
            check(low < high, f"ranges.{name}", "lower bound must be below the upper bound")
        check(self.samples >= 1, "samples", "must be at least 1")
        check(0 < self.tolerance <= 1, "tolerance", "must be in (0, 1]")


@dataclass(frozen=True, slots=True)
class CaptionsTask:
    path: Path
    shift_seconds: float
    scale: float
    output: Path | None

    def __post_init__(self) -> None:
        check(self.scale > 0, "scale", "must be positive")


IngestKind = Literal[
    "markdown",
    "latex",
    "typst",
    "text",
    "csv",
    "json",
    "python",
    "notebook",
    "pdf",
    "svg",
    "image",
    "audio",
    "video",
    "other",
]


@dataclass(frozen=True, slots=True)
class IngestSource:
    path: Path
    kind: IngestKind
    destination_dir: Path
    id: str | None
    license: str | None
    attribution: str | None


@dataclass(frozen=True, slots=True)
class IngestTask:
    sources: list[IngestSource]
    normalize: bool
    force: bool
    manifest: Path

    def __post_init__(self) -> None:
        check(bool(self.sources), "sources", "must not be empty")


@dataclass(frozen=True, slots=True)
class ExportEntry:
    path: Path
    archive_path: str


@dataclass(frozen=True, slots=True)
class ZipExportTask:
    format: Literal["zip"]
    output: Path
    project_name: str
    source_job_id: str | None
    entries: list[ExportEntry]


@dataclass(frozen=True, slots=True)
class GifOptions:
    fps: int
    width: int

    def __post_init__(self) -> None:
        check(self.fps >= 1, "fps", "must be at least 1")
        check(self.width >= 2, "width", "must be at least 2")


@dataclass(frozen=True, slots=True)
class MediaExportTask:
    format: Literal["mp4", "webm", "gif"]
    output: Path
    source: Path
    alpha: bool
    gif: GifOptions | None

    def __post_init__(self) -> None:
        check((self.gif is not None) == (self.format == "gif"), "gif", "is set iff format is gif")


ExportTask = ZipExportTask | MediaExportTask


def parse_task(task_type: Any, raw: object) -> Any:
    try:
        return _parse(task_type, raw, "")
    except InvalidField as exc:
        raise invalid_params(exc.field or None, exc.reason) from None


def _join(path: str, name: str) -> str:
    return f"{path}.{name}" if path else name


@functools.cache
def _hints(cls: type) -> dict[str, Any]:
    return typing.get_type_hints(cls)


def _parse(tp: Any, value: object, path: str) -> Any:
    origin = typing.get_origin(tp)
    if dataclasses.is_dataclass(tp):
        return _parse_dataclass(tp, value, path)
    if origin in (types.UnionType, typing.Union):
        return _parse_union(typing.get_args(tp), value, path)
    if origin is Literal:
        allowed = typing.get_args(tp)
        check(value in allowed, path, f"must be one of {', '.join(map(str, allowed))}")
        return value
    if origin is list:
        check(isinstance(value, list), path, "must be an array")
        (item_type,) = typing.get_args(tp)
        return [_parse(item_type, item, f"{path}[{i}]") for i, item in enumerate(value)]
    if origin is dict:
        check(isinstance(value, dict), path, "must be an object")
        _, item_type = typing.get_args(tp)
        return {key: _parse(item_type, item, _join(path, key)) for key, item in value.items()}
    if origin is tuple:
        item_types = typing.get_args(tp)
        check(
            isinstance(value, list) and len(value) == len(item_types),
            path,
            f"must be an array of {len(item_types)} items",
        )
        pairs = enumerate(zip(item_types, value, strict=True))
        return tuple(_parse(t, item, f"{path}[{i}]") for i, (t, item) in pairs)
    if tp is bool:
        check(isinstance(value, bool), path, "must be a boolean")
        return value
    if tp is int:
        check(isinstance(value, int) and not isinstance(value, bool), path, "must be an integer")
        return value
    if tp is float:
        check(
            isinstance(value, (int, float))
            and not isinstance(value, bool)
            and math.isfinite(value),
            path,
            "must be a finite number",
        )
        return float(value)
    if tp is str:
        check(isinstance(value, str), path, "must be a string")
        return value
    if tp is Path:
        check(
            isinstance(value, str) and Path(value).is_absolute(), path, "must be an absolute path"
        )
        return Path(value)
    raise TypeError(f"unsupported task field type {tp!r}")


def _parse_dataclass(cls: type, value: object, path: str) -> Any:
    if not isinstance(value, dict):
        raise InvalidField(path, "must be an object")
    hints = _hints(cls)
    fields = {f.name: f for f in dataclasses.fields(cls)}
    for key in value:
        check(key in fields, _join(path, str(key)), "unknown field")
    kwargs = {}
    for name, spec in fields.items():
        if name in value:
            kwargs[name] = _parse(hints[name], value[name], _join(path, name))
        else:
            has_default = spec.default is not dataclasses.MISSING
            check(has_default, _join(path, name), "missing field")
    try:
        return cls(**kwargs)
    except InvalidField as exc:
        raise InvalidField(_join(path, exc.field), exc.reason) from None


def _parse_union(options: tuple[Any, ...], value: object, path: str) -> Any:
    if value is None and type(None) in options:
        return None
    candidates = [option for option in options if option is not type(None)]
    if len(candidates) == 1:
        return _parse(candidates[0], value, path)
    # Tagged unions (ExportTask) are discriminated by one Literal field.
    if not isinstance(value, dict):
        raise InvalidField(path, "must be an object")
    tag = next(name for name, hint in _hints(candidates[0]).items() if _is_literal(hint))
    check(tag in value, _join(path, tag), "missing field")
    for option in candidates:
        if value.get(tag) in typing.get_args(_hints(option)[tag]):
            return _parse(option, value, path)
    allowed = [str(arg) for option in candidates for arg in typing.get_args(_hints(option)[tag])]
    raise InvalidField(_join(path, tag), f"must be one of {', '.join(allowed)}")


def _is_literal(hint: Any) -> bool:
    return typing.get_origin(hint) is Literal
