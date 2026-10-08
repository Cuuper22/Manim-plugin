"""The `qa` operation: measure sampled frames for blankness, contrast and safe-area use, and
judge a DirectedScene render's pacing from its beat timeline."""

from __future__ import annotations

import statistics
from collections.abc import Sequence
from dataclasses import dataclass, replace
from pathlib import Path
from typing import TYPE_CHECKING, Literal

from . import pacing
from .errors import invalid_source
from .media import (
    BeatTile,
    VideoInfo,
    beat_sheet,
    grab_frame,
    load_timeline,
    probe_video,
    require_pillow,
    sample_times,
)
from .model import ArtifactKind, Finding, RuntimeArtifact, Severity, SourceLocation
from .paths import atomic_target, ensure_dir
from .project import StoryBeat, Viewer, load_style
from .tasks import QaTask, SafeArea

if TYPE_CHECKING:
    from PIL.Image import Image

    from .protocol import Context
    from .timeline import Timeline

_FOREGROUND_DELTA = 16  # grey-level difference from the background that counts as content
_MIN_CONTRAST = 3.0  # WCAG AA for large text; video text and formulas are large text
_MAX_UNSAFE_SHARE = 0.02  # of the content: sparse text clipped by the frame is still caught
_MAX_UNSAFE_FRACTION = 0.012  # of the frame
_MAX_EDGE_ACTIVITY = 0.2
_SHEET_TILES = 24  # beats.png: the first 21 beats, two more frames of the aha, the final frame


@dataclass(frozen=True, slots=True)
class FrameMetrics:
    mean_luminance: float
    luminance_stddev: float
    contrast_ratio: float
    foreground_fraction: float
    unsafe_fraction: float
    edge_activity: float
    foreground_bbox: list[int] | None
    background_rgb: list[int]


@dataclass(frozen=True, slots=True)
class QaFrame:
    at_seconds: float | None
    path: str
    metrics: FrameMetrics


@dataclass(frozen=True, slots=True)
class QaResult:
    status: Literal["pass", "warn", "fail"]
    frames: list[QaFrame]
    findings: list[Finding]
    artifacts: list[RuntimeArtifact]


def qa(task: QaTask, ctx: Context) -> QaResult:
    pil = require_pillow()
    beats = load_timeline(task.timeline, ctx)
    samples: list[tuple[float | None, Path]] = []
    artifacts: list[RuntimeArtifact] = []
    info: VideoInfo | None = None
    if task.source_kind == "image":
        samples.append((None, task.source))
    else:
        frames_dir = ensure_dir(ctx.require_inside(task.out_dir, "out_dir") / "frames")
        info = probe_video(task.source)
        times = [info.frame_time(t) for t in sample_times(info.duration_seconds, task.frames)]
        for number, at in enumerate(times, start=1):
            path = frames_dir / f"frame-{number:02d}.png"
            with atomic_target(path) as temp:
                temp.write_bytes(grab_frame(task.source, at, info))
            samples.append((round(at, 6), path))
            artifacts.append(ctx.artifact(ArtifactKind.IMAGE, path))
            ctx.progress("extract", number, len(times))
    frames: list[QaFrame] = []
    findings: list[Finding] = []
    for number, (at, path) in enumerate(samples, start=1):
        try:
            with pil.open(path) as image:
                metrics = measure(image.convert("RGB"), task.safe_area)
        except (OSError, pil.DecompressionBombError) as exc:
            raise invalid_source(path, "it is not a readable image") from exc
        frame = QaFrame(at_seconds=at, path=ctx.relative(path), metrics=metrics)
        frames.append(frame)
        findings += _findings(frame, beats)
        ctx.progress("analyze", number, len(samples))
    if info is not None and beats is not None and beats.version >= 2:  # v2 records pacing
        paced, sheet = _pacing(task, info, beats, frames, ctx)
        findings += paced
        artifacts.append(sheet)
    severities = {finding.severity for finding in findings}
    status = "fail" if Severity.ERROR in severities else "warn" if severities else "pass"
    return QaResult(status=status, frames=frames, findings=findings, artifacts=artifacts)


