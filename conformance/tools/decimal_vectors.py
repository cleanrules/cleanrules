"""Generate and check the vectors of decimal arithmetic, rounding and sums.

The expected results come from an oracle independent of the core: it follows
`spec/ast.md` (*Arithmetic*, *Decimal arithmetic* and *Rounding*) and
`spec/rules.md` (*`COLLECT` aggregates*) with Python's exact rationals
(`fractions.Fraction`) for decimals, ints and the sums of floats, and Python
floats for float arithmetic.

    python3 conformance/tools/decimal_vectors.py --write   # ast/decimals.json, ast/rounding.json
    python3 conformance/tools/decimal_vectors.py --check   # CI: files up to date, and every
                                                            # arithmetic node and aggregate of
                                                            # every vector recomputed
"""

import glob
import json
import math
import os
import sys
from fractions import Fraction

VECTORS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "vectors")
MODES = ["half_even", "half_up", "half_down", "up", "down", "ceiling", "floor"]

LIMIT = 10**38
MAX_SCALE = 38


class Dec:
    """A decimal of the data model: (coefficient, scale) or an infinity."""

    def __init__(self, text):
        self.text = text
        if text in ("inf", "-inf"):
            self.inf = -1 if text.startswith("-") else 1
            return
        self.inf = 0
        neg = text.startswith("-")
        body = text.lstrip("-")
        if "." in body:
            i, f = body.split(".")
        else:
            i, f = body, ""
        self.scale = len(f)
        self.coef = int(i + f) * (-1 if neg else 1)

    def value(self):
        """The exact value."""
        return Fraction(self.coef, 10**self.scale)


def canonical(coef, scale):
    """The canonical text of `coef * 10**-scale` (`json-format.md`, *Numbers*)."""
    digits = str(abs(coef)).rjust(scale + 1, "0")
    integer, fraction = digits[: len(digits) - scale], digits[len(digits) - scale :]
    sign = "-" if coef < 0 else ""
    return sign + integer + ("." + fraction if scale else "")


def fit(v, ideal):
    """The decimal for an exact rational v (ast.md, Decimal arithmetic 3-5)."""
    s_max = None
    for s in range(MAX_SCALE, -1, -1):
        if abs(round(v * 10**s)) < LIMIT:
            s_max = s
            break
    if s_max is None:
        return None
    if (v * 10**s_max).denominator == 1:
        s_min = next(s for s in range(0, s_max + 1) if (v * 10**s).denominator == 1)
        scale = min(max(ideal, s_min), s_max)
        coef = v * 10**scale
        assert coef.denominator == 1
        return canonical(int(coef), scale)
    return canonical(round(v * 10**s_max), s_max)


def kind(x):
    """The kind of an operand."""
    return x[0]


def to_json(x):
    """An operand in the JSON encoding of values."""
    k, v = x
    if k == "int":
        return v if abs(v) <= 2**53 - 1 else {"$int": str(v)}
    if k == "float":
        if v in (float("inf"), float("-inf")):
            return {"$float": "inf" if v > 0 else "-inf"}
        return v
    if k == "decimal":
        return {"$decimal": v}
    raise ValueError(k)


def as_float(x):
    """An operand converted to float, decimals correctly rounded."""
    k, v = x
    if k == "int":
        return float(v)
    if k == "float":
        return v
    d = Dec(v)
    if d.inf:
        return float("inf") * d.inf
    # Python's float(Fraction) is correctly rounded.
    return float(d.value())


def as_dec(x):
    """An int or decimal operand as a decimal."""
    k, v = x
    if k == "int":
        return Dec(str(v))
    return Dec(v)


def is_zero(x):
    """Whether an operand is a zero of any kind."""
    k, v = x
    if k == "decimal":
        d = Dec(v)
        return not d.inf and d.coef == 0
    return v == 0


