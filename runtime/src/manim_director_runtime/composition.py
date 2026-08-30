"""Pure visual-direction primitives used by the optional Manim adapter.

This module deliberately has no Manim (or NumPy) dependency.  It describes a
small design language and resolves objects into non-overlapping rectangles in
Manim's usual, centre-origin coordinate system.  Keeping the solver pure makes
it cheap to import from authoring tools and deterministic in headless builds.
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace
from enum import Enum
from math import ceil, sqrt
from typing import Any, ClassVar, Iterable, Mapping, Sequence


class CompositionError(ValueError):
    """A direction or layout request cannot be satisfied safely."""


class Region(str, Enum):
    """Named lanes in the safe frame.

    ``safe`` is the complete inset frame and is intended for a single hero
    visual.  ``content`` is the main lane with the header and caption lanes
    already removed.  ``left`` and ``right`` are disjoint halves of content.
    ``footer`` aliases the caption lane when captions are enabled.
    """

    SAFE = "safe"
    HEADER = "header"
    CONTENT = "content"
    LEFT = "left"
    RIGHT = "right"
    FOOTER = "footer"
    CAPTION = "caption"


class BeatIntent(str, Enum):
    INTRODUCE = "introduce"
    EXPLAIN = "explain"
    COMPARE = "compare"
    REVEAL = "reveal"
    PROVE = "prove"
    RECAP = "recap"


class TransitionKind(str, Enum):
    CONTINUATION = "continuation"
    CONTRAST = "contrast"
    REVEAL = "reveal"
    CHAPTER = "chapter"


@dataclass(frozen=True, slots=True)
class Rect:
    """An axis-aligned rectangle in scene coordinates."""

    x: float
    y: float
    width: float
    height: float

    def __post_init__(self) -> None:
        if self.width <= 0 or self.height <= 0:
            raise CompositionError("rectangle dimensions must be positive")

    @property
    def left(self) -> float:
        return self.x - self.width / 2

    @property
    def right(self) -> float:
        return self.x + self.width / 2

    @property
    def top(self) -> float:
        return self.y + self.height / 2

    @property
    def bottom(self) -> float:
        return self.y - self.height / 2

    def inset(self, *, top: float, right: float, bottom: float, left: float) -> "Rect":
        width = self.width - left - right
        height = self.height - top - bottom
        if width <= 0 or height <= 0:
            raise CompositionError("insets consume the entire frame")
        return Rect(
            self.x + (left - right) / 2,
            self.y + (bottom - top) / 2,
            width,
            height,
        )

    def padded(self, amount: float) -> "Rect":
        if amount < 0 and (-2 * amount >= self.width or -2 * amount >= self.height):
            raise CompositionError("padding collapses rectangle")
        return Rect(self.x, self.y, self.width + 2 * amount, self.height + 2 * amount)

    def contains(self, other: "Rect", *, tolerance: float = 1e-9) -> bool:
        return (
            other.left >= self.left - tolerance
            and other.right <= self.right + tolerance
            and other.bottom >= self.bottom - tolerance
            and other.top <= self.top + tolerance
        )

    def intersects(self, other: "Rect", *, gap: float = 0.0, tolerance: float = 1e-9) -> bool:
        return not (
            self.right + gap <= other.left + tolerance
            or other.right + gap <= self.left + tolerance
            or self.top + gap <= other.bottom + tolerance
            or other.top + gap <= self.bottom + tolerance
        )


@dataclass(frozen=True, slots=True)
class Insets:
    top: float = 0.45
    right: float = 0.60
    bottom: float = 0.48
    left: float = 0.60

    def __post_init__(self) -> None:
        if min(self.top, self.right, self.bottom, self.left) < 0:
            raise CompositionError("safe-area insets cannot be negative")


@dataclass(frozen=True, slots=True)
class TypeScale:
    """A deliberately narrow hierarchy for a 1920x1080 composition."""

    hero: float = 64
    title: float = 44
    section: float = 36
    body: float = 30
    math: float = 48
    label: float = 24
    caption: float = 25
    micro: float = 18

    def __post_init__(self) -> None:
        for name in self.__dataclass_fields__:
            value = float(getattr(self, name))
            if not 8 <= value <= 120:
                raise CompositionError(f"typography scale {name!r} must be between 8 and 120")
        if not (self.hero >= self.title >= self.section >= self.body >= self.label >= self.micro):
            raise CompositionError("typography scale must preserve a visible hierarchy")

    def get(self, role: str) -> float:
        if role not in self.__dataclass_fields__:
            raise CompositionError(f"unknown typography role: {role}")
        return float(getattr(self, role))

    @classmethod
    def from_mapping(cls, values: Mapping[str, Any] | None) -> "TypeScale":
        if not values:
            return cls()
        allowed = set(cls.__dataclass_fields__)
        unknown = set(values) - allowed
        if unknown:
            raise CompositionError(f"unknown typography roles: {', '.join(sorted(unknown))}")
        return cls(**{key: float(value) for key, value in values.items()})


@dataclass(frozen=True, slots=True)
class SpacingScale:
    xs: float = 0.12
    sm: float = 0.22
    md: float = 0.36
    lg: float = 0.58
    xl: float = 0.90

    def __post_init__(self) -> None:
        values = [self.xs, self.sm, self.md, self.lg, self.xl]
        if values != sorted(values) or self.xs <= 0:
            raise CompositionError("spacing values must be positive and monotonically increasing")

    def get(self, role: str) -> float:
        if role not in self.__dataclass_fields__:
            raise CompositionError(f"unknown spacing role: {role}")
        return float(getattr(self, role))

    def for_density(self, density: str) -> "SpacingScale":
        factor = {"spacious": 1.0, "balanced": 0.82, "dense": 0.66}[density]
        return replace(self, **{name: getattr(self, name) * factor for name in self.__dataclass_fields__})

    @classmethod
    def from_mapping(cls, values: Mapping[str, Any] | None) -> "SpacingScale":
        if not values:
            return cls()
        allowed = set(cls.__dataclass_fields__)
        unknown = set(values) - allowed
        if unknown:
            raise CompositionError(f"unknown spacing roles: {', '.join(sorted(unknown))}")
        return cls(**{key: float(value) for key, value in values.items()})


@dataclass(frozen=True, slots=True)
class ColorPalette:
    background: str = "#0B1020"
    foreground: str = "#F7F8FC"
    primary: str = "#78DCE8"
    secondary: str = "#FFD866"
    accent: str = "#FF6188"
    muted: str = "#72798C"
    success: str = "#A9DC76"

    def get(self, role: str) -> str:
        if role not in self.__dataclass_fields__:
            raise CompositionError(f"unknown color role: {role}")
        return str(getattr(self, role))

    @classmethod
    def from_mapping(cls, values: Mapping[str, Any] | None) -> "ColorPalette":
        if not values:
            return cls()
        allowed = set(cls.__dataclass_fields__)
        selected = {key: str(value) for key, value in values.items() if key in allowed}
        return cls(**selected)


@dataclass(frozen=True, slots=True)
class MotionGrammar:
    """Concrete motion choices behind semantic beat transitions."""

    continuation: str = "morph"
    contrast: str = "lateral"
    reveal: str = "draw"
    chapter: str = "reset"
    quick: float = 0.32
    standard: float = 0.62
    deliberate: float = 0.90

    _ALLOWED: ClassVar[dict[str, frozenset[str]]] = {
        "continuation": frozenset({"morph", "crossfade", "hold"}),
        "contrast": frozenset({"lateral", "crossfade"}),
        "reveal": frozenset({"draw", "fade", "scale"}),
        "chapter": frozenset({"reset", "crossfade"}),
    }

    def __post_init__(self) -> None:
        for key, choices in self._ALLOWED.items():
            if getattr(self, key) not in choices:
                raise CompositionError(f"motion {key!r} must be one of {', '.join(sorted(choices))}")
        if min(self.quick, self.standard, self.deliberate) <= 0:
            raise CompositionError("motion timings must be positive")

    @classmethod
    def from_mapping(cls, values: Mapping[str, Any] | None) -> "MotionGrammar":
        if not values:
            return cls()
        allowed = set(cls.__dataclass_fields__) - {"_ALLOWED"}
        unknown = set(values) - allowed
        if unknown:
            raise CompositionError(f"unknown motion keys: {', '.join(sorted(unknown))}")
        data: dict[str, Any] = {}
        for key, value in values.items():
            data[key] = float(value) if key in {"quick", "standard", "deliberate"} else str(value)
        return cls(**data)

    def style_for(self, transition: TransitionKind | str) -> str:
        kind = TransitionKind(transition)
        return str(getattr(self, kind.value))


@dataclass(frozen=True, slots=True)
class DesignSystem:
    """The visual decisions shared by every scene in a film."""

    palette: ColorPalette = field(default_factory=ColorPalette)
    typography: TypeScale = field(default_factory=TypeScale)
    spacing: SpacingScale = field(default_factory=SpacingScale)
    motion: MotionGrammar = field(default_factory=MotionGrammar)
    safe_area: Insets = field(default_factory=Insets)
    font: str = "DejaVu Sans"
    frame_width: float = 14.2222222222
    frame_height: float = 8.0
    density: str = "spacious"
    max_active: int = 4
    caption_lane: bool = True
    audience: str = "curious general audience"
    principle: str = "one-idea-per-beat"
    stroke_width: float = 3.5

    def __post_init__(self) -> None:
        if self.density not in {"spacious", "balanced", "dense"}:
            raise CompositionError("density must be spacious, balanced, or dense")
        if not 1 <= self.max_active <= 8:
            raise CompositionError("max_active must be between 1 and 8")
        if self.frame_width <= 0 or self.frame_height <= 0:
            raise CompositionError("frame dimensions must be positive")
        if not self.font.strip() or not self.audience.strip() or not self.principle.strip():
            raise CompositionError("font and narrative fields cannot be empty")
        if self.stroke_width <= 0:
            raise CompositionError("stroke_width must be positive")

    @property
    def effective_spacing(self) -> SpacingScale:
        return self.spacing.for_density(self.density)

    def color(self, role: str) -> str:
        return self.palette.get(role)

    def stroke(self, role: str = "foreground", *, width: float = 1.0, opacity: float = 1.0) -> dict[str, Any]:
        """Return keyword arguments accepted by Manim's stroked mobjects."""

        if width <= 0 or not 0 <= opacity <= 1:
            raise CompositionError("stroke width must be positive and opacity in [0, 1]")
        return {
            "stroke_color": self.color(role),
            "stroke_width": self.stroke_width * width,
            "stroke_opacity": opacity,
        }

    def font_size(self, role: str) -> float:
        return self.typography.get(role)

    @classmethod
    def from_mapping(cls, source: Mapping[str, Any] | None) -> "DesignSystem":
        """Load either a whole ``director.yaml`` mapping or its direction block."""

        if not source:
            return cls()
        defaults = cls()
        root = source.get("direction", {})
        if not root and any(key in source for key in ("composition", "typography", "motion", "narrative")):
            root = source
        if not isinstance(root, Mapping):
            raise CompositionError("direction must be a mapping")
        theme = source.get("theme", {}) if isinstance(source, Mapping) else {}
        if not theme and any(
            key in source
            for key in (*ColorPalette.__dataclass_fields__, "font", "stroke_width")
        ):
            # A project may pass the result of get_theme() directly instead of
            # wrapping it in a director.yaml-shaped mapping.
            theme = source
        if not isinstance(theme, Mapping):
            theme = {}

        def merged_block(name: str) -> dict[str, Any]:
            theme_block = theme.get(name, {})
            root_block = root.get(name, {})
            if not isinstance(theme_block, Mapping) or not isinstance(root_block, Mapping):
                raise CompositionError(f"direction.{name} must be a mapping")
            return {**theme_block, **root_block}

        composition = merged_block("composition")
        typography = merged_block("typography")
        motion = merged_block("motion")
        narrative = merged_block("narrative")
        spacing = merged_block("spacing")
        for name, block in {
            "composition": composition,
            "typography": typography,
            "motion": motion,
            "narrative": narrative,
            "spacing": spacing,
        }.items():
            if not isinstance(block, Mapping):
                raise CompositionError(f"direction.{name} must be a mapping")
        palette_values = {
            key: theme[key]
            for key in ColorPalette.__dataclass_fields__
            if key in theme
        }
        direction_palette = root.get("palette", {})
        if isinstance(direction_palette, Mapping):
            palette_values.update(direction_palette)

        safe_values = source.get("safe_area", {}) if isinstance(source, Mapping) else {}
        if not isinstance(safe_values, Mapping):
            safe_values = {}
        safe = Insets(
            top=float(safe_values.get("top", defaults.safe_area.top)),
            right=float(safe_values.get("right", defaults.safe_area.right)),
            bottom=float(safe_values.get("bottom", defaults.safe_area.bottom)),
            left=float(safe_values.get("left", defaults.safe_area.left)),
        )
        # director.yaml historically expressed safe areas as fractions.  Values
        # below .2 are unambiguously fractional for a Manim frame.
        if max(safe.top, safe.right, safe.bottom, safe.left) <= 0.2:
            safe = Insets(
                top=safe.top * defaults.frame_height,
                right=safe.right * defaults.frame_width,
                bottom=safe.bottom * defaults.frame_height,
                left=safe.left * defaults.frame_width,
            )

        scale_values = typography.get("scale", {})
        if not isinstance(scale_values, Mapping):
            raise CompositionError("direction.typography.scale must be a mapping")
        return cls(
            palette=ColorPalette.from_mapping(palette_values),
            typography=TypeScale.from_mapping(scale_values),
            spacing=SpacingScale.from_mapping(spacing),
            motion=MotionGrammar.from_mapping(motion),
            safe_area=safe,
            font=str(typography.get("font", theme.get("font", defaults.font))),
            density=str(composition.get("density", defaults.density)),
            max_active=int(composition.get("max_active", defaults.max_active)),
            caption_lane=bool(composition.get("caption_lane", defaults.caption_lane)),
            audience=str(narrative.get("audience", defaults.audience)),
            principle=str(narrative.get("principle", defaults.principle)),
            stroke_width=float(root.get("stroke_width", theme.get("stroke_width", defaults.stroke_width))),
        )


