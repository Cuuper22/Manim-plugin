"""Concrete first: you test positive, so how worried should you be?

One person in 100 is sick and tests positive; the test also flags 5 of the 99 healthy people.
Pull everyone who tested positive out of the crowd and count: 1 of 6 is sick.
"""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import DotArray

SICK = (2, 6)
FLAGGED = [(0, 3), (4, 8), (6, 1), (7, 5), (9, 2)]


class PositiveTest(DirectedScene):
    def construct(self):
        crowd = DotArray(10, radius=0.15, gap=0.22)
        sick = crowd.at(*SICK)
        flagged = VGroup(*(crowd.at(*rc) for rc in FLAGGED))
        with self.beat("people", transition="reveal"):
            self.title("You test positive. How worried should you be?")
            self.place(crowd, region="left")
            self.caption("Picture 100 people. 1 of them is sick.")

        with self.beat("sick", keep=[crowd]):
            self.caption("This one is sick, and tests positive.")
            self.play(crowd.paint(lambda r, c: (r, c) == SICK, "accent"))
            self.annotate(sick, "sick", side=RIGHT)

        with self.beat("flagged", keep=[crowd]):
            self.caption("The test also flags 5 healthy people.")
            self.play(crowd.paint(lambda r, c: (r, c) in FLAGGED, "secondary"), run_time=1.5)

        with self.beat("predict", keep=[crowd]):
            self.ask("You test positive. How likely is it you are sick?")

        with self.beat("gather", keep=[crowd], run_time=2, aha=True):
            positives = self.place(sick, *flagged, region="right", direction=RIGHT, buff=0.3)
            self.caption("Everyone who tested positive: 6 people.")

        answer = self.math(r"\frac{1}{1 + 5} = \frac{1}{6} \approx 17\%", font_size=56)
        with self.beat("answer", keep=[crowd, positives]):
            self.place(positives, answer, region="right")
            self.caption("Only 1 of these 6 is sick.")
            self.link(self.term(answer, "1", occurrence=0), sick)

        with self.beat("recap", keep=[crowd, positives, answer]):
            self.caption("About 17%, not 95%. Count the positives first.")
            self.focus(positives, answer)
