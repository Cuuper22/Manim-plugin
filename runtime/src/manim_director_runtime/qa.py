"""The `qa` operation: measure sampled frames for blankness, contrast and safe-area use."""

from __future__ import annotations

import statistics
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Literal

from .errors import invalid_source
from .media import grab_frame, load_timeline, probe_video, require_pillow, sample_times
from .model import ArtifactKind, Finding, RuntimeArtifact, Severity, SourceLocation
from .paths import atomic_target, ensure_dir
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
    if task.source_kind == "image":
        samples.append((None, task.source))
    else:
        frames_dir = ensure_dir(ctx.require_inside(task.out_dir, "out_dir") / "frames")
        info = probe_video(task.source)
        # Judge the states the stage settles in, not the moves between them.
        settled = sample_times(
            info.duration_seconds, task.frames, beats.transitions if beats else ()
        )
        times = [info.frame_time(t) for t in settled]
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
    severities = {finding.severity for finding in findings}
    status = "fail" if Severity.ERROR in severities else "warn" if severities else "pass"
    return QaResult(status=status, frames=frames, findings=findings, artifacts=artifacts)


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
