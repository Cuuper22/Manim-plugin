"""The `frame` and `contact_sheet` operations, plus frame grabbing and sheets shared with
`qa`."""

from __future__ import annotations

import io
import json
import math
import types
from collections.abc import Sequence
from dataclasses import dataclass
from fractions import Fraction
from functools import cache
from pathlib import Path
from typing import TYPE_CHECKING

from . import process, timeline
from .errors import DirectorError, dependency_missing
from .model import ArtifactKind, RuntimeArtifact
from .paths import atomic_target, ensure_dir
from .tasks import ContactSheetTask, FrameTask

if TYPE_CHECKING:
    from PIL.Image import Image
    from PIL.ImageFont import FreeTypeFont

    from .protocol import Context

THUMBNAIL_WIDTH = 480
_INK, _MUTED = "#F5F7FF", "#9AA3B5"
_GUTTER, _LINE_HEIGHT = 12, 24

Line = Sequence[tuple[str, str]]  # runs of text, each with its color


@dataclass(frozen=True, slots=True)
class VideoInfo:
    duration_seconds: float
    fps: float | None
    frame_count: int | None

    def frame_time(self, seconds: float) -> float:
        """The start time of the frame on screen at `seconds`, clamped to the last frame."""

        if not self.fps:
            return min(max(seconds, 0.0), self.duration_seconds)
        frames = self.frame_count or max(1, round(self.duration_seconds * self.fps))
        index = min(math.floor(seconds * self.fps + 1e-6), frames - 1)
        return max(index, 0) / self.fps


@dataclass(frozen=True, slots=True)
class FrameResult:
    at_seconds: float
    artifacts: list[RuntimeArtifact]


@dataclass(frozen=True, slots=True)
class SheetFrame:
    at_seconds: float
    beat: str | None


@dataclass(frozen=True, slots=True)
class BeatTile:
    """A tile of a beat sheet: the frame at `at_seconds` under `name · detail` and the
    question it should answer."""

    at_seconds: float
    name: str
    detail: str
    question: str | None


@dataclass(frozen=True, slots=True)
class ContactSheetResult:
    frames: list[SheetFrame]
    columns: int
    rows: int
    artifacts: list[RuntimeArtifact]


def frame(task: FrameTask, ctx: Context) -> FrameResult:
    out_dir = ensure_dir(ctx.require_inside(task.out_dir, "out_dir"))
    info = probe_video(task.video)
    at = info.frame_time(task.at_seconds)
    ctx.progress("extract", 0, 1)
    png = grab_frame(task.video, at, info)
    path = out_dir / f"frame-{round(task.at_seconds * 1000):08d}.png"
    with atomic_target(path) as temp:
        temp.write_bytes(png)
    ctx.progress("extract", 1, 1)
    return FrameResult(at_seconds=round(at, 6), artifacts=[ctx.artifact(ArtifactKind.IMAGE, path)])


def contact_sheet(task: ContactSheetTask, ctx: Context) -> ContactSheetResult:
    pil = require_pillow()
    out_dir = ensure_dir(ctx.require_inside(task.out_dir, "out_dir"))
    beats = load_timeline(task.timeline, ctx)
    info = probe_video(task.video)
    times = [info.frame_time(t) for t in sample_times(info.duration_seconds, task.count)]
    images: list[Image] = []
    frames: list[SheetFrame] = []
    for done, at in enumerate(times, start=1):
        images.append(pil.open(io.BytesIO(grab_frame(task.video, at, info))).convert("RGB"))
        beat = beats.beat_at(at) if beats else None
        frames.append(SheetFrame(at_seconds=round(at, 6), beat=beat.id if beat else None))
        ctx.progress("extract", done, len(times))
    columns = min(task.columns, len(images))
    rows = math.ceil(len(images) / columns)
    labels = [
        [[(_clock(item.at_seconds) + "  ", _MUTED), *([(item.beat, _INK)] if item.beat else [])]]
        for item in frames
    ]
    path = out_dir / "contact-sheet.png"
    _save(_compose(images, labels, columns, rows), path)
    return ContactSheetResult(
        frames=frames,
        columns=columns,
        rows=rows,
        artifacts=[ctx.artifact(ArtifactKind.CONTACT_SHEET, path)],
    )


