"""The quadratic formula by completing the square, one justified step at a time; a, b and c keep
their colors through every step."""

from manim import *

from manim_director_runtime import DirectedScene


class CompletingTheSquare(DirectedScene):
    symbols = {"a": "primary", "b": "secondary", "c": "accent"}

    def step(self, tex: str):
        return self.math(tex, font_size=80)

    def construct(self):
        with self.beat("derive", transition="reveal", hold=0.4):
            self.title("Where the quadratic formula comes from")
            steps = self.derive(
                self.step(r"a x^2 + b x + c = 0"),
                (self.step(r"x^2 + \frac{b}{a} x = -\frac{c}{a}"), "divide by $a$"),
                (
                    self.step(
                        r"x^2 + \frac{b}{a} x + \frac{b^2}{4a^2} = \frac{b^2}{4a^2} - \frac{c}{a}"
                    ),
                    "add $(b/2a)^2$",
                ),
                (
                    self.step(r"\left(x + \frac{b}{2a}\right)^2 = \frac{b^2 - 4ac}{4a^2}"),
                    "complete the square",
                ),
                (
                    self.step(r"x + \frac{b}{2a} = \pm \frac{\sqrt{b^2 - 4ac}}{2a}"),
                    "square roots",
                ),
                (
                    self.step(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
                    "subtract $b/2a$",
                ),
                in_place=True,
            )

        with self.beat("result", keep=[steps], aha=True, hold=1.6):
            self.highlight(steps.lines[-1], r"b^2 - 4ac", color=None, box=True)

        with self.beat("loop", transition="chapter", hold=0.2):
            pass
