"""Concrete first: you test positive, so how worried should you be?

One person in 100 is sick, and the test flags them; it also flags 5% of the 99 healthy people,
about 5. Ring everyone who tests positive, line them up and count: 1 of 6 is sick. The rates
come first, then the prediction, then the evidence, so the viewer cannot count the answer
before they guess.
"""

from manim import *

from manim_director_runtime import DirectedScene
from manim_director_runtime.kit import DotArray, label
from manim_director_runtime.themes import Role

SICK = (3, 9)
FLAGGED = [(0, 3), (4, 6), (6, 1), (7, 5), (9, 2)]


def positive(r, c):
    return (r, c) == SICK or (r, c) in FLAGGED


class PositiveTest(DirectedScene):
    def line_up(self, crowd):
        """The aha: everyone who tested positive steps out of the crowd into a row (copies,
        so the crowd keeps its 100 people)."""

        copies = [crowd.at(*SICK).copy(), *(crowd.at(*rc).copy() for rc in FLAGGED)]
        self.add(*copies)
        return self.place(*copies, region="right", direction=RIGHT, buff=0.3)

    def count(self, positives):
        """The answer under the row, its terms linked to the people they count."""

        answer = self.math(r"\frac{1}{1 + 5} = \frac{1}{6} \approx 17\%", font_size=56)
        self.place(positives, answer, region="right")
        self.link(self.term(answer, "1", occurrence=0), positives[0])
        self.link(self.term(answer, "5"), VGroup(*positives[1:]))
        return answer

    def construct(self):
        crowd = DotArray(10, radius=0.15, gap=0.22)
        with self.beat("people", transition="reveal"):
            self.title("You test positive. How worried should you be?")
            self.place(crowd, region="left")
            self.caption("Picture 100 people.")

        facts = VGroup(
            label("1 in 100 is sick", Role.BODY), label("The test: 95% accurate", Role.BODY)
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.4)
        with self.beat("rates", keep=[crowd]):
            self.place(facts, region="right")
            self.caption("It flags the sick, and 5% of healthy people.")

        with self.beat("predict", keep=[crowd, facts]):
            self.ask("You test positive. How likely are you sick?")

        rings = crowd.ring(positive)
        with self.beat("positives", keep=[crowd, facts]):
            self.caption("Rings mark a positive test: 1 sick, 5 healthy.")
            self.pause()
            self.play(crowd.paint(lambda r, c: (r, c) == SICK, "accent"))
            self.show(rings)

        with self.beat("gather", keep=[crowd, rings], run_time=2, aha=True):
            self.caption("Line up everyone with a ring.")
            self.pause()
            positives = self.line_up(crowd)

        with self.beat("answer", keep=[crowd, rings, positives]):
            self.caption("Only 1 of these 6 is sick: about 17%.")
            self.count(positives)

        with self.beat("recap", keep=[crowd, rings], run_time=1.2):
            self.caption("Not 95%: most positives are false alarms.")
            self.pause()
            positives = self.line_up(crowd)
            self.pause()
            self.count(positives)
