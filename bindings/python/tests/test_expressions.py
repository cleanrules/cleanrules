"""Conditions built with `F`, constants and operators (task C.2a).

Ported from the Python v1's tests (git tag `python-reference`): the API and the
pitfall guards behave as there, and every construction rule is now checked by
the core.
"""

import copy
import datetime as dt
import enum
import pickle
import zoneinfo
from collections.abc import Callable
from decimal import Decimal
from typing import Any

import pytest
from cleanrules import Const, Expr, ExpressionError, F, Field
from hypothesis import given

from tests.strategies import conditions, decimals, value_exprs

I64_MAX, I64_MIN = 2**63 - 1, -(2**63)

# --- F and fields -------------------------------------------------------------------


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
        (Field("orders", 0, "total"), ("orders", 0, "total")),
    ],
)
def test_fields_have_paths(built: Field, path: tuple[str | int, ...]) -> None:
    assert isinstance(built, Field)
    assert built.path == path
    assert built.tag == "field"
    assert built.kind == "any"


@pytest.mark.parametrize("path", [(), ("",), (-1,), (True,), (1.5,), (None,), (("a", "b"),)])
def test_invalid_paths_are_rejected(path: tuple[Any, ...]) -> None:
    with pytest.raises(ExpressionError):
        Field(*path)


def test_underscore_names_are_not_fields() -> None:
    with pytest.raises(AttributeError):
        _ = F._private
    with pytest.raises(AttributeError):
        _ = F.customer._id
    assert F["_id"].path == ("_id",)


def test_reserved_names_need_item_access() -> None:
    assert F.item.kind == "any"  # the node's kind, not a field
    assert F.item["kind"].path == ("item", "kind")


@pytest.mark.parametrize("name", ["str", "dt", "list", "arr", "struct", "cat"])
def test_polars_style_namespaces_are_rejected(name: str) -> None:
    with pytest.raises(AttributeError, match=name):
        getattr(F.x, name)
    assert F.x[name].path == ("x", name)


@pytest.mark.parametrize(
    ("call", "suggestion"),
    [
        (lambda: F.x.startswith("a"), "starts_with"),
        (lambda: F.x.isin([1]), "is_in"),
        (lambda: F.x.isnull(), "is_null"),
        (lambda: F.x.is_between(1, 2), "between"),
    ],
)
def test_calling_a_field_suggests_the_method(call: Callable[[], object], suggestion: str) -> None:
    with pytest.raises(ExpressionError, match="not a method") as info:
        call()
    assert suggestion in (info.value.hint or "")


# --- Constants ------------------------------------------------------------------------


@pytest.mark.parametrize(
    "value",
    [
        None,
        True,
        0,
        -3,
        I64_MAX,
        I64_MIN,
        2.5,
        float("inf"),
        "",
        "text",
        dt.date(2026, 1, 1),
        dt.datetime(2026, 1, 1, 9, 30, 0, 250),
        Decimal("0.10"),
        Decimal("-Infinity"),
    ],
)
def test_constants_keep_builtin_scalars(value: object) -> None:
    node = Const(value)
    assert node.value == value
    assert type(node.value) is type(value)
    assert Const(node.value).same_as(node)


class Color(enum.StrEnum):
    RED = "red"


class Size(enum.IntEnum):
    LARGE = 3


class MyDate(dt.date):
    pass


class Money(Decimal):
    pass


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Color.RED, "red"),
        (Size.LARGE, 3),
        (MyDate(2026, 1, 2), dt.date(2026, 1, 2)),
        (Money("1.5"), Decimal("1.5")),
    ],
)
def test_subclasses_become_exact_types(value: object, expected: object) -> None:
    node = Const(value)
    assert type(node.value) is type(expected)
    assert node.same_as(Const(expected))


def test_numpy_bools_are_booleans_without_importing_numpy() -> None:
    def truthy(_: object) -> bool:
        return True

    fake_bool = type("bool_", (), {"__module__": "numpy", "__bool__": truthy})
    assert Const(fake_bool()).value is True


def test_aware_datetimes_keep_their_zone_when_the_core_knows_it() -> None:
    madrid = zoneinfo.ZoneInfo("Europe/Madrid")
    value = Const(dt.datetime(2026, 11, 1, 10, tzinfo=madrid)).value
    assert isinstance(value, dt.datetime)
    assert value == dt.datetime(2026, 11, 1, 9, tzinfo=dt.UTC)
    assert value.tzinfo == madrid


