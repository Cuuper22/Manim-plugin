"""A complete Manim CE example for x[n+2] = p*x[n+1] + q*x[n].

The scenes are intentionally standalone: `manim` can render this file without the
director runtime.  The director metadata beside it adds narration, profiles,
themes, expected outputs, and semantic beat IDs.
"""

from __future__ import annotations

import csv
import json
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Sequence

from manim import (
    DEGREES,
    DOWN,
    FadeIn,
    FadeOut,
    GrowArrow,
    LaggedStart,
    LEFT,
    Line,
    MathTex,
    MovingCameraScene,
    RIGHT,
    Restore,
    Scene,
    SVGMobject,
    Text,
    ThreeDAxes,
    ThreeDScene,
    TransformMatchingTex,
    UP,
    VGroup,
    VMobject,
    Write,
    Arrow,
    Axes,
    Create,
    Dot,
    Dot3D,
    RoundedRectangle,
    config,
)


ROOT = Path(__file__).resolve().parent


@dataclass(frozen=True)
class Palette:
    background: str
    foreground: str
    muted: str
    primary: str
    secondary: str
    accent: str
    negative: str


def load_palette() -> Palette:
    theme_name = os.getenv("MANIM_DIRECTOR_THEME", "midnight")
    themes = json.loads((ROOT / "themes.json").read_text(encoding="utf-8"))
    selected = themes.get(theme_name, themes["midnight"])
    return Palette(**selected["colors"])


PALETTE = load_palette()


def generalized_terms(
    p: float, q: float, x0: float, x1: float, count: int
) -> list[float]:
    """Return exactly ``count`` terms of the generalized recurrence."""
    if count < 0:
        raise ValueError("count must be non-negative")
    if count == 0:
        return []
    if count == 1:
        return [x0]
    values = [x0, x1]
    for _ in range(2, count):
        values.append(p * values[-1] + q * values[-2])
    return values


def compact_number(value: float) -> str:
    rounded = round(value)
    return str(rounded) if abs(value - rounded) < 1e-9 else f"{value:.2f}"


def fit(mobject, *, width: float | None = None, height: float | None = None):
    if width is not None and mobject.width > width:
        mobject.scale_to_fit_width(width)
    if height is not None and mobject.height > height:
        mobject.scale_to_fit_height(height)
    return mobject


def sequence_row(
    name: str,
    parameters: str,
    values: Sequence[float],
    color: str,
    width: float,
) -> VGroup:
    name_text = Text(name, font_size=28, color=color, weight="BOLD")
    parameter_text = Text(parameters, font_size=19, color=PALETTE.muted)
    terms_text = Text(
        "  ·  ".join(compact_number(value) for value in values),
        font_size=24,
        color=PALETTE.foreground,
    )
    labels = VGroup(name_text, parameter_text).arrange(DOWN, aligned_edge=LEFT, buff=0.05)
    row = VGroup(labels, terms_text).arrange(RIGHT, buff=0.45)
    return fit(row, width=width)


def load_sequence_data() -> dict[str, list[float]]:
    rows: dict[str, list[tuple[int, float]]] = {}
    with (ROOT / "data" / "sequences.csv").open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            rows.setdefault(row["sequence"], []).append((int(row["n"]), float(row["value"])))
    return {
        name: [value for _, value in sorted(points)]
        for name, points in rows.items()
    }


class DirectorScene:
    """Small presentation helpers shared by the independently renderable scenes."""

    def apply_theme(self) -> None:
        self.camera.background_color = PALETTE.background

    @property
    def portrait(self) -> bool:
        return config.frame_height > config.frame_width

    @property
    def safe_width(self) -> float:
        return config.frame_width * 0.88

    def heading(self, text: str, subtitle: str | None = None) -> VGroup:
        title = Text(text, color=PALETTE.foreground, font_size=42, weight="BOLD")
        group = VGroup(title)
        if subtitle:
            sub = Text(subtitle, color=PALETTE.muted, font_size=23)
            group.add(sub).arrange(DOWN, buff=0.12)
        return fit(group, width=self.safe_width)

    def wipe(self, run_time: float = 0.45) -> None:
        visible = list(self.mobjects)
        if visible:
            self.play(*(FadeOut(mob) for mob in visible), run_time=run_time)

