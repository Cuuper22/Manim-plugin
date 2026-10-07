from __future__ import annotations

from conftest import as_json
from manim_director_runtime.doctor import doctor
from manim_director_runtime.tasks import DoctorTask


def test_doctor_reports_checks_in_contract_order(ctx) -> None:
    result = as_json(doctor(DoctorTask(), ctx))
    assert [(c["name"], c["kind"]) for c in result["checks"]] == [
        ("manim", "package"),
        ("numpy", "package"),
        ("PIL", "package"),
        ("av", "package"),
        ("yaml", "package"),
        ("sympy", "package"),
        ("pypdf", "package"),
        ("moderngl", "package"),
        ("ffmpeg", "executable"),
        ("ffprobe", "executable"),
        ("latex", "executable"),
        ("pdflatex", "executable"),
        ("xelatex", "executable"),
        ("lualatex", "executable"),
        ("dvisvgm", "executable"),
    ]
    assert result["runtime"]["protocol"] == 2
    assert set(result["capabilities"]) == {
        "render",
        "renderers",
        "latex",
        "video_tools",
        "visual_qa",
        "symbolic_math",
        "pdf_ingest",
    }
    assert result["ok"] == (
        result["capabilities"]["render"] and result["capabilities"]["video_tools"]
    )
    assert result["disk"]["total_bytes"] >= result["disk"]["free_bytes"] > 0
    codes = {finding["code"] for finding in result["findings"]}
    assert codes <= {
        "manim_missing",
        "numpy_missing",
        "pillow_missing",
        "av_missing",
        "yaml_missing",
        "ffmpeg_missing",
        "latex_missing",
        "low_disk",
        "sympy_missing",
        "pypdf_missing",
        "opengl_unavailable",
    }
    if result["capabilities"]["render"]:
        assert ("opengl" in result["capabilities"]["renderers"]) != ("opengl_unavailable" in codes)


def test_a_package_that_fails_to_import_is_reported_as_broken(ctx, tmp_path, monkeypatch) -> None:
    from manim_director_runtime import doctor as module

    package = tmp_path / "brokenpkg"
    package.mkdir()
    (package / "__init__.py").write_text("raise ImportError('libfoo.so: cannot open')\n")
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.setitem(module.PACKAGES, "sympy", ("brokenpkg", "sympy"))
    result = as_json(doctor(DoctorTask(), ctx))
    (sympy,) = [check for check in result["checks"] if check["name"] == "sympy"]
    assert sympy["available"] is False
    assert result["capabilities"]["symbolic_math"] is False
    (finding,) = [f for f in result["findings"] if f["code"] == "sympy_missing"]
    assert finding["message"] == (
        "SymPy is installed but fails to import (ImportError: libfoo.so: cannot open); "
        "validate_math checks numerically only."
    )
