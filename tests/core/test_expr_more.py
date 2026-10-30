"""Nodes of task 2.1b: In, Between, IsNull, Matches, StringTest and arithmetic."""

import datetime as dt
import enum

import pytest
from hypothesis import given
from hypothesis import strategies as st

from rulepy.core.expr import (
    Abs,
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
    StringTest,
)
from rulepy.errors import ExpressionError
from tests.strategies import conditions, value_exprs

x = Field("x")


class Code(enum.StrEnum):
    A = "a"


cond = Compare("gt", x, Const(1))


def test_value_taking_nodes_reject_conditions() -> None:
    builders = [
        lambda: In(cond, [1]),
        lambda: Between(cond, Const(1), Const(2)),
        lambda: Between(x, cond, Const(2)),
        lambda: IsNull(cond),
        lambda: Matches(cond, "a"),
        lambda: StringTest("contains", cond, "a"),
        lambda: Arith("add", cond, Const(1)),
        lambda: Abs(cond),
        lambda: Neg(cond),
    ]
    for build in builders:
        with pytest.raises(ExpressionError, match="condition"):
            build()


def test_value_taking_nodes_reject_none_constants_except_is_null() -> None:
    for build in (
        lambda: Between(x, Const(None), Const(2)),
        lambda: Arith("add", x, Const(None)),
        lambda: Neg(Const(None)),
        lambda: Matches(Const(None), "a"),
    ):
        with pytest.raises(ExpressionError, match="None"):
            build()
    assert IsNull(Const(None)).tag == "is_null"


# --- In --------------------------------------------------------------------------------


def test_in_stores_values_sorted_and_deduplicated() -> None:
    assert In(x, ["b", "a", "b"]).values == ("a", "b")


def test_in_accepts_sets_deterministically() -> None:
    assert In(x, {"pear", "apple", "fig"}).values == ("apple", "fig", "pear")


def test_in_value_order_does_not_change_identity() -> None:
    assert In(x, [2, 1]).same_as(In(x, (1, 2)))


def test_in_normalizes_values_like_const() -> None:
    assert In(x, [True]).values == (True,)
    assert type(In(x, [dt.datetime(2026, 1, 1)]).values[0]) is dt.datetime


@pytest.mark.parametrize(
    ("values", "match"),
    [
        ([], "at least one"),
        ([1, None], "None"),
        ([1, "1"], "type group"),
        ([True, 1], "type group"),
        ([dt.date(2026, 1, 1), dt.datetime(2026, 1, 1)], "type group"),
        ([dt.datetime(2026, 1, 1), dt.datetime(2026, 1, 1, tzinfo=dt.UTC)], "type group"),
        ([float("nan")], "NaN"),
        ([[1]], "'in' values: list"),
    ],
    ids=["empty", "none", "num-str", "bool-int", "date-datetime", "naive-aware", "nan", "list"],
)
def test_in_rejects_invalid_values(values: list[object], match: str) -> None:
    with pytest.raises(ExpressionError, match=match):
        In(x, values)  # pyright: ignore[reportArgumentType]


@pytest.mark.parametrize(
    "values", ["abc", b"abc", {"a": 1}, 5], ids=["str", "bytes", "dict", "int"]
)
def test_in_rejects_values_that_are_not_a_collection(values: object) -> None:
    with pytest.raises(ExpressionError, match="collection") as info:
        In(x, values)  # pyright: ignore[reportArgumentType]
    assert "In(x, [" in (info.value.hint or "")


def test_in_keeps_equal_int_and_float_deterministically() -> None:
    assert In(x, [1, 1.0]).same_as(In(x, [1.0, 1]))


def test_in_without_values_has_a_hint() -> None:
    with pytest.raises(ExpressionError, match="at least one") as info:
        In(x, [])
    assert info.value.hint is not None


def test_in_has_no_child_nodes_besides_operand() -> None:
    node = In(x, [1, 2])
    assert node.children()[0] is x
    assert len(node.children()) == 1


# --- Between ---------------------------------------------------------------------------


def test_between_defaults_to_closed_both() -> None:
    node = Between(x, Const(1), Field("hi"))
    assert (node.tag, node.kind, node.closed) == ("between", "condition", "both")
    assert [c.tag for c in node.children()] == ["field", "const", "field"]


@pytest.mark.parametrize("closed", ["both", "left", "right", "none"])
def test_between_accepts_polars_closed_values(closed: str) -> None:
    assert Between(x, Const(1), Const(2), closed).closed == closed  # pyright: ignore[reportArgumentType]


