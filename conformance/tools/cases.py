"""Hand-picked cases for the exporter: what to give the Python v1, and why.

Each case names the spec sections it checks. The exporter runs the v1 on it and
records the output as the expected value. Cases where the spec deviates on purpose
from the v1 (`docs/PLAN.md`, task A.3) are not listed here.
"""

from __future__ import annotations

import datetime as dt
from collections.abc import Callable
from dataclasses import dataclass, field
from decimal import Decimal
from typing import Any
from zoneinfo import ZoneInfo

from rulepy import F, Rule, RuleSet
from rulepy.core.expr import Compare, Const, Expr


@dataclass(frozen=True)
class Case:
    """One vector to export: `kind` selects how the v1 is run (see export.py)."""

    id: str
    spec: tuple[str, ...]
    description: str
    kind: str  # "condition", "value", "evaluate", "explain" or "summary"
    expr: Expr | None = None
    ruleset: Callable[[], RuleSet] | None = None
    input: Any = None
    inputs: list[Any] | None = None
    strict: bool = False
    extra: dict[str, Any] = field(default_factory=dict[str, Any])


CASES: list[Case] = []


def cond(id: str, spec: str, description: str, expr: Expr, data: Any, strict: bool = False) -> None:
    """Add a vector that evaluates one condition."""
    CASES.append(Case(id, (spec,), description, "condition", expr=expr, input=data, strict=strict))


def val(id: str, spec: str, description: str, expr: Expr, data: Any, strict: bool = False) -> None:
    """Add a vector that evaluates one value node."""
    CASES.append(Case(id, (spec,), description, "value", expr=expr, input=data, strict=strict))


def evaluate(
    id: str,
    spec: tuple[str, ...],
    description: str,
    rs: Callable[[], RuleSet],
    data: Any = None,
    *,
    inputs: list[Any] | None = None,
) -> None:
    """Add a vector that evaluates a rule set on one input or a batch."""
    CASES.append(Case(id, spec, description, "evaluate", ruleset=rs, input=data, inputs=inputs))


def explain(
    id: str, spec: tuple[str, ...], description: str, rs: Callable[[], RuleSet], data: Any
) -> None:
    """Add a vector that explains a decision."""
    CASES.append(Case(id, spec, description, "explain", ruleset=rs, input=data))


def summary(id: str, spec: tuple[str, ...], description: str, rs: Callable[[], RuleSet]) -> None:
    """Add a vector that summarizes a rule set."""
    CASES.append(Case(id, spec, description, "summary", ruleset=rs))


TRUTH = "ast.md#truth-values"
MISSING = "ast.md#missing-values"
TYPES = "ast.md#types"
NODES = "ast.md#node-semantics"
READING = "ast.md#reading-data"
REASONS = "ast.md#results-and-reasons"
EVALUATION = "rules.md#evaluation"
PAYLOADS = "rules.md#when-payloads-are-evaluated"
OUTPUTS = "rules.md#outputs"
AGGREGATES = "rules.md#collect-aggregates"
STRICT = "rules.md#strict-mode-and-errors"
DECISIONS = "json-format.md#decisions"

# --- Kleene truth tables ------------------------------------------------------------------

_WORD = {True: "true", False: "false", None: "unknown"}
for a in (True, False, None):
    for b in (True, False, None):
        data = {"a": a, "b": b}
        cond(
            f"ast/truth/and-{_WORD[a]}-{_WORD[b]}",
            TRUTH,
            f"{_WORD[a]} and {_WORD[b]}",
            F.a & F.b,
            data,
        )
        cond(
            f"ast/truth/or-{_WORD[a]}-{_WORD[b]}",
            TRUTH,
            f"{_WORD[a]} or {_WORD[b]}",
            F.a | F.b,
            data,
        )
    cond(f"ast/truth/not-{_WORD[a]}", TRUTH, f"not {_WORD[a]}", ~F.a, {"a": a})

# --- Reading data -------------------------------------------------------------------------

