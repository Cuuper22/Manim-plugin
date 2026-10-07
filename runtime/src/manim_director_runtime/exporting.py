"""The `export` operation: a project zip, or one video delivered as mp4, webm or gif."""

from __future__ import annotations

import json
import shutil
import zipfile
from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from . import __version__, process
from .errors import io_error
from .model import ArtifactKind, RuntimeArtifact
from .paths import atomic_target
from .tasks import ExportTask, MediaExportTask, ZipExportTask

if TYPE_CHECKING:
    from .protocol import Context

EXPORT_MANIFEST = "manim-director-export.json"
MAX_MISSING_LISTED = 200


@dataclass(frozen=True, slots=True)
class ExportResult:
    format: Literal["zip", "mp4", "webm", "gif"]
    transcoded: bool
    effective_fps: float | None
    files: int | None
    uncompressed_bytes: int | None
    missing: list[str] | None
    artifacts: list[RuntimeArtifact]


def export(task: ExportTask, ctx: Context) -> ExportResult:
    if isinstance(task, ZipExportTask):
        return _export_zip(task, ctx)
    return _export_media(task, ctx)


def gif_delay_centiseconds(fps: int) -> int:
    """GIF frame delays are whole centiseconds; pick the nearest representable cadence."""

    return min(100, max(2, round(100 / fps)))


def _export_zip(task: ZipExportTask, ctx: Context) -> ExportResult:
    output = ctx.require_inside(task.output, "output")
    files: list[dict[str, object]] = []
    missing: list[str] = []
    total = 0
    with atomic_target(output) as temp, zipfile.ZipFile(temp, "w", zipfile.ZIP_DEFLATED) as archive:
        for done, entry in enumerate(task.entries, start=1):
            if not entry.path.is_file():
                missing.append(entry.archive_path)
                continue
            try:
                archive.write(entry.path, entry.archive_path)
            except FileNotFoundError:  # vanished between the check and the read
                missing.append(entry.archive_path)
                continue
            except OSError as exc:
                raise io_error(entry.path, exc) from exc
            size = archive.getinfo(entry.archive_path).file_size
            files.append({"path": entry.archive_path, "bytes": size})
            total += size
            ctx.progress("package", done, len(task.entries))
        manifest = {
            "version": 1,
            "project": task.project_name,
            "source_job_id": task.source_job_id,
            "runtime_version": __version__,
            "files": files,
            "missing": missing,
        }
        archive.writestr(EXPORT_MANIFEST, json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
    return ExportResult(
        format="zip",
        transcoded=False,
        effective_fps=None,
        files=len(files),
        uncompressed_bytes=total,
        missing=missing[:MAX_MISSING_LISTED],
        artifacts=[ctx.artifact(ArtifactKind.ARCHIVE, output)],
    )


def _export_media(task: MediaExportTask, ctx: Context) -> ExportResult:
    output = ctx.require_inside(task.output, "output")
    effective_fps = None
    transcoded = task.source.suffix.lower() != f".{task.format}"
    with atomic_target(output) as temp:
        if not transcoded:
            try:
                shutil.copyfile(task.source, temp)
            except OSError as exc:
                raise io_error(task.source, exc) from exc
        else:
            ctx.progress("transcode", 0)
            args = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i", str(task.source)]
            if task.format == "mp4":
                args += [
                    "-c:v",
                    "libx264",
                    "-crf",
                    "18",
                    "-pix_fmt",
                    "yuv420p",
                    "-movflags",
                    "+faststart",
                    "-c:a",
                    "aac",
                ]
            elif task.format == "webm":
                args += ["-c:v", "libvpx-vp9", "-crf", "30", "-b:v", "0", "-c:a", "libopus"]
                if task.alpha:
                    args += ["-pix_fmt", "yuva420p"]
            else:
                assert task.gif is not None
                delay = gif_delay_centiseconds(task.gif.fps)
                effective_fps = 100 / delay
                graph = (
                    f"fps=100/{delay},scale='min({task.gif.width},iw)':-2:flags=lanczos,"
                    "split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=sierra2_4a"
                )
                args += ["-filter_complex", graph, "-loop", "0"]
            process.run("ffmpeg", [*args, "-f", task.format, str(temp)])
    return ExportResult(
        format=task.format,
        transcoded=transcoded,
        effective_fps=effective_fps,
        files=None,
        uncompressed_bytes=None,
        missing=None,
        artifacts=[ctx.artifact(ArtifactKind.VIDEO, output)],
    )
