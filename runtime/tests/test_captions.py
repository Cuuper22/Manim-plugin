from __future__ import annotations

from pathlib import Path

import pytest

from conftest import as_json
from manim_director_runtime.captions import captions
from manim_director_runtime.errors import DirectorError
from manim_director_runtime.tasks import CaptionsTask

VTT = """WEBVTT

NOTE written by hand

intro
00:00.000 --> 00:01.500
A small beginning

00:00:01.400 --> 00:00:02.000
This line is much longer than forty-eight characters, sadly
and it has
three lines
"""


def run(ctx, path: Path, **options) -> dict:
    task = CaptionsTask(
        path=path,
        shift_seconds=options.get("shift", 0.0),
        scale=options.get("scale", 1.0),
        output=options.get("output"),
    )
    return as_json(captions(task, ctx))


def test_findings_point_at_cue_timing_lines(project: Path, ctx) -> None:
    path = project / "captions" / "en.vtt"
    path.parent.mkdir()
    path.write_text(VTT, encoding="utf-8")
    result = run(ctx, path)
    assert result["cue_count"] == 2
    assert result["duration_seconds"] == 2.0
    assert result["valid"] is False
    found = {(f["code"], f["location"]["line"], f["at_seconds"]) for f in result["findings"]}
    assert found == {
        ("overlap", 9, 1.4),
        ("too_many_lines", 9, 1.4),
        ("line_too_long", 9, 1.4),
        ("reading_speed", 9, 1.4),
    }
    assert all(f["location"]["file"] == "captions/en.vtt" for f in result["findings"])


def test_retime_and_convert_to_srt(project: Path, ctx) -> None:
    source = project / "en.srt"
    source.write_text(
        "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n2\n00:00:03,000 --> 00:00:04,000\nWorld\n"
    )
    output = project / "out" / "en.vtt"
    result = run(ctx, source, shift=-0.5, scale=2.0, output=output)
    assert result["valid"] is True
    assert result["artifacts"] == [{"kind": "captions", "path": "out/en.vtt", "label": None}]
    assert output.read_text() == (
        "WEBVTT\n\n1\n00:00:01.500 --> 00:00:03.500\nHello\n\n"
        "2\n00:00:05.500 --> 00:00:07.500\nWorld\n"
    )


def test_malformed_files_name_the_line(project: Path, ctx) -> None:
    path = project / "bad.srt"
    path.write_text("1\n00:00:01,000 --> soon\nHello\n")
    with pytest.raises(DirectorError) as raised:
        run(ctx, path)
    assert (raised.value.code, raised.value.data) == ("invalid_captions", {"line": 2})
    vtt = project / "bad.vtt"
    vtt.write_text("00:00.000 --> 00:01.000\nNo header\n")
    with pytest.raises(DirectorError) as raised:
        run(ctx, vtt)
    assert raised.value.data == {"line": 1}


def test_vtt_keeps_cue_settings_and_style_blocks(project: Path, ctx) -> None:
    source = project / "styled.vtt"
    source.write_text(
        "WEBVTT\n\nSTYLE\n::cue { color: yellow }\n\nNOTE dropped\n\n"
        "00:00.000 --> 00:02.000 line:0 align:start\nTop left\n"
    )
    output = project / "out.vtt"
    run(ctx, source, shift=1.0, output=output)
    assert output.read_text() == (
        "WEBVTT\n\nSTYLE\n::cue { color: yellow }\n\n"
        "00:00:01.000 --> 00:00:03.000 line:0 align:start\nTop left\n"
    )


def test_timing_messages_say_what_is_wrong(project: Path, ctx) -> None:
    path = project / "en.srt"
    path.write_text(
        "1\n00:00:01,500 --> 00:00:01,500\nNo time\n\n"
        "2\n00:00:02,000 --> 00:00:03,000\nIntro\n\n"
        "3\n00:00:06,000 --> 00:00:08,000\nKept\n"
    )
    result = run(ctx, path, shift=-5.0)  # trims the intro: cues 1 and 2 fall before 0:00
    assert result["cue_count"] == 1 and result["valid"] is True
    assert [(f["code"], f["location"]["line"]) for f in result["findings"]] == [
        ("cue_dropped", 2),
        ("cue_dropped", 6),
    ]
    (empty,) = run(ctx, path)["findings"][:1]
    assert empty["message"] == "The cue has no duration (its end is not after its start)."
