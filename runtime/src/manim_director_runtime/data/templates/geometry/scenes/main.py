"""Pythagoras by rearrangement: the same four triangles leave either c² or a² + b² uncovered."""

from manim import *

from manim_director_runtime import DirectedScene, Region


class PythagoreanRearrangement(DirectedScene):
    symbols = {"a": "primary", "b": "secondary", "c": "accent"}

    def triangle(self, *corners):
        return Polygon(
            *corners,
            stroke_color=self.theme.foreground,
            stroke_width=2,
            fill_color=self.theme.muted,
            fill_opacity=0.4,
        )

    def patch(self, corners, token, label):
        shape = Polygon(*corners, stroke_width=0, fill_color=self.theme.color(token))
        shape.set_fill(opacity=0.35)
        return VGroup(shape, self.math(label).move_to(shape))

    def construct(self):
        a, b = 1.6, 2.6
        s = a + b
        frame = Square(s, color=self.theme.foreground, stroke_width=3)

        def at(x, y):
            """A point of the big square, measured from its lower-left corner in units of a, b."""
            return frame.get_corner(DL) + frame.width / s * (x * RIGHT + y * UP)

        triangles = VGroup(
            self.triangle(at(0, 0), at(b, 0), at(0, a)),
            self.triangle(at(s, 0), at(s, b), at(b, 0)),
            self.triangle(at(s, s), at(a, s), at(s, b)),
            self.triangle(at(0, s), at(0, a), at(a, s)),
        )
        sides = VGroup(
            self.math("a").next_to(at(b + a / 2, 0), DOWN),
            self.math("b").next_to(at(s, b / 2), RIGHT),
        )
        tilted = self.patch([at(b, 0), at(s, b), at(a, s), at(0, a)], "accent", "c^2")
        before = self.math(r"(a + b)^2 = c^2 + 2ab")
        with self.beat("square", transition="reveal"):
            self.title("Four triangles prove Pythagoras")
            self.place(VGroup(triangles, frame, sides, tilted), region=Region.LEFT)
            self.place(before, region=Region.RIGHT)
            self.caption("Four right triangles of area ab/2 leave a tilted square of area c².")

        after = self.math(r"(a + b)^2 = a^2 + b^2 + 2ab")
        with self.beat("rearrange", keep=[triangles, frame, sides]):
            self.place(after, region=Region.RIGHT, replaces=before)
            self.caption("Slide three of them: the same square now leaves a² and b².")
            self.play(
                triangles[0].animate.shift(at(0, b) - at(0, 0)),
                triangles[2].animate.shift(at(b, s) - at(s, s)),
                triangles[3].animate.shift(at(b, b) - at(0, s)),
                run_time=2,
            )
            squares = VGroup(
                self.patch([at(0, 0), at(b, 0), at(b, b), at(0, b)], "secondary", "b^2"),
                self.patch([at(b, b), at(s, b), at(s, s), at(b, s)], "primary", "a^2"),
            )
            self.play(FadeIn(squares))

        result = self.math(r"c^2 = a^2 + b^2", font_size=64)
        with self.beat("conclude", keep=[triangles, frame, sides, squares]):
            self.place(result, region=Region.RIGHT, replaces=after)
            self.caption("Both are (a + b)² minus the same four triangles, so c² = a² + b².")
        box = SurroundingRectangle(result, color=self.theme.muted, buff=0.3, corner_radius=0.12)
        self.play(Create(box))
        self.wait()
