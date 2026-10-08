"""The derivative as a limit: as h shrinks, the secant through x = 1 turns into the tangent of
y = x² there, and its slope 2 + h settles on f'(1) = 2."""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import FunctionPlot, Readout


class SecantToTangent(DirectedScene):
    symbols = {"h": "accent"}

    def construct(self):
        plot = FunctionPlot(
            lambda x: x**2,
            x_range=(-0.4, 2.6),
            y_range=(-0.6, 5.4),
            size=(6.4, 5.2),
            labels=["y = x^2"],
        )
        h = ValueTracker(1.3)
        secant = plot.secant(1, h, labels=("h", r"f(1+h) - f(1)"))
        slope = Readout(
            "slope",
            lambda: ((1 + h.get_value()) ** 2 - 1) / h.get_value(),
            decimals=3,
            color="accent",
        )
        readouts = VGroup(Readout("h", h, decimals=3), slope).arrange(
            DOWN, aligned_edge=LEFT, buff=0.45
        )

        with self.beat("secant", transition="reveal", hold=0.3):
            self.title("The slope at x = 1, as a limit")
            self.place(plot, region="left")
            self.show(plot.dot(1, label="P"), secant, run_time=1)
            self.place(readouts, region="right")

        with self.beat("shrink", keep=[plot, readouts, secant], aha=True, hold=0.6):
            self.pause()
            self.play(h.animate.set_value(0.001), run_time=3.5, rate_func=smooth)

        limit = self.math(r"f'(1) = \lim_{h \to 0} \frac{f(1+h) - f(1)}{h} = 2", font_size=52)
        with self.beat("definition", keep=[plot, secant], hold=1.6):
            self.place(limit, region="right")
            self.highlight(limit, "2", box=True)

        with self.beat("loop", transition="chapter", hold=0.2):
            pass
