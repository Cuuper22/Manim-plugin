"""Euler's formula, drawn: e^{iθ} runs around the unit circle while its real and imaginary parts
trace cos θ and sin θ; at θ = π it lands on −1, and e^{iθ} = cos θ + i sin θ becomes
e^{iπ} + 1 = 0."""

import numpy as np
from manim import *

from manim_director_runtime import DirectedScene

R = 1.45  # the circle's radius and the waves' unit height, so heights match across the picture


class EulerHero(DirectedScene):
    symbols = {r"\cos": "primary", r"\sin": "secondary", r"\theta": "accent", r"\pi": "accent"}

    def construct(self):
        theta = ValueTracker(0.0)

        plane = VGroup(
            Line(LEFT * (R + 0.35), RIGHT * (R + 0.35), stroke_width=2, color=self.theme.muted),
            Line(DOWN * (R + 0.35), UP * (R + 0.35), stroke_width=2, color=self.theme.muted),
            Circle(radius=R, stroke_width=3, color=self.theme.foreground).set_stroke(opacity=0.5),
        )
        waves = Axes(
            x_range=(0, 2 * PI, PI / 2),
            y_range=(-1, 1, 1),
            x_length=6.2,
            y_length=2 * R,
            axis_config={"stroke_width": 2, "color": self.theme.muted, "include_ticks": False},
            tips=False,
        )
        stage = VGroup(plane, waves).arrange(RIGHT, buff=1.2)
        circle = plane[2]

        def point() -> np.ndarray:  # read from the circle, so it follows wherever place() puts it
            t = theta.get_value()
            return circle.get_center() + circle.width / 2 * np.array([np.cos(t), np.sin(t), 0])

        def traced(fn, color: str) -> VMobject:
            t = max(theta.get_value(), 1e-3)
            return waves.plot(fn, x_range=(0, t), color=self.theme.color(color), stroke_width=5)

        moving = VGroup(
            always_redraw(lambda: traced(np.cos, "primary")),
            always_redraw(lambda: traced(np.sin, "secondary")),
            always_redraw(  # the real part, as a horizontal from the axis to the point
                lambda: Line(
                    circle.get_center(),
                    [point()[0], circle.get_y(), 0],
                    color=self.theme.primary,
                    stroke_width=5,
                )
            ),
            always_redraw(  # the imaginary part, vertical
                lambda: Line(
                    [point()[0], circle.get_y(), 0],
                    point(),
                    color=self.theme.secondary,
                    stroke_width=5,
                )
            ),
            always_redraw(
                lambda: Line(circle.get_center(), point(), color=self.theme.accent, stroke_width=4)
            ),
            always_redraw(  # the height of the point carried across to the sine wave
                lambda: DashedLine(
                    point(),
                    waves.c2p(theta.get_value(), np.sin(theta.get_value())),
                    color=self.theme.secondary,
                    stroke_width=2,
                    dash_length=0.08,
                ).set_stroke(opacity=0.6)
            ),
            always_redraw(lambda: Dot(point(), radius=0.09, color=self.theme.accent)),
        )

        euler = self.math(r"e^{i\theta} = \cos\theta + i \sin\theta", font_size=64)
        with self.beat("draw", transition="reveal", hold=0.2):
            self.place(euler, region="header")
            self.place(stage, region="content")
            self.pause()  # the formula and the empty stage enter and are read before anything moves
            self.add(moving)
            self.play(theta.animate.set_value(PI), run_time=3.2, rate_func=smooth)

        with self.beat("identity", keep=[stage, moving], aha=True, hold=1.4):
            identity = self.derive(
                self.math(r"e^{i\pi} = \cos\pi + i \sin\pi", font_size=64),
                self.math(r"e^{i\pi} = -1", font_size=64),
                self.math(r"e^{i\pi} + 1 = 0", font_size=72),
                region="header",
                in_place=True,
                replaces=euler,
            )
            self.highlight(identity.lines[-1], color=None, box=True)

        with self.beat("around", keep=[stage, moving, identity], hold=0.2):
            self.play(theta.animate.set_value(2 * PI), run_time=2.6, rate_func=smooth)

        with self.beat("loop", transition="chapter", hold=0.2):
            pass
