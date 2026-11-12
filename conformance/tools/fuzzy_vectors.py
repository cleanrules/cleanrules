"""Generate `vectors/ast/logics.json`: conditions under the fuzzy logics.

The expected degrees come from fuzzytool, the behavioural reference of the
norms (`spec/ast.md`, *Logics*), with the rules the spec adds on top: exact
boundary conditions, results clamped to `[0, 1]`, and intervals for unknown
(*Truth values*). Regenerate with the fuzzytool version named below:

    uv run --no-project --with fuzzytool==0.7.0 python \
        conformance/tools/fuzzy_vectors.py > conformance/vectors/ast/logics.json

Each connective is called on one-element NumPy arrays, as fuzzytool's engines
call it, so the powers 1, -1, 2 and 0.5 are the exact operations the spec
names. Other powers and logarithms are NumPy's (the platform's); the core
computes them with the `libm` crate, and the parameters below were chosen so
that both give the same bits, which the core's runner checks.
"""

from __future__ import annotations

import json
from typing import Any

import fuzzytool.norms as n
import numpy as np

FUZZYTOOL = "0.7.0"
Json = Any

# --- The spec's connectives over fuzzytool's --------------------------------


def call(f, *args: float) -> float:
    """One value of a vectorised connective."""
    return float(f(*(np.array([a]) for a in args))[0])


def unit(x: float) -> float:
    """Clamp to [0, 1], with a negative zero as 0."""
    return min(x, 1.0) if x > 0.0 else 0.0


def tnorm(f, a: float, b: float) -> float:
    """`T(a, b)` with the boundary conditions of `spec/ast.md`, *Logics*."""
    if a == 0.0 or b == 0.0:
        return 0.0
    if a == 1.0:
        return b
    if b == 1.0:
        return a
    return unit(call(f, a, b))


def snorm(f, a: float, b: float) -> float:
    """`S(a, b)` with the boundary conditions of `spec/ast.md`, *Logics*."""
    if a == 1.0 or b == 1.0:
        return 1.0
    if a == 0.0:
        return b
    if b == 0.0:
        return a
    return unit(call(f, a, b))


class Logic:
    """A fuzzy logic: its JSON form and its connectives on intervals."""

    def __init__(self, form: Json, t, s, c=n.complement) -> None:
        self.form, self.t, self.s, self.c = form, t, s, c

    def conj(self, x: tuple[float, float], y: tuple[float, float]) -> tuple[float, float]:
        """`x and y` on the bounds."""
        lo, hi = tnorm(self.t, x[0], y[0]), tnorm(self.t, x[1], y[1])
        return lo, max(lo, hi)

    def disj(self, x: tuple[float, float], y: tuple[float, float]) -> tuple[float, float]:
        """`x or y` on the bounds."""
        lo, hi = snorm(self.s, x[0], y[0]), snorm(self.s, x[1], y[1])
        return lo, max(lo, hi)

    def neg(self, x: tuple[float, float]) -> tuple[float, float]:
        """`not x`: the complement swaps the bounds."""
        lo, hi = unit(call(self.c, x[1])), unit(call(self.c, x[0]))
        return lo, max(lo, hi)


def named(name: str, snorm_name: str) -> Logic:
    """A logic of fuzzytool's named norms."""
    return Logic(name, n.get_tnorm(name), n.get_snorm(snorm_name))


def family(name: str, key: str, p: float, t, s) -> Logic:
    """A logic of a parametric family."""
    return Logic({"name": name, key: p}, t(p), s(p))


LOGICS = [
    named("min", "max"),
    named("prod", "probor"),
    named("lukasiewicz", "lukasiewicz"),
    named("einstein", "einstein"),
    named("hamacher", "hamacher"),
    named("drastic", "drastic"),
    named("nilpotent", "nilpotent"),
    family("yager", "p", 2.0, n.yager_tnorm, n.yager_snorm),
    family("yager", "p", 3.0, n.yager_tnorm, n.yager_snorm),
    family("dombi", "p", 1.0, n.dombi_tnorm, n.dombi_snorm),
    family("dombi", "p", 2.0, n.dombi_tnorm, n.dombi_snorm),
    family("frank", "s", 2.0, n.frank_tnorm, n.frank_snorm),
    family("frank", "s", 0.5, n.frank_tnorm, n.frank_snorm),
]