@pytest.mark.parametrize(
    ("low", "high"),
    [(1, "a"), (True, 2), (dt.date(2026, 1, 1), 5)],
    ids=["num-str", "bool", "date-num"],
)
def test_between_rejects_incompatible_constant_bounds(low: object, high: object) -> None:
    with pytest.raises(ExpressionError, match="bounds"):
        Between(x, Const(low), Const(high))  # pyright: ignore[reportArgumentType]


def test_between_rejects_unknown_closed() -> None:
    with pytest.raises(ExpressionError, match="'neither'"):
        Between(x, Const(1), Const(2), "neither")  # pyright: ignore[reportArgumentType]


# --- IsNull / Matches / StringTest -----------------------------------------------------


def test_is_null_tag_and_kind() -> None:
    assert (IsNull(x).tag, IsNull(x).kind) == ("is_null", "condition")


def test_matches_keeps_the_pattern() -> None:
    node = Matches(x, r"^\d{3}$")
    assert (node.tag, node.pattern) == ("matches", r"^\d{3}$")


def test_matches_rejects_invalid_regex_with_position() -> None:
    with pytest.raises(ExpressionError, match=r"invalid regular expression .*position 3"):
        Matches(x, "abc(")


def test_matches_invalid_regex_has_a_hint() -> None:
    with pytest.raises(ExpressionError) as info:
        Matches(x, "(")
    assert info.value.hint is not None


def test_string_subclasses_are_normalized_in_patterns_and_text() -> None:
    matches = Matches(x, Code.A)
    test = StringTest("contains", x, Code.A)
    assert type(matches.pattern) is str
    assert type(test.text) is str
    assert matches.same_as(Matches(x, "a"))
    assert test.same_as(StringTest("contains", x, "a"))


def test_matches_rejects_non_string_pattern() -> None:
    with pytest.raises(ExpressionError, match="pattern"):
        Matches(x, 5)  # pyright: ignore[reportArgumentType]


@pytest.mark.parametrize("op", ["starts_with", "ends_with", "contains"])
def test_string_test_tag_is_its_operator(op: str) -> None:
    node = StringTest(op, x, "ab")  # pyright: ignore[reportArgumentType]
    assert (node.tag, node.kind, node.text) == (op, "condition", "ab")


def test_string_test_rejects_unknown_operator() -> None:
    with pytest.raises(ExpressionError, match="'startswith'"):
        StringTest("startswith", x, "a")  # pyright: ignore[reportArgumentType]


def test_string_test_requires_literal_text() -> None:
    with pytest.raises(ExpressionError, match="text"):
        StringTest("contains", x, Field("y"))  # pyright: ignore[reportArgumentType]


# --- Arithmetic ------------------------------------------------------------------------


@pytest.mark.parametrize("op", ["add", "sub", "mul", "div"])
def test_arith_tag_is_its_operator(op: str) -> None:
    node = Arith(op, x, Const(2))  # pyright: ignore[reportArgumentType]
    assert (node.tag, node.kind) == (op, "value")


def test_arith_rejects_unknown_operator() -> None:
    with pytest.raises(ExpressionError, match="'pow'"):
        Arith("pow", x, Const(2))  # pyright: ignore[reportArgumentType]


@pytest.mark.parametrize("value", ["a", True, dt.date(2026, 1, 1)])
def test_arithmetic_rejects_non_numeric_constants(value: object) -> None:
    with pytest.raises(ExpressionError, match="number"):
        Arith("add", x, Const(value))  # pyright: ignore[reportArgumentType]
    with pytest.raises(ExpressionError, match="number"):
        Neg(Const(value))  # pyright: ignore[reportArgumentType]


def test_unary_arithmetic_tags_and_kinds() -> None:
    assert [(n.tag, n.kind) for n in (Abs(x), Neg(Const(2.5)))] == [
        ("abs", "value"),
        ("neg", "value"),
    ]


def test_arithmetic_can_be_compared() -> None:
    total = Arith("mul", Field("price"), Field("qty"))
    assert Compare("gt", Abs(total), Const(100)).tag == "gt"


# --- Properties ------------------------------------------------------------------------


@given(value_exprs)
def test_value_expressions_are_never_conditions(node: Expr) -> None:
    assert node.kind != "condition"


@given(conditions)
def test_generated_conditions_use_new_nodes_without_errors(tree: Expr) -> None:
    assert all(n.tag for n in tree.walk())


@given(st.lists(st.integers(-5, 5) | st.integers(-5, 5).map(float), min_size=1))
def test_in_is_order_and_duplicate_insensitive(values: list[int | float]) -> None:
    assert In(x, values).same_as(In(x, list(reversed(values)) + values))