def round_value(x, scale, mode="half_even"):
    """`round` (ast.md, *Rounding*): (value json or None, reason code or None)."""
    if kind(x) == "float":
        return None, "type_mismatch"
    d = as_dec(x)
    if d.inf:
        return {"$decimal": d.text}, None
    v = d.value() * 10**scale
    negative = v < 0
    m = abs(v)
    low = math.floor(m)
    frac = m - low
    half = Fraction(1, 2)
    away = (
        frac != 0
        and {
            "half_even": frac > half or (frac == half and low % 2 == 1),
            "half_up": frac >= half,
            "half_down": frac > half,
            "up": True,
            "down": False,
            "ceiling": not negative,
            "floor": negative,
        }[mode]
    )
    coef = low + (1 if away else 0)
    if coef >= LIMIT:
        return None, "decimal_overflow"
    return {"$decimal": canonical(-coef if negative else coef, scale)}, None


def arith(op, a, b=None):
    """(value json or None, reason code or None)."""
    kinds = {kind(a)} | ({kind(b)} if b is not None else set())
    if op in ("abs", "neg"):
        if kind(a) == "int":
            r = abs(a[1]) if op == "abs" else -a[1]
            if not -(2**63) <= r < 2**63:
                return None, "int_overflow"
            return to_json(("int", r)), None
        if kind(a) == "float":
            r = abs(a[1]) if op == "abs" else -a[1]
            return to_json(("float", r)), None
        d = Dec(a[1])
        if d.inf:
            sign = 1 if op == "abs" else -d.inf
            return {"$decimal": "inf" if sign > 0 else "-inf"}, None
        v = abs(d.value()) if op == "abs" else -d.value()
        return {"$decimal": fit(v, d.scale)}, None
    if op == "div" and is_zero(b):
        return None, "division_by_zero"
    if "float" in kinds or (kinds == {"int"} and op == "div"):
        x, y = as_float(a), as_float(b)
        r = {
            "add": lambda: x + y,
            "sub": lambda: x - y,
            "mul": lambda: x * y,
            "div": lambda: x / y,
        }[op]()
        if r != r:
            return None, "nan_result"
        return to_json(("float", r)), None
    if kinds == {"int"}:
        x, y = a[1], b[1]
        r = {"add": x + y, "sub": x - y, "mul": x * y}[op]
        if not -(2**63) <= r < 2**63:
            return None, "int_overflow"
        return to_json(("int", r)), None
    x, y = as_dec(a), as_dec(b)
    if x.inf or y.inf:
        return dec_inf(op, x, y)
    sa, sb = x.scale, y.scale
    if op == "add":
        v, ideal = x.value() + y.value(), max(sa, sb)
    elif op == "sub":
        v, ideal = x.value() - y.value(), max(sa, sb)
    elif op == "mul":
        v, ideal = x.value() * y.value(), sa + sb
    else:
        v, ideal = x.value() / y.value(), max(sa - sb, 0)
    r = fit(v, ideal)
    if r is None:
        return None, "decimal_overflow"
    return {"$decimal": r}, None


def dec_inf(op, x, y):
    """Decimal arithmetic with an infinity (*Decimal arithmetic*, 6)."""

    def sign(d):
        if d.inf:
            return d.inf
        return (d.coef > 0) - (d.coef < 0)

    nan = (None, "nan_result")

    def inf(s):
        return {"$decimal": "inf" if s > 0 else "-inf"}, None

    if op in ("add", "sub"):
        ys = y.inf if op == "add" else -y.inf
        if x.inf and y.inf:
            return inf(x.inf) if x.inf == ys else nan
        return inf(x.inf or ys)
    if op == "mul":
        s = sign(x) * sign(y)
        return nan if s == 0 else inf(s)
    # div, y non-zero
    if x.inf and y.inf:
        return nan
    if x.inf:
        return inf(x.inf * sign(y))
    return {"$decimal": canonical(0, x.scale)}, None


SPEC = ["ast.md#arithmetic", "ast.md#decimal-arithmetic", "json-format.md#result-trees"]