def beat_sheet(
    video: Path, info: VideoInfo, tiles: Sequence[BeatTile], path: Path, ctx: Context
) -> None:
    """One frame per tile, three to a row, each under its name and question: the sheet for
    judging a film from its frames alone."""

    pil = require_pillow()
    images: list[Image] = []
    for done, tile in enumerate(tiles, start=1):
        images.append(pil.open(io.BytesIO(grab_frame(video, tile.at_seconds, info))).convert("RGB"))
        ctx.progress("extract", done, len(tiles))
    width = THUMBNAIL_WIDTH - 4
    labels: list[list[Line]] = []
    for tile in tiles:
        lines = [[(tile.name, _INK), (f" · {tile.detail}", _MUTED)]]
        if tile.question:
            lines += [[(line, _INK)] for line in _wrapped(tile.question, width, lines=2)]
        else:
            lines.append([("(no audience question)", _MUTED)])
        labels.append(lines)
    columns = min(3, len(images))
    _save(_compose(images, labels, columns, math.ceil(len(images) / columns)), path)


def sample_times(duration: float, count: int, skip: Sequence[timeline.Span] = ()) -> list[float]:
    """`count` evenly spaced times, keeping a small margin from both ends. The spans in `skip`
    (in order, disjoint) are cut out first, so no time falls inside one unless all do."""

    kept: list[tuple[float, float]] = []
    start = 0.0
    for span in skip:
        kept.append((start, min(span.start_seconds, duration)))
        start = max(start, span.end_seconds)
    kept.append((start, duration))
    kept = [(a, b) for a, b in kept if b > a] or [(0.0, duration)]
    length = sum(b - a for a, b in kept)
    if count == 1:
        offsets = [length / 2]
    else:
        margin = min(0.08 * length, 0.5)
        step = (length - 2 * margin) / (count - 1)
        offsets = [margin + step * index for index in range(count)]
    return [_position(kept, offset) for offset in offsets]


def _position(parts: list[tuple[float, float]], offset: float) -> float:
    """The time `offset` seconds into `parts` laid end to end."""

    for start, end in parts:
        if offset <= end - start:
            return start + offset
        offset -= end - start
    return parts[-1][1]


def probe_video(path: Path) -> VideoInfo:
    output = process.run(
        "ffprobe",
        [
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=avg_frame_rate,r_frame_rate,nb_frames,duration:format=duration",
            "-of",
            "json",
            str(path),
        ],
    )
    payload = json.loads(output)
    streams = payload.get("streams") or []
    if not streams:
        raise DirectorError(
            "media_error",
            f"{path.name} has no video stream.",
            {"tool": "ffprobe", "exit_code": 0, "stderr_tail": ""},
        )
    stream = streams[0]
    duration = _number(stream.get("duration")) or _number(payload.get("format", {}).get("duration"))
    fps = _rate(stream.get("avg_frame_rate")) or _rate(stream.get("r_frame_rate"))
    frames = stream.get("nb_frames")
    return VideoInfo(
        duration_seconds=duration or 0.0,
        fps=fps,
        frame_count=int(frames) if frames and str(frames).isdigit() else None,
    )


def grab_frame(video: Path, at_seconds: float, info: VideoInfo) -> bytes:
    """PNG bytes of the frame starting at `at_seconds` (a value from `VideoInfo.frame_time`)."""

    # Accurate input seeking returns the first frame at or after the target, so aim half a
    # frame early to land exactly on the frame that starts at `at_seconds`.
    seek = max(0.0, at_seconds - 0.5 / info.fps) if info.fps else at_seconds
    png = process.run(
        "ffmpeg",
        [
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-ss",
            f"{seek:.6f}",
            "-i",
            str(video),
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-c:v",
            "png",
            "-",
        ],
    )
    if not png:
        raise DirectorError(
            "media_error",
            f"ffmpeg returned no frame at {at_seconds:.3f}s of {video.name}.",
            {"tool": "ffmpeg", "exit_code": 0, "stderr_tail": ""},
        )
    return png


