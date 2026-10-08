"""Token-level TeX rewriting behind symbol colors, term lookup and derivation morphs.

Everything here edits source strings only, so the result is still compiled by Manim in one
pass. Matching is by token: `x` never matches inside `\\max`, and the arguments of text and
font macros (`\\text{max}`, `\\mathbf{x}`) are not searched.
"""

from __future__ import annotations

import re
from collections.abc import Mapping, Sequence
from dataclasses import dataclass

from .errors import CompositionError

_TOKEN = re.compile(r"\\(?:[A-Za-z@]+\*?|.)|\$\$|\s+|.", re.DOTALL)

# fmt: off
# Macros whose arguments are names or text, not math to search: {macro: arguments skipped}.
_OPAQUE = dict.fromkeys(
    (
        r"\text", r"\textrm", r"\textit", r"\textbf", r"\textsf", r"\texttt", r"\textnormal",
        r"\mbox", r"\hbox", r"\operatorname", r"\operatorname*", r"\mathrm", r"\mathbf",
        r"\mathit", r"\mathsf", r"\mathtt", r"\mathbb", r"\mathcal", r"\mathfrak", r"\mathscr",
        r"\boldsymbol", r"\bm", r"\label", r"\tag", r"\ref", r"\eqref", r"\begin", r"\end",
        r"\color", r"\textcolor", r"\special", r"\hspace", r"\vspace",
    ),
    1,
)
# Macros that may take their arguments unbraced (`\frac12`, `\hat x`): {macro: arity}.
_ARITY = {
    **_OPAQUE,
    **dict.fromkeys(
        (
            r"\frac", r"\dfrac", r"\tfrac", r"\cfrac", r"\binom", r"\dbinom", r"\tbinom",
            r"\overset", r"\underset", r"\stackrel", r"\textcolor",
        ),
        2,
    ),
    **dict.fromkeys(
        (
            r"\sqrt", r"\hat", r"\bar", r"\vec", r"\tilde", r"\dot", r"\ddot", r"\dddot",
            r"\check", r"\breve", r"\acute", r"\grave", r"\mathring", r"\widehat", r"\widetilde",
            r"\overline", r"\underline", r"\overbrace", r"\underbrace", r"\overrightarrow",
            r"\overleftarrow", r"\cancel", r"\boxed", r"\phantom", r"\not",
        ),
        1,
    ),
}
_OPTIONAL_ARG = frozenset({r"\sqrt", r"\color", r"\textcolor"})
_DELIMITED = frozenset(
    (
        r"\left", r"\right", r"\middle", r"\big", r"\Big", r"\bigg", r"\Bigg", r"\bigl", r"\bigr",
        r"\Bigl", r"\Bigr", r"\biggl", r"\biggr", r"\Biggl", r"\Biggr",
    )
)
# Operators whose class must survive a wrapper (only needed when scripts follow them).
_BINARY = frozenset(
    (
        "+", "-", "*", r"\times", r"\cdot", r"\pm", r"\mp", r"\div", r"\cup", r"\cap",
        r"\wedge", r"\vee", r"\oplus", r"\otimes", r"\circ", r"\ast", r"\star", r"\setminus",
    )
)
_LIMITS = frozenset(
    (
        r"\sum", r"\prod", r"\coprod", r"\bigcup", r"\bigcap", r"\bigoplus", r"\bigotimes",
        r"\bigodot", r"\biguplus", r"\bigsqcup", r"\bigvee", r"\bigwedge", r"\lim", r"\liminf",
        r"\limsup", r"\max", r"\min", r"\sup", r"\inf", r"\det", r"\gcd", r"\Pr",
    )
)
_NO_LIMITS = frozenset(
    (
        r"\int", r"\iint", r"\iiint", r"\oint", r"\sin", r"\cos", r"\tan", r"\cot", r"\sec",
        r"\csc", r"\arcsin", r"\arccos", r"\arctan", r"\sinh", r"\cosh", r"\tanh", r"\coth",
        r"\log", r"\ln", r"\lg", r"\exp", r"\arg", r"\deg", r"\dim", r"\hom", r"\ker",
    )
)
# fmt: on
_SCRIPT_MODIFIERS = frozenset({"'", r"\limits", r"\nolimits"})
_PUSH = re.compile(r"\\special\{color push [^}]*\}")
_POP = r"\special{color pop}"
_PAINT = re.compile(r"\\special\{color (?:push [^}]*|pop)\}")
# Top-level constructs that cannot be split into separately wrapped pieces.
_UNSPLITTABLE = frozenset({"&", "\\\\", r"\over", r"\atop", r"\choose", r"\above", r"\cr", "%"})
_MANIM_GROUP = re.compile(r"(?:^|\s)\{\{")

