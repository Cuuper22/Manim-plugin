from __future__ import annotations

import json
from pathlib import Path

import pytest

from conftest import as_json, make_video, requires_ffmpeg
from manim_director_runtime.media import contact_sheet, frame
from manim_director_runtime.qa import qa
from manim_director_runtime.tasks import SAFE_AREA_DEFAULT, ContactSheetTask, FrameTask, QaTask

pytestmark = requires_ffmpeg


@pytest.fixture
def video(project: Path) -> Path:
    return make_video(project / ".manim-director/artifacts/src/Demo.mp4", seconds=2.0, fps=10)


@pytest.fixture
def timeline(project: Path) -> Path:
    path = project / ".manim-director/artifacts/src/Demo.timeline.json"
    beats = [
        {
            "id": "hook",
            "start_seconds": 0.0,
            "end_seconds": 1.0,
            "file": "scenes/main.py",
            "line": 20,
        },
        {
            "id": "rule",
            "start_seconds": 1.0,
            "end_seconds": 2.0,
            "file": "scenes/main.py",
            "line": 30,
        },
        {
            "id": "detail",
            "start_seconds": 1.5,
            "end_seconds": 1.9,
            "file": "scenes/main.py",
            "line": 33,
        },
    ]
    path.write_text(
        json.dumps({"version": 1, "scene": "Demo", "duration_seconds": 2.0, "beats": beats})
    )
    return path


def png_size(path: Path) -> tuple[int, int]:
    from PIL import Image

    with Image.open(path) as image:
        return image.size


def test_frame_grabs_the_frame_on_screen_and_clamps_to_the_end(
    project: Path, ctx, video: Path
) -> None:
    out = project / ".manim-director/artifacts/f1"
    result = as_json(frame(FrameTask(video=video, at_seconds=1.25, out_dir=out), ctx))
    assert result["at_seconds"] == 1.2
    assert result["artifacts"] == [
        {"kind": "image", "path": ".manim-director/artifacts/f1/frame-00001250.png", "label": None}
    ]
    assert png_size(out / "frame-00001250.png") == (320, 180)
    end = as_json(frame(FrameTask(video=video, at_seconds=2.0, out_dir=out), ctx))
    assert end["at_seconds"] == 1.9
    assert (out / "frame-00002000.png").stat().st_size > 0


def test_contact_sheet_labels_frames_with_beats(
    project: Path, ctx, video: Path, timeline: Path
) -> None:
    out = project / ".manim-director/artifacts/cs"
    task = ContactSheetTask(video=video, count=4, columns=3, timeline=timeline, out_dir=out)
    result = as_json(contact_sheet(task, ctx))
    assert (result["columns"], result["rows"]) == (3, 2)
    assert [(f["at_seconds"], f["beat"]) for f in result["frames"]] == [
        (0.1, "hook"),
        (0.7, "hook"),
        (1.2, "rule"),
        (1.8, "detail"),
    ]
    assert result["artifacts"][0]["kind"] == "contact_sheet"
    width, _ = png_size(out / "contact-sheet.png")
    assert width == 3 * 480 + 4 * 12


def test_qa_keeps_frames_and_maps_findings_to_beats(
    project: Path, ctx, video: Path, timeline: Path
) -> None:
    out = project / ".manim-director/artifacts/qa"
    task = QaTask(
        source=video,
        source_kind="video",
        frames=3,
        safe_area=SAFE_AREA_DEFAULT,
        timeline=timeline,
        out_dir=out,
    )
    result = as_json(qa(task, ctx))
    assert [frame["path"] for frame in result["frames"]] == [
        f".manim-director/artifacts/qa/frames/frame-0{n}.png" for n in (1, 2, 3)
    ]
    assert [a["path"] for a in result["artifacts"]] == [f["path"] for f in result["frames"]]
    # The full-bleed test pattern fills the frame edges.
    safe = [f for f in result["findings"] if f["code"] == "safe_area"]
    assert len(safe) == 3 and result["status"] == "warn"
    assert [(f["beat"], f["location"]) for f in safe] == [
        ("hook", {"file": "scenes/main.py", "line": 20, "column": None}),
        ("rule", {"file": "scenes/main.py", "line": 30, "column": None}),
        ("detail", {"file": "scenes/main.py", "line": 33, "column": None}),
    ]
    assert safe[0]["frame"] == result["frames"][0]["path"]


