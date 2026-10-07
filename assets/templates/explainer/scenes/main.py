"""A recurrence, seen as motion: one number row, one inference per beat."""

from manim import *

from manim_director_runtime import DirectedScene, Region


class DirectedRecurrence(DirectedScene):
    def cell(self, label, ring, ink):
        return VGroup(
            Circle(radius=0.42, color=ring, stroke_width=3), self.text(label, "heading", color=ink)
        )

    def number_row(self, values, *, missing=False):
        cells = [self.cell(str(v), self.theme.muted, self.theme.foreground) for v in values]
        if missing:
            cells.append(self.cell("?", self.theme.accent, self.theme.accent))
        return VGroup(*cells).arrange(RIGHT, buff=0.24)

    def construct(self):
        row = self.number_row([1, 1, 2, 3, 5], missing=True)
        with self.beat("hook", focus=row, transition="reveal"):
            self.title("Where does the next number come from?")
            self.place(row)
            self.caption("Keep your eye on the last two values.")

        with self.beat("construction"):
            self.place(row, region=Region.TOP)
            total = self.math("3 + 5 = 8")
            self.place(total, region=Region.BOTTOM)
            window = SurroundingRectangle(
                row[-3:-1], color=self.theme.primary, buff=0.12, corner_radius=0.1
            )
            self.play(Create(window))
            self.caption("Adding the last two values produces the next one.")

        with self.beat("rule"):
            self.place(self.number_row([1, 1, 2, 3, 5, 8]), region=Region.TOP, replaces=row)
            self.place(self.math(r"a_{n+2} = a_{n+1} + a_n"), region=Region.BOTTOM)
            self.caption("The symbols name the motion we already saw.")
        self.wait()
