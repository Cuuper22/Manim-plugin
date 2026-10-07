"""DirectedScene: Manim scenes with a theme, layout regions, beats and math-first helpers.

Everything here is plain Manim underneath: helpers return ordinary mobjects, and staged
objects simply enter at the next animation, so arbitrary Manim code mixes in freely.
"""

from __future__ import annotations

import inspect
import sys
import weakref
from collections.abc import Callable, Iterable, Mapping, Sequence
from pathlib import Path
from typing import Any

import numpy as np
from manim import (
    DOWN,
    LEFT,
    Animation,
    AnimationGroup,
    AnnotationDot,
    AnnularSector,
    Annulus,
    Arrow3D,
    Circumscribe,
    Dot,
    Dot3D,
    FadeIn,
    FadeOut,
    FadeToColor,
    Flash,
    FunctionGraph,
    Indicate,
    ManimColor,
    MarkupText,
    MathTex,
    Mobject,
    MovingCameraScene,
    Rectangle,
    RoundedRectangle,
    Scene,
    SingleStringMathTex,
    SurroundingRectangle,
    Tex,
    Text,
    ThreeDScene,
    TracedPath,
    TransformMatchingTex,
    ValueTracker,
    VGroup,
    VMobject,
    Write,
    config,
)
from manim.constants import RendererType

from . import motion, timeline
from .beats import Beat, Intent, Transition
from .derivation import Derivation, overlay, stack
from .errors import CompositionError, parse_choice
from .layout import LANES, Rect, Region, frame_regions
from .project import load_style
from .staging import (
    Stage,
    bounds,
    describe,
    on_stage,
    parts_inside,
    parts_outside,
    plan,
    split_by_stage,
    within,
)
from .terms import term_glyphs
from .texscan import atoms, colorize
from .themes import MATH_FONT_SIZE, TEXT_STYLES, Role, Theme, default_theme, theme

MIN_SCALE = 0.5
SPACING = 0.4


