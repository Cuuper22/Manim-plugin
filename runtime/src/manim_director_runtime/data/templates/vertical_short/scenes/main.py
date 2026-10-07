"""Gauss's sum as a 9:16 short: two staircases of blocks make a rectangle."""

from manim import *

from manim_director_runtime import DirectedScene, Region


class TriangularNumbers(DirectedScene):
    symbols = {"n": "primary"}

    def staircase(self, n, cell):
        """Column k holds k blocks, for k = 1..n."""

        return VGroup(
            *(
                Square(cell * 0.88, stroke_width=0)
                .set_fill(self.theme.primary, opacity=0.9)
                .move_to([k * cell, j * cell, 0])
                for k in range(n)
                for j in range(k + 1)
            )
        )

    def construct(self):
        question = self.math(r"1 + 2 + 3 + \cdots + 100")
        with self.beat("question", transition="reveal"):
            self.title("Add 1 to 100, fast")
            self.place(question)
            self.caption("Draw the sum instead of adding it.")

        n, cell = 5, 0.34
        stairs = self.staircase(n, cell)
        terms = self.math(r"1 + 2 + 3 + 4 + 5")
        with self.beat("staircase"):
            self.place(stairs, region=Region.TOP, anchor=(0, -1))
            self.place(terms, region=Region.BOTTOM, replaces=question)
            self.caption("Each term is a column of blocks.")

        with self.beat("double", keep=[stairs, terms]):
            self.caption("A turned copy completes a rectangle.")
            flipped = stairs.copy().set_fill(self.theme.secondary)
            self.play(FadeIn(flipped))
            pivot = stairs.get_center() + UP * stairs[0].width / 0.88 / 2
            self.play(Rotate(flipped, PI, about_point=pivot), run_time=1.4)

        rectangle = VGroup(stairs, flipped)
        width = Brace(rectangle, DOWN, color=self.theme.muted)
        height = Brace(rectangle, LEFT, color=self.theme.muted)
        # Through self.math, so n has its symbol color here too.
        labels = VGroup(
            self.math("n").next_to(width, DOWN), self.math("n + 1").next_to(height, LEFT)
        )
        with self.beat("count", keep=[rectangle, terms]):
            self.place(VGroup(rectangle, width, height, labels), region=Region.TOP)
            self.caption("Each staircase is half the rectangle.")
            formula = self.derive(
                r"2(1 + \cdots + n) = n(n + 1)",
                r"1 + \cdots + n = \frac{n(n + 1)}{2}",
                region=Region.BOTTOM,
                replaces=terms,
            )

        with self.beat("answer", keep=[rectangle, width, height, labels]):
            self.caption("Put n = 100.")
            answer = self.derive(
                r"1 + \cdots + 100 = \frac{100 \cdot 101}{2}",
                r"= 5050",
                region=Region.BOTTOM,
                replaces=formula.lines[-1],
            )
        self.highlight(answer.lines[-1], "5050", box=True)
        self.wait()
