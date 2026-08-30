"""Manim adapter for the pure composition and narrative primitives.

Importing this module intentionally imports Manim.  Package-level exports are
lazy so command-line and inspection work stays light when no render is running.
"""

from __future__ import annotations

from dataclasses import dataclass
import textwrap
from typing import Any, Iterable, Mapping, Sequence

from manim import (
    DOWN,
    LEFT,
    RIGHT,
    UP,
    Create,
    FadeIn,
    FadeOut,
    GrowFromCenter,
    MathTex,
    Mobject,
    MovingCameraScene,
    RoundedRectangle,
    Scene,
    Text,
    ThreeDScene,
    VGroup,
    ReplacementTransform,
)

from .composition import (
    Beat,
    CompositionError,
    CompositionLayout,
    DesignSystem,
    LayoutItem,
    LayoutPlan,
    Placement,
    Rect,
    Region,
    TransitionKind,
)


@dataclass(slots=True)
class _Active:
    key: str
    mobject: Mobject
    region: Region
    priority: int
    placement: Placement


class CompositionMixin:
    """Add directed composition to any compatible Manim ``Scene`` class.

    Prefer one of the ready-made scene classes below.  The mixin remains public
    for specialized Manim scene types and optional integrations.
    """

    design = DesignSystem()

    def __init__(
        self,
        *args: Any,
        design: DesignSystem | Mapping[str, Any] | None = None,
        direction: Mapping[str, Any] | None = None,
        **kwargs: Any,
    ) -> None:
        if design is not None and direction is not None:
            raise CompositionError("pass design or direction, not both")
        configured: DesignSystem
        if isinstance(design, DesignSystem):
            configured = design
        elif isinstance(design, Mapping):
            configured = DesignSystem.from_mapping(design)
        elif direction is not None:
            configured = DesignSystem.from_mapping({"direction": direction})
        else:
            inherited = getattr(type(self), "design", DesignSystem())
            configured = (
                inherited
                if isinstance(inherited, DesignSystem)
                else DesignSystem.from_mapping(inherited)
            )
        self.design = configured
        self._director_ready = False
        super().__init__(*args, **kwargs)

    def setup(self) -> None:
        super().setup()
        self._ensure_director()

    def _ensure_director(self) -> None:
        if self._director_ready:
            return
        self.camera.background_color = self.design.color("background")
        self.composition = CompositionLayout(self.design)
        self._director_active: dict[str, _Active] = {}
        self._director_focus_saved: list[Mobject] = []
        self.current_beat: Beat | None = None
        self.beat_history: list[Beat] = []
        self._director_counter = 0
        self._director_ready = True

    def styled_text(
        self,
        text: str,
        *,
        role: str = "body",
        color_role: str = "foreground",
        max_width: float | None = None,
        **kwargs: Any,
    ) -> Text:
        """Create restrained, role-based text and fit it without clipping."""

        self._ensure_director()
        if not str(text).strip():
            raise CompositionError("text cannot be empty")
        font = kwargs.pop("font", self.design.font)
        font_size = kwargs.pop("font_size", self.design.font_size(role))
        color = kwargs.pop("color", self.design.color(color_role))
        result = Text(str(text), font=font, font_size=font_size, color=color, **kwargs)
        allowed_width = max_width or self.composition.region(Region.SAFE).width
        if result.width > allowed_width:
            result.scale_to_fit_width(allowed_width)
        return result

    def styled_math(
        self,
        *tex: str,
        role: str = "math",
        color_role: str = "foreground",
        max_width: float | None = None,
        **kwargs: Any,
    ) -> MathTex:
        """Create hierarchy-aware math and fit it to the safe width."""

        self._ensure_director()
        if not tex or not all(str(part).strip() for part in tex):
            raise CompositionError("math cannot be empty")
        font_size = kwargs.pop("font_size", self.design.font_size(role))
        color = kwargs.pop("color", self.design.color(color_role))
        result = MathTex(*tex, font_size=font_size, color=color, **kwargs)
        allowed_width = max_width or self.composition.region(Region.SAFE).width
        if result.width > allowed_width:
            result.scale_to_fit_width(allowed_width)
        return result

    def panel(
        self,
        content: Mobject,
        *,
        padding: float | None = None,
        stroke_role: str = "muted",
        fill_role: str = "background",
        fill_opacity: float = 0.82,
        corner_radius: float = 0.16,
    ) -> VGroup:
        """Wrap content in a quiet panel using design tokens."""

        self._ensure_director()
        pad = self.design.effective_spacing.md if padding is None else float(padding)
        if pad < 0:
            raise CompositionError("panel padding cannot be negative")
        background = RoundedRectangle(
            width=max(0.05, content.width + 2 * pad),
            height=max(0.05, content.height + 2 * pad),
            corner_radius=corner_radius,
            stroke_color=self.design.color(stroke_role),
            stroke_width=self.design.stroke_width * 0.55,
            fill_color=self.design.color(fill_role),
            fill_opacity=fill_opacity,
        ).move_to(content)
        background.set_z_index(content.z_index - 1)
        return VGroup(background, content)

    def place(
        self,
        mobject: Mobject,
        region: Region | str = Region.CONTENT,
        *,
        key: str | None = None,
        anchor: str = "center",
        priority: int = 50,
        min_scale: float = 0.58,
        gap: float | None = None,
    ) -> Mobject:
        """Place one object without covering existing directed objects.

        If the requested lane is full, conflicting lower-context objects are
        retired rather than left underneath the new visual.
        """

        self._ensure_director()
        resolved_key = self._key(key)
        item = self._item(resolved_key, mobject, priority, min_scale)
        occupied = [active.placement for active in self._director_active.values() if active.key != resolved_key]
        plan = self.composition.place(
            item,
            region=region,
            anchor=anchor,
            occupied=occupied,
            gap=gap,
        )
        for old_key in plan.evicted:
            self._retire(old_key, animate=False)
        placement = plan.for_key(resolved_key)
        self._apply_placement(mobject, placement)
        self._register(resolved_key, mobject, Region(region), priority, placement)
        return mobject

    def layout(
        self,
        *mobjects: Mobject,
        region: Region | str = Region.CONTENT,
        flow: str = "auto",
        keys: Sequence[str] | None = None,
        priorities: Sequence[int] | None = None,
        min_scale: float = 0.58,
        gap: float | None = None,
    ) -> LayoutPlan:
        """Reflow a related set into one collision-free lane and track it."""

        self._ensure_director()
        plan, records = self._position_many(
            mobjects,
            region=region,
            flow=flow,
            keys=keys,
            priorities=priorities,
            min_scale=min_scale,
            gap=gap,
            max_active=None,
        )
        new_by_key = {record.key: record for record in records}
        separation = self.design.effective_spacing.md if gap is None else float(gap)
        for old_key, old in list(self._director_active.items()):
            replacement = new_by_key.get(old_key)
            if replacement is not None:
                if replacement.mobject is not old.mobject:
                    self._retire(old_key, animate=False)
                continue
            if any(
                old.placement.rect.intersects(record.placement.rect, gap=separation)
                for record in records
            ):
                self._retire(old_key, animate=False)
        for record in records:
            self._register(record.key, record.mobject, record.region, record.priority, record.placement)
        return plan

    def beat(
        self,
        beat: Beat,
        *mobjects: Mobject,
        region: Region | str = Region.CONTENT,
        flow: str = "auto",
        keys: Sequence[str] | None = None,
        priorities: Sequence[int] | None = None,
        min_scale: float = 0.58,
        gap: float | None = None,
        run_time: float | None = None,
    ) -> LayoutPlan:
        """Stage and animate one audience-centered explanatory beat.

        A beat replaces the prior stage content according to its semantic
        transition. Header and caption lanes persist except across a chapter
        reset. New content is bounded by the beat's cognitive-load budget.
        """

        self._ensure_director()
        if not isinstance(beat, Beat):
            raise TypeError("beat must be a Beat instance")
        if not mobjects:
            raise CompositionError("a beat needs at least one visual object")
        budget = beat.max_active or self.design.max_active
        if len(mobjects) > budget:
            raise CompositionError(
                f"beat has {len(mobjects)} active visuals but its budget is {budget}; group or sequence them"
            )
        resolved_keys = self._resolve_keys(mobjects, keys)
        if beat.focus not in resolved_keys:
            raise CompositionError(
                f"beat focus {beat.focus!r} must name one of its visual keys: {', '.join(resolved_keys)}"
            )

        self._release_focus(animate=False)
        plan, new_records = self._position_many(
            mobjects,
            region=region,
            flow=flow,
            keys=resolved_keys,
            priorities=priorities,
            min_scale=min_scale,
            gap=gap,
            max_active=budget,
        )
        old_records = self._stage_records(
            include_header=beat.transition is TransitionKind.CHAPTER,
            include_caption=beat.transition is TransitionKind.CHAPTER,
        )
        old_keys = {record.key for record in old_records}
        separation = self.design.effective_spacing.md if gap is None else float(gap)
        persistent = [
            record for record in self._director_active.values()
            if record.key not in old_keys
        ]
        for record in new_records:
            collision = next((
                kept for kept in persistent
                if kept.key == record.key
                or kept.placement.rect.intersects(record.placement.rect, gap=separation)
            ), None)
            if collision is not None:
                raise CompositionError(
                    f"beat visual {record.key!r} would cover persistent {collision.region.value} "
                    f"visual {collision.key!r}; use the content lane or clear that lane first"
                )
        self._play_transition(beat, old_records, new_records, run_time=run_time)

        for old in old_records:
            self._director_active.pop(old.key, None)
        for record in new_records:
            self._register(record.key, record.mobject, record.region, record.priority, record.placement)
        self.current_beat = beat
        self.beat_history.append(beat)
        target = next(record.mobject for record in new_records if record.key == beat.focus)
        self.focus(target, run_time=self.design.motion.quick)
        return plan

    def caption(
        self,
        text: str | None,
        *,
        key: str = "__caption__",
        run_time: float | None = None,
    ) -> Mobject | None:
        """Replace the dedicated caption lane with at most two short lines."""

        self._ensure_director()
        old = self._director_active.get(key)
        duration = run_time or self.design.motion.quick
        if text is None or not text.strip():
            if old is not None:
                self.play(FadeOut(old.mobject), run_time=duration)
                self._director_active.pop(key, None)
            return None
        lines = textwrap.wrap(" ".join(text.split()), width=78)
        if len(lines) > 2:
            raise CompositionError("caption exceeds two lines; split it across beats")
        caption_text = self.styled_text(
            "\n".join(lines),
            role="caption",
            color_role="foreground",
            max_width=self.composition.region(Region.CAPTION).width - 0.48,
            line_spacing=0.85,
        )
        caption_panel = self.panel(
            caption_text,
            padding=self.design.effective_spacing.sm,
            stroke_role="muted",
            fill_opacity=0.90,
            corner_radius=0.12,
        )
        item = self._item(key, caption_panel, 100, 0.52)
        plan = self.composition.arrange([item], region=Region.CAPTION, flow="grid", max_active=1)
        placement = plan.for_key(key)
        self._apply_placement(caption_panel, placement)
        collision = next((
            active for active in self._director_active.values()
            if active.key != key and active.placement.rect.intersects(placement.rect)
        ), None)
        if collision is not None:
            raise CompositionError(
                f"caption lane is occupied by {collision.key!r}; place stage visuals in content"
            )
        if old is None:
            self.play(FadeIn(caption_panel, shift=UP * 0.08), run_time=duration)
        else:
            self.play(ReplacementTransform(old.mobject, caption_panel), run_time=duration)
        self._register(key, caption_panel, Region.CAPTION, 100, placement)
        return caption_panel

    def focus(
        self,
        *mobjects: Mobject,
        dim_opacity: float = 0.16,
        run_time: float | None = None,
    ) -> None:
        """Dim non-focus stage objects while preserving labels and captions."""

        self._ensure_director()
        if not mobjects:
            raise CompositionError("focus needs at least one mobject")
        if not 0 <= dim_opacity <= 1:
            raise CompositionError("dim_opacity must be between 0 and 1")
        self._release_focus(animate=False)
        target_ids = {id(mobject) for mobject in mobjects}
        dimmed: list[Mobject] = []
        for active in self._director_active.values():
            if active.region in {Region.CAPTION, Region.FOOTER, Region.HEADER}:
                continue
            if id(active.mobject) in target_ids:
                continue
            active.mobject.save_state()
            dimmed.append(active.mobject)
        if dimmed:
            self.play(
                *(mobject.animate.set_opacity(dim_opacity) for mobject in dimmed),
                run_time=run_time or self.design.motion.quick,
            )
        self._director_focus_saved = dimmed

    def release_focus(self, *, run_time: float | None = None) -> None:
        self._release_focus(animate=True, run_time=run_time)

    def clear_stage(
        self,
        *,
        include_header: bool = False,
        include_caption: bool = False,
        run_time: float | None = None,
    ) -> None:
        """Clear directed content without disturbing persistent lanes by default."""

        self._ensure_director()
        self._release_focus(animate=False)
        records = self._stage_records(include_header=include_header, include_caption=include_caption)
        if records:
            self.play(
                *(FadeOut(record.mobject) for record in records),
                run_time=run_time or self.design.motion.quick,
            )
        for record in records:
            self._director_active.pop(record.key, None)

    def _position_many(
        self,
        mobjects: Sequence[Mobject],
        *,
        region: Region | str,
        flow: str,
        keys: Sequence[str] | None,
        priorities: Sequence[int] | None,
        min_scale: float,
        gap: float | None,
        max_active: int | None,
    ) -> tuple[LayoutPlan, list[_Active]]:
        resolved_keys = self._resolve_keys(mobjects, keys)
        if priorities is None:
            resolved_priorities = [50] * len(mobjects)
        else:
            resolved_priorities = [int(value) for value in priorities]
            if len(resolved_priorities) != len(mobjects):
                raise CompositionError("priorities must match the number of mobjects")
        items = [
            self._item(key, mobject, priority, min_scale)
            for key, mobject, priority in zip(resolved_keys, mobjects, resolved_priorities)
        ]
        plan = self.composition.arrange(
            items,
            region=region,
            flow=flow,
            gap=gap,
            max_active=max_active,
        )
        if plan.evicted:
            raise CompositionError(
                "composition cannot preserve legibility for: " + ", ".join(plan.evicted)
            )
        records: list[_Active] = []
        for key, mobject, priority in zip(resolved_keys, mobjects, resolved_priorities):
            placement = plan.for_key(key)
            self._apply_placement(mobject, placement)
            records.append(_Active(key, mobject, Region(region), priority, placement))
        return plan, records

    def _play_transition(
        self,
        beat: Beat,
        old: Sequence[_Active],
        new: Sequence[_Active],
        *,
        run_time: float | None,
    ) -> None:
        old_objects = [record.mobject for record in old]
        new_objects = [record.mobject for record in new]
        duration = run_time or self.design.motion.standard
        style = self.design.motion.style_for(beat.transition)

        if beat.transition is TransitionKind.CHAPTER and old_objects:
            self.play(*(FadeOut(mobject) for mobject in old_objects), run_time=self.design.motion.quick)
            old_objects = []

        if style == "morph" and old_objects:
            old_by_key = {record.key: record for record in old}
            new_by_key = {record.key: record for record in new}
            shared_keys = old_by_key.keys() & new_by_key.keys()
            animations = [
                ReplacementTransform(old_by_key[key].mobject, new_by_key[key].mobject)
                for key in shared_keys
                if old_by_key[key].mobject is not new_by_key[key].mobject
            ]
            animations.extend(
                FadeOut(record.mobject) for record in old if record.key not in new_by_key
            )
            animations.extend(
                FadeIn(record.mobject) for record in new if record.key not in old_by_key
            )
            if animations:
                self.play(*animations, run_time=duration)
            return
        if style == "lateral":
            animations = [FadeOut(mobject, shift=LEFT * 0.28) for mobject in old_objects]
            animations.extend(FadeIn(mobject, shift=RIGHT * 0.28) for mobject in new_objects)
        elif style == "draw":
            animations = [FadeOut(mobject) for mobject in old_objects]
            animations.extend(Create(mobject) for mobject in new_objects)
        elif style == "scale":
            animations = [FadeOut(mobject) for mobject in old_objects]
            animations.extend(GrowFromCenter(mobject) for mobject in new_objects)
        elif style == "hold":
            # Hold means no spatial flourish, not that stale content may remain.
            for mobject in old_objects:
                self.remove(mobject)
            for mobject in new_objects:
                self.add(mobject)
            return
        else:  # crossfade, fade, reset, and first content in a morph beat
            animations = [FadeOut(mobject) for mobject in old_objects]
            animations.extend(FadeIn(mobject) for mobject in new_objects)
        if animations:
            self.play(*animations, run_time=duration)

    def _release_focus(self, *, animate: bool, run_time: float | None = None) -> None:
        if not getattr(self, "_director_focus_saved", None):
            return
        saved = list(self._director_focus_saved)
        self._director_focus_saved = []
        if animate:
            self.play(
                *(mobject.animate.restore() for mobject in saved),
                run_time=run_time or self.design.motion.quick,
            )
        else:
            for mobject in saved:
                mobject.restore()

    def _stage_records(
        self,
        *,
        include_header: bool = False,
        include_caption: bool = False,
    ) -> list[_Active]:
        excluded = {Region.CAPTION, Region.FOOTER}
        if not include_header:
            excluded.add(Region.HEADER)
        if include_caption:
            excluded.discard(Region.CAPTION)
            excluded.discard(Region.FOOTER)
        return [record for record in self._director_active.values() if record.region not in excluded]

    def _retire(self, key: str, *, animate: bool) -> None:
        active = self._director_active.pop(key, None)
        if active is None:
            return
        if animate:
            self.play(FadeOut(active.mobject), run_time=self.design.motion.quick)
        else:
            self.remove(active.mobject)

    def _register(
        self,
        key: str,
        mobject: Mobject,
        region: Region,
        priority: int,
        placement: Placement,
    ) -> None:
        self._director_active[key] = _Active(key, mobject, region, priority, placement)

    def _item(self, key: str, mobject: Mobject, priority: int, min_scale: float) -> LayoutItem:
        return LayoutItem(
            key,
            max(0.001, float(mobject.width)),
            max(0.001, float(mobject.height)),
            priority=int(priority),
            min_scale=float(min_scale),
        )

    @staticmethod
    def _apply_placement(mobject: Mobject, placement: Placement) -> None:
        if abs(placement.scale - 1.0) > 1e-9:
            mobject.scale(placement.scale)
        mobject.move_to([placement.rect.x, placement.rect.y, 0])

    def _resolve_keys(
        self,
        mobjects: Sequence[Mobject],
        keys: Sequence[str] | None,
    ) -> tuple[str, ...]:
        if keys is None:
            return tuple(self._key(None) for _ in mobjects)
        resolved = tuple(str(key) for key in keys)
        if len(resolved) != len(mobjects):
            raise CompositionError("keys must match the number of mobjects")
        if any(not key for key in resolved) or len(set(resolved)) != len(resolved):
            raise CompositionError("mobject keys must be non-empty and unique")
        return resolved

    def _key(self, value: str | None) -> str:
        if value is not None:
            if not str(value):
                raise CompositionError("mobject key cannot be empty")
            return str(value)
        self._director_counter += 1
        return f"visual_{self._director_counter}"


class DirectedScene(CompositionMixin, Scene):
    """The default collision-safe 2D directed scene."""


class DirectedMovingCameraScene(CompositionMixin, MovingCameraScene):
    """Directed composition with Manim's movable camera frame."""


class DirectedThreeDScene(CompositionMixin, ThreeDScene):
    """Directed overlay lanes around Manim's 3D scene capabilities."""


__all__ = [
    "CompositionMixin",
    "DirectedMovingCameraScene",
    "DirectedScene",
    "DirectedThreeDScene",
]
