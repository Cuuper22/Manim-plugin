from __future__ import annotations

import ast
import math
import random
import re
from typing import Any, Mapping

from .errors import DirectorError


FUNCTIONS = {
    "abs": abs, "sin": math.sin, "cos": math.cos, "tan": math.tan,
    "asin": math.asin, "acos": math.acos, "atan": math.atan,
    "sinh": math.sinh, "cosh": math.cosh, "tanh": math.tanh,
    "sqrt": math.sqrt, "exp": math.exp, "log": math.log, "ln": math.log,
    "log10": math.log10, "floor": math.floor, "ceil": math.ceil,
    "min": min, "max": max,
}
CONSTANTS = {"pi": math.pi, "e": math.e, "tau": math.tau}
BINOPS = {ast.Add: lambda a, b: a + b, ast.Sub: lambda a, b: a - b, ast.Mult: lambda a, b: a * b, ast.Div: lambda a, b: a / b, ast.Pow: lambda a, b: a ** b, ast.Mod: lambda a, b: a % b}
UNARYOPS = {ast.UAdd: lambda a: a, ast.USub: lambda a: -a}


def _normalize_expression(value: str) -> str:
    value = value.strip().replace("^", "**")
    if "=" in value and not any(op in value for op in ("==", "<=", ">=", "!=")):
        left, right = value.split("=", 1)
        value = f"({left})-({right})"
    return value


def _tree(expression: str) -> ast.Expression:
    try:
        tree = ast.parse(_normalize_expression(expression), mode="eval")
    except SyntaxError as exc:
        raise DirectorError("invalid_expression", f"Invalid mathematical expression: {expression}", {"line": exc.lineno, "column": exc.offset}) from exc
    allowed = (ast.Expression, ast.Constant, ast.Name, ast.Load, ast.BinOp, ast.UnaryOp, *BINOPS.keys(), *UNARYOPS.keys(), ast.Call)
    for node in ast.walk(tree):
        if not isinstance(node, allowed):
            raise DirectorError("unsupported_expression", f"Unsupported mathematical syntax: {type(node).__name__}")
        if isinstance(node, ast.Constant) and not isinstance(node.value, (int, float)):
            raise DirectorError("unsupported_expression", "Only numeric constants are allowed")
        if isinstance(node, ast.Call) and (not isinstance(node.func, ast.Name) or node.func.id not in FUNCTIONS or node.keywords):
            raise DirectorError("unsupported_expression", "Only approved mathematical function calls are allowed")
    return tree


def _names(tree: ast.AST) -> set[str]:
    return {node.id for node in ast.walk(tree) if isinstance(node, ast.Name) and node.id not in FUNCTIONS and node.id not in CONSTANTS}


def _evaluate_node(node: ast.AST, variables: Mapping[str, float]) -> float:
    if isinstance(node, ast.Expression):
        return _evaluate_node(node.body, variables)
    if isinstance(node, ast.Constant) and isinstance(node.value, (int, float)):
        return float(node.value)
    if isinstance(node, ast.Name):
        if node.id in variables:
            return float(variables[node.id])
        if node.id in CONSTANTS:
            return CONSTANTS[node.id]
        raise ValueError(f"unknown variable {node.id}")
    if isinstance(node, ast.BinOp) and type(node.op) in BINOPS:
        return float(BINOPS[type(node.op)](_evaluate_node(node.left, variables), _evaluate_node(node.right, variables)))
    if isinstance(node, ast.UnaryOp) and type(node.op) in UNARYOPS:
        return float(UNARYOPS[type(node.op)](_evaluate_node(node.operand, variables)))
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in FUNCTIONS and not node.keywords:
        return float(FUNCTIONS[node.func.id](*[_evaluate_node(arg, variables) for arg in node.args]))
    raise ValueError(f"unsupported syntax: {type(node).__name__}")


def safe_evaluate(expression: str, variables: Mapping[str, float]) -> float:
    value = _evaluate_node(_tree(expression), variables)
    if not math.isfinite(value):
        raise ValueError("non-finite result")
    return value


def _symbolic(left: str, right: str, variables: set[str]) -> dict[str, Any]:
    try:
        import sympy
    except ImportError:
        return {"available": False, "equivalent": None, "difference": None}
    # AST validation above ensures expressions contain only arithmetic and allowed calls.
    local = {name: sympy.Symbol(name, real=True) for name in variables}
    local.update({name: getattr(sympy, "log" if name == "ln" else name) for name in FUNCTIONS if hasattr(sympy, "log" if name == "ln" else name)})
    local.update({"pi": sympy.pi, "e": sympy.E, "tau": 2 * sympy.pi})
    try:
        lhs = sympy.sympify(_normalize_expression(left), locals=local, evaluate=True)
        rhs = sympy.sympify(_normalize_expression(right), locals=local, evaluate=True)
        difference = sympy.simplify(lhs - rhs)
        equivalent = bool(difference == 0)
        return {"available": True, "equivalent": equivalent, "difference": str(difference)}
    except Exception as exc:
        return {"available": True, "equivalent": None, "difference": None, "error": str(exc)}


