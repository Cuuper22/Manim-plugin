from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest

from conftest import request, requires_latex, requires_manim, run_bridge
from manim_director_runtime.errors import DirectorError
from manim_director_runtime.rendering import resolve_scene

SETTINGS = {
    "profile": "draft",
    "width": 320,
    "height": 180,
    "fps": 10,
    "renderer": "cairo",
    "format": "mp4",
    "transparent": False,
}

DIRECTED = """from manim import *
from manim_director_runtime import Beat, DirectedScene


def beat(focus):
    return Beat(intent="explain", audience_question="Which shape?", takeaway="This one.",
                focus=focus, visual_metaphor="shapes")


class Shapes(DirectedScene):
    def construct(self):
        print("this must not reach the protocol stream")
        self.next_section("circle")
        self.beat(beat("circle"), Circle(), keys=("circle",), run_time=0.3)
        self.wait(0.2)
        self.next_section("square")
        self.beat(beat("square"), Square(), keys=("square",), run_time=0.3)
        self.wait(0.2)
"""


def write_scene(project: Path, source: str, relative: str = "scenes/main.py") -> Path:
    path = project / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(source, encoding="utf-8")
    return path


def render_request(project: Path, scene: str | None, files: list[Path], **overrides) -> dict:
    params = {
        "scene": scene,
        "files": [str(f) for f in files],
        "settings": SETTINGS,
        "media_dir": str(project / ".manim-director/media"),
        "out_dir": str(project / ".manim-director/artifacts/job"),
        "sections": False,
        "fresh": False,
    }
    return request("render", project, {**params, **overrides}, request_id="job")


def probe(path: Path) -> dict:
    args = "-v error -select_streams v:0 -show_entries stream=width,height,avg_frame_rate -of json"
    output = subprocess.run(
        ["ffprobe", *args.split(), str(path)], capture_output=True, check=True
    ).stdout
    return json.loads(output)["streams"][0]


@requires_manim
def test_render_moves_video_sections_and_beat_timeline_into_out_dir(project: Path) -> None:
    scene = write_scene(project, DIRECTED)
    frames, completed = run_bridge(
        project, render_request(project, "Shapes", [scene], sections=True), preload=True
    )
    assert completed.returncode == 0, completed.stderr.decode()[-2000:]
    assert b"must not reach" in completed.stderr
    assert {frame["type"] for frame in frames} <= {"ready", "progress", "log", "result"}
    assert {frame["phase"] for frame in frames if frame["type"] == "progress"} >= {"import"}
    result = frames[-1]["result"]
    out = ".manim-director/artifacts/job"
    assert result["scene"] == {"name": "Shapes", "file": "scenes/main.py"}
    assert result["animations"] == 4
    assert result["duration_seconds"] == pytest.approx(1.0)
    assert [(a["kind"], a["path"], a["label"]) for a in result["artifacts"]] == [
        ("video", f"{out}/Shapes.mp4", None),
        ("section", f"{out}/sections/0001-circle.mp4", "circle"),
        ("section", f"{out}/sections/0002-square.mp4", "square"),
        ("timeline", f"{out}/Shapes.timeline.json", None),
    ]
    assert probe(project / out / "Shapes.mp4") == {
        "width": 320,
        "height": 180,
        "avg_frame_rate": "10/1",
    }
    timeline = json.loads((project / out / "Shapes.timeline.json").read_text())
    assert timeline["version"] == 1 and timeline["scene"] == "Shapes"
    assert [(b["id"], b["file"], b["line"]) for b in timeline["beats"]] == [
        ("beat-1", "scenes/main.py", 14),
        ("beat-2", "scenes/main.py", 17),
    ]
    assert timeline["beats"][0]["start_seconds"] == 0.0
    assert timeline["beats"][1]["end_seconds"] == pytest.approx(timeline["duration_seconds"])


@requires_manim
def test_directed_scenes_get_a_timeline_without_sections(project: Path) -> None:
    scene = write_scene(project, DIRECTED)
    frames, _ = run_bridge(project, render_request(project, "Shapes", [scene]))
    kinds = [artifact["kind"] for artifact in frames[-1]["result"]["artifacts"]]
    assert kinds == ["video", "timeline"]