def test_utc_and_fixed_offsets_stay_fixed() -> None:
    assert Const(dt.datetime(2026, 1, 1, tzinfo=dt.UTC)).value.tzinfo is dt.UTC  # type: ignore[union-attr]
    plus_two = dt.timezone(dt.timedelta(hours=2))
    value = Const(dt.datetime(2026, 1, 1, tzinfo=plus_two)).value
    assert isinstance(value, dt.datetime)
    assert value.utcoffset() == dt.timedelta(hours=2)


def test_sub_second_offsets_and_nanoseconds_are_rejected() -> None:
    odd = dt.timezone(dt.timedelta(seconds=1, microseconds=5))
    with pytest.raises(ExpressionError, match="cannot be a constant"):
        Const(dt.datetime(2026, 1, 1, tzinfo=odd))

    class NanoDatetime(dt.datetime):  # stand-in for pandas.Timestamp
        nanosecond = 5

    with pytest.raises(ExpressionError, match="finer than a microsecond"):
        Const(NanoDatetime(2026, 1, 1))


@pytest.mark.parametrize("value", [[1], {"a": 1}, (1, 2), object(), b"bytes", 1j, dt.time(1)])
def test_non_scalars_are_rejected(value: object) -> None:
    with pytest.raises(ExpressionError, match="cannot be a constant"):
        Const(value)


@pytest.mark.parametrize("value", [float("nan"), Decimal("NaN"), Decimal("sNaN")])
def test_nan_is_rejected_with_a_hint(value: object) -> None:
    with pytest.raises(ExpressionError, match="NaN") as info:
        Const(value)
    assert "is_null" in (info.value.hint or "")


@pytest.mark.parametrize("value", [I64_MAX + 1, I64_MIN - 1, 10**30])
def test_integers_beyond_64_bits_are_rejected(value: int) -> None:
    with pytest.raises(ExpressionError, match="64 bits"):
        Const(value)


def test_decimals_beyond_38_digits_are_rejected_never_rounded() -> None:
    assert Const(Decimal("1E+37")).value == Decimal("1E+37")
    assert Const(Decimal("1." + "0" * 40)).value == 1
    with pytest.raises(ExpressionError, match="38 digits"):
        Const(Decimal("1E+38"))
    with pytest.raises(ExpressionError, match="38 digits"):
        Const(Decimal("1." + "1" * 40))
    with pytest.raises(ExpressionError, match="38 digits"):
        Const(Decimal("1E-39"))  # scale 39


@given(decimals)
def test_decimals_are_exact(value: Decimal) -> None:
    assert Const(value).value == value


@pytest.mark.parametrize(
    ("value", "kind"),
    [(True, "any"), (None, "value"), (3, "value"), ("x", "value"), (dt.date(2026, 1, 1), "value")],
)
def test_constant_kind_depends_on_the_value(value: object, kind: str) -> None:
    # `any` is for fields and booleans only; the v1 also gave it to None (spec/ast.md).
    assert Const(value).kind == kind


def test_constant_identity_includes_the_type() -> None:
    assert not Const(1).same_as(Const(1.0))
    assert not Const(Decimal("1")).same_as(Const(1))
    assert not Const(True).same_as(Const(1))


# --- Operators ------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("built", "tag"),
    [(F.x == 1, "eq"), (F.x != 1, "ne"), (F.x < 1, "lt"), (F.x <= 1, "le"),
     (F.x > 1, "gt"), (F.x >= 1, "ge")],
)  # fmt: skip
def test_comparisons(built: Expr, tag: str) -> None:
    assert built.tag == tag
    assert built.kind == "condition"
    assert [c.tag for c in built.children()] == ["field", "const"]


def test_reflected_comparisons_swap_sides() -> None:
    # Python turns `5 < F.x` into `F.x.__gt__(5)`.
    assert (5 < F.x).same_as(F.x > 5)  # noqa: SIM300


