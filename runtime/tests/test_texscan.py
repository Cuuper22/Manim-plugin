from __future__ import annotations

import pytest

from manim_director_runtime.errors import CompositionError
from manim_director_runtime.texscan import (
    atoms,
    colorize,
    is_relation,
    occurrences,
    paint,
    upright_words,
    without_alignment,
)

RED, GREEN = "#FF0000", "#00FF00"
PUSH_RED = r"\special{color push rgb 1.0000 0.0000 0.0000}"
POP = r"\special{color pop}"


def found(tex: str, term: str, **kwargs) -> list[str]:
    return [tex[start:end] for start, end in occurrences(tex, term, **kwargs)]


def test_terms_match_whole_tokens_only() -> None:
    tex = r"\max(x) + x^2 + \exp x + 12 + 1"
    assert found(tex, "x") == ["x", "x", "x"]
    assert found(tex, "1") == ["1"]  # never the 1 inside 12
    assert found(tex, "x^2") == ["x^2"]
    assert found(r"a_{n+1} + a_n", "a_n") == ["a_n"]
    assert found(r"\frac{b}{2a}", r"\frac{b}{2a}") == [r"\frac{b}{2a}"]
    assert found(r"\frac{b}{2 a}", r"\frac{b}{2a}") == [r"\frac{b}{2 a}"]  # spacing is irrelevant


def test_text_and_font_arguments_are_not_searched() -> None:
    tex = r"x \text{ for x } \mathrm{max} \mathbf{x} \operatorname{x}"
    assert found(tex, "x") == ["x"]
    assert found(tex, r"\mathbf{x}") == [r"\mathbf{x}"]


def test_text_mode_only_searches_math() -> None:
    tex = r"Let $x > 0$, so x is positive and $$x^2 > 0$$ \(x\)"
    assert len(found(tex, "x", math_only=True)) == 3


def test_bad_terms_fail_clearly() -> None:
    with pytest.raises(CompositionError, match="empty"):
        occurrences("x", "  ")
    with pytest.raises(CompositionError, match="unbalanced"):
        occurrences("x", "{x")


def test_paint_wraps_arguments_in_braces_but_never_doubles_them() -> None:
    assert paint(r"e^x", [((2, 3), RED)]) == "e^{" + PUSH_RED + "x" + POP + "}"
    assert paint(r"\hat x", [((5, 6), RED)]) == r"\hat {" + PUSH_RED + "x" + POP + "}"
    # Right after an opening brace the span already is one argument; `{{` would make Manim
    # split the string at its double-brace notation.
    assert paint(r"{x}", [((1, 2), RED)]) == "{" + PUSH_RED + "x" + POP + "}"
    with pytest.raises(CompositionError, match="overlap"):
        paint("xy", [((0, 2), RED), ((1, 2), GREEN)])


def test_paint_keeps_the_atom_class_of_operators() -> None:
    def red(tex: str, term: str) -> str:
        return paint(tex, [(span, RED) for span in occurrences(tex, term)])

    def body(token: str) -> str:
        return PUSH_RED + token + POP

    # Specials are invisible to TeX's spacing: a bare `=` is still a relation.
    assert red("a = b", "=") == f"a {body('=')} b"
    assert red("a + b", "+") == f"a {body('+')} b"
    # Before scripts the span needs one atom, of its own class.
    total, integral = body(r"\sum"), body(r"\int")
    assert red(r"\sum_{k=1}^n k", r"\sum") == r"\mathop{" + total + "}_{k=1}^n k"
    assert red(r"\int_0^1 f", r"\int") == r"\mathop{" + integral + r"}\nolimits_0^1 f"
    assert red(r"x^2", "x") == r"\mathord{" + body("x") + "}^2"
    # A delimiter after \left cannot be wrapped at all.
    assert red(r"\left( x \right)", "(") == r"\left( x \right)"