def load_timeline(path: Path | None, ctx: Context) -> timeline.Timeline | None:
    if path is None:
        return None
    try:
        return timeline.load(path)
    except DirectorError as error:
        ctx.log("warning", f"{error.message} Frames are not mapped to beats.")
        return None


def require_pillow() -> types.ModuleType:
    try:
        from PIL import Image
    except ImportError:
        raise dependency_missing("PIL", "Install Pillow in the runtime environment.") from None
    return Image


def _compose(
    images: list[Image], labels: Sequence[Sequence[Line]], columns: int, rows: int
) -> Image:
    from PIL import Image, ImageDraw

    font = _font()
    label_height = 10 + _LINE_HEIGHT * max(len(label) for label in labels)
    ratio = max(image.height / image.width for image in images)
    thumb = (THUMBNAIL_WIDTH, max(1, round(THUMBNAIL_WIDTH * ratio)))
    size = (
        columns * thumb[0] + (columns + 1) * _GUTTER,
        rows * (thumb[1] + label_height) + (rows + 1) * _GUTTER,
    )
    sheet = Image.new("RGB", size, "#111318")
    draw = ImageDraw.Draw(sheet)
    for index, (image, label) in enumerate(zip(images, labels, strict=True)):
        row, column = divmod(index, columns)
        x = _GUTTER + column * (thumb[0] + _GUTTER)
        y = _GUTTER + row * (thumb[1] + label_height + _GUTTER)
        image.thumbnail(thumb)
        sheet.paste(image, (x + (thumb[0] - image.width) // 2, y + (thumb[1] - image.height) // 2))
        for number, line in enumerate(label):
            left, top = x + 2, y + thumb[1] + 8 + number * _LINE_HEIGHT
            for text, color in line:
                draw.text((left, top), text, fill=color, font=font)
                left += font.getlength(text)
    return sheet


def _save(sheet: Image, path: Path) -> None:
    with atomic_target(path) as temp:
        sheet.save(temp, format="PNG", optimize=True)


@cache
def _font() -> FreeTypeFont:
    from PIL import ImageFont

    try:  # questions and the aha quote ×, ⋯ and Greek, which Pillow's own font lacks
        return ImageFont.truetype("DejaVuSans.ttf", 18)
    except OSError:
        return ImageFont.load_default(size=18)


def _wrapped(text: str, width: float, *, lines: int) -> list[str]:
    """`text` broken into at most `lines` lines no wider than `width`; a cut ends in "…"."""

    font, words, out = _font(), text.split(), []
    while words and len(out) < lines:
        line = words.pop(0)
        while words and font.getlength(f"{line} {words[0]}") <= width:
            line += " " + words.pop(0)
        out.append(line)
    out = [_clipped(line, width) for line in out]
    if words and not out[-1].endswith("…"):
        out[-1] = _clipped(out[-1] + " …", width)
    return out


def _clipped(line: str, width: float) -> str:
    font = _font()
    if font.getlength(line) <= width:
        return line
    while line and font.getlength(line + "…") > width:
        line = line[:-1]
    return line.rstrip() + "…"


def _clock(seconds: float) -> str:
    minutes, tenths = divmod(round(seconds * 10), 600)  # round first: 59.95 s is 01:00.0
    return f"{minutes:02d}:{tenths / 10:04.1f}"


def _number(value: object) -> float | None:
    if not isinstance(value, (str, int, float)):
        return None
    try:
        number = float(value)
    except ValueError:
        return None
    return number if math.isfinite(number) and number > 0 else None


def _rate(value: object) -> float | None:
    try:
        rate = float(Fraction(str(value)))
    except (ValueError, ZeroDivisionError):
        return None
    return rate if rate > 0 else None
