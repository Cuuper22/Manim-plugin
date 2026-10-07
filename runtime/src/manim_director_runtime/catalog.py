"""Static runtime data advertised in the bridge `ready` frame (contract §1.9, §2.3)."""

from __future__ import annotations

from typing import Any

from .templates import SCENE_TEMPLATES
from .themes import BUILTIN_THEMES

COLOR_TOKENS = ("background", "foreground", "primary", "secondary", "accent", "muted", "success")
PROJECT_TEMPLATES = ("explainer",)


def theme_names() -> list[str]:
    return list(BUILTIN_THEMES)


def default_theme() -> str:
    return next(iter(BUILTIN_THEMES))


def catalog() -> dict[str, Any]:
    return {
        "themes": [
            {"name": name, "tokens": [[token, theme[token].upper()] for token in COLOR_TOKENS]}
            for name, theme in BUILTIN_THEMES.items()
        ],
        "project_templates": list(PROJECT_TEMPLATES),
        "scene_templates": list(SCENE_TEMPLATES),
    }
