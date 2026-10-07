"""The `ingest` operation: copy external files into the project with summaries and provenance."""

from __future__ import annotations

import hashlib
import os
import re
import shutil
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from .errors import invalid_source, io_error
from .jsonio import write_json
from .media import require_pillow
from .model import ArtifactKind, RuntimeArtifact
from .paths import atomic_target, create_exclusive, ensure_dir, slug
from .summaries import (
    compact,
    local_name,
    media_tool,
    parse_svg,
    read_json,
    summarize,
)
from .tasks import IngestSource, IngestTask

if TYPE_CHECKING:
    from .protocol import Context

MANIFEST_VERSION = 2
MAX_RASTER_SIDE = 4096
TARGET_LOUDNESS_LUFS = -16
_MAX_HEADINGS, _MAX_COLUMNS, _MAX_SCENES = 30, 100, 100
_UNSAFE_SVG_ELEMENTS = {"script", "foreignobject"}
_UNSAFE_HREF = re.compile(r"(?i)\s*(?:https?:|javascript:|data:text/html)")


@dataclass(frozen=True, slots=True)
class IngestedSource:
    id: str
    kind: str
    path: str
    bytes: int
    sha256: str
    origin: str
    summary: str
    headings: list[str]
    columns: list[str]
    rows: int | None
    pages: int | None
    scenes: list[str]
    width: int | None
    height: int | None
    duration_seconds: float | None
    normalized: bool


@dataclass(frozen=True, slots=True)
class IngestResult:
    sources: list[IngestedSource]
    artifacts: list[RuntimeArtifact]


def ingest(task: IngestTask, ctx: Context) -> IngestResult:
    manifest = ctx.require_inside(task.manifest, "manifest")
    created: list[Path] = []
    targets: list[Path] = []
    results: list[IngestedSource] = []
    try:
        for index, source in enumerate(task.sources):
            field = f"sources[{index}].destination_dir"
            target = _allocate(
                ctx.require_inside(source.destination_dir, field), source, task.force
            )
            if not task.force or not target.exists():
                created.append(target)
            with atomic_target(target) as temp:
                normalized = _store(source, temp, normalize=task.normalize)
            targets.append(target)
            results.append(_describe(source, target, normalized, ctx))
            ctx.progress("ingest", index + 1, len(task.sources))
        _merge_manifest(manifest, results, task.sources)
    except Exception:
        # A failed request leaves no new files behind; forced overwrites stay overwritten.
        for path in created:
            path.unlink(missing_ok=True)
        raise
    artifacts = [ctx.artifact(ArtifactKind.FILE, target) for target in targets]
    artifacts.append(ctx.artifact(ArtifactKind.FILE, manifest))
    return IngestResult(sources=results, artifacts=artifacts)


def _allocate(directory: Path, source: IngestSource, force: bool) -> Path:
    stem, suffix = slug(source.id or source.path.stem, "source"), source.path.suffix.lower()
    if force:
        return ensure_dir(directory) / f"{stem}{suffix}"
    return create_exclusive(directory, stem, suffix)


def _store(source: IngestSource, target: Path, *, normalize: bool) -> bool:
    """Copy `source` to `target`, normalizing when asked; True if the bytes were transformed."""

    if normalize and source.kind == "svg":
        _sanitize_svg(source.path, target)
        return True
    if normalize and source.kind == "image" and _downscale(source.path, target):
        return True
    if normalize and source.kind == "audio":
        loudnorm = f"loudnorm=I={TARGET_LOUDNESS_LUFS}:TP=-1.5:LRA=11"
        args = ["-loglevel", "error", "-nostdin", "-y", "-i", str(source.path), "-af", loudnorm]
        media_tool(source.path, "ffmpeg", [*args, str(target)])
        return True
    try:
        shutil.copyfile(source.path, target)
    except OSError as exc:
        raise io_error(source.path, exc) from exc
    return False


def _describe(source: IngestSource, target: Path, normalized: bool, ctx: Context) -> IngestedSource:
    summary = summarize(target, source.kind)
    return IngestedSource(
        id=target.stem,
        kind=source.kind,
        path=ctx.relative(target),
        bytes=target.stat().st_size,
        sha256=_sha256(target),
        origin=str(source.path),
        summary=compact(summary.summary),
        headings=summary.headings[:_MAX_HEADINGS],
        columns=summary.columns[:_MAX_COLUMNS],
        rows=summary.rows,
        pages=summary.pages,
        scenes=summary.scenes[:_MAX_SCENES],
        width=summary.width,
        height=summary.height,
        duration_seconds=summary.duration_seconds,
        normalized=normalized,
    )


def _merge_manifest(path: Path, results: list[IngestedSource], sources: list[IngestSource]) -> None:
    existing: list[Any] = []
    if path.exists():
        current = read_json(path)
        if isinstance(current, dict) and current.get("version") == MANIFEST_VERSION:
            existing = [e for e in current.get("entries", []) if isinstance(e, dict)]
        else:
            # Keep an older or foreign manifest instead of silently dropping its provenance.
            backup = create_exclusive(path.parent, f"{path.stem}.v1", path.suffix)
            try:
                os.replace(path, backup)
            except OSError as exc:
                raise io_error(path, exc) from exc
    entries = [
        {
            "id": item.id,
            "kind": item.kind,
            "path": item.path,
            "bytes": item.bytes,
            "sha256": item.sha256,
            "origin": item.origin,
            "license": source.license,
            "attribution": source.attribution,
        }
        for item, source in zip(results, sources, strict=True)
    ]
    replaced = {entry["path"] for entry in entries}
    kept = [entry for entry in existing if entry.get("path") not in replaced]
    write_json(path, {"version": MANIFEST_VERSION, "entries": kept + entries})


def _sanitize_svg(source: Path, target: Path) -> None:
    root = parse_svg(source)
    for parent in root.iter():
        for child in list(parent):
            if local_name(child.tag).lower() in _UNSAFE_SVG_ELEMENTS:
                parent.remove(child)
        for key, value in list(parent.attrib.items()):
            name = local_name(key).lower()
            if name.startswith("on") or (name == "href" and _UNSAFE_HREF.match(value)):
                del parent.attrib[key]
    ET.ElementTree(root).write(target, encoding="utf-8", xml_declaration=True)


def _downscale(source: Path, target: Path) -> bool:
    pil = require_pillow()
    from PIL import ImageOps

    try:
        with pil.open(source) as loaded:
            if max(loaded.size) <= MAX_RASTER_SIDE:
                return False
            image = ImageOps.exif_transpose(loaded)
            image.thumbnail((MAX_RASTER_SIDE, MAX_RASTER_SIDE), pil.Resampling.LANCZOS)
            if target.suffix.lower() in (".jpg", ".jpeg") and image.mode not in ("RGB", "L"):
                image = image.convert("RGB")
            image.save(target, format=loaded.format)
    except (OSError, ValueError, pil.DecompressionBombError) as exc:
        raise invalid_source(source, "it is not a readable image") from exc
    return True


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()