def test_painted_relations_stay_relations_and_atoms() -> None:
    pieces = atoms(colorize(r"a = b", {"=": RED}))
    assert pieces == ["a", PUSH_RED + "=" + POP, "b"]
    assert is_relation(pieces[1]) and not is_relation(PUSH_RED + "x" + POP)


def test_colorize_prefers_longer_symbols() -> None:
    colored = colorize(r"a_n = a_{n-1} + a", {"a": GREEN, "a_n": RED})
    assert colored.count("1.0000 0.0000 0.0000") == 1  # a_n
    assert colored.count("0.0000 1.0000 0.0000") == 2  # the other two a
    assert colorize("y", {"x": RED}) == "y"


@pytest.mark.parametrize(
    ("tex", "expected"),
    [
        (r"(a+b)^2 = a^2 + 2ab", ["(", "a", "+", "b", ")^2", "=", "a^2", "+", "2", "a", "b"]),
        (r"\frac{b}{a}x = 0", [r"\frac{b}{a}", "x", "=", "0"]),
        (r"\sum_{k=1}^{n} k^2", [r"\sum_{k=1}^{n}", "k^2"]),
        (r"\left(x + 1\right)^2", [r"\left(", "x", "+", "1", r"\right)^2"]),
        (r"\hat x \cdot \vec v", [r"\hat x", r"\cdot", r"\vec v"]),
        (r"\frac12 + 3.5x - 10", [r"\frac12", "+", "3.5", "x", "-", "10"]),
        (r"f'(x) = \lim_{h \to 0} h", ["f'", "(", "x", ")", "=", r"\lim_{h \to 0}", "h"]),
        (r"\sqrt[3]{x} \mathbb{R}", [r"\sqrt[3]{x}", r"\mathbb{R}"]),
        (r"\begin{pmatrix} a & b \end{pmatrix} v", [r"\begin{pmatrix} a & b \end{pmatrix}", "v"]),
        (r"{a \over b} c", [r"{a \over b}", "c"]),
    ],
)
def test_atoms_split_at_the_top_level(tex: str, expected: list[str]) -> None:
    assert atoms(tex) == expected


@pytest.mark.parametrize(
    "tex",
    [r"a & b", r"a \\ b", r"a \over b", r"x {{ y }}", r"{{x}} + y", r"x^", r"{x", r"a % c"],
)
def test_atoms_leave_unsplittable_tex_whole(tex: str) -> None:
    assert atoms(tex) == [tex]


def test_colored_symbols_stay_inside_their_atoms() -> None:
    pieces = atoms(colorize(r"x^2 + 2x", {"x": RED}))
    assert pieces == [r"\mathord{" + PUSH_RED + "x" + POP + "}^2", "+", "2", PUSH_RED + "x" + POP]


def test_relations() -> None:
    assert all(is_relation(r) for r in ["=", r"\le", r" \approx ", r"\Rightarrow"])
    assert not any(is_relation(r) for r in ["+", "x", r"\cdot"])


def test_alignment_marks_are_dropped_only_at_the_top_level() -> None:
    assert without_alignment(r"(a+b)^2 &= a^2") == r"(a+b)^2 = a^2"
    assert without_alignment(r"&= b") == "= b"
    matrix = r"\begin{pmatrix} a & b \end{pmatrix} &= M"
    assert without_alignment(matrix) == r"\begin{pmatrix} a & b \end{pmatrix} = M"
    assert without_alignment(r"{a & b}") == r"{a & b}"


def test_bare_words_are_set_upright_but_commands_and_products_are_not() -> None:
    assert upright_words("area =") == r"\text{area} ="
    assert upright_words(r"x_{max} + \sin x") == r"x_{\text{max}} + \sin x"
    assert upright_words(r"\text{slope} = ad - bc") == r"\text{slope} = ad - bc"
    assert upright_words(r"\frac{\Delta y}{h}") == r"\frac{\Delta y}{h}"
