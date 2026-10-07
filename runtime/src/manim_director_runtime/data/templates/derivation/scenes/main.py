"""The quadratic formula, derived by completing the square one justified step at a time."""

from manim import *

from manim_director_runtime import DirectedScene


class QuadraticFormula(DirectedScene):
    symbols = {"a": "primary", "b": "secondary", "c": "accent"}

    def construct(self):
        claim = self.math(r"ax^2 + bx + c = 0")
        with self.beat("claim", transition="reveal"):
            self.title("Where the quadratic formula comes from")
            self.place(claim)
            self.caption("Any quadratic equation with a ≠ 0.")

        with self.beat("complete"):
            self.caption("Add exactly what turns the left side into a square.")
            square = self.derive(
                r"ax^2 + bx + c = 0",
                (r"x^2 + \frac{b}{a}x = -\frac{c}{a}", "divide by a"),
                (
                    r"x^2 + \frac{b}{a}x + \frac{b^2}{4a^2} = \frac{b^2}{4a^2} - \frac{c}{a}",
                    "add the same term to both sides",
                ),
                (r"\left(x + \frac{b}{2a}\right)^2 = \frac{b^2 - 4ac}{4a^2}", "a perfect square"),
                replaces=claim,
            )

        with self.beat("solve"):
            self.caption("A square has two square roots, so x has two values.")
            steps = self.derive(
                r"\left(x + \frac{b}{2a}\right)^2 = \frac{b^2 - 4ac}{4a^2}",
                (r"x + \frac{b}{2a} = \pm\frac{\sqrt{b^2 - 4ac}}{2a}", "take square roots"),
                (r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", "subtract b/2a"),
                replaces=square.lines[-1],
            )

        formula = self.math(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}", font_size=72)
        with self.beat("discriminant"):
            self.caption("The discriminant b² − 4ac decides: two, one or no real roots.")
            self.place(formula, replaces=steps.lines[-1])
            self.tag(formula)
        self.highlight(formula, r"b^2 - 4ac", box=True)
        self.wait()