# fmt: off
RELATIONS = frozenset(
    (
        "=", "<", ">", r"\le", r"\leq", r"\ge", r"\geq", r"\leqslant", r"\geqslant", r"\ne",
        r"\neq", r"\not=", r"\approx", r"\equiv", r"\sim", r"\simeq", r"\cong", r"\propto",
        r"\ll", r"\gg", r"\to", r"\rightarrow", r"\Rightarrow", r"\Leftrightarrow", r"\iff",
        r"\implies", r"\mapsto", r"\coloneqq", r"\in", r"\subset", r"\subseteq",
    )
)
# fmt: on


@dataclass(frozen=True, slots=True)
class Token:
    text: str
    start: int
    end: int


class _Unsplittable(Exception):
    pass


def tokenize(tex: str) -> list[Token]:
    """Significant tokens; whitespace only separates them."""

    return [
        Token(m.group(), m.start(), m.end())
        for m in _TOKEN.finditer(tex)
        if not m.group().isspace()
    ]


def occurrences(tex: str, term: str, *, math_only: bool = False) -> list[tuple[int, int]]:
    """Character spans where `term` occurs in `tex`, compared token by token."""

    key = [token.text for token in tokenize(term)]
    if not key:
        raise CompositionError("A TeX term cannot be empty.")
    if _depth_error(key):
        raise CompositionError(f"The TeX term {term!r} has unbalanced braces.", term=term)
    tokens = tokenize(tex)
    searchable = _searchable(tokens, math_only)
    spans = []
    i = 0
    while i <= len(tokens) - len(key):
        window = tokens[i : i + len(key)]
        if (
            searchable[i]
            and [token.text for token in window] == key
            and not _splits_number(tokens, i, i + len(key))
        ):
            spans.append((window[0].start, window[-1].end))
            i += len(key)
        else:
            i += 1
    return spans


def paint(tex: str, spans: Sequence[tuple[tuple[int, int], str]]) -> str:
    """Wrap each `(start, end)` span (on token boundaries) in TeX color specials of its
    `#RRGGBB` color.

    Specials need no LaTeX package, and Manim keeps the glyph colors they produce. They are
    invisible to TeX's spacing, so a bare span keeps its atom class: a colored `=` is still a
    relation and a colored `\\sum` still takes limits. A span gets a wrapper only where TeX
    needs one atom: as an unbraced argument (`e^x`, `\\hat x`), or before scripts, where the
    wrapper keeps the operator's class (`\\mathop{..\\sum..}_{k=1}^n`). A delimiter after
    `\\left` and friends cannot be wrapped, so it stays uncolored.
    """

    tokens = tokenize(tex)
    first_at = {token.start: i for i, token in enumerate(tokens)}
    last_at = {token.end: i for i, token in enumerate(tokens)}
    arguments = _argument_starts(tokens)
    out, cursor = [], 0
    for (start, end), color in sorted(spans):
        if start < cursor:
            raise CompositionError("Colored TeX spans overlap.", tex=tex)
        first, last = first_at[start], last_at[end]
        if first > 0 and tokens[first - 1].text in _DELIMITED:
            continue
        red, green, blue = (int(color[i : i + 2], 16) / 255 for i in (1, 3, 5))
        body = rf"\special{{color push rgb {red:.4f} {green:.4f} {blue:.4f}}}{tex[start:end]}{_POP}"
        if first in arguments:
            body = "{" + body + "}"
        elif last + 1 < len(tokens) and tokens[last + 1].text in ("^", "_", *_SCRIPT_MODIFIERS):
            body = _nucleus(body, tokens[first].text if first == last else None)
        out += [tex[cursor:start], body]
        cursor = end
    out.append(tex[cursor:])
    return "".join(out)


