"""The skill's documents agree with the runtime: every code sample renders within the pacing
budgets, every signature is exact, and every number and finding code is the one QA uses."""

from __future__ import annotations

import importlib.util
import inspect
import json
import re
import sys
import textwrap
from collections.abc import Callable
from enum import Enum
from pathlib import Path
from typing import Any

import pytest
import yaml

from conftest import requires_latex, requires_manim
from manim_director_runtime import pacing, timeline
from manim_director_runtime.model import Severity
from manim_director_runtime.project import load_style

ROOT = Path(__file__).resolve().parents[2]
SKILL = ROOT / "skills" / "manim-director"
DOCS = sorted([SKILL / "SKILL.md", *(SKILL / "references").glob("*.md")])
GALLERY = ROOT / "examples" / "gallery"
BUDGETS = json.loads(
    (ROOT / "runtime/src/manim_director_runtime/data/budgets.json").read_text(encoding="utf-8")
)
_FENCE = re.compile(r"^```(\w*)\n(.*?)^```$", re.M | re.S)
_LINK = re.compile(r"\]\(([^)#\s]+)(?:#[^)]*)?\)")
_SCENE = re.compile(r"^class (\w+)\(DirectedScene\)", re.M)


def read(name: str) -> str:
    return (SKILL / name).read_text(encoding="utf-8")


def fenced(name: str, language: str) -> list[str]:
    return [body for lang, body in _FENCE.findall(read(name)) if lang == language]


def section(name: str, heading: str) -> str:
    """The text under `heading` (a whole `## ...` line), up to the next heading of its level."""

    text = read(name)
    level = heading.split(" ", 1)[0]
    start = text.index(f"\n{heading}\n")
    end = text.find(f"\n{level} ", start + 1)
    return text[start : end if end != -1 else len(text)]


def rows(text: str) -> list[list[str]]:
    """Table body rows, cells stripped; the header and the --- rule are left out."""

    lines = [line for line in text.splitlines() if line.startswith("|")]
    return [[cell.strip() for cell in line.strip("|").split(" | ")] for line in lines[2:]]


def unquote(cell: str) -> str:
    return cell.removeprefix("`").removesuffix("`")


def kit_samples() -> list[str]:
    """kit.md's samples as scene modules. The first block is the header the others assume; a
    block that defines no scene class is the body of `construct`."""

    header, *samples = fenced("references/kit.md", "python")
    modules = []
    for sample in samples:
        if not _SCENE.search(sample):
            body = textwrap.indent(sample, " " * 8)
            sample = f"class Sample(DirectedScene):\n    def construct(self):\n{body}"
        modules.append(f"{header}\n\n{sample}")
    return modules


# Prose ------------------------------------------------------------------------------------------


def test_skill_md_stays_short() -> None:
    assert len(read("SKILL.md").split()) <= 1600


@pytest.mark.parametrize(("name", "most"), [("viewer.md", 2200), ("kit.md", 2000)])
def test_new_references_stay_within_their_budgets(name: str, most: int) -> None:
    assert len(read(f"references/{name}").split()) <= most


@pytest.mark.parametrize("doc", DOCS, ids=lambda p: p.name)
def test_every_relative_link_resolves(doc: Path) -> None:
    targets = _LINK.findall(doc.read_text(encoding="utf-8"))
    missing = [t for t in targets if "://" not in t and not (doc.parent / t).exists()]
    assert missing == []


def test_the_budget_table_is_budgets_json() -> None:
    (block,) = _FENCE.findall(section("references/viewer.md", "## 3. Budgets"))
    table = {
        key: tuple(float(v) for v in values)
        for key, *values in (line.split()[:4] for line in block[1].splitlines())
    }
    levels = ("general", "intro", "expert")
    expected = {
        key: tuple(float(budget[level]) for level in levels)
        for key, budget in BUDGETS["budgets"].items()
    }
    assert table == expected


def test_the_project_example_uses_every_viewer_key_and_loads(tmp_path: Path) -> None:
    (block,) = fenced("references/project.md", "yaml")
    spec = yaml.safe_load(block)
    documented = [
        unquote(key) for key, _ in rows(section("references/viewer.md", "## 2. The viewer model"))
    ]
    assert list(spec["brief"]["viewer"]) == documented
    (tmp_path / "director.yaml").write_text(block, encoding="utf-8")
    style = load_style(tmp_path)
    assert style.viewer is not None and style.viewer.missing == ()
    assert [beat.aha for beat in style.storyboard] == [False, True]
    pacing.settings(style.viewer.level, style.pacing)  # a known level and budget names


