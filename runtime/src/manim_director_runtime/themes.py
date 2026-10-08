"""Themes and text roles. Theme data lives only in `data/themes.json` (contract §1.9)."""

from __future__ import annotations

import dataclasses
import json
import re
from dataclasses import dataclass
from enum import StrEnum
from functools import cache
from importlib import resources
from typing import Any

from .errors import CompositionError

COLOR_TOKENS = (
    "background",
    "foreground",
    "primary",
    "secondary",
    "accent",
    "muted",
    "success",
    "highlight",  # the kit's own marks (notes, links, boxes, ✗): never a content meaning
)
_HEX = re.compile(r"#[0-9A-F]{6}")


@dataclass(frozen=True, slots=True)
class Theme:
    name: str
    fonts: tuple[str, ...]
    background: str
    foreground: str
    primary: str
    secondary: str
    accent: str
    muted: str
    success: str
    highlight: str

    def __post_init__(self) -> None:
        for token in COLOR_TOKENS:
            if not _HEX.fullmatch(getattr(self, token)):
                raise CompositionError(
                    f"Theme {self.name!r} {token} must be an uppercase #RRGGBB color.",
                    theme=self.name,
                    token=token,
                )

    @property
    def font(self) -> str:
        """A Pango family list: the first installed family is used."""

        return ", ".join(self.fonts)

    def color(self, value: Any) -> str:
        """Resolve a token name (`"accent"`), a `#RRGGBB` color or a Manim color (`YELLOW`)
        to `#RRGGBB`."""

        if not isinstance(value, str):
            # ManimColor, duck-typed so themes never import Manim; it cannot be compared
            # with a str, so it must not reach the token lookup.
            to_hex = getattr(value, "to_hex", None)
            value = to_hex() if callable(to_hex) else repr(value)
        if value in COLOR_TOKENS:
            return str(getattr(self, value))
        if _HEX.fullmatch(value.upper()):
            return value.upper()
        raise CompositionError(
            f"{value!r} is neither a theme token ({', '.join(COLOR_TOKENS)}) nor a #RRGGBB color.",
            value=value,
            tokens=list(COLOR_TOKENS),
        )

    def tokens(self) -> list[tuple[str, str]]:
        return [(token, getattr(self, token)) for token in COLOR_TOKENS]

    def with_colors(self, **colors: Any) -> Theme:
        """A variant with some tokens replaced, e.g. `with_colors(accent="#D1495B")`."""

        unknown = sorted(set(colors) - set(COLOR_TOKENS))
        if unknown:
            raise CompositionError(
                f"Unknown color tokens {', '.join(unknown)}; "
                f"themes have {', '.join(COLOR_TOKENS)}.",
                unknown=unknown,
            )
        return dataclasses.replace(self, **{k: self.color(v) for k, v in colors.items()})


@cache
def themes() -> dict[str, Theme]:
    """Every packaged theme by name, in file order (the first is the default)."""

    source = resources.files(__package__).joinpath("data", "themes.json")
    raw = json.loads(source.read_text(encoding="utf-8"))
    return {
        entry["name"]: Theme(entry["name"], tuple(entry["fonts"]), **entry["colors"])
        for entry in raw["themes"]
    }


def theme(name: str) -> Theme:
    try:
        return themes()[name]
    except KeyError:
        available = list(themes())
        raise CompositionError(
            f"Unknown theme {name!r}; choose one of {', '.join(available)}.",
            theme=name,
            available=available,
        ) from None


def default_theme() -> Theme:
    return next(iter(themes().values()))


class Role(StrEnum):
    TITLE = "title"
    HEADING = "heading"
    BODY = "body"
    CAPTION = "caption"
    NOTE = "note"
    LABEL = "label"


@dataclass(frozen=True, slots=True)
class TextStyle:
    font_size: float
    weight: str  # a Pango weight name as Manim spells it
    color: str  # a color token


TEXT_STYLES = {
    Role.TITLE: TextStyle(42, "SEMIBOLD", "foreground"),
    Role.HEADING: TextStyle(34, "MEDIUM", "foreground"),
    Role.BODY: TextStyle(30, "NORMAL", "foreground"),
    Role.CAPTION: TextStyle(26, "NORMAL", "foreground"),
    Role.NOTE: TextStyle(26, "NORMAL", "foreground"),  # what a note says is read, not glanced at
    Role.LABEL: TextStyle(22, "NORMAL", "muted"),
}
MATH_FONT_SIZE = 44