@requires_manim
def test_still_renders_the_last_frame_at_the_task_size(project: Path) -> None:
    scene = write_scene(project, DIRECTED)
    params = render_request(project, None, [scene])["params"]
    del params["sections"]
    params["settings"] = {**SETTINGS, "width": 400, "height": 400, "format": "png"}
    frames, completed = run_bridge(project, request("still", project, params, request_id="job"))
    assert completed.returncode == 0, completed.stderr.decode()[-2000:]
    (artifact,) = frames[-1]["result"]["artifacts"]
    assert artifact == {
        "kind": "image",
        "path": ".manim-director/artifacts/job/Shapes.png",
        "label": None,
    }
    from PIL import Image

    with Image.open(project / artifact["path"]) as image:
        assert image.size == (400, 400)


@requires_manim
def test_construct_errors_point_at_the_scene_line(project: Path) -> None:
    source = (
        "from manim import *\n\nclass Broken(Scene):\n"
        "    def construct(self):\n        self.play(Create(rowz))\n"
    )
    scene = write_scene(project, source)
    frames, _ = run_bridge(project, render_request(project, "Broken", [scene]))
    error = frames[-1]["error"]
    assert error["code"] == "render_failed"
    assert error["message"] == "Broken.construct raised NameError: name 'rowz' is not defined."
    assert (error["data"]["stage"], error["data"]["exception"]) == ("construct", "NameError")
    finding = error["data"]["findings"][0]
    assert finding["code"] == "python_name"
    assert finding["location"] == {"file": "scenes/main.py", "line": 5, "column": None}
    assert "Traceback" in error["data"]["traceback"]


@requires_manim
@requires_latex
def test_latex_errors_point_at_the_tex_call(project: Path) -> None:
    source = (
        "from manim import *\n\nclass Formula(Scene):\n    def construct(self):\n"
        "        eq = MathTex(r'\\phii = 1')\n        self.play(Write(eq))\n"
    )
    scene = write_scene(project, source)
    frames, _ = run_bridge(project, render_request(project, "Formula", [scene]))
    error = frames[-1]["error"]
    assert error["code"] == "render_failed"
    finding = error["data"]["findings"][0]
    assert (finding["code"], finding["message"]) == ("latex_error", "Undefined control sequence.")
    assert "\\phii" in finding["hint"]
    assert finding["location"] == {"file": "scenes/main.py", "line": 5, "column": None}


@requires_manim
def test_a_scene_without_animations_has_no_video(project: Path) -> None:
    source = (
        "from manim import *\n\nclass Static(Scene):\n"
        "    def construct(self):\n        self.add(Circle())\n"
    )
    frames, _ = run_bridge(
        project, render_request(project, "Static", [write_scene(project, source)])
    )
    error = frames[-1]["error"]
    assert (error["code"], error["data"]["stage"]) == ("render_failed", "write")
    assert "no animations" in error["message"]


def test_scene_resolution_by_ast(project: Path, ctx) -> None:
    one = write_scene(
        project, "from manim import Scene\nclass A(Scene):\n    pass\nclass B(A):\n    pass\n"
    )
    two = write_scene(
        project, "from manim import Scene\nclass A(Scene):\n    pass\n", "other/two.py"
    )
    assert resolve_scene("B", [one, two], ctx).path == one
    cases = [
        ("Missing", [one], "scene_not_found", {"scene": "Missing", "available": ["A", "B"]}),
        (None, [one], "scene_required", {"available": ["A", "B"]}),
        (
            "A",
            [one, two],
            "scene_ambiguous",
            {"scene": "A", "files": ["scenes/main.py", "other/two.py"]},
        ),
    ]
    for scene, files, code, data in cases:
        with pytest.raises(DirectorError) as raised:
            resolve_scene(scene, files, ctx)
        assert (raised.value.code, raised.value.data) == (code, data)
    assert resolve_scene(None, [two], ctx).name == "A"


def test_syntax_errors_fail_resolution_with_the_exact_location(project: Path, ctx) -> None:
    broken = write_scene(
        project, "from manim import *\nclass A(Scene):\n    def construct(self:\n        pass\n"
    )
    with pytest.raises(DirectorError) as raised:
        resolve_scene("A", [broken], ctx)
    error = raised.value
    assert error.code == "render_failed"
    assert (error.data["stage"], error.data["exception"]) == ("import", "SyntaxError")
    location = error.data["findings"][0].location
    assert (location.file, location.line, location.column) == ("scenes/main.py", 3, 18)