def value_vector(name, description, op, a, b=None, strict=False, const_b=False):
    """A `value` vector: `a op b` on fields `a` and `b` (or a constant `b`)."""

    def operand(key, x, const):
        if const:
            return {"op": "const", "value": to_json(x)}, {"op": "const", "value": to_json(x)}
        return {"op": "field", "path": [key]}, {"op": "field", "path": [key], "value": to_json(x)}

    inp = {"a": to_json(a)}
    left, left_tree = operand("a", a, False)
    paths = [["a"]]
    if b is None:
        node = {"op": op, "operand": left}
        children = [left_tree]
    else:
        right, right_tree = operand("b", b, const_b)
        if not const_b:
            inp["b"] = to_json(b)
            paths.append(["b"])
        node = {"op": op, "left": left, "right": right}
        children = [left_tree, right_tree]
    value, reason = arith(op, a, b)
    result = {"op": op, "value": value}
    if reason is not None:
        result["reason"] = {"code": reason, "paths": paths}
    result["children"] = children
    given = {"kind": "value", "value": node, "input": inp}
    if strict:
        given["strict"] = True
    return {
        "id": f"ast/decimals/{name}",
        "spec": SPEC,
        "description": description,
        "given": given,
        "expect": {"result": result},
    }


def dec(text):
    """A decimal operand."""
    return ("decimal", text)


def num(n):
    """An int operand."""
    return ("int", n)


def flt(x):
    """A float operand."""
    return ("float", x)


N38 = "9" * 38
E37 = "1" + "0" * 37