class Directed:
    """Mixin behind DirectedScene; combine it with other Scene bases (`Directed, ZoomedScene`)."""

    theme: Any = None
    """A theme name or Theme. None: the nearest director.yaml's theme, else the default.
    During a render this attribute holds the resolved Theme (`self.theme.primary`)."""

    symbols: Mapping[str, str | ManimColor] = {}
    """TeX symbol -> color token, #RRGGBB or Manim color, merged over director.yaml
    `direction.symbols`."""

    # Lifecycle ------------------------------------------------------------------------------

    def render(self, preview: bool = False) -> Any:
        self._direct()
        try:
            return super().render(preview)  # type: ignore[misc]
        finally:
            self._restore_defaults()

    def tear_down(self) -> None:
        self._flush(instant=True)
        super().tear_down()  # type: ignore[misc]

    def play(self, *args: Any, **kwargs: Any) -> None:
        self._flush(introduced=motion.introduced(args))
        super().play(*args, **kwargs)  # type: ignore[misc]
        self._unwrap(args)

    def _direct(self) -> None:
        _require_matching_shape()
        style = load_style(_scene_directory(type(self)))
        self.theme = _resolve_theme(type(self).theme, style.theme)
        symbols = {**style.symbols, **type(self).symbols}
        self._symbol_colors = {tex: self.theme.color(value) for tex, value in symbols.items()}
        self._regions = frame_regions(config.frame_width, config.frame_height, style.safe_area)
        self._stage = Stage()
        self._beat: Beat | None = None
        self._beats_entered = 0
        self._tags = 0
        self._sources: weakref.WeakKeyDictionary[Mobject, str] = weakref.WeakKeyDictionary()
        if config.renderer == RendererType.OPENGL:
            self.renderer.background_color = self.theme.background
        else:
            self.camera.background_color = self.theme.background
        self._restore_defaults = _apply_theme_defaults(self.theme)
        recorder = timeline.active()
        if recorder is not None:
            recorder.attach()

    # Content --------------------------------------------------------------------------------

    def text(self, text: str, role: Role | str = Role.BODY, **kwargs: Any) -> Text:
        style = TEXT_STYLES[parse_choice(Role, role)]
        options = {
            "font": self.theme.font,
            "font_size": style.font_size,
            "weight": style.weight,
            "color": self.theme.color(style.color),
            "warn_missing_font": False,
        }
        return Text(text, **{**options, **kwargs})

    def tex(self, *strings: str, role: Role | str = Role.BODY, **kwargs: Any) -> Tex:
        """Text-mode LaTeX; symbols inside `$...$` get their colors."""

        style = TEXT_STYLES[parse_choice(Role, role)]
        pieces = [colorize(s, self._symbol_colors, math_only=True) for s in strings]
        options = {"font_size": style.font_size, "color": self.theme.color(style.color)}
        mobject = Tex(*pieces, **{**options, **kwargs})
        self._sources[mobject] = mobject.arg_separator.join(strings)
        return mobject

    def math(self, *strings: str, **kwargs: Any) -> MathTex:
        """MathTex with symbol colors, split into atoms so TransformMatchingTex can match them."""

        pieces = [colorize(s, self._symbol_colors) for s in strings]
        if len(pieces) == 1:
            pieces = atoms(pieces[0])
            kwargs.setdefault("arg_separator", "")
        options = {"font_size": MATH_FONT_SIZE, "color": self.theme.foreground}
        mobject = MathTex(*pieces, **{**options, **kwargs})
        source = strings[0] if len(strings) == 1 else mobject.arg_separator.join(strings)
        self._sources[mobject] = source
        return mobject

    def title(self, text: str, **kwargs: Any) -> Text:
        """Set the title lane; a previous title cross-fades into this one."""

        self._retire(self._occupant(Region.HEADER))
        mobject = self.text(text, Role.TITLE, **kwargs)
        self.place(mobject, region=Region.HEADER)
        return mobject

    def caption(self, text: str | None, **kwargs: Any) -> Mobject | None:
        """Set the caption lane (at most two lines), or clear it with None."""

        self._retire(self._occupant(Region.CAPTION))
        if text is None:
            return None
        mobject = self._caption_lines(" ".join(text.split()), **kwargs)
        self.place(mobject, region=Region.CAPTION)
        return mobject

    def region(self, region: Region | str) -> Rect:
        return self._regions[parse_choice(Region, region)]

    def place(
        self,
        *mobjects: Mobject,
        region: Region | str = Region.CONTENT,
        anchor: Sequence[float] | None = None,
        direction: Sequence[float] = DOWN,
        buff: float = SPACING,
        replaces: Mobject | None = None,
        min_scale: float = MIN_SCALE,
    ) -> Mobject:
        """Fit mobjects into a region (arranged along `direction`); they enter at the next
        animation. Parts already on stage glide there instead, so a new VGroup can gather
        on-stage objects and new ones; `replaces` morphs an on-stage object into this one.
        Nothing moves unless the placement is valid."""

        if not mobjects:
            raise CompositionError("place() needs at least one mobject.")
        area_name = parse_choice(Region, region)
        if replaces is not None:
            if len(mobjects) != 1:
                raise CompositionError("replaces= morphs one object; place the others separately.")
            if not self._visible(replaces) and replaces not in self._stage.entering:
                raise CompositionError(f"replaces={describe(replaces)} is not on stage.")
        layout = plan(
            mobjects,
            area_name,
            self._regions[area_name],
            anchor=_anchor(anchor),
            axis=_axis(direction),
            buff=buff,
            min_scale=min_scale,
        )
        self._require_free(layout.bounds, area_name, ignore=[*mobjects, replaces])
        visible = self._visible_ids()
        for mobject, center in zip(mobjects, layout.centers, strict=True):
            staged, new = split_by_stage(mobject, visible)
            for part in staged:
                self._stage.glides.setdefault(id(part), (part, part.copy()))
                self._carry(part)
            if replaces is None:
                self._stage.entering += [m for m in new if m not in self._stage.entering]
            mobject.scale(layout.scale).move_to(center)
            for member in mobject.get_family()[1:]:  # the group now owns their placement
                self._stage.placed.pop(id(member), None)
            self._stage.place(mobject, area_name)
            self._pin(mobject)
        if replaces is not None:
            self._stage.placed.pop(id(replaces), None)
            if replaces in self._stage.entering:
                self._stage.entering.remove(replaces)
                self._stage.entering.append(mobjects[0])
            else:
                self._stage.morphs.append((replaces, mobjects[0]))
                self._carry(replaces)
        return mobjects[0] if len(mobjects) == 1 else VGroup(*mobjects)

    # Beats and focus ------------------------------------------------------------------------

    def beat(
        self,
        id: str | None = None,
        *,
        focus: Mobject | None = None,
        transition: Transition | str = Transition.CONTINUE,
        keep: Iterable[Mobject] = (),
        hold: float = motion.HOLD_SECONDS,
        run_time: float | None = None,
        intent: Intent | str | None = None,
        question: str | None = None,
        takeaway: str | None = None,
    ) -> Beat:
        """A named stage change: `with self.beat("hook", focus=row): ...`.

        On entry nothing moves. At the first animation inside (or at the end) everything on
        stage that was not kept or placed again leaves, carried objects glide or morph, and
        newly placed objects enter, all styled by `transition`. On exit, `focus` is
        emphasized and the beat holds for `hold` seconds. `intent`, `question` and
        `takeaway` are narrative notes kept on the Beat.
        """

        if hold < 0 or (run_time is not None and run_time < 0):
            raise CompositionError("A beat's hold and run_time cannot be negative.")
        caller = sys._getframe(1)
        return Beat(
            id=id,
            transition=parse_choice(Transition, transition),
            focus=focus,
            keep=tuple(keep),
            hold=hold,
            run_time=run_time,
            intent=None if intent is None else parse_choice(Intent, intent),
            question=question,
            takeaway=takeaway,
            scene=self,
            file=caller.f_code.co_filename,
            line=caller.f_lineno,
        )

    def focus(self, *mobjects: Mobject, run_time: float | None = None) -> None:
        """Dim everything on stage except `mobjects` (title and caption stay lit)."""

        if not mobjects:
            raise CompositionError("focus() needs at least one mobject.")
        self._flush()
        missing = [describe(m) for m in mobjects if not self._visible(m)]
        if missing:
            raise CompositionError(f"Cannot focus {', '.join(missing)}: not on stage.")
        spared = {id(leaf) for m in mobjects for leaf in m.get_family()}
        animations = []
        for top in self.mobjects:
            if self._stage.region_of(top) in LANES or self._backstage(top):
                continue
            _, base = self._stage.dimmed.get(id(top), (top, motion.opacities(top)))
            animation = motion.dim(top, base, spared)
            if animation is not None:
                self._stage.dimmed[id(top)] = (top, base)
            elif self._stage.dimmed.pop(id(top), None):
                animation = motion.restore(top, base)
            if animation is not None:
                animations.append(animation)
        self._perform(animations, motion.FOCUS_SECONDS if run_time is None else run_time)

    def unfocus(self, run_time: float | None = None) -> None:
        self._flush()
        animations = [
            motion.restore(top, base)
            for top, base in self._stage.dimmed.values()
            if self._visible(top)
        ]
        self._stage.dimmed.clear()
        self._perform(animations, motion.FOCUS_SECONDS if run_time is None else run_time)

    # Math -----------------------------------------------------------------------------------

    def derive(
        self,
        *steps: str | MathTex | tuple[str | MathTex, str],
        region: Region | str = Region.CONTENT,
        in_place: bool = False,
        run_time: float | None = None,
        pause: float | None = None,
        replaces: Mobject | None = None,
        min_scale: float = MIN_SCALE,
    ) -> Derivation:
        """Play a derivation, one step at a time, with matching terms carried between steps.

        Steps are TeX strings (or MathTex), optionally paired with a note: `(r"= x^2", "expand")`.
        Lines stack with their relations aligned; `in_place=True` transforms one line instead.
        `replaces` morphs an on-stage expression into the first step instead of writing it.
        """

        if not steps:
            raise CompositionError("derive() needs at least one step.")
        if replaces is not None and not self._visible(replaces):
            raise CompositionError(f"replaces={describe(replaces)} is not on stage.")
        lines, notes = [], []
        for step in steps:
            tex, note = step if isinstance(step, tuple) else (step, None)
            line = self.math(tex) if isinstance(tex, str) else tex
            if not isinstance(line, SingleStringMathTex):
                raise CompositionError(
                    "derive() steps are TeX strings or MathTex, optionally paired with a note "
                    f"as (tex, note); got {describe(tex)}."
                )
            lines.append(line)
            notes.append(None if note is None else self.text(note, Role.LABEL))
        (overlay if in_place else stack)(lines, notes)
        block = Derivation(lines, notes)
        area_name = parse_choice(Region, region)
        layout = plan(
            [block],
            area_name,
            self._regions[area_name],
            anchor=(0.0, 0.0),
            axis=(0, -1),
            buff=0.0,
            min_scale=min_scale,
        )
        self._require_free(layout.bounds, area_name, ignore=[replaces])
        block.scale(layout.scale).move_to(layout.centers[0])
        self._pin(block)
        seconds = motion.STEP_SECONDS if run_time is None else run_time
        rest = motion.STEP_PAUSE if pause is None else pause
        if replaces is None:
            first = Write(lines[0])
        else:
            self._carry(replaces)
            self._stage.placed.pop(id(replaces), None)
            first = motion.morph(replaces, lines[0])
        self._flush(along=[first, *_fade_in(notes[0])], run_time=seconds)
        for i in range(1, len(lines)):
            if rest > 0:
                self.wait(rest)
            source = lines[i - 1] if in_place else lines[i - 1].copy()
            animations = [TransformMatchingTex(source, lines[i]), *_fade_in(notes[i])]
            if in_place and notes[i - 1] is not None:
                animations.append(FadeOut(notes[i - 1]))
            self._perform(animations, seconds)
        self.remove(*block.submobjects)
        if in_place:
            block.remove(*(m for m in block.submobjects if m not in (lines[-1], notes[-1])))
        self.add(block)
        self._stage.place(block, area_name)
        return block

    def term(self, mobject: Mobject, tex: str, *, occurrence: int | None = None) -> VGroup:
        """The glyphs of `tex` inside a MathTex or Tex, e.g. `self.term(eq, r"\\frac{b}{2a}")`."""

        if not isinstance(mobject, SingleStringMathTex):
            raise CompositionError(
                f"term() looks inside MathTex or Tex; {describe(mobject)} is neither."
            )
        source = self._sources.get(mobject, mobject.tex_string)
        return term_glyphs(mobject, source, tex, occurrence)

    def highlight(
        self,
        mobject: Mobject,
        *terms: str,
        color: str | ManimColor = "accent",
        box: bool = False,
        run_time: float | None = None,
    ) -> VGroup:
        """Recolor sub-terms (or the whole mobject) and optionally back them with a soft box,
        like a highlighter pen. Returns the highlighted glyphs."""

        glyphs = (
            VGroup(*(glyph for tex in terms for glyph in self.term(mobject, tex)))
            if terms
            else VGroup(*mobject.family_members_with_points())
        )
        hue = self.theme.color(color)
        animations: list[Animation] = [FadeToColor(glyph, hue) for glyph in glyphs]
        if box:
            # Tight sideways so neighbouring operators keep their space; taller like a marker.
            pad_x, pad_y = 0.05, 0.05 + 0.12 * glyphs.height
            backdrop = RoundedRectangle(
                width=glyphs.width + 2 * pad_x,
                height=glyphs.height + 2 * pad_y,
                corner_radius=0.08,
                stroke_width=0,
                fill_color=hue,
                fill_opacity=0.16,
            ).move_to(glyphs)
            backdrop.set_z_index(mobject.z_index - 1)
            if self._pinned(mobject):
                self._pin(backdrop)
            animations.append(FadeIn(backdrop))
        self._flush()
        self._perform(animations, motion.FOCUS_SECONDS if run_time is None else run_time)
        return glyphs

    def tag(self, mobject: Mobject, label: str | None = None) -> MathTex:
        """An equation tag at the right edge of the content region, following `mobject`."""

        if label is None:
            self._tags += 1
            label = f"({self._tags})"
        mark = MathTex(label, font_size=MATH_FONT_SIZE * 0.72, color=self.theme.muted)
        area = self._regions[Region.CONTENT]
        mark.move_to((area.right - mark.width / 2, mobject.get_center()[1], 0))
        if bounds(mark).overlaps(bounds(mobject)):
            raise CompositionError(
                f"Tag {label} would overlap {describe(mobject)}; place it narrower first.",
                label=label,
            )
        mark.add_updater(lambda m: m.set_y(mobject.get_center()[1]))
        self._stage.attached[id(mark)] = mobject
        self._stage.entering.append(mark)
        self._stage.place(mark, Region.CONTENT)
        self._pin(mark)
        return mark

    # Internals ------------------------------------------------------------------------------

    def _enter_beat(self, beat: Beat) -> None:
        if self._beat is not None:
            raise CompositionError(
                f"Beat {beat.id or 'unnamed'!r} starts inside beat {self._beat.id!r}; beats do "
                "not nest. End the first beat before starting the next.",
                beat=beat.id,
                open=self._beat.id,
            )
        self._beats_entered += 1
        beat.id = beat.id or f"beat-{self._beats_entered}"
        chapter = beat.transition is Transition.CHAPTER
        # Manim's wait() leaves empty Mobjects behind; only visible content takes part.
        beat.before = [
            m
            for m in self.mobjects
            if m.family_members_with_points()
            and not self._backstage(m)
            and (chapter or self._stage.region_of(m) not in LANES)
        ]
        beat.carried = {id(leaf) for m in beat.keep for leaf in m.get_family()}
        self.next_section(beat.id)
        recorder = timeline.active()
        if recorder is not None:
            beat.record = recorder.enter(beat.id, beat.file, beat.line, self.renderer.time)
        self._beat = beat

    def _exit_beat(self, beat: Beat, *, completed: bool) -> None:
        try:
            if completed:
                self._flush()
                if beat.focus is not None and not beat.focused:
                    raise CompositionError(
                        f"Beat {beat.id!r} focuses {describe(beat.focus)}, which never reached "
                        "the stage; place it, play it in, or keep it.",
                        beat=beat.id,
                    )
                if beat.hold > 0:
                    self.wait(beat.hold)
        finally:
            self._beat = None
            recorder = timeline.active()
            if recorder is not None and beat.record is not None:
                recorder.exit(beat.record, self.renderer.time)

    def _flush(
        self,
        introduced: Sequence[Mobject] = (),
        *,
        instant: bool = False,
        along: Sequence[Animation] = (),
        run_time: float | None = None,
    ) -> None:
        """Play everything staged since the last animation as one transition, together with
        `along` (animations that should not wait for it)."""

        stage, beat = self._stage, self._beat
        starting = beat is not None and not beat.transitioned
        focusing = beat is not None and beat.focus is not None and not beat.focused
        if not (starting or focusing or along or stage.pending()):
            return
        transition = beat.transition if beat is not None else Transition.CONTINUE
        visible = self._visible_ids()
        stage.prune(visible)
        restores: list[Animation] = []
        if starting:
            beat.transitioned = True
            for m in beat.before:
                if id(m) in visible and not self._carried(m, beat):
                    stage.leaving += parts_outside(m, beat.carried)
            restores = self._release_dimmed(set(map(id, stage.leaving)))
        covered = {id(leaf) for m in introduced for leaf in m.get_family()}
        entering = [m for m in stage.entering if id(m) not in visible | covered]
        leaving, morphs, glides = stage.leaving, stage.morphs, list(stage.glides.values())
        stage.entering, stage.leaving, stage.morphs, stage.glides = [], [], [], {}
        seconds = motion.TRANSITION_SECONDS[transition]
        if beat is not None and beat.run_time is not None:
            seconds = beat.run_time
        if run_time is not None:
            seconds = run_time
        if instant:
            seconds = 0.0
        if transition is Transition.CHAPTER and (leaving or morphs):
            self._perform(
                [motion.depart(m, transition) for m in [*leaving, *(old for old, _ in morphs)]],
                seconds,
            )
            leaving, entering, morphs = [], [new for _, new in morphs] + entering, []
        changes = [
            *(motion.morph(old, new) for old, new in morphs),
            *(motion.glide(m, before, self._dim_base(m)) for m, before in glides),
            *(motion.arrive(m, transition) for m in entering),
            *along,
        ]
        departures = [*(motion.depart(m, transition) for m in leaving), *restores]
        self._perform(motion.staggered(departures, changes, transition), seconds)
        focus = None if beat is None or beat.focused else beat.focus
        if focus is not None and self._visible(focus):
            beat.focused = True
            self.focus(focus, run_time=0.0 if seconds == 0 else None)

    def _perform(self, animations: Sequence[Animation], run_time: float) -> None:
        if not animations:
            return
        if run_time > 0:
            super().play(*animations, run_time=run_time)  # type: ignore[misc]
            self._unwrap(animations)
            return
        for animation in animations:  # run_time 0: land on the end state without frames
            animation._setup_scene(self)
            animation.begin()
            animation.finish()
            animation.clean_up_from_scene(self)

    def _unwrap(self, animations: Sequence[object]) -> None:
        """Beats track what was played, not the Groups Manim leaves in its place."""

        self.mobjects = motion.unwrapped(self.mobjects, animations)

    def _release_dimmed(self, leaving: set[int]) -> list[Animation]:
        """Restore what the previous focus dimmed, except what is leaving or gliding."""

        dimmed = self._stage.dimmed
        gliding = set(self._stage.glides)
        # Gliding objects get their opacity back as part of the glide (see _dim_base).
        self._stage.dimmed = {k: v for k, v in dimmed.items() if k in gliding}
        return [
            motion.restore(top, base)
            for key, (top, base) in dimmed.items()
            if key not in leaving and key not in gliding
        ]

    def _dim_base(self, mobject: Mobject) -> dict[int, tuple[float, float]] | None:
        entry = self._stage.dimmed.pop(id(mobject), None)
        return entry[1] if entry else None

    def _retire(self, mobject: Mobject | None) -> None:
        if mobject is None:
            return
        self._stage.placed.pop(id(mobject), None)
        if mobject in self._stage.entering:
            self._stage.entering.remove(mobject)
        else:
            self._stage.leaving.append(mobject)

    def _carry(self, mobject: Mobject) -> None:
        if self._beat is not None and not self._beat.transitioned:
            self._beat.carried.update(id(leaf) for leaf in mobject.get_family())

    def _carried(self, mobject: Mobject, beat: Beat) -> bool:
        """Carried as a whole; a group carried only in part leaves its other parts behind."""

        if id(mobject) in beat.carried:
            return True
        parent = self._stage.attached.get(id(mobject))
        return parent is not None and self._carried(parent, beat)

    def _staying(self, mobject: Mobject) -> list[Mobject]:
        """The parts of an on-stage object that outlive the beat being set up."""

        beat = self._beat
        if within(mobject, self._stage.leaving):
            return []
        if (
            beat is None
            or beat.transitioned
            or not within(mobject, beat.before)
            or self._carried(mobject, beat)
        ):
            return [mobject]
        return parts_inside(mobject, beat.carried)

    def _leaving(self, mobject: Mobject) -> bool:
        return not self._staying(mobject)

    def _require_free(self, area: Rect, region: Region, ignore: Sequence[Mobject | None]) -> None:
        skipped = {id(member) for m in ignore if m is not None for member in m.get_family()}
        attached = self._stage.attached.items()
        skipped |= {child for child, parent in attached if id(parent) in skipped}
        visible = self._visible_ids()
        for mobject, _ in self._stage.placed.values():
            if id(mobject) in skipped:
                continue
            if id(mobject) not in visible and mobject not in self._stage.entering:
                continue
            for part in self._staying(mobject):
                if id(part) not in skipped and area.overlaps(bounds(part)):
                    raise CompositionError(
                        f"This placement in the {region} region would overlap {describe(part)}. "
                        "Place both in one call (self.place(a, b)), use another region, or let "
                        "the next beat retire it.",
                        region=region.value,
                    )

    def _occupant(self, region: Region) -> Mobject | None:
        visible = self._visible_ids()
        candidates = [
            m
            for m in self._stage.occupants(region)
            if (id(m) in visible or m in self._stage.entering) and not self._leaving(m)
        ]
        return candidates[-1] if candidates else None

    def _caption_lines(self, text: str, **kwargs: Any) -> Mobject:
        line = self.text(text, Role.CAPTION, **kwargs)
        if line.width <= self._regions[Region.CAPTION].width or " " not in text:
            return line
        middle = len(text) / 2
        cut = min((i for i, ch in enumerate(text) if ch == " "), key=lambda i: abs(i - middle))
        halves = (text[:cut], text[cut + 1 :])
        return VGroup(*(self.text(half, Role.CAPTION, **kwargs) for half in halves)).arrange(
            DOWN, buff=0.14
        )

    def _backstage(self, mobject: Mobject) -> bool:
        """Scene machinery that Manim keeps among the mobjects but that is not content: a
        moving camera's frame, a ZoomedScene's display, value trackers (their value is a
        point, so a transition would shift it)."""

        zoomed = getattr(self, "zoomed_camera", None)
        machinery = (
            getattr(self.camera, "frame", None),
            getattr(zoomed, "frame", None),
            getattr(self, "zoomed_display", None),
        )
        return isinstance(mobject, ValueTracker) or any(mobject is m for m in machinery)

    def _visible_ids(self) -> set[int]:
        return {id(m) for m in self.get_mobject_family_members()}

    def _visible(self, mobject: Mobject) -> bool:
        return on_stage(mobject, self._visible_ids())

    def _pin(self, mobject: Mobject) -> None:
        """Hook for scenes whose camera moves: hold what should stay put on screen."""

    def _pinned(self, mobject: Mobject) -> bool:
        return False