def test_qa_on_images(project: Path, ctx) -> None:
    from PIL import Image, ImageDraw

    blank = project / "blank.png"
    Image.new("RGB", (320, 180), "#0B1020").save(blank)
    faint = project / "faint.png"
    image = Image.new("RGB", (320, 180), "#0B1020")
    ImageDraw.Draw(image).rectangle((120, 60, 200, 120), fill="#1E2436")
    image.save(faint)
    clean = project / "clean.png"
    image = Image.new("RGB", (320, 180), "#0B1020")
    ImageDraw.Draw(image).rectangle((120, 60, 200, 120), fill="#F7F8FC")
    image.save(clean)
    outcomes = {}
    for path in (blank, faint, clean):
        task = QaTask(
            source=path,
            source_kind="image",
            frames=1,
            safe_area=SAFE_AREA_DEFAULT,
            timeline=None,
            out_dir=project / "unused",
        )
        result = as_json(qa(task, ctx))
        assert result["artifacts"] == [] and result["frames"][0]["at_seconds"] is None
        assert result["frames"][0]["path"] == path.name
        outcomes[path.name] = (result["status"], [f["code"] for f in result["findings"]])
    assert outcomes == {
        "blank.png": ("fail", ["blank_frame"]),
        "faint.png": ("warn", ["low_contrast"]),
        "clean.png": ("pass", []),
    }
    assert not (project / "unused").exists()


def strokes(draw, xs, top: int, bottom: int, fill: str = "#F7F8FC") -> None:
    """Thin vertical bars: as sparse as text or a formula."""

    for x in xs:
        draw.rectangle((x, top, x + 1, bottom), fill=fill)


@pytest.mark.parametrize(
    ("name", "paint", "codes"),
    [
        ("clipped_left", lambda d: strokes(d, range(0, 40, 4), 80, 100), ["safe_area"]),
        ("title_at_top", lambda d: strokes(d, range(100, 220, 6), 1, 12), ["safe_area"]),
        ("centred", lambda d: strokes(d, range(100, 220, 6), 80, 100), []),
        # Rec.601 luma rated this 2.1:1; the WCAG ratio of pure red on black is 5.25:1.
        ("red", lambda d: strokes(d, range(100, 220, 6), 80, 100, "#FF0000"), []),
        (
            "soft_fill",  # a large translucent-looking fill must not outvote the text on it
            lambda d: (
                d.rectangle((40, 30, 200, 150), fill="#1F2738"),
                strokes(d, range(220, 280, 6), 80, 100),
            ),
            [],
        ),
    ],
)
def test_qa_judges_safe_area_and_contrast_by_the_content(
    project: Path, ctx, name, paint, codes
) -> None:
    from PIL import Image, ImageDraw

    path = project / f"{name}.png"
    image = Image.new("RGB", (320, 180), "#0B1020" if name != "red" else "#000000")
    paint(ImageDraw.Draw(image))
    image.save(path)
    task = QaTask(path, "image", 1, SAFE_AREA_DEFAULT, None, project / "unused")
    result = as_json(qa(task, ctx))
    assert [f["code"] for f in result["findings"]] == codes, result["findings"]


def test_qa_rejects_an_unreadable_image(project: Path, ctx) -> None:
    from manim_director_runtime.errors import DirectorError

    broken = project / "broken.png"
    broken.write_bytes(b"not a png")
    task = QaTask(
        source=broken,
        source_kind="image",
        frames=1,
        safe_area=SAFE_AREA_DEFAULT,
        timeline=None,
        out_dir=project / "unused",
    )
    with pytest.raises(DirectorError) as raised:
        qa(task, ctx)
    assert (raised.value.code, raised.value.data) == ("invalid_source", {"path": str(broken)})


def test_contact_sheet_clock_rounds_before_splitting_minutes() -> None:
    from manim_director_runtime.media import _clock

    assert [_clock(t) for t in (5.04, 59.95, 119.99, 61.0)] == [
        "00:05.0",
        "01:00.0",
        "02:00.0",
        "01:01.0",
    ]