def test_comparisons_keep_their_orientation_between_nodes() -> None:
    # A field on the right must not take over through Python's reflected operators.
    assert repr((F.x + 2) * 3 > F.y) == "(F.x + 2) * 3 > F.y"
    assert repr(abs(F.a) <= F.b) == "abs(F.a) <= F.b"


def test_equality_with_none_points_to_is_null() -> None:
    with pytest.raises(ExpressionError, match="None") as info:
        _ = F.x == None  # noqa: E711
    assert "is_null" in (info.value.hint or "")


def test_conditions_cannot_be_compared() -> None:
    with pytest.raises(ExpressionError, match="condition"):
        _ = (F.a > 1) == (F.b > 2)


def test_connectives_are_flat_and_keep_order() -> None:
    a, b, c = F.a > 1, F.b > 2, F.c > 3
    assert repr(a & b & c) == "(F.a > 1) & (F.b > 2) & (F.c > 3)"
    assert len((a & b & c).children()) == 3
    assert len((a | b | c).children()) == 3
    assert repr((a & b) | c) == "((F.a > 1) & (F.b > 2)) | (F.c > 3)"
    assert (~a).tag == "not"


def _and(a: Expr) -> Expr:
    return a & True


def _rand(a: Expr) -> Expr:
    return True & a


def _or(a: Expr) -> Expr:
    return a | False


def _ror(a: Expr) -> Expr:
    return False | a


@pytest.mark.parametrize("build", [_and, _rand, _or, _ror])
def test_connectives_reject_python_bools(build: Callable[[Expr], Expr]) -> None:
    with pytest.raises(ExpressionError, match="Python bool") as info:
        build(F.a > 1)
    assert "Const(True)" in (info.value.hint or "")


def test_a_reserved_attribute_compared_in_python_is_caught() -> None:
    # F.product.tag is the node's tag, so `== "sale"` is a Python bool.
    with pytest.raises(ExpressionError, match="Python bool") as info:
        _ = (F.p > 5) & (F.product.tag == "sale")
    assert '["kind"]' in (info.value.hint or "")


def test_constants_combine_explicitly() -> None:
    assert repr(Const(True) & (F.a > 1)) == "Const(True) & (F.a > 1)"
    assert repr(F.vip | Const(False)) == "F.vip | Const(False)"


def test_connectives_reject_values() -> None:
    with pytest.raises(ExpressionError):
        _ = (F.a > 1) & (F.b + 1)
    with pytest.raises(ExpressionError):
        _ = ~(F.b * 2)


@pytest.mark.parametrize(
    ("built", "text"),
    [
        (F.a + 1, "F.a + 1"),
        (1 + F.a, "Const(1) + F.a"),
        (F.a - F.b, "F.a - F.b"),
        (10 - F.a, "Const(10) - F.a"),
        (F.a * 2, "F.a * 2"),
        (2 * F.a, "Const(2) * F.a"),
        (F.a / 2, "F.a / 2"),
        (1 / F.a, "Const(1) / F.a"),
        (-F.a, "-F.a"),
        (abs(F.a), "abs(F.a)"),
        (F.a.abs(), "abs(F.a)"),
    ],
)
def test_arithmetic(built: Expr, text: str) -> None:
    assert repr(built) == text
    assert built.kind == "value"


@pytest.mark.parametrize("value", ["text", True, dt.date(2026, 1, 1)])
def test_arithmetic_rejects_non_numeric_constants(value: object) -> None:
    with pytest.raises(ExpressionError):
        _ = F.x + value


# --- Methods --------------------------------------------------------------------------


def test_is_in_sorts_and_deduplicates() -> None:
    assert repr(F.c.is_in(["PT", "ES", "PT"])) == "F.c.is_in(['ES', 'PT'])"
    assert F.c.is_in([3, 1, 2]).same_as(F.c.is_in([1, 2, 3, 3]))


@pytest.mark.parametrize(
    ("values", "match"),
    [([], "at least one"), ([1, None], "null"), ([1, "a"], "mix"), ([float("nan")], "NaN")],
)
def test_is_in_rejects_invalid_values(values: list[object], match: str) -> None:
    with pytest.raises(ExpressionError, match=match):
        F.x.is_in(values)


@pytest.mark.parametrize("values", ["abc", b"abc", {"a": 1}, 5])
def test_is_in_needs_a_collection(values: Any) -> None:
    with pytest.raises(ExpressionError, match="collection"):
        F.x.is_in(values)


