"""Taylor polynomials of sin x: each new term bends the polynomial along more of the sine, and
all of the terms together are the sine itself."""

from math import factorial

import numpy as np
from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import FunctionPlot

X, Y = 6.5, 2.2  # the plot's window: |x| <= X, |y| <= Y


def taylor(degree: int):
    """The Taylor polynomial of sin x at 0, up to x^degree."""

    terms = [(k, (-1) ** (k // 2) / factorial(k)) for k in range(1, degree + 1, 2)]
    return lambda x: sum(c * x**k for k, c in terms)


def partial(degree: int) -> str:
    """sin x ≈ x − x³/3! + … up to x^degree, in TeX."""

    terms = "".join(
        rf" {'-' if k % 4 == 3 else '+'} \frac{{x^{{{k}}}}}{{{k}!}}"
        for k in range(3, degree + 1, 2)
    )
    return r"\sin x \approx x" + terms


class TaylorSine(DirectedScene):
    symbols = {r"\sin": "primary"}

    def polynomial(self, plot: FunctionPlot, degree: int) -> VMobject:
        """The polynomial's graph over the stretch around 0 where it stays inside the window."""

        p = taylor(degree)
        xs = np.linspace(0, X, 651)
        outside = np.abs(p(xs)) > Y
        reach = xs[np.argmax(outside) - 1] if outside.any() else X  # p is odd: symmetric
        return plot.axes.plot(p, x_range=(-reach, reach), color=self.theme.accent, stroke_width=5)

    def construct(self):
        plot = FunctionPlot(
            np.sin, x_range=(-X, X), y_range=(-Y, Y), size=(12, 2.8), labels=[r"\sin x"]
        )
        formula = self.math(partial(1), font_size=56)

        with self.beat("line", transition="reveal", hold=0.2):
            self.title("Polynomials that turn into the sine")
            self.place(plot, region="content", anchor=(0, -1))
            self.place(formula, region="top", anchor=(0, 1))
            self.pause()
            approx = self.polynomial(plot, 1)
            self.play(Create(approx), run_time=1)

        with self.beat("terms", keep=[plot, approx, formula], hold=0.3):
            for degree in (3, 5, 7, 9):
                longer = self.math(partial(degree), font_size=56)
                self.place(longer, region="top", anchor=(0, 1), replaces=formula)
                self.play(Transform(approx, self.polynomial(plot, degree)), run_time=1.2)
                self.wait(0.3)
                formula = longer

        series = self.math(
            r"\sin x = \sum_{n=0}^{\infty} \frac{(-1)^n\, x^{2n+1}}{(2n+1)!}", font_size=52
        )
        with self.beat("series", keep=[plot, approx, formula], aha=True, hold=1.6):
            self.place(series, region="top", anchor=(0, 1), replaces=formula)
            self.play(Transform(approx, self.polynomial(plot, 41)), run_time=1.5)
            self.highlight(series, color=None, box=True)

        with self.beat("loop", transition="chapter", hold=0.2):
            pass
