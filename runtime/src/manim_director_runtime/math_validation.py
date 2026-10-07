"""The `validate_math` operation: are consecutive derivation steps equal?

Each step is parsed once into a whitelisted AST and then evaluated two ways: exactly with
SymPy (when installed) and numerically at seeded random points. No step is ever `eval`ed.
"""

from __future__ import annotations

import ast
import keyword
import math
import operator
import random
import re
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from itertools import pairwise
from typing import TYPE_CHECKING, Any, NoReturn

from .errors import DirectorError
from .model import RuntimeArtifact
from .tasks import ValidateMathTask

if TYPE_CHECKING:
    from .protocol import Context

DEFAULT_RANGE = (-10.0, 10.0)
_FUNCTIONS = (
    "abs",
    "sin",
    "cos",
    "tan",
    "asin",
    "acos",
    "atan",
    "sinh",
    "cosh",
    "tanh",
    "sqrt",
    "exp",
    "log",
    "ln",
    "log10",
    "floor",
    "ceil",
    "min",
    "max",
)
_CONSTANTS = ("pi", "e", "tau")
_TOKEN = re.compile(r"[A-Za-z_]\w*|.", re.DOTALL)
_KEYWORD_PREFIX = "_reserved_"


@dataclass(frozen=True, slots=True)
class Symbolic:
    available: bool
    equivalent: bool | None
    difference: str | None


@dataclass(frozen=True, slots=True)
class Counterexample:
    variables: dict[str, float]
    left: float
    right: float


@dataclass(frozen=True, slots=True)
class Numeric:
    samples_valid: int
    samples_skipped: int
    max_abs_error: float
    max_rel_error: float
    counterexample: Counterexample | None


@dataclass(frozen=True, slots=True)
class Pair:
    index: int
    equivalent: bool | None
    symbolic: Symbolic
    numeric: Numeric


@dataclass(frozen=True, slots=True)
class ValidateMathResult:
    valid: bool | None
    variables: list[str]
    pairs: list[Pair]
    artifacts: list[RuntimeArtifact]


def validate_math(task: ValidateMathTask, ctx: Context) -> ValidateMathResult:
    trees = [parse_step(step, index) for index, step in enumerate(task.steps)]
    variables = sorted(set().union(*(_variables(tree) for tree in trees)))
    ranges = {name: task.ranges.get(name, DEFAULT_RANGE) for name in variables}
    pairs = []
    for index, (left, right) in enumerate(pairwise(trees)):
        symbolic = _symbolic(left, right, ranges)
        numeric = _numeric(left, right, ranges, task.samples, task.tolerance, task.seed)
        pairs.append(Pair(index, _verdict(symbolic, numeric), symbolic, numeric))
    verdicts = [pair.equivalent for pair in pairs]
    valid = False if False in verdicts else None if None in verdicts else True
    return ValidateMathResult(valid=valid, variables=variables, pairs=pairs, artifacts=[])


def parse_step(text: str, index: int) -> ast.expr:
    """Parse one step, mapping error columns back to the original text (1-based)."""

    if "=" in text:
        _invalid(
            index, text.index("=") + 1, "a step is an expression; write each side as its own step"
        )
    source, origin = _python_source(text)
    try:
        tree = ast.parse(source, mode="eval").body
    except SyntaxError as exc:
        column = origin[min(max((exc.offset or 1) - 1, 0), len(origin) - 1)] + 1
        _invalid(index, column, "it does not parse; write multiplication explicitly, like 2*x")
    for node in ast.walk(tree):
        if isinstance(node, ast.Name):
            node.id = node.id.removeprefix(_KEYWORD_PREFIX)
        problem = _unsupported(node)
        if problem:
            column = origin[min(getattr(node, "col_offset", 0), len(origin) - 1)] + 1
            _invalid(index, column, problem)
    return tree


def _python_source(text: str) -> tuple[str, list[int]]:
    """`^` becomes `**`, and names Python reserves (`lambda`) get a prefix that the parsed
    tree drops again; `origin[i]` is the index in `text` of source character `i`."""

    chars: list[str] = []
    origin: list[int] = []
    for match in _TOKEN.finditer(text):
        start, token = match.start(), match.group()
        if token == "^":
            chars.append("**")
            origin += [start, start]
            continue
        if keyword.iskeyword(token):
            chars.append(_KEYWORD_PREFIX)
            origin += [start] * len(_KEYWORD_PREFIX)
        chars.append(token)
        origin += range(start, match.end())
    return "".join(chars), origin or [0]


