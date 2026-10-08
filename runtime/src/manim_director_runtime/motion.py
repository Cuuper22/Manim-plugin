"""The animations behind beat transitions, focus and morphs."""

from __future__ import annotations

from collections import Counter
from collections.abc import Collection, Iterable, Iterator, Mapping, Sequence

from manim import (
    LEFT,
    UP,
    Animation,
    AnimationGroup,
    Arrow,
    Create,
    Dot,
    FadeIn,
    FadeOut,
    GrowArrow,
    GrowFromCenter,
    LaggedStart,
    MarkupText,
    Mobject,
    ReplacementTransform,
    SingleStringMathTex,
    Succession,
    Text,
    Transform,
    TransformMatchingShapes,
    TransformMatchingTex,
    VMobject,
    Write,
)

from .beats import Transition

TRANSITION_SECONDS = {
    Transition.CONTINUE: 0.8,
    Transition.CONTRAST: 0.9,
    Transition.REVEAL: 1.3,
    Transition.CHAPTER: 0.7,
}
FOCUS_SECONDS = 0.5
STEP_SECONDS = 1.1
STEP_PAUSE = 0.7  # the shortest pause between derive steps
DIM_OPACITY = 0.28
SHOW_SECONDS = 1.0
WRITE_GLYPHS = 12  # longer text and math fade in: writing them out takes too long
_RISE = 0.15
_SLIDE = 0.6
_ORDER_LAG = 0.35
# How far into the departures the arrivals start, so outgoing and incoming objects never
# share the stage at full strength.
_LAG = {
    Transition.CONTINUE: 0.5,
    Transition.CONTRAST: 0.2,
    Transition.REVEAL: 0.6,
    Transition.CHAPTER: 0.0,
}

Opacities = dict[int, tuple[float, float]]  # id(leaf) -> (fill, stroke)


def arrive(mobject: Mobject, transition: Transition) -> Animation:
    if transition is Transition.REVEAL and isinstance(mobject, VMobject):
        return Write(mobject) if _has_fill(mobject) else Create(mobject)
    if transition is Transition.CONTRAST:
        return FadeIn(mobject, shift=LEFT * _SLIDE)
    return FadeIn(mobject, shift=UP * _RISE)


def entrance(mobject: Mobject, lag: float = 0.15) -> Animation:
    """How `show()` brings in a part or overlay: dots grow, arrows grow from their tail,
    strokes are drawn, short text and math are written, fills fade in. A component may name
    its own entrance with a `director_entrance(mobject)` attribute."""

    custom = getattr(mobject, "director_entrance", None)
    if custom is not None:
        return custom(mobject)
    leaves = mobject.family_members_with_points()
    if isinstance(mobject, Arrow):
        return GrowArrow(mobject)
    if _written(mobject):
        return Write(mobject) if len(leaves) <= WRITE_GLYPHS else FadeIn(mobject)
    if leaves and all(isinstance(leaf, Dot) for leaf in leaves):
        if len(leaves) == 1:
            return GrowFromCenter(leaves[0])
        return LaggedStart(*(GrowFromCenter(leaf) for leaf in leaves), lag_ratio=lag)
    filled = [_has_fill(leaf) for leaf in leaves]
    if not any(filled):
        return Create(mobject)
    if all(filled) or not mobject.submobjects:
        return FadeIn(mobject)
    return AnimationGroup(*(entrance(part, lag) for part in mobject.submobjects))


def depart(mobject: Mobject, transition: Transition) -> Animation:
    if transition is Transition.CONTRAST:
        return FadeOut(mobject, shift=LEFT * _SLIDE)
    return FadeOut(mobject)


def staggered(
    departures: list[Animation],
    arrivals: list[Animation],
    transition: Transition,
    *,
    waiting: Sequence[Animation] = (),
    ordered: bool = False,
) -> list[Animation]:
    """Departures, with `arrivals` starting partway into them and `waiting` (what takes the
    place of something leaving) once they are done; `ordered` arrivals come one after the
    other."""

    arriving = (
        [LaggedStart(*arrivals, lag_ratio=_ORDER_LAG)]
        if ordered and len(arrivals) > 1
        else arrivals
    )
    if not departures:
        return [*arriving, *waiting]
    leave = AnimationGroup(*departures)
    if not waiting:
        if not arriving:
            return departures
        return [AnimationGroup(leave, AnimationGroup(*arriving), lag_ratio=_LAG[transition])]
    if not arriving:
        return [Succession(leave, AnimationGroup(*waiting))]
    lag = _LAG[transition]
    # Arrivals start at `lag` into the departures; the waiting ones exactly when they end.
    after = AnimationGroup(AnimationGroup(*arriving), AnimationGroup(*waiting), lag_ratio=1 - lag)
    return [AnimationGroup(leave, after, lag_ratio=lag)]