def test_the_finding_table_is_pacing_codes() -> None:
    table = {
        unquote(code): (severity, hint)
        for code, severity, _, hint in rows(
            section("references/verify.md", "## Pacing and viewer findings")
        )
    }
    assert table == {code: (str(sev), hint) for code, (sev, hint) in pacing.CODES.items()}
    assert {str(sev) for sev, _ in table.values()} <= {str(Severity.WARNING), str(Severity.INFO)}


# Signatures -------------------------------------------------------------------------------------


def plain(function: Callable[..., Any]) -> str:
    """A signature as the catalog writes it: no annotations, no self, enum defaults by value."""

    parts, starred = [], False
    for p in inspect.signature(function).parameters.values():
        if p.name == "self":
            continue
        if p.kind is p.VAR_POSITIONAL:
            parts.append(f"*{p.name}")
            starred = True
            continue
        if p.kind is p.VAR_KEYWORD:
            parts.append(f"**{p.name}")
            continue
        if p.kind is p.KEYWORD_ONLY and not starred:
            parts.append("*")
            starred = True
        default = p.default.value if isinstance(p.default, Enum) else p.default
        parts.append(p.name if p.default is p.empty else f"{p.name}={default!r}")
    return f"({', '.join(parts)})"


def resolve(name: str) -> Callable[..., Any]:
    from manim_director_runtime import DirectedScene, kit
    from manim_director_runtime.kit.card import Misconception

    owner, _, attribute = name.rpartition(".")
    if owner == "self":
        return getattr(DirectedScene, attribute)
    if owner == "Misconception":
        return getattr(Misconception, attribute)
    return getattr(getattr(kit, owner), attribute) if owner else getattr(kit, attribute)


def catalog() -> list[tuple[str, str, str]]:
    """(name, signature, example) per catalog row."""

    entries = []
    for signature, example in rows(section("references/kit.md", "## Catalog")):
        name, _, params = unquote(signature).partition("(")
        entries.append((name, f"({params}", unquote(example)))
    return entries


@requires_manim
def test_catalog_signatures_are_exact() -> None:
    wrong = {
        name: plain(resolve(name)) for name, sig, _ in catalog() if plain(resolve(name)) != sig
    }
    assert wrong == {}


def test_catalog_examples_are_lines_of_rendered_samples() -> None:
    rendered = "\n".join(
        [*kit_samples(), *(p.read_text(encoding="utf-8") for p in GALLERY.glob("*/scenes/*.py"))]
    )
    assert [example for _, _, example in catalog() if example not in rendered] == []


# Samples ----------------------------------------------------------------------------------------


@pytest.fixture(scope="module")
def tex_dir(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Shared by the samples so common TeX is compiled once."""

    return tmp_path_factory.mktemp("tex")


def findings(source: str, root: Path, render: Callable[..., Any]) -> list[tuple[str, str | None]]:
    """Render every scene `source` defines from `root/scenes/sample.py` (so it reads
    `root/director.yaml`), each frame stepped, and return its pacing findings. `viewer_plan` is
    left out: a sample is part of a film, not a planned one."""

    path = root / "scenes" / "sample.py"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(source, encoding="utf-8")
    spec = importlib.util.spec_from_file_location(f"_doc_sample_{id(path)}", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
        style = load_style(path.parent)
        budgets = pacing.settings(style.viewer.level if style.viewer else "general", style.pacing)
        found = []
        for name in _SCENE.findall(source):
            with timeline.recording(root) as recorder:
                scene = render(getattr(module, name), every_frame=True)
            film = recorder.timeline(name, scene.renderer.time)
            checked = pacing.check(film, budgets, style.viewer, style.storyboard_of(name))
            found += [(f.code, f.beat) for f in checked if f.code != "viewer_plan"]
        return found
    finally:
        del sys.modules[spec.name]


@requires_manim
@requires_latex
@pytest.mark.parametrize("index", range(len(kit_samples())))
def test_kit_samples_render_within_the_pacing_budgets(
    index: int, tmp_path: Path, render: Callable[..., Any]
) -> None:
    assert findings(kit_samples()[index], tmp_path, render) == []


@requires_manim
@requires_latex
def test_skill_example_renders_for_the_viewer_its_brief_describes(
    tmp_path: Path, render: Callable[..., Any]
) -> None:
    spec: dict[str, Any] = {"project": {"name": "Odd squares"}}
    for block in fenced("SKILL.md", "yaml"):
        spec.update(yaml.safe_load(block))
    (tmp_path / "director.yaml").write_text(yaml.safe_dump(spec), encoding="utf-8")
    style = load_style(tmp_path)
    assert style.viewer is not None and style.viewer.missing == ()
    assert [beat.id for beat in style.storyboard] == ["grow"]
    (example,) = fenced("SKILL.md", "python")
    assert findings(example, tmp_path, render) == []