cond(
    "ast/reading/nested-path",
    READING,
    "a nested path reads through maps",
    F.customer.address.city == "Madrid",
    {"customer": {"address": {"city": "Madrid"}}},
)
cond(
    "ast/reading/list-index",
    READING,
    "an integer segment indexes a list",
    F.items[1].price > 10,
    {"items": [{"price": 5}, {"price": 20}]},
)
cond(
    "ast/reading/index-out-of-range",
    READING,
    "an index out of range is missing",
    F.items[2].price > 10,
    {"items": [{"price": 5}]},
)
cond(
    "ast/reading/integer-on-map",
    READING,
    "an integer segment never looks up a map key",
    F.items[0] == "x",
    {"items": {"0": "x"}},
)
cond(
    "ast/reading/string-on-list",
    READING,
    "a string segment on a list is missing",
    F.items.price > 1,
    {"items": [1, 2]},
)
cond(
    "ast/reading/underscore-key",
    READING,
    "keys starting with an underscore are ordinary keys",
    F["_id"] == 7,
    {"_id": 7},
)
cond(
    "ast/reading/attribute-of-scalar",
    READING,
    "a segment on a scalar is missing",
    F.name.length > 1,
    {"name": "abc"},
)

# --- Missing values -----------------------------------------------------------------------

cond(
    "ast/missing/missing-field",
    MISSING,
    "a missing field makes a comparison unknown",
    F.age >= 18,
    {},
)
cond(
    "ast/missing/explicit-null",
    MISSING,
    "an explicit null makes a comparison unknown",
    F.age >= 18,
    {"age": None},
)
cond(
    "ast/missing/null-in-the-middle",
    MISSING,
    "a null in the middle of a path makes the field null",
    F.customer.age >= 18,
    {"customer": None},
)
cond("ast/missing/null-input", MISSING, "a null input is null at the empty path", F.age >= 18, None)
cond(
    "ast/missing/is-null-missing",
    MISSING,
    "is_null is true for a missing field",
    F.age.is_null(),
    {},
)
cond(
    "ast/missing/is-null-value",
    MISSING,
    "is_null is false for a present value",
    F.age.is_null(),
    {"age": 0},
)
cond(
    "ast/missing/null-before-types",
    MISSING,
    "null is checked before types",
    F.age > "x",
    {"age": None},
)
cond(
    "ast/missing/strict-missing",
    MISSING,
    "strict mode raises on a missing field",
    F.age >= 18,
    {},
    strict=True,
)
cond(
    "ast/missing/strict-missing-under-is-null",
    MISSING,
    "strict mode raises on a missing field even under is_null",
    F.age.is_null(),
    {},
    strict=True,
)
cond(
    "ast/missing/strict-null-in-the-middle",
    MISSING,
    "strict mode accepts a null in the middle of a path",
    F.customer.age >= 18,
    {"customer": None},
    strict=True,
)
cond(
    "ast/missing/field-as-condition-not-boolean",
    MISSING,
    "a non-boolean field used as a condition is a type mismatch",
    F.age,
    {"age": 3},
)

# --- Types --------------------------------------------------------------------------------

