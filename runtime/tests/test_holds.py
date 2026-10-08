"""Automatic holds (R7): a film that writes no timing code is paced for its viewer."""

from __future__ import annotations

import importlib.util
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest
from manim import Circle, Square, Triangle, ValueTracker

from conftest import requires_latex, requires_manim
from manim_director_runtime import DirectedScene, pacing, timeline
from manim_director_runtime.kit import DotArray
from manim_director_runtime.timeline import Timeline

pytestmark = requires_manim

HERE = Path(__file__).resolve().parent
GENERAL = pacing.settings()
EPS = 1e-6
Render = Callable[..., Any]


def record(render: Render, scene: type) -> Timeline:
    with timeline.recording(HERE) as recorder:
        rendered = render(scene, every_frame=True)
    return recorder.timeline(scene.__name__, rendered.renderer.time)


def stills(film: Timeline) -> dict[str | None, float]:
    """How long the stage stays still at the end of each beat (and after the last one)."""

    ends: dict[str | None, float] = {}
    for event, following in zip(film.events, [*film.events[1:], None], strict=True):
        until = film.duration_seconds if following is None else following.at
        ends[event.beat] = until - (event.at + event.seconds)
    return ends


class Defaults(DirectedScene):
    def construct(self) -> None:
        with self.beat("plain"):
            self.place(Square())
        with self.beat("result", intent="reveal"):
            self.place(Circle())
        with self.beat("read"):
            self.caption("A caption of twelve words takes the viewer a while to read.")
        with self.beat("set", hold=0.3):
            self.place(Triangle())


def test_beats_hold_long_enough_by_themselves_and_explicit_holds_win(render: Render) -> None:
    film = record(render, Defaults)
    plain, result, read, fixed = film.beats
    assert (plain.hold, plain.hold_auto) == (pytest.approx(GENERAL.beat_end_min), True)
    assert result.hold == pytest.approx(GENERAL.result_end_min)  # a reveal must land
    (caption,) = film.captions
    assert caption.until - caption.at >= pacing.caption_seconds(12, GENERAL) - EPS
    assert read.hold > GENERAL.beat_end_min  # the caption, not the floor, set it
    assert (fixed.hold, fixed.hold_auto) == (0.3, False)
    last = film.events[-1]
    assert film.duration_seconds - (last.at + last.seconds) >= GENERAL.final_hold - EPS
    found = {(f.code, f.beat) for f in pacing.check(film, GENERAL) if f.code != "viewer_plan"}
    assert found == set()  # the explicit 0.3 s is followed by the final still


class Loop(DirectedScene):
    final_hold = 0

    def construct(self) -> None:
        with self.beat("only", hold=0):
            self.place(Square())


def test_a_scene_can_turn_the_final_still_off(render: Render) -> None:
    film = record(render, Loop)
    assert film.duration_seconds == pytest.approx(film.events[-1].at + film.events[-1].seconds)


@requires_latex
def test_derive_steps_and_devices_wait_for_what_came_before(render: Render) -> None:
    class Paced(DirectedScene):
        def construct(self) -> None:
            dots = DotArray(3, shown=lambda r, c: r == c == 0)
            with self.beat("grow"):
                self.place(dots, region="left")
                for k in (1, 2):  # back-to-back reveals of one component read as one
                    self.show(dots.select(lambda r, c, k=k: max(r, c) == k), run_time=0.5)
                self.derive("1 + 3 + 5", "= 9", r"= 3^2", region="right")
                self.pause()
                self.play(dots.paint(lambda r, c: r == 0, "accent"))

    film = record(render, Paced)
    shows = [e for e in film.events if e.source == "show"]
    assert shows[1].at == pytest.approx(shows[0].at + shows[0].seconds)
    findings = [f for f in pacing.check(film, GENERAL) if f.code in ("short_hold", "rushed_step")]
    assert findings == []
    steps = [e for e in film.events if e.source == "derive"]
    for step, after in zip(steps, steps[1:], strict=False):
        assert after.at - (step.at + step.seconds) >= pacing.settle_seconds(step, GENERAL) - EPS


def test_the_viewer_level_sets_the_pace(render: Render, tmp_path: Path) -> None:
    project = tmp_path / "project"
    (project / "scenes").mkdir(parents=True)
    (project / "director.yaml").write_text("brief:\n  viewer:\n    level: intro\n")
    source = project / "scenes" / "slow.py"
    source.write_text(
        "from manim import Square\n"
        "from manim_director_runtime import DirectedScene\n"
        "class Slow(DirectedScene):\n"
        "    def construct(self):\n"
        "        with self.beat('plain'):\n"
        "            self.place(Square())\n"
        "        with self.beat('aha', aha=True):\n"
        "            self.ask('Bigger or smaller?')\n"
    )
    spec = importlib.util.spec_from_file_location("director_slow_scene", source)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    film = record(render, module.Slow)
    intro = pacing.settings("intro")
    plain, aha = film.beats
    assert plain.hold == pytest.approx(intro.beat_end_min)
    (ask,) = film.asks
    assert ask.hold == intro.ask_hold
    assert stills(film)["aha"] >= intro.final_hold - EPS


def test_devices_wait_until_the_title_and_caption_are_read(render: Render) -> None:
    class ReadFirst(DirectedScene):
        def construct(self) -> None:
            dots = DotArray(3, shown=lambda r, c: r == 0)
            with self.beat("hook"):
                self.title("Why do the dots grow?")
                self.place(dots)
                self.caption("Watch the second row fill in.")
                self.show(dots.select(lambda r, c: r == 1))

    film = record(render, ReadFirst)
    (title,), (caption,) = film.titles, film.captions
    (show,) = [e for e in film.events if e.source == "show"]
    reading = sum(pacing.caption_seconds(lane.words, GENERAL) for lane in (title, caption))
    assert show.at - caption.at >= reading - EPS  # one after the other, before anything moves
    assert not [f for f in pacing.check(film, GENERAL) if f.code == "motion_while_reading"]


def test_an_answer_and_a_changed_readout_land_before_the_next_motion(render: Render) -> None:
    from manim_director_runtime.kit import Readout

    class Answer(DirectedScene):
        def construct(self) -> None:
            x = ValueTracker(0.0)
            readout = Readout("x", x)
            with self.beat("predict"):
                self.place(readout)
                self.ask("Where does x stop?")
            with self.beat("answer", keep=[readout]):
                self.play(x.animate.set_value(2), run_time=2)
                self.pause()
                self.play(x.animate.set_value(3), run_time=0.5)
                self.pause()
                self.play(x.animate.set_value(4), run_time=0.5)

    film = record(render, Answer)
    slow, quick, last = [e for e in film.events if e.beat == "answer"]
    assert slow.changed == quick.changed == 1
    assert quick.at - (slow.at + slow.seconds) >= GENERAL.result_end_min - EPS  # the answer
    assert last.at - (quick.at + quick.seconds) >= GENERAL.read_per_value - EPS  # the new value
    assert film.beats[1].hold >= GENERAL.result_end_min - EPS
