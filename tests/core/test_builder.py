"""Tasks 2.2-2.3: the F proxy, operator overloading, Python pitfalls and readable repr."""

import copy
import datetime
import pickle
from collections.abc import Callable
from typing import Any

import pytest
from hypothesis import given

from rulepy import F
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
from rulepy.errors import ExpressionError
from tests.strategies import conditions, value_exprs

# --- F: building fields ----------------------------------------------------------------


@pytest.mark.parametrize(
    ("built", "path"),
    [
        (F.total, ("total",)),
        (F.customer.age, ("customer", "age")),
        (F["key with spaces"], ("key with spaces",)),
        (F.orders[0].total, ("orders", 0, "total")),
        (F["orders", 0, "total"], ("orders", 0, "total")),
        (F.a["b", 1].c, ("a", "b", 1, "c")),
        (F.path, ("path",)),  # the root has no reserved names
    ],
    ids=["attr", "nested", "item", "index", "item-tuple", "mixed", "root-path"],
)
def test_f_builds_field_paths(built: Field, path: tuple[str | int, ...]) -> None:
    assert isinstance(built, Field)
    assert built.path == path


def test_underscore_names_are_not_fields() -> None:
    with pytest.raises(AttributeError):
        _ = F._private
    with pytest.raises(AttributeError):
        _ = F.customer._id
    assert F["_id"].path == ("_id",)


def test_reserved_names_need_item_access() -> None:
    # `kind` is a node property; a data field with that name is reached with [].
    assert F.item.kind == "any"
    assert F.item["kind"].path == ("item", "kind")


def test_root_repr() -> None:
    assert repr(F) == "F"


def test_fields_copy_and_pickle_cleanly() -> None:
    field = F.customer.age
    for clone in (copy.copy(field), copy.deepcopy(field), pickle.loads(pickle.dumps(field))):
        assert clone.same_as(field)


# --- Operators -------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("built", "op"),
    [
        (F.x == 1, "eq"),
        (F.x != 1, "ne"),
        (F.x < 1, "lt"),
        (F.x <= 1, "le"),
        (F.x > 1, "gt"),
        (F.x >= 1, "ge"),
    ],
)
def test_comparison_operators_build_compare(built: Compare, op: str) -> None:
    assert built.same_as(Compare(op, Field("x"), Const(1)))  # pyright: ignore[reportArgumentType]


def test_reflected_comparison_swaps_sides() -> None:
    # Python turns `5 < F.x` into `F.x.__gt__(5)`: the Yoda form is the point here.
    assert (5 < F.x).same_as(Compare("gt", Field("x"), Const(5)))  # noqa: SIM300


def test_equality_with_none_points_to_is_null() -> None:
    with pytest.raises(ExpressionError, match="None") as info:
        _ = F.x == None  # noqa: E711
    assert "is_null" in (info.value.hint or "")


def test_comparing_two_conditions_is_rejected() -> None:
    with pytest.raises(ExpressionError, match="condition"):
        _ = (F.a > 1) == (F.b > 2)


def test_logical_operators_build_flat_connectives() -> None:
    a, b, c = F.a > 1, F.b > 2, F.c > 3
    assert (a & b & c).same_as(And(a, b, c))
    assert (a | b | c).same_as(Or(a, b, c))
    assert (~a).same_as(Not(a))
    assert ((a & b) | c).same_as(Or(And(a, b), c))


def _and(a: Expr) -> Expr:
    return a & True


def _rand(a: Expr) -> Expr:
    return True & a


def _or(a: Expr) -> Expr:
    return a | False


def _ror(a: Expr) -> Expr:
    return False | a


@pytest.mark.parametrize("build", [_and, _rand, _or, _ror], ids=["and", "rand", "or", "ror"])
def test_connectives_reject_python_bools(build: Callable[[Expr], Expr]) -> None:
    with pytest.raises(ExpressionError, match="Python bool") as info:
        build(F.a > 1)
    assert "Const(True)" in (info.value.hint or "")


def test_reserved_attribute_compared_in_python_is_caught() -> None:
    # F.product.tag is the node's tag, so `== "sale"` is a plain Python bool.
    with pytest.raises(ExpressionError, match="Python bool") as info:
        _ = (F.p > 5) & (F.product.tag == "sale")
    assert '["kind"]' in (info.value.hint or "")


def test_explicit_constants_combine_in_order() -> None:
    a = F.a > 1
    assert (Const(True) & a).same_as(And(Const(True), a))
    assert (a | Const(False)).same_as(Or(a, Const(False)))