def _unsupported(node: ast.AST) -> str | None:
    allowed = (
        ast.Expression,
        ast.BinOp,
        ast.UnaryOp,
        ast.Constant,
        ast.Name,
        ast.Load,
        ast.Call,
        ast.Add,
        ast.Sub,
        ast.Mult,
        ast.Div,
        ast.Mod,
        ast.Pow,
        ast.UAdd,
        ast.USub,
    )
    if not isinstance(node, allowed):
        return f"{type(node).__name__} is not part of the expression language"
    if isinstance(node, ast.Constant) and (
        isinstance(node.value, bool) or not isinstance(node.value, (int, float))
    ):
        return "only numbers are allowed as literals"
    if isinstance(node, ast.Call):
        if not isinstance(node.func, ast.Name) or node.func.id in _CONSTANTS:
            return "implicit multiplication is not supported; write the * explicitly"
        if node.func.id not in _FUNCTIONS:
            return f"unknown function {node.func.id}; write a*(...) for multiplication"
        if node.keywords:
            return "functions take positional arguments only"
    return None


def _invalid(index: int, column: int, reason: str) -> NoReturn:
    raise DirectorError(
        "invalid_expression",
        f"Step {index + 1} is not a valid expression at column {column}: {reason}.",
        {"step": index, "column": column},
    )


def _variables(tree: ast.expr) -> set[str]:
    called = {id(n.func) for n in ast.walk(tree) if isinstance(n, ast.Call)}
    return {
        node.id
        for node in ast.walk(tree)
        if isinstance(node, ast.Name) and id(node) not in called and node.id not in _CONSTANTS
    }


def _build(
    tree: ast.expr,
    leaf: Callable[[ast.Name | ast.Constant], Any],
    ops: Mapping[Any, Any],
    call: Callable[[str, list[Any]], Any],
) -> Any:
    """Fold a validated step tree with one arithmetic backend (floats or SymPy)."""

    if isinstance(tree, (ast.Name, ast.Constant)):
        return leaf(tree)
    if isinstance(tree, ast.UnaryOp):
        operand = _build(tree.operand, leaf, ops, call)
        return -operand if isinstance(tree.op, ast.USub) else operand
    if isinstance(tree, ast.BinOp):
        left = _build(tree.left, leaf, ops, call)
        right = _build(tree.right, leaf, ops, call)
        return ops[type(tree.op)](left, right)
    assert isinstance(tree, ast.Call) and isinstance(tree.func, ast.Name)
    return call(tree.func.id, [_build(arg, leaf, ops, call) for arg in tree.args])


_FLOAT_OPS = {
    ast.Add: operator.add,
    ast.Sub: operator.sub,
    ast.Mult: operator.mul,
    ast.Div: operator.truediv,
    ast.Mod: operator.mod,
    # math.pow, unlike **, refuses to leave the reals: (-8)^(1/3) is outside the domain.
    ast.Pow: math.pow,
}
_FLOAT_FUNCTIONS: dict[str, Callable[..., float]] = {
    "abs": abs,
    "sin": math.sin,
    "cos": math.cos,
    "tan": math.tan,
    "asin": math.asin,
    "acos": math.acos,
    "atan": math.atan,
    "sinh": math.sinh,
    "cosh": math.cosh,
    "tanh": math.tanh,
    "sqrt": math.sqrt,
    "exp": math.exp,
    "log": math.log,
    "ln": math.log,
    "log10": math.log10,
    "floor": math.floor,
    "ceil": math.ceil,
    "min": min,
    "max": max,
}
_FLOAT_CONSTANTS = {"pi": math.pi, "e": math.e, "tau": math.tau}