cond("ast/types/int-equals-float", TYPES, "ints and floats compare by value", F.x == 1.0, {"x": 1})
cond(
    "ast/types/decimal-vs-float",
    TYPES,
    "the decimal 0.1 does not equal the float 0.1",
    F.x == 0.1,
    {"x": Decimal("0.1")},
)
cond(
    "ast/types/decimal-equals-int",
    TYPES,
    "a decimal equals an int of the same value",
    F.x == 2,
    {"x": Decimal("2.00")},
)
cond(
    "ast/types/big-int-vs-float",
    TYPES,
    "2^53 + 1 does not equal the float 2^53",
    F.x == float(2**53),
    {"x": 2**53 + 1},
)
cond(
    "ast/types/string-vs-number",
    TYPES,
    "comparing a string with a number is a type mismatch",
    F.x > 1,
    {"x": "2"},
)
cond(
    "ast/types/order-booleans", TYPES, "ordering booleans is unorderable", F.x > False, {"x": True}
)
cond(
    "ast/types/string-code-point-order",
    TYPES,
    "strings order by Unicode code point",
    F.x < "a",
    {"x": "Z"},
)
cond(
    "ast/types/string-non-ascii-order",
    TYPES,
    "a non-ASCII letter orders after z",
    F.x > "z",
    {"x": "é"},
)
cond(
    "ast/types/date-vs-datetime",
    TYPES,
    "a date and a datetime are different groups",
    F.x == dt.date(2026, 10, 31),
    {"x": dt.datetime(2026, 10, 31)},
)
cond(
    "ast/types/aware-by-instant",
    TYPES,
    "aware datetimes compare by instant",
    F.x == dt.datetime(2026, 10, 31, 8, 30, tzinfo=dt.UTC),
    {"x": dt.datetime(2026, 10, 31, 9, 30, tzinfo=ZoneInfo("Europe/Madrid"))},
)
cond(
    "ast/types/naive-vs-aware",
    TYPES,
    "naive and aware datetimes are different groups",
    F.x < dt.datetime(2026, 1, 1, tzinfo=dt.UTC),
    {"x": dt.datetime(2025, 1, 1)},
)
cond("ast/types/list-is-other", TYPES, "comparing a list is a type mismatch", F.x == 1, {"x": [1]})
cond(
    "ast/types/strict-mismatch",
    TYPES,
    "strict mode raises on a type mismatch",
    F.x > 1,
    {"x": "2"},
    strict=True,
)

# --- Node semantics -----------------------------------------------------------------------

cond(
    "ast/nodes/in-true",
    NODES,
    "in is true when the operand is one of the values",
    F.c.is_in(["ES", "FR"]),
    {"c": "ES"},
)
cond("ast/nodes/in-false", NODES, "in is false otherwise", F.c.is_in(["ES", "FR"]), {"c": "PT"})
cond(
    "ast/nodes/in-other-group",
    NODES,
    "in with an operand of another group is a type mismatch",
    F.c.is_in([1, 2]),
    {"c": "1"},
)
cond(
    "ast/nodes/in-int-and-float",
    NODES,
    "an int operand matches equal float values in in",
    F.c.is_in([1, 1.0, 2]),
    {"c": 1},
)
for closed, x in (("both", 18), ("left", 65), ("right", 18), ("none", 30)):
    cond(
        f"ast/nodes/between-{closed}-{x}",
        NODES,
        f"between closed={closed} with x={x}",
        F.age.between(18, 65, closed=closed),
        {"age": x},
    )
