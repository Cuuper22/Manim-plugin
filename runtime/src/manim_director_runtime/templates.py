from __future__ import annotations

import textwrap
from pathlib import Path
from typing import Any, Callable, Mapping

from .errors import DirectorError
from .sample import generalized_fibonacci_source
from .themes import get_theme
from .util import atomic_write, confined_path, project_root


def _preamble(theme: dict[str, Any], description: str) -> str:
    """Return the shared, public-API-only preamble for generated scenes."""

    return textwrap.dedent(
        f'''\
        """{description}"""
        from manim import *
        from manim_director_runtime import Beat, DesignSystem, DirectedScene

        THEME = {theme!r}
        DESIGN = DesignSystem.from_mapping({{"theme": THEME}})
        '''
    )


def _equation(theme: dict[str, Any]) -> str:
    return _preamble(theme, "A directed visual derivation of the sum of odd numbers.") + textwrap.dedent(
        r'''


        class EquationDerivationScene(DirectedScene):
            design = DESIGN

            def construct(self):
                title = self.styled_text(
                    "Odd numbers grow perfect squares", role="title", color_role="foreground"
                )
                self.place(title, "header", key="title")
                self.play(Write(title))

                rows = []
                equations = (r"1=1^2", r"1+3=2^2", r"1+3+5=3^2", r"1+3+5+7=4^2")
                for n, equation in enumerate(equations, start=1):
                    tiles = VGroup(*[
                        Square(
                            side_length=.30,
                            stroke_width=0,
                            fill_color=THEME["primary"],
                            fill_opacity=1,
                        )
                        for _ in range(2 * n - 1)
                    ]).arrange(RIGHT, buff=.06)
                    label = self.styled_math(equation, role="label", color_role="secondary")
                    rows.append(VGroup(tiles, label).arrange(RIGHT, buff=.55))
                pattern = VGroup(*rows).arrange(DOWN, buff=.28)

                self.beat(
                    Beat(
                        intent="introduce",
                        audience_question="What shape is hidden in the running sums?",
                        takeaway="Each new odd row completes the next square.",
                        focus="pattern",
                        visual_metaphor="odd rows completing one square",
                        transition="reveal",
                        max_active=2,
                    ),
                    pattern,
                    region="content",
                    keys=("pattern",),
                )
                self.caption("Every step adds the next L-shaped odd layer.")

                claim = self.styled_math(
                    r"\sum_{k=1}^n(2k-1)=n^2", role="hero", color_role="primary"
                )
                self.beat(
                    Beat(
                        intent="explain",
                        audience_question="Can the picture be stated for every n?",
                        takeaway="The geometric pattern is exactly a finite-sum identity.",
                        focus="claim",
                        visual_metaphor="odd rows completing one square",
                        transition="continuation",
                        max_active=2,
                    ),
                    claim,
                    region="content",
                    keys=("claim",),
                )
                self.focus(claim)
                self.wait(.35)
                self.release_focus()

                first = self.styled_math(
                    r"\sum_{k=1}^n(2k-1)=2\sum_{k=1}^n k-\sum_{k=1}^n1",
                    role="math",
                    color_role="foreground",
                )
                second = self.styled_math(
                    r"=n(n+1)-n=n^2", role="math", color_role="secondary"
                )
                self.beat(
                    Beat(
                        intent="prove",
                        audience_question="Why must the identity hold?",
                        takeaway="The arithmetic-series formula collapses the sum to n squared.",
                        focus="result",
                        visual_metaphor="odd rows completing one square",
                        transition="continuation",
                        max_active=2,
                    ),
                    first,
                    second,
                    region="content",
                    flow="column",
                    keys=("derivation", "result"),
                )
                self.play(Indicate(second, color=THEME["accent"]))
                self.caption("The picture predicts the algebra; the algebra proves it.")
                self.wait(.7)
        '''
    ).lstrip("\n")