COMPLEMENTS = [
    ("standard", None, n.complement),
    ("sugeno", {"name": "sugeno", "lam": 2.0}, n.sugeno_complement(2.0)),
    ("sugeno", {"name": "sugeno", "lam": -0.5}, n.sugeno_complement(-0.5)),
    ("yager", {"name": "yager", "w": 2.0}, n.yager_complement(2.0)),
    ("yager", {"name": "yager", "w": 3.0}, n.yager_complement(3.0)),
]

# --- Result trees -------------------------------------------------------------

UNKNOWN = (0.0, 1.0)


def truth(x: tuple[float, float]) -> Json:
    """A graded truth value in JSON: a degree, or `[lo, hi]`."""
    return x[0] if x[0] == x[1] else [x[0], x[1]]


def field(name: str) -> Json:
    """A field node."""
    return {"op": "field", "path": [name]}


def missing_reason(name: str) -> Json:
    """The reason of a missing field."""
    return {"code": "missing_field", "paths": [[name]]}


def leaf(
    name: str, value: Json, degree: tuple[float, float], reason: Json = None, *, read: bool = True
) -> Json:
    """A field in a condition position, with its value result.

    The reason is the value's too when `read` (a null), and only the
    condition's otherwise (a type mismatch).
    """
    inner: dict[str, Json] = {"op": "field", "path": [name], "value": value}
    outer: dict[str, Json] = {"op": "field", "path": [name], "truth": truth(degree)}
    if reason is not None:
        if read:
            inner["reason"] = reason
        outer["reason"] = reason
    outer["children"] = [inner]
    return outer


def node(op: str, x: tuple[float, float], children: list[Json]) -> Json:
    """A connective's result: an undetermined one takes the first undetermined child's reason."""
    out: dict[str, Json] = {"op": op, "truth": truth(x)}
    if x[0] != x[1]:
        reason = next(c["reason"] for c in children if isinstance(c["truth"], list))
        out["reason"] = reason
    out["children"] = children
    return out


def slug(x: float) -> str:
    """A number in an id: `0.75` as `0p75`, `-0.5` as `minus-0p5`."""
    text = f"{x:g}".replace(".", "p")
    return "minus-" + text[1:] if text.startswith("-") else text


def name_of(logic: Logic) -> str:
    """A logic's name and parameter, for ids."""
    if isinstance(logic.form, str):
        return logic.form
    key = next(k for k in logic.form if k != "name")
    return f"{logic.form['name']}-{key}-{slug(logic.form[key])}"


def vector(
    vid: str,
    spec: list[str],
    description: str,
    condition: Json,
    data: Json,
    logic: Json,
    result: Json,
) -> Json:
    """A condition vector under `logic`."""
    return {
        "id": f"ast/logics/{vid}",
        "spec": spec,
        "description": description,
        "given": {"kind": "condition", "condition": condition, "input": data, "logic": logic},
        "expect": result,
    }


NORMS = ["ast.md#logics", "ast.md#truth-values", "json-format.md#result-trees"]
INTERVALS = ["ast.md#truth-values", "ast.md#results-and-reasons", "json-format.md#result-trees"]
DEGREES = ["data-model.md#fuzzy-degrees", "ast.md#missing-values", "json-format.md#result-trees"]
FORMS = ["json-format.md#logics", "ast.md#logics", "ast.md#evaluating-one-node"]


