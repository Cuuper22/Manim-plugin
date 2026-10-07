from __future__ import annotations

import json
import subprocess
from concurrent.futures import ThreadPoolExecutor
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
from manim_director_runtime import DirectedScene


class Shapes(DirectedScene):
    def construct(self):
        print("this must not reach the protocol stream")
        with self.beat("circle", run_time=0.3, hold=0.2):
            self.place(Circle())
        with self.beat("square", run_time=0.3, hold=0.2):
            self.place(Square())
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
    # Manim's records are whole lines (Rich wrapped them at 80 columns into fragments).
    written = [line for line in completed.stderr.decode().splitlines() if "file written" in line]
    assert written and all(line.startswith("INFO ") and "media" in line for line in written)
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
        ("circle", "scenes/main.py", 8),
        ("square", "scenes/main.py", 10),
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
    assert "\\phii" in finding["hint"] and "unique" not in finding["hint"]
    assert finding["location"] == {"file": "scenes/main.py", "line": 5, "column": None}


@requires_manim
def test_failures_without_a_project_frame_point_at_the_scene_class(project: Path) -> None:
    source = (
        "from manim import *\nfrom manim_director_runtime import DirectedScene\n\n\n"
        "class Neon(DirectedScene):\n    theme = 'neon'\n\n    def construct(self):\n"
        "        self.wait()\n"
    )
    frames, _ = run_bridge(project, render_request(project, "Neon", [write_scene(project, source)]))
    (finding,) = frames[-1]["error"]["data"]["findings"]
    assert finding["code"] == "composition" and finding["hint"] is None
    assert finding["location"] == {"file": "scenes/main.py", "line": 5, "column": None}


@requires_manim
def test_authoring_errors_point_at_the_scene_line(project: Path) -> None:
    source = (
        "from manim import *\nfrom manim_director_runtime import DirectedScene\n\n"
        "class Crowded(DirectedScene):\n    def construct(self):\n"
        "        self.place(Rectangle(width=100, height=1))\n"
    )
    frames, _ = run_bridge(
        project, render_request(project, "Crowded", [write_scene(project, source)])
    )
    error = frames[-1]["error"]
    assert (error["code"], error["data"]["stage"]) == ("render_failed", "construct")
    finding = error["data"]["findings"][0]
    assert finding["code"] == "composition"
    assert finding["message"].startswith("Rectangle needs 0.13x to fit the content region")
    assert finding["location"] == {"file": "scenes/main.py", "line": 6, "column": None}


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


@requires_manim
def test_a_malformed_manim_cfg_fails_the_setup_stage(project: Path) -> None:
    scene = write_scene(project, DIRECTED)
    (project / "manim.cfg").write_text("frame_rate = 30\n")  # no [CLI] section header
    frames, _ = run_bridge(project, render_request(project, "Shapes", [scene]))
    error = frames[-1]["error"]
    assert (error["code"], error["data"]["stage"]) == ("render_failed", "setup")
    assert error["data"]["exception"] == "MissingSectionHeaderError"
    location = error["data"]["findings"][0]["location"]
    assert location == {"file": "manim.cfg", "line": 1, "column": None}


@requires_manim
def test_a_missing_asset_is_named_at_the_scene_line(project: Path) -> None:
    source = (
        "from manim import *\n\nclass Badge(Scene):\n    def construct(self):\n"
        "        badge = SVGMobject('missing-badge.svg')\n        self.play(FadeIn(badge))\n"
    )
    frames, _ = run_bridge(
        project, render_request(project, "Badge", [write_scene(project, source)])
    )
    finding = frames[-1]["error"]["data"]["findings"][0]
    assert finding["code"] == "asset_missing"
    assert "missing-badge.svg" in finding["message"]
    assert finding["location"] == {"file": "scenes/main.py", "line": 5, "column": None}


@requires_manim
@requires_latex
def test_concurrent_renders_share_a_cold_tex_cache(project: Path) -> None:
    # Unguarded, workers typesetting the same formulas read each other's half-written
    # .dvi/.svg files and Manim's cleanup deletes the others' in-flight files.
    formulas = "".join(
        f"        self.add(MathTex(r'x^{{{i}}} + \\frac{{{i}}}{{y_{i}}}'))\n" for i in range(12)
    )
    scenes = [f"class Race{n}(Scene):\n    def construct(self):\n{formulas}" for n in "ABC"]
    path = write_scene(project, "from manim import *\n\n\n" + "\n\n".join(scenes))

    def still(scene: str) -> dict:
        params = render_request(project, scene, [path])["params"]
        del params["sections"]
        params["out_dir"] += scene
        params["settings"] = {**SETTINGS, "format": "png"}
        return run_bridge(project, request("still", project, params))[0][-1]

    with ThreadPoolExecutor(3) as pool:
        results = list(pool.map(still, ["RaceA", "RaceB", "RaceC"]))
    assert [frame["type"] for frame in results] == ["result"] * 3, results


@requires_manim
def test_beat_ids_with_path_separators_render_sections(project: Path) -> None:
    source = DIRECTED.replace('"circle"', '"proof/step-1"').replace('"square"', '"proof/step-2"')
    scene = write_scene(project, source)
    frames, _ = run_bridge(project, render_request(project, "Shapes", [scene], sections=True))
    sections = [a for a in frames[-1]["result"]["artifacts"] if a["kind"] == "section"]
    assert [a["path"].rsplit("/", 1)[-1] for a in sections] == [
        "0001-proof-step-1.mp4",
        "0002-proof-step-2.mp4",
    ]
