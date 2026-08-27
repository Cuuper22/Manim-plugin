from __future__ import annotations

import json
import math
import shutil
import tempfile
from pathlib import Path
from typing import Any, Mapping, Sequence

from .errors import DirectorError
from .util import Emit, confined_path, noop_emit, project_root, run_command


def _exe(name: str) -> str:
    path = shutil.which(name)
    if not path:
        raise DirectorError("executable_not_found", f"{name} is required for this media operation")
    return path


def probe_media(path: Path) -> dict[str, Any]:
    if not path.exists():
        raise DirectorError("media_not_found", f"Media file does not exist: {path}")
    command = [
        _exe("ffprobe"), "-v", "error", "-show_streams", "-show_format",
        "-of", "json", str(path),
    ]
    result = run_command(command, timeout=30)
    if result.returncode != 0:
        raise DirectorError("probe_failed", f"ffprobe could not inspect {path}", {"tail": result.output[-4000:]})
    try:
        payload = json.loads(result.output)
    except json.JSONDecodeError as exc:
        raise DirectorError("probe_invalid_json", "ffprobe returned malformed JSON") from exc
    format_info = payload.get("format") or {}
    streams = payload.get("streams") or []
    video = next((s for s in streams if s.get("codec_type") == "video"), None)
    audio = next((s for s in streams if s.get("codec_type") == "audio"), None)
    duration_raw = format_info.get("duration") or (video or {}).get("duration") or (audio or {}).get("duration")
    try:
        duration = float(duration_raw) if duration_raw is not None else None
    except (TypeError, ValueError):
        duration = None
    return {
        "path": str(path), "available": True, "bytes": path.stat().st_size,
        "format": format_info.get("format_name"), "duration_seconds": duration,
        "video": None if not video else {
            "codec": video.get("codec_name"), "width": video.get("width"), "height": video.get("height"),
            "pixel_format": video.get("pix_fmt"), "frame_rate": video.get("avg_frame_rate"),
            "frames": video.get("nb_frames"),
            "alpha_mode": (video.get("tags") or {}).get("alpha_mode"),
        },
        "audio": None if not audio else {
            "codec": audio.get("codec_name"), "sample_rate": audio.get("sample_rate"),
            "channels": audio.get("channels"), "layout": audio.get("channel_layout"),
        },
    }


def _compact_frame_rate(value: Any) -> float | None:
    try:
        if isinstance(value, str) and "/" in value:
            numerator, denominator = value.split("/", 1)
            rate = float(numerator) / float(denominator)
        else:
            rate = float(value)
    except (TypeError, ValueError, ZeroDivisionError):
        return None
    return round(rate, 3) if math.isfinite(rate) and rate >= 0 else None


def compact_media_summary(info: Mapping[str, Any]) -> dict[str, Any]:
    """Reduce ffprobe-derived data to a stable, bounded render-result summary."""

    duration = info.get("duration_seconds")
    try:
        duration_value = round(float(duration), 3) if duration is not None and math.isfinite(float(duration)) else None
    except (TypeError, ValueError):
        duration_value = None
    raw_format = str(info.get("format") or "")
    summary: dict[str, Any] = {
        "path": str(info.get("path", "")),
        "bytes": max(0, int(info.get("bytes", 0))),
        "container": raw_format.split(",", 1)[0][:32] or None,
        "format_name": raw_format[:64] or None,
        "duration_seconds": duration_value,
    }
    video = info.get("video")
    if isinstance(video, Mapping):
        pixel_format = str(video.get("pixel_format") or "")[:32]
        alpha_mode = str(video.get("alpha_mode") or "")[:16]
        has_alpha = (
            pixel_format.startswith(("rgba", "argb", "bgra", "abgr", "yuva", "gbrap", "ya"))
            or pixel_format == "gray8a"
            or (bool(alpha_mode) and alpha_mode != "0")
        )
        summary["video"] = {
            "codec": str(video.get("codec") or "")[:32] or None,
            "width": int(video["width"]) if video.get("width") is not None else None,
            "height": int(video["height"]) if video.get("height") is not None else None,
            "fps": _compact_frame_rate(video.get("frame_rate")),
            "pixel_format": pixel_format or None,
            "pix_fmt": pixel_format or None,
            "has_alpha": has_alpha,
        }
    audio = info.get("audio")
    if isinstance(audio, Mapping):
        summary["audio"] = {
            "codec": str(audio.get("codec") or "")[:32] or None,
            "channels": int(audio["channels"]) if audio.get("channels") is not None else None,
        }
    return summary


