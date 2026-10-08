"""Generalized Fibonacci: every sequence with x_{n+2} = p x_{n+1} + q x_n, from data to roots.

Each scene renders with plain Manim (`manim -ql scenes.py GeneralizedFibonacci`). The theme and
the symbol colors come from director.yaml; the plotted values come from data/sequences.csv.
"""

import csv
from itertools import pairwise, takewhile
from pathlib import Path

from manim import *

from manim_director_runtime import (
    DirectedMovingCameraScene,
    DirectedScene,
    DirectedThreeDScene,
    Region,
    Role,
)

DATA = Path(__file__).resolve().parent / "data" / "sequences.csv"
RECURRENCE = r"x_{n+2} = p\,x_{n+1} + q\,x_n"
# Each sequence: its name, p, q, the seeds x_0 and x_1, and its color token in charts.
FAMILY = (
    ("Fibonacci", 1, 1, 0, 1, "primary"),
    ("Lucas", 1, 1, 2, 1, "secondary"),
    ("Pell", 2, 1, 0, 1, "accent"),
    ("Oscillator", 1, -1, 0, 1, "success"),
)


def terms(p, q, x0, x1, count):
    values = [x0, x1]
    while len(values) < count:
        values.append(p * values[-1] + q * values[-2])
    return values[:count]


def recorded_sequences():
    """Sequence name -> its values in order of n, as data/sequences.csv records them."""

    found = {}
    with DATA.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            found.setdefault(row["sequence"], {})[int(row["n"])] = float(row["value"])
    return {name: [values[n] for n in sorted(values)] for name, values in found.items()}


def family_table(scene):
    """Name, coefficients and first terms of every sequence, in left-aligned columns."""

    cells = []
    for name, p, q, x0, x1, _ in FAMILY:
        values = ",\\ ".join(str(v) for v in terms(p, q, x0, x1, 8))
        cells += [
            scene.text(name),
            scene.math(rf"p = {p},\ q = {q}"),
            scene.math(values + r",\ \dots"),
        ]
    return VGroup(*cells).arrange_in_grid(cols=3, col_alignments="lll", buff=(0.7, 0.36))


def chart_axes(scene, y_range, y_axis_config):
    return Axes(
        x_range=[0, 7, 1],
        y_range=y_range,
        x_length=9.4,
        y_length=4.4,
        tips=False,
        axis_config={"color": scene.theme.muted, "font_size": 28},
        x_axis_config={"numbers_to_include": range(8)},
        y_axis_config=y_axis_config,
    )


def labeled_line(scene, axes, name, token, points):
    """One sequence as dots joined by a line, named where the line ends."""

    color = scene.theme.color(token)
    line = axes.plot_line_graph(
        [n for n, _ in points],
        [value for _, value in points],
        line_color=color,
        stroke_width=3,
        vertex_dot_radius=0.05,
        vertex_dot_style={"fill_color": color, "stroke_width": 0},
    )
    label = scene.text(name, Role.LABEL, color=color)
    return VGroup(line, label.next_to(axes.c2p(*points[-1]), RIGHT, buff=0.15))


def sequence_chart(scene, ceiling=30):
    """Every recorded sequence against n, up to the first value above `ceiling`."""

    axes = chart_axes(scene, [-5, ceiling, 5], {"numbers_to_include": range(0, ceiling + 1, 10)})
    recorded = recorded_sequences()
    lines = [
        labeled_line(
            scene,
            axes,
            name,
            token,
            list(enumerate(takewhile(lambda v: v <= ceiling, recorded[name.lower()]))),
        )
        for name, *_, token in FAMILY
    ]
    return VGroup(axes, *lines)


def growth_chart(scene):
    """The growing sequences on a log scale, where exponential growth is a straight line."""

    axes = chart_axes(
        scene, [0, 2.5, 1], {"scaling": LogBase(custom_labels=True), "include_numbers": True}
    )
    recorded = recorded_sequences()
    lines = [
        labeled_line(
            scene,
            axes,
            name,
            token,
            [(n, v) for n, v in enumerate(recorded[name.lower()]) if v > 0],
        )
        for name, *_, token in FAMILY
        if name != "Oscillator"  # zero and negative terms have no logarithm
    ]
    return VGroup(axes, *lines)


def recap_card(scene, symbols, meaning, token):
    label = scene.text(meaning, color=scene.theme.muted)
    body = VGroup(scene.math(symbols, font_size=72), label).arrange(DOWN, buff=0.35)
    frame = RoundedRectangle(
        width=3.8, height=2.6, corner_radius=0.22, stroke_color=scene.theme.color(token)
    )
    return VGroup(frame, body.move_to(frame))


def cropped(plane, x_max, y_max, zoom):
    """A copy of `plane` (axes, then the labels of their x and y axes) drawn only up to
    (x_max, y_max), for a camera frame scaled by `zoom`: each axis stops there, its ticks beyond
    shrink into its end, and its label rides along at the same size on screen."""

    view = plane.copy()
    axes, *labels = view
    for axis, limit, label in zip((axes.x_axis, axes.y_axis), (x_max, y_max), labels, strict=True):
        end = axis.n2p(limit)
        for tick, number in zip(axis.ticks, axis.get_tick_range(), strict=True):
            if number > limit:
                tick.scale(0).move_to(end)
        label.scale(zoom).move_to(end + zoom * (label.get_center() - axis.get_end()))
        low, high = axis.x_range[:2]
        axis.pointwise_become_partial(axis.copy(), 0, (limit - low) / (high - low))
    return view