class GeneralizedFibonacci(DirectorScene, MovingCameraScene):
    """The narrative cut: intuition, data, state space, roots, and edge cases."""

    def construct(self) -> None:
        self.apply_theme()

        self.next_section("hook")
        knot = SVGMobject(str(ROOT / "assets" / "recurrence-knot.svg"))
        knot.set_color(PALETTE.accent).set_height(1.15)
        title = self.heading(
            "Fibonacci is one point in a universe",
            "Two coefficients turn one famous sequence into an entire family.",
        )
        recurrence = MathTex(
            r"x_{n+2}", "=", "p", r"x_{n+1}", "+", "q", "x_n",
            color=PALETTE.foreground,
        )
        recurrence.set_color_by_tex("p", PALETTE.primary)
        recurrence.set_color_by_tex("q", PALETTE.secondary)
        intro = VGroup(knot, title, recurrence).arrange(DOWN, buff=0.38)
        fit(intro, width=self.safe_width, height=config.frame_height * 0.8)
        self.play(FadeIn(knot, scale=0.7), Write(title[0]), run_time=1.2)
        self.play(FadeIn(title[1], shift=UP * 0.15), Write(recurrence), run_time=1.3)
        self.wait(1.6)

        self.next_section("family")
        self.wipe()
        family_title = self.heading("Same machine. Very different behavior.")
        family_title.to_edge(UP, buff=0.5)
        examples = [
            ("Fibonacci", "p=1, q=1; seeds 0, 1", (1, 1, 0, 1), PALETTE.primary),
            ("Lucas", "p=1, q=1; seeds 2, 1", (1, 1, 2, 1), PALETTE.secondary),
            ("Pell", "p=2, q=1; seeds 0, 1", (2, 1, 0, 1), PALETTE.accent),
            ("Oscillator", "p=1, q=−1; seeds 0, 1", (1, -1, 0, 1), PALETTE.negative),
        ]
        rows = VGroup(
            *[
                sequence_row(
                    name,
                    params,
                    generalized_terms(*coeffs, count=8),
                    color,
                    self.safe_width,
                )
                for name, params, coeffs, color in examples
            ]
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.34)
        rows.next_to(family_title, DOWN, buff=0.5)
        fit(rows, height=config.frame_height * 0.64)
        self.play(Write(family_title), run_time=0.7)
        self.play(
            LaggedStart(*(FadeIn(row, shift=RIGHT * 0.25) for row in rows), lag_ratio=0.18),
            run_time=2.3,
        )
        self.wait(2.05)

        self.next_section("data")
        self.wipe()
        data = load_sequence_data()
        chart_title = self.heading(
            "Data exposes the behavior",
            "Equal recurrence order does not mean equal growth.",
        ).to_edge(UP, buff=0.35)
        axes = Axes(
            x_range=[0, 7, 1],
            y_range=[-2, 31, 5],
            x_length=min(9.5, self.safe_width),
            y_length=min(4.6, config.frame_height * 0.53),
            axis_config={"color": PALETTE.muted, "include_numbers": True, "font_size": 22},
            tips=False,
        )
        axes.shift(DOWN * 0.35)
        plotted = VGroup()
        legend = VGroup()
        direct_labels = VGroup()
        colors = {
            "fibonacci": PALETTE.primary,
            "lucas": PALETTE.secondary,
            "oscillator": PALETTE.negative,
        }
        for name in ("fibonacci", "lucas", "oscillator"):
            values = data[name]
            graph = axes.plot_line_graph(
                x_values=list(range(len(values))),
                y_values=values,
                line_color=colors[name],
                vertex_dot_style={"fill_color": colors[name], "stroke_width": 0},
                vertex_dot_radius=0.055,
            )
            plotted.add(graph)
            endpoint_label = Text(name.title(), font_size=16, color=PALETTE.foreground)
            endpoint_label.next_to(axes.c2p(len(values) - 1, values[-1]), RIGHT, buff=0.08)
            direct_labels.add(endpoint_label)
            key = VGroup(
                Line(LEFT * 0.2, RIGHT * 0.2, color=colors[name], stroke_width=5),
                Text(name.title(), font_size=19, color=PALETTE.foreground),
            ).arrange(RIGHT, buff=0.1)
            legend.add(key)
        legend.arrange(RIGHT if not self.portrait else DOWN, buff=0.35)
        legend.next_to(axes, DOWN, buff=0.18)
        fit(
            VGroup(axes, plotted, legend, direct_labels),
            width=self.safe_width,
            height=config.frame_height * 0.68,
        )
        self.play(Write(chart_title), Create(axes), run_time=1.1)
        self.play(LaggedStart(*(Create(graph) for graph in plotted), lag_ratio=0.2), run_time=2.0)
        self.play(FadeIn(legend), FadeIn(direct_labels), run_time=0.6)
        self.camera.frame.save_state()
        focus = axes.c2p(6, data["fibonacci"][6])
        self.play(self.camera.frame.animate.scale(0.58).move_to(focus), run_time=0.9)
        self.wait(0.45)
        self.play(Restore(self.camera.frame), run_time=0.7)

        self.next_section("state-space")
        self.wipe()
        state_title = self.heading("A recurrence is a matrix step").to_edge(UP, buff=0.55)
        recurrence_state = MathTex(
            r"\begin{pmatrix}x_{n+2}\\x_{n+1}\end{pmatrix}",
            "=",
            r"\underbrace{\begin{pmatrix}p&q\\1&0\end{pmatrix}}_{C}",
            r"\begin{pmatrix}x_{n+1}\\x_n\end{pmatrix}",
            color=PALETTE.foreground,
        )
        recurrence_state[2].set_color(PALETTE.accent)
        recurrence_state = fit(recurrence_state, width=self.safe_width * 0.92)
        state_caption = Text(
            "The companion matrix C moves the entire two-number state forward.",
            font_size=24,
            color=PALETTE.muted,
        )
        state_caption = fit(state_caption, width=self.safe_width)
        state_group = VGroup(recurrence_state, state_caption).arrange(DOWN, buff=0.5)
        self.play(Write(state_title), run_time=0.7)
        self.play(Write(recurrence_state), run_time=1.8)
        self.play(FadeIn(state_caption, shift=UP * 0.15), run_time=0.7)
        self.wait(1.05)

        self.next_section("roots")
        self.wipe()
        root_title = self.heading("Growth lives in two characteristic roots").to_edge(UP, buff=0.5)
        equation = MathTex(r"\lambda^2-p\lambda-q", "=", "0", color=PALETTE.foreground)
        solution = MathTex(
            r"\lambda_{\pm}", "=", r"\frac{p\pm\sqrt{p^2+4q}}{2}",
            color=PALETTE.foreground,
        )
        closed = MathTex(
            r"x_n", "=", r"A\lambda_+^n+B\lambda_-^n",
            color=PALETTE.foreground,
        )
        closed[2].set_color(PALETTE.primary)
        root_stack = VGroup(equation, solution, closed).arrange(DOWN, buff=0.48)
        fit(root_stack, width=self.safe_width * 0.92, height=config.frame_height * 0.58)
        self.play(Write(root_title), Write(equation), run_time=1.2)
        self.play(TransformMatchingTex(equation.copy(), solution), run_time=1.4)
        self.play(FadeIn(closed, shift=UP * 0.25), run_time=0.8)
        dominance = Text(
            "Usually, the root with the largest magnitude controls long-run growth.",
            font_size=23,
            color=PALETTE.muted,
        )
        fit(dominance, width=self.safe_width)
        dominance.next_to(root_stack, DOWN, buff=0.4)
        self.play(FadeIn(dominance), run_time=0.6)
        self.wait(1.25)

        self.next_section("edge-case")
        self.wipe()
        edge_title = self.heading("The repeated-root case changes the shape").to_edge(UP, buff=0.5)
        discriminant = MathTex(r"p^2+4q=0", color=PALETTE.secondary)
        repeated = MathTex(
            r"x_n=(A+Bn)\left(\frac p2\right)^n",
            color=PALETTE.foreground,
        )
        example = MathTex(
            r"p=2,\ q=-1,\ (x_0,x_1)=(0,1)",
            r"\quad\Longrightarrow\quad",
            r"x_n=n",
            color=PALETTE.foreground,
        )
        example[2].set_color(PALETTE.accent)
        edge_group = VGroup(discriminant, repeated, example).arrange(DOWN, buff=0.5)
        fit(edge_group, width=self.safe_width * 0.92)
        self.play(Write(edge_title), Write(discriminant), run_time=1.0)
        self.play(Write(repeated), run_time=0.9)
        self.play(FadeIn(example, shift=UP * 0.2), run_time=0.8)
        self.wait(1.35)

        self.next_section("recap")
        self.wipe()
        recap_title = self.heading("The whole universe in four choices")
        cards = VGroup(
            self._recap_card("p, q", "the recurrence", PALETTE.primary),
            self._recap_card("x₀, x₁", "the starting state", PALETTE.secondary),
            self._recap_card("λ±", "growth or oscillation", PALETTE.accent),
        )
        cards.arrange(DOWN if self.portrait else RIGHT, buff=0.35)
        fit(cards, width=self.safe_width, height=config.frame_height * 0.55)
        group = VGroup(recap_title, cards).arrange(DOWN, buff=0.65)
        self.play(Write(recap_title), run_time=0.8)
        self.play(LaggedStart(*(FadeIn(card, shift=UP * 0.2) for card in cards), lag_ratio=0.2), run_time=1.5)
        self.wait(1.45)

    def _recap_card(self, symbol: str, label: str, color: str) -> VGroup:
        box = RoundedRectangle(
            corner_radius=0.18,
            width=3.55,
            height=1.55,
            stroke_color=color,
            stroke_width=3,
            fill_color=PALETTE.background,
            fill_opacity=0.7,
        )
        symbol_text = Text(symbol, font_size=34, color=color, weight="BOLD")
        label_text = Text(label, font_size=19, color=PALETTE.foreground)
        contents = VGroup(symbol_text, label_text).arrange(DOWN, buff=0.16)
        contents.move_to(box)
        return VGroup(box, contents)