def _numeric(left: str, right: str, variables: set[str], ranges: Mapping[str, Any], samples: int, tolerance: float, seed: int) -> dict[str, Any]:
    rng = random.Random(seed)
    valid = skipped = failures = 0
    max_abs = max_rel = 0.0
    examples = []
    for _ in range(samples):
        values = {}
        for name in sorted(variables):
            bounds = ranges.get(name, [-10.0, 10.0])
            if not isinstance(bounds, (list, tuple)) or len(bounds) != 2:
                raise DirectorError("invalid_range", f"Range for {name} must be [minimum, maximum]")
            low, high = float(bounds[0]), float(bounds[1])
            values[name] = rng.uniform(low, high)
        try:
            lhs = safe_evaluate(left, values)
            rhs = safe_evaluate(right, values)
        except (ArithmeticError, ValueError, OverflowError):
            skipped += 1
            continue
        valid += 1
        absolute = abs(lhs - rhs)
        relative = absolute / max(1.0, abs(lhs), abs(rhs))
        max_abs, max_rel = max(max_abs, absolute), max(max_rel, relative)
        if absolute > tolerance and relative > tolerance:
            failures += 1
            if len(examples) < 5:
                examples.append({"variables": values, "left": lhs, "right": rhs, "absolute_error": absolute, "relative_error": relative})
    return {
        "equivalent": valid > 0 and failures == 0, "samples_requested": samples,
        "samples_valid": valid, "samples_skipped": skipped, "failures": failures,
        "max_absolute_error": max_abs, "max_relative_error": max_rel, "counterexamples": examples,
    }


def _validate_data(params: Mapping[str, Any]) -> dict[str, Any]:
    expected = list(params.get("expected", []))
    displayed = list(params.get("displayed", []))
    decimals = int(params.get("decimals", 3))
    if len(expected) != len(displayed):
        return {"valid": False, "mismatches": [{"code": "length", "expected": len(expected), "displayed": len(displayed)}]}
    mismatches = []
    for index, (source, shown) in enumerate(zip(expected, displayed)):
        try:
            rounded = round(float(source), decimals)
            observed = float(shown)
            if observed != rounded:
                mismatches.append({"index": index, "source": source, "expected_display": rounded, "displayed": shown})
        except (TypeError, ValueError):
            if source != shown:
                mismatches.append({"index": index, "source": source, "displayed": shown})
    return {"valid": not mismatches, "decimals": decimals, "mismatches": mismatches}


def math_validate(params: Mapping[str, Any]) -> dict[str, Any]:
    operation = str(params.get("operation", "equivalent"))
    if operation == "data":
        return {"operation": operation, **_validate_data(params)}
    left = str(params.get("left", ""))
    right = str(params.get("right", ""))
    if not left or not right:
        raise DirectorError("expressions_required", "left and right expressions are required")
    left_tree, right_tree = _tree(left), _tree(right)
    variables = _names(left_tree) | _names(right_tree)
    # Validate all syntax before handing expressions to an optional symbolic backend.
    probe_values = {name: 1.2345 for name in variables}
    for expression in (left, right):
        try:
            safe_evaluate(expression, probe_values)
        except (ArithmeticError, ValueError, OverflowError):
            # Domain failures at a single probe are allowed; unsupported AST is not.
            pass
    symbolic = _symbolic(left, right, variables)
    numeric = _numeric(
        left, right, variables, params.get("ranges", {}),
        max(1, min(10000, int(params.get("samples", 100)))),
        float(params.get("tolerance", 1e-9)), int(params.get("seed", 1729)),
    )
    if numeric["samples_valid"] == 0 and symbolic.get("equivalent") is None:
        verdict = None
    elif symbolic.get("equivalent") is False:
        verdict = False
    elif numeric["equivalent"] is False:
        verdict = False
    elif symbolic.get("equivalent") is True or numeric["equivalent"] is True:
        verdict = True
    else:
        verdict = None
    return {
        "operation": operation, "left": left, "right": right,
        "variables": sorted(variables), "assumptions": params.get("assumptions", {}),
        "equivalent": verdict, "symbolic": symbolic, "numeric": numeric,
    }