class GeneralizedFibonacci(DirectedScene):
    """The full cut: one rule, a family of sequences, their data, the matrix and its roots."""

    def construct(self):
        recurrence = self.math(RECURRENCE, font_size=56)
        fibonacci = self.math(r"0,\ 1,\ 1,\ 2,\ 3,\ 5,\ 8,\ 13,\ \dots")
        with self.beat("hook", transition="reveal", hold=3):
            self.title("Fibonacci is one point in a family")
            self.place(recurrence, fibonacci, buff=0.7)
            self.caption("Pick p and q, then two seeds: every choice is a new sequence.")

        with self.beat("family", hold=3.5):
            self.place(recurrence, family_table(self), buff=0.6)
            self.caption("The same rule with other coefficients and seeds.")

        with self.beat("data", hold=3.5):
            self.place(sequence_chart(self))
            self.caption("Plotted from data/sequences.csv: three grow, one cycles.")

        with self.beat("state-space", hold=2.5):
            self.title("One step is one matrix")
            self.caption("Stack two neighbors into a state: C moves the whole state forward.")
            self.derive(
                RECURRENCE,
                (
                    r"\begin{pmatrix} x_{n+2} \\ x_{n+1} \end{pmatrix}"
                    r" = \underbrace{\begin{pmatrix} p & q \\ 1 & 0 \end{pmatrix}}_{C}"
                    r"\begin{pmatrix} x_{n+1} \\ x_n \end{pmatrix}",
                    "the same rule, for a state",
                ),
            )

        with self.beat("roots", hold=2.5):
            self.title("Its roots decide the long run")
            self.caption("Usually the root of largest size sets the growth.")
            self.derive(
                (r"\det(C - \lambda I) = \lambda^2 - p\lambda - q = 0", "eigenvalues of C"),
                (r"\lambda_\pm = \frac{p \pm \sqrt{p^2 + 4q}}{2}", "quadratic formula"),
                (r"x_n = A\lambda_+^n + B\lambda_-^n", "when the roots differ"),
            )

        with self.beat("edge-case", transition="contrast", hold=2.5):
            self.title("When the roots collide")
            self.caption("A repeated root brings in a factor linear in n.")
            self.derive(
                (r"p^2 + 4q = 0", "one double root, p/2"),
                (r"x_n = (A + Bn)\left(\frac{p}{2}\right)^n", "the general solution"),
                (r"x_n = n", "p = 2, q = −1, seeds 0 and 1"),
            )

        with self.beat("recap", transition="chapter", hold=3):
            self.title("Three choices make a sequence")
            cards = [
                recap_card(self, r"p,\ q", "the rule", "primary"),
                recap_card(self, r"x_0,\ x_1", "the start", "secondary"),
                recap_card(self, r"\lambda_\pm", "the long run", "accent"),
            ]
            self.place(*cards, direction=RIGHT, buff=0.5)
            self.caption("Coefficients pick the rule, seeds the start, roots the long run.")
        self.wait()


class SequenceData(DirectedScene):
    """The data on its own, then on a log scale, where growth rates become slopes."""

    def construct(self):
        linear = sequence_chart(self)
        with self.beat("linear", transition="reveal", hold=2.5):
            self.title("One CSV, four behaviors")
            self.place(linear)
            self.caption("Fibonacci, Lucas and Pell grow; the oscillator repeats every six steps.")

        with self.beat("log", hold=3):
            self.place(growth_chart(self))
            self.caption("On a log scale growth is a slope: Fibonacci and Lucas share φ.")
        self.wait()


class CharacteristicRoots(DirectedScene):
    """Where the characteristic equation comes from, and what a double root changes."""

    def construct(self):
        guess = self.math(r"x_n = \lambda^n", font_size=56)
        with self.beat("guess", transition="reveal", hold=2):
            self.title("Guess pure growth")
            self.place(guess)
            self.caption("Which sequences multiply by the same number at every step?")

        with self.beat("derive", hold=2):
            self.caption("Put the guess into the rule, divide by λⁿ, and solve.")
            roots = self.derive(
                r"\lambda^{n+2} = p\lambda^{n+1} + q\lambda^n",
                (r"\lambda^2 = p\lambda + q", r"divide by $\lambda^n$"),
                (r"\lambda_\pm = \frac{p \pm \sqrt{p^2 + 4q}}{2}", "quadratic formula"),
                replaces=guess,
            )

        distinct = VGroup(
            self.text("two roots", Role.LABEL), self.math(r"x_n = A\lambda_+^n + B\lambda_-^n")
        ).arrange(DOWN, buff=0.3)
        repeated = VGroup(
            self.text("one double root", Role.LABEL), self.math(r"x_n = (A + Bn)\lambda^n")
        ).arrange(DOWN, buff=0.3)
        with self.beat("cases", hold=3):
            self.place(roots.lines[-1], region=Region.TOP)
            self.place(distinct, repeated, region=Region.BOTTOM, direction=RIGHT, buff=1.5)
            self.caption("Two roots give two modes; a double root needs an extra factor n.")
        self.wait()


