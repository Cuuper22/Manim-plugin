from __future__ import annotations

import ast
import json
from dataclasses import fields, replace
from pathlib import Path

import pytest

from manim_director_runtime import pacing
from manim_director_runtime.errors import CompositionError
from manim_director_runtime.jsonio import write_json
from manim_director_runtime.model import Severity
from manim_director_runtime.project import StoryBeat, Viewer
from manim_director_runtime.timeline import (
    Ask,
    Definition,
    Event,
    Lane,
    Seen,
    Settle,
    TextBox,
    Timeline,
    TimelineBeat,
    load,
)

GENERAL = pacing.settings()
VIEWER = Viewer(level="general", knows=("odd numbers", "$n^2$"))
STORY = (StoryBeat("hook", question="Why squares?", takeaway="They grow by Ls."),)


def shape(label: str = "square", chunks: int = 1) -> Seen:
    return Seen(kind="shape", label=label, chunks=chunks)


def event(at: float, seconds: float = 1.0, **values) -> Event:
    base = {"beat": "hook", "source": "play", "file": "s.py", "line": 10}
    return Event(at=at, seconds=seconds, **{**base, **values})


def lane(at: float, until: float, text: str, line: int = 5) -> Lane:
    return Lane(at, until, text, len(text.split()), "s.py", line)


def clean() -> Timeline:
    """A film that meets every budget: one beat, one reveal, a caption, a calm still."""

    return Timeline(
        version=2,
        scene="Demo",
        duration_seconds=6.0,
        beats=[TimelineBeat("hook", 0.0, 6.0, "s.py", 3)],
        events=[event(0.0, entered=[shape()])],
        captions=[lane(0.0, 6.0, "Each odd number wraps the square.")],
        titles=[lane(0.0, 6.0, "Why squares?")],
        settles=[
            Settle(
                1.0,
                "hook",
                5.0,
                visible_chunks=2,
                colors=["foreground", "muted", "primary"],
                text_boxes=[
                    TextBox("1+3", [0.1, 0.1, 0.3, 0.2]),
                    TextBox("n", [0.5, 0.5, 0.6, 0.6]),
                ],
            )
        ],
    )


def codes(timeline: Timeline, s: pacing.Settings = GENERAL, **plan) -> list[str]:
    plan = {"viewer": VIEWER, "storyboard": STORY, **plan}
    return [f.code for f in pacing.check(timeline, s, **plan)]


def test_a_film_within_every_budget_has_no_findings() -> None:
    assert codes(clean()) == []


def test_a_v1_timeline_is_not_judged() -> None:
    assert codes(replace(clean(), version=1, captions=[lane(0.0, 0.2, "far too quick")])) == []


def test_a_caption_must_stay_long_enough_to_read() -> None:
    quick = replace(clean(), captions=[lane(0.0, 1.9, "Each odd number wraps the square.")])
    (finding,) = pacing.check(quick, GENERAL, VIEWER, STORY)
    assert finding.code == "caption_too_fast" and finding.severity is Severity.WARNING
    assert "1.9 s; 6 words need 2.9 s" in finding.message
    assert (finding.location.file, finding.location.line, finding.beat) == ("s.py", 5, "hook")


def test_long_captions_and_titles_are_info() -> None:
    wordy = replace(
        clean(),
        captions=[
            lane(0.0, 6.0, "one two three four five six seven eight nine ten eleven twelve 13")
        ],
        titles=[lane(0.0, 6.0, "a title of more than eight words is a sentence", line=4)],
    )
    found = pacing.check(wordy, GENERAL, VIEWER, STORY)
    assert [(f.code, f.severity) for f in found] == [("long_text", Severity.INFO)] * 2


def test_a_reveal_needs_a_still_before_the_next_change() -> None:
    formula = Seen(kind="math", label="1+3+5", chunks=1, glyphs=5)  # 1.0 + 0.3 * 2 = 1.6 s
    rushed = replace(
        clean(),
        events=[
            event(0.0, entered=[formula], line=12),
            event(1.5, entered=[shape("ring")], line=13),
        ],
    )
    (finding,) = pacing.check(rushed, GENERAL, VIEWER, STORY)
    assert finding.code == "short_hold" and finding.location.line == 12
    assert "is still for 0.5 s before line 13 moves on; taking it in needs 1.6 s" in finding.message
    calm = replace(rushed, events=[rushed.events[0], replace(rushed.events[1], at=2.6)])
    assert codes(calm) == []