def extract_frame(source: Path, timestamp: float, destination: Path, *, emit: Emit = noop_emit) -> Path:
    if timestamp < 0:
        raise DirectorError("invalid_timestamp", "Frame timestamp cannot be negative")
    destination.parent.mkdir(parents=True, exist_ok=True)
    command = [
        _exe("ffmpeg"), "-hide_banner", "-loglevel", "error", "-y",
        "-ss", f"{timestamp:.6f}", "-i", str(source), "-frames:v", "1", str(destination),
    ]
    result = run_command(command, timeout=60, emit=emit)
    if result.returncode != 0 or not destination.exists():
        raise DirectorError("frame_extraction_failed", f"Could not extract a frame at {timestamp:g}s", {"tail": result.output[-4000:]})
    return destination


def representative_frames(
    source: Path, destination_dir: Path, *, count: int = 6, emit: Emit = noop_emit
) -> list[dict[str, Any]]:
    if not (1 <= count <= 40):
        raise DirectorError("invalid_frame_count", "Representative frame count must be between 1 and 40")
    info = probe_media(source)
    duration = info.get("duration_seconds")
    if not duration or duration <= 0:
        raise DirectorError("duration_unknown", "Cannot choose representative frames because media duration is unavailable")
    destination_dir.mkdir(parents=True, exist_ok=True)
    if count == 1:
        times = [duration / 2]
    else:
        margin = min(0.08 * duration, 0.5)
        usable = max(0.0, duration - 2 * margin)
        times = [margin + usable * i / (count - 1) for i in range(count)]
    frames = []
    for index, timestamp in enumerate(times):
        path = extract_frame(source, timestamp, destination_dir / f"frame-{index + 1:03d}.png", emit=emit)
        frames.append({"path": str(path), "timestamp": round(timestamp, 6)})
    return frames


def create_contact_sheet(images: Sequence[Path], destination: Path, *, columns: int = 3, thumb_width: int = 480) -> dict[str, Any]:
    try:
        from PIL import Image, ImageDraw
    except ImportError as exc:
        raise DirectorError("visual_dependency_missing", "Contact sheets require Pillow (`pip install Pillow`).") from exc
    if not images:
        raise DirectorError("images_required", "At least one image is required for a contact sheet")
    if not (1 <= columns <= 10 and 64 <= thumb_width <= 2048):
        raise DirectorError("invalid_contact_sheet", "columns or thumbnail width is outside supported bounds")
    loaded = []
    for path in images:
        try:
            loaded.append((path, Image.open(path).convert("RGB")))
        except Exception as exc:
            raise DirectorError("image_decode_failed", f"Could not decode image: {path}") from exc
    ratio = max(image.height / image.width for _, image in loaded)
    thumb_height = max(1, round(thumb_width * ratio))
    label_height, gutter = 34, 12
    rows = math.ceil(len(loaded) / columns)
    sheet = Image.new("RGB", (columns * thumb_width + (columns + 1) * gutter, rows * (thumb_height + label_height) + (rows + 1) * gutter), "#111318")
    draw = ImageDraw.Draw(sheet)
    for index, (path, image) in enumerate(loaded):
        row, col = divmod(index, columns)
        image.thumbnail((thumb_width, thumb_height))
        x = gutter + col * (thumb_width + gutter) + (thumb_width - image.width) // 2
        y = gutter + row * (thumb_height + label_height) + (thumb_height - image.height) // 2
        sheet.paste(image, (x, y))
        draw.text((gutter + col * (thumb_width + gutter) + 6, y + thumb_height + 5), path.name, fill="#F5F7FF")
    destination.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(destination, format="PNG", optimize=True)
    for _, image in loaded:
        image.close()
    return {"path": str(destination), "width": sheet.width, "height": sheet.height, "images": len(images), "columns": columns, "rows": rows}


