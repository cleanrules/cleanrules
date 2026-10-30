import dataclasses
import datetime as dt
import enum
import pickle
import re
import weakref
from collections.abc import Sequence
from pathlib import Path
from typing import get_args

import pytest
from hypothesis import given

from rulepy.core.expr import And, Compare, Const, Expr, Field, Node, Not, Or, Tag
from rulepy.errors import ExpressionError
from tests.strategies import I64_MAX, I64_MIN, conditions, scalars

a = Compare("gt", Field("a"), Const(1))
b = Compare("lt", Field("b"), Const(2))
c = Compare("eq", Field("c"), Const("x"))


def same_nodes(xs: Sequence[Expr], ys: Sequence[Expr]) -> bool:
    """Identity comparison of node sequences (`==` on nodes will build Compare in 2.3)."""
    return len(xs) == len(ys) and all(x is y for x, y in zip(xs, ys, strict=True))


# --- Field -----------------------------------------------------------------------------


def test_field_takes_path_segments_as_arguments() -> None:
    assert Field("orders", 0, "total").path == ("orders", 0, "total")


def test_single_name_field_is_not_split_into_characters() -> None:
    assert Field("total").path == ("total",)


@pytest.mark.parametrize(
    "path",
    [(), ("",), (-1,), (True,), (1.5,), (None,), (("a", "b"),)],
    ids=["empty", "empty-name", "negative", "bool", "float", "none", "tuple"],
)
def test_field_rejects_invalid_paths(path: tuple[object, ...]) -> None:
    with pytest.raises(ExpressionError):
        Field(*path)  # pyright: ignore[reportArgumentType]


def test_field_empty_path_has_a_hint() -> None:
    with pytest.raises(ExpressionError) as info:
        Field()
    assert info.value.hint is not None


# --- Const -----------------------------------------------------------------------------


@pytest.mark.parametrize(
    "value",
    [
        None,
        True,
        0,
        -3,
        2.5,
        float("inf"),
        "",
        "text",
        dt.date(2026, 1, 1),
        dt.datetime(2026, 1, 1),
    ],
)
def test_const_keeps_builtin_scalars(value: object) -> None:
    assert Const(value).value == value  # pyright: ignore[reportArgumentType]
    assert type(Const(value).value) is type(value)  # pyright: ignore[reportArgumentType]


class Color(enum.StrEnum):
    RED = "red"


class Size(enum.IntEnum):
    LARGE = 3


class MyDate(dt.date):
    pass


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (Color.RED, "red"),
        (Size.LARGE, 3),
        (MyDate(2026, 1, 2), dt.date(2026, 1, 2)),
    ],
    ids=["StrEnum", "IntEnum", "date-subclass"],
)
def test_const_normalizes_subclasses_to_exact_types(value: object, expected: object) -> None:
    node = Const(value)  # pyright: ignore[reportArgumentType]
    assert type(node.value) is type(expected)
    assert node.same_as(Const(expected))  # pyright: ignore[reportArgumentType]


def test_const_converts_numpy_bool_without_importing_numpy() -> None:
    # Stand-in for numpy.bool_: detected by module and class name only.
    def truthy(_: object) -> bool:
        return True

    fake_bool = type("bool_", (), {"__module__": "numpy", "__bool__": truthy})
    node = Const(fake_bool())  # pyright: ignore[reportArgumentType]
    assert node.value is True


def test_const_keeps_timezone_of_datetimes() -> None:
    value = Const(dt.datetime(2026, 1, 1, 12, tzinfo=dt.UTC)).value
    assert isinstance(value, dt.datetime)
    assert value.tzinfo is dt.UTC


def test_const_rejects_sub_microsecond_datetimes() -> None:
    class NanoDatetime(dt.datetime):  # stand-in for pandas.Timestamp
        nanosecond = 5

    with pytest.raises(ExpressionError, match="sub-microsecond"):
        Const(NanoDatetime(2026, 1, 1))


@pytest.mark.parametrize("value", [[1], {"a": 1}, (1, 2), object(), b"bytes", 1j])
def test_const_rejects_non_scalars(value: object) -> None:
    with pytest.raises(ExpressionError, match="not a valid constant"):
        Const(value)  # pyright: ignore[reportArgumentType]