def test_a_beat_end_needs_a_longer_still_and_a_result_beat_longer_still() -> None:
    beats = [TimelineBeat("hook", 0.0, 1.6, "s.py", 3), TimelineBeat("rule", 1.6, 6.0, "s.py", 20)]
    two = replace(
        clean(),
        beats=beats,
        events=[event(0.0, entered=[shape()]), event(1.6, beat="rule", entered=[shape("L")])],
    )
    story = (*STORY, StoryBeat("rule", question="And then?", takeaway="Squares."))
    assert codes(two, storyboard=story) == ["short_hold"]  # 0.6 s at the end of hook
    held = replace(two, events=[two.events[0], replace(two.events[1], at=2.0)])
    beats[1].start_seconds = 2.0
    assert codes(held, storyboard=story) == []
    proof = (replace(story[0], intent="prove"), story[1])
    assert codes(held, storyboard=proof) == ["short_hold"]  # a result beat ends on 2 s


def test_focus_and_highlight_do_not_end_a_still() -> None:
    signal = event(1.0, 0.5, source="focus")
    late = event(1.6, entered=[shape("L")])
    focused = replace(clean(), events=[event(0.0, entered=[shape()]), signal, late])
    assert codes(focused) == []


def test_back_to_back_reveals_of_one_component_read_as_one() -> None:
    dots = Seen(kind="dots", label="selection", chunks=1, read=0.5)
    growing = [
        event(0.0, entered=[dots], of="dots-1", source="show"),
        event(1.0, entered=[dots], of="dots-1", source="show"),
        event(2.0, entered=[dots], of="dots-1", source="show"),
    ]
    assert codes(replace(clean(), events=growing)) == []
    apart = [replace(e, of=None, line=11 + n) for n, e in enumerate(growing)]
    assert codes(replace(clean(), events=apart)) == ["short_hold"] * 2


def test_a_loop_reports_its_finding_once() -> None:
    dot = Seen(kind="shape", label="Dot", chunks=1)
    loop = [event(t, 0.5, entered=[dot]) for t in (0.0, 0.5, 1.0)]  # one line, three turns
    (finding,) = pacing.check(replace(clean(), events=loop), GENERAL, VIEWER, STORY)
    assert finding.code == "short_hold" and finding.at_seconds == 0.0
    assert finding.message.endswith("The same line does it 1 more time.")


def test_a_prediction_prompt_needs_three_still_seconds() -> None:
    asked = replace(
        clean(),
        asks=[Ask(1.0, 3.0, "What shape comes next?", "s.py", 30)],
        events=[*clean().events, event(3.5, entered=[shape("L")])],
    )
    (finding,) = pacing.check(asked, GENERAL, VIEWER, STORY)
    assert finding.code == "question_hold_short" and finding.location.line == 30
    assert "gets 2.5 s" in finding.message
    patient = replace(asked, events=[*clean().events, event(4.0, entered=[shape("L")])])
    assert codes(patient) == []


@pytest.mark.parametrize(
    ("events", "message"),
    [
        ([event(0.0, 0.4, source="derive")], "This derive step runs 0.4 s"),
        ([event(0.0, 0.4, morph_glyphs=2)], "This morph runs 0.4 s"),
        ([event(0.0, 0.9, morph_glyphs=9, source="transition")], "9 glyphs change in 0.9 s"),
        (
            [event(0.0, 1.0, source="derive"), event(1.1, 1.0, source="derive")],
            "Derive steps follow each other after 0.1 s",
        ),
    ],
)
def test_steps_and_morphs_must_not_rush(events: list[Event], message: str) -> None:
    found = pacing.check(replace(clean(), events=events), GENERAL, VIEWER, STORY)
    rushed = [f for f in found if f.code == "rushed_step"]
    assert len(rushed) == 1 and message in rushed[0].message


def test_the_aha_needs_one_slow_motion() -> None:
    story = (replace(STORY[0], aha=True),)
    film = replace(clean(), events=[event(0.0, 1.0, entered=[shape()])])
    found = pacing.check(film, GENERAL, VIEWER, story)
    assert [(f.code, f.beat) for f in found] == [("rushed_step", "hook")]
    slow = replace(film, events=[event(0.0, 1.6, entered=[shape()])])
    assert codes(slow, storyboard=story) == []


