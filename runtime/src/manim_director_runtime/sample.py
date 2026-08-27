from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Mapping

from .errors import DirectorError
from .themes import get_theme
from .util import atomic_write, confined_path, project_root


def _class_name(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9]+", " ", value).title().replace(" ", "")
    if not cleaned or cleaned[0].isdigit():
        cleaned = "Directed" + cleaned
    return cleaned + "Scene" if not cleaned.endswith("Scene") else cleaned


def generalized_fibonacci_source(
    *, class_name: str = "GeneralizedFibonacciScene", p: float = 1, q: float = 1, a0: float = 0, a1: float = 1,
    terms: int = 9, theme: str = "midnight"
) -> str:
    if terms < 4 or terms > 24:
        raise DirectorError("invalid_terms", "terms must be between 4 and 24")
    palette = get_theme(theme)
    return rf'''"""Generated generalized Fibonacci explainer: a[n] = p*a[n-1] + q*a[n-2]."""
from manim import *


class {class_name}(MovingCameraScene):
    P = {p!r}
    Q = {q!r}
    A0 = {a0!r}
    A1 = {a1!r}
    TERMS = {terms}

    def construct(self):
        self.camera.background_color = "{palette['background']}"
        fg, primary, secondary, accent = map(ManimColor, (
            "{palette['foreground']}", "{palette['primary']}",
            "{palette['secondary']}", "{palette['accent']}"
        ))
        title = Text("One recurrence, a family of sequences", font="{palette['font']}", color=fg, font_size=42)
        rule = MathTex(r"a_n=p\,a_{{n-1}}+q\,a_{{n-2}}", color=primary, font_size=58)
        rule.next_to(title, DOWN, buff=.45)
        self.play(Write(title), run_time=.8)
        self.play(Write(rule), run_time=1.0)
        self.wait(.5)
        self.next_section("concrete_sequence")

        values = [self.A0, self.A1]
        for _ in range(self.TERMS - 2):
            values.append(self.P * values[-1] + self.Q * values[-2])
        def shown(value):
            return str(int(value)) if float(value).is_integer() else f"{{value:.3g}}"
        cells = VGroup(*[
            VGroup(
                MathTex("a_{{" + str(i) + "}}", color=secondary, font_size=30),
                Text(shown(v), font="{palette['font']}", color=fg, font_size=30),
            ).arrange(DOWN, buff=.15)
            for i, v in enumerate(values)
        ]).arrange(RIGHT, buff=.44).scale_to_fit_width(12.4).to_edge(DOWN, buff=.7)
        parameters = MathTex(
            rf"p={{self.P:g}},\ q={{self.Q:g}},\ a_0={{self.A0:g}},\ a_1={{self.A1:g}}",
            color=fg, font_size=36,
        ).next_to(rule, DOWN, buff=.45)
        self.play(FadeIn(parameters, shift=UP * .15))
        for cell in cells:
            self.play(FadeIn(cell, shift=UP * .18), run_time=.18)
        self.wait(.7)
        self.next_section("state_space")

        state = MathTex(
            r"\begin{{bmatrix}}a_n\\a_{{n-1}}\end{{bmatrix}}="
            r"\begin{{bmatrix}}p&q\\1&0\end{{bmatrix}}"
            r"\begin{{bmatrix}}a_{{n-1}}\\a_{{n-2}}\end{{bmatrix}}",
            color=fg, font_size=48,
        )
        state.set_color_by_tex("p", primary)
        state.set_color_by_tex("q", secondary)
        self.play(FadeOut(cells), FadeOut(parameters), Transform(rule, state), run_time=1.2)
        self.play(self.camera.frame.animate.set(width=state.width + 2.0), run_time=.7)
        self.wait(.7)
        self.next_section("characteristic_roots")

        roots = MathTex(r"r^2-pr-q=0", color=accent, font_size=58)
        solution = MathTex(r"a_n=C_1r_1^n+C_2r_2^n", color=fg, font_size=52)
        pair = VGroup(roots, solution).arrange(DOWN, buff=.55)
        self.play(Transform(rule, roots), run_time=.8)
        self.play(Write(solution), run_time=.9)
        self.wait(1.2)
        recap = Text(
            "Change p, q, or the seeds—and the same machine makes a new sequence.",
            font="{palette['font']}", color=primary, font_size=30,
        ).to_edge(DOWN, buff=.5)
        self.play(FadeIn(recap, shift=UP * .2))
        self.wait(1.4)
'''


def generate_sample(params: Mapping[str, Any]) -> dict[str, Any]:
    root = project_root(params)
    raw_name = str(params.get("name", "generalized_fibonacci"))
    target = confined_path(root, str(params.get("path", f"scenes/{raw_name}.py")))
    if target.exists() and not bool(params.get("force", False)):
        raise DirectorError("file_exists", f"Refusing to overwrite existing scene: {target}")
    class_name = str(params.get("class_name", _class_name(raw_name)))
    source = generalized_fibonacci_source(
        class_name=class_name,
        p=float(params.get("p", 1)),
        q=float(params.get("q", 1)),
        a0=float(params.get("a0", 0)),
        a1=float(params.get("a1", 1)),
        terms=int(params.get("terms", 9)),
        theme=str(params.get("theme", "midnight")),
    )
    atomic_write(target, source)
    return {"path": str(target), "scene": class_name, "parameters": {k: params.get(k, v) for k, v in {"p": 1, "q": 1, "a0": 0, "a1": 1, "terms": 9}.items()}}
