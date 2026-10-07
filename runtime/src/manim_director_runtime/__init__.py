"""Manim Director runtime: the bridge worker and the DirectedScene authoring API.

Scene classes import Manim on first use, so the bridge and other tools stay light.
"""

from typing import TYPE_CHECKING, Any

from .beats import Beat, Intent, Transition
from .errors import CompositionError, DirectorError
from .layout import Rect, Region
from .themes import Role, Theme

if TYPE_CHECKING:
    from .derivation import Derivation
    from .scene import Directed, DirectedMovingCameraScene, DirectedScene, DirectedThreeDScene

__version__ = "2.0.0"

_SCENE_EXPORTS = {
    "Derivation": "derivation",
    "Directed": "scene",
    "DirectedScene": "scene",
    "DirectedMovingCameraScene": "scene",
    "DirectedThreeDScene": "scene",
}

__all__ = [
    "Beat",
    "CompositionError",
    "Derivation",
    "Directed",
    "DirectedMovingCameraScene",
    "DirectedScene",
    "DirectedThreeDScene",
    "DirectorError",
    "Intent",
    "Rect",
    "Region",
    "Role",
    "Theme",
    "Transition",
    "__version__",
]


def __getattr__(name: str) -> Any:
    if name not in _SCENE_EXPORTS:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    from importlib import import_module

    value = getattr(import_module(f".{_SCENE_EXPORTS[name]}", __name__), name)
    globals()[name] = value
    return value
