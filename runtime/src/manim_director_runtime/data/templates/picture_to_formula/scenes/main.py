"""Picture to formula: why do the odd numbers add up to squares?

Each odd number is an L of dots that wraps a k×k square into a (k+1)×(k+1) one: one arm of k,
another arm of k, and a corner. The picture comes first; the formula only names what the viewer
has already seen, with n = 4 on screen, and then answers the question for 19.
"""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import DotArray, reserve

SIZE = 48  # the sums are the hero until the picture arrives
COLORS = ("primary", "secondary")  # the Ls alternate, so the square reads 1 + 3 + 5 + 7


class OddSquares(DirectedScene):
    def column(self, *rows):
        """Equations stacked with their = signs lined up."""

        lines = VGroup(*(self.math(row, font_size=SIZE) for row in rows)).arrange(DOWN, buff=0.3)
        right = max(self.term(line, "=").get_x() for line in lines)
        for line in lines:
            line.shift((right - self.term(line, "=").get_x()) * RIGHT)
        return lines

    def wrap(self, dots, k, run_time=1.0):
        """The aha: the L of 2k + 1 dots grows down one arm, then along the other to the
        corner (a selection keeps the array's row-by-row order)."""

        ell = dots.select(lambda r, c: max(r, c) == k, color=COLORS[k % 2])
        self.show(ell, run_time=run_time)
        return ell

    def construct(self):
        sums = self.column(
            "1 = 1",
            "1 + 3 = 4",
            "1 + 3 + 5 = 9",
            "1 + 3 + 5 + 7 = 16",
            r"1 + 3 + \cdots + (2n-1) = n^2",
            r"1 + 3 + \cdots + 19 =",
        )
        answer = self.math("10^2 = 100", font_size=SIZE).next_to(sums[5], RIGHT, buff=0.25)
        sums.add(answer)
        reserve(VGroup(sums[3], sums[4], sums[5], answer))
        with self.beat("hook", transition="reveal"):
            self.title("Why do odd numbers add up to squares?")
            self.place(sums)
            self.caption("1, 4, 9: all squares so far.")

        with self.beat("predict", keep=[sums]):
            self.show(VGroup(sums[3], sums[5]))
            self.ask("Keep going to 19. Still a square?")

        dots = DotArray(4, shown=lambda r, c: r == c == 0, radius=0.24, gap=0.38, color=COLORS[0])
        with self.beat("picture", keep=[sums]):
            self.place(sums, region="right")
            self.place(dots, region="left")
            self.caption("Draw 3 as an L around 1.")
            three = self.wrap(dots, 1)
            self.link(self.term(sums[1], "3"), three)

        with self.beat("again", keep=[sums, dots]):
            self.caption("5 wraps the 2×2 the same way.")
            five = self.wrap(dots, 2)
            self.link(self.term(sums[2], "5"), five)

        with self.beat("why", keep=[sums, dots], aha=True):
            self.caption("Watch 7: arm, arm, corner.")
            seven = self.wrap(dots, 3, run_time=2)
            self.annotate(dots.select(lambda r, c: c == 3 and r < 3), "3", style="brace")
            self.annotate(dots.select(lambda r, c: r == 3 and c < 3), "3", style="brace")
            self.link(self.term(sums[3], "7"), seven)

        with self.beat("formula", keep=[sums, dots]):
            self.caption("The 4th L has 2·4 − 1 = 7 dots.")
            self.show(sums[4])
            self.link(self.term(sums[4], "(2n-1)"), seven)

        with self.beat("recap", keep=[sums, dots]):
            self.caption("19 is the 10th odd number: 10 Ls, 10×10.")
            self.pause()
            self.play(dots.hide(lambda r, c: max(r, c) == 3))
            self.wrap(dots, 3, run_time=1)
            self.show(answer)
