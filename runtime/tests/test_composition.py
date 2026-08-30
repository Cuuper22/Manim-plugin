from __future__ import annotations

import unittest

from manim_director_runtime.composition import (
    Beat,
    CompositionError,
    CompositionLayout,
    DesignSystem,
    LayoutItem,
    Region,
)


class CompositionTests(unittest.TestCase):
    def test_named_lanes_are_disjoint_and_inside_safe_frame(self) -> None:
        layout = CompositionLayout()
        safe = layout.region(Region.SAFE)
        header = layout.region(Region.HEADER)
        content = layout.region(Region.CONTENT)
        caption = layout.region(Region.CAPTION)
        self.assertTrue(safe.contains(header))
        self.assertTrue(safe.contains(content))
        self.assertTrue(safe.contains(caption))
        self.assertFalse(header.intersects(content))
        self.assertFalse(content.intersects(caption))

    def test_arrange_reflows_without_overlap_and_honors_budget(self) -> None:
        layout = CompositionLayout(DesignSystem(max_active=3))
        items = [
            LayoutItem("hero", 3.0, 1.5, priority=100),
            LayoutItem("proof", 3.2, 1.7, priority=80),
            LayoutItem("label", 2.4, 0.8, priority=60),
            LayoutItem("aside", 2.4, 0.8, priority=10),
        ]
        plan = layout.arrange(items, flow="auto")
        self.assertEqual(plan.evicted, ("aside",))
        self.assertEqual({item.key for item in plan.placements}, {"hero", "proof", "label"})
        lane = layout.region(Region.CONTENT)
        for index, first in enumerate(plan.placements):
            self.assertTrue(lane.contains(first.rect))
            for second in plan.placements[index + 1:]:
                self.assertFalse(first.rect.intersects(second.rect))

    def test_place_never_uses_illegibly_small_scale(self) -> None:
        layout = CompositionLayout()
        with self.assertRaisesRegex(CompositionError, "readable minimum"):
            layout.place(LayoutItem("wall", 100, 100, min_scale=0.7))
        with self.assertRaisesRegex(CompositionError, "readable minimum"):
            layout.arrange([LayoutItem("wall", 100, 100, min_scale=0.7)])

    def test_direction_mapping_and_beat_contract(self) -> None:
        design = DesignSystem.from_mapping({
            "theme": {"primary": "#123456"},
            "safe_area": {"top": 0.05, "right": 0.05, "bottom": 0.08, "left": 0.05},
            "direction": {
                "composition": {"density": "balanced", "max_active": 3, "caption_lane": True},
                "typography": {"scale": {"hero": 68}},
                "motion": {"continuation": "morph", "contrast": "lateral", "reveal": "fade", "chapter": "reset"},
                "narrative": {"audience": "algebra students", "principle": "one-idea-per-beat"},
            },
        })
        self.assertEqual(design.color("primary"), "#123456")
        self.assertEqual(design.typography.hero, 68)
        self.assertAlmostEqual(design.safe_area.top, 0.4)
        self.assertEqual(design.stroke("accent")["stroke_color"], design.color("accent"))
        direct_style = DesignSystem.from_mapping({
            "background": "#010203",
            "foreground": "#FAFAFA",
            "primary": "#ABCDEF",
            "font": "DejaVu Sans",
        })
        self.assertEqual(direct_style.color("background"), "#010203")
        self.assertEqual(direct_style.color("primary"), "#ABCDEF")
        beat = Beat(
            intent="prove",
            audience_question="Why is this exact?",
            takeaway="The factors cancel.",
            focus="identity",
            visual_metaphor="A balanced ledger",
            transition="continuation",
        )
        self.assertEqual(beat.focus, "identity")
        with self.assertRaises(CompositionError):
            Beat(
                intent="explain",
                audience_question="What matters?",
                takeaway="One thing.",
                focus="",
                visual_metaphor="A spotlight",
            )


if __name__ == "__main__":
    unittest.main()
