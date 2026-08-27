from manim import *


class DirectedRecurrence(Scene):
    """A complete, editable starter scene used by the explainer template."""

    def construct(self):
        self.camera.background_color = "#0B1020"
        foreground = "#F7F8FC"
        primary = "#78DCE8"
        secondary = "#FFD866"
        accent = "#FF6188"

        title = Text("Where does the next number come from?", color=foreground, font_size=42)
        title.to_edge(UP, buff=0.65)
        values = [1, 1, 2, 3, 5]
        terms = VGroup(*[
            Integer(value, color=foreground, font_size=50) for value in values
        ]).arrange(RIGHT, buff=0.78).shift(DOWN * 0.25)
        unknown = MathTex("?", color=accent, font_size=58).next_to(terms, RIGHT, buff=0.78)

        self.next_section("hook")
        self.play(Write(title), LaggedStart(*[FadeIn(term, shift=UP * 0.15) for term in terms], lag_ratio=0.12))
        self.play(FadeIn(unknown, scale=0.75))
        self.wait(0.7)

        self.next_section("construction")
        left, right = terms[-2], terms[-1]
        left_box = SurroundingRectangle(left, color=primary, buff=0.12)
        right_box = SurroundingRectangle(right, color=secondary, buff=0.12)
        sum_label = MathTex("3", "+", "5", "=", "8", font_size=50)
        sum_label.set_color_by_tex("3", primary)
        sum_label.set_color_by_tex("5", secondary)
        sum_label.set_color_by_tex("8", accent)
        sum_label.next_to(terms, DOWN, buff=0.8)
        self.play(Create(left_box), Create(right_box))
        self.play(TransformFromCopy(left, sum_label[0]), Write(sum_label[1]), TransformFromCopy(right, sum_label[2]))
        self.play(Write(sum_label[3:]), unknown.animate.set_opacity(0.25))
        eight = Integer(8, color=accent, font_size=50).move_to(unknown)
        self.play(ReplacementTransform(unknown, eight))
        self.wait(0.6)

        self.next_section("rule")
        recurrence = MathTex("a_{n+2}", "=", "a_{n+1}", "+", "a_n", font_size=52)
        recurrence[0].set_color(accent)
        recurrence[2].set_color(secondary)
        recurrence[4].set_color(primary)
        recurrence.next_to(title, DOWN, buff=0.65)
        self.play(
            FadeOut(sum_label, shift=DOWN * 0.15),
            FadeOut(left_box),
            FadeOut(right_box),
            ReplacementTransform(title, Text("Remember two. Make the next.", color=foreground, font_size=42).to_edge(UP, buff=0.65)),
            Write(recurrence),
            terms.animate.shift(DOWN * 0.55),
            eight.animate.shift(DOWN * 0.55),
        )
        self.wait(0.9)

        self.next_section("resolve")
        pair = SurroundingRectangle(VGroup(terms[-1], eight), color=accent, buff=0.18)
        self.play(Create(pair))
        self.wait(1.2)
