"""Shared hypothesis strategies. Extend these instead of writing parallel ones."""

import datetime as dt

from hypothesis import strategies as st

from rulepy.core.expr import And, Compare, Const, Expr, Field, Not, Or, Scalar

COMPARE_OPS = ("eq", "ne", "lt", "le", "gt", "ge")
I64_MIN, I64_MAX = -(2**63), 2**63 - 1

# Values a Const accepts as-is. NaN is excluded: RulePy treats it as missing, and
# Const rejects it. Timezones are limited to UTC so no tzdata is needed on Windows.
scalars: st.SearchStrategy[Scalar] = st.one_of(
    st.none(),
    st.booleans(),
    st.integers(min_value=I64_MIN, max_value=I64_MAX),
    st.floats(allow_nan=False),
    st.text(),
    st.dates(),
    st.datetimes(timezones=st.none() | st.just(dt.UTC)),
)

path_segments: st.SearchStrategy[str | int] = st.one_of(
    st.text(min_size=1, max_size=8), st.integers(min_value=0, max_value=5)
)

fields: st.SearchStrategy[Field] = st.lists(path_segments, min_size=1, max_size=3).map(
    lambda path: Field(*path)
)

value_exprs: st.SearchStrategy[Expr] = st.one_of(
    fields, scalars.filter(lambda v: v is not None).map(Const)
)

compares: st.SearchStrategy[Expr] = st.builds(
    Compare, st.sampled_from(COMPARE_OPS), value_exprs, value_exprs
)


def _extend(children: st.SearchStrategy[Expr]) -> st.SearchStrategy[Expr]:
    operands = st.lists(children, min_size=2, max_size=4)
    return st.one_of(
        operands.map(lambda xs: And(*xs)),
        operands.map(lambda xs: Or(*xs)),
        st.builds(Not, children),
    )


conditions: st.SearchStrategy[Expr] = st.recursive(
    st.one_of(compares, fields, st.booleans().map(Const)), _extend, max_leaves=12
)
