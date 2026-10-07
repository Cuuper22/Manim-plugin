from __future__ import annotations

from pathlib import Path

from conftest import as_json
from manim_director_runtime.diagnostics import classify, diagnose, location_from_text
from manim_director_runtime.tasks import DiagnoseTask


def run(ctx, text: str) -> dict:
    return as_json(diagnose(DiagnoseTask(text=text), ctx))


def test_name_error_points_at_the_innermost_project_frame(project: Path, ctx) -> None:
    text = (
        "Traceback (most recent call last):\n"
        f'  File "{project}/scenes/main.py", line 23, in construct\n'
        "    self.play(Write(rowz))\n"
        f'  File "{project}/.venv/lib/site-packages/manim/scene.py", line 9, in play\n'
        "    pass\n"
        "NameError: name 'rowz' is not defined\n"
    )
    result = run(ctx, text)
    assert result["recognized"] is True
    (finding,) = result["findings"]
    assert finding == {
        "code": "python_name",
        "severity": "error",
        "message": "name 'rowz' is not defined",
        "hint": "Define or import the referenced name.",
        "location": {"file": "scenes/main.py", "line": 23, "column": None},
        "at_seconds": None,
        "beat": None,
        "frame": None,
    }


def test_syntax_error_uses_the_header_above_the_caret(project: Path) -> None:
    text = (
        "Traceback (most recent call last):\n"
        '  File "/usr/lib/python3.12/runpy.py", line 5, in run\n'
        f'  File "{project}/scenes/main.py", line 7\n'
        "    def construct(self:\n"
        "                      ^\n"
        "SyntaxError: '(' was never closed\n"
    )
    location = location_from_text(text, project)
    assert (location.file, location.line) == ("scenes/main.py", 7)
    assert [f.code for f in classify(text, location)] == ["python_syntax"]


def test_frames_outside_the_project_stay_absolute(project: Path) -> None:
    text = 'File "/opt/lib/thing.py", line 3, in f\nValueError: nope\n'
    location = location_from_text(text, project)
    assert (location.file, location.line) == ("/opt/lib/thing.py", 3)


def test_latex_errors_carry_the_offending_tex(project: Path, ctx) -> None:
    text = (
        f'File "{project}/scenes/main.py", line 42, in construct\n'
        "ValueError: latex error converting to dvi. See log output above or the log file: x.log\n"
        "! Undefined control sequence.\n"
        "l.8 \\phii\n"
    )
    (finding,) = run(ctx, text)["findings"]
    assert finding["code"] == "latex_error"
    assert finding["message"] == "Undefined control sequence."
    assert finding["hint"] == "Check the TeX near \\phii."
    assert finding["location"]["line"] == 42


def test_known_failure_families() -> None:
    cases = {
        "ModuleNotFoundError: No module named 'networkx'": "python_import",
        "AttributeError: 'Circle' object has no attribute 'foo'": "python_attribute",
        "TypeError: __init__() got an unexpected keyword argument 'colour'": "api_signature",
        "FileNotFoundError: [Errno 2] No such file or directory: 'latex'": "latex_missing",
        "! LaTeX Error: File `physics.sty' not found.": "latex_package",
        "Unknown encoder 'libx265'": "ffmpeg_encoder",
        "FileNotFoundError: [Errno 2] No such file or directory: 'knot.svg'": "asset_missing",
    }
    for text, code in cases.items():
        assert [f.code for f in classify(text, None)] == [code], text


def test_unrecognized_text_is_reported_honestly(ctx) -> None:
    result = run(ctx, "something odd happened\nfinal line\n")
    assert result["recognized"] is False
    assert [(f["code"], f["message"]) for f in result["findings"]] == [
        ("unclassified", "final line")
    ]


def test_compiled_extension_frames_are_not_project_files(project: Path) -> None:
    text = (
        f'  File "{project}/scenes/main.py", line 9, in construct\n'
        '  File "av/error.py", line 354, in av.error.err_check\n'
        "av.error.FileNotFoundError: [Errno 2] No such file or directory\n"
    )
    location = location_from_text(text, project)
    assert (location.file, location.line) == ("scenes/main.py", 9)
    assert location_from_text(text.split("\n", 1)[1], project) is None


def test_latex_hints_skip_manims_wrapper_and_the_terminal_prompt() -> None:
    text = "! Undefined control sequence.\nl.8 \\special{dvisvgm:raw <g id='unique000'>}\\phii\n"
    (finding,) = classify(text, None)
    assert finding.hint == "Check the TeX near \\phii."
    missing = "! LaTeX Error: File `nopkg.sty' not found.\n! Emergency stop.\n<*> ...\n"
    assert [f.code for f in classify(missing, None)] == ["latex_package"]
    (prompt,) = classify("! Emergency stop.\n<read *>\n", None)
    assert prompt.hint == "Correct the TeX in the expression."


def test_diagnose_reads_the_named_tex_log_inside_the_project(project: Path, ctx) -> None:
    log = project / ".manim-director/media/Tex/b771.log"
    log.parent.mkdir(parents=True)
    log.write_text("! Undefined control sequence.\nl.8 \\phii\n", encoding="utf-8")
    outside = project.parent / "elsewhere.log"
    outside.write_text("! Secret line.\n", encoding="utf-8")

    def text(path: Path) -> str:
        return (
            "render_failed: Phii.construct raised ValueError.\n"
            f'  File "{project}/scenes/main.py", line 7, in construct\n'
            "ValueError: latex error converting to dvi. See log output above or the log file: "
            f"{path}\nERROR LaTeX compilation error:\n"
        )

    (finding,) = run(ctx, text(log))["findings"]
    assert (finding["message"], finding["hint"]) == (
        "Undefined control sequence.",
        "Check the TeX near \\phii.",
    )
    assert finding["location"]["line"] == 7
    (generic,) = run(ctx, text(outside))["findings"]
    assert generic["message"] == "LaTeX could not compile the expression."


def test_unrecognized_failures_report_the_exception_not_the_last_log_line(ctx) -> None:
    text = (
        "render_failed: Writing X raised OSError.\nTraceback (most recent call last):\n"
        "OSError: [Errno 28] No space left on device\nWARNING be slower than other formats\n"
    )
    (finding,) = run(ctx, text)["findings"]
    assert finding["message"] == "OSError: [Errno 28] No space left on device"