CASES = [
    # add
    (
        "add-decimals",
        "two decimals add exactly, with the larger scale",
        "add",
        dec("1.10"),
        dec("2.205"),
    ),
    (
        "add-point-one-point-two",
        "0.1 + 0.2 is exactly 0.3 with decimals",
        "add",
        dec("0.1"),
        dec("0.2"),
    ),
    (
        "add-scale-kept",
        "a sum keeps the ideal scale, trailing zeros included",
        "add",
        dec("1.5"),
        dec("1.5"),
    ),
    ("add-decimal-int", "a decimal plus an int is a decimal", "add", dec("0.5"), num(1)),
    ("add-int-decimal", "an int plus a decimal is a decimal", "add", num(-3), dec("0.25")),
    ("add-decimal-float", "a decimal plus a float is a float", "add", dec("0.1"), flt(0.2)),
    ("add-float-decimal", "a float plus a decimal is a float", "add", flt(0.5), dec("0.25")),
    ("add-int-min", "the smallest int promotes exactly", "add", num(-(2**63)), dec("0.5")),
    (
        "add-rounds-fraction",
        "a sum without room for its fraction is rounded half to even",
        "add",
        dec(E37),
        dec("0.5"),
    ),
    (
        "add-rounds-up",
        "a tie rounds to the even neighbour, here up",
        "add",
        dec("1" + "0" * 36 + "1"),
        dec("0.5"),
    ),
    (
        "add-overflow",
        "a sum beyond 38 digits is null with decimal_overflow",
        "add",
        dec(N38),
        num(1),
    ),
    (
        "add-inf",
        "an infinity plus a finite decimal is that infinity",
        "add",
        dec("inf"),
        dec("1.5"),
    ),
    ("add-inf-int", "an infinity plus an int is a decimal infinity", "add", num(7), dec("-inf")),
    ("add-infs", "inf + inf is inf", "add", dec("inf"), dec("inf")),
    # sub
    ("sub-decimals", "two decimals subtract exactly", "sub", dec("10.05"), dec("9.9")),
    (
        "sub-to-zero",
        "a zero difference keeps the scale and has no sign",
        "sub",
        dec("1.50"),
        dec("1.5"),
    ),
    ("sub-int-decimal", "an int minus a decimal is a decimal", "sub", num(1), dec("0.001")),
    ("sub-decimal-float", "a decimal minus a float is a float", "sub", dec("1"), flt(0.1)),
    ("sub-overflow", "a difference beyond 38 digits is null", "sub", dec("-" + N38), dec("1")),
    ("sub-inf-inf", "inf - inf is null with nan_result", "sub", dec("inf"), dec("inf")),
    ("sub-inf-minus-inf", "inf - (-inf) is inf", "sub", dec("inf"), dec("-inf")),
    # mul
    ("mul-decimals", "a product has the sum of the scales", "mul", dec("19.99"), dec("0.15")),
    ("mul-int-decimal", "an int times a decimal is a decimal", "mul", num(2), dec("0.10")),
    ("mul-decimal-int", "a decimal times an int is a decimal", "mul", dec("1.25"), num(4)),
    ("mul-decimal-float", "a decimal times a float is a float", "mul", dec("0.1"), flt(3.0)),
    (
        "mul-negative-zero",
        "a negative decimal times zero is an unsigned zero",
        "mul",
        dec("-1.5"),
        num(0),
    ),
    (
        "mul-tiny",
        "a product below 38 places rounds to a zero with scale 38",
        "mul",
        dec("0." + "0" * 19 + "1"),
        dec("0." + "0" * 19 + "1"),
    ),
    (
        "mul-rounds",
        "a product with more than 38 significant digits is rounded half to even",
        "mul",
        dec("1.23456789012345678901"),
        dec("1.23456789012345678901"),
    ),
    (
        "mul-ideal-scale-too-large",
        "an exact product whose ideal scale does not fit keeps the largest scale that fits",
        "mul",
        dec("1." + "0" * 37),
        dec("1.00"),
    ),
    (
        "mul-int-max-squared",
        "the largest int squared as a decimal is exact at scale 0",
        "mul",
        num(2**63 - 1),
        dec(str(2**63 - 1) + ".0"),
    ),
    (
        "mul-overflow",
        "a product beyond 38 digits is null",
        "mul",
        dec("1" + "0" * 20),
        dec("1" + "0" * 20),
    ),
    (
        "mul-inf-negative",
        "an infinity times a negative decimal changes sign",
        "mul",
        dec("inf"),
        dec("-2"),
    ),
    (
        "mul-inf-zero",
        "an infinity times zero is null with nan_result",
        "mul",
        dec("inf"),
        dec("0.00"),
    ),
    # div
    ("div-exact", "an exact quotient has the ideal scale", "div", dec("7.5"), dec("2.5")),
    ("div-int-decimal", "an int divided by a decimal is a decimal", "div", num(1), dec("0.25")),
    (
        "div-decimal-int",
        "a terminating quotient keeps the digits it needs",
        "div",
        dec("1.00"),
        num(4),
    ),
    (
        "div-ideal-scale",
        "trailing zeros up to the ideal scale are kept",
        "div",
        dec("1.000"),
        num(4),
    ),
    (
        "div-terminating-longer",
        "a terminating quotient beyond the ideal scale is exact",
        "div",
        num(1),
        dec("8.0"),
    ),
    ("div-one-third", "a repeating quotient keeps 38 places", "div", num(1), dec("3.0")),
    (
        "div-two-thirds",
        "a repeating quotient is rounded half to even at the last place",
        "div",
        num(2),
        dec("3.0"),
    ),
    (
        "div-ten-thirds",
        "a repeating quotient keeps 38 significant digits",
        "div",
        dec("10"),
        num(3),
    ),
    (
        "div-hundred-sevenths",
        "a repeating quotient with an integer part keeps 38 significant digits",
        "div",
        num(100),
        dec("7.0"),
    ),
    (
        "div-small-quotient",
        "a quotient below 1 keeps 38 places, not 38 significant digits",
        "div",
        dec("0.1"),
        num(3),
    ),
    (
        "div-negative",
        "a negative repeating quotient rounds its magnitude",
        "div",
        dec("-2"),
        dec("3"),
    ),
    (
        "div-tie-to-even-up",
        "a quotient exactly halfway rounds to the even neighbour, up",
        "div",
        dec(N38),
        num(2),
    ),
    (
        "div-tie-to-even-down",
        "a quotient exactly halfway rounds to the even neighbour, down",
        "div",
        dec("9" * 37 + "7"),
        num(2),
    ),
    (
        "div-underflow",
        "a quotient below 38 places is a zero with scale 38",
        "div",
        dec("0." + "0" * 19 + "1"),
        dec("1" + "0" * 30),
    ),
    ("div-overflow", "a quotient beyond 38 digits is null", "div", dec(E37), dec("0.01")),
    ("div-by-int-zero", "a decimal divided by the int 0 is null", "div", dec("1.5"), num(0)),
    ("div-by-decimal-zero", "a division by a decimal zero is null", "div", num(1), dec("0.00")),
    (
        "div-inf-by-zero",
        "an infinity divided by zero is a division by zero",
        "div",
        dec("inf"),
        dec("0"),
    ),
    ("div-decimal-float", "a decimal divided by a float is a float", "div", dec("1"), flt(4.0)),
    (
        "div-by-inf",
        "a finite decimal divided by an infinity is zero with the ideal scale",
        "div",
        dec("1.50"),
        dec("inf"),
    ),
    (
        "div-inf-by-negative",
        "an infinity divided by a negative decimal changes sign",
        "div",
        dec("inf"),
        num(-2),
    ),
    ("div-inf-by-inf", "inf / inf is null with nan_result", "div", dec("-inf"), dec("inf")),
    # abs, neg
    ("abs-decimal", "abs of a decimal is a decimal with its scale", "abs", dec("-1.50")),
    ("neg-decimal", "neg of a decimal is a decimal with its scale", "neg", dec("2.25")),
    ("neg-zero", "neg of a decimal zero is the same unsigned zero", "neg", dec("0.00")),
    ("neg-inf", "neg of an infinity is the other infinity", "neg", dec("-inf")),
    ("abs-inf", "abs of an infinity is inf", "abs", dec("-inf")),
]