def test_const_rejects_nan_with_a_hint() -> None:
    with pytest.raises(ExpressionError, match="NaN") as info:
        Const(float("nan"))
    assert "IsNull" in (info.value.hint or "")


@pytest.mark.parametrize("value", [I64_MAX + 1, I64_MIN - 1, 10**30])
def test_const_rejects_integers_beyond_64_bits(value: int) -> None:
    with pytest.raises(ExpressionError, match="64 bits"):
        Const(value)


@pytest.mark.parametrize("value", [I64_MAX, I64_MIN])
def test_const_accepts_64_bit_limits(value: int) -> None:
    assert Const(value).value == value


@pytest.mark.parametrize(
    ("value", "kind"),
    [(True, "any"), (None, "any"), (3, "value"), ("x", "value"), (dt.date(2026, 1, 1), "value")],
)
def test_const_kind_depends_on_value(value: object, kind: str) -> None:
    assert Const(value).kind == kind  # pyright: ignore[reportArgumentType]


# --- Compare ---------------------------------------------------------------------------


@pytest.mark.parametrize("op", ["eq", "ne", "lt", "le", "gt", "ge"])
def test_compare_tag_is_its_operator(op: str) -> None:
    node = Compare(op, Field("x"), Const(1))  # pyright: ignore[reportArgumentType]
    assert (node.tag, node.kind) == (op, "condition")


def test_compare_accepts_boolean_constants_as_values() -> None:
    assert Compare("eq", Field("flag"), Const(True)).tag == "eq"


def test_compare_rejects_unknown_operator() -> None:
    with pytest.raises(ExpressionError, match="'gte'"):
        Compare("gte", Field("x"), Const(1))  # pyright: ignore[reportArgumentType]


def test_compare_rejects_raw_python_values_with_a_hint() -> None:
    with pytest.raises(ExpressionError, match="expression node") as info:
        Compare("gt", Field("x"), 1)  # pyright: ignore[reportArgumentType]
    assert "Const" in (info.value.hint or "")


def test_compare_rejects_conditions_as_operands() -> None:
    with pytest.raises(ExpressionError, match="condition"):
        Compare("eq", a, b)


def test_compare_with_none_points_to_is_null() -> None:
    with pytest.raises(ExpressionError, match="None") as info:
        Compare("eq", Field("x"), Const(None))
    assert "IsNull" in (info.value.hint or "")


# --- And / Or / Not --------------------------------------------------------------------


def test_and_takes_operands_as_arguments_and_flattens() -> None:
    assert same_nodes(And(And(a, b), c).operands, (a, b, c))


def test_or_flattens_nested_ors_but_keeps_ands() -> None:
    inner = And(a, b)
    assert same_nodes(Or(Or(a, c), inner).operands, (a, c, inner))


def test_connectives_keep_operand_order_and_duplicates() -> None:
    assert same_nodes(And(c, a, c).operands, (c, a, c))


@pytest.mark.parametrize("cls", [And, Or])
@pytest.mark.parametrize("operands", [(), (a,)], ids=["none", "one"])
def test_connectives_need_two_operands(cls: type[And | Or], operands: tuple[Expr, ...]) -> None:
    with pytest.raises(ExpressionError, match="at least two") as info:
        cls(*operands)
    assert info.value.hint is not None


@pytest.mark.parametrize("cls", [And, Or])
def test_connectives_reject_value_operands(cls: type[And | Or]) -> None:
    with pytest.raises(ExpressionError, match="value"):
        cls(a, Const(5))


def test_connectives_accept_fields_and_boolean_or_null_constants() -> None:
    assert And(Field("is_vip"), Const(True), Const(None)).tag == "and"


def test_not_rejects_value_operand() -> None:
    with pytest.raises(ExpressionError, match="value"):
        Not(Const("x"))


def test_tags_and_kinds() -> None:
    assert [(n.tag, n.kind) for n in (Field("x"), And(a, b), Or(a, b), Not(a))] == [
        ("field", "any"),
        ("and", "condition"),
        ("or", "condition"),
        ("not", "condition"),
    ]


@pytest.mark.parametrize("cls", [Expr, And.__mro__[1]], ids=["Expr", "_Connective"])
def test_abstract_bases_cannot_be_instantiated(cls: type[Expr]) -> None:
    with pytest.raises(ExpressionError, match="abstract"):
        cls()


