"""Shared hypothesis strategies. Extend these instead of writing parallel ones."""

import datetime as dt

from hypothesis import strategies as st

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
    Scalar,
    StringTest,
)

COMPARE_OPS = ("eq", "ne", "lt", "le", "gt", "ge")
STRING_OPS = ("starts_with", "ends_with", "contains")
ARITH_OPS = ("add", "sub", "mul", "div")
I64_MIN, I64_MAX = -(2**63), 2**63 - 1

# Values a Const accepts as-is. NaN is excluded: RulePy treats it as missing, and
# Const rejects it. Timezones are limited to UTC so no tzdata is needed on Windows.
numbers: st.SearchStrategy[int | float] = st.one_of(
    st.integers(min_value=I64_MIN, max_value=I64_MAX), st.floats(allow_nan=False)
)

scalars: st.SearchStrategy[Scalar] = st.one_of(
    st.none(),
    st.booleans(),
    numbers,
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


def _extend_numeric(children: st.SearchStrategy[Expr]) -> st.SearchStrategy[Expr]:
    return st.one_of(
        st.builds(Arith, st.sampled_from(ARITH_OPS), children, children),
        st.builds(Abs, children),
        st.builds(Neg, children),
    )


numeric_exprs: st.SearchStrategy[Expr] = st.recursive(
    st.one_of(fields, numbers.map(Const)), _extend_numeric, max_leaves=4
)

value_exprs: st.SearchStrategy[Expr] = st.one_of(
    fields, scalars.filter(lambda v: v is not None).map(Const), numeric_exprs
)

compares: st.SearchStrategy[Expr] = st.builds(
    Compare, st.sampled_from(COMPARE_OPS), value_exprs, value_exprs
)

# One type group per In, as the node requires.
_in_values = st.one_of(
    st.lists(numbers, min_size=1, max_size=4),
    st.lists(st.text(max_size=4), min_size=1, max_size=4),
    st.lists(st.dates(), min_size=1, max_size=4),
)

predicates: st.SearchStrategy[Expr] = st.one_of(
    compares,
    st.builds(In, value_exprs, _in_values),
    st.builds(
        Between,
        value_exprs,
        numeric_exprs,
        numeric_exprs,
        st.sampled_from(["both", "left", "right", "none"]),
    ),
    st.builds(IsNull, value_exprs),
    st.builds(Matches, value_exprs, st.sampled_from([r"^a", r"\d+", "(?i)x"])),
    st.builds(StringTest, st.sampled_from(STRING_OPS), value_exprs, st.text(max_size=4)),
)


def _extend(children: st.SearchStrategy[Expr]) -> st.SearchStrategy[Expr]:
    operands = st.lists(children, min_size=2, max_size=4)
    return st.one_of(
        operands.map(lambda xs: And(*xs)),
        operands.map(lambda xs: Or(*xs)),
        st.builds(Not, children),
    )


conditions: st.SearchStrategy[Expr] = st.recursive(
    st.one_of(predicates, fields, st.booleans().map(Const)), _extend, max_leaves=12
)