COMPARE_EXACT_SUM = {
    "id": "ast/decimals/compare-exact-sum",
    "spec": ["ast.md#arithmetic", "ast.md#types", "ast.md#evaluating-one-node"],
    "description": "a decimal sum compares exactly: 0.1 + 0.2 equals 0.3",
    "given": {
        "kind": "condition",
        "condition": {
            "op": "eq",
            "left": {
                "op": "add",
                "left": {"op": "field", "path": ["a"]},
                "right": {"op": "field", "path": ["b"]},
            },
            "right": {"op": "const", "value": {"$decimal": "0.3"}},
        },
        "input": {"a": {"$decimal": "0.1"}, "b": {"$decimal": "0.2"}},
    },
    "expect": {
        "result": {
            "op": "eq",
            "truth": True,
            "children": [
                {
                    "op": "add",
                    "value": {"$decimal": "0.3"},
                    "children": [
                        {"op": "field", "path": ["a"], "value": {"$decimal": "0.1"}},
                        {"op": "field", "path": ["b"], "value": {"$decimal": "0.2"}},
                    ],
                },
                {"op": "const", "value": {"$decimal": "0.3"}},
            ],
        }
    },
}


ROUND_SPEC = ["ast.md#rounding", "json-format.md#result-trees"]


def round_vector(name, description, a, scale, mode=None, strict=False):
    """A `value` vector: `round(a, scale, mode)` on the field `a`."""
    node = {"op": "round", "operand": {"op": "field", "path": ["a"]}, "scale": scale}
    if mode is not None:
        node["mode"] = mode
    child = {"op": "field", "path": ["a"], "value": to_json(a) if a is not None else None}
    given = {"kind": "value", "value": node, "input": {"a": to_json(a) if a is not None else None}}
    if strict:
        given["strict"] = True
    if a is None:
        child["reason"] = {"code": "null", "paths": [["a"]]}
        result = {"op": "round", "value": None, "reason": child["reason"], "children": [child]}
    else:
        value, reason = round_value(a, scale, mode or "half_even")
        result = {"op": "round", "value": value}
        if reason == "type_mismatch":
            if strict:
                error = {"code": "type_mismatch", "path": ["a"], "types": ["number", "number"]}
                return {
                    "id": f"ast/rounding/{name}",
                    "spec": ROUND_SPEC,
                    "description": description,
                    "given": given,
                    "expect": {"error": error},
                }
            result["reason"] = {"code": reason, "paths": [["a"]], "types": ["number", "number"]}
        elif reason is not None:
            result["reason"] = {"code": reason, "paths": [["a"]]}
        result["children"] = [child]
    return {
        "id": f"ast/rounding/{name}",
        "spec": ROUND_SPEC,
        "description": description,
        "given": given,
        "expect": {"result": result},
    }


