"""The scene a component is built for (R1): its theme, symbol colors and typesetting.

Components are usually built in `construct` before they are placed, so they cannot take the
theme from a scene method. DirectedScene sets a context variable for the length of a render;
outside one, components get the default theme and no symbol colors.
"""

from __future__ import annotations

from collections.abc import Mapping
from contextvars import ContextVar, Token
from typing import TYPE_CHECKING, Any

from manim import MathTex, Tex, Text

from ..errors import parse_choice
from ..texscan import atoms, colorize
from ..themes import MATH_FONT_SIZE, TEXT_STYLES, Role, Theme, default_theme

if TYPE_CHECKING:
    from ..scene import Directed

_SCENE: ContextVar[Directed | None] = ContextVar("manim_director_scene", default=None)


def enter(scene: Directed) -> Token[Directed | None]:
    return _SCENE.set(scene)


def leave(token: Token[Directed | None]) -> None:
    _SCENE.reset(token)


def scene() -> Directed | None:
    """The DirectedScene being rendered, if any."""

    return _SCENE.get()


def theme() -> Theme:
    active = _SCENE.get()
    return default_theme() if active is None else active.theme


def symbol_colors() -> Mapping[str, str]:
    active = _SCENE.get()
    return {} if active is None else active._symbol_colors


def color(value: Any) -> str:
    """A theme token, `#RRGGBB` or Manim color as `#RRGGBB`, in the active theme."""

    return theme().color(value)


def text(content: str, role: Role | str = Role.BODY, **kwargs: Any) -> Text:
    return typeset_text(theme(), content, role, **kwargs)


def tex(*strings: str, role: Role | str = Role.BODY, **kwargs: Any) -> Tex:
    return typeset_tex(theme(), symbol_colors(), *strings, role=role, **kwargs)


def math(*strings: str, **kwargs: Any) -> MathTex:
    return typeset_math(theme(), symbol_colors(), *strings, **kwargs)


def typeset_text(active: Theme, content: str, role: Role | str, **kwargs: Any) -> Text:
    style = TEXT_STYLES[parse_choice(Role, role)]
    options = {
        "font": active.font,
        "font_size": style.font_size,
        "weight": style.weight,
        "color": active.color(style.color),
        "warn_missing_font": False,
    }
    return Text(content, **{**options, **kwargs})


def typeset_tex(
    active: Theme, symbols: Mapping[str, str], *strings: str, role: Role | str, **kwargs: Any
) -> Tex:
    """Text-mode LaTeX; symbols inside `$...$` get their colors."""

    style = TEXT_STYLES[parse_choice(Role, role)]
    pieces = [colorize(s, symbols, math_only=True) for s in strings]
    options = {"font_size": style.font_size, "color": active.color(style.color)}
    mobject = Tex(*pieces, **{**options, **kwargs})
    mobject.authored_tex = mobject.arg_separator.join(strings)  # copies keep it
    return mobject


def typeset_math(
    active: Theme, symbols: Mapping[str, str], *strings: str, **kwargs: Any
) -> MathTex:
    """MathTex with symbol colors, split into atoms so TransformMatchingTex can match them."""

    pieces = [colorize(s, symbols) for s in strings]
    if len(pieces) == 1:
        pieces = atoms(pieces[0])
        kwargs.setdefault("arg_separator", "")
    options = {"font_size": MATH_FONT_SIZE, "color": active.foreground}
    mobject = MathTex(*pieces, **{**options, **kwargs})
    source = strings[0] if len(strings) == 1 else mobject.arg_separator.join(strings)
    mobject.authored_tex = source
    return mobject