cond(
    "ast/nodes/between-field-bounds",
    NODES,
    "between takes fields as bounds",
    F.x.between(F.lo, F.hi),
    {"x": 5, "lo": 1, "hi": 4},
)
cond(
    "ast/nodes/matches-search",
    NODES,
    "matches searches anywhere in the string",
    F.email.matches(r"@example\.com"),
    {"email": "ana@example.com"},
)
cond(
    "ast/nodes/matches-case",
    NODES,
    "matches is case-sensitive",
    F.email.matches("EXAMPLE"),
    {"email": "ana@example.com"},
)
cond(
    "ast/nodes/matches-inline-flag",
    NODES,
    "the (?i) flag makes matches case-insensitive",
    F.email.matches("(?i)EXAMPLE"),
    {"email": "ana@example.com"},
)
cond(
    "ast/nodes/contains-literal",
    NODES,
    "contains takes literal text",
    F.code.contains("a.c"),
    {"code": "xabcx"},
)
cond("ast/nodes/starts-with", NODES, "starts_with", F.code.starts_with("ES-"), {"code": "ES-001"})
cond(
    "ast/nodes/ends-with-mismatch",
    NODES,
    "a string test on a number is a type mismatch",
    F.code.ends_with("1"),
    {"code": 1},
)
val("ast/nodes/div-int", NODES, "div returns a float", F.a / F.b, {"a": 7, "b": 2})
val("ast/nodes/div-by-zero", NODES, "division by zero is null", F.a / F.b, {"a": 7, "b": 0})
val(
    "ast/nodes/int-overflow",
    NODES,
    "an int result outside 64 bits is null",
    F.a * F.b,
    {"a": 2**62, "b": 4},
)
val(
    "ast/nodes/decimal-to-float",
    NODES,
    "decimal operands are converted to float",
    F.a + 1,
    {"a": Decimal("0.5")},
)
val("ast/nodes/inf-kept", NODES, "inf results are kept", F.a * 2, {"a": float("inf")})
val(
    "ast/nodes/nan-result",
    NODES,
    "a NaN result is null",
    F.a - F.b,
    {"a": float("inf"), "b": float("inf")},
)
val("ast/nodes/abs-neg", NODES, "abs and neg", abs(-F.a), {"a": 3})
val(
    "ast/nodes/arith-not-number",
    NODES,
    "arithmetic on a string is null with a type mismatch",
    F.a + 1,
    {"a": "x"},
)
cond(
    "ast/reasons/repeated-field", REASONS, "a reason keeps duplicate paths", F.a > F.a, {"a": True}
)
cond(
    "ast/reasons/first-unknown",
    REASONS,
    "an unknown and takes the reason of its first unknown operand",
    (F.a > 1) & (F.b > 1),
    {"b": "x"},
)


# --- Rule sets ----------------------------------------------------------------------------


def _pricing(
    policy: str, aggregate: Any = None, strict: bool = False, base_priority: int = 0
) -> Callable[[], RuleSet]:
    def build() -> RuleSet:
        return RuleSet(
            [
                Rule("vip", F.customer.years > 5, then={"discount": Decimal("0.15")}, priority=10),
                Rule("big", F.order.total > 1000, then={"discount": Decimal("0.10")}, priority=10),
                Rule(
                    "base", Const(True), then={"discount": Decimal("0.00")}, priority=base_priority
                ),
            ],
            hit_policy=policy,
            aggregate=aggregate,
            strict=strict,
        )

    return build


_BOTH = {"customer": {"years": 7}, "order": {"total": 1500}}
_ONE = {"customer": {"years": 7}, "order": {"total": 100}}
for policy in ("first", "priority", "rule_order", "collect"):
    evaluate(
        f"rules/policies/{policy.replace('_', '-')}",
        (EVALUATION, DECISIONS),
        f"{policy} with two matching rules and a default",
        _pricing(policy),
        _BOTH,
    )
evaluate(
    "rules/policies/unique-one",
    (EVALUATION,),
    "unique with exactly one rule firing",
    lambda: RuleSet([Rule("a", F.x > 1), Rule("b", F.x < 0)], hit_policy="unique"),
    {"x": 2},
)
evaluate(
    "rules/policies/unique-several",
    (EVALUATION, STRICT),
    "unique raises when several rules fire",
    _pricing("unique"),
    _ONE,
)
evaluate(
    "rules/policies/any-agree",
    (EVALUATION, PAYLOADS),
    "any with agreeing outputs reports the first",
    lambda: RuleSet(
        [Rule("a", F.x > 1, then={"y": 1}), Rule("b", F.x > 0, then={"y": 1.0})], hit_policy="any"
    ),
    {"x": 2},
)
evaluate(
    "rules/policies/any-disagree",
    (EVALUATION, STRICT),
    "any raises when outputs differ",
    _pricing("any"),
    _ONE,
)
evaluate(
    "rules/policies/priority-tie",
    (EVALUATION,),
    "priority ties go to rule order",
    _pricing("priority"),
    _BOTH,
)
evaluate(
    "rules/policies/nothing-fires",
    (EVALUATION, OUTPUTS),
    "first with no rule firing selects nothing",
    lambda: RuleSet([Rule("a", F.x > 1)]),
    {"x": 0},
)
for agg in ("sum", "min", "max", "count"):
    evaluate(
        f"rules/aggregates/{agg}",
        (AGGREGATES,),
        f"collect with {agg}",
        lambda agg=agg: RuleSet(
            [
                Rule("a", F.x > 0, then={"n": 2, "m": None}),
                Rule("b", F.x > 1, then={"n": 3.5, "m": 1}),
                Rule("c", F.x > 9, then={"n": 100}),
            ],
            hit_policy="collect",
            aggregate=agg,
        ),
        {"x": 5},
    )
