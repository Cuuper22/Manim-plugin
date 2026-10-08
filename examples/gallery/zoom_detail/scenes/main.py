"""Zoom in on a detail: why is sin(0.01) almost exactly 0.01?

Far from 0 the graphs of sin x and y = x part ways; magnified around 0 they are one line. The
chord from the origin then measures it: its slope, sin x / x, heads to 1.
"""

import numpy as np
from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import FunctionPlot, Readout


class SinNearZero(DirectedScene):
    def zoom(self, plot, *below):
        """The aha: a magnified window on the plot around 0."""

        inset = plot.inset(around=(0, 0), radius=0.25)
        self.place(inset, *below, region="right")
        return inset

    def construct(self):
        values = VGroup(
            self.math(r"\sin(0.1) = 0.0998\ldots"),
            self.math(r"\sin(0.01) = 0.0099998\ldots"),
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.4)
        with self.beat("hook", transition="reveal"):
            self.title("Why is sin(0.01) almost 0.01?")
            self.place(values)
            self.caption("A calculator keeps handing back almost the input.")

        plot = FunctionPlot(
            np.sin,
            lambda x: x,
            x_range=(-1.2, 3.2),
            y_range=(-1.2, 1.6),
            labels=[r"\sin x", "y = x"],
        )
        with self.beat("plot", keep=[values]):
            self.place(values, region="right")
            self.place(plot, region="left")
            self.caption("Far from 0, the two graphs clearly differ.")

        with self.beat("predict", keep=[plot]):
            self.ask("Zoom in near 0. What will the two graphs look like?")

        with self.beat("zoom", keep=[plot], transition="reveal", run_time=2, aha=True):
            self.zoom(plot)
            self.caption("Up close, the two graphs lie on top of each other.")

        x = ValueTracker(1.0)
        readouts = VGroup(
            Readout("x", x),
            Readout(r"\frac{\sin x}{x}", lambda: np.sin(x.get_value()) / x.get_value(), decimals=3),
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.5)
        with self.beat("measure", keep=[plot]):
            self.place(readouts, region="right")
            self.caption("As x shrinks, the slope sin x / x heads to 1.")
            self.show(plot.secant(0, x))
            self.pause()
            self.play(x.animate.set_value(0.05), run_time=3)

        with self.beat("recap", keep=[plot], transition="reveal"):
            self.zoom(plot, self.math(r"\sin x \approx x"))
            self.caption("Near 0, in radians, sine is almost straight: sin x ≈ x.")
