"""Before and after: does a shear change area?

A shear tilts the unit square into a parallelogram that keeps its base and its height, so its
area stays 1. A stretch beside it, at the same scale, shows what a change of area looks like.
"""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import Readout, VectorGrid, reserve

SHEAR = [[1, 1], [0, 1]]
STRETCH = [[2, 0], [0, 1]]


class ShearKeepsArea(DirectedScene):
    def shear(self, grid, run_time):
        """The aha: the grid shears and the square tilts."""

        self.play(grid.apply(SHEAR), run_time=run_time)

    def construct(self):
        grid = VectorGrid(extent=1, unit=1.3, fits=[SHEAR])
        square = grid.unit_square(color="accent", opacity=0.3)
        with self.beat("hook", transition="reveal"):
            self.title("Does a shear change area?")
            self.place(grid, region="left")
            self.caption("The square on î and ĵ has area 1.")
            self.show(square)

        area = Readout("area", grid.det)
        panel = VGroup(grid.matrix(SHEAR), area).arrange(DOWN, buff=0.7)
        with self.beat("predict", keep=[grid, square]):
            self.place(panel, region="right")
            self.ask("After this shear: bigger, smaller, or the same?")

        with self.beat("shear", keep=[grid, square, panel], aha=True):
            self.caption("The square tilts, but its area stays 1.")
            self.shear(grid, run_time=2.5)

        with self.beat("why", keep=[grid, square, area]):
            self.caption("Same base, same height: the same area.")
            self.annotate(grid.i_hat, "base 1", style="brace")
            height = DashedLine(grid.to_point((0, 0)), grid.to_point((0, 1)), color=self.theme.accent)
            self.pause()
            self.play(Create(height))
            self.annotate(height, "height 1", style="label", side="left")

        stretched = VectorGrid(extent=1, unit=1.3, fits=[STRETCH], basis=False)
        wide = stretched.unit_square(color="accent", opacity=0.3)
        doubled = reserve(Readout("area", stretched.det))
        with self.beat("contrast", keep=[grid, square, area]):
            self.place(VGroup(grid, area).arrange(DOWN, buff=0.4), region="left")
            self.place(VGroup(stretched, doubled).arrange(DOWN, buff=0.4), region="right")
            self.caption("Now a stretch, at the same scale.")
            self.show(wide)

        with self.beat("stretch", keep=[grid, square, area, stretched, wide, doubled]):
            self.caption("A stretch is different: the area doubles.")
            self.show(doubled)
            self.pause()
            self.play(stretched.apply(STRETCH), run_time=2)

        with self.beat("recap", keep=[grid, square, area, stretched, wide, doubled]):
            self.caption("A shear keeps every area; a stretch changes it.")
            self.play(grid.reset())
            self.shear(grid, run_time=1.5)