evaluate(
    "rules/aggregates/nothing-selected",
    (AGGREGATES,),
    "an aggregate with nothing selected is null",
    lambda: RuleSet([Rule("a", F.x > 9, then={"n": 1})], hit_policy="collect", aggregate="sum"),
    {"x": 0},
)
evaluate(
    "rules/aggregates/sum-overflow",
    (AGGREGATES,),
    "an int sum outside 64 bits is null",
    lambda: RuleSet(
        [Rule("a", Const(True), then={"n": 2**62}), Rule("b", Const(True), then={"n": 2**62})],
        hit_policy="collect",
        aggregate="sum",
    ),
    {},
)
evaluate(
    "rules/aggregates/sum-not-number",
    (AGGREGATES,),
    "sum of a string raises",
    lambda: RuleSet(
        [Rule("a", Const(True), then={"n": "x"})], hit_policy="collect", aggregate="sum"
    ),
    {},
)
evaluate(
    "rules/aggregates/decimal-to-float",
    (AGGREGATES,),
    "decimals are converted to float before sum",
    lambda: RuleSet(
        [
            Rule("a", Const(True), then={"n": Decimal("0.1")}),
            Rule("b", Const(True), then={"n": Decimal("0.2")}),
        ],
        hit_policy="collect",
        aggregate="sum",
    ),
    {},
)
evaluate(
    "rules/payloads/passthrough",
    (PAYLOADS, OUTPUTS),
    "a field in a payload passes a list and a map through",
    lambda: RuleSet(
        [
            Rule(
                "a",
                Const(True),
                then={"items": F.items, "who": F.customer, "total": F.order.total * 0.9},
            )
        ]
    ),
    {"items": [1, "x"], "customer": {"name": "Ana"}, "order": {"total": 100}},
)
evaluate(
    "rules/payloads/null-payload-value",
    (PAYLOADS,),
    "a null payload value keeps its reason",
    lambda: RuleSet([Rule("a", Const(True), then={"v": F.missing})]),
    {},
)
evaluate(
    "rules/payloads/order-kept",
    (PAYLOADS, OUTPUTS),
    "payload keys keep their order",
    lambda: RuleSet([Rule("a", Const(True), then={"b": 1, "a": 2, "10": 3, "2": 4})]),
    {},
)
evaluate(
    "rules/strict/unselected-payload",
    (STRICT, PAYLOADS),
    "strict mode does not evaluate the payload of a rule that is not selected",
    lambda: RuleSet(
        [Rule("a", Const(True), then={"v": 1}), Rule("b", Const(True), then={"v": F.missing})],
        strict=True,
    ),
    {},
)
evaluate(
    "rules/strict/selected-payload",
    (STRICT, PAYLOADS),
    "strict mode raises on the payload of a selected rule",
    lambda: RuleSet([Rule("a", Const(True), then={"v": F.missing})], strict=True),
    {},
)
evaluate(
    "rules/strict/conditions-first",
    (STRICT,),
    "strict mode raises the first condition error in rule order",
    lambda: RuleSet([Rule("a", F.x > 1), Rule("b", F.y > 1)], strict=True),
    {"x": "s"},
)
evaluate(
    "rules/batch/row",
    (STRICT,),
    "an error in a batch carries the row",
    lambda: RuleSet([Rule("a", F.x > 1)], strict=True),
    inputs=[{"x": 2}, {"x": 0}, {}],
)
evaluate(
    "rules/batch/decisions",
    (EVALUATION,),
    "a batch gives one decision per input",
    lambda: RuleSet([Rule("a", F.x > 1, then={"ok": True})]),
    inputs=[{"x": 2}, {"x": 0}, {}],
)