def norm_vectors() -> list[Json]:
    """`a and b` and `a or b` for every logic, on interior degrees."""
    out = []
    for logic in LOGICS:
        for a, b in [(0.3, 0.6), (0.75, 0.9)]:
            da, db = (a, a), (b, b)
            for op, f in [("and", logic.conj), ("or", logic.disj)]:
                result = node(op, f(da, db), [leaf("a", a, da), leaf("b", b, db)])
                out.append(
                    vector(
                        f"{name_of(logic)}-{op}-{slug(a)}-{slug(b)}",
                        NORMS,
                        f"{a} {op} {b} under {json.dumps(logic.form)}",
                        {"op": op, "operands": [field("a"), field("b")]},
                        {"a": a, "b": b},
                        logic.form,
                        {"result": result},
                    )
                )
    return out


def complement_vectors() -> list[Json]:
    """`not a` under each complement."""
    out = []
    for name, form, c in COMPLEMENTS:
        logic = Logic(
            "prod" if form is None else {"name": "prod", "complement": form}, None, None, c
        )
        for a in [0.3, 0.75]:
            da = (a, a)
            result = node("not", logic.neg(da), [leaf("a", a, da)])
            suffix = (
                "" if form is None else "-" + "-".join(slug(form[k]) for k in form if k != "name")
            )
            out.append(
                vector(
                    f"complement-{name}{suffix}-{slug(a)}",
                    NORMS,
                    f"not {a} under the {name} complement",
                    {"op": "not", "operand": field("a")},
                    {"a": a},
                    logic.form,
                    {"result": result},
                )
            )
    return out