def _function(theme: dict[str, Any]) -> str:
    return _preamble(theme, "A directed exploration of sine and its derivative.") + textwrap.dedent(
        r'''


        class FunctionExplorerScene(DirectedScene):
            design = DESIGN

            def construct(self):
                title = self.styled_text(
                    "The derivative is a moving slope", role="title", color_role="foreground"
                )
                self.place(title, "header", key="title")
                self.play(Write(title))

                axes = Axes(
                    x_range=[-TAU, TAU, PI / 2],
                    y_range=[-1.5, 1.5, .5],
                    x_length=10.8,
                    y_length=4.7,
                    tips=False,
                    axis_config={"color": THEME["muted"]},
                )
                sine = axes.plot(
                    lambda x: np.sin(x), x_range=[-TAU, TAU], color=THEME["primary"]
                )
                plot = VGroup(axes, sine)
                sine_label = self.styled_math(
                    r"f(x)=\sin x", role="label", color_role="primary"
                )
                plot_with_label = VGroup(plot, sine_label).arrange(DOWN, buff=.24)
                self.beat(
                    Beat(
                        intent="introduce",
                        audience_question="How does sine's slope change as we travel along it?",
                        takeaway="A tangent turns continuously while its contact point moves.",
                        focus="plot",
                        visual_metaphor="walking along a curve with a local ruler",
                        transition="reveal",
                        max_active=3,
                    ),
                    plot_with_label,
                    region="content",
                    keys=("plot",),
                )

                tracker = ValueTracker(-TAU)
                dot = always_redraw(lambda: Dot(
                    axes.c2p(tracker.get_value(), np.sin(tracker.get_value())),
                    color=THEME["accent"],
                    radius=.075,
                ))
                tangent = always_redraw(lambda: axes.get_secant_slope_group(
                    tracker.get_value(),
                    sine,
                    dx=.035,
                    secant_line_color=THEME["accent"],
                    secant_line_length=2.8,
                ))
                self.add(dot, tangent)
                self.focus(dot, tangent)
                self.play(tracker.animate.set_value(TAU), run_time=4, rate_func=linear)
                self.release_focus()
                self.caption("The tangent is horizontal exactly at sine's peaks and valleys.")
                self.remove(dot, tangent)

                cosine = axes.plot(
                    lambda x: np.cos(x), x_range=[-TAU, TAU], color=THEME["secondary"]
                )
                cosine_label = self.styled_math(
                    r"f'(x)=\cos x", role="label", color_role="secondary"
                )
                complete_plot = VGroup(axes.copy(), sine.copy(), cosine)
                self.beat(
                    Beat(
                        intent="reveal",
                        audience_question="What single graph records all those tangent slopes?",
                        takeaway="Cosine is the slope record of sine.",
                        focus="cosine",
                        visual_metaphor="walking along a curve with a local ruler",
                        transition="reveal",
                        max_active=3,
                    ),
                    complete_plot,
                    cosine_label,
                    region="content",
                    flow="column",
                    keys=("cosine", "cosine-label"),
                )
                self.focus(complete_plot)
                self.caption("Blue is height; gold is the rate at which that height changes.")
                self.wait(.8)
        '''
    ).lstrip("\n")


def _geometry(theme: dict[str, Any]) -> str:
    return _preamble(theme, "A directed right-triangle similarity proof.") + textwrap.dedent(
        r'''


        class GeometryProofScene(DirectedScene):
            design = DESIGN

            def construct(self):
                title = self.styled_text(
                    "One altitude reveals three copies", role="title", color_role="foreground"
                )
                self.place(title, "header", key="title")
                self.play(Write(title))

                a = np.array([-3.0, -2.0, 0.0])
                b = np.array([3.0, -2.0, 0.0])
                c = np.array([-3.0, 2.0, 0.0])
                foot = np.array([-15 / 13, 10 / 13, 0.0])
                triangle = Polygon(
                    a,
                    b,
                    c,
                    color=THEME["primary"],
                    fill_color=THEME["primary"],
                    fill_opacity=.10,
                    stroke_width=THEME["stroke_width"],
                )
                right_angle = RightAngle(
                    Line(a, b), Line(a, c), length=.34, color=THEME["secondary"]
                )
                labels = VGroup(
                    self.styled_math("a", role="label").move_to((a + c) / 2 + LEFT * .32),
                    self.styled_math("b", role="label").move_to((a + b) / 2 + DOWN * .32),
                    self.styled_math("c", role="label", color_role="accent").move_to((b + c) / 2 + UR * .28),
                )
                diagram = VGroup(triangle, right_angle, labels)
                altitude = DashedLine(a, foot, color=THEME["accent"], dash_length=.12)
                left_copy = Polygon(a, c, foot, color=THEME["secondary"], fill_opacity=.12)
                right_copy = Polygon(a, foot, b, color=THEME["primary"], fill_opacity=.12)
                similar_triangles = VGroup(
                    triangle.copy(),
                    right_angle.copy(),
                    labels.copy(),
                    altitude,
                    left_copy,
                    right_copy,
                )
                self.beat(
                    Beat(
                        intent="introduce",
                        audience_question="What structure is hiding inside a right triangle?",
                        takeaway="The side labels define one geometric object to keep tracking.",
                        focus="triangle",
                        visual_metaphor="one shape containing scaled copies of itself",
                        transition="reveal",
                        max_active=2,
                    ),
                    diagram,
                    region="content",
                    keys=("triangle",),
                )

                claim = self.styled_math(
                    r"a^2+b^2=c^2", role="hero", color_role="secondary"
                )
                self.beat(
                    Beat(
                        intent="reveal",
                        audience_question="Why draw the altitude to the hypotenuse?",
                        takeaway="It splits the original into two triangles with the same angles.",
                        focus="similar-triangles",
                        visual_metaphor="one shape containing scaled copies of itself",
                        transition="reveal",
                        max_active=2,
                    ),
                    similar_triangles,
                    claim,
                    region="content",
                    flow="row",
                    keys=("similar-triangles", "claim"),
                )
                self.caption("Same angles means the corresponding side ratios agree.")
                self.wait(.45)

                relation = self.styled_math(
                    r"a^2=c\,x,\qquad b^2=c\,(c-x)", role="math", color_role="foreground"
                )
                conclusion = self.styled_math(
                    r"a^2+b^2=c[x+(c-x)]=c^2", role="math", color_role="secondary"
                )
                self.beat(
                    Beat(
                        intent="prove",
                        audience_question="How do the two similarities recover the whole triangle?",
                        takeaway="The two projected pieces add back to the hypotenuse.",
                        focus="conclusion",
                        visual_metaphor="one shape containing scaled copies of itself",
                        transition="continuation",
                        max_active=2,
                    ),
                    relation,
                    conclusion,
                    region="content",
                    flow="column",
                    keys=("relation", "conclusion"),
                )
                self.play(Indicate(conclusion, color=THEME["accent"]))
                self.wait(.7)
        '''
    ).lstrip("\n")