class SequenceData(DirectorScene, Scene):
    """A data-only chapter demonstrating deterministic CSV-backed plotting."""

    def construct(self) -> None:
        self.apply_theme()
        self.next_section("csv-to-chart")
        data = load_sequence_data()
        title = self.heading("One CSV, four recurrence behaviors").to_edge(UP)
        axes = Axes(
            x_range=[0, 7, 1],
            y_range=[-2, 32, 5],
            x_length=min(self.safe_width, 10),
            y_length=5,
            tips=False,
            axis_config={"color": PALETTE.muted, "include_numbers": True, "font_size": 22},
        ).shift(DOWN * 0.35)
        palette = [PALETTE.primary, PALETTE.secondary, PALETTE.accent, PALETTE.negative]
        graphs = VGroup()
        direct_labels = VGroup()
        for (name, values), color in zip(data.items(), palette):
            display_values = values if max(values) <= 31 else values[:6]
            graph = axes.plot_line_graph(
                x_values=list(range(len(display_values))),
                y_values=display_values,
                line_color=color,
                vertex_dot_style={"fill_color": color, "stroke_width": 0},
                vertex_dot_radius=0.06,
            )
            graphs.add(graph)
            label = Text(name.title(), font_size=16, color=PALETTE.foreground)
            label.next_to(
                axes.c2p(len(display_values) - 1, display_values[-1]),
                RIGHT,
                buff=0.08,
            )
            direct_labels.add(label)
        fit(VGroup(axes, graphs, direct_labels), width=self.safe_width, height=config.frame_height * 0.75)
        self.play(Write(title), Create(axes), run_time=1.0)
        self.play(LaggedStart(*(Create(graph) for graph in graphs), lag_ratio=0.22), run_time=2.4)
        self.play(FadeIn(direct_labels), run_time=0.5)
        self.wait(0.8)