def interval_vectors() -> list[Json]:
    """Unknown as `[0, 1]`, carried through the connectives."""
    prod, luk, mn = LOGICS[1], LOGICS[2], LOGICS[0]
    m = missing_reason("m")
    out = []

    def missing_leaf() -> Json:
        return leaf("m", None, UNKNOWN, m)

    # and / or / not with a missing operand.
    for logic in [prod, luk]:
        x = (0.4, 0.4)
        out.append(
            vector(
                f"{name_of(logic)}-and-unknown",
                INTERVALS,
                "a degree and unknown is [T(d, 0), T(d, 1)], with the missing field's reason",
                {"op": "and", "operands": [field("a"), field("m")]},
                {"a": 0.4},
                logic.form,
                {
                    "result": node(
                        "and", logic.conj(x, UNKNOWN), [leaf("a", 0.4, x), missing_leaf()]
                    )
                },
            )
        )
        out.append(
            vector(
                f"{name_of(logic)}-or-unknown",
                INTERVALS,
                "a degree or unknown is the interval [S(d, 0), S(d, 1)]",
                {"op": "or", "operands": [field("a"), field("m")]},
                {"a": 0.4},
                logic.form,
                {"result": node("or", logic.disj(x, UNKNOWN), [leaf("a", 0.4, x), missing_leaf()])},
            )
        )
    out.append(
        vector(
            "not-unknown",
            INTERVALS,
            "not unknown is unknown, with its reason",
            {"op": "not", "operand": field("m")},
            {},
            "min",
            {"result": node("not", mn.neg(UNKNOWN), [missing_leaf()])},
        )
    )
    # not swaps the bounds of an undetermined interval.
    sugeno = Logic(
        {"name": "prod", "complement": {"name": "sugeno", "lam": 2.0}},
        prod.t,
        prod.s,
        n.sugeno_complement(2.0),
    )
    inner = node(
        "and", sugeno.conj((0.4, 0.4), UNKNOWN), [leaf("a", 0.4, (0.4, 0.4)), missing_leaf()]
    )
    out.append(
        vector(
            "not-swaps-the-bounds",
            INTERVALS,
            "not [lo, hi] is [N(hi), N(lo)]",
            {"op": "not", "operand": {"op": "and", "operands": [field("a"), field("m")]}},
            {"a": 0.4},
            sugeno.form,
            {"result": node("not", sugeno.neg(sugeno.conj((0.4, 0.4), UNKNOWN)), [inner])},
        )
    )
    # A determined result carries no reason, as FALSE and UNKNOWN under Kleene.
    for op, value, f in [("and", False, mn.conj), ("or", True, mn.disj)]:
        d = (1.0, 1.0) if value else (0.0, 0.0)
        out.append(
            vector(
                f"{op}-{str(value).lower()}-unknown-is-determined",
                INTERVALS,
                f"{str(value).lower()} {op} unknown is {str(value).lower()}, without a reason",
                {"op": op, "operands": [field("a"), field("m")]},
                {"a": value},
                "lukasiewicz",
                {"result": node(op, f(d, UNKNOWN), [leaf("a", value, d), missing_leaf()])},
            )
        )
    # n-ary connectives fold from the first operand.
    values = [0.7, 0.6, 0.9]
    acc = (0.7, 0.7)
    for v in values[1:]:
        acc = luk.conj(acc, (v, v))
    out.append(
        vector(
            "lukasiewicz-and-folds-left",
            INTERVALS,
            "a and b and c is (a and b) and c",
            {"op": "and", "operands": [field("a"), field("b"), field("c")]},
            {"a": 0.7, "b": 0.6, "c": 0.9},
            "lukasiewicz",
            {
                "result": node(
                    "and",
                    acc,
                    [leaf(k, v, (v, v)) for k, v in zip("abc", values, strict=True)],
                )
            },
        )
    )
    # Crisp comparisons are [1, 1], [0, 0] and [0, 1], and the boundary
    # conditions keep a degree exact (fuzzytool's Łukasiewicz gives
    # 0.10000000000000009 for T(1, 0.1)).
    gt = {"op": "gt", "left": field("x"), "right": {"op": "const", "value": 1}}
    gt_true = {
        "op": "gt",
        "truth": 1.0,
        "children": [{"op": "field", "path": ["x"], "value": 2}, {"op": "const", "value": 1}],
    }
    out.append(
        vector(
            "crisp-and-degree-is-exact",
            [*INTERVALS, "ast.md#logics"],
            "a true comparison and a degree d is exactly d",
            {"op": "and", "operands": [gt, field("a")]},
            {"x": 2, "a": 0.1},
            "lukasiewicz",
            {
                "result": node(
                    "and", luk.conj((1.0, 1.0), (0.1, 0.1)), [gt_true, leaf("a", 0.1, (0.1, 0.1))]
                )
            },
        )
    )
    gt_missing = {
        "op": "gt",
        "truth": [0.0, 1.0],
        "reason": missing_reason("x"),
        "children": [
            {"op": "field", "path": ["x"], "value": None, "reason": missing_reason("x")},
            {"op": "const", "value": 1},
        ],
    }
    out.append(
        vector(
            "comparison-with-null-is-unknown",
            INTERVALS,
            "a comparison with a missing field is [0, 1]",
            {"op": "or", "operands": [gt, field("a")]},
            {"a": 0.25},
            "min",
            {
                "result": node(
                    "or",
                    mn.disj(UNKNOWN, (0.25, 0.25)),
                    [gt_missing, leaf("a", 0.25, (0.25, 0.25))],
                )
            },
        )
    )
    return out


def degree_vectors() -> list[Json]:
    """Data values used as conditions under a graded logic."""
    out = []
    cases = [
        ("float", 0.25, (0.25, 0.25), None),
        ("int-one", 1, (1.0, 1.0), None),
        ("int-zero", 0, (0.0, 0.0), None),
        ("decimal", {"$decimal": "0.10"}, (0.1, 0.1), None),
        ("negative-zero", -0.0, (0.0, 0.0), None),
        ("boolean", True, (1.0, 1.0), None),
        ("null", None, UNKNOWN, {"code": "null", "paths": [["a"]]}),
    ]
    for name, value, degree, reason in cases:
        out.append(
            vector(
                f"degree-{name}",
                DEGREES,
                f"{json.dumps(value)} used as a condition under a graded logic",
                field("a"),
                {"a": value},
                "min",
                {"result": leaf("a", value, degree, reason)},
            )
        )
    mismatches = [
        ("above-one", 1.5, "number"),
        ("int-two", 2, "number"),
        ("negative", -0.25, "number"),
        ("decimal-above-one", {"$decimal": "1.0000000000000000001"}, "number"),
        ("infinity", {"$float": "inf"}, "number"),
        ("string", "0.5", "string"),
    ]
    for name, value, group in mismatches:
        reason = {"code": "type_mismatch", "paths": [["a"]], "types": [group, "boolean"]}
        written = {"$float": "inf"} if name == "infinity" else value
        out.append(
            vector(
                f"degree-mismatch-{name}",
                DEGREES,
                f"{json.dumps(value)} is not a degree: a type mismatch",
                field("a"),
                {"a": value},
                "prod",
                {"result": leaf("a", written, UNKNOWN, reason, read=False)},
            )
        )
    strict = vector(
        "degree-mismatch-strict",
        DEGREES,
        "in strict mode a number outside [0, 1] used as a condition raises",
        field("a"),
        {"a": 1.5},
        "prod",
        {"error": {"code": "type_mismatch", "path": ["a"], "types": ["number", "boolean"]}},
    )
    strict["given"]["strict"] = True
    out.append(strict)
    return out