@pytest.mark.parametrize(
    ("built", "expected"),
    [
        (F.a + 1, Arith("add", Field("a"), Const(1))),
        (1 + F.a, Arith("add", Const(1), Field("a"))),
        (F.a - F.b, Arith("sub", Field("a"), Field("b"))),
        (10 - F.a, Arith("sub", Const(10), Field("a"))),
        (F.a * 2, Arith("mul", Field("a"), Const(2))),
        (2 * F.a, Arith("mul", Const(2), Field("a"))),
        (F.a / 2, Arith("div", Field("a"), Const(2))),
        (1 / F.a, Arith("div", Const(1), Field("a"))),
        (-F.a, Neg(Field("a"))),
        (abs(F.a), Abs(Field("a"))),
    ],
    ids=["add", "radd", "sub", "rsub", "mul", "rmul", "div", "rdiv", "neg", "abs"],
)
def test_arithmetic_operators(built: Expr, expected: Expr) -> None:
    assert built.same_as(expected)


@pytest.mark.parametrize(
    ("built", "expected"),
    [
        (F.c.is_in({"ES", "PT"}), In(Field("c"), ["ES", "PT"])),
        (F.x.between(1, F.hi), Between(Field("x"), Const(1), Field("hi"))),
        (F.x.between(1, 2, closed="left"), Between(Field("x"), Const(1), Const(2), "left")),
        (F.x.is_null(), IsNull(Field("x"))),
        (F.email.matches(r".+@.+"), Matches(Field("email"), r".+@.+")),
        (F.s.starts_with("ab"), StringTest("starts_with", Field("s"), "ab")),
        (F.s.ends_with("ab"), StringTest("ends_with", Field("s"), "ab")),
        (F.s.contains("ab"), StringTest("contains", Field("s"), "ab")),
        (F.x.abs(), Abs(Field("x"))),
    ],
    ids=[
        "is_in",
        "between",
        "between-closed",
        "is_null",
        "matches",
        "starts",
        "ends",
        "contains",
        "abs",
    ],
)
def test_methods_build_nodes(built: Expr, expected: Expr) -> None:
    assert built.same_as(expected)


def test_raw_values_that_are_not_scalars_are_rejected() -> None:
    with pytest.raises(ExpressionError, match="not a valid constant"):
        _ = F.x == [1, 2]


# --- Pitfalls: nodes are not Python booleans -------------------------------------------


def test_nodes_cannot_be_used_as_python_bools() -> None:
    with pytest.raises(ExpressionError, match="&") as info:
        bool(F.x > 1)
    assert isinstance(info.value, TypeError)


def test_and_or_keywords_are_rejected() -> None:
    with pytest.raises(ExpressionError):
        _ = (F.a > 1) and (F.b > 2)


def test_chained_comparison_is_rejected_with_between_hint() -> None:
    with pytest.raises(ExpressionError) as info:
        _ = 0 < F.x < 10  # pyright: ignore[reportOperatorIssue]
    assert "between" in (info.value.hint or "")


def test_nodes_are_hashable_by_identity() -> None:
    a, b = F.x > 1, F.x > 1
    assert len({a, b}) == 2
    assert {a: 1}[a] == 1


def test_list_membership_only_works_when_no_other_node_comes_first() -> None:
    a = F.x > 1
    assert a in [a]
    with pytest.raises(ExpressionError):
        _ = a in [F.x > 1, a]  # documented pitfall: use same_as


def _unpack(node: Any) -> object:
    return [*node]


def _next(node: Any) -> object:
    return next(iter(node))


@pytest.mark.parametrize(
    "use",
    [list, tuple, set, sorted, _unpack, _next],
    ids=["list", "tuple", "set", "sorted", "unpack", "iter"],
)
def test_nodes_are_not_iterable(use: Callable[[Any], object]) -> None:
    for node in (F.orders, F, F.x > 1):
        with pytest.raises(ExpressionError, match="not iterable"):
            use(node)


def test_in_operator_on_a_node_points_to_is_in() -> None:
    with pytest.raises(ExpressionError, match="in") as info:
        _ = "vip" in F.tags  # pyright: ignore[reportOperatorIssue]
    assert "is_in" in (info.value.hint or "")


def test_in_operator_on_f_points_to_is_in() -> None:
    with pytest.raises(ExpressionError, match="membership") as info:
        _ = "x" in F  # pyright: ignore[reportOperatorIssue]
    assert "is_in" in (info.value.hint or "")