class CompanionMatrix(DirectorScene, MovingCameraScene):
    """A geometric state-space walk for the companion matrix."""

    def construct(self) -> None:
        self.apply_theme()
        self.next_section("state-walk")
        title = self.heading("Each term is a point moving through state space").to_edge(UP)
        axes = Axes(
            x_range=[-1, 14, 2],
            y_range=[-1, 22, 4],
            x_length=min(8.2, self.safe_width * 0.68),
            y_length=5.2,
            tips=False,
            axis_config={"color": PALETTE.muted, "include_numbers": True, "font_size": 19},
        ).shift(DOWN * 0.35)
        values = generalized_terms(1, 1, 0, 1, 8)
        states = [(values[n], values[n + 1]) for n in range(len(values) - 1)]
        dots = VGroup(*(Dot(axes.c2p(x, y), radius=0.07, color=PALETTE.primary) for x, y in states))
        arrows = VGroup(
            *(
                Arrow(dots[n].get_center(), dots[n + 1].get_center(), buff=0.09, color=PALETTE.accent)
                for n in range(len(dots) - 1)
            )
        )
        labels = VGroup(
            *(
                MathTex(rf"({int(x)},{int(y)})", font_size=24, color=PALETTE.foreground)
                .next_to(dot, UP, buff=0.08)
                for dot, (x, y) in zip(dots, states)
            )
        )
        fit(VGroup(axes, dots, arrows, labels), width=self.safe_width, height=config.frame_height * 0.74)
        self.play(Write(title), Create(axes), run_time=1.0)
        self.play(FadeIn(dots[0]), FadeIn(labels[0]), run_time=0.4)
        for index, arrow in enumerate(arrows):
            self.play(GrowArrow(arrow), FadeIn(dots[index + 1]), FadeIn(labels[index + 1]), run_time=0.38)
        self.camera.frame.save_state()
        self.play(self.camera.frame.animate.scale(0.52).move_to(dots[-1]), run_time=0.9)
        self.wait(0.4)
        self.play(Restore(self.camera.frame), run_time=0.8)


