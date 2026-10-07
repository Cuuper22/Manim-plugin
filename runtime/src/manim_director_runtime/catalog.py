"""Static runtime data advertised in the bridge `ready` frame (contract §1.9, §2.3)."""

from __future__ import annotations

from typing import Any

from .scaffold import templates
from .themes import themes


def catalog() -> dict[str, Any]:
    return {
        "themes": [
            {"name": name, "tokens": [list(pair) for pair in theme.tokens()]}
            for name, theme in themes().items()
        ],
        # Every template is a whole project whose scene can also join an existing project.
        "project_templates": list(templates()),
        "scene_templates": list(templates()),
    }
