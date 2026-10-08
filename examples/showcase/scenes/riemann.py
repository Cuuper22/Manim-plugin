"""Riemann sums close in on the area under y = x² on [0, 2], which is exactly 8/3."""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import FunctionPlot, Readout


def left_sum(n: int) -> float:
    """Left Riemann sum of x² on [0, 2] with n bars."""

    width = 2 / n
    return sum((k * width) ** 2 * width for k in range(n))


class RiemannToIntegral(DirectedScene):
    symbols = {"L_n": "primary"}

    def construct(self):
        plot = FunctionPlot(
            lambda x: x**2, x_range=(0, 2.2), y_range=(0, 4.4), size=(6.4, 5.2), labels=["y = x^2"]
        )
        bars = plot.riemann(0, 2, 4)
        count = Readout("n", lambda: bars.riemann.n, decimals=0, width=2)
        total = Readout("L_n", lambda: left_sum(bars.riemann.n), decimals=4, color="primary")
        readouts = VGroup(count, total).arrange(DOWN, aligned_edge=LEFT, buff=0.45)

        with self.beat("bars", transition="reveal", hold=0.3):
            self.title("How much area is under y = x²?")
            self.place(plot, region="left")
            self.place(readouts, region="right")
            self.show(bars, run_time=1)

        with self.beat("refine", keep=[plot, readouts, bars], hold=0.3):
            for _ in range(4):  # 4 → 8 → 16 → 32 → 64 bars
                self.play(plot.refine(bars), run_time=0.8)
                self.wait(0.2)

        exact = self.math(r"\int_0^2 x^2\,dx = \frac{8}{3} \approx 2.6667", font_size=56)
        with self.beat("limit", keep=[plot, readouts], aha=True, hold=1.8):
            self.show(plot.area(0, 2), run_time=0.8)
            self.place(readouts, exact, region="right")
            self.highlight(exact, r"\frac{8}{3}", box=True)

        with self.beat("loop", transition="chapter", hold=0.2):
            pass