def _pacing(
    task: QaTask, info: VideoInfo, beats: Timeline, frames: list[QaFrame], ctx: Context
) -> tuple[list[Finding], RuntimeArtifact]:
    """Pacing findings, each pointing at the sampled frame nearest to it, and `beats.png`."""

    style = load_style(ctx.project_root)
    viewer = style.viewer
    budgets = pacing.settings(viewer.level if viewer is not None else "general", style.pacing)
    storyboard = style.storyboard_of(beats.scene)
    found = pacing.check(beats, budgets, viewer, storyboard)

    def nearest(at: float | None) -> str | None:
        if at is None:
            return None
        return min(frames, key=lambda frame: abs((frame.at_seconds or 0.0) - at)).path

    path = ctx.require_inside(task.out_dir, "out_dir") / "beats.png"
    beat_sheet(task.source, info, _tiles(beats, storyboard, viewer, info), path, ctx)
    findings = [replace(finding, frame=nearest(finding.at_seconds)) for finding in found]
    return findings, ctx.artifact(ArtifactKind.CONTACT_SHEET, path, label="beats")


def _tiles(
    beats: Timeline, storyboard: Sequence[StoryBeat], viewer: Viewer | None, info: VideoInfo
) -> list[BeatTile]:
    """A tile per beat at its last still (what the beat leaves the viewer with), else its
    last frame, under its audience question, and before the aha's tile its motion starting
    and halfway, under the aha; the final frame closes the sheet under the viewer's own
    question."""

    frame = 1 / info.fps if info.fps else 0.0
    tiles = []
    for plan in pacing.planned(beats, storyboard)[: _SHEET_TILES - 3]:
        beat = plan.beat
        stills = [settle for settle in beats.settles if settle.beat == beat.id]
        last = stills[-1] if stills else None
        at = last.at + last.still_seconds / 2 if last else beat.end_seconds - frame
        detail = f"{beat.end_seconds - beat.start_seconds:.1f} s" + (" · aha" if plan.aha else "")
        motions = [e for e in beats.events if e.beat == beat.id] if plan.aha else []
        if motions:  # the aha's longest motion, starting and halfway: does it show the idea?
            aha = viewer.aha if viewer is not None else None
            motion = max(motions, key=lambda e: e.seconds)
            for share, moment in ((0.0, "motion starts"), (0.5, "mid-motion")):
                at_motion = info.frame_time(motion.at + share * motion.seconds + frame)
                tiles.append(BeatTile(at_motion, beat.id, moment, aha or plan.question))
        tiles.append(BeatTile(info.frame_time(at), beat.id, detail, plan.question))
    question = viewer.question if viewer is not None else None
    end = f"{info.duration_seconds:.1f} s"
    return [*tiles, BeatTile(info.frame_time(info.duration_seconds), "final frame", end, question)]


