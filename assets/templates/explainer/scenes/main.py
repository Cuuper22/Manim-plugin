"""A directed starter: one visual spine, one inference per beat."""

from manim import Circle, CurvedArrow, RIGHT, SurroundingRectangle, VGroup

from manim_director_runtime.composition import Beat, DesignSystem, DirectedScene, Region


PROJECT_STYLE = {
    "theme": {
        "background": "#0B1020",
        "foreground": "#F7F8FC",
        "primary": "#78DCE8",
        "secondary": "#FFD866",
        "accent": "#FF6188",
        "muted": "#72798C",
        "success": "#A9DC76",
        "font": "DejaVu Sans",
        "stroke_width": 4,
    },
    "direction": {
        "composition": {"density": "spacious", "max_active": 4, "caption_lane": True},
        "typography": {
            "scale": {
                "hero": 64,
                "title": 44,
                "section": 36,
                "body": 30,
                "math": 48,
                "label": 24,
                "caption": 25,
                "micro": 18,
            }
        },
        "motion": {
            "continuation": "morph",
            "contrast": "lateral",
            "reveal": "draw",
            "chapter": "reset",
        },
        "narrative": {
            "audience": "Curious adults comfortable with basic algebra",
            "principle": "one-idea-per-beat",
        },
    },
    "safe_area": {"top": 0.05, "right": 0.05, "bottom": 0.08, "left": 0.05},
}


class DirectedRecurrence(DirectedScene):
    """Show a recurrence by transforming one recognizable number row."""

    design = DesignSystem.from_mapping(PROJECT_STYLE)

    def number_row(self, values, *, missing=False):
        nodes = VGroup()
        for value in values:
            ring = Circle(
                radius=0.43,
                color=self.design.color("muted"),
                stroke_width=self.design.stroke_width,
            )
            label = self.styled_text(str(value), role="section")
            nodes.add(VGroup(ring, label))
        if missing:
            ring = Circle(
                radius=0.43,
                color=self.design.color("accent"),
                stroke_width=self.design.stroke_width,
            )
            nodes.add(VGroup(ring, self.styled_text("?", role="section", color_role="accent")))
        return nodes.arrange(RIGHT, buff=self.design.spacing.sm)

    def construct(self):
        self.next_section("hook")
        question = self.styled_text("Where does the next number come from?", role="title")
        open_row = self.number_row([1, 1, 2, 3, 5], missing=True)
        self.beat(
            Beat(
                intent="introduce",
                audience_question="Where does the next number come from?",
                takeaway="The missing term depends on the two values immediately before it.",
                focus="sequence",
                transition="reveal",
                visual_metaphor="a two-term window moving along one number row",
                max_active=3,
            ),
            question,
            open_row,
            keys=("question", "sequence"),
            region=Region.CONTENT,
            flow="column",
        )
        self.caption("Keep your eye on the last two values.")

        self.next_section("construction")
        active_row = self.number_row([1, 1, 2, 3, 5], missing=True)
        left_box = SurroundingRectangle(
            active_row[-3], color=self.design.color("primary"), buff=self.design.spacing.xs
        )
        right_box = SurroundingRectangle(
            active_row[-2], color=self.design.color("secondary"), buff=self.design.spacing.xs
        )
        equation = self.styled_math(r"3+5=8", color_role="accent")
        equation.next_to(active_row, direction=(0, -1, 0), buff=self.design.spacing.lg)
        arrows = VGroup(
            CurvedArrow(
                active_row[-3].get_bottom(),
                equation.get_top() + (-0.55, 0, 0),
                color=self.design.color("primary"),
                angle=0.35,
            ),
            CurvedArrow(
                active_row[-2].get_bottom(),
                equation.get_top() + (0.55, 0, 0),
                color=self.design.color("secondary"),
                angle=-0.35,
            ),
        )
        construction = VGroup(active_row, left_box, right_box, arrows, equation)
        self.beat(
            Beat(
                intent="explain",
                audience_question="What operation turns the pair into the missing value?",
                takeaway="Adding the active pair produces the next term.",
                focus="construction",
                transition="continuation",
                visual_metaphor="two remembered values converging into one result",
                max_active=4,
            ),
            construction,
            keys=("construction",),
            region=Region.CONTENT,
        )
        self.caption("Three and five do not decorate the rule—they perform it.")

        self.next_section("rule")
        concrete = self.number_row([1, 1, 2, 3, 5, 8])
        recurrence = self.styled_math(r"a_{n+2}=a_{n+1}+a_n", color_role="primary")
        self.beat(
            Beat(
                intent="reveal",
                audience_question="How can the motion be named once and reused?",
                takeaway="The recurrence equation is a compact name for the operation already seen.",
                focus="rule",
                transition="continuation",
                visual_metaphor="concrete motion condensing into notation",
                max_active=3,
            ),
            concrete,
            recurrence,
            keys=("concrete", "rule"),
            region=Region.CONTENT,
            flow="column",
        )
        self.caption("The symbols arrive after the idea, not before it.")

        self.next_section("resolve")
        resolved = self.number_row([1, 1, 2, 3, 5, 8])
        window = SurroundingRectangle(
            VGroup(resolved[-2], resolved[-1]),
            color=self.design.color("accent"),
            buff=self.design.spacing.sm,
        )
        prompt = self.styled_text("Remember two. Add. Advance.", role="section")
        final_state = VGroup(resolved, window)
        self.beat(
            Beat(
                intent="recap",
                audience_question="Can I now predict what happens next?",
                takeaway="Remember the latest pair, add it, then advance the window.",
                focus="resolved-sequence",
                transition="reveal",
                visual_metaphor="a two-term window stepping along the sequence",
                max_active=3,
            ),
            final_state,
            prompt,
            keys=("resolved-sequence", "payoff"),
            region=Region.CONTENT,
            flow="column",
        )
        self.caption("The next step is now visible before it happens.")
        self.wait(1.2)