def unpaint(tex: str) -> str:
    """`tex` without the color specials that `paint` added."""

    return _PAINT.sub("", tex)


def colorize(tex: str, colors: Mapping[str, str], *, math_only: bool = False) -> str:
    """Color every occurrence of each symbol; longer symbols win where they overlap."""

    claimed: list[tuple[tuple[int, int], str]] = []
    for symbol in sorted(colors, key=lambda s: -len(tokenize(s))):
        for span in occurrences(tex, symbol, math_only=math_only):
            if not any(span[0] < end and start < span[1] for (start, end), _ in claimed):
                claimed.append((span, colors[symbol]))
    return paint(tex, claimed) if claimed else tex


def atoms(tex: str) -> list[str]:
    """Split math TeX at the top level into atoms that TransformMatchingTex can match.

    An atom is a base (symbol, number, group, macro with its arguments, environment) with its
    scripts and primes. Returns `[tex]` when a split could change the typeset result.
    """

    if _MANIM_GROUP.search(tex):
        return [tex]
    tokens = tokenize(tex)
    if _depth_error([token.text for token in tokens]):
        return [tex]
    pieces = []
    try:
        i = 0
        while i < len(tokens):
            if tokens[i].text in _UNSPLITTABLE:
                return [tex]
            painted = _painted_end(tex, tokens, i)
            end = _attach_scripts(tokens, painted or _unit_end(tokens, i))
            pieces.append(tex[tokens[i].start : tokens[end - 1].end])
            i = end
    except _Unsplittable:
        return [tex]
    return pieces or [tex]


def upright_words(tex: str) -> str:
    """`tex` with each bare run of three or more letters (`area`, `slope`) wrapped in
    `\\text{}`, so it reads as a word rather than a product of variables; commands
    (`\\sin`) and the arguments of text macros are left alone."""

    tokens = tokenize(tex)
    searchable = _searchable(tokens, math_only=False)

    def letter(j: int) -> bool:
        return searchable[j] and len(tokens[j].text) == 1 and tokens[j].text.isalpha()

    out, cursor, i = [], 0, 0
    while i < len(tokens):
        j = i
        while j < len(tokens) and letter(j) and (j == i or tokens[j].start == tokens[j - 1].end):
            j += 1
        if j - i >= 3:
            start, end = tokens[i].start, tokens[j - 1].end
            out += [tex[cursor:start], rf"\text{{{tex[start:end]}}}"]
            cursor = end
        i = max(j, i + 1)
    return "".join([*out, tex[cursor:]])


def without_alignment(tex: str) -> str:
    """`tex` without its top-level `&` (an align* column mark), which would keep the step one
    unsplittable atom; inside environments (`pmatrix`) and groups `&` stays."""

    tokens = tokenize(tex)
    marks, i = [], 0
    try:
        while i < len(tokens):
            if tokens[i].text == "&":
                marks.append(tokens[i])
            i = _unit_end(tokens, i)
    except _Unsplittable:
        return tex
    for mark in reversed(marks):
        tex = tex[: mark.start] + tex[mark.end :]
    return tex


def is_relation(atom: str) -> bool:
    atom = unpaint(atom).strip()
    return atom in RELATIONS or atom.startswith((r"\xrightarrow", r"\xleftarrow"))


def _nucleus(body: str, token: str | None) -> str:
    """One atom of the span's own class for scripts to attach to (not `{..}`: a group is
    an ordinary atom, and `{{` could start Manim's double-brace notation)."""

    if token in _LIMITS:
        return rf"\mathop{{{body}}}"
    if token in _NO_LIMITS:
        return rf"\mathop{{{body}}}\nolimits"
    if token in RELATIONS:
        return rf"\mathrel{{{body}}}"
    if token in _BINARY:
        return rf"\mathbin{{{body}}}"
    return rf"\mathord{{{body}}}"


def _argument_starts(tokens: list[Token]) -> set[int]:
    """Indices of tokens that are an unbraced argument of a script or a macro (`\\frac12`)."""

    starts = set()
    for i, token in enumerate(tokens):
        try:
            if token.text in ("^", "_"):
                starts.add(i + 1)
            elif token.text in _ARITY:
                j = _arguments_end(tokens, i, 0)  # past an optional [..] argument
                for _ in range(_ARITY[token.text]):
                    starts.add(j)
                    j = _argument_end(tokens, j)
        except _Unsplittable:
            continue
    return starts


