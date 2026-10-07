"""Halving forever adds up to one: a picture first, then the algebra that names it."""

from manim import *

from manim_director_runtime import DirectedScene, Region

FRACTIONS = (r"\frac{1}{2}", r"\frac{1}{4}", r"\frac{1}{8}")


class GeometricSeries(DirectedScene):
    symbols = {"S_n": "primary"}

    def halves(self, square, count):
        """`count` tiles, each covering half of what the earlier ones left empty."""

        (left, top, _), width, height = square.get_corner(UL), square.width, square.height
        tiles = []
        for k in range(count):
            w, h = (width / 2, height) if k % 2 == 0 else (width, height / 2)
            tile = Rectangle(width=w - 0.06, height=h - 0.06, stroke_width=0)
            tile.set_fill(self.theme.primary, opacity=1 - 0.08 * k)
            tile.move_to([left + w / 2, top - h / 2, 0])
            if k < len(FRACTIONS):
                label = MathTex(FRACTIONS[k], color=self.theme.background)
                tile.add(label.scale_to_fit_height(min(0.8, w / 2, h / 2)).move_to(tile))
            tiles.append(tile)
            if k % 2 == 0:  # the empty part keeps its right half, then its bottom half
                left, width = left + w, width - w
            else:
                top, height = top - h, height - h
        return tiles

    def construct(self):
        square = Square(4.2, color=self.theme.muted, stroke_width=3)
        with self.beat("hook", transition="reveal"):
            self.title("Can infinitely many pieces add up to one?")
            self.place(square)
            self.caption("Start with one whole square.")

        figure = VGroup(square)
        with self.beat("halving", keep=[square]):
            self.caption("Each piece covers half of what is still empty.")
            for k, tile in enumerate(self.halves(square, 6)):
                figure.add(tile)
                self.play(FadeIn(tile, scale=0.94), run_time=0.9 if k < 3 else 0.35)

        partial = self.math(r"S_n = \frac{1}{2} + \frac{1}{4} + \cdots + \frac{1}{2^n}")
        with self.beat("gap"):
            self.place(figure, region=Region.LEFT)
            self.place(partial, region=Region.RIGHT)
            self.caption("After n pieces, the empty corner is exactly 1/2ⁿ of the square.")
            last = figure[-1]
            gap = Rectangle(width=last.width, height=last.height, color=self.theme.accent)
            gap.set_fill(self.theme.accent, opacity=0.25).align_to(square, DR)
            self.play(GrowFromCenter(gap))

        with self.beat("limit", keep=[figure, gap]):
            proof = self.derive(
                r"S_n = \frac{1}{2} + \frac{1}{4} + \cdots + \frac{1}{2^n}",
                r"= 1 - \frac{1}{2^n}",
                r"\to 1",
                region=Region.RIGHT,
                replaces=partial,
            )
            self.caption("The gap halves forever, so the pieces fill the square exactly.")
        self.highlight(proof.lines[-1], "1", box=True)
        self.wait()