def contact_sheet(params: Mapping[str, Any], emit: Emit = noop_emit) -> dict[str, Any]:
    root = project_root(params)
    output = confined_path(root, str(params.get("output", "output/contact-sheet.png")))
    raw_images = params.get("images")
    temp_parent = confined_path(root, ".manim-director/tmp")
    temp_parent.mkdir(parents=True, exist_ok=True)
    if raw_images:
        images = [confined_path(root, str(value), must_exist=True) for value in raw_images]
        frames = [{"path": str(path), "timestamp": None} for path in images]
        result = create_contact_sheet(images, output, columns=int(params.get("columns", 3)), thumb_width=int(params.get("thumb_width", 480)))
    else:
        source = confined_path(root, str(params.get("source", params.get("video", "output/final.mp4"))), must_exist=True)
        with tempfile.TemporaryDirectory(prefix="contact-", dir=temp_parent) as raw_temp:
            frames = representative_frames(source, Path(raw_temp), count=int(params.get("count", 6)), emit=emit)
            result = create_contact_sheet([Path(item["path"]) for item in frames], output, columns=int(params.get("columns", 3)), thumb_width=int(params.get("thumb_width", 480)))
    result["frames"] = frames
    return result


def normalize_audio(source: Path, destination: Path, *, target_lufs: float = -16.0, emit: Emit = noop_emit) -> dict[str, Any]:
    destination.parent.mkdir(parents=True, exist_ok=True)
    command = [
        _exe("ffmpeg"), "-hide_banner", "-loglevel", "error", "-y", "-i", str(source),
        "-af", f"loudnorm=I={target_lufs:g}:TP=-1.5:LRA=11", str(destination),
    ]
    result = run_command(command, timeout=600, emit=emit)
    if result.returncode != 0 or not destination.exists():
        raise DirectorError("audio_normalization_failed", "FFmpeg could not normalize the audio", {"tail": result.output[-4000:]})
    return probe_media(destination)


def mix_audio(tracks: Sequence[Mapping[str, Any]], destination: Path, *, emit: Emit = noop_emit) -> dict[str, Any]:
    if not tracks:
        raise DirectorError("tracks_required", "At least one audio track is required")
    destination.parent.mkdir(parents=True, exist_ok=True)
    command = [_exe("ffmpeg"), "-hide_banner", "-loglevel", "error", "-y"]
    filters = []
    labels = []
    for index, track in enumerate(tracks):
        path = Path(str(track["path"]))
        command += ["-i", str(path)]
        delay = max(0, round(float(track.get("start", 0)) * 1000))
        gain = float(track.get("gain", 1.0))
        label = f"a{index}"
        filters.append(f"[{index}:a]adelay={delay}|{delay},volume={gain:g}[{label}]")
        labels.append(f"[{label}]")
    filters.append(f"{''.join(labels)}amix=inputs={len(labels)}:duration=longest:normalize=0[out]")
    command += ["-filter_complex", ";".join(filters), "-map", "[out]", str(destination)]
    result = run_command(command, timeout=900, emit=emit)
    if result.returncode != 0 or not destination.exists():
        raise DirectorError("audio_mix_failed", "FFmpeg could not mix the audio tracks", {"tail": result.output[-4000:]})
    return probe_media(destination)