def rounding_vectors():
    """The vectors of `ast/rounding.json`."""
    vectors = []
    # Every mode on a tie, above and below a tie, for both signs.
    for mode in MODES:
        for text, what in [
            ("2.345", "a positive tie"),
            ("-2.345", "a negative tie"),
            ("2.3451", "just above a tie"),
            ("-2.3449", "just below a negative tie"),
            ("2.355", "a tie after an odd digit"),
        ]:
            name = f"{mode.replace('_', '-')}-{text.replace('-', 'minus-').replace('.', 'p')}"
            vectors.append(
                round_vector(name, f"{mode} rounds {what} to 2 places", dec(text), 2, mode)
            )
    vectors += [
        round_vector("default-mode", "the mode is half_even when omitted", dec("2.5"), 0),
        round_vector("exact-padded", "a value with fewer places is padded", dec("1.5"), 3),
        round_vector("int-padded", "an int becomes a decimal with the places", num(5), 2),
        round_vector(
            "int-zero-places", "an int with scale 0 is the same number as a decimal", num(-7), 0
        ),
        round_vector("scale-zero", "rounding to an integer", dec("16.9915"), 0, "half_up"),
        round_vector("cents", "a price rounded to cents", dec("16.9915"), 2),
        round_vector("cents-up", "a price rounded up to cents", dec("16.9915"), 2, "up"),
        round_vector("scale-38", "38 places", dec("0.1"), 38),
        round_vector("carry", "rounding can carry into a new digit", dec("9.995"), 2, "half_up"),
        round_vector(
            "no-negative-zero",
            "a negative value rounded to zero is an unsigned zero",
            dec("-0.001"),
            2,
        ),
        round_vector("negative-down", "down truncates toward zero", dec("-1.999"), 1, "down"),
        round_vector(
            "overflow-padding", "padding beyond 38 digits is null", dec("1" + "0" * 37), 2
        ),
        round_vector(
            "carry-at-the-limit",
            "a carry to a 38th digit fits",
            dec("9" * 37 + ".5"),
            0,
            "half_up",
        ),
        round_vector("infinity-kept", "an infinity is kept", dec("-inf"), 2),
        round_vector(
            "float-mismatch", "a float operand is a type mismatch, never rounded", flt(2.345), 2
        ),
        round_vector(
            "float-strict", "a float operand raises in strict mode", flt(2.345), 2, strict=True
        ),
        round_vector("null", "a null operand is null with its reason", None, 2),
    ]
    # A discount rounded to cents: round(price * 0.85, 2) on 19.99.
    vectors.append(
        {
            "id": "ast/rounding/chained-discount",
            "spec": [*ROUND_SPEC, "ast.md#decimal-arithmetic"],
            "description": "a discounted price rounded to cents: 19.99 * 0.85 is 16.9915, so 16.99",
            "given": {
                "kind": "value",
                "value": {
                    "op": "round",
                    "operand": {
                        "op": "mul",
                        "left": {"op": "field", "path": ["price"]},
                        "right": {"op": "const", "value": {"$decimal": "0.85"}},
                    },
                    "scale": 2,
                    "mode": "half_even",
                },
                "input": {"price": {"$decimal": "19.99"}},
            },
            "expect": {
                "result": {
                    "op": "round",
                    "value": {"$decimal": "16.99"},
                    "children": [
                        {
                            "op": "mul",
                            "value": {"$decimal": "16.9915"},
                            "children": [
                                {"op": "field", "path": ["price"], "value": {"$decimal": "19.99"}},
                                {"op": "const", "value": {"$decimal": "0.85"}},
                            ],
                        }
                    ],
                }
            },
        }
    )
    return {"version": 1, "area": "ast", "vectors": vectors}


