from __future__ import annotations

from pathlib import Path

import pytest

from manim_director_runtime.errors import DirectorError
from manim_director_runtime.protocol import METHODS
from manim_director_runtime.tasks import (
    SAFE_AREA_DEFAULT,
    MediaExportTask,
    ZipExportTask,
    parse_task,
)

P = "/p"
SETTINGS = {
    "profile": "preview",
    "width": 1280,
    "height": 720,
    "fps": 30,
    "renderer": "cairo",
    "format": "mp4",
    "transparent": False,
}
VALID = {
    "init": {
        "mode": "create",
        "name": "Film",
        "template": None,
        "scene_template": None,
        "theme": None,
        "seed": None,
        "source_dir": None,
    },
    "discover": {"files": [f"{P}/scenes/main.py"]},
    "doctor": {},
    "render": {
        "scene": "Demo",
        "files": [f"{P}/scenes/main.py"],
        "settings": SETTINGS,
        "media_dir": f"{P}/m",
        "out_dir": f"{P}/o",
        "sections": False,
        "fresh": False,
    },
    "still": {
        "scene": None,
        "files": [f"{P}/scenes/main.py"],
        "settings": {**SETTINGS, "format": "png"},
        "media_dir": f"{P}/m",
        "out_dir": f"{P}/o",
        "fresh": True,
    },
    "frame": {"video": f"{P}/v.mp4", "at_seconds": 1.5, "out_dir": f"{P}/o"},
    "contact_sheet": {
        "video": f"{P}/v.mp4",
        "count": 6,
        "columns": 3,
        "timeline": None,
        "out_dir": f"{P}/o",
    },
    "qa": {
        "source": f"{P}/v.mp4",
        "source_kind": "video",
        "frames": 8,
        "safe_area": {"top": 0.05, "right": 0.05, "bottom": 0.08, "left": 0.05},
        "timeline": f"{P}/t.json",
        "out_dir": f"{P}/o",
    },
    "diagnose": {"text": "Traceback"},
    "validate_math": {
        "steps": ["(a+b)^2", "a^2+2*a*b+b^2"],
        "ranges": {"a": [-3, 3]},
        "samples": 200,
        "tolerance": 1e-9,
        "seed": 73,
    },
    "captions": {"path": f"{P}/c.vtt", "shift_seconds": 0, "scale": 1.0, "output": None},
    "ingest": {
        "sources": [
            {
                "path": "/home/u/n.md",
                "kind": "markdown",
                "destination_dir": f"{P}/sources",
                "id": None,
                "license": None,
                "attribution": None,
            }
        ],
        "normalize": False,
        "force": False,
        "manifest": f"{P}/sources/manifest.json",
    },
    "export": {
        "format": "gif",
        "output": f"{P}/out.gif",
        "source": f"{P}/v.mp4",
        "alpha": False,
        "gif": {"fps": 15, "width": 960},
    },
}


def parse(method: str, raw: object):
    return parse_task(METHODS[method].task, raw)


def error_of(method: str, raw: object) -> dict:
    with pytest.raises(DirectorError) as raised:
        parse(method, raw)
    assert raised.value.code == "invalid_params"
    return raised.value.data or {}


@pytest.mark.parametrize("method", list(METHODS))
def test_every_method_accepts_its_contract_example(method: str) -> None:
    task = parse(method, VALID[method])
    assert type(task).__name__.endswith("Task")


@pytest.mark.parametrize("method", [m for m in METHODS if m != "doctor"])
def test_every_method_rejects_unknown_and_missing_fields(method: str) -> None:
    assert error_of(method, {**VALID[method], "extra": 1}) == {
        "field": "extra",
        "reason": "unknown field",
    }
    first = next(iter(VALID[method]))
    missing = {key: value for key, value in VALID[method].items() if key != first}
    assert error_of(method, missing) == {"field": first, "reason": "missing field"}


def test_doctor_task_takes_no_fields() -> None:
    assert error_of("doctor", {"project_root": "."})["field"] == "project_root"
    assert error_of("doctor", [])["reason"] == "must be an object"


@pytest.mark.parametrize(
    ("raw", "message"),
    [
        ({"project_root": "."}, "Invalid project_root: unknown field."),
        ([], "Invalid parameters: must be an object."),
    ],
)
def test_messages_name_the_parameter_plainly(raw: object, message: str) -> None:
    with pytest.raises(DirectorError) as raised:
        parse("doctor", raw)
    assert raised.value.message == message