def morph(old: Mobject, new: Mobject) -> Animation:
    """The most legible continuation from `old` to `new`. From several lines (a derivation),
    the line sharing the most terms with `new` turns into it and the rest fade, rather than
    every glyph flying to the nearest shape."""

    lines = [m for m in old.submobjects if _tex_parts(m)]
    if _tex_parts(new) and not _tex_parts(old) and len(lines) > 1:
        source = max(lines, key=lambda line: _shared_terms(line, new))
        rest = [FadeOut(m) for m in old.submobjects if m is not source]
        return AnimationGroup(TransformMatchingTex(source, new), *rest)
    if _tex_parts(old) and _tex_parts(new):
        return TransformMatchingTex(old, new)
    if _is_text(old) or _is_text(new):
        return TransformMatchingShapes(old, new)
    return ReplacementTransform(old, new)


def glide(mobject: Mobject, before: Mobject, base: Opacities | None = None) -> Animation:
    """Animate an on-screen object from its `before` snapshot to its current state (and back
    to its `base` opacities if a focus had dimmed it)."""

    target = mobject.copy()
    if base:
        _set_opacities(mobject, target, base)
    mobject.become(before)
    return Transform(mobject, target)


def opacities(mobject: Mobject) -> Opacities:
    return {
        id(leaf): (leaf.get_fill_opacity(), leaf.get_stroke_opacity())
        for leaf in mobject.family_members_with_points()
        if isinstance(leaf, VMobject)
    }


def dim(mobject: Mobject, base: Opacities, spared: Collection[int]) -> Animation | None:
    """Fade every leaf of `mobject` not in `spared` to DIM_OPACITY of its `base` opacity."""

    faded = {
        leaf: (fill * DIM_OPACITY, stroke * DIM_OPACITY)
        for leaf, (fill, stroke) in base.items()
        if leaf not in spared
    }
    if not faded:
        return None
    target = mobject.copy()
    _set_opacities(mobject, target, {**base, **faded})
    return Transform(mobject, target)


def restore(mobject: Mobject, base: Opacities) -> Animation:
    target = mobject.copy()
    _set_opacities(mobject, target, base)
    return Transform(mobject, target)


def animates(animations: Iterable[object], mobject: Mobject) -> bool:
    """Whether one of `animations` (or `.animate` builders) changes `mobject` itself."""

    return any(
        animates(animation.animations, mobject)
        if isinstance(animation, AnimationGroup)
        else getattr(animation, "mobject", None) is mobject
        for animation in animations
    )


def introduced(animations: Iterable[object]) -> list[Mobject]:
    """Mobjects that `play(*animations)` brings onto the stage itself (Write, Create, FadeIn)."""

    found: list[Mobject] = []
    for animation in animations:
        if isinstance(animation, AnimationGroup):
            found += introduced(animation.animations)
        elif isinstance(animation, Animation) and animation.is_introducer():
            found.append(animation.mobject)
    return found


def unwrapped(mobjects: Iterable[Mobject], animations: Iterable[object]) -> list[Mobject]:
    """`mobjects` with every Group that an AnimationGroup among `animations` put on stage
    replaced by its members (Manim adds that Group to the scene in their place)."""

    wrappers = {id(group) for group in _groups(animations)}

    def expand(items: Iterable[Mobject]) -> list[Mobject]:
        expanded: list[Mobject] = []
        for item in items:
            expanded += expand(item.submobjects) if id(item) in wrappers else [item]
        return expanded

    return expand(mobjects)


def _groups(animations: Iterable[object]) -> Iterator[Mobject]:
    for animation in animations:
        if isinstance(animation, AnimationGroup):
            yield animation.group
            yield from _groups(animation.animations)


def _set_opacities(source: Mobject, target: Mobject, values: Mapping[int, tuple[float, float]]):
    for original, copy in zip(source.get_family(), target.get_family(), strict=True):
        if id(original) in values:
            fill, stroke = values[id(original)]
            copy.set_fill(opacity=fill, family=False)
            copy.set_stroke(opacity=stroke, family=False)


def _has_fill(mobject: Mobject) -> bool:
    return any(
        leaf.get_fill_opacity() > 0
        for leaf in mobject.family_members_with_points()
        if isinstance(leaf, VMobject)
    )


def _tex_parts(mobject: Mobject) -> bool:
    return isinstance(mobject, SingleStringMathTex) and len(mobject.submobjects) > 1


def _shared_terms(a: Mobject, b: Mobject) -> int:
    """How many of `b`'s atoms TransformMatchingTex would carry over from `a`."""

    have = Counter(part.tex_string for part in a.submobjects)
    want = Counter(part.tex_string for part in b.submobjects)
    return sum((have & want).values())


def _written(mobject: Mobject) -> bool:
    """Text or math as a whole (a kit label is a row of both)."""

    return isinstance(mobject, Text | MarkupText | SingleStringMathTex) or hasattr(
        mobject, "authored_text"
    )


def _is_text(mobject: Mobject) -> bool:
    return any(isinstance(m, Text | MarkupText | SingleStringMathTex) for m in mobject.get_family())