class CharacteristicRoots(DirectorScene, MovingCameraScene):
    """The distinct-root derivation with an explicit repeated-root branch."""

    def construct(self) -> None:
        self.apply_theme()
        self.next_section("derive")
        title = self.heading("Solve the recurrence by asking for pure growth").to_edge(UP)
        ansatz = MathTex(r"x_n=\lambda^n", color=PALETTE.primary)
        substitution = MathTex(
            r"\lambda^{n+2}=p\lambda^{n+1}+q\lambda^n",
            color=PALETTE.foreground,
        )
        polynomial = MathTex(r"\lambda^2-p\lambda-q=0", color=PALETTE.secondary)
        roots = MathTex(
            r"\lambda_{\pm}=\frac{p\pm\sqrt{p^2+4q}}2",
            color=PALETTE.accent,
        )
        stack = VGroup(ansatz, substitution, polynomial, roots).arrange(DOWN, buff=0.36)
        fit(stack, width=self.safe_width, height=config.frame_height * 0.67)
        self.play(Write(title), FadeIn(ansatz), run_time=1.0)
        self.play(TransformMatchingTex(ansatz.copy(), substitution), run_time=1.0)
        self.play(TransformMatchingTex(substitution.copy(), polynomial), run_time=1.0)
        self.play(TransformMatchingTex(polynomial.copy(), roots), run_time=1.0)
        self.next_section("branch")
        branch = VGroup(
            Text("distinct roots", font_size=22, color=PALETTE.primary),
            MathTex(r"A\lambda_+^n+B\lambda_-^n", color=PALETTE.foreground),
            Text("repeated root", font_size=22, color=PALETTE.secondary),
            MathTex(r"(A+Bn)\lambda^n", color=PALETTE.foreground),
        ).arrange(DOWN, buff=0.18)
        fit(branch, width=self.safe_width * 0.7)
        branch.to_edge(DOWN, buff=0.25)
        self.play(FadeIn(branch, shift=UP * 0.2), run_time=1.0)
        self.wait(0.8)


class StateOrbit3D(DirectorScene, ThreeDScene):
    """A light 3D camera scene: an oscillatory recurrence becomes a state helix."""

    def construct(self) -> None:
        self.apply_theme()
        self.next_section("orbit")
        title = self.heading("Oscillation is a closed state orbit through time")
        title.to_edge(UP)
        self.add_fixed_in_frame_mobjects(title)
        axes = ThreeDAxes(
            x_range=[0, 8, 1],
            y_range=[-1.5, 1.5, 1],
            z_range=[-1.5, 1.5, 1],
            x_length=7.2,
            y_length=3.5,
            z_length=3.5,
            axis_config={"color": PALETTE.muted},
        )
        values = generalized_terms(1, -1, 0, 1, 10)
        points = [axes.c2p(n, values[n], values[n + 1]) for n in range(9)]
        path = VMobject(color=PALETTE.accent, stroke_width=5).set_points_as_corners(points)
        dots = VGroup(*(Dot3D(point=point, radius=0.065, color=PALETTE.primary) for point in points))
        self.set_camera_orientation(phi=68 * DEGREES, theta=-48 * DEGREES, zoom=0.9)
        self.play(Write(title), Create(axes), run_time=1.0)
        self.play(Create(path), LaggedStart(*(FadeIn(dot) for dot in dots), lag_ratio=0.08), run_time=2.2)
        self.begin_ambient_camera_rotation(rate=0.12)
        self.wait(2.0)
        self.stop_ambient_camera_rotation()
