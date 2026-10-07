from __future__ import annotations

import itertools

import numpy as np
import pytest

from manim_director_runtime.catalog import catalog
from manim_director_runtime.errors import CompositionError
from manim_director_runtime.themes import COLOR_TOKENS, default_theme, theme, themes

# Machado et al. (2009) dichromacy simulations at full severity, on linear RGB.
CVD = {
    "protan": [
        [0.152286, 1.052583, -0.204868],
        [0.114503, 0.786281, 0.099216],
        [-0.003882, -0.048116, 1.051998],
    ],
    "deutan": [
        [0.367322, 0.860646, -0.227968],
        [0.280085, 0.672501, 0.047413],
        [-0.011820, 0.042940, 0.968881],
    ],
    "tritan": [
        [1.255528, -0.076749, -0.178779],
        [-0.078411, 0.930809, 0.147602],
        [0.004733, 0.691367, 0.303900],
    ],
}
SIGNAL_TOKENS = ("foreground", "primary", "secondary", "accent", "muted", "success")


def linear(hex_color: str) -> np.ndarray:
    srgb = np.array([int(hex_color[i : i + 2], 16) / 255 for i in (1, 3, 5)])
    return np.where(srgb <= 0.04045, srgb / 12.92, ((srgb + 0.055) / 1.055) ** 2.4)


def contrast(a: str, b: str) -> float:
    lum = sorted((float(linear(c) @ [0.2126, 0.7152, 0.0722]) for c in (a, b)), reverse=True)
    return (lum[0] + 0.05) / (lum[1] + 0.05)


def lab(rgb_linear: np.ndarray) -> np.ndarray:
    xyz = np.array(
        [[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]]
    ) @ np.clip(rgb_linear, 0, 1)
    t = xyz / np.array([0.95047, 1.0, 1.08883])
    f = np.where(t > (6 / 29) ** 3, np.cbrt(t), t / (3 * (6 / 29) ** 2) + 4 / 29)
    return np.array([116 * f[1] - 16, 500 * (f[0] - f[1]), 200 * (f[1] - f[2])])


def test_first_theme_is_the_default_and_names_are_unique() -> None:
    names = list(themes())
    assert names[0] == default_theme().name == "midnight"
    assert {"midnight", "paper", "chalkboard", "contrast"} <= set(names)


def test_tokens_are_attributes_and_resolve_by_name() -> None:
    midnight = theme("midnight")
    assert midnight.color("primary") == midnight.primary
    assert midnight.color("#a1b2c3") == "#A1B2C3"
    assert [token for token, _ in midnight.tokens()] == list(COLOR_TOKENS)
    assert midnight.font.startswith(midnight.fonts[0])
    with pytest.raises(CompositionError, match="neither a theme token"):
        midnight.color("teal")


def test_unknown_theme_names_the_choices() -> None:
    with pytest.raises(CompositionError, match="choose one of midnight, paper") as raised:
        theme("sepia")
    assert raised.value.data["available"] == list(themes())


def test_variants_replace_only_colors() -> None:
    branded = theme("paper").with_colors(accent="#d1495b")
    assert branded.accent == "#D1495B" and branded.primary == theme("paper").primary
    with pytest.raises(CompositionError, match="Unknown color tokens teal"):
        theme("paper").with_colors(teal="#000000")


@pytest.mark.parametrize("name", list(themes()))
def test_every_signal_color_is_legible_on_the_background(name: str) -> None:
    palette = theme(name)
    assert contrast(palette.foreground, palette.background) >= 12
    for token in SIGNAL_TOKENS:
        assert contrast(getattr(palette, token), palette.background) >= 4.5, token


@pytest.mark.parametrize("name", list(themes()))
def test_signal_colors_stay_distinct_under_color_vision_deficiencies(name: str) -> None:
    palette = theme(name)
    for kind, matrix in {"normal": np.eye(3), **CVD}.items():
        seen = {t: lab(np.asarray(matrix) @ linear(getattr(palette, t))) for t in SIGNAL_TOKENS}
        for a, b in itertools.combinations(SIGNAL_TOKENS, 2):
            assert np.linalg.norm(seen[a] - seen[b]) >= 15, f"{kind}: {a} vs {b}"


def test_catalog_lists_ordered_uppercase_tokens() -> None:
    entries = catalog()["themes"]
    assert [entry["name"] for entry in entries] == list(themes())
    for entry in entries:
        assert [token for token, _ in entry["tokens"]] == list(COLOR_TOKENS)
        assert all(color == color.upper() and len(color) == 7 for _, color in entry["tokens"])
