"""Cases generated with hypothesis, fixed by derandomization (task A.3c).

The strategies are focused on a few keys (`a`, `b`, `c` and a nested map `n`) so that
conditions mostly read data that is present. Hypothesis runs derandomized, without its
example database and without shrinking, so the same hypothesis version always produces
the same cases, and their ids (`generated/<topic>/<n>`) stay stable.
"""

from __future__ import annotations

import datetime as dt
from collections.abc import Callable
from typing import Any

from cases import Case
from hypothesis import HealthCheck, Phase, given, settings
from hypothesis import strategies as st

from rulepy import Rule, RuleSet
from rulepy.core.expr import (
    Abs,
    And,
    Arith,
    Between,
    Compare,
    Const,
    Expr,
    Field,
    In,
    IsNull,
    Matches,
    Neg,
    Not,
    Or,
    StringTest,
)

COUNTS = {"conditions": 1000, "strict": 200, "values": 200, "rulesets": 300, "explanations": 300}

_KEYS = ("a", "b", "c")


def _weighted(*options: tuple[int, st.SearchStrategy[Any]]) -> st.SearchStrategy[Any]:
    """Pick a strategy with the given integer weights."""
    pool = [strategy for weight, strategy in options for _ in range(weight)]
    return st.integers(0, len(pool) - 1).flatmap(lambda i: pool[i])


_small = st.integers(-5, 5)
_numbers = _weighted(
    (6, _small),
    (1, st.integers(-(2**63), 2**63 - 1)),
    (2, st.floats(allow_nan=False, width=64, min_value=-10, max_value=10)),
    (1, st.floats(allow_nan=False, width=64)),
    (2, st.decimals(allow_nan=False, places=2, min_value=-10, max_value=10)),
)
_dates = st.dates(min_value=dt.date(2026, 1, 1), max_value=dt.date(2026, 1, 10))
_scalars = _weighted(
    (8, _numbers),
    (1, st.booleans()),
    (1, st.text(alphabet="abcxyzé ", max_size=3)),
    (1, _dates),
    (
        1,
        st.datetimes(
            min_value=dt.datetime(2026, 1, 1),
            max_value=dt.datetime(2026, 1, 3),
            timezones=st.none() | st.just(dt.UTC),
        ),
    ),
)
_fields = _weighted(
    (6, st.sampled_from(_KEYS).map(lambda k: Field(k))),
    (2, st.sampled_from(_KEYS).map(lambda k: Field("n", k))),
    (1, st.just(Field("missing"))),
)
_maybe = _weighted((1, st.none()), (7, _scalars))
_data = st.fixed_dictionaries(
    {"a": _maybe, "b": _maybe},
    optional={
        "c": _maybe,
        "n": _weighted(
            (1, st.none()),
            (5, st.fixed_dictionaries({k: _maybe for k in _KEYS})),
        ),
    },
)


def _extend_numeric(children: st.SearchStrategy[Expr]) -> st.SearchStrategy[Expr]:
    return st.one_of(
        st.builds(Arith, st.sampled_from(("add", "sub", "mul", "div")), children, children),
        st.builds(Abs, children),
        st.builds(Neg, children),
    )


_numeric = st.recursive(st.one_of(_fields, _numbers.map(Const)), _extend_numeric, max_leaves=3)
_values = _weighted((4, _fields), (3, _scalars.map(Const)), (1, _numeric))
_in_values = st.one_of(
    st.lists(_numbers, min_size=1, max_size=4),
    st.lists(st.text(alphabet="abcxyzé", max_size=2), min_size=1, max_size=4),
    st.lists(
        st.dates(min_value=dt.date(2025, 1, 1), max_value=dt.date(2027, 1, 1)),
        min_size=1,
        max_size=3,
    ),
    st.lists(st.booleans(), min_size=1, max_size=2),
)
_predicates = st.one_of(
    st.builds(Compare, st.sampled_from(("eq", "ne", "lt", "le", "gt", "ge")), _values, _values),
    st.builds(In, _values, _in_values),
    st.builds(
        Between, _values, _numeric, _numeric, st.sampled_from(["both", "left", "right", "none"])
    ),
    st.builds(IsNull, _values),
    st.builds(Matches, _values, st.sampled_from([r"^a", r"\d", "(?i)X", "é$"])),
    st.builds(
        StringTest,
        st.sampled_from(("starts_with", "ends_with", "contains")),
        _values,
        st.text(alphabet="abcé", max_size=2),
    ),
)