# --- Explanations -------------------------------------------------------------------------

EXPLAIN = ("explain.md#requirements", "explain.md#causes", "json-format.md#explanations")
explain(
    "explain/requirements/priority-example",
    (
        *EXPLAIN,
        "explain.md#counterfactuals",
        "explain.md#notes-for-rules-that-fired-but-were-not-selected",
        "explain.md#keeping-only-the-changes-that-work",
    ),
    "the worked example of explain.md",
    lambda: RuleSet(
        [
            Rule("a", (F.x > 10) & ~(F.y <= 3)),
            Rule("b", F.x.between(0, 5)),
            Rule("c", (F.x > 1) & ~(F.x > 1)),
            Rule("d", F.x > 1, priority=1),
            Rule("e", F.x > 0, priority=5),
        ],
        hit_policy="priority",
    ),
    {"x": 8, "y": 2},
)
explain(
    "explain/requirements/unknown-and-dates",
    (*EXPLAIN, "explain.md#rule-status", "explain.md#counterfactuals"),
    "unknown rules and a gap in days",
    lambda: RuleSet(
        [
            Rule("late", F.due < dt.date(2026, 10, 1), then={"fee": Decimal("5.00")}),
            Rule("vip", (F.customer.years > 5) | F.customer.vip, then={"discount": 0.1}),
            Rule("none", F.missing.code == "X"),
        ],
        hit_policy="collect",
        aggregate="sum",
    ),
    {"due": dt.date(2026, 10, 31), "customer": {"years": 3, "vip": None}},
)
explain(
    "explain/requirements/de-morgan",
    (*EXPLAIN, "explain.md#building-the-requirement"),
    "not over and becomes any of the negated operands",
    lambda: RuleSet([Rule("r", ~((F.a > 1) & (F.b > 1)))]),
    {"a": 5, "b": 5},
)
explain(
    "explain/requirements/absorption",
    (*EXPLAIN, "explain.md#simplifying"),
    "a and (a or b) needs only a",
    lambda: RuleSet([Rule("r", (F.a > 1) & ((F.a > 1) | (F.b > 1)))]),
    {"a": 0, "b": 0},
)
explain(
    "explain/requirements/fired-all",
    (*EXPLAIN,),
    "a rule that fired lists every cause",
    lambda: RuleSet([Rule("r", (F.a > 1) & (F.b > 1))]),
    {"a": 5, "b": 5},
)
explain(
    "explain/requirements/or-fired",
    (*EXPLAIN,),
    "an or that fired lists the true operands",
    lambda: RuleSet([Rule("r", (F.a > 1) | (F.b > 1) | (F.c > 1))]),
    {"a": 5, "b": 0, "c": 5},
)
explain(
    "explain/notes/same-priority",
    ("explain.md#notes-for-rules-that-fired-but-were-not-selected",),
    "same priority, earlier rule wins",
    _pricing("priority"),
    _BOTH,
)
explain(
    "explain/notes/earlier-rule",
    ("explain.md#notes-for-rules-that-fired-but-were-not-selected",),
    "first: the earlier rule wins",
    _pricing("first"),
    _BOTH,
)
explain(
    "explain/notes/any-agrees",
    ("explain.md#notes-for-rules-that-fired-but-were-not-selected",),
    "any: the outputs agree",
    lambda: RuleSet(
        [Rule("a", F.x > 1, then={"y": 1}), Rule("b", F.x > 0, then={"y": 1})], hit_policy="any"
    ),
    {"x": 2},
)
explain(
    "explain/counterfactuals/at-the-limit",
    ("explain.md#counterfactuals",),
    "a strict comparison exactly at its limit has a gap of zero",
    lambda: RuleSet([Rule("r", F.x > 10)]),
    {"x": 10},
)
explain(
    "explain/counterfactuals/constant-on-the-left",
    ("explain.md#counterfactuals",),
    "a constant on the left is flipped",
    lambda: RuleSet([Rule("r", Compare("lt", Const(10), F.x))]),
    {"x": 4},
)
explain(
    "explain/counterfactuals/negated-eq",
    ("explain.md#counterfactuals",),
    "a negated equality has a counterfactual without a gap",
    lambda: RuleSet([Rule("r", ~(F.x == 3))]),
    {"x": 3},
)
explain(
    "explain/counterfactuals/decimal-gap",
    ("explain.md#counterfactuals",),
    "a decimal gap is exact",
    lambda: RuleSet([Rule("r", F.x >= Decimal("10.05"))]),
    {"x": Decimal("9.9")},
)
explain(
    "explain/counterfactuals/float-gap",
    ("explain.md#counterfactuals",),
    "a float gap",
    lambda: RuleSet([Rule("r", F.x <= 2.5)]),
    {"x": 3.75},
)
explain(
    "explain/counterfactuals/datetime-gap",
    ("explain.md#counterfactuals",),
    "a datetime gap in microseconds",
    lambda: RuleSet([Rule("r", F.t >= dt.datetime(2026, 10, 31, 12))]),
    {"t": dt.datetime(2026, 10, 31, 9, 30)},
)
explain(
    "explain/counterfactuals/decimal-and-float",
    ("explain.md#counterfactuals",),
    "a decimal and a float have no gap",
    lambda: RuleSet([Rule("r", F.x > 1.5)]),
    {"x": Decimal("1")},
)

