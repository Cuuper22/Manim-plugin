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
