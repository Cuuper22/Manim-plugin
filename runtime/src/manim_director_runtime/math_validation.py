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
import signal
import sys
import threading
from collections.abc import Callable, Iterator, Mapping
from contextlib import contextmanager
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
_ONE_ARGUMENT = (1, 1, "one argument")
_ARITY = {
    "log": (1, 2, "one or two arguments"),
    "min": (2, math.inf, "two or more arguments"),
    "max": (2, math.inf, "two or more arguments"),
}
_CONSTANTS = ("pi", "e", "tau")
_TOKEN = re.compile(r"[A-Za-z_]\w*|.", re.DOTALL)
_KEYWORD_PREFIX = "_reserved_"
# A float sample is off only by more than round-off: 32 ulp of the largest value met on the way.
_ROUND_OFF = 32 * sys.float_info.epsilon
_WITNESS = 1e-30  # an exact difference at least this large at a sampled point disproves a step
_MAX_POWER_BITS = 100_000  # larger exact powers (10^10^10) are left to the numeric check
_SYMBOLIC_SECONDS = 20.0
_WITNESS_POINTS = 8


@dataclass(frozen=True, slots=True)
class Symbolic:
    available: bool
    equivalent: bool | None
    difference: str | None


@dataclass(frozen=True, slots=True)
class Counterexample:
    """The first failing sample; a side is null where it has no real value (the other has)."""

    variables: dict[str, float]
    left: float | None
    right: float | None


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
        numeric, points = _numeric(left, right, ranges, task.samples, task.tolerance, task.seed)
        symbolic = _symbolic(left, right, ranges, points)
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
    if isinstance(node, ast.Constant):
        if isinstance(node.value, bool) or not isinstance(node.value, (int, float)):
            return "only numbers are allowed as literals"
        if isinstance(node.value, float) and not math.isfinite(node.value):
            return "the number is too large"
    if isinstance(node, ast.Call):
        if not isinstance(node.func, ast.Name) or node.func.id in _CONSTANTS:
            return "implicit multiplication is not supported; write the * explicitly"
        name = node.func.id
        if name not in _FUNCTIONS:
            return f"unknown function {name}; write a*(...) for multiplication"
        if node.keywords:
            return "functions take positional arguments only"
        low, high, wanted = _ARITY.get(name, _ONE_ARGUMENT)
        if not low <= len(node.args) <= high:
            return f"{name} takes {wanted}"
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
    seen: Callable[[Any], Any] = lambda value: value,
) -> Any:
    """Fold a validated step tree with one arithmetic backend (floats or SymPy); `seen` sees
    every intermediate value. Iterative, so deep nesting (`- - - x`) cannot overflow the stack."""

    values: list[Any] = []
    stack: list[tuple[ast.expr, bool]] = [(tree, False)]
    while stack:
        node, operands_done = stack.pop()
        if isinstance(node, (ast.Name, ast.Constant)):
            values.append(seen(leaf(node)))
            continue
        operands = (
            [node.operand]
            if isinstance(node, ast.UnaryOp)
            else [node.left, node.right]
            if isinstance(node, ast.BinOp)
            else node.args
        )
        if not operands_done:
            stack.append((node, True))
            stack += [(operand, False) for operand in reversed(operands)]
            continue
        args = values[len(values) - len(operands) :]
        del values[len(values) - len(operands) :]
        if isinstance(node, ast.UnaryOp):
            values.append(-args[0] if isinstance(node.op, ast.USub) else args[0])
        elif isinstance(node, ast.BinOp):
            values.append(seen(ops[type(node.op)](*args)))
        else:
            assert isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
            values.append(seen(call(node.func.id, args)))
    return values[0]


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


def evaluate(tree: ast.expr, values: Mapping[str, float]) -> tuple[float, float]:
    """Evaluate with floats: the value and the largest magnitude met on the way (it bounds the
    round-off). Raises ValueError or ZeroDivisionError outside the domain and OverflowError
    when a value is not finite."""

    largest = 0.0

    def leaf(node: ast.Name | ast.Constant) -> float:
        if isinstance(node, ast.Constant):
            return float(node.value)
        return _FLOAT_CONSTANTS[node.id] if node.id in _FLOAT_CONSTANTS else values[node.id]

    def seen(value: float) -> float:
        nonlocal largest
        if not math.isfinite(value):
            raise OverflowError("the value is not finite")
        largest = max(largest, abs(value))
        return value

    def call(name: str, args: list[float]) -> float:
        return _FLOAT_FUNCTIONS[name](*args)

    return float(_build(tree, leaf, _FLOAT_OPS, call, seen)), largest


def _side(tree: ast.expr, point: Mapping[str, float]) -> tuple[float, float] | str:
    try:
        return evaluate(tree, point)
    except OverflowError:
        return "overflow"
    except (ValueError, ZeroDivisionError):
        return "undefined"