# --- Traversal -------------------------------------------------------------------------


def test_children_and_preorder_walk() -> None:
    negated = Not(b)
    tree = And(a, negated)
    assert same_nodes(tree.children(), (a, negated))
    assert [n.tag for n in tree.walk()] == [
        "and", "gt", "field", "const", "not", "lt", "field", "const",
    ]  # fmt: skip


def test_leaves_have_no_children() -> None:
    assert Field("x").children() == ()
    assert Const(1).children() == ()


# --- Structural identity ---------------------------------------------------------------


def test_structurally_equal_trees_are_same() -> None:
    assert And(a, b).same_as(And(a, Compare("lt", Field("b"), Const(2))))


def test_operand_order_matters() -> None:
    assert not And(a, b).same_as(And(b, a))


def test_const_identity_includes_type() -> None:
    assert not Const(1).same_as(Const(True))
    assert not Const(1).same_as(Const(1.0))
    assert not Const(dt.date(2026, 1, 1)).same_as(Const(dt.datetime(2026, 1, 1)))


def test_same_as_other_types_is_false() -> None:
    assert not Const(1).same_as(1)


def test_structural_key_works_as_a_dict_key() -> None:
    seen = {And(a, b).structural_key(): "first"}
    assert seen[And(a, b).structural_key()] == "first"


def test_nodes_are_immutable() -> None:
    with pytest.raises(dataclasses.FrozenInstanceError):
        a.op = "lt"  # pyright: ignore[reportAttributeAccessIssue]


@dataclasses.dataclass(frozen=True, eq=False, slots=True)
class Opaque(Expr):
    payload: object

    TAGS = frozenset({"opaque"})
    _tag = "opaque"
    _kind = "condition"


def test_structural_key_refuses_unknown_data() -> None:
    # A future node holding non-scalar data (e.g. a fuzzytool term) must define its key.
    with pytest.raises(TypeError, match="override structural_key"):
        Opaque(object()).structural_key()


def test_nodes_are_hashable_and_weak_referenceable() -> None:
    for node in (Field("x"), Const(1), a, And(a, b), Or(a, b), Not(a)):
        assert hash(node) == hash(node)
        assert weakref.ref(node)() is node


# --- Catalogue vs spec -----------------------------------------------------------------


def test_node_union_covers_exactly_the_declared_tags() -> None:
    node_tags = {tag for cls in get_args(Node) for tag in cls.TAGS}
    assert node_tags == set(get_args(Tag))


def test_every_tag_is_in_the_spec_catalogue() -> None:
    spec = (Path(__file__).parents[2] / "docs" / "spec" / "ast.md").read_text(encoding="utf-8")
    catalogue = spec.split("## Catalogue", 1)[1].split("\n## ", 1)[0]
    first_cells = (line.split("|")[1] for line in catalogue.splitlines() if line.startswith("| `"))
    spec_tags = {tag for cell in first_cells for tag in re.findall(r"`([a-z_]+)`", cell)}
    assert set(get_args(Tag)) <= spec_tags


# --- Properties ------------------------------------------------------------------------


@given(conditions)
def test_walk_starts_at_root_and_visits_every_child(tree: Expr) -> None:
    nodes = list(tree.walk())
    assert nodes[0] is tree
    for node in nodes:
        for child in node.children():
            assert any(child is n for n in nodes)


@given(conditions)
def test_connectives_never_directly_contain_themselves(tree: Expr) -> None:
    for node in tree.walk():
        if isinstance(node, And | Or):
            assert not any(type(op) is type(node) for op in node.operands)


@given(conditions)
def test_trees_survive_pickling(tree: Expr) -> None:
    assert pickle.loads(pickle.dumps(tree)).same_as(tree)


@given(conditions)
def test_structural_key_is_stable_across_copies(tree: Expr) -> None:
    clone = pickle.loads(pickle.dumps(tree))
    assert hash(clone.structural_key()) == hash(tree.structural_key())


@given(scalars)
def test_const_is_idempotent(value: object) -> None:
    once = Const(value)  # pyright: ignore[reportArgumentType]
    assert Const(once.value).same_as(once)