def decimal_vectors():
    """The vectors of `ast/decimals.json`."""
    vectors = []
    for name, description, op, a, *rest in CASES:
        b = rest[0] if rest else None
        vectors.append(value_vector(name, description, op, a, b))
    vectors.append(
        value_vector(
            "strict-overflow",
            "decimal overflow is a null with its reason, never an error, in strict mode",
            "mul",
            dec(N38),
            num(10),
            strict=True,
        )
    )
    vectors.append(
        value_vector(
            "const-operand",
            "a decimal constant in arithmetic keeps the result exact",
            "mul",
            dec("80.00"),
            dec("0.85"),
            const_b=True,
        )
    )
    vectors.append(COMPARE_EXACT_SUM)
    return {"version": 1, "area": "ast", "vectors": vectors}


# --- Checking every vector ---------------------------------------------------------


def operand_of(value):
    """A result tree's value as an oracle operand, or None when it is not a number."""
    if isinstance(value, dict):
        if "$decimal" in value:
            d = Dec(value["$decimal"])
            return ("decimal", value["$decimal"] if d.inf else canonical(d.coef, d.scale))
        if "$float" in value:
            return ("float", float(value["$float"]))
        if "$int" in value:
            return ("int", int(value["$int"]))
        return None
    if isinstance(value, bool) or not isinstance(value, int | float):
        return None
    return ("int", value) if isinstance(value, int) else ("float", value)


def same_value(a, b):
    """Values equal as the vectors compare them: by JSON type and exact value."""
    return (
        type(a) is type(b)
        and a == b
        and (not isinstance(a, float) or math.copysign(1, a) == math.copysign(1, b))
    )


def check_tree(tree, where, problems):
    """Recompute every arithmetic and round node of a result tree."""
    if isinstance(tree, list):
        for item in tree:
            check_tree(item, where, problems)
        return
    if not isinstance(tree, dict):
        return
    for value in tree.values():
        check_tree(value, where, problems)
    op = tree.get("op")
    if op not in ("add", "sub", "mul", "div", "abs", "neg", "round") or "children" not in tree:
        return
    if "value" not in tree:
        return
    operands = [operand_of(c.get("value")) for c in tree["children"]]
    if any(o is None for o in operands):
        return
    if op == "round":
        # The tree holds neither the scale nor the mode: a decimal result must
        # be the operand rounded to the result's scale in some mode, and a null
        # one must say why.
        value, code = tree["value"], tree.get("reason", {}).get("code")
        if isinstance(value, dict) and "$decimal" in value:
            scale = Dec(value["$decimal"]).scale if not Dec(value["$decimal"]).inf else 0
            if any(round_value(operands[0], scale, m)[0] == value for m in MODES):
                return
        elif value is None and code in ("decimal_overflow", "type_mismatch"):
            return
        problems.append(f"{where}: round of {operands[0]} gave {value} ({code})")
        return
    expected, reason = arith(op, *operands)
    got_reason = tree.get("reason", {}).get("code")
    if not same_value(tree["value"], expected) or got_reason != reason:
        problems.append(
            f"{where}: {op} {operands} is {expected} ({reason}), "
            f"the vector says {tree['value']} ({got_reason})"
        )


