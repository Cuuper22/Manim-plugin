"""Thin execution bridge and lazy visual-direction authoring API."""

from .errors import DirectorError

_COMPOSITION_EXPORTS = frozenset({
    "Beat",
    "BeatIntent",
    "ColorPalette",
    "CompositionError",
    "CompositionLayout",
    "DesignSystem",
    "Insets",
    "LayoutItem",
    "LayoutPlan",
    "MotionGrammar",
    "Placement",
    "Rect",
    "Region",
    "SpacingScale",
    "TransitionKind",
    "TypeScale",
    "CompositionMixin",
    "DirectedMovingCameraScene",
    "DirectedScene",
    "DirectedThreeDScene",
})

__all__ = ["DirectorError", "__version__", *_COMPOSITION_EXPORTS]
__version__ = "1.1.0"


def __getattr__(name: str):
    if name in _COMPOSITION_EXPORTS:
        from . import composition

        value = getattr(composition, name)
        globals()[name] = value
        return value
    raise AttributeError(name)
