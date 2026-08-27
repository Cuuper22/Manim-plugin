"""Expected surgical repair for broken_scene.py: retain intent, remove dependency."""

from manim import Circle, FadeIn, Scene, Text, VGroup


class MissingAssetScene(Scene):
    def construct(self) -> None:
        badge = Circle(radius=1.0, color="#FFD166", stroke_width=6)
        mark = Text("λ", font_size=56, color="#FFD166").move_to(badge)
        icon = VGroup(badge, mark)
        self.play(FadeIn(icon))
