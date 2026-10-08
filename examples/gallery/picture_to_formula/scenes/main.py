"""Picture to formula: why do the odd numbers add up to squares?

Each odd number is an L of dots that wraps a k×k square into a (k+1)×(k+1) one. The picture
comes first; the formula only names what the viewer has already seen.
"""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import DotArray, reserve

SIZE = 56  # the sums are the hero until the picture arrives


def ell(k):
    """The dots of the L that turns a k×k square into a (k+1)×(k+1) one."""

    return lambda r, c: max(r, c) == k


class OddSquares(DirectedScene):
    def column(self, *rows):
        """Equations stacked with their = signs lined up."""

        lines = VGroup(*(self.math(row, font_size=SIZE) for row in rows)).arrange(DOWN, buff=0.3)
        right = max(self.term(line, "=").get_x() for line in lines)
        for line in lines:
            line.shift((right - self.term(line, "=").get_x()) * RIGHT)
        return lines

    def wrap(self, dots, k, run_time=1.0):
        """The k-th L arrives in accent; the square it wraps turns primary first."""

        if k > 1:
            self.play(dots.paint(ell(k - 1), "primary"), run_time=0.6)
        ring = dots.select(ell(k), color="accent")
        self.show(ring, run_time=run_time)
        return ring

    def construct(self):
        *done, last = self.column("1 = 1", "1 + 3 = 4", "1 + 3 + 5 = 9", "1 + 3 + 5 + 7 =")
        guess = self.math("?", font_size=SIZE).next_to(last, RIGHT, buff=0.25)
        sums = VGroup(*done, reserve(VGroup(last, guess)))
        with self.beat("hook", transition="reveal"):
            self.title("Why do odd numbers add up to squares?")
            self.place(sums)
            self.caption("Every total is a square: 1×1, 2×2, 3×3.")

        with self.beat("predict", keep=[sums]):
            self.show(sums[3])
            self.ask("Add the next odd number, 7. Still a square?")

        dots = DotArray(4, shown=lambda r, c: r == c == 0, radius=0.25, gap=0.4, color="primary")
        with self.beat("picture", keep=[sums]):
            self.place(sums, region="right")
            self.place(dots, region="left")
            self.caption("Draw each odd number as an L around the square.")
            three = self.wrap(dots, 1)
            self.link(self.term(sums[1], "3"), three)

        with self.beat("again", keep=[sums, dots]):
            self.caption("The next odd number wraps the square again.")
            five = self.wrap(dots, 2)
            self.link(self.term(sums[2], "5"), five)

        with self.beat("why", keep=[sums, dots], aha=True):
            self.caption("Two sides of 3, plus a corner: 7 dots.")
            seven = self.wrap(dots, 3, run_time=2)
            self.pause()
            self.play(Transform(guess, self.math("4^2", font_size=SIZE).move_to(guess, LEFT)))
            self.annotate(dots.select(lambda r, c: r == 3 and c < 3), "3", style="brace")
            self.annotate(dots.select(lambda r, c: c == 3 and r < 3), "3", style="brace")

        with self.beat("formula", keep=[dots, sums[3]]):
            self.caption("The n-th L has 2n − 1 dots.")
            formula = self.derive(
                self.math(r"1 + 3 + \cdots + (2n-1) = n^2", font_size=SIZE),
                region="right",
                replaces=sums[3],
            )
            self.link(self.term(formula.lines[0], "(2n-1)"), seven)

        with self.beat("recap", keep=[dots, formula]):
            self.caption("Odd numbers are the Ls of a growing square.")
            self.play(dots.hide(ell(3)))
            self.show(seven, run_time=1)
            self.highlight(formula.lines[0], color=None, box=True)
