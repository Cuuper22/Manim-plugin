"""A matrix moves the whole plane; the unit square's image has area |det A| = 3."""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import Readout, VectorGrid

A = [[2, 1], [-1, 1]]  # det A = 2·1 − 1·(−1) = 3


class DeterminantIsArea(DirectedScene):
    def construct(self):
        grid = VectorGrid(extent=1, unit=1.3, fits=[A])
        square = grid.unit_square(color="accent", opacity=0.35)
        area = Readout("area", grid.det, decimals=2, color="accent")
        matrix = self.math(r"A = \begin{pmatrix} 2 & 1 \\ -1 & 1 \end{pmatrix}", font_size=56)
        side = VGroup(matrix, area).arrange(DOWN, aligned_edge=LEFT, buff=0.6)

        with self.beat("plane", transition="reveal", hold=0.3):
            self.title("A matrix stretches area by det A")
            self.place(grid, region="left")
            self.show(square, run_time=0.8)
            self.place(side, region="right")

        with self.beat("apply", keep=[grid, square, side], aha=True, hold=0.6):
            self.pause()
            self.play(grid.apply(A), run_time=2.5)

        det = self.math(r"\det A = 2 \cdot 1 - 1 \cdot (-1) = 3", font_size=48)
        with self.beat("det", keep=[grid, square, side], hold=1.4):
            self.place(side, det, region="right")
            self.highlight(det, "3", box=True)

        with self.beat("loop", transition="chapter", hold=0.2):
            pass