def test_a_beat_holds_three_new_things() -> None:
    crowded = replace(
        clean(),
        events=[
            event(0.0, entered=[shape("a"), shape("b")]),
            event(3.0, entered=[shape("c"), shape("d")]),
        ],
    )
    (finding,) = [
        f for f in pacing.check(crowded, GENERAL, VIEWER, STORY) if f.code != "short_hold"
    ]
    assert finding.code == "crowded_beat" and finding.at_seconds == 3.0
    assert "brings in 4 new things" in finding.message
    expert = pacing.settings("expert")
    assert "crowded_beat" not in codes(crowded, expert)


def test_one_motion_at_a_time() -> None:
    three = replace(clean(), events=[event(0.0, targets=3)])
    assert codes(three) == ["crowded_moment"]
    split = replace(clean(), events=[event(0.0, targets=2, spread=0.5)])
    assert codes(split) == ["crowded_moment"]
    pair = replace(clean(), events=[event(0.0, targets=2, spread=0.1)])
    assert codes(pair) == []


def test_overlapping_text_is_found_at_a_settle() -> None:
    boxes = [TextBox("a = 1", [0.1, 0.1, 0.3, 0.2]), TextBox("b", [0.25, 0.1, 0.35, 0.2])]
    settle = replace(clean().settles[0], text_boxes=boxes)
    found = pacing.check(replace(clean(), settles=[settle]), GENERAL, VIEWER, STORY)
    assert [f.code for f in found] == ["text_overlap"]
    assert "'a = 1' and 'b' overlap by 50%" in found[0].message
    assert found[0].location.line == 10  # the last change before the still


def test_long_beats_and_unsignaled_reveals_are_info() -> None:
    long = replace(
        clean(), duration_seconds=20.0, beats=[TimelineBeat("hook", 0.0, 20.0, "s.py", 3)]
    )
    long = replace(long, captions=[lane(0.0, 20.0, "Each odd number wraps the square.")])
    assert codes(long) == ["long_beat"]
    silent = replace(clean(), captions=[], events=[event(0.0, entered=[shape("a"), shape("b")])])
    assert codes(silent) == ["unsignaled_reveal"]
    pointed = replace(silent, events=[*silent.events, event(1.0, 0.5, source="focus")])
    assert codes(pointed) == []


def test_density_and_palette_at_a_settle() -> None:
    settle = replace(
        clean().settles[0],
        visible_chunks=8,
        colors=["accent", "foreground", "primary", "secondary", "success", "#123456"],
    )
    assert codes(replace(clean(), settles=[settle])) == ["too_dense", "palette_overload"]
    assert codes(replace(clean(), settles=[settle]), pacing.settings("expert")) == [
        "palette_overload"
    ]


def test_notation_must_be_named_before_or_as_it_appears() -> None:
    formula = Seen(kind="math", label="2n - 1", chunks=1, glyphs=4, symbols=["n"])
    film = replace(clean(), captions=[], events=[event(0.0, entered=[formula])])
    unknown = Viewer(level="general", knows=("odd numbers",))
    (finding,) = pacing.check(film, GENERAL, unknown, STORY)
    assert finding.code == "unexplained_notation" and "'n' appears" in finding.message
    assert codes(film) == ["unexplained_notation"]  # knowing `$n^2$` is not knowing n
    assert codes(film, viewer=Viewer(knows=("$n$",))) == []
    named = replace(film, captions=[lane(0.0, 6.0, "Here n counts the odd numbers.")])
    assert codes(named, viewer=unknown) == []
    labeled = replace(film, events=[event(0.0, entered=[replace(formula, label="$n$")])])
    assert codes(labeled, viewer=unknown) == []
    noted = replace(film, definitions=[Definition(0.5, "n", "annotate")])
    assert codes(noted, viewer=unknown) == []
    late = replace(film, definitions=[Definition(2.0, "n", "annotate")])
    assert codes(late, viewer=unknown) == ["unexplained_notation"]
    angle = Seen(kind="right angle", label="right angle", chunks=1, conventions=["right angle"])
    marked = replace(film, events=[event(0.0, entered=[angle])])
    assert codes(marked, viewer=unknown) == ["unexplained_notation"]
    said = replace(marked, captions=[lane(0.0, 6.0, "Two sides meet at a right angle.")])
    assert codes(said, viewer=unknown) == []