class DirectedScene(Directed, Scene):
    pass


class DirectedMovingCameraScene(Directed, MovingCameraScene):
    """Title and caption stay put on screen while the camera moves; placed content does not."""

    def _pin(self, mobject: Mobject) -> None:
        if self._stage.region_of(mobject) in LANES:
            _hold_on_screen(mobject, self.camera.frame)


class DirectedThreeDScene(Directed, ThreeDScene):
    """Placed objects are screen-space overlays, fixed in frame while the camera moves."""

    def _pin(self, mobject: Mobject) -> None:
        if config.renderer == RendererType.OPENGL:
            mobject.fix_in_frame()
        else:
            self.renderer.camera.add_fixed_in_frame_mobjects(mobject)

    def _pinned(self, mobject: Mobject) -> bool:
        if config.renderer == RendererType.OPENGL:
            return bool(getattr(mobject, "is_fixed_in_frame", False))
        return mobject in self.renderer.camera.fixed_in_frame_mobjects

    def begin_animations(self) -> None:
        """Transforms of overlays (derive steps, morphs) draw copies that Manim makes in
        begin(); pin those too, or the camera projects them into the 3D world. World objects
        stay as they are."""

        super().begin_animations()
        pending = [(animation, False) for animation in self.animations or []]
        while pending:
            animation, overlay = pending.pop()
            target = getattr(animation, "to_add", None)
            target = target or getattr(animation, "target_mobject", None)
            # A plain group's own mobject gathers all its members, overlays and world alike.
            group = isinstance(animation, AnimationGroup)
            ends = [target] if group else [animation.mobject, target]
            overlay = overlay or any(
                self._pinned(m) for end in ends if end is not None for m in end.get_family()
            )
            if overlay:
                self._pin(animation.mobject)
            if group:  # members may have put their own mobjects on stage (FadeIn)
                pending += [(member, overlay) for member in animation.animations]


