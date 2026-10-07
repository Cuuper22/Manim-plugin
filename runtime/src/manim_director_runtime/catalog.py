"""Static runtime data advertised in the bridge `ready` frame (contract §1.9, §2.3)."""

from __future__ import annotations

from typing import Any

from .templates import SCENE_TEMPLATES
from .themes import themes

PROJECT_TEMPLATES = ("explainer",)


def theme_names() -> list[str]:
    return list(themes())


def default_theme() -> str:
    return next(iter(themes()))


def catalog() -> dict[str, Any]:
    return {
        "themes": [
            {"name": name, "tokens": [list(pair) for pair in theme.tokens()]}
            for name, theme in themes().items()
        ],
        "project_templates": list(PROJECT_TEMPLATES),
        "scene_templates": list(SCENE_TEMPLATES),
    }