def form_vectors() -> list[Json]:
    """The JSON form of a logic, and its errors."""
    condition = {"op": "and", "operands": [field("a"), field("b")]}
    data = {"a": 0.5, "b": 0.5}
    hamacher = Logic({"name": "hamacher", "gamma": 1.0}, n.t_prod, n.s_probor)
    result = node(
        "and",
        hamacher.conj((0.5, 0.5), (0.5, 0.5)),
        [leaf("a", 0.5, (0.5, 0.5)), leaf("b", 0.5, (0.5, 0.5))],
    )
    out = [
        vector(
            "hamacher-gamma-1-is-prod",
            FORMS,
            "hamacher with gamma 1 is the product",
            condition,
            data,
            hamacher.form,
            {"result": result},
        ),
        vector(
            "object-form-of-a-name",
            FORMS,
            "a logic without parameters may be written as an object",
            condition,
            data,
            {"name": "prod"},
            {"result": result},
        ),
        vector(
            "alias-product",
            FORMS,
            "fuzzytool's alias product is prod",
            condition,
            data,
            "product",
            {"result": result},
        ),
    ]
    zadeh = Logic({"name": "min", "complement": "zadeh"}, None, None, n.get_complement("zadeh"))
    out.append(
        vector(
            "alias-zadeh",
            FORMS,
            "fuzzytool's alias zadeh is the standard complement",
            {"op": "not", "operand": field("a")},
            {"a": 0.25},
            zadeh.form,
            {"result": node("not", zadeh.neg((0.25, 0.25)), [leaf("a", 0.25, (0.25, 0.25))])},
        )
    )
    errors = [
        ("unknown-logic", "fuzzy", {"code": "registry_error", "name": "fuzzy"}),
        (
            "unknown-complement",
            {"name": "min", "complement": "nope"},
            {"code": "registry_error", "name": "nope"},
        ),
        ("missing-parameter", "yager", {"code": "rule_definition_error"}),
        ("parameter-out-of-range", {"name": "frank", "s": 1}, {"code": "rule_definition_error"}),
        (
            "parameter-of-another-logic",
            {"name": "hamacher", "p": 1},
            {"code": "rule_definition_error"},
        ),
        ("parameter-of-no-logic", {"name": "min", "s": 2}, {"code": "rule_definition_error"}),
        (
            "complement-out-of-range",
            {"name": "min", "complement": {"name": "sugeno", "lam": -1}},
            {"code": "rule_definition_error"},
        ),
    ]
    for name, logic, error in errors:
        out.append(
            vector(
                f"error-{name}",
                FORMS,
                f"the logic {json.dumps(logic)} is refused",
                condition,
                data,
                logic,
                {"error": error},
            )
        )
    return out


def main() -> None:
    """Print the vector file."""
    vectors = (
        norm_vectors()
        + complement_vectors()
        + interval_vectors()
        + degree_vectors()
        + form_vectors()
    )
    document = {"version": 1, "area": "ast", "vectors": vectors}
    print(json.dumps(document, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