def test_between() -> None:
    assert repr(F.x.between(1, F.hi)) == "F.x.between(1, F.hi)"
    assert repr(F.x.between(1, 10, closed="none")) == "F.x.between(1, 10, closed='none')"
    with pytest.raises(ExpressionError, match="closed"):
        F.x.between(1, 2, closed="open")  # type: ignore[arg-type]
    with pytest.raises(ExpressionError):
        F.x.between(1, "z")


def test_string_tests_and_patterns() -> None:
    assert repr(F.s.starts_with("ab")) == "F.s.starts_with('ab')"
    assert repr(F.s.ends_with("ab")) == "F.s.ends_with('ab')"
    assert repr(F.s.contains("ab")) == "F.s.contains('ab')"
    assert repr(F.email.matches(r".+@.+")) == "F.email.matches('.+@.+')"
    with pytest.raises(ExpressionError, match="takes a str"):
        F.s.starts_with(F.t)  # type: ignore[arg-type]
    with pytest.raises(ExpressionError, match="pattern") as info:
        F.s.matches("(")
    assert "regex" in (info.value.hint or "")
    with pytest.raises(ExpressionError):
        F.s.matches("(?=lookahead)")


def test_is_null_takes_values_and_null() -> None:
    assert F.x.is_null().kind == "condition"
    assert repr((F.a + 1).is_null()) == "(F.a + 1).is_null()"
    assert Const(None).is_null().tag == "is_null"


# --- Pitfalls: nodes are not Python values --------------------------------------------


def test_nodes_are_not_python_bools() -> None:
    with pytest.raises(ExpressionError, match="&") as info:
        bool(F.x > 1)
    assert isinstance(info.value, TypeError)
    with pytest.raises(ExpressionError):
        _ = (F.a > 1) and (F.b > 2)


def test_chained_comparisons_point_to_between() -> None:
    with pytest.raises(ExpressionError) as info:
        _ = 0 < F.x < 10  # type: ignore[operator]
    assert "between" in (info.value.hint or "")


def _unpack(node: Any) -> object:
    return [*node]


def _next(node: Any) -> object:
    return next(iter(node))


USES: list[Callable[[Any], object]] = [list, tuple, set, sorted, _unpack, _next]


@pytest.mark.parametrize("use", USES)
def test_nodes_are_not_iterable(use: Callable[[Any], object]) -> None:
    for node in (F.orders, F, F.x > 1):
        with pytest.raises(ExpressionError, match="not iterable"):
            use(node)


def test_in_points_to_is_in() -> None:
    for build in (lambda: "vip" in F.tags, lambda: "x" in F, lambda: F.x in [1, 2]):
        with pytest.raises(ExpressionError) as info:
            build()  # type: ignore[operator]
        assert "is_in" in (info.value.hint or "")


def test_nodes_hash_by_identity() -> None:
    a, b = F.x > 1, F.x > 1
    assert len({a, b}) == 2
    assert {a: 1}[a] == 1
    assert a in [a]
    with pytest.raises(ExpressionError):
        _ = a in [F.x > 1, a]  # documented: use same_as


def test_structural_keys_index_by_structure() -> None:
    table = {(F.x > 1).structural_key(): "rule"}
    assert table[(F.x > 1).structural_key()] == "rule"
    assert (F.x > 1).structural_key() != (F.x > 2).structural_key()
    # Equal decimals with different scales are the same constant.
    assert Const(Decimal("1.0")).same_as(Const(Decimal("1.00")))
    assert Const(Decimal("1.0")).structural_key() == Const(Decimal("1.00")).structural_key()


def test_nodes_are_immutable() -> None:
    node = F.x > 1
    with pytest.raises(AttributeError):
        node._node = None  # type: ignore[misc]


def test_numpy_scalars_on_the_left_reflect() -> None:
    np = pytest.importorskip("numpy")
    assert (np.int64(5) < F.x).same_as(F.x > 5)


def test_numpy_arrays_on_the_left_are_rejected() -> None:
    np = pytest.importorskip("numpy")
    with pytest.raises(ExpressionError, match="cannot be a constant"):
        _ = np.array([1, 2]) == F.x