def _splits_number(tokens: list[Token], start: int, end: int) -> bool:
    """True when tokens[start:end] begins or ends inside a longer number such as `12.5`."""

    def numeric(i: int) -> bool:
        return 0 <= i < len(tokens) and (tokens[i].text.isdigit() or tokens[i].text == ".")

    return (numeric(start) and numeric(start - 1)) or (numeric(end - 1) and numeric(end))


def _searchable(tokens: list[Token], math_only: bool) -> list[bool]:
    mask = [False] * len(tokens)
    in_math = not math_only
    i = 0
    while i < len(tokens):
        text = tokens[i].text
        if math_only and text in ("$", "$$", r"\(", r"\)", r"\[", r"\]"):
            in_math = not in_math if text in ("$", "$$") else text in (r"\(", r"\[")
            i += 1
            continue
        mask[i] = in_math
        if text in _OPAQUE:
            try:
                i = _arguments_end(tokens, i, _OPAQUE[text])
            except _Unsplittable:
                break
            continue
        i += 1
    return mask


def _painted_end(tex: str, tokens: list[Token], i: int) -> int | None:
    """A span painted without a wrapper is one unit: from its push to the matching pop."""

    if tokens[i].text != r"\special" or not _PUSH.match(tex, tokens[i].start):
        return None
    depth = 0
    for special in _PAINT.finditer(tex, tokens[i].start):
        depth += -1 if special.group() == _POP else 1
        if depth == 0:
            return next(
                (k for k in range(i, len(tokens)) if tokens[k].start >= special.end()), len(tokens)
            )
    raise _Unsplittable


def _unit_end(tokens: list[Token], i: int) -> int:
    text = tokens[i].text
    if text == "{":
        return _group_end(tokens, i)
    if text == r"\begin":
        return _environment_end(tokens, i)
    if text in _DELIMITED:
        if i + 1 >= len(tokens):
            raise _Unsplittable
        return i + 2
    if text in ("^", "_"):
        return _argument_end(tokens, i + 1)
    if text in _ARITY:
        return _arguments_end(tokens, i, _ARITY[text])
    if text.startswith("\\"):
        i += 1
        while i < len(tokens) and tokens[i].text == "{":
            i = _group_end(tokens, i)
        return i
    if text.isdigit():
        i += 1
        while i < len(tokens) and (tokens[i].text.isdigit() or tokens[i].text == "."):
            i += 1
        return i
    return i + 1


def _attach_scripts(tokens: list[Token], i: int) -> int:
    while i < len(tokens):
        text = tokens[i].text
        if text in ("^", "_"):
            i = _argument_end(tokens, i + 1)
        elif text in _SCRIPT_MODIFIERS:
            i += 1
        else:
            break
    return i


def _arguments_end(tokens: list[Token], i: int, count: int) -> int:
    macro, i = tokens[i].text, i + 1
    if macro in _OPTIONAL_ARG and i < len(tokens) and tokens[i].text == "[":
        while i < len(tokens) and tokens[i].text != "]":
            i += 1
        i += 1
    for _ in range(count):
        i = _argument_end(tokens, i)
    return i


def _argument_end(tokens: list[Token], i: int) -> int:
    if i >= len(tokens):
        raise _Unsplittable
    return _group_end(tokens, i) if tokens[i].text == "{" else i + 1


def _group_end(tokens: list[Token], i: int) -> int:
    depth = 0
    for j in range(i, len(tokens)):
        depth += {"{": 1, "}": -1}.get(tokens[j].text, 0)
        if depth == 0:
            return j + 1
    raise _Unsplittable


def _environment_end(tokens: list[Token], i: int) -> int:
    depth = 0
    for j in range(i, len(tokens)):
        depth += {r"\begin": 1, r"\end": -1}.get(tokens[j].text, 0)
        if depth == 0:
            return _argument_end(tokens, j + 1)
    raise _Unsplittable


def _depth_error(texts: list[str]) -> bool:
    depth = 0
    for text in texts:
        depth += {"{": 1, "}": -1}.get(text, 0)
        if depth < 0:
            return True
    return depth != 0
