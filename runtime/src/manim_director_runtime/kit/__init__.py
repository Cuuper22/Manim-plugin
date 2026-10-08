"""Explainer components: themed, placement-safe building blocks for DirectedScene.

```python
from manim_director_runtime.kit import DotArray, Figure, FunctionPlot, Readout, VectorGrid
```

Each component is a VGroup: `place()` it like any mobject. Its parts are attributes
(`plot.axes`, `grid.i_hat`); noun methods return overlays drawn on it (`plot.tangent(1)`),
which `self.show(...)` reveals and which follow it and leave with it; verbs return one
Animation to play (`plot.refine(bars)`, `grid.apply(M)`). Colors are theme tokens.
`reserve(part)` keeps any part laid out but hidden until `self.show(part)`, so content that
arrives later never reflows what is on screen; `hide(part)` fades it back, for a replay.
"""

from typing import TYPE_CHECKING, Any

from .overlay import Live, hide, reserve

if TYPE_CHECKING:
    from .dots import DotArray
    from .geometry import Figure
    from .grid import VectorGrid
    from .labels import label
    from .plot import FunctionPlot, Readout

_EXPORTS = {
    "DotArray": "dots",
    "Figure": "geometry",
    "FunctionPlot": "plot",
    "Readout": "plot",
    "VectorGrid": "grid",
    "label": "labels",
}

__all__ = [
    "DotArray",
    "Figure",
    "FunctionPlot",
    "Live",
    "Readout",
    "VectorGrid",
    "hide",
    "label",
    "reserve",
]


def __getattr__(name: str) -> Any:
    if name not in _EXPORTS:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    from importlib import import_module

    value = getattr(import_module(f".{_EXPORTS[name]}", __name__), name)
    globals()[name] = value
    return value