# --- Introspection --------------------------------------------------------------------


def test_children_and_walk() -> None:
    tree = (F.a > 1) & ~F.b
    assert [n.tag for n in tree.walk()] == ["and", "gt", "field", "const", "not", "field"]
    assert F.a.children() == ()


def test_field_paths_in_first_appearance_order() -> None:
    tree = (F.b > 1) & F.a.between(F.lo, F.hi + 1) & (F.b < F.orders[0].total)
    assert tree.field_paths() == (("b",), ("a",), ("lo",), ("hi",), ("orders", 0, "total"))
    assert Const(1).field_paths() == ()


@pytest.mark.parametrize(
    ("node", "text"),
    [
        (F.customer.age, "F.customer.age"),
        (F["key with spaces"], "F['key with spaces']"),
        (F.orders[0]["kind"], "F.orders[0]['kind']"),
        (F["class"], "F['class']"),
        (F.x > 5, "F.x > 5"),
        (((F.a > 1) | (F.b < 2)) & ~F.vip, "((F.a > 1) | (F.b < 2)) & ~F.vip"),
        (F.x["str"], "F.x['str']"),
        ((F.a + F.b) * 2 > 10, "(F.a + F.b) * 2 > 10"),
        (-(F.a - 1), "-(F.a - 1)"),
        (F.x.between(0, float("inf")), "F.x.between(0, float('inf'))"),
        (F.d >= dt.date(2026, 1, 1), "F.d >= datetime.date(2026, 1, 1)"),
        (F.p == Decimal("1.50"), "F.p == Decimal('1.50')"),
    ],
)
def test_repr_is_python(node: Expr, text: str) -> None:
    assert repr(node) == text


@pytest.mark.parametrize(
    ("node", "text"),
    [
        (F.customer.age, "customer.age"),
        (F["key with spaces"], "`key with spaces`"),
        (F.orders[0]["kind"], "orders[0].kind"),
        (F["class"].date, "class.date"),
        (F["and"], "`and`"),
        (F.x > 5, "x > 5"),
        (((F.a > 1) | (F.b < 2)) & ~F.vip, "(a > 1 OR b < 2) AND NOT vip"),
        ((F.a + F.b) * 2 > 10, "(a + b) * 2 > 10"),
        (-(F.a - 1), "-(a - 1)"),
        (F.x.between(0, float("inf")), 'x BETWEEN 0 AND FLOAT "inf"'),
        (F.d >= dt.date(2026, 1, 1), 'd >= DATE "2026-01-01"'),
        (F.p == Decimal("1.50"), "p = 1.50"),
        (F.r < 0.035, "r < 0.035e0"),
        (F.n.is_in(["b", "a"]), 'n IN ("a", "b")'),
    ],
)
def test_str_is_the_canonical_text(node: Expr, text: str) -> None:
    assert str(node) == text


ENV = {"F": F, "Const": Const, "datetime": dt, "Decimal": Decimal}


@given(conditions)
def test_repr_of_conditions_round_trips(tree: Expr) -> None:
    assert eval(repr(tree), ENV).same_as(tree)


@given(value_exprs)
def test_repr_of_values_round_trips(tree: Expr) -> None:
    assert eval(repr(tree), ENV).same_as(tree)


@given(conditions)
def test_copies_and_pickles_are_the_same_tree(tree: Expr) -> None:
    assert copy.copy(tree) is tree
    assert copy.deepcopy(tree) is tree
    clone = pickle.loads(pickle.dumps(tree))
    assert clone.same_as(tree)
    assert type(clone) is type(tree)
    assert clone.structural_key() == tree.structural_key()


@given(conditions)
def test_walk_visits_every_node_once(tree: Expr) -> None:
    nodes = list(tree.walk())
    assert nodes[0] is tree
    assert sum(len(n.children()) for n in nodes) == len(nodes) - 1


@given(conditions)
def test_connectives_never_contain_themselves(tree: Expr) -> None:
    for node in tree.walk():
        if node.tag in ("and", "or"):
            assert all(child.tag != node.tag for child in node.children())


def test_pretty_printers_use_the_repr() -> None:
    from hypothesis.vendor.pretty import pretty

    assert pretty((F.a > 1) & ~F.b) == "(F.a > 1) & ~F.b"
