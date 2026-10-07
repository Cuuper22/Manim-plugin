"""Find the glyphs of a TeX sub-term inside a rendered MathTex or Tex.

The source is compiled once more with each occurrence of the term painted a marker color.
Glyphs keep their TeX colors, so the marked glyphs of that probe say where the term sits;
they are matched to the real glyphs by position within the expression.
"""

from __future__ import annotations

from manim import MathTex, SingleStringMathTex, Tex, VGroup, VMobject

from .errors import CompositionError
from .texscan import occurrences, paint

_PLAIN = "#000001"  # unpainted probe glyphs; any color the markers never use


def _marker(index: int) -> str:
    return f"#FE{index // 256:02X}{index % 256:02X}"


def term_glyphs(
    mobject: SingleStringMathTex, source: str, term: str, occurrence: int | None = None
) -> VGroup:
    text_mode = isinstance(mobject, Tex)
    spans = occurrences(source, term, math_only=text_mode)
    if not spans:
        raise CompositionError(
            f"{term!r} does not occur in {source!r}; write the term as it appears in the source.",
            term=term,
            source=source,
        )
    if occurrence is not None:
        if not 0 <= occurrence < len(spans):
            raise CompositionError(
                f"{term!r} occurs {len(spans)} time(s) in {source!r}; occurrence {occurrence} "
                "is out of range.",
                term=term,
                occurrences=len(spans),
            )
        spans = [spans[occurrence]]
    probe_type = Tex if text_mode else MathTex
    probe = probe_type(
        paint(source, [(span, _marker(i)) for i, span in enumerate(spans)]),
        tex_environment=mobject.tex_environment,
        tex_template=mobject.tex_template,
        color=_PLAIN,
    )
    glyphs = mobject.family_members_with_points()
    marks = probe.family_members_with_points()
    if len(glyphs) != len(marks):
        raise CompositionError(
            f"Cannot locate {term!r}: {mobject!r} no longer has the glyphs of its source.",
            term=term,
        )
    markers = {_marker(i) for i in range(len(spans))}
    wanted = [i for i, mark in enumerate(marks) if mark.get_color().to_hex() in markers]
    return VGroup(*(glyphs[i] for i in _match(marks, glyphs, wanted)))


def _match(marks: list[VMobject], glyphs: list[VMobject], wanted: list[int]) -> list[int]:
    """Indices of the glyphs nearest to the wanted probe glyphs, in normalized positions."""

    probe_points, glyph_points = _normalized(marks), _normalized(glyphs)
    taken: set[int] = set()
    chosen = []
    for index in wanted:
        x, y = probe_points[index]
        best = min(
            (j for j in range(len(glyphs)) if j not in taken),
            key=lambda j: (glyph_points[j][0] - x) ** 2 + (glyph_points[j][1] - y) ** 2,
        )
        taken.add(best)
        chosen.append(best)
    return sorted(chosen)


def _normalized(glyphs: list[VMobject]) -> list[tuple[float, float]]:
    group = VGroup(*glyphs)
    left, bottom = group.get_left()[0], group.get_bottom()[1]
    width, height = max(group.width, 1e-9), max(group.height, 1e-9)
    return [
        ((g.get_center()[0] - left) / width, (g.get_center()[1] - bottom) / height) for g in glyphs
    ]