def _hold_on_screen(mobject: Mobject, frame: Mobject) -> None:
    """Keep `mobject` where it sits in the unmoved frame, however the camera pans or zooms."""

    offset, width = mobject.get_center(), mobject.width

    def follow(m: Mobject) -> None:
        zoom = frame.width / config.frame_width
        m.scale(width * zoom / m.width).move_to(frame.get_center() + offset * zoom)

    follow(mobject)
    mobject.add_updater(follow)


def _fade_in(mobject: Mobject | None) -> list[Animation]:
    return [] if mobject is None else [FadeIn(mobject, shift=LEFT * 0.2)]


def _anchor(anchor: Sequence[float] | None) -> tuple[float, float]:
    if anchor is None:
        return (0.0, 0.0)
    x, y = float(anchor[0]), float(anchor[1])
    return (float(np.clip(x, -1, 1)), float(np.clip(y, -1, 1)))


def _axis(direction: Sequence[float]) -> tuple[int, int]:
    x, y = float(direction[0]), float(direction[1])
    if (x == 0) == (y == 0):
        raise CompositionError("direction must be UP, DOWN, LEFT or RIGHT.")
    return (int(np.sign(x)), 0) if x else (0, int(np.sign(y)))


def _scene_directory(scene_class: type) -> Path:
    try:
        return Path(inspect.getfile(scene_class)).resolve().parent
    except (TypeError, OSError):
        return Path.cwd()


