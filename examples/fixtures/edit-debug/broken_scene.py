"""Deliberately failing fixture used to exercise missing-asset diagnosis."""

from pathlib import Path

from manim import FadeIn, Scene, SVGMobject


ROOT = Path(__file__).resolve().parent


class MissingAssetScene(Scene):
    def construct(self) -> None:
        icon = SVGMobject(str(ROOT / "assets" / "missing-badge.svg"))
        self.play(FadeIn(icon))