def _algorithm(theme: dict[str, Any]) -> str:
    return _preamble(theme, "A directed breadth-first-search walkthrough.") + textwrap.dedent(
        r'''


        class AlgorithmWalkthroughScene(DirectedScene):
            design = DESIGN

            def construct(self):
                title = self.styled_text(
                    "Breadth-first search expands in rings", role="title", color_role="foreground"
                )
                self.place(title, "header", key="title")
                self.play(Write(title))

                vertices = [0, 1, 2, 3, 4, 5]
                edges = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 5)]
                graph = Graph(
                    vertices,
                    edges,
                    layout={
                        0: UP * 2,
                        1: LEFT * 2,
                        2: RIGHT * 2,
                        3: LEFT * 3 + DOWN * 2,
                        4: LEFT + DOWN * 2,
                        5: RIGHT * 2 + DOWN * 2,
                    },
                    vertex_config={"color": THEME["muted"]},
                    edge_config={"color": THEME["muted"]},
                )
                queue = self.styled_text("queue  [0]", role="body", color_role="secondary")
                queue_panel = self.panel(queue, stroke_role="secondary")
                self.beat(
                    Beat(
                        intent="introduce",
                        audience_question="How can a search avoid diving down the wrong branch?",
                        takeaway="The queue preserves a whole frontier at the same distance.",
                        focus="graph",
                        visual_metaphor="ripples expanding from a source",
                        transition="reveal",
                        max_active=3,
                    ),
                    graph,
                    region="left",
                    keys=("graph",),
                )
                self.place(queue_panel, "right", key="queue")
                self.play(FadeIn(queue_panel))

                order = [0, 1, 2, 3, 4, 5]
                queue_states = ["[1, 2]", "[2, 3, 4]", "[3, 4, 5]", "[4, 5]", "[5]", "[]"]
                for vertex, state in zip(order, queue_states):
                    next_queue = self.styled_text(
                        f"queue  {state}", role="body", color_role="secondary"
                    )
                    next_panel = self.panel(next_queue, stroke_role="secondary")
                    next_panel.move_to(queue_panel)
                    self.play(
                        graph[vertex].animate.set_color(THEME["primary"]),
                        ReplacementTransform(queue_panel, next_panel),
                        run_time=.5,
                    )
                    queue_panel = next_panel
                self.caption("A first visit therefore always uses a shortest number of edges.")
                self.wait(.8)
        '''
    ).lstrip("\n")


GENERATORS: dict[str, tuple[str, Callable[[dict[str, Any]], str]]] = {
    "equation_derivation": ("EquationDerivationScene", _equation),
    "function_explorer": ("FunctionExplorerScene", _function),
    "geometry_proof": ("GeometryProofScene", _geometry),
    "algorithm_walkthrough": ("AlgorithmWalkthroughScene", _algorithm),
}


def templates(params: Mapping[str, Any]) -> dict[str, Any]:
    operation = str(params.get("operation", "list"))
    available = [
        {"name": name, "scene": descriptor[0], "themes": ["midnight", "paper", "neon", "colorblind"]}
        for name, descriptor in sorted(GENERATORS.items())
    ] + [{"name": "generalized_fibonacci", "scene": "GeneralizedFibonacciScene", "themes": ["midnight", "paper", "neon", "colorblind"]}]
    if operation == "list":
        return {"templates": available}
    if operation != "generate":
        raise DirectorError("invalid_template_operation", f"Unknown template operation: {operation}")
    root = project_root(params)
    name = str(params.get("name", "equation_derivation"))
    theme_name = str(params.get("theme", "midnight"))
    if name == "generalized_fibonacci":
        source = generalized_fibonacci_source(theme=theme_name)
        scene = "GeneralizedFibonacciScene"
    elif name in GENERATORS:
        scene, generator = GENERATORS[name]
        source = generator(get_theme(theme_name))
    else:
        raise DirectorError("template_not_found", f"Unknown template: {name}", {"available": [item["name"] for item in available]})
    output = confined_path(root, str(params.get("output", f"scenes/{name}.py")))
    if output.exists() and not bool(params.get("force", False)):
        raise DirectorError("file_exists", f"Refusing to overwrite existing scene: {output}")
    compile(source, str(output), "exec")
    atomic_write(output, source)
    return {"template": name, "theme": theme_name, "scene": scene, "path": str(output)}
