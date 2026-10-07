from __future__ import annotations

import json
import zipfile
from pathlib import Path

from conftest import as_json, make_video, requires_ffmpeg
from manim_director_runtime.exporting import export, gif_delay_centiseconds
from manim_director_runtime.tasks import ExportEntry, GifOptions, MediaExportTask, ZipExportTask


def test_zip_skips_vanished_entries_and_writes_its_manifest(project: Path, ctx) -> None:
    (project / "director.yaml").write_text("version: 1\n")
    (project / "scenes").mkdir()
    (project / "scenes/main.py").write_text("print('hi')\n")
    task = ZipExportTask(
        format="zip",
        output=project / "output/film.zip",
        project_name="Film",
        source_job_id=None,
        entries=[
            ExportEntry(project / "director.yaml", "director.yaml"),
            ExportEntry(project / "scenes/main.py", "scenes/main.py"),
            ExportEntry(project / "gone.mp4", "deliverables/gone.mp4"),
        ],
    )
    result = as_json(export(task, ctx))
    assert result == {
        "format": "zip",
        "transcoded": False,
        "effective_fps": None,
        "files": 2,
        "uncompressed_bytes": 23,
        "missing": ["deliverables/gone.mp4"],
        "artifacts": [{"kind": "archive", "path": "output/film.zip", "label": None}],
    }
    with zipfile.ZipFile(project / "output/film.zip") as archive:
        assert sorted(archive.namelist()) == [
            "director.yaml",
            "manim-director-export.json",
            "scenes/main.py",
        ]
        manifest = json.loads(archive.read("manim-director-export.json"))
    assert manifest["project"] == "Film" and manifest["missing"] == ["deliverables/gone.mp4"]
    assert manifest["files"] == [
        {"path": "director.yaml", "bytes": 11},
        {"path": "scenes/main.py", "bytes": 12},
    ]
    assert [p.name for p in (project / "output").iterdir()] == ["film.zip"]


def test_same_format_media_is_copied_byte_for_byte(project: Path, ctx) -> None:
    source = project / "render.mp4"
    source.write_bytes(b"not really a video")
    task = MediaExportTask(
        format="mp4", output=project / "output/final.mp4", source=source, alpha=False, gif=None
    )
    result = as_json(export(task, ctx))
    assert result["transcoded"] is False and result["files"] is None
    assert (project / "output/final.mp4").read_bytes() == b"not really a video"


@requires_ffmpeg
def test_gif_export_uses_a_representable_frame_delay(project: Path, ctx) -> None:
    source = make_video(project / "render.mp4", seconds=1.0, size="320x180", fps=30)
    task = MediaExportTask(
        format="gif",
        output=project / "output/loop.gif",
        source=source,
        alpha=False,
        gif=GifOptions(fps=15, width=160),
    )
    result = as_json(export(task, ctx))
    assert result["transcoded"] is True
    assert result["effective_fps"] == 100 / 7
    from PIL import Image

    with Image.open(project / "output/loop.gif") as gif:
        assert gif.size == (160, 90)
        assert gif.info["duration"] == 70


def test_gif_delays_are_whole_centiseconds() -> None:
    assert [gif_delay_centiseconds(fps) for fps in (1, 15, 30, 50)] == [100, 7, 3, 2]
