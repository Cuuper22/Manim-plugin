from __future__ import annotations

import hashlib
import json
import mimetypes
import re
import shutil
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any, Mapping

from .errors import DirectorError
from .media import normalize_audio, probe_media
from .util import Emit, atomic_write, confined_path, noop_emit, project_root


RASTER = {".png", ".jpg", ".jpeg", ".webp", ".bmp", ".tiff"}
AUDIO = {".wav", ".mp3", ".m4a", ".aac", ".flac", ".ogg", ".opus"}
VIDEO = {".mp4", ".mov", ".webm", ".mkv", ".avi"}


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def describe_asset(path: Path, root: Path, *, hashes: bool = False) -> dict[str, Any]:
    mime, _ = mimetypes.guess_type(path.name)
    item: dict[str, Any] = {
        "path": path.relative_to(root).as_posix(), "bytes": path.stat().st_size,
        "mime_type": mime or "application/octet-stream", "extension": path.suffix.lower(),
    }
    if hashes:
        item["sha256"] = _sha256(path)
    if path.suffix.lower() in RASTER:
        try:
            from PIL import Image

            with Image.open(path) as image:
                item["image"] = {"width": image.width, "height": image.height, "mode": image.mode, "format": image.format}
        except (ImportError, OSError):
            pass
    elif path.suffix.lower() in AUDIO | VIDEO:
        try:
            item["media"] = probe_media(path)
        except DirectorError:
            pass
    return item


def inventory(root: Path, asset_dir: Path, *, hashes: bool = False) -> list[dict[str, Any]]:
    if not asset_dir.exists():
        return []
    return [
        describe_asset(path, root, hashes=hashes)
        for path in sorted(asset_dir.rglob("*"))
        if path.is_file() and path.name != "manifest.json" and not path.is_symlink()
    ]


def _normalize_svg(source: Path, destination: Path) -> dict[str, Any]:
    try:
        tree = ET.parse(source)
    except ET.ParseError as exc:
        raise DirectorError("invalid_svg", f"SVG is malformed: {source}", {"error": str(exc)}) from exc
    root = tree.getroot()
    removed = 0
    for parent in list(root.iter()):
        for child in list(parent):
            if child.tag.rsplit("}", 1)[-1].lower() in {"script", "foreignobject"}:
                parent.remove(child)
                removed += 1
        for key in list(parent.attrib):
            local = key.rsplit("}", 1)[-1].lower()
            value = parent.attrib[key]
            if local.startswith("on") or (local == "href" and re.match(r"(?i)\s*(?:https?:|javascript:|data:text/html)", value)):
                del parent.attrib[key]
                removed += 1
    destination.parent.mkdir(parents=True, exist_ok=True)
    tree.write(destination, encoding="utf-8", xml_declaration=True)
    return {"removed_unsafe_nodes_or_attributes": removed}


def _normalize_raster(source: Path, destination: Path, max_dimension: int) -> dict[str, Any]:
    try:
        from PIL import Image, ImageOps
    except ImportError as exc:
        raise DirectorError("visual_dependency_missing", "Raster normalization requires Pillow (`pip install Pillow`).") from exc
    with Image.open(source) as loaded:
        image = ImageOps.exif_transpose(loaded)
        original = image.size
        if max(image.size) > max_dimension:
            image.thumbnail((max_dimension, max_dimension), Image.Resampling.LANCZOS)
        destination.parent.mkdir(parents=True, exist_ok=True)
        save_kwargs: dict[str, Any] = {"optimize": True}
        if destination.suffix.lower() in {".jpg", ".jpeg"}:
            if image.mode not in {"RGB", "L"}:
                image = image.convert("RGB")
            save_kwargs.update({"quality": 92, "progressive": True})
        image.save(destination, **save_kwargs)
        return {"original_size": list(original), "size": list(image.size)}


def _source_path(raw: Any) -> Path:
    path = Path(str(raw)).expanduser().resolve()
    if not path.exists() or not path.is_file():
        raise DirectorError("asset_not_found", f"Asset source does not exist: {path}")
    return path


def assets(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    root = project_root(params)
    asset_dir = confined_path(root, str(params.get("asset_dir", "assets")))
    asset_dir.mkdir(parents=True, exist_ok=True)
    operation = str(params.get("operation", "inventory"))
    if operation in {"inventory", "manifest"}:
        listed = inventory(root, asset_dir, hashes=bool(params.get("hashes", False)))
        result = {"operation": operation, "asset_dir": str(asset_dir), "assets": listed, "count": len(listed), "bytes": sum(item["bytes"] for item in listed)}
        if operation == "manifest" or params.get("write"):
            manifest_path = confined_path(root, str(params.get("manifest", "assets/manifest.json")))
            existing_metadata: dict[str, Any] = {}
            if manifest_path.exists():
                try:
                    current = json.loads(manifest_path.read_text(encoding="utf-8"))
                    asset_prefix = asset_dir.relative_to(root).as_posix().rstrip("/")
                    for item in current.get("assets", []):
                        if "path" not in item:
                            continue
                        recorded = str(item["path"]).replace("\\", "/").lstrip("./")
                        existing_metadata[recorded] = item
                        if asset_prefix and not recorded.startswith(f"{asset_prefix}/"):
                            existing_metadata[f"{asset_prefix}/{recorded}"] = item
                except (json.JSONDecodeError, OSError):
                    pass
            for item in listed:
                old = existing_metadata.get(item["path"], {})
                for key in ("source", "origin", "license", "attribution", "notes"):
                    if key in old:
                        item[key] = old[key]
            atomic_write(manifest_path, json.dumps({"version": 1, "assets": listed}, indent=2, ensure_ascii=False) + "\n")
            result["manifest"] = str(manifest_path)
        return result
    if operation not in {"add", "normalize"}:
        raise DirectorError("invalid_asset_operation", f"Unknown asset operation: {operation}")
    source = _source_path(params.get("source"))
    destination = confined_path(root, str(params.get("destination", f"assets/{source.name}")))
    if destination.exists() and not bool(params.get("force", False)):
        raise DirectorError("file_exists", f"Refusing to overwrite existing asset: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    suffix = source.suffix.lower()
    details: dict[str, Any] = {}
    if operation == "normalize" and suffix == ".svg":
        details = _normalize_svg(source, destination)
    elif operation == "normalize" and suffix in RASTER:
        details = _normalize_raster(source, destination, int(params.get("max_dimension", 4096)))
    elif operation == "normalize" and suffix in AUDIO:
        details = normalize_audio(source, destination, target_lufs=float(params.get("target_lufs", -16)), emit=emit)
    else:
        shutil.copy2(source, destination)
    item = describe_asset(destination, root, hashes=bool(params.get("hashes", False)))
    for key in ("license", "attribution", "notes"):
        if params.get(key) is not None:
            item[key] = params[key]
    item["source"] = str(params.get("provenance", source))
    return {"operation": operation, "asset": item, "normalization": details}