@dataclass(frozen=True, slots=True)
class Beat:
    """A pedagogical unit with an explicit audience question and payoff."""

    intent: BeatIntent | str
    audience_question: str
    takeaway: str
    focus: str
    visual_metaphor: str
    transition: TransitionKind | str = TransitionKind.CONTINUATION
    max_active: int | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "intent", BeatIntent(self.intent))
        object.__setattr__(self, "transition", TransitionKind(self.transition))
        if not self.audience_question.strip():
            raise CompositionError("every beat needs the audience's current question")
        if not self.takeaway.strip():
            raise CompositionError("every beat needs one explicit takeaway")
        if not self.focus.strip():
            raise CompositionError("every beat needs one named visual focus")
        if not self.visual_metaphor.strip():
            raise CompositionError("every beat needs one visual metaphor")
        if self.max_active is not None and not 1 <= self.max_active <= 8:
            raise CompositionError("beat max_active must be between 1 and 8")


@dataclass(frozen=True, slots=True)
class LayoutItem:
    key: str
    width: float
    height: float
    priority: int = 50
    min_scale: float = 0.58

    def __post_init__(self) -> None:
        if not self.key:
            raise CompositionError("layout item key cannot be empty")
        if self.width <= 0 or self.height <= 0:
            raise CompositionError("layout item dimensions must be positive")
        if not 0 < self.min_scale <= 1:
            raise CompositionError("min_scale must be in (0, 1]")


