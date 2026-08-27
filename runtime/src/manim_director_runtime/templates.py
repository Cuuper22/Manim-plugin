from __future__ import annotations

import re
import textwrap
from pathlib import Path
from typing import Any, Callable, Mapping

from .errors import DirectorError
from .sample import generalized_fibonacci_source
from .themes import get_theme
from .util import atomic_write, confined_path, project_root


def _equation(theme: dict[str, Any]) -> str:
    return textwrap.dedent(rf'''
        """A complete equation-derivation template using the sum of odd numbers."""
        from manim import *

        class EquationDerivationScene(Scene):
            def construct(self):
                self.camera.background_color = "{theme['background']}"
                title = Text("Why the first n odd numbers sum to n²", font="{theme['font']}", color="{theme['foreground']}", font_size=38).to_edge(UP)
                examples = MathTex(r"1=1^2", r"\quad 1+3=2^2", r"\quad 1+3+5=3^2", color="{theme['primary']}", font_size=44)
                formula = MathTex(r"\sum_{{k=1}}^n(2k-1)=n^2", color="{theme['secondary']}", font_size=58)
                proof = MathTex(r"2\sum_{{k=1}}^n k-n=n(n+1)-n=n^2", color="{theme['foreground']}", font_size=48)
                group = VGroup(examples, formula, proof).arrange(DOWN, buff=.7)
                self.play(Write(title))
                self.play(LaggedStart(*[Write(part) for part in examples], lag_ratio=.2))
                self.next_section("general_claim")
                self.play(Write(formula))
                self.next_section("derivation")
                self.play(Write(proof), run_time=1.6)
                self.play(Indicate(formula, color="{theme['accent']}"))
                self.wait(1)
        ''').lstrip()


def _function(theme: dict[str, Any]) -> str:
    return textwrap.dedent(rf'''
        """A complete function-exploration template for sine and its derivative."""
        from manim import *

        class FunctionExplorerScene(Scene):
            def construct(self):
                self.camera.background_color = "{theme['background']}"
                axes = Axes(x_range=[-TAU, TAU, PI/2], y_range=[-1.5, 1.5, .5], x_length=11, y_length=5, tips=False)
                axes.set_color("{theme['muted']}")
                sine = axes.plot(lambda x: np.sin(x), x_range=[-TAU, TAU], color="{theme['primary']}")
                cosine = axes.plot(lambda x: np.cos(x), x_range=[-TAU, TAU], color="{theme['secondary']}")
                labels = VGroup(MathTex(r"f(x)=\sin x", color="{theme['primary']}"), MathTex(r"f'(x)=\cos x", color="{theme['secondary']}")).arrange(RIGHT, buff=.8).to_edge(UP)
                tracker = ValueTracker(-TAU)
                dot = always_redraw(lambda: Dot(axes.c2p(tracker.get_value(), np.sin(tracker.get_value())), color="{theme['accent']}"))
                tangent = always_redraw(lambda: axes.get_secant_slope_group(tracker.get_value(), sine, dx=.05, secant_line_color="{theme['accent']}", secant_line_length=3))
                self.play(Create(axes), Create(sine), Write(labels[0]))
                self.add(dot, tangent)
                self.play(tracker.animate.set_value(TAU), run_time=4, rate_func=linear)
                self.next_section("derivative")
                self.play(Create(cosine), Write(labels[1]))
                self.wait(1)
        ''').lstrip()


def _geometry(theme: dict[str, Any]) -> str:
    return textwrap.dedent(rf'''
        """A complete right-triangle similarity template."""
        from manim import *

        class GeometryProofScene(Scene):
            def construct(self):
                self.camera.background_color = "{theme['background']}"
                triangle = Polygon(LEFT*3+DOWN*2, RIGHT*3+DOWN*2, LEFT*3+UP*2, color="{theme['primary']}", fill_opacity=.12)
                right_angle = RightAngle(Line(LEFT*3+DOWN*2, RIGHT*3+DOWN*2), Line(LEFT*3+DOWN*2, LEFT*3+UP*2), length=.35, color="{theme['secondary']}")
                labels = VGroup(MathTex("a", color="{theme['foreground']}").next_to(triangle, LEFT), MathTex("b", color="{theme['foreground']}").next_to(triangle, DOWN), MathTex("c", color="{theme['accent']}").move_to(UP*.25+RIGHT*.2))
                claim = MathTex(r"a^2+b^2=c^2", color="{theme['secondary']}", font_size=58).to_edge(UP)
                self.play(Create(triangle), Create(right_angle), FadeIn(labels))
                self.next_section("claim")
                self.play(Write(claim))
                altitude = DashedLine(LEFT*3+DOWN*2, triangle.get_vertices()[1] + UP*2.77, color="{theme['accent']}")
                self.play(Create(altitude))
                explanation = Text("The altitude creates two triangles similar to the original.", font="{theme['font']}", color="{theme['foreground']}", font_size=28).to_edge(DOWN)
                self.play(FadeIn(explanation))
                self.play(Indicate(claim, color="{theme['accent']}"))
                self.wait(1)
        ''').lstrip()


def _algorithm(theme: dict[str, Any]) -> str:
    return textwrap.dedent(rf'''
        """A complete breadth-first traversal template."""
        from manim import *

        class AlgorithmWalkthroughScene(Scene):
            def construct(self):
                self.camera.background_color = "{theme['background']}"
                vertices = [0, 1, 2, 3, 4, 5]
                edges = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 5)]
                layout = {{0: UP*2, 1: LEFT*2, 2: RIGHT*2, 3: LEFT*3+DOWN*2, 4: LEFT+DOWN*2, 5: RIGHT*2+DOWN*2}}
                graph = Graph(vertices, edges, layout=layout, vertex_config={{"color": "{theme['muted']}"}}, edge_config={{"color": "{theme['muted']}"}})
                title = Text("Breadth-first search visits one layer at a time", font="{theme['font']}", color="{theme['foreground']}", font_size=34).to_edge(UP)
                queue = Text("queue: [0]", font="{theme['font']}", color="{theme['secondary']}", font_size=30).to_edge(DOWN)
                self.play(Write(title), Create(graph), Write(queue))
                order = [0, 1, 2, 3, 4, 5]
                queue_states = ["[1, 2]", "[2, 3, 4]", "[3, 4, 5]", "[4, 5]", "[5]", "[]"]
                for vertex, state in zip(order, queue_states):
                    new_queue = Text(f"queue: {{state}}", font="{theme['font']}", color="{theme['secondary']}", font_size=30).to_edge(DOWN)
                    self.play(graph[vertex].animate.set_color("{theme['primary']}"), Transform(queue, new_queue), run_time=.55)
                self.wait(1)
        ''').lstrip()


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