def measure(image: Image, safe_area: SafeArea) -> FrameMetrics:
    from PIL import Image as PILImage
    from PIL import ImageChops, ImageFilter, ImageStat

    width, height = image.size
    grey = image.convert("L")
    stat = ImageStat.Stat(grey)
    corners = [
        image.getpixel(xy)
        for xy in ((0, 0), (width - 1, 0), (0, height - 1), (width - 1, height - 1))
    ]
    background = tuple(round(statistics.median(pixel[c] for pixel in corners)) for c in range(3))
    delta = ImageChops.difference(image, PILImage.new("RGB", image.size, background)).convert("L")
    foreground = delta.point(lambda value: 255 if value > _FOREGROUND_DELTA else 0)
    area = width * height
    content = foreground.histogram()[255]
    # Legibility is about text and strokes: an opening (erode, then dilate, by box blurs)
    # finds the large flat regions, such as soft fills, that would outvote them.
    box = ImageFilter.BoxBlur(max(2, round(min(width, height) / 120)))
    core = foreground.filter(box).point(lambda value: 255 if value > 254 else 0)
    detail = ImageChops.subtract(foreground, core.filter(box).point(lambda v: 255 if v else 0))
    mask = detail if detail.histogram()[255] else foreground
    ink = [_median_level(band.histogram(mask=mask)) for band in image.split()]

    band = max(1, round(min(width, height) * 0.012))
    edges = [
        (0, 0, width, band),
        (0, height - band, width, height),
        (0, band, band, height - band),
        (width - band, band, width, height - band),
    ]
    edge_pixels = sum(foreground.crop(box).histogram()[255] for box in edges)
    edge_area = 2 * width * band + 2 * max(0, height - 2 * band) * band
    safe = PILImage.new("L", image.size, 0)
    safe.paste(
        255,
        (
            round(width * safe_area.left),
            round(height * safe_area.top),
            round(width * (1 - safe_area.right)),
            round(height * (1 - safe_area.bottom)),
        ),
    )
    unsafe = ImageChops.subtract(foreground, safe).histogram()[255]
    bbox = foreground.getbbox()
    return FrameMetrics(
        mean_luminance=round(stat.mean[0], 3),
        luminance_stddev=round(stat.stddev[0], 3),
        contrast_ratio=round(contrast_ratio(ink, background) if content else 1.0, 3),
        foreground_fraction=round(content / area, 6),
        unsafe_fraction=round(unsafe / area, 6),
        edge_activity=round(edge_pixels / max(1, edge_area), 6),
        foreground_bbox=list(bbox) if bbox else None,
        background_rgb=list(background),
    )


def _findings(frame: QaFrame, beats: Timeline | None) -> list[Finding]:
    m = frame.metrics
    checks: list[tuple[str, Severity, str, str]] = []
    if m.foreground_bbox is None or m.foreground_fraction < 0.0001:
        checks.append(
            (
                "blank_frame",
                Severity.ERROR,
                "The frame is blank or nearly uniform.",
                "Check that the scene shows content at this time.",
            )
        )
    elif m.contrast_ratio < _MIN_CONTRAST:
        checks.append(
            (
                "low_contrast",
                Severity.WARNING,
                f"Content contrast against the background is {m.contrast_ratio:.1f}:1.",
                "Use the theme foreground or primary colors; small text needs at least 4.5:1.",
            )
        )
    share = m.unsafe_fraction / m.foreground_fraction if m.foreground_fraction else 0.0
    if (
        share > _MAX_UNSAFE_SHARE
        or m.unsafe_fraction > _MAX_UNSAFE_FRACTION
        or m.edge_activity > _MAX_EDGE_ACTIVITY
    ):
        where = "reaches the frame edge" if m.edge_activity else "extends outside the safe area"
        checks.append(
            (
                "safe_area",
                Severity.WARNING,
                f"Content {where}: {share:.0%} of it lies outside the safe area.",
                "Scale or move content inward; the margins come from safe_area in director.yaml.",
            )
        )
    beat = beats.beat_at(frame.at_seconds) if beats and frame.at_seconds is not None else None
    location = SourceLocation(beat.file, beat.line) if beat else None
    return [
        Finding(
            code=code,
            severity=severity,
            message=message,
            hint=hint,
            location=location,
            at_seconds=frame.at_seconds,
            beat=beat.id if beat else None,
            frame=frame.path,
        )
        for code, severity, message, hint in checks
    ]


def _median_level(histogram: list[int]) -> int:
    half, running = sum(histogram) / 2, 0
    for level, count in enumerate(histogram):
        running += count
        if running >= half:
            return level
    return len(histogram) - 1


def contrast_ratio(first: Sequence[float], second: Sequence[float]) -> float:
    """WCAG 2 contrast ratio of two sRGB colors (channels 0-255)."""

    def luminance(rgb: Sequence[float]) -> float:
        srgb = [c / 255 for c in rgb]
        linear = [c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in srgb]
        return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]

    high, low = sorted((luminance(first), luminance(second)), reverse=True)
    return (high + 0.05) / (low + 0.05)