def evaluate(tree: ast.expr, values: Mapping[str, float]) -> float:
    """Evaluate with floats; raises ArithmeticError/ValueError/TypeError outside the domain."""

    def leaf(node: ast.Name | ast.Constant) -> float:
        if isinstance(node, ast.Constant):
            return float(node.value)
        return _FLOAT_CONSTANTS[node.id] if node.id in _FLOAT_CONSTANTS else values[node.id]

    result = _build(tree, leaf, _FLOAT_OPS, lambda name, args: _FLOAT_FUNCTIONS[name](*args))
    if not math.isfinite(result):
        raise ValueError("the value is not finite")
    return float(result)


def _numeric(
    left: ast.expr,
    right: ast.expr,
    ranges: Mapping[str, tuple[float, float]],
    samples: int,
    tolerance: float,
    seed: int,
) -> Numeric:
    rng = random.Random(seed)
    valid = skipped = 0
    max_abs = max_rel = 0.0
    counterexample = None
    for _ in range(samples):
        point = {name: rng.uniform(low, high) for name, (low, high) in ranges.items()}
        try:
            a, b = evaluate(left, point), evaluate(right, point)
        except (ArithmeticError, ValueError, TypeError):
            skipped += 1
            continue
        valid += 1
        absolute = abs(a - b)
        scale = max(abs(a), abs(b))
        relative = absolute / scale if scale else 0.0
        max_abs, max_rel = max(max_abs, absolute), max(max_rel, relative)
        if counterexample is None and absolute > tolerance and relative > tolerance:
            counterexample = Counterexample(variables=point, left=a, right=b)
    return Numeric(valid, skipped, max_abs, max_rel, counterexample)


def _symbolic(
    left: ast.expr, right: ast.expr, ranges: Mapping[str, tuple[float, float]]
) -> Symbolic:
    try:
        import sympy
    except ImportError:
        return Symbolic(available=False, equivalent=None, difference=None)

    # Ranges become assumptions, so log(a*b) = log(a) + log(b) holds when a, b > 0.
    symbols = {
        name: sympy.Symbol(name, positive=True)
        if low > 0
        else sympy.Symbol(name, nonnegative=True)
        if low >= 0
        else sympy.Symbol(name, real=True)
        for name, (low, high) in ranges.items()
    }
    constants = {"pi": sympy.pi, "e": sympy.E, "tau": 2 * sympy.pi}
    functions: dict[str, Callable[..., Any]] = {
        "abs": sympy.Abs,
        "sin": sympy.sin,
        "cos": sympy.cos,
        "tan": sympy.tan,
        "asin": sympy.asin,
        "acos": sympy.acos,
        "atan": sympy.atan,
        "sinh": sympy.sinh,
        "cosh": sympy.cosh,
        "tanh": sympy.tanh,
        "sqrt": sympy.sqrt,
        "exp": sympy.exp,
        "log": sympy.log,
        "ln": sympy.log,
        "log10": lambda x: sympy.log(x, 10),
        "floor": sympy.floor,
        "ceil": sympy.ceiling,
        "min": sympy.Min,
        "max": sympy.Max,
    }
    ops = {**_FLOAT_OPS, ast.Mod: sympy.Mod, ast.Pow: operator.pow}

    def leaf(node: ast.Name | ast.Constant) -> Any:
        if isinstance(node, ast.Constant):
            # Rational keeps 0.1 exact, so 0.1*x - x/10 simplifies to 0.
            return sympy.Rational(repr(node.value))
        return constants[node.id] if node.id in constants else symbols[node.id]

    try:
        difference = sympy.simplify(
            _build(left, leaf, ops, lambda n, a: functions[n](*a))
            - _build(right, leaf, ops, lambda n, a: functions[n](*a))
        )
        if difference == 0:
            return Symbolic(available=True, equivalent=True, difference="0")
        proof = difference.equals(0)
    except (TypeError, ValueError, ArithmeticError, sympy.SympifyError):
        return Symbolic(available=True, equivalent=None, difference=None)
    equivalent = True if proof is True else False if proof is False else None
    return Symbolic(available=True, equivalent=equivalent, difference=str(difference)[:2000])


def _verdict(symbolic: Symbolic, numeric: Numeric) -> bool | None:
    if symbolic.equivalent is False or numeric.counterexample is not None:
        return False
    if symbolic.equivalent is True or numeric.samples_valid > 0:
        return True
    return None
