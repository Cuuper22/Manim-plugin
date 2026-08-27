from __future__ import annotations

import json
import math
import statistics
import tempfile
from pathlib import Path
from typing import Any, Mapping, Sequence

from .errors import DirectorError
from .media import representative_frames
from .util import Emit, confined_path, noop_emit, project_root


def _percentile_from_histogram(histogram: Sequence[int], fraction: float) -> int:
    target = sum(histogram) * fraction
    running = 0
    for index, count in enumerate(histogram):
        running += count
        if running >= target:
            return index
    return len(histogram) - 1


def _frame_metrics(path: Path, safe_area: Mapping[str, float]) -> dict[str, Any]:
    try:
        from PIL import Image, ImageChops, ImageStat
    except ImportError as exc:
        raise DirectorError("visual_dependency_missing", "Visual QA requires Pillow (`pip install Pillow`).") from exc
    with Image.open(path) as loaded:
        image = loaded.convert("RGB")
        width, height = image.size
        gray = image.convert("L")
        stat = ImageStat.Stat(gray)
        corners = [image.getpixel((x, y)) for x, y in ((0, 0), (width - 1, 0), (0, height - 1), (width - 1, height - 1))]
        background = tuple(round(statistics.median(pixel[channel] for pixel in corners)) for channel in range(3))
        background_image = Image.new("RGB", image.size, background)
        difference = ImageChops.difference(image, background_image).convert("L")
        foreground = difference.point(lambda value: 255 if value > 16 else 0)
        box = foreground.getbbox()
        foreground_pixels = foreground.histogram()[255]
        foreground_fraction = foreground_pixels / max(1, width * height)
        background_luminance = round(0.2126 * background[0] + 0.7152 * background[1] + 0.0722 * background[2])
        if foreground_pixels:
            foreground_histogram = gray.histogram(mask=foreground)
            foreground_luminance = _percentile_from_histogram(foreground_histogram, 0.5)
        else:
            foreground_luminance = background_luminance
        low = min(background_luminance, foreground_luminance)
        high = max(background_luminance, foreground_luminance)
        contrast_ratio = (high / 255 + 0.05) / (low / 255 + 0.05)
        band = max(1, round(min(width, height) * 0.012))
        edge_pixels = sum(
            region.histogram()[255]
            for region in (
                foreground.crop((0, 0, width, band)), foreground.crop((0, height - band, width, height)),
                foreground.crop((0, band, band, height - band)), foreground.crop((width - band, band, width, height - band)),
            )
        )
        edge_total = 2 * width * band + 2 * max(0, height - 2 * band) * band
        safe_box = (
            round(width * float(safe_area.get("left", 0.05))),
            round(height * float(safe_area.get("top", 0.05))),
            round(width * (1 - float(safe_area.get("right", 0.05)))),
            round(height * (1 - float(safe_area.get("bottom", 0.08)))),
        )
        safe_mask = Image.new("L", image.size, 0)
        safe_mask.paste(255, safe_box)
        unsafe = ImageChops.subtract(foreground, safe_mask)
        unsafe_fraction = unsafe.histogram()[255] / max(1, width * height)
        return {
            "path": str(path), "width": width, "height": height,
            "mean_luminance": round(stat.mean[0], 4), "luminance_stddev": round(stat.stddev[0], 4),
            "luminance_range_90": high - low, "contrast_ratio_90": round(contrast_ratio, 4),
            "background_rgb": list(background), "foreground_bbox": list(box) if box else None,
            "foreground_fraction": round(foreground_fraction, 6),
            "edge_activity": round(edge_pixels / max(1, edge_total), 6),
            "unsafe_foreground_fraction": round(unsafe_fraction, 6), "safe_box": list(safe_box),
        }


def _metadata_issues(metadata: Mapping[str, Any], width: int, height: int, minimum_text_px: float) -> list[dict[str, Any]]:
    issues: list[dict[str, Any]] = []
    objects = list(metadata.get("objects", []))
    for obj in objects:
        bbox = obj.get("bbox")
        kind = str(obj.get("kind", obj.get("type", ""))).lower()
        if "text" in kind and obj.get("font_px") is not None and float(obj["font_px"]) < minimum_text_px:
            issues.append({"code": "tiny_text", "severity": "warning", "object": obj.get("name"), "font_px": float(obj["font_px"]), "minimum_px": minimum_text_px})
        if isinstance(bbox, Sequence) and len(bbox) == 4:
            x0, y0, x1, y1 = map(float, bbox)
            if x0 < 0 or y0 < 0 or x1 > width or y1 > height:
                issues.append({"code": "object_clipped", "severity": "error", "object": obj.get("name"), "bbox": list(bbox)})
    for index, left in enumerate(objects):
        if not left.get("bbox") or left.get("allow_overlap"):
            continue
        ax0, ay0, ax1, ay1 = map(float, left["bbox"])
        for right in objects[index + 1:]:
            if not right.get("bbox") or right.get("allow_overlap"):
                continue
            bx0, by0, bx1, by1 = map(float, right["bbox"])
            overlap = max(0, min(ax1, bx1) - max(ax0, bx0)) * max(0, min(ay1, by1) - max(ay0, by0))
            smaller = min(max(1, (ax1 - ax0) * (ay1 - ay0)), max(1, (bx1 - bx0) * (by1 - by0)))
            if overlap / smaller > 0.15:
                issues.append({"code": "object_overlap", "severity": "warning", "objects": [left.get("name"), right.get("name")], "overlap_fraction": round(overlap / smaller, 4)})
    return issues