def test_python_in_against_values_points_to_is_in() -> None:
    with pytest.raises(ExpressionError) as info:
        _ = F.x in [1, 2]
    assert "is_in" in (info.value.hint or "")


@pytest.mark.parametrize(
    ("typo", "suggestion"),
    [
        (lambda: F.x.startswith("a"), "starts_with"),  # pyright: ignore[reportCallIssue]
        (lambda: F.x.isin([1]), "is_in"),  # pyright: ignore[reportCallIssue]
        (lambda: F.x.isnull(), "is_null"),  # pyright: ignore[reportCallIssue]
        (lambda: F.x.is_between(1, 2), "between"),  # pyright: ignore[reportCallIssue]
    ],
    ids=["startswith", "isin", "isnull", "is_between"],
)
def test_calling_a_field_suggests_the_method(typo: Callable[[], object], suggestion: str) -> None:
    with pytest.raises(ExpressionError, match="not a method") as info:
        typo()
    assert suggestion in (info.value.hint or "")


def test_calling_a_field_without_a_close_method_still_explains() -> None:
    with pytest.raises(ExpressionError, match="not a method"):
        F.customer.frobnicate()  # pyright: ignore[reportCallIssue]


@pytest.mark.parametrize("name", ["str", "dt", "list", "arr", "struct", "cat"])
def test_polars_style_namespaces_are_rejected(name: str) -> None:
    with pytest.raises(AttributeError, match=name):
        getattr(F.x, name)
    assert F.x[name].path == ("x", name)  # an actual field with that name


def test_reserved_names_raise_attribute_error_when_unset() -> None:
    # A Field whose __init__ failed must still repr without recursing.
    broken = Field.__new__(Field)
    with pytest.raises(AttributeError):
        _ = broken.path


# --- numpy / pandas on the left -------------------------------------------------------


def test_numpy_scalars_on_the_left_reflect() -> None:
    np = pytest.importorskip("numpy")
    assert (np.int64(5) < F.x).same_as(F.x > 5)


def test_numpy_arrays_on_the_left_are_rejected_clearly() -> None:
    np = pytest.importorskip("numpy")
    with pytest.raises(ExpressionError, match="not a valid constant"):
        _ = np.array([1, 2]) == F.x


def test_pandas_series_on_the_left_are_rejected_clearly() -> None:
    pd = pytest.importorskip("pandas")
    with pytest.raises(ExpressionError, match="not a valid constant"):
        _ = pd.Series([1, 2]) == F.x


# --- repr ------------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("node", "text"),
    [
        (F.customer.age, "F.customer.age"),
        (F["key with spaces"], "F['key with spaces']"),
        (F.orders[0]["kind"], "F.orders[0]['kind']"),
        (F["class"], "F['class']"),
        (F.x > 5, "F.x > 5"),
        (Compare("lt", Const(5), Field("x")), "Const(5) < F.x"),
        ((F.a > 1) & (F.b < 2), "(F.a > 1) & (F.b < 2)"),
        (((F.a > 1) | (F.b < 2)) & ~F.vip, "((F.a > 1) | (F.b < 2)) & ~F.vip"),
        (And(Const(True), F.vip), "Const(True) & F.vip"),
        (And(F.vip, Const(False)), "F.vip & Const(False)"),
        (F.x["str"], "F.x['str']"),
        ((F.a + F.b) * 2 > 10, "(F.a + F.b) * 2 > 10"),
        (-(F.a - 1), "-(F.a - 1)"),
        (F.c.is_in(["PT", "ES"]), "F.c.is_in(['ES', 'PT'])"),
        (F.x.between(1, 10, closed="none"), "F.x.between(1, 10, closed='none')"),
        (F.x.between(0, float("inf")), "F.x.between(0, float('inf'))"),
        ((F.a + 1).is_null(), "(F.a + 1).is_null()"),
        (F.s.matches("^a"), "F.s.matches('^a')"),
        (F.d >= datetime.date(2026, 1, 1), "F.d >= datetime.date(2026, 1, 1)"),
    ],
)
def test_repr_is_readable_python(node: Expr, text: str) -> None:
    assert repr(node) == text
    assert str(node) == text


ENV = {"F": F, "Const": Const, "datetime": datetime}


@given(conditions)
def test_repr_of_conditions_round_trips(tree: Expr) -> None:
    assert eval(repr(tree), ENV).same_as(tree)


@given(value_exprs)
def test_repr_of_values_round_trips(tree: Expr) -> None:
    assert eval(repr(tree), ENV).same_as(tree)