def _numeric(
    left: ast.expr,
    right: ast.expr,
    ranges: Mapping[str, tuple[float, float]],
    samples: int,
    tolerance: float,
    seed: int,
) -> tuple[Numeric, list[dict[str, float]]]:
    """Compare the steps at seeded random points; also returns points where both are defined
    (the counterexample first) for SymPy to test a disproof against."""

    rng = random.Random(seed)
    valid = skipped = 0
    max_abs = max_rel = 0.0
    mismatch = gap = None
    points: list[dict[str, float]] = []
    for _ in range(samples):
        point = {name: rng.uniform(low, high) for name, (low, high) in ranges.items()}
        a, b = _side(left, point), _side(right, point)
        defined = [isinstance(side, tuple) for side in (a, b)]
        if not all(defined):
            skipped += 1
            if gap is None and any(defined) and "undefined" in (a, b):
                # Defined on one side only: a branch or a real root the other step lost.
                values = [side[0] if isinstance(side, tuple) else None for side in (a, b)]
                gap = Counterexample(point, *values)
            continue
        valid += 1
        (a, a_largest), (b, b_largest) = a, b
        absolute = abs(a - b)
        scale = max(abs(a), abs(b))
        relative = absolute / scale if scale else 0.0
        max_abs, max_rel = max(max_abs, absolute), max(max_rel, relative)
        excess = absolute - _ROUND_OFF * max(a_largest, b_largest)
        if mismatch is None and excess > tolerance and excess > tolerance * scale:
            mismatch = Counterexample(variables=point, left=a, right=b)
            points.insert(0, point)
        elif len(points) < _WITNESS_POINTS:
            points.append(point)
    numeric = Numeric(valid, skipped, max_abs, max_rel, mismatch or gap)
    return numeric, points[:_WITNESS_POINTS]


class _Undecided(BaseException):
    """SymPy cannot answer within reason. A BaseException, so SymPy's own handlers let it pass."""


def _symbolic(
    left: ast.expr,
    right: ast.expr,
    ranges: Mapping[str, tuple[float, float]],
    points: list[dict[str, float]],
) -> Symbolic:
    try:
        import sympy
    except ImportError:
        return Symbolic(available=False, equivalent=None, difference=None)

    # Ranges become assumptions, so log(a*b) = log(a) + log(b) holds when a, b > 0.
    symbols = {name: _symbol(sympy, name, low, high) for name, (low, high) in ranges.items()}
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
    ops = {**_FLOAT_OPS, ast.Mod: sympy.Mod, ast.Pow: _exact_power}

    def leaf(node: ast.Name | ast.Constant) -> Any:
        if isinstance(node, ast.Constant):
            # Rational keeps 0.1 exact, so 0.1*x - x/10 simplifies to 0.
            return sympy.Rational(repr(node.value))
        return constants[node.id] if node.id in constants else symbols[node.id]

    def build(tree: ast.expr) -> Any:
        return _build(tree, leaf, ops, lambda n, a: functions[n](*a))

    try:
        with _time_limit(_SYMBOLIC_SECONDS):
            difference = sympy.simplify(build(left) - build(right))
            if difference == 0:
                return Symbolic(available=True, equivalent=True, difference="0")
            if difference.has(sympy.nan, sympy.zoo, sympy.oo, -sympy.oo):
                proof = None  # a step is undefined somewhere, so the difference proves nothing
            else:
                proof = difference.equals(0)
            # equals(0) is a heuristic and can say False for a true identity (atan(x) +
            # atan(1/x) = pi/2 for x > 0): a disproof needs a point in range that shows it.
            if proof is False and not _witnessed(difference, symbols, points):
                proof = None
            text = str(difference)[:2000]
    except (_Undecided, RecursionError, TypeError, ValueError, ArithmeticError, sympy.SympifyError):
        return Symbolic(available=True, equivalent=None, difference=None)
    return Symbolic(available=True, equivalent=proof, difference=text)


def _symbol(sympy: Any, name: str, low: float, high: float) -> Any:
    if low > 0:
        return sympy.Symbol(name, positive=True)
    if low >= 0:
        return sympy.Symbol(name, nonnegative=True)
    if high < 0:
        return sympy.Symbol(name, negative=True)
    if high <= 0:
        return sympy.Symbol(name, nonpositive=True)
    return sympy.Symbol(name, real=True)


def _exact_power(base: Any, exponent: Any) -> Any:
    """`base ** exponent`, refusing exact numbers too large to write down (10^10^10)."""

    if base.is_Rational and exponent.is_Rational and base not in (0, 1, -1):
        bits = max(abs(base.p).bit_length(), base.q.bit_length())
        if abs(exponent) * bits > _MAX_POWER_BITS:
            raise _Undecided
    return base**exponent


def _witnessed(difference: Any, symbols: Mapping[str, Any], points: list[dict[str, float]]) -> bool:
    """Whether the exact difference is clearly non-zero and real at one of the points."""

    for point in points:
        try:
            value = difference.evalf(50, subs={symbols[name]: v for name, v in point.items()})
            if value.is_extended_real and value.is_finite and abs(float(value)) > _WITNESS:
                return True
        except (TypeError, ValueError, ArithmeticError):
            continue
    return False


@contextmanager
def _time_limit(seconds: float) -> Iterator[None]:
    """Raise _Undecided after `seconds` (a SIGALRM timer, so main thread and POSIX only)."""

    if (
        not hasattr(signal, "setitimer")
        or threading.current_thread() is not threading.main_thread()
    ):
        yield
        return

    def expire(signum: int, frame: Any) -> NoReturn:
        raise _Undecided

    previous = signal.signal(signal.SIGALRM, expire)
    signal.setitimer(signal.ITIMER_REAL, seconds)
    try:
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def _verdict(symbolic: Symbolic, numeric: Numeric) -> bool | None:
    """SymPy decides when it can (a simplification to 0 is a proof; a disproof is witnessed);
    otherwise a value mismatch is False, a step defined where the other is not is undecided,
    and samples that all agree are True."""

    if symbolic.equivalent is not None:
        return symbolic.equivalent
    example = numeric.counterexample
    if example is not None:
        return False if None not in (example.left, example.right) else None
    return True if numeric.samples_valid > 0 else None