def _require_matching_shape() -> None:
    frame = config.frame_width / config.frame_height
    pixels = config.pixel_width / config.pixel_height
    if abs(frame / pixels - 1) > 0.01:
        raise CompositionError(
            f"The frame is {config.frame_width:.2f} x {config.frame_height:.2f} units but the "
            f"video is {config.pixel_width} x {config.pixel_height} pixels, so it would come out "
            "squashed. Manim takes the frame's shape from manim.cfg, not from -q or -r: set "
            "pixel_width and pixel_height there, or pass -r with the same shape.",
            frame=[config.frame_width, config.frame_height],
            pixels=[config.pixel_width, config.pixel_height],
        )


def _resolve_theme(declared: str | Theme | None, from_project: str | None) -> Theme:
    if isinstance(declared, Theme):
        return declared
    name = declared if declared is not None else from_project
    return default_theme() if name is None else theme(name)


def _apply_theme_defaults(active: Theme) -> Callable[[], None]:
    """Make plain Manim objects match the theme for the length of one render: the classes
    below hard-code white or yellow, which vanish on a light theme."""

    text = {"font": active.font, "warn_missing_font": False}
    ink, highlight = {"color": active.foreground}, {"color": active.accent}
    defaults: list[tuple[type, dict[str, Any]]] = [
        (VMobject, ink),
        (Text, text),
        (MarkupText, text),
        *((cls, ink) for cls in (Dot, Dot3D, Rectangle, Annulus, AnnularSector, Arrow3D)),
        *((cls, {"stroke_color": active.foreground}) for cls in (AnnotationDot, TracedPath)),
        *((cls, highlight) for cls in (SurroundingRectangle, Indicate, Flash, Circumscribe)),
        (FunctionGraph, {"color": active.primary}),
    ]
    saved = {cls: vars(cls)["__init__"] for cls, _ in defaults}
    for cls, values in defaults:
        cls.set_default(**values)
    # BackgroundRectangle (and add_background_rectangle()) fill with the config's color.
    background, config.background_color = config.background_color, active.background

    def restore() -> None:
        for cls, init in saved.items():
            cls.__init__ = init
        config.background_color = background

    return restore
