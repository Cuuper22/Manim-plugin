"""Misconception, then repair: does the square root split over a sum?

The viewer is invited to believe the tempting rule, tests it with numbers, sees it fail, and
then sees why on a right triangle: √a and √b are its legs and √(a + b) its long side, so
straight across is never longer than around.
"""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import Figure, hide


class RootOfASum(DirectedScene):
    def construct(self):
        product = self.math(r"\sqrt{4 \cdot 9} = \sqrt{4} \cdot \sqrt{9} = 6")
        with self.beat("hook", transition="reveal"):
            self.title("Does the square root split over a sum?")
            self.place(product)
            self.caption("Over a product it does: both sides make 6.")

        with self.beat("predict"):
            card = self.misconception(r"\sqrt{a + b} = \sqrt{a} + \sqrt{b}")
            self.ask("Does it split over sums too? Decide first.")

        with self.beat("test", keep=[card]):
            self.caption("Try it with a = 9 and b = 16.")
            self.play(
                card.test(r"\sqrt{9 + 16} = \sqrt{25} = 5", r"\sqrt{9} + \sqrt{16} = 3 + 4 = 7"),
                run_time=2,
            )

        with self.beat("refute", keep=[card]):
            self.caption("5 is not 7: the shortcut fails.")
            self.play(card.refute())

        triangle = Figure({"A": (0, 0), "B": (4, 0), "C": (0, 3)}, unit=1.05, labels=False)
        triangle.polygon("ABC")
        triangle.right_angle("CAB")
        triangle.length("AC", r"\sqrt{a}")
        triangle.length("AB", r"\sqrt{b}")
        across = triangle.segment("BC")
        with self.beat("picture", keep=[card]):
            self.place(triangle, region="right")
            self.caption("Put √a and √b at a right angle.")

        long_side = triangle.length("BC", r"\sqrt{a + b}")
        with self.beat("hypotenuse", keep=[card, triangle], aha=True):
            self.caption("Pythagoras: the long side is √(a + b).")
            self.show(long_side, run_time=2)
            self.annotate(long_side, r"$(\sqrt{a})^2 + (\sqrt{b})^2 = a + b$")

        with self.beat("repair", keep=[card, triangle]):
            self.caption("For a, b ≥ 0, straight across is never longer than around.")
            self.play(card.repair(r"\sqrt{a + b} \le \sqrt{a} + \sqrt{b}"), run_time=2)
            self.link(self.term(card.fix, r"\sqrt{a + b}"), across)

        with self.beat("recap", keep=[card, triangle]):
            self.caption("Roots split over products, not over sums.")
            self.play(hide(long_side))
            self.show(long_side, run_time=1)