@pytest.mark.parametrize(
    ("method", "patch", "field", "reason"),
    [
        (
            "render",
            {"settings": {**SETTINGS, "width": True}},
            "settings.width",
            "must be an integer",
        ),
        (
            "render",
            {"settings": {**SETTINGS, "renderer": "skia"}},
            "settings.renderer",
            "must be one of cairo, opengl",
        ),
        (
            "render",
            {"settings": {**SETTINGS, "format": "png"}},
            "settings.format",
            "png is only for still",
        ),
        (
            "render",
            {"settings": {**SETTINGS, "transparent": True}},
            "settings.transparent",
            "mp4 cannot carry an alpha channel",
        ),
        ("render", {"files": ["scenes/main.py"]}, "files[0]", "must be an absolute path"),
        ("render", {"files": []}, "files", "must not be empty"),
        ("still", {"settings": SETTINGS}, "settings.format", "a still is always png"),
        ("frame", {"at_seconds": -1}, "at_seconds", "must not be negative"),
        ("frame", {"at_seconds": "1"}, "at_seconds", "must be a finite number"),
        ("contact_sheet", {"count": 0}, "count", "must be at least 1"),
        ("qa", {"source_kind": "audio"}, "source_kind", "must be one of video, image"),
        (
            "qa",
            {"safe_area": {"top": 0.5, "right": 0, "bottom": 0, "left": 0}},
            "safe_area.top",
            "must be between 0 and 0.45",
        ),
        ("validate_math", {"steps": ["x"]}, "steps", "a derivation needs at least two steps"),
        (
            "validate_math",
            {"ranges": {"a": [3, -3]}},
            "ranges.a",
            "lower bound must be below the upper bound",
        ),
        ("validate_math", {"ranges": {"a": [1]}}, "ranges.a", "must be an array of 2 items"),
        ("validate_math", {"tolerance": 0}, "tolerance", "must be in (0, 1]"),
        ("captions", {"scale": 0}, "scale", "must be positive"),
        ("ingest", {"sources": []}, "sources", "must not be empty"),
        (
            "ingest",
            {"sources": [{**VALID["ingest"]["sources"][0], "kind": "zip"}]},
            "sources[0].kind",
            "must be one of markdown, latex, typst, text, csv, json, python, "
            "notebook, pdf, svg, image, audio, video, other",
        ),
        ("export", {"format": "avi"}, "format", "must be one of zip, mp4, webm, gif"),
        ("export", {"gif": None}, "gif", "is set iff format is gif"),
        ("init", {"seed": -1}, "seed", "must be between 0 and 2147483647"),
        (
            "init",
            {"mode": "add_scene"},
            "scene_template",
            "is required for add_scene and only allowed there",
        ),
    ],
)
def test_field_validation(method: str, patch: dict, field: str, reason: str) -> None:
    assert error_of(method, {**VALID[method], **patch}) == {"field": field, "reason": reason}


def test_add_scene_rejects_create_only_fields() -> None:
    raw = {
        **VALID["init"],
        "mode": "add_scene",
        "scene_template": "equation_derivation",
        "source_dir": f"{P}/scenes",
    }
    assert error_of("init", raw)["field"] == "name"
    task = parse("init", {**raw, "name": None})
    assert task.force is False


def test_export_union_is_discriminated_by_format() -> None:
    media = parse("export", VALID["export"])
    assert isinstance(media, MediaExportTask) and media.gif and media.gif.fps == 15
    zipped = parse(
        "export",
        {
            "format": "zip",
            "output": f"{P}/out.zip",
            "project_name": "Film",
            "source_job_id": None,
            "entries": [{"path": f"{P}/director.yaml", "archive_path": "director.yaml"}],
        },
    )
    assert isinstance(zipped, ZipExportTask)
    assert zipped.entries[0].path == Path(f"{P}/director.yaml")


@pytest.mark.parametrize("archive_path", ["../escape.py", "/etc/passwd", "a//b", "a\\b", ""])
def test_zip_entries_stay_inside_the_archive(archive_path: str) -> None:
    raw = {
        "format": "zip",
        "output": f"{P}/out.zip",
        "project_name": "Film",
        "source_job_id": None,
        "entries": [{"path": f"{P}/director.yaml", "archive_path": archive_path}],
    }
    assert error_of("export", raw)["field"] == "entries[0].archive_path"


def test_ranges_parse_to_float_pairs() -> None:
    task = parse("validate_math", VALID["validate_math"])
    assert task.ranges == {"a": (-3.0, 3.0)}


def test_safe_area_default_matches_the_contract() -> None:
    assert (
        SAFE_AREA_DEFAULT.top,
        SAFE_AREA_DEFAULT.right,
        SAFE_AREA_DEFAULT.bottom,
        SAFE_AREA_DEFAULT.left,
    ) == (0.05, 0.05, 0.08, 0.05)
