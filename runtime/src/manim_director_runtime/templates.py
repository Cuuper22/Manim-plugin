"""Scene templates that `init` can add to a project."""

from __future__ import annotations

_EQUATION = r'''"""Odd numbers build perfect squares: first a picture, then a derivation."""

from manim import *
from manim_director_runtime import DirectedScene


class EquationDerivationScene(DirectedScene):
    symbols = {"n": "primary", "k": "secondary"}

    def odd_layers(self, count):
        """A count x count square built from L-shaped layers of 1, 3, 5, ... tiles."""
        layers = VGroup()
        for layer in range(count):
            color = self.theme.primary if layer % 2 == 0 else self.theme.secondary
            cells = [(layer, j) for j in range(layer + 1)] + [(i, layer) for i in range(layer)]
            tiles = [
                Square(0.42, stroke_width=0, fill_color=color, fill_opacity=0.9).move_to(
                    RIGHT * 0.5 * i + DOWN * 0.5 * j
                )
                for i, j in cells
            ]
            layers.add(VGroup(*tiles))
        return layers

    def construct(self):
        square = self.odd_layers(4)
        sums = self.math(r"1 + 3 + 5 + 7 = 4^2")
        with self.beat("pattern", transition="reveal"):
            self.title("Odd numbers build perfect squares")
            self.place(square, sums, direction=RIGHT, buff=1.0)
            self.caption("Each odd number wraps one more layer around the square.")

        claim = self.math(r"\sum_{k=1}^{n} (2k-1) = n^2")
        with self.beat("claim"):
            self.place(claim)

        with self.beat("proof"):
            proof = self.derive(
                r"\sum_{k=1}^{n} (2k-1)",
                (r"= 2\sum_{k=1}^{n} k - \sum_{k=1}^{n} 1", "split the sum"),
                (r"= n(n+1) - n", "sum of the first n integers"),
                (r"= n^2", "simplify"),
                replaces=claim,
            )
            self.caption("The picture predicted it; the algebra proves it.")
        self.highlight(proof.lines[-1], "n^2", box=True)
        self.wait()
'''

_FUNCTION = r'''"""The derivative of sine, seen as the slope of a moving tangent."""

from manim import *
from manim_director_runtime import DirectedScene


class FunctionExplorerScene(DirectedScene):
    def construct(self):
        axes = Axes(
            x_range=[-TAU, TAU, PI / 2],
            y_range=[-1.5, 1.5, 0.5],
            x_length=11,
            y_length=4,
            tips=False,
            axis_config={"color": self.theme.muted},
        )
        sine = axes.plot(np.sin, color=self.theme.primary)
        graph = VGroup(axes, sine)
        with self.beat("curve", transition="reveal"):
            self.title("The derivative is a moving slope")
            self.place(graph)

        with self.beat("slope", keep=[graph]):
            x = ValueTracker(-TAU)
            dot = always_redraw(
                lambda: Dot(axes.i2gp(x.get_value(), sine), color=self.theme.accent)
            )
            tangent = always_redraw(
                lambda: axes.get_secant_slope_group(
                    x.get_value(),
                    sine,
                    dx=0.01,
                    secant_line_color=self.theme.accent,
                    secant_line_length=2.6,
                )
            )
            self.caption("Slide along the curve and watch the tangent turn.")
            self.add(tangent, dot)
            self.play(x.animate.set_value(TAU), run_time=5, rate_func=linear)

        with self.beat("derivative", keep=[graph]):
            cosine = axes.plot(np.cos, color=self.theme.secondary)
            self.play(Create(cosine))
            self.caption("The slope of sine traces out cosine.")
        self.wait()
'''

