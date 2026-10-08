"""Timeline v2 as DirectedScene records it: dry-run renders under an active recorder."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from typing import Any

from manim import RIGHT, Dot, FadeIn, Square

from conftest import requires_latex, requires_manim
from manim_director_runtime import DirectedScene, pacing, timeline
from manim_director_runtime.kit import DotArray
from manim_director_runtime.timeline import Timeline

pytestmark = requires_manim

HERE = Path(__file__).resolve().parent
LINES = Path(__file__).read_text(encoding="utf-8").splitlines()


def line(marker: str) -> int:
    """The line of this file that ends with the comment `# marker`."""

    (number,) = [n for n, text in enumerate(LINES, 1) if text.endswith(f"# {marker}")]
    return number


def record(render: Callable[..., Any], scene: type) -> Timeline:
    with timeline.recording(HERE) as recorder:
        rendered = render(scene, every_frame=True)
    return recorder.timeline(scene.__name__, rendered.renderer.time)


class Paced(DirectedScene):
    def construct(self) -> None:
        a, b = Square(), Square()
        with self.beat("hook", question="Why squares?", hold=0.5):
            self.title("Odd numbers make squares")
            self.place(a, b)  # place
            self.caption("Each odd number wraps the square.")  # caption
        with self.beat("rule", keep=[a, b], intent="reveal", aha=True):  # rule
            dot = Dot()
            self.play(FadeIn(dot), run_time=0.4)  # dot
            self.focus(dot)  # focus
            self.play(dot.animate.shift(RIGHT))  # move
            self.caption(None)


def test_reveals_captions_and_stills_are_recorded_with_their_statements(render) -> None:
    film = record(render, Paced)
    assert film.version == 2
    hook, rule = film.beats
    assert (hook.id, hook.question, hook.hold, hook.aha) == ("hook", "Why squares?", 0.5, False)
    assert (rule.intent, rule.aha, rule.transition) == ("reveal", True, "continue")

    staged, dot, focus, move, *rest = film.events
    assert (staged.at, staged.source, staged.line) == (0.0, "transition", line("place"))
    assert [(s.kind, s.chunks) for s in staged.entered] == [("shape", 1), ("shape", 1)]
    assert (dot.at, dot.source, dot.seconds, dot.line) == (
        staged.seconds + 0.5,
        "play",
        0.4,
        line("dot"),
    )
    assert [(s.kind, s.label, s.chunks) for s in dot.entered] == [("shape", "Dot", 1)]
    assert (focus.source, focus.line, focus.entered) == ("focus", line("focus"), [])
    assert (move.source, move.line, move.entered, move.targets) == ("play", line("move"), [], 1)
    assert all(event.file == "test_viewing.py" for event in film.events)

    (caption,) = film.captions
    assert (caption.at, caption.text, caption.words) == (
        0.0,
        "Each odd number wraps the square.",
        6,
    )
    assert (caption.file, caption.line) == ("test_viewing.py", line("caption"))
    assert caption.until == rest[0].at  # it starts to leave with the beat's last change
    assert [(t.text, t.at, t.until) for t in film.titles] == [
        ("Odd numbers make squares", 0.0, film.duration_seconds)
    ]

    held, ending, final = film.settles
    assert (held.beat, held.at, held.still_seconds, held.visible_chunks) == (
        "hook",
        staged.seconds,
        0.5,
        2,
    )
    assert "foreground" in held.colors
    assert [box.label for box in held.text_boxes] == [
        "Odd numbers make squares",
        "Each odd number wraps the square.",
    ]
    assert (rule.hold, rule.hold_auto) == (2.0, True)  # an aha beat ends on 2 s by itself
    assert (ending.beat, ending.still_seconds, ending.visible_chunks) == ("rule", 2.0, 1)
    assert (final.beat, ending.still_seconds + final.still_seconds) == (None, 3.0)  # final still


def test_a_recorded_film_is_judged_against_its_beats(render) -> None:
    found = pacing.check(record(render, Paced), pacing.settings())
    assert [(f.code, f.beat, f.location.line) for f in found if f.code != "viewer_plan"] == [
        ("short_hold", "hook", line("place")),  # 0.5 s hold; a beat ends on 1 s
        ("rushed_step", "rule", line("move")),  # the aha's longest motion is 1 s, not 1.5 s
    ]  # the aha beat's own hold is automatic, so its end is not short


class Grown(DirectedScene):
    symbols = {"n": "primary"}

    def construct(self) -> None:
        dots = DotArray(3, shown=lambda r, c: r == c == 0)
        with self.beat("grow"):
            self.place(dots, region="left")
            for k in (2, 3):
                self.show(dots.select(lambda r, c, k=k: max(r, c) == k - 1), run_time=0.5)  # show
        with self.beat("sum", keep=[dots]):
            self.derive("1 + 3 + 5", "= 3^2", "= n^2", region="right")  # derive


@requires_latex
def test_component_reveals_chain_and_morphs_carry_their_notation(render) -> None:
    film = record(render, Grown)
    shows = [e for e in film.events if e.source == "show"]
    assert [(e.line, e.of, [s.kind for s in e.entered]) for e in shows] == [
        (line("show"), "dots-1", ["dots"]),
        (line("show"), "dots-1", ["dots"]),
    ]
    first, second, third = [e for e in film.events if e.source == "derive"]
    assert {e.line for e in (first, second, third)} == {line("derive")}
    assert [(s.kind, s.glyphs, s.symbols) for s in first.entered] == [("math", 5, [])]
    assert (second.entered, second.morph_glyphs > 0) == ([], True)
    # "= n^2" enters by a morph: no new chunk, but n is new notation.
    assert [(s.chunks, s.symbols) for s in third.entered] == [(0, ["n"])]
    found = pacing.check(film, pacing.settings())
    notation = [f for f in found if f.code == "unexplained_notation"]
    assert [(f.location.line, f.beat) for f in notation] == [(line("derive"), "sum")]