def test_greek_symbols_are_named_by_word_or_letter() -> None:
    lam = Seen(kind="math", label=r"\lambda t", chunks=1, glyphs=2, symbols=[r"\lambda"])
    film = replace(
        clean(), captions=[lane(0.0, 6.0, "λ is the rate.")], events=[event(0.0, entered=[lam])]
    )
    assert codes(film) == []
    spelled = replace(film, captions=[lane(0.0, 6.0, "Lambda is the rate.")])
    assert codes(spelled) == []


def test_the_viewer_plan_is_checked_against_the_film() -> None:
    assert codes(clean(), viewer=None) == ["viewer_plan"]
    vague = Viewer(missing=("wrong_guess", "aha"))
    (finding,) = pacing.check(clean(), GENERAL, vague, STORY)
    assert "does not say wrong_guess, aha" in finding.message
    assert codes(clean(), storyboard=(StoryBeat("intro", "q", "t"),)) == ["viewer_plan"] * 2
    long = replace(
        clean(),
        duration_seconds=40.0,
        beats=[
            TimelineBeat("hook", 0.0, 14.0, "s.py", 3, question="q", takeaway="t"),
            TimelineBeat("turn", 14.0, 28.0, "s.py", 9, question="q", takeaway="t", aha=True),
            TimelineBeat("recap", 28.0, 40.0, "s.py", 15, question="q", takeaway="t"),
        ],
        captions=[lane(0.0, 40.0, "Each odd number wraps the square.")],
        events=[event(14.0, 2.0, beat="turn")],
    )
    found = pacing.check(long, GENERAL, VIEWER, ())
    assert [f.message for f in found] == [
        "The film never asks the viewer to predict (self.ask) before its aha."
    ]
    asked = replace(long, asks=[Ask(10.0, 3.0, "Next?", "s.py", 7)])
    assert codes(asked, storyboard=()) == []
    no_aha = replace(asked, beats=[replace(b, aha=False) for b in asked.beats])
    assert codes(no_aha, storyboard=()) == ["viewer_plan"]


def test_presets_and_overrides_change_the_budgets() -> None:
    caption = replace(clean(), captions=[lane(0.0, 3.0, "Each odd number wraps the square.")])
    assert codes(caption) == []  # 0.5 + 6 words / 2.5 = 2.9 s
    intro = pacing.settings("intro")
    assert (intro.words_per_second, intro.max_new_per_beat) == (2.0, 2)
    assert codes(caption, intro) == ["caption_too_fast"]  # 0.7 + 6 / 2.0 = 3.7 s
    slower = pacing.settings("general", {"words_per_second": 2.0})
    assert codes(caption, slower) == ["caption_too_fast"]
    with pytest.raises(CompositionError, match="unknown budgets wpm"):
        pacing.settings("general", {"wpm": 2.0})
    with pytest.raises(CompositionError, match="level is 'novice'"):
        pacing.settings("novice")


def test_budgets_json_is_the_only_source_of_numbers() -> None:
    data = pacing.budgets()
    assert [f.name for f in fields(pacing.Settings)] == list(data["budgets"])
    for name, entry in data["budgets"].items():
        assert set(entry) == {*data["levels"], "about"}, name
    source = Path(pacing.__file__).read_text(encoding="utf-8")
    numbers = {
        node.value
        for node in ast.walk(ast.parse(source))
        if isinstance(node, ast.Constant)
        and isinstance(node.value, int | float)
        and not isinstance(node.value, bool)
    }
    # 2: the timeline version and pairs of boxes; 3: names listed in a message
    assert numbers <= {0, 1, 2, 3}, numbers


def test_timelines_round_trip_and_v1_files_still_load(tmp_path: Path) -> None:
    film = replace(
        clean(),
        asks=[Ask(1.0, 3.0, "Next?", "s.py", 7)],
        definitions=[Definition(0.5, "n", "legend")],
    )
    path = tmp_path / "Demo.timeline.json"
    write_json(path, film)
    assert load(path) == film
    v1 = {"version": 1, "scene": "Demo", "duration_seconds": 2.0, "beats": []}
    path.write_text(json.dumps(v1))
    assert load(path) == Timeline(1, "Demo", 2.0, [])
