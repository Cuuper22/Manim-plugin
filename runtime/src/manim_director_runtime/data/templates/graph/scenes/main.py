"""The derivative of x², seen as the limit of secant slopes and then computed."""

from manim import *

from manim_director_runtime import DirectedScene, Region


class TangentSlope(DirectedScene):
    symbols = {"h": "accent"}

    def readout(self, label, tracker, color):
        number = DecimalNumber(tracker.get_value(), num_decimal_places=2, color=color)
        number.add_updater(lambda n: n.set_value(tracker.get_value()))
        return VGroup(self.math(label), number).arrange(RIGHT, buff=0.2)

    def construct(self):
        axes = Axes(
            x_range=[-0.5, 2.6, 1],
            y_range=[-0.5, 6.5, 1],
            x_length=5.6,
            y_length=4.6,
            tips=False,
            axis_config={"color": self.theme.muted},
            x_axis_config={"numbers_to_include": [1, 2]},
        )
        curve = axes.plot(lambda x: x**2, x_range=[-0.5, 2.5], color=self.theme.primary)
        name = self.math(r"f(x) = x^2", color=self.theme.primary).scale(0.8)
        graph = VGroup(axes, curve, name.next_to(axes.c2p(2.5, 6.25), LEFT, buff=0.3))
        with self.beat("curve", transition="reveal"):
            self.title("The derivative is a limit of slopes")
            self.place(graph, region=Region.LEFT)
            self.caption("How steep is the curve at x = 1?")

        h = ValueTracker(1.4)
        secant = always_redraw(
            lambda: axes.get_secant_slope_group(
                1,
                curve,
                dx=h.get_value(),
                dx_line_color=self.theme.muted,
                dy_line_color=self.theme.muted,
                secant_line_color=self.theme.accent,
                secant_line_length=4,
            )
        )
        p = Dot(axes.i2gp(1, curve), color=self.theme.foreground)
        q = always_redraw(lambda: Dot(axes.i2gp(1 + h.get_value(), curve), color=self.theme.accent))
        slope = ValueTracker(2 + h.get_value())
        slope.add_updater(lambda s: s.set_value(2 + h.get_value()))
        readouts = VGroup(
            self.math(r"\text{slope} = \frac{f(1 + h) - f(1)}{h}"),
            self.readout("h =", h, self.theme.accent),
            self.readout(r"\text{slope} =", slope, self.theme.foreground),
        ).arrange(DOWN, aligned_edge=LEFT, buff=0.45)
        with self.beat("secant", keep=[graph]):
            self.place(readouts, region=Region.RIGHT)
            self.caption("Join the point to a neighbor h further along.")
            self.play(FadeIn(secant), FadeIn(p), FadeIn(q))
            self.add(slope)
            self.play(h.animate.set_value(0.01), run_time=5)
            self.caption("As h shrinks the secant turns into the tangent, with slope 2.")

        with self.beat("limit", keep=[graph, secant, p, q]):
            self.caption("Every x works the same way: the slope tends to 2x.")
            proof = self.derive(
                r"\frac{f(x + h) - f(x)}{h} = \frac{(x + h)^2 - x^2}{h}",
                r"= \frac{2xh + h^2}{h}",
                r"= 2x + h",
                r"\to 2x",
                region=Region.RIGHT,
            )

        derivative = axes.plot(lambda x: 2 * x, x_range=[-0.25, 2.5], color=self.theme.secondary)
        with self.beat("derivative", keep=[graph, proof]):
            self.caption("Those slopes form a new function: the derivative f′(x) = 2x.")
            self.play(Create(derivative), run_time=1.5)
        self.highlight(proof.lines[-1], "2x", color="secondary", box=True)
        self.wait()
