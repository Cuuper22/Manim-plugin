from __future__ import annotations

from pathlib import Path

from conftest import as_json
from manim_director_runtime.inspection import discover
from manim_director_runtime.tasks import DiscoverTask

SCENE = '''from manim import *
from manim_director_runtime import DirectedScene


class Recurrence(DirectedScene):
    """Where does the next number come from?

    More detail.
    """

    theme = "paper"

    def helper(self):
        return 1

    def construct(self):
        with self.beat("hook", focus=None):
            self.next_section("roots")
            with self.beat(id="inner"):
                pass
        with self.beat(name):
            self.next_section(label)


class Base(Scene):
    pass
'''


def write(project: Path, relative: str, text: str | bytes) -> Path:
    path = project / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(text, bytes):
        path.write_bytes(text)
    else:
        path.write_text(text, encoding="utf-8")
    return path


def test_discover_reports_spans_sections_beats_and_theme(project: Path, ctx) -> None:
    main = write(project, "scenes/main.py", SCENE)
    result = as_json(discover(DiscoverTask(files=[main]), ctx))
    assert "files" not in result  # an engine-owned field
    assert result["truncated"] is False and result["findings"] == []
    recurrence, base = result["scenes"]
    assert recurrence == {
        "name": "Recurrence",
        "file": "scenes/main.py",
        "line": 5,
        "end_line": 22,
        "construct_line": 16,
        "bases": ["DirectedScene"],
        "doc": "Where does the next number come from?",
        "theme": "paper",
        "sections": [{"name": "roots", "line": 18}, {"name": None, "line": 22}],
        "beats": [
            {"id": "hook", "line": 17, "end_line": 20},
            {"id": "inner", "line": 19, "end_line": 20},
            {"id": None, "line": 21, "end_line": 22},
        ],
    }
    assert base["construct_line"] is None and base["theme"] is None and base["doc"] is None


def test_scene_bases_resolve_across_files(project: Path, ctx) -> None:
    base = write(
        project, "scenes/base.py", "import manim\nclass Base(manim.MovingCameraScene):\n    pass\n"
    )
    child = write(
        project,
        "scenes/child.py",
        "from base import Base\nclass Child(Base):\n    pass\nclass Helper:\n    pass\n",
    )
    result = as_json(discover(DiscoverTask(files=[base, child]), ctx))
    assert [(s["name"], s["file"], s["bases"]) for s in result["scenes"]] == [
        ("Base", "scenes/base.py", ["manim.MovingCameraScene"]),
        ("Child", "scenes/child.py", ["Base"]),
    ]


def test_unparseable_files_become_findings(project: Path, ctx) -> None:
    broken = write(
        project, "scenes/broken.py", "class A(Scene):\n    def construct(self:\n        pass\n"
    )
    latin = write(project, "scenes/latin.py", "# caf\xe9\n".encode("latin-1"))
    findings = as_json(discover(DiscoverTask(files=[broken, latin]), ctx))["findings"]
    assert [
        (f["code"], f["severity"], f["location"]["file"], f["location"]["line"]) for f in findings
    ] == [
        ("python_syntax", "error", "scenes/broken.py", 2),
        ("source_encoding", "error", "scenes/latin.py", 1),
    ]
    assert findings[0]["location"]["column"] is not None


def test_discover_never_executes_code(project: Path, ctx) -> None:
    marker = project / "executed"
    source = f"open({str(marker)!r}, 'w').write('x')\nclass Demo(Scene):\n    pass\n"
    result = discover(DiscoverTask(files=[write(project, "scenes/main.py", source)]), ctx)
    assert [scene.name for scene in result.scenes] == ["Demo"]
    assert not marker.exists()