_GEOMETRY = r'''"""Why a^2 + b^2 = c^2: an altitude splits a right triangle into similar copies."""

from manim import *
from manim_director_runtime import DirectedScene, Region


class GeometryProofScene(DirectedScene):
    symbols = {"a": "primary", "b": "secondary", "c": "accent"}

    def construct(self):
        A, B, C = np.array([-3.0, -1.5, 0]), np.array([3.0, -1.5, 0]), np.array([-3.0, 1.5, 0])
        triangle = Polygon(A, B, C, color=self.theme.foreground, stroke_width=3)
        labels = VGroup(
            self.math("a").next_to(Line(A, C), LEFT),
            self.math("b").next_to(Line(A, B), DOWN),
            self.math("c").move_to((B + C) / 2 + 0.35 * UR),
        )
        figure = VGroup(triangle, RightAngle(Line(A, B), Line(A, C), length=0.3), labels)
        with self.beat("triangle", transition="reveal"):
            self.title("One altitude, two similar copies")
            self.place(figure, region=Region.LEFT)

        with self.beat("split", keep=[figure]):
            A, B, C = triangle.get_vertices()
            hypotenuse = (B - C) / np.linalg.norm(B - C)
            foot = C + np.dot(A - C, hypotenuse) * hypotenuse
            shade = {"stroke_width": 0, "fill_opacity": 0.3}
            pieces = VGroup(
                Polygon(A, C, foot, fill_color=self.theme.primary, **shade),
                Polygon(A, foot, B, fill_color=self.theme.secondary, **shade),
                DashedLine(A, foot, color=self.theme.muted),
            )
            self.play(FadeIn(pieces))
            self.caption("Both pieces have the same angles as the whole triangle.")

        with self.beat("proof", keep=[figure, pieces]):
            proof = self.derive(
                (r"a^2 = c\,x", "left piece ~ whole"),
                (r"b^2 = c\,(c - x)", "right piece ~ whole"),
                (r"a^2 + b^2 = c^2", "add"),
                region=Region.RIGHT,
            )
        self.highlight(proof.lines[-1], "c^2", box=True)
        self.wait()
'''

_ALGORITHM = r'''"""Breadth-first search expands from its source in rings."""

from manim import *
from manim_director_runtime import DirectedScene, Region


class AlgorithmWalkthroughScene(DirectedScene):
    def construct(self):
        edges = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 5)]
        layout = {
            0: UP * 2,
            1: LEFT * 1.6,
            2: RIGHT * 1.6,
            3: LEFT * 2.6 + DOWN * 2,
            4: LEFT * 0.6 + DOWN * 2,
            5: RIGHT * 1.6 + DOWN * 2,
        }
        graph = Graph(
            list(range(6)),
            edges,
            layout=layout,
            labels=True,
            vertex_config={
                "fill_color": self.theme.background,
                "stroke_color": self.theme.muted,
                "stroke_width": 3,
                "radius": 0.3,
            },
            edge_config={"stroke_color": self.theme.muted},
        )
        queue = self.text("queue: 0", "body", color=self.theme.secondary)
        with self.beat("graph", transition="reveal"):
            self.title("Breadth-first search expands in rings")
            self.place(graph, region=Region.LEFT)
            self.place(queue, region=Region.RIGHT)

        with self.beat("search", keep=[graph, queue]):
            states = ["1, 2", "2, 3, 4", "3, 4, 5", "4, 5", "5", "(empty)"]
            for vertex, state in enumerate(states):
                following = self.text(f"queue: {state}", "body", color=self.theme.secondary)
                self.place(following, region=Region.RIGHT, replaces=queue)
                self.play(graph[vertex].animate.set_stroke(self.theme.primary, width=5))
                queue = following
            self.caption("The first visit to a vertex always uses the fewest edges.")
        self.wait()
'''

# Scene template name -> (scene class, source).
SCENE_TEMPLATES: dict[str, tuple[str, str]] = {
    "equation_derivation": ("EquationDerivationScene", _EQUATION),
    "function_explorer": ("FunctionExplorerScene", _FUNCTION),
    "geometry_proof": ("GeometryProofScene", _GEOMETRY),
    "algorithm_walkthrough": ("AlgorithmWalkthroughScene", _ALGORITHM),
}