class CompanionMatrix(DirectedMovingCameraScene):
    """The Fibonacci states under C: the camera starts close, then pulls back to the trend."""

    def walk(self, axes):
        """Dots at the states (x_{n+1}, x_n) and an arrow for every step between them."""

        states = [axes.c2p(later, earlier) for earlier, later in pairwise(terms(1, 1, 0, 1, 9))]
        dots = [Dot(state, radius=0.06, color=self.theme.primary) for state in states]
        arrows = [
            Arrow(
                start,
                end,
                buff=0.08,
                stroke_width=3,
                max_tip_length_to_length_ratio=0.12,
                color=self.theme.accent,
            )
            for start, end in pairwise(states)
        ]
        return dots, arrows

    def construct(self):
        step = self.math(
            r"\begin{pmatrix} x_{n+2} \\ x_{n+1} \end{pmatrix}"
            r" = \begin{pmatrix} 1 & 1 \\ 1 & 0 \end{pmatrix}"
            r"\begin{pmatrix} x_{n+1} \\ x_n \end{pmatrix}",
            font_size=56,
        )
        with self.beat("step", transition="reveal", hold=2):
            self.title("Fibonacci, one matrix step at a time")
            self.place(step)
            self.caption("With p = q = 1 the companion matrix is all ones but one.")

        axes = Axes(
            x_range=[0, 22, 2],
            y_range=[0, 14, 2],
            x_length=9,
            y_length=4.4,
            tips=False,
            axis_config={"color": self.theme.muted},
        )
        plane = VGroup(
            axes,
            self.math("x_{n+1}").next_to(axes.x_axis, RIGHT),
            self.math("x_n").next_to(axes.y_axis, UP),
        )
        camera = self.camera.frame
        with self.beat("walk", hold=1):
            self.place(plane)
            self.caption("Each arrow is one multiplication by C.")
            dots, arrows = self.walk(axes)
            whole, zoom = plane.copy(), 0.32
            camera.save_state()
            # Close in on the first states. The axes stop inside the close-up, clear of the
            # frame's edges, the title and the caption.
            close_up = axes.c2p(0, 0) + 1.8 * RIGHT + 0.55 * UP
            self.play(
                camera.animate.scale(zoom).move_to(close_up),
                Transform(plane, cropped(plane, 6, 4, zoom)),
                FadeIn(dots[0]),
                run_time=1.5,
            )
            for n, (arrow, dot) in enumerate(zip(arrows, dots[1:], strict=True)):
                self.play(GrowArrow(arrow), FadeIn(dot), run_time=0.7)
                if n == 3:
                    self.play(Restore(camera), Transform(plane, whole), run_time=2)

        golden = (1 + 5**0.5) / 2
        trend = DashedLine(axes.c2p(0, 0), axes.c2p(22, 22 / golden), color=self.theme.muted)
        with self.beat("trend", keep=[plane, *dots, *arrows], hold=3):
            self.caption("The states line up along a line of slope 1/φ, where φ² = φ + 1.")
            self.play(Create(trend), run_time=1.5)
        self.wait()


class StateOrbit3D(DirectedThreeDScene):
    """The oscillator's states, followed through time, wind around the time axis."""

    def construct(self):
        axes = ThreeDAxes(
            x_range=[0, 12, 1],
            y_range=[-1.5, 1.5, 1],
            z_range=[-1.5, 1.5, 1],
            x_length=9,
            y_length=3.4,
            z_length=3.4,
            tips=False,
            axis_config={"color": self.theme.muted},
        )
        values = terms(1, -1, 0, 1, 14)
        dots = VGroup(
            *(
                Dot3D(axes.c2p(n, values[n], values[n + 1]), radius=0.07, color=self.theme.primary)
                for n in range(13)
            )
        )
        amplitude = 2 / np.sqrt(3)  # x_n = amplitude * sin(n pi / 3) solves the recurrence
        orbit = axes.plot_parametric_curve(
            lambda t: [t, amplitude * np.sin(t * PI / 3), amplitude * np.sin((t + 1) * PI / 3)],
            t_range=[0, 12],
            color=self.theme.accent,
        )
        self.set_camera_orientation(phi=72 * DEGREES, theta=-72 * DEGREES, zoom=1.05)
        with self.beat("orbit", transition="reveal"):
            self.title("The oscillator turns in circles")
            self.caption("Each pair of neighbors winds once around the time axis every six steps.")
            self.play(Create(axes), run_time=1.2)
            self.play(Create(orbit), LaggedStart(*(FadeIn(d) for d in dots)), run_time=3)
        self.begin_ambient_camera_rotation(rate=0.12)
        self.wait(5)
        self.stop_ambient_camera_rotation()