def _extend(children: st.SearchStrategy[Expr]) -> st.SearchStrategy[Expr]:
    operands = st.lists(children, min_size=2, max_size=3)
    return st.one_of(
        operands.map(lambda xs: And(*xs)),
        operands.map(lambda xs: Or(*xs)),
        st.builds(Not, children),
    )


_conditions = st.recursive(
    st.one_of(_predicates, _fields, st.booleans().map(Const)), _extend, max_leaves=6
)
_payloads = st.dictionaries(
    st.sampled_from(["v", "w"]), st.one_of(_fields, _numbers.map(Const), _numeric), max_size=2
)


@st.composite
def _rulesets(draw: st.DrawFn) -> RuleSet:
    count = draw(st.integers(1, 4))
    policy = draw(st.sampled_from(["first", "priority", "unique", "any", "rule_order", "collect"]))
    aggregate = (
        draw(st.sampled_from([None, "sum", "min", "max", "count"])) if policy == "collect" else None
    )
    rules = [
        Rule(
            f"r{i}",
            draw(_conditions),
            then=draw(_payloads) or None,
            priority=draw(st.integers(0, 2)),
        )
        for i in range(count)
    ]
    return RuleSet(
        rules,
        hit_policy=policy,
        aggregate=aggregate,
        strict=draw(_weighted((4, st.just(False)), (1, st.just(True)))),
    )


@st.composite
def _explained(draw: st.DrawFn) -> RuleSet:
    whens = draw(st.lists(_conditions, min_size=1, max_size=3))
    return RuleSet([Rule(f"r{i}", w) for i, w in enumerate(whens)], hit_policy="rule_order")


def _collect(strategy: st.SearchStrategy[Any], count: int) -> list[Any]:
    """Draw `count` examples, reproducibly."""
    found: list[Any] = []

    @settings(
        max_examples=count,
        derandomize=True,
        database=None,
        phases=[Phase.generate],
        deadline=None,
        suppress_health_check=list(HealthCheck),
    )
    @given(strategy)
    def draw(example: Any) -> None:
        found.append(example)

    draw()
    return found[:count]


def _ruleset(rs: RuleSet) -> Callable[[], RuleSet]:
    return lambda: rs


def _unique(pairs: list[Any]) -> list[Any]:
    """Drop repeated examples, keeping the first (rule sets are compared rule by rule)."""
    seen: set[str] = set()
    kept: list[Any] = []
    for pair in pairs:
        key = repr(
            [
                (x.hit_policy, x.aggregate, x.strict, [repr(r) for r in x])
                if isinstance(x, RuleSet)
                else x
                for x in pair
            ]
        )
        if key not in seen:
            seen.add(key)
            kept.append(pair)
    return kept


def generated_cases() -> list[Case]:
    """Every generated case, in a fixed order."""
    spec_eval = ("ast.md#evaluation-semantics", "ast.md#results-and-reasons")
    cases: list[Case] = []
    for i, (expr, data) in enumerate(
        _unique(_collect(st.tuples(_conditions, _data), COUNTS["conditions"]))
    ):
        cases.append(
            Case(
                f"generated/conditions/{i:04d}",
                spec_eval,
                "a generated condition",
                "condition",
                expr=expr,
                input=data,
            )
        )
    for i, (expr, data) in enumerate(
        _unique(_collect(st.tuples(_conditions, _data), COUNTS["strict"]))
    ):
        cases.append(
            Case(
                f"generated/strict/{i:04d}",
                (*spec_eval, "errors.md#codes"),
                "a generated condition in strict mode",
                "condition",
                expr=expr,
                input=data,
                strict=True,
            )
        )
    for i, (expr, data) in enumerate(
        _unique(_collect(st.tuples(_numeric, _data), COUNTS["values"]))
    ):
        cases.append(
            Case(
                f"generated/values/{i:04d}",
                ("ast.md#node-semantics",),
                "a generated arithmetic value",
                "value",
                expr=expr,
                input=data,
            )
        )
    for i, (rs, data) in enumerate(
        _unique(_collect(st.tuples(_rulesets(), _data), COUNTS["rulesets"]))
    ):
        cases.append(
            Case(
                f"generated/rulesets/{i:04d}",
                ("rules.md#evaluation", "json-format.md#decisions"),
                "a generated rule set",
                "evaluate",
                ruleset=_ruleset(rs),
                input=data,
            )
        )
    for i, (rs, data) in enumerate(
        _collect(st.tuples(_explained(), _data), COUNTS["explanations"])
    ):
        cases.append(
            Case(
                f"generated/explanations/{i:04d}",
                ("explain.md#requirements", "json-format.md#explanations"),
                "a generated explanation",
                "explain",
                ruleset=_ruleset(rs),
                input=data,
            )
        )
    return cases
