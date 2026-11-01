"""Hypothesis strategies: random condition and value trees built with `F` and operators."""

import datetime as dt
from decimal import Decimal
from typing import cast

from cleanrules import Const, Expr, F
from cleanrules._expr import Closed
from hypothesis import strategies as st

fields = st.sampled_from([F.a, F.b, F.c, F.customer.age, F.orders[0].total, F["key with spaces"]])

numbers = st.one_of(
    st.integers(-1000, 1000),
    st.floats(-1e6, 1e6, allow_nan=False),
    st.decimals(-1000, 1000, places=2, allow_nan=False, allow_infinity=False),
)
texts = st.text(st.characters(codec="utf-8"), max_size=5)
CLOSED: list[Closed] = ["both", "left", "right", "none"]
dates = st.dates(dt.date(2000, 1, 1), dt.date(2030, 12, 31))


def _numeric_leaf() -> st.SearchStrategy[Expr]:
    return st.one_of(fields, numbers.map(Const))


value_exprs: st.SearchStrategy[Expr] = st.recursive(
    _numeric_leaf(),
    lambda inner: st.one_of(
        st.tuples(inner, inner).map(lambda p: p[0] + p[1]),
        st.tuples(inner, inner).map(lambda p: p[0] - p[1]),
        st.tuples(inner, inner).map(lambda p: p[0] * p[1]),
        st.tuples(inner, inner).map(lambda p: p[0] / p[1]),
        inner.map(lambda e: -e),
        inner.map(abs),
    ),
    max_leaves=6,
)


def _predicates() -> st.SearchStrategy[Expr]:
    comparable = st.one_of(value_exprs, fields)
    constants = st.one_of(numbers, texts, dates)
    return st.one_of(
        st.tuples(comparable, comparable).map(lambda p: p[0] > p[1]),
        st.tuples(fields, constants).map(lambda p: p[0] == p[1]),
        st.tuples(fields, numbers).map(lambda p: p[0] <= p[1]),
        st.tuples(fields, st.lists(st.integers(-5, 5), min_size=1, max_size=4)).map(
            lambda p: p[0].is_in(p[1])
        ),
        st.tuples(fields, numbers, numbers, st.sampled_from(["both", "left", "right", "none"])).map(
            lambda p: p[0].between(p[1], p[2], closed=cast(Closed, p[3]))
        ),
        fields.map(lambda f: f.is_null()),
        st.tuples(fields, texts).map(lambda p: p[0].starts_with(p[1])),
        st.tuples(fields, texts).map(lambda p: p[0].contains(p[1])),
        st.tuples(fields, st.sampled_from(["^a", "b$", "(?i)x+"])).map(
            lambda p: p[0].matches(p[1])
        ),
        fields,
    )


conditions: st.SearchStrategy[Expr] = st.recursive(
    _predicates(),
    lambda inner: st.one_of(
        st.tuples(inner, inner).map(lambda p: p[0] & p[1]),
        st.tuples(inner, inner).map(lambda p: p[0] | p[1]),
        inner.map(lambda e: ~e),
    ),
    max_leaves=8,
)


def _fits(value: Decimal) -> bool:
    """Whether a decimal fits in the model: scale up to 38, coefficient below 10**38."""
    if value.is_infinite():
        return True
    _, digits, exponent = value.as_tuple()
    assert isinstance(exponent, int)
    return -exponent <= 38 and len(digits) + max(0, exponent) <= 38


decimals = st.decimals(allow_nan=False, places=None).filter(_fits)
__all__ = ["Decimal", "conditions", "decimals", "fields", "value_exprs"]
