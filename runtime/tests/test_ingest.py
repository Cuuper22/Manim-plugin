from __future__ import annotations

import hashlib
import json
from pathlib import Path

from conftest import as_json
from manim_director_runtime.ingest import ingest
from manim_director_runtime.tasks import IngestSource, IngestTask


def source(path: Path, kind: str, destination: Path, **fields) -> IngestSource:
    return IngestSource(
        path=path,
        kind=kind,
        destination_dir=destination,
        id=fields.get("id"),
        license=fields.get("license"),
        attribution=fields.get("attribution"),
    )


def run(ctx, project: Path, sources: list[IngestSource], **options) -> dict:
    task = IngestTask(
        sources=sources,
        normalize=options.get("normalize", False),
        force=options.get("force", False),
        manifest=project / "sources/manifest.json",
    )
    return as_json(ingest(task, ctx))


def test_ingest_summarizes_and_records_provenance(tmp_path: Path, project: Path, ctx) -> None:
    incoming = tmp_path / "incoming"
    incoming.mkdir()
    (incoming / "Notes.md").write_text("# Setup\n\nSome *words*.\n\n## Proof\n\n```py\nx=1\n```\n")
    (incoming / "values.csv").write_text("n,value\n0,1\n1,1\n2,2\n")
    (incoming / "scene.py").write_text("from manim import Scene\nclass Demo(Scene):\n    pass\n")
    (incoming / "nb.ipynb").write_text(
        json.dumps(
            {
                "cells": [
                    {"cell_type": "markdown", "source": ["# Result"]},
                    {"cell_type": "code", "source": ["1+1"]},
                ]
            }
        )
    )
    dest = project / "sources"
    result = run(
        ctx,
        project,
        [
            source(incoming / "Notes.md", "markdown", dest, license="CC-BY-4.0", attribution="Ada"),
            source(incoming / "values.csv", "csv", dest, id="Fib Values"),
            source(incoming / "scene.py", "python", dest),
            source(incoming / "nb.ipynb", "notebook", dest),
        ],
    )
    notes, values, scene, notebook = result["sources"]
    assert (notes["id"], notes["path"], notes["headings"]) == (
        "notes",
        "sources/notes.md",
        ["Setup", "Proof"],
    )
    assert notes["sha256"] == hashlib.sha256((dest / "notes.md").read_bytes()).hexdigest()
    assert notes["origin"] == str(incoming / "Notes.md")
    assert (values["path"], values["columns"], values["rows"]) == (
        "sources/fib-values.csv",
        ["n", "value"],
        3,
    )
    assert scene["scenes"] == ["Demo"]
    assert notebook["headings"] == ["Result"] and "2 cells" in notebook["summary"]
    manifest = json.loads((dest / "manifest.json").read_text())
    assert manifest["version"] == 2
    assert manifest["entries"][0] == {
        "id": "notes",
        "kind": "markdown",
        "path": "sources/notes.md",
        "bytes": notes["bytes"],
        "sha256": notes["sha256"],
        "origin": str(incoming / "Notes.md"),
        "license": "CC-BY-4.0",
        "attribution": "Ada",
    }
    assert result["artifacts"][-1] == {
        "kind": "file",
        "path": "sources/manifest.json",
        "label": None,
    }


def test_names_are_suffixed_unless_forced_and_the_manifest_merges(
    tmp_path: Path, project: Path, ctx
) -> None:
    first = tmp_path / "a" / "data.json"
    second = tmp_path / "b" / "data.json"
    for path, value in ((first, 1), (second, 2)):
        path.parent.mkdir()
        path.write_text(json.dumps({"value": value}))
    dest = project / "sources"
    run(ctx, project, [source(first, "json", dest)])
    again = run(ctx, project, [source(second, "json", dest)])
    assert again["sources"][0]["path"] == "sources/data-2.json"
    forced = run(ctx, project, [source(second, "json", dest)], force=True)
    assert forced["sources"][0]["path"] == "sources/data.json"
    assert json.loads((dest / "data.json").read_text()) == {"value": 2}
    entries = json.loads((dest / "manifest.json").read_text())["entries"]
    assert [(e["path"], e["origin"]) for e in entries] == [
        ("sources/data-2.json", str(second)),
        ("sources/data.json", str(second)),
    ]


def test_an_older_manifest_is_kept_aside(tmp_path: Path, project: Path, ctx) -> None:
    dest = project / "sources"
    dest.mkdir()
    (dest / "manifest.json").write_text('{"version": 1, "sources": [{"path": "old.md"}]}')
    note = tmp_path / "n.txt"
    note.write_text("plain words")
    run(ctx, project, [source(note, "text", dest)])
    assert json.loads((dest / "manifest.v1.json").read_text())["sources"] == [{"path": "old.md"}]
    assert json.loads((dest / "manifest.json").read_text())["version"] == 2


def test_normalize_sanitizes_svg(tmp_path: Path, project: Path, ctx) -> None:
    svg = tmp_path / "knot.svg"
    svg.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" onload="x()">'
        '<script>alert(1)</script><a href="javascript:x()"><circle r="4"/></a></svg>'
    )
    result = run(ctx, project, [source(svg, "svg", project / "assets")], normalize=True)
    item = result["sources"][0]
    assert (item["path"], item["normalized"], item["width"], item["height"]) == (
        "assets/knot.svg",
        True,
        40,
        20,
    )
    stored = (project / "assets/knot.svg").read_text()
    assert "script" not in stored and "onload" not in stored and "javascript" not in stored
    assert "circle" in stored


def test_malformed_sources_fail_without_leaving_files(tmp_path: Path, project: Path, ctx) -> None:
    import pytest

    from manim_director_runtime.errors import DirectorError

    bad = tmp_path / "bad.json"
    bad.write_text("{nope")
    with pytest.raises(DirectorError) as raised:
        run(ctx, project, [source(bad, "json", project / "sources")])
    assert raised.value.code == "invalid_source"
    assert list((project / "sources").iterdir()) == []


def test_table_rows_are_counted_beyond_the_text_sample(tmp_path: Path, project: Path, ctx) -> None:
    from manim_director_runtime.summaries import TEXT_SAMPLE_BYTES

    table = tmp_path / "long.csv"
    rows = TEXT_SAMPLE_BYTES // 4 + 1000  # four bytes per row
    table.write_text("n,v\n" + "".join(f"{i % 10},{i % 7}\n" for i in range(rows)))
    (summary,) = run(ctx, project, [source(table, "csv", project / "sources")])["sources"]
    assert (summary["columns"], summary["rows"]) == (["n", "v"], rows)


def test_a_malformed_pdf_is_an_invalid_source(tmp_path: Path, project: Path, ctx) -> None:
    import io

    import pytest

    from manim_director_runtime.errors import DirectorError

    pypdf = pytest.importorskip("pypdf")
    writer = pypdf.PdfWriter()
    writer.add_blank_page(100, 100)
    buffer = io.BytesIO()
    writer.write(buffer)
    # A number where a dictionary belongs makes pypdf raise a TypeError while extracting text.
    broken = buffer.getvalue().replace(b"/Resources <<\n>>", b"/Resources 7    ")
    assert broken != buffer.getvalue()
    pdf = tmp_path / "paper.pdf"
    pdf.write_bytes(broken)
    with pytest.raises(DirectorError) as raised:
        run(ctx, project, [source(pdf, "pdf", project / "sources")])
    assert raised.value.code == "invalid_source"
    assert list((project / "sources").iterdir()) == []