def exact(value):
    """A data value as an exact rational, with its kind; None for a non-number."""
    o = operand_of(value)
    if o is None:
        return None
    k, v = o
    if k == "decimal":
        d = Dec(v)
        return (k, float("inf") * d.inf if d.inf else d.value())
    if k == "float":
        return (k, v if math.isinf(v) else Fraction(v))
    return (k, Fraction(v))


def expected_aggregate(aggregate, values):
    """The aggregate of one key's non-null values (rules.md, *`COLLECT` aggregates*)."""
    values = [v for v in values if v is not None]
    if aggregate == "count":
        return len(values)
    if not values:
        return None
    numbers = [exact(v) for v in values]
    if aggregate in ("min", "max"):
        best = 0
        for i, (_, x) in enumerate(numbers):
            if (x < numbers[best][1]) if aggregate == "min" else (x > numbers[best][1]):
                best = i
        return values[best]
    kinds = {k for k, _ in numbers}
    infinities = {x for _, x in numbers if isinstance(x, float)}
    if len(infinities) == 2:
        return None
    if infinities:
        (inf,) = infinities
        if "float" in kinds:
            return {"$float": "inf" if inf > 0 else "-inf"}
        return {"$decimal": "inf" if inf > 0 else "-inf"}
    total = sum((x for _, x in numbers), Fraction(0))
    if "float" in kinds:
        try:
            return float(total)
        except OverflowError:
            return {"$float": "inf" if total > 0 else "-inf"}
    if "decimal" in kinds:
        scale = max(
            Dec(v["$decimal"]).scale for v in values if isinstance(v, dict) and "$decimal" in v
        )
        text = fit(total, scale)
        return None if text is None else {"$decimal": text}
    if not -(2**63) <= total < 2**63:
        return None
    return to_json(("int", int(total)))


def check_decision(decision, where, problems):
    """Recompute a decision's aggregate from its selected rules' outputs."""
    aggregate = decision.get("aggregate")
    if aggregate is None or "aggregated" not in decision:
        return
    selected = set(decision.get("selected", []))
    keys = {}
    for result in decision.get("results", []):
        if result["rule"] not in selected:
            continue
        for key, tree in (result.get("then") or {}).items():
            keys.setdefault(key, []).append(tree.get("value"))
    if not selected:
        return
    for key, values in keys.items():
        expected = expected_aggregate(aggregate, values)
        got = decision["aggregated"].get(key)
        if not same_value(got, expected) and got != expected:
            problems.append(f"{where}: {aggregate} of {key!r} is {expected}, the vector says {got}")


def check_all():
    """Every arithmetic node and aggregate of every vector, recomputed."""
    problems = []
    for path in sorted(glob.glob(os.path.join(VECTORS, "**", "*.json"), recursive=True)):
        with open(path, encoding="utf-8") as f:
            vectors = json.load(f)["vectors"]
        for vector in vectors:
            expect = vector.get("expect", {})
            check_tree(expect.get("result"), vector["id"], problems)
            for decision in [expect.get("decision"), *expect.get("decisions", [])]:
                if decision:
                    check_tree(decision.get("results"), vector["id"], problems)
                    check_decision(decision, vector["id"], problems)
    return problems


FILES = {"decimals.json": decimal_vectors, "rounding.json": rounding_vectors}


def main():
    """Write the generated files, or check them and every vector."""
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    problems = []
    for name, make in FILES.items():
        path = os.path.join(VECTORS, "ast", name)
        text = json.dumps(make(), indent=2, ensure_ascii=False) + "\n"
        if mode == "--write":
            with open(path, "w", encoding="utf-8") as f:
                f.write(text)
        elif mode == "--check":
            with open(path, encoding="utf-8") as f:
                if f.read() != text:
                    problems.append(f"{name} is out of date: run --write")
        else:
            sys.exit("usage: decimal_vectors.py --write | --check")
    if mode == "--check":
        problems += check_all()
        for problem in problems:
            print(problem)
        print(f"decimal vectors: {len(problems)} problems")
        sys.exit(1 if problems else 0)


if __name__ == "__main__":
    main()