# --- Summaries ----------------------------------------------------------------------------

summary(
    "explain/summaries/pricing",
    ("explain.md#summaries", "json-format.md#summaries"),
    "fields, outputs and tags of a rule set",
    lambda: RuleSet(
        [
            Rule(
                "vip",
                (F.customer.years > 5) & (F.order.total > 1000),
                then={"discount": 0.15, "total": F.order.total * 0.85},
                tags=["pricing", "loyalty"],
            ),
            Rule(
                "blocked",
                F.customer.blocked | F.customer.is_null(),
                then={"discount": 0},
                priority=10,
                tags=["risk"],
            ),
        ],
        hit_policy="priority",
        strict=True,
    ),
)
summary(
    "explain/summaries/empty", ("explain.md#summaries",), "an empty rule set", lambda: RuleSet([])
)
summary(
    "explain/summaries/between-order",
    ("explain.md#summaries",),
    "between visits operand, low and high",
    lambda: RuleSet(
        [Rule("r", F.x.between(F.lo, F.hi), then={"v": F.y})],
        hit_policy="collect",
        aggregate="count",
    ),
)

evaluate(
    "rules/policies/priority-later-rule-wins",
    (EVALUATION, DECISIONS),
    "priority selects a later rule with a higher priority",
    _pricing("priority", base_priority=20),
    _BOTH,
)
cond(
    "ast/missing/strict-missing-nested",
    MISSING,
    "the missing_field error carries the field's full path",
    F.customer.age >= 18,
    {},
    strict=True,
)
explain(
    "explain/counterfactuals/datetime-gap-whole-days",
    ("explain.md#counterfactuals",),
    "a datetime gap of whole days is still in microseconds",
    lambda: RuleSet([Rule("r", F.t > dt.datetime(2026, 11, 1, 9, 30))]),
    {"t": dt.datetime(2026, 10, 31, 9, 30)},
)
explain(
    "explain/counterfactuals/aware-gap-across-dst",
    ("explain.md#counterfactuals",),
    "the v1 subtracts wall-clock times across a DST change; this case must be skipped",
    lambda: RuleSet(
        [Rule("r", F.t >= dt.datetime(2026, 10, 25, 4, tzinfo=ZoneInfo("Europe/Madrid")))]
    ),
    {"t": dt.datetime(2026, 10, 25, 1, tzinfo=ZoneInfo("Europe/Madrid"))},
)
