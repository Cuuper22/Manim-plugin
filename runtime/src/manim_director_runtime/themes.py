from __future__ import annotations

from copy import deepcopy
from typing import Any

from .errors import DirectorError


BUILTIN_THEMES: dict[str, dict[str, Any]] = {
    "midnight": {
        "background": "#0B1020",
        "foreground": "#F7F8FC",
        "primary": "#78DCE8",
        "secondary": "#FFD866",
        "accent": "#FF6188",
        "muted": "#72798C",
        "success": "#A9DC76",
        "font": "DejaVu Sans",
        "math_font_size": 48,
        "text_font_size": 36,
        "stroke_width": 4,
    },
    "paper": {
        "background": "#FAF7F0",
        "foreground": "#18212B",
        "primary": "#16697A",
        "secondary": "#DB6400",
        "accent": "#8F2D56",
        "muted": "#69747C",
        "success": "#2D6A4F",
        "font": "DejaVu Sans",
        "math_font_size": 48,
        "text_font_size": 36,
        "stroke_width": 4,
    },
    "neon": {
        "background": "#050608",
        "foreground": "#F4F7FF",
        "primary": "#00F5D4",
        "secondary": "#FEE440",
        "accent": "#F15BB5",
        "muted": "#75809A",
        "success": "#9BFF4D",
        "font": "DejaVu Sans",
        "math_font_size": 50,
        "text_font_size": 36,
        "stroke_width": 5,
    },
    "colorblind": {
        "background": "#111111",
        "foreground": "#FFFFFF",
        "primary": "#56B4E9",
        "secondary": "#E69F00",
        "accent": "#CC79A7",
        "muted": "#999999",
        "success": "#009E73",
        "font": "DejaVu Sans",
        "math_font_size": 48,
        "text_font_size": 36,
        "stroke_width": 5,
    },
}


def get_theme(name: str = "midnight", overrides: dict[str, Any] | None = None) -> dict[str, Any]:
    if name not in BUILTIN_THEMES:
        raise DirectorError(
            "theme_not_found", f"Unknown theme: {name}", {"available": sorted(BUILTIN_THEMES)}
        )
    theme = deepcopy(BUILTIN_THEMES[name])
    if overrides:
        allowed = set(theme)
        unknown = sorted(set(overrides) - allowed)
        if unknown:
            raise DirectorError("invalid_theme", "Theme contains unsupported keys", {"unknown": unknown})
        theme.update(overrides)
    theme["name"] = name
    return theme


def list_themes() -> list[dict[str, Any]]:
    return [get_theme(name) for name in sorted(BUILTIN_THEMES)]
