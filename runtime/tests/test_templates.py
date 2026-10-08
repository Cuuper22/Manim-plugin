"""Every packaged template scaffolds, through the bridge, into a project whose scene renders."""

from __future__ import annotations

from pathlib import Path

import pytest
import yaml

from conftest import request, requires_latex, requires_manim, run_bridge
from manim_director_runtime.scaffold import templates

pytestmark = [requires_manim, requires_latex]


@pytest.fixture(scope="module")
def media_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Shared across templates so common TeX is compiled once."""

    return tmp_path_factory.mktemp("media")


def bridge_result(project: Path, method: str, params: dict) -> dict:
    frames, completed = run_bridge(project, request(method, project, params))
    assert frames[-1]["type"] == "result", frames[-1].get("error") or completed.stderr[-3000:]
    return frames[-1]["result"]


@pytest.mark.parametrize("template", templates())
def test_template_renders_its_last_frame(template: str, project: Path, media_dir: Path) -> None:
    from PIL import Image

    created = bridge_result(
        project,
        "init",
        {
            "mode": "create",
            "name": "Template Check",
            "template": template,
            "scene_template": None,
            "theme": None,
            "seed": None,
            "source_dir": None,
        },
    )
    render = yaml.safe_load((project / "director.yaml").read_text()).get("render", {})
    portrait = render.get("height", 1080) > render.get("width", 1920)
    width, height = (180, 320) if portrait else (320, 180)
    still = bridge_result(
        project,
        "still",
        {
            "scene": created["scene"]["name"],
            "files": [str(project / created["scene"]["file"])],
            "settings": {
                "profile": "draft",
                "width": width,
                "height": height,
                "fps": 10,
                "renderer": "cairo",
                "format": "png",
                "transparent": False,
            },
            "media_dir": str(media_dir),
            "out_dir": str(project / ".manim-director/artifacts/still"),
            "fresh": False,
        },
    )
    image = Image.open(project / still["artifacts"][0]["path"])
    assert image.size == (width, height)
    darkest, brightest = image.convert("L").getextrema()
    assert brightest - darkest > 100, "the last frame is blank"
