from __future__ import annotations

import pytest

from conftest import as_json
from manim_director_runtime.errors import DirectorError
from manim_director_runtime.math_validation import validate_math
from manim_director_runtime.tasks import ValidateMathTask


def check(ctx, *steps: str, ranges=None, samples: int = 50) -> dict:
    task = ValidateMathTask(
        steps=list(steps), ranges=ranges or {}, samples=samples, tolerance=1e-9, seed=1729
    )
    return as_json(validate_math(task, ctx))


def test_a_correct_derivation_is_valid(ctx) -> None:
    result = check(ctx, "(a+b)^2", "a^2 + 2*a*b + b^2", "(b+a)**2")
    assert result["valid"] is True
    assert result["variables"] == ["a", "b"]
    first = result["pairs"][0]
    assert first["equivalent"] is True
    assert first["symbolic"] == {"available": True, "equivalent": True, "difference": "0"}
    assert first["numeric"]["samples_valid"] == 50 and first["numeric"]["counterexample"] is None


def test_reserved_python_words_work_as_variable_names(ctx) -> None:
    result = check(ctx, "lambda^2 - p*lambda", "lambda*(lambda - p)", ranges={"lambda": [0, 3]})
    assert result["valid"] is True and result["variables"] == ["lambda", "p"]
    with pytest.raises(DirectorError) as raised:
        check(ctx, "lambda^2", "lambda lambda")
    assert raised.value.data == {"step": 1, "column": 8}


def test_a_wrong_step_is_pinpointed_with_a_counterexample(ctx) -> None:
    result = check(ctx, "x^2 - 1", "(x-1)*(x+1)", "(x-1)^2")
    assert result["valid"] is False
    assert [pair["equivalent"] for pair in result["pairs"]] == [True, False]
    bad = result["pairs"][1]
    assert bad["symbolic"]["equivalent"] is False
    example = bad["numeric"]["counterexample"]
    assert set(example) == {"variables", "left", "right"} and "x" in example["variables"]


def test_ranges_become_symbolic_assumptions(ctx) -> None:
    unrestricted = check(ctx, "log(a*b)", "log(a) + log(b)")
    assert unrestricted["pairs"][0]["symbolic"]["equivalent"] is not True
    positive = check(ctx, "log(a*b)", "log(a) + log(b)", ranges={"a": (0.1, 5), "b": (0.1, 5)})
    assert positive["valid"] is True
    assert positive["pairs"][0]["symbolic"]["equivalent"] is True


def test_decimal_literals_are_exact(ctx) -> None:
    assert check(ctx, "0.1*x", "x/10")["pairs"][0]["symbolic"]["equivalent"] is True


@pytest.mark.parametrize(
    ("steps", "step", "column"),
    [
        (["2x", "x+x"], 0, 1),
        (["x", "2(x+1)"], 1, 1),
        (["a = b", "b"], 0, 3),
        (["x", "x^2 + __import__('os')"], 1, 7),
        (["x", "sin(x).real"], 1, 1),
    ],
)
def test_invalid_expressions_report_step_and_column(ctx, steps, step, column) -> None:
    with pytest.raises(DirectorError) as raised:
        check(ctx, *steps)
    assert raised.value.code == "invalid_expression"
    assert raised.value.data == {"step": step, "column": column}


def test_domain_failures_are_skipped_not_counted(ctx) -> None:
    result = check(ctx, "sqrt(x)^2", "x", ranges={"x": (-1, 1)}, samples=40)
    numeric = result["pairs"][0]["numeric"]
    assert numeric["samples_skipped"] > 0
    assert numeric["samples_valid"] + numeric["samples_skipped"] == 40


def test_powers_stay_real(ctx) -> None:
    # Python's ** would return a complex root for x < 3, and abs() would make it look real.
    result = check(ctx, "abs((x - 3)^0.5)", "abs(x - 3)^0.5", ranges={"x": (-1, 5)}, samples=40)
    numeric = result["pairs"][0]["numeric"]
    assert numeric["samples_skipped"] > 0
    assert numeric["samples_valid"] + numeric["samples_skipped"] == 40


@pytest.mark.parametrize(
    "steps",
    [("cosh(x)^2 - sinh(x)^2", "1"), ("exp(x)^2 - exp(2*x) + x", "x"), ("x^10 - (x^5)^2", "0")],
)
def test_float_round_off_is_not_a_counterexample(ctx, steps) -> None:
    pair = check(ctx, *steps, samples=200)["pairs"][0]
    assert pair["equivalent"] is True and pair["numeric"]["counterexample"] is None


@pytest.mark.parametrize(
    ("steps", "ranges"),
    [
        (("atan(x) + atan(1/x)", "pi/2"), {"x": (0.1, 10)}),  # SymPy's equals(0) says False
        (("sqrt((x-1)^2)", "x - 1"), {"x": (1, 5)}),
        (("sqrt(x^2)", "-x"), {"x": (-5, -1)}),
    ],
)
def test_an_unwitnessed_symbolic_disproof_does_not_reject_an_identity(ctx, steps, ranges) -> None:
    pair = check(ctx, *steps, ranges=ranges)["pairs"][0]
    assert pair["equivalent"] is True and pair["symbolic"]["equivalent"] is not False


def test_a_difference_below_the_tolerance_is_still_disproved(ctx) -> None:
    pair = check(ctx, "x + 0.0000000001", "x")["pairs"][0]
    assert pair["numeric"]["counterexample"] is None
    assert pair["symbolic"]["equivalent"] is False and pair["equivalent"] is False


def test_a_step_defined_where_the_other_is_not_is_undecided(ctx) -> None:
    pair = check(ctx, "x^(1/3)", "abs(x)^(1/3)")["pairs"][0]
    assert pair["equivalent"] is None
    example = pair["numeric"]["counterexample"]
    assert example["left"] is None and example["right"] > 0 and example["variables"]["x"] < 0


def test_undefined_steps_and_huge_powers_are_undecided_not_hung(ctx) -> None:
    assert check(ctx, "1/0", "2/0")["valid"] is None
    assert check(ctx, "10^10^10", "1")["valid"] is None


def test_deep_nesting_is_folded_without_recursion(ctx) -> None:
    assert check(ctx, "-" * 1500 + "x", "x")["valid"] is True
    assert check(ctx, "+".join(["x"] * 600), "600*x")["valid"] is True


@pytest.mark.parametrize(
    ("step", "reason"),
    [
        ("sin(x, y)", "sin takes one argument"),
        ("min()", "min takes two or more arguments"),
        ("log(x, 2, 3)", "log takes one or two arguments"),
        ("1e400", "the number is too large"),
    ],
)
def test_arity_and_literals_are_checked(ctx, step, reason) -> None:
    with pytest.raises(DirectorError) as raised:
        check(ctx, step, "1")
    assert raised.value.code == "invalid_expression" and reason in raised.value.message
