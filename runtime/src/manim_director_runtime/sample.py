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
    *,
    class_name: str = "GeneralizedFibonacciScene",
    p: float = 1,
    q: float = 1,
    a0: float = 0,
    a1: float = 1,
    terms: int = 9,
    theme: str = "midnight",
) -> str:
    if terms < 4 or terms > 24:
        raise DirectorError("invalid_terms", "terms must be between 4 and 24")
    palette = get_theme(theme)
    return rf'''"""Generated generalized Fibonacci explainer: a[n] = p*a[n-1] + q*a[n-2]."""
from manim import *
from manim_director_runtime import Beat, DesignSystem, DirectedScene

THEME = {palette!r}
DESIGN = DesignSystem.from_mapping({{"theme": THEME}})


class {class_name}(DirectedScene):
    design = DESIGN
    P = {p!r}
    Q = {q!r}
    A0 = {a0!r}
    A1 = {a1!r}
    TERMS = {terms}

    def construct(self):
        title = self.styled_text(
            "One recurrence, a family of sequences",
            role="title",
            color_role="foreground",
        )
        rule = self.styled_math(
            r"a_n=p\,a_{{n-1}}+q\,a_{{n-2}}",
            role="hero",
            color_role="primary",
        )
        self.place(title, "header", key="title")
        self.play(Write(title))
        self.next_section("recurrence")
        self.beat(
            Beat(
                intent="introduce",
                audience_question="What stays fixed when a whole family of sequences changes?",
                takeaway="One two-step rule generates every term from the previous two.",
                focus="rule",
                visual_metaphor="a two-input sequence machine",
                transition="reveal",
                max_active=2,
            ),
            rule,
            region="content",
            keys=("rule",),
        )
        self.caption("Two remembered values are enough to make the next one.")

        values = [self.A0, self.A1]
        for _ in range(self.TERMS - 2):
            values.append(self.P * values[-1] + self.Q * values[-2])

        def shown(value):
            return str(int(value)) if float(value).is_integer() else f"{{value:.3g}}"

        cells = []
        for index, value in enumerate(values):
            label = self.styled_math(
                "a_{{" + str(index) + "}}", role="label", color_role="secondary"
            )
            number = self.styled_text(shown(value), role="label", color_role="foreground")
            cells.append(self.panel(VGroup(label, number).arrange(DOWN, buff=.10)))
        parameters = self.styled_math(
            rf"p={{self.P:g}},\ q={{self.Q:g}},\ a_0={{self.A0:g}},\ a_1={{self.A1:g}}",
            role="math",
            color_role="foreground",
        )
        sequence = VGroup(*cells).arrange_in_grid(rows=2, buff=(.16, .12))
        self.next_section("concrete_sequence")
        self.beat(
            Beat(
                intent="explain",
                audience_question="What sequence do these particular settings produce?",
                takeaway="The same local rule propagates the chosen seeds across the row.",
                focus="sequence",
                visual_metaphor="a two-input sequence machine",
                transition="continuation",
                max_active=2,
            ),
            sequence,
            parameters,
            region="content",
            flow="column",
            keys=("sequence", "parameters"),
        )
        self.caption("Change the seeds or coefficients; the machine itself does not change.")
        self.wait(.5)

        self.clear_stage()
        self.next_section("state_space")
        state = self.styled_math(
            r"\begin{{bmatrix}}a_n\\a_{{n-1}}\end{{bmatrix}}="
            r"\begin{{bmatrix}}p&q\\1&0\end{{bmatrix}}"
            r"\begin{{bmatrix}}a_{{n-1}}\\a_{{n-2}}\end{{bmatrix}}",
            role="hero",
            color_role="foreground",
        )
        state.set_color_by_tex("p", THEME["primary"])
        state.set_color_by_tex("q", THEME["secondary"])
        self.beat(
            Beat(
                intent="reveal",
                audience_question="Why is remembering two values the natural state?",
                takeaway="The recurrence is repeated multiplication by one two-by-two matrix.",
                focus="state",
                visual_metaphor="a two-input sequence machine",
                transition="chapter",
                max_active=2,
            ),
            state,
            region="content",
            keys=("state",),
        )
        self.focus(state)
        self.caption("The sequence has become an orbit of a linear transformation.")
        self.wait(.5)
        self.release_focus()

        roots = self.styled_math(
            r"r^2-pr-q=0", role="hero", color_role="accent"
        )
        solution = self.styled_math(
            r"a_n=C_1r_1^n+C_2r_2^n", role="math", color_role="foreground"
        )
        self.next_section("characteristic_roots")
        self.beat(
            Beat(
                intent="recap",
                audience_question="What controls the long-term shape of the sequence?",
                takeaway="The matrix's two characteristic roots set the growth modes.",
                focus="roots",
                visual_metaphor="a two-input sequence machine",
                transition="continuation",
                max_active=2,
            ),
            roots,
            solution,
            region="content",
            flow="column",
            keys=("roots", "solution"),
        )
        self.caption("Different parameters tune the machine by moving those roots.")
        self.wait(1)
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
    compile(source, str(target), "exec")
    atomic_write(target, source)
    return {
        "path": str(target),
        "scene": class_name,
        "parameters": {
            key: params.get(key, default)
            for key, default in {"p": 1, "q": 1, "a0": 0, "a1": 1, "terms": 9}.items()
        },
    }