@dataclass(frozen=True, slots=True)
class Placement:
    key: str
    rect: Rect
    scale: float
    region: Region
    priority: int = 50


@dataclass(frozen=True, slots=True)
class LayoutPlan:
    placements: tuple[Placement, ...]
    evicted: tuple[str, ...] = ()
    flow: str = "auto"
    region: Region = Region.CONTENT
    reflowed: bool = False

    def for_key(self, key: str) -> Placement:
        for placement in self.placements:
            if placement.key == key:
                return placement
        raise KeyError(key)


class CompositionLayout:
    """Deterministic, collision-free allocation inside named safe regions."""

    _FLOWS = frozenset({"auto", "row", "column", "grid"})
    _ANCHORS = frozenset({
        "center", "top", "bottom", "left", "right",
        "top_left", "top_right", "bottom_left", "bottom_right",
    })

    def __init__(self, design: DesignSystem | None = None):
        self.design = design or DesignSystem()
        self._regions = self._make_regions()

    def _make_regions(self) -> dict[Region, Rect]:
        design = self.design
        frame = Rect(0, 0, design.frame_width, design.frame_height)
        safe = frame.inset(
            top=design.safe_area.top,
            right=design.safe_area.right,
            bottom=design.safe_area.bottom,
            left=design.safe_area.left,
        )
        spacing = design.effective_spacing
        header_height = min(0.72, safe.height * 0.14)
        caption_height = min(0.86, safe.height * 0.17) if design.caption_lane else 0.01
        header = Rect(safe.x, safe.top - header_height / 2, safe.width, header_height)
        caption = Rect(safe.x, safe.bottom + caption_height / 2, safe.width, caption_height)
        content_top = header.bottom - spacing.md
        content_bottom = caption.top + spacing.md if design.caption_lane else safe.bottom
        content_height = content_top - content_bottom
        if content_height <= 0:
            raise CompositionError("safe frame leaves no content lane")
        content = Rect(safe.x, (content_top + content_bottom) / 2, safe.width, content_height)
        half_width = (content.width - spacing.md) / 2
        left = Rect(content.left + half_width / 2, content.y, half_width, content.height)
        right = Rect(content.right - half_width / 2, content.y, half_width, content.height)
        return {
            Region.SAFE: safe,
            Region.HEADER: header,
            Region.CONTENT: content,
            Region.LEFT: left,
            Region.RIGHT: right,
            Region.FOOTER: caption,
            Region.CAPTION: caption,
        }

    @property
    def regions(self) -> Mapping[Region, Rect]:
        return dict(self._regions)

    def region(self, name: Region | str) -> Rect:
        try:
            return self._regions[Region(name)]
        except (ValueError, KeyError) as exc:
            raise CompositionError(f"unknown layout region: {name}") from exc

    def arrange(
        self,
        items: Sequence[LayoutItem],
        *,
        region: Region | str = Region.CONTENT,
        flow: str = "auto",
        gap: float | None = None,
        max_active: int | None = None,
    ) -> LayoutPlan:
        if flow not in self._FLOWS:
            raise CompositionError(f"flow must be one of {', '.join(sorted(self._FLOWS))}")
        if not items:
            return LayoutPlan((), flow=flow, region=Region(region))
        keys = [item.key for item in items]
        if len(keys) != len(set(keys)):
            raise CompositionError("layout item keys must be unique")
        budget = self.design.max_active if max_active is None else int(max_active)
        if not 1 <= budget <= 8:
            raise CompositionError("max_active must be between 1 and 8")

        indexed = list(enumerate(items))
        kept_indices = {
            index
            for index, _item in sorted(indexed, key=lambda pair: (-pair[1].priority, pair[0]))[:budget]
        }
        selected = [item for index, item in indexed if index in kept_indices]
        evicted = [item.key for index, item in indexed if index not in kept_indices]
        lane = self.region(region)
        actual_gap = self.design.effective_spacing.md if gap is None else float(gap)
        if actual_gap < 0:
            raise CompositionError("layout gap cannot be negative")

        flows = self._flow_candidates(flow, selected, lane)
        reflowed = False
        while selected:
            for candidate_index, candidate in enumerate(flows):
                placements = self._arrange_flow(selected, lane, Region(region), candidate, actual_gap)
                if all(p.scale + 1e-9 >= item.min_scale for p, item in zip(placements, selected)):
                    self._assert_collision_free(placements, actual_gap)
                    return LayoutPlan(
                        tuple(placements), tuple(evicted), candidate, Region(region),
                        reflowed or candidate_index > 0,
                    )
            if len(selected) == 1:
                placements = self._arrange_flow(selected, lane, Region(region), flows[0], actual_gap)
                required = placements[0].scale
                minimum = selected[0].min_scale
                raise CompositionError(
                    f"{selected[0].key!r} would need scale {required:.3f}, below its readable "
                    f"minimum of {minimum:.3f}; simplify it or choose a larger region"
                )
            drop_index = min(range(len(selected)), key=lambda index: (selected[index].priority, -index))
            evicted.append(selected.pop(drop_index).key)
            flows = self._flow_candidates(flow, selected, lane)
            reflowed = True
        return LayoutPlan((), tuple(evicted), flow, Region(region), True)

    def place(
        self,
        item: LayoutItem,
        *,
        region: Region | str = Region.CONTENT,
        anchor: str = "center",
        occupied: Iterable[Placement] = (),
        gap: float | None = None,
    ) -> LayoutPlan:
        if anchor not in self._ANCHORS:
            raise CompositionError(f"unknown anchor: {anchor}")
        lane = self.region(region)
        actual_gap = self.design.effective_spacing.md if gap is None else float(gap)
        occupied_items = list(occupied)
        fit_scale = min(1.0, lane.width / item.width, lane.height / item.height)
        if fit_scale + 1e-9 < item.min_scale:
            raise CompositionError(
                f"{item.key!r} would need scale {fit_scale:.3f}, below its readable minimum "
                f"of {item.min_scale:.3f}; shorten it or choose a larger region"
            )
        width, height = item.width * fit_scale, item.height * fit_scale
        for candidate_anchor in self._anchor_order(anchor):
            rect = self._anchored_rect(width, height, lane, candidate_anchor)
            if not any(rect.intersects(existing.rect, gap=actual_gap) for existing in occupied_items):
                placement = Placement(item.key, rect, fit_scale, Region(region), item.priority)
                return LayoutPlan((placement,), (), "place", Region(region), candidate_anchor != anchor)

        preferred = self._anchored_rect(width, height, lane, anchor)
        conflicts = [
            existing for existing in occupied_items
            if preferred.intersects(existing.rect, gap=actual_gap)
        ]
        evicted = tuple(existing.key for existing in sorted(conflicts, key=lambda value: value.priority))
        placement = Placement(item.key, preferred, fit_scale, Region(region), item.priority)
        return LayoutPlan((placement,), evicted, "place", Region(region), True)

    @staticmethod
    def _anchor_order(preferred: str) -> tuple[str, ...]:
        all_anchors = (
            "center", "top", "bottom", "left", "right",
            "top_left", "top_right", "bottom_left", "bottom_right",
        )
        return (preferred,) + tuple(anchor for anchor in all_anchors if anchor != preferred)

    @staticmethod
    def _anchored_rect(width: float, height: float, lane: Rect, anchor: str) -> Rect:
        x, y = lane.x, lane.y
        if "left" in anchor or anchor == "left":
            x = lane.left + width / 2
        elif "right" in anchor or anchor == "right":
            x = lane.right - width / 2
        if "top" in anchor or anchor == "top":
            y = lane.top - height / 2
        elif "bottom" in anchor or anchor == "bottom":
            y = lane.bottom + height / 2
        return Rect(x, y, width, height)

    @staticmethod
    def _flow_candidates(flow: str, items: Sequence[LayoutItem], lane: Rect) -> list[str]:
        if flow != "auto":
            fallback = [flow] + [candidate for candidate in ("row", "column", "grid") if candidate != flow]
            return fallback
        if len(items) == 1:
            return ["grid"]
        if len(items) == 2:
            preferred = "row" if lane.width / lane.height >= 1.35 else "column"
            return [preferred, "column" if preferred == "row" else "row", "grid"]
        return ["grid", "row", "column"]

    def _arrange_flow(
        self,
        items: Sequence[LayoutItem],
        lane: Rect,
        region: Region,
        flow: str,
        gap: float,
    ) -> list[Placement]:
        count = len(items)
        if flow == "row":
            rows, columns = 1, count
        elif flow == "column":
            rows, columns = count, 1
        else:
            rows, columns = self._grid_shape(count, lane)
        cell_width = (lane.width - gap * (columns - 1)) / columns
        cell_height = (lane.height - gap * (rows - 1)) / rows
        if cell_width <= 0 or cell_height <= 0:
            raise CompositionError("layout gap leaves no room for content")
        result: list[Placement] = []
        for index, item in enumerate(items):
            row, column = divmod(index, columns)
            remaining = count - row * columns
            columns_in_row = min(columns, remaining)
            row_width = columns_in_row * cell_width + (columns_in_row - 1) * gap
            row_left = lane.x - row_width / 2
            cell_x = row_left + column * (cell_width + gap) + cell_width / 2
            cell_y = lane.top - row * (cell_height + gap) - cell_height / 2
            scale = min(1.0, cell_width / item.width, cell_height / item.height)
            result.append(Placement(
                item.key,
                Rect(cell_x, cell_y, item.width * scale, item.height * scale),
                scale,
                region,
                item.priority,
            ))
        return result

    @staticmethod
    def _grid_shape(count: int, lane: Rect) -> tuple[int, int]:
        best: tuple[float, int, int] | None = None
        for rows in range(1, count + 1):
            columns = ceil(count / rows)
            cell_aspect = (lane.width / columns) / (lane.height / rows)
            ragged = rows * columns - count
            score = abs(cell_aspect - 1.55) + ragged * 0.16
            candidate = (score, rows, columns)
            if best is None or candidate < best:
                best = candidate
        assert best is not None
        return best[1], best[2]

    @staticmethod
    def _assert_collision_free(placements: Sequence[Placement], gap: float) -> None:
        for index, first in enumerate(placements):
            for second in placements[index + 1:]:
                if first.rect.intersects(second.rect, gap=gap):
                    raise CompositionError(
                        f"internal layout collision between {first.key!r} and {second.key!r}"
                    )


__all__ = [
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
]


_DIRECTED_EXPORTS = frozenset({
    "CompositionMixin",
    "DirectedMovingCameraScene",
    "DirectedScene",
    "DirectedThreeDScene",
})


def __getattr__(name: str) -> Any:
    """Load Manim-backed classes only when an author asks for one."""

    if name in _DIRECTED_EXPORTS:
        from . import directed_scene

        value = getattr(directed_scene, name)
        globals()[name] = value
        return value
    raise AttributeError(name)
