"""Stable edit fixture with semantic targets and externally controlled values."""

from __future__ import annotations

import json
from pathlib import Path

from manim import Circle, Create, FadeIn, Scene, Text, VGroup, DOWN


ROOT = Path(__file__).resolve().parent
SETTINGS = json.loads((ROOT / "fixture.json").read_text(encoding="utf-8"))["scene"]


class EditablePulse(Scene):
    semantic_ids = {
        "pulse.ring": "ring",
        "pulse.label": "label",
        "pulse.composition": "composition",
    }

    def construct(self) -> None:
        self.camera.background_color = SETTINGS["background"]
        ring = Circle(
            radius=SETTINGS["radius"],
            color=SETTINGS["color"],
            stroke_width=SETTINGS["stroke_width"],
        )
        label = Text(SETTINGS["label"], font_size=SETTINGS["font_size"], color="#FFFFFF")
        composition = VGroup(ring, label).arrange(DOWN, buff=0.3)
        self.next_section("pulse.enter")
        self.play(Create(ring), FadeIn(label), run_time=SETTINGS["enter_seconds"])
        self.next_section("pulse.hold")
        self.wait(SETTINGS["hold_seconds"])