def analyze_images(
    images: Sequence[Path], *, timestamps: Sequence[float | None] | None = None,
    safe_area: Mapping[str, float] | None = None, metadata: Mapping[str, Any] | None = None,
    minimum_text_px: float = 22,
) -> dict[str, Any]:
    safe_area = safe_area or {"top": 0.05, "right": 0.05, "bottom": 0.08, "left": 0.05}
    timestamps = timestamps or [None] * len(images)
    metrics = [_frame_metrics(path, safe_area) for path in images]
    issues: list[dict[str, Any]] = []
    for index, frame in enumerate(metrics):
        at = timestamps[index] if index < len(timestamps) else None
        context = {"frame": frame["path"], **({"timestamp": at} if at is not None else {})}
        if (
            frame["foreground_bbox"] is None
            or frame["foreground_fraction"] < 0.0001
            or (frame["luminance_stddev"] < 1.8 and frame["luminance_range_90"] < 4)
        ):
            issues.append({"code": "blank_frame", "severity": "error", "message": "Frame is blank or nearly uniform.", **context})
        if frame["foreground_bbox"] is not None and frame["contrast_ratio_90"] < 2.0:
            issues.append({"code": "low_contrast", "severity": "warning", "message": "The frame has little luminance separation.", **context})
        if frame["unsafe_foreground_fraction"] > 0.012 or frame["edge_activity"] > 0.2:
            issues.append({"code": "safe_area", "severity": "warning", "message": "Visible content reaches the configured safe-area boundary.", **context})
    if metadata and metrics:
        issues.extend(_metadata_issues(metadata, metrics[0]["width"], metrics[0]["height"], minimum_text_px))
    errors = sum(issue["severity"] == "error" for issue in issues)
    warnings = sum(issue["severity"] == "warning" for issue in issues)
    return {"status": "fail" if errors else "warn" if warnings else "pass", "frames": metrics, "issues": issues, "summary": {"frames": len(metrics), "errors": errors, "warnings": warnings}}


def qa(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    root = project_root(params)
    safe_area = params.get("safe_area") or {"top": 0.05, "right": 0.05, "bottom": 0.08, "left": 0.05}
    metadata = params.get("metadata")
    if isinstance(metadata, str):
        meta_path = confined_path(root, metadata, must_exist=True)
        try:
            metadata = json.loads(meta_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise DirectorError("invalid_metadata", f"Visual metadata is invalid JSON: {meta_path}") from exc
    raw_images = params.get("images")
    if raw_images:
        images = [confined_path(root, str(path), must_exist=True) for path in raw_images]
        timestamps: list[float | None] = [None] * len(images)
        return analyze_images(images, timestamps=timestamps, safe_area=safe_area, metadata=metadata, minimum_text_px=float(params.get("minimum_text_px", 22)))
    source = confined_path(root, str(params.get("source", params.get("video", params.get("artifact", "output/final.mp4")))), must_exist=True)
    if source.suffix.lower() in {".png", ".jpg", ".jpeg", ".webp", ".bmp"}:
        return analyze_images([source], safe_area=safe_area, metadata=metadata, minimum_text_px=float(params.get("minimum_text_px", 22)))
    temp_parent = confined_path(root, ".manim-director/tmp")
    temp_parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="qa-", dir=temp_parent) as raw_temp:
        frames = representative_frames(source, Path(raw_temp), count=int(params.get("frame_count", 8)), emit=emit)
        result = analyze_images(
            [Path(frame["path"]) for frame in frames], timestamps=[frame["timestamp"] for frame in frames],
            safe_area=safe_area, metadata=metadata, minimum_text_px=float(params.get("minimum_text_px", 22)),
        )
        # Paths in the metrics refer to temporary frames, so identify them by timestamp after analysis.
        for frame in result["frames"]:
            frame.pop("path", None)
        for issue in result["issues"]:
            if "frame" in issue:
                issue.pop("frame")
        result["source"] = str(source)
        return result
