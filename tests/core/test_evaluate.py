"""Task 2.6: the reference evaluator (the semantic oracle for every other backend)."""

import collections
import contextlib
import dataclasses
import datetime as dt
import math
import os
from decimal import Decimal
from fractions import Fraction
from typing import Any, NamedTuple

import pytest
from hypothesis import given
from hypothesis import strategies as st

from rulepy import F
from rulepy.core.evaluate import (
    NodeResult,
    Reason,
    evaluate,
    evaluate_tree,
    evaluate_value,
    evaluate_value_tree,
    type_group,
)
from rulepy.core.expr import And, Const, Expr, Field, Not, Or
from rulepy.core.logic import BooleanLogic, Truth
from rulepy.errors import EvaluationError, ExpressionError, MissingFieldError, TypeMismatchError
from tests.strategies import conditions, scalars

T, FALSE, U = Truth.TRUE, Truth.FALSE, Truth.UNKNOWN


def ev(condition: Expr, data: object, **options: Any) -> Truth:
    return evaluate(condition, data, **options)


def why(condition: Expr, data: object) -> str | None:
    reason = evaluate_tree(condition, data).reason
    return None if reason is None else str(reason)


# --- Reading fields ---------------------------------------------------------------------


@dataclasses.dataclass
class Customer:
    age: int
    tags: list[str]


class Plain:
    def __init__(self) -> None:
        self.score = 7


class Point(NamedTuple):
    x: int
    y: int


class RowLike:
    """Duck-typed like a pandas row: keys() + __getitem__, plus methods named like data."""

    def __init__(self, values: dict[str, object]) -> None:
        self._values = values

    def keys(self) -> list[str]:
        return list(self._values)

    def __getitem__(self, key: str) -> object:
        return self._values[key]

    def count(self) -> int:  # a method that shadows a column named "count"
        return 0


@pytest.mark.parametrize(
    ("data", "condition"),
    [
        ({"customer": {"age": 30}}, F.customer.age > 18),
        ({"orders": [{"total": 5}, {"total": 50}]}, F.orders[1].total == 50),
        ({"c": Customer(age=30, tags=["vip"])}, F.c.age >= 30),
        ({"c": Customer(age=30, tags=["vip"])}, F.c["tags"][0] == "vip"),
        (Plain(), F.score == 7),
        (RowLike({"count": 3}), F["count"] == 3),
        ({"p": Point(1, 2)}, F.p.y == 2),
        ({"p": Point(1, 2)}, F.p[0] == 1),
        ({"_id": 4}, F["_id"] == 4),  # underscore keys are fine in mappings
    ],
    ids=[
        "dict", "list-index", "dataclass", "dataclass-list", "object", "row-like",
        "namedtuple-name", "namedtuple-index", "underscore-key",
    ],
)  # fmt: skip
def test_fields_resolve_in_common_containers(data: object, condition: Expr) -> None:
    assert ev(condition, data) is T


@pytest.mark.parametrize(
    ("data", "condition", "expected"),
    [
        ({}, F.age > 1, "missing field age"),
        ({"orders": [1]}, F.orders[3] > 1, "missing field orders[3]"),
        ({"orders": [1]}, F.orders["x"] > 1, "missing field orders.x"),
        ({"s": "abc"}, F.s["upper"] == "ABC", "missing field s.upper"),
        (Plain(), F.missing > 1, "missing field missing"),
        ({"x": 5}, F.x[0] == 1, "missing field x[0]"),
        (RowLike({"a": 1}), F["b"] == 1, "missing field b"),
        ({1: "one"}, F[1] == "one", "missing field [1]"),  # ints index sequences only
        ({"d": dt.date(2020, 1, 1)}, F.d.year == 2020, "missing field d.year"),
        ({"n": 5}, F.n.real == 5, "missing field n.real"),
        ({"k": "a b"}, F["x y"][0] == 1, "missing field ['x y']"),
    ],
    ids=[
        "key", "index", "name-on-list", "method-is-not-data", "attribute",
        "index-into-scalar", "row-missing-key", "int-key-on-dict", "date-attribute",
        "int-attribute", "quoted-path",
    ],
)  # fmt: skip
def test_missing_fields_are_unknown_with_a_reason(
    data: object, condition: Expr, expected: str
) -> None:
    assert ev(condition, data) is U
    assert why(condition, data) == expected


# --- Security: rules from files must not reach into the process --------------------------


class Holder:
    def __init__(self) -> None:
        self.module = os
        self.kind = int
        self.method = self.__init__


@pytest.mark.parametrize(
    "condition",
    [
        Field("o", "__class__", "__init__", "__globals__", "os", "environ", "HOME").is_null(),
        Field("o", "__dict__").is_null(),
        F.o.module.environ.is_null(),
        F.o["kind"]["__name__"].is_null(),
        F.o.method.is_null(),
    ],
    ids=["dunder-chain", "dunder-dict", "module", "class", "method"],
)
def test_attribute_lookup_never_leaves_public_data(condition: Expr) -> None:
    result = evaluate_tree(condition, {"o": Holder()})
    assert result.value is T  # reads as missing, so is_null holds
    reason = result.children[0].reason
    assert reason is not None
    assert reason.code == "missing_field"


def test_private_attributes_are_not_data() -> None:
    class Secret:
        def __init__(self) -> None:
            self._token = "s3cret"

    assert ev(F.s["_token"].starts_with("s"), {"s": Secret()}) is U


# --- Missing and null ------------------------------------------------------------------


def test_null_in_the_middle_of_a_path_is_null_not_missing() -> None:
    data = {"customer": None}
    assert ev(F.customer.age > 1, data) is U
    assert why(F.customer.age > 1, data) == "customer is null"
    assert ev(F.customer.age > 1, data, strict=True) is U


def test_null_input_is_named() -> None:
    assert why(F.x > 1, None) == "the input is null"


def test_dict_subclasses_are_read_without_side_effects() -> None:
    counts: collections.Counter[str] = collections.Counter(a=2)
    defaults: collections.defaultdict[str, int] = collections.defaultdict(int, a=1)
    assert ev(F.b > 0, counts) is U  # missing, not Counter's 0
    assert ev(F.b > 0, defaults) is U
    assert "b" not in defaults  # reading did not insert a default


def test_strict_mode_raises_on_missing_fields_with_available_keys() -> None:
    with pytest.raises(MissingFieldError) as info:
        ev(F.customer.agee > 18, {"customer": {"age": 30, "name": "x"}}, strict=True, rule="adult")
    err = info.value
    assert err.path == ("customer", "agee")
    assert err.rule == "adult"
    assert "'age'" in (err.hint or "")


@pytest.mark.parametrize(
    ("data", "hint"),
    [({}, "the mapping is empty"), (Plain(), None)],
    ids=["empty-mapping", "object"],
)
def test_strict_missing_field_hints(data: object, hint: str | None) -> None:
    with pytest.raises(MissingFieldError) as info:
        ev(F.nope > 1, data, strict=True)
    assert info.value.hint == hint


class Flaky:
    @property
    def total(self) -> int:
        raise ValueError("database is down")


def test_a_property_that_raises_is_unknown_not_a_crash() -> None:
    result = evaluate_tree(F.f.total > 1, {"f": Flaky()})
    assert result.value is U
    assert str(result.reason) == "cannot read f.total: ValueError: database is down"


def test_a_property_that_raises_in_strict_mode_is_an_evaluation_error() -> None:
    with pytest.raises(EvaluationError, match="database is down") as info:
        ev(F.f.total > 1, {"f": Flaky()}, strict=True)
    assert isinstance(info.value.__cause__, ValueError)


# --- Normalization of data values ------------------------------------------------------


@pytest.mark.parametrize(
    "value", [None, math.nan, Decimal("NaN")], ids=["none", "nan", "decimal-nan"]
)
def test_null_and_nan_data_are_unknown(value: object) -> None:
    assert ev(F.x > 1, {"x": value}) is U
    assert ev(F.x.is_null(), {"x": value}) is T


def test_numpy_scalars_and_arrays() -> None:
    np = pytest.importorskip("numpy")
    data = {
        "i": np.int64(2**53 + 1),
        "f": np.float32(1.5),
        "b": np.bool_(True),
        "v": np.array([10, 20]),
        "t": np.datetime64("2026-01-02T03:04:05"),
    }
    assert ev(F.i > float(2**53), data) is T  # exact, not via float
    assert ev(F.f == 1.5, data) is T
    assert ev(F.b, data) is T
    assert ev(F.v[1] == 20, data) is T
    assert ev(F.t > dt.datetime(2026, 1, 1), data) is T


def test_numpy_datetime64_becomes_a_datetime() -> None:
    class FakeDatetime64:  # stand-in for numpy.datetime64, detected by module and name
        __module__ = "numpy"

        def __ne__(self, other: object) -> bool:  # not NaT
            return other is not self

        def astype(self, unit: str) -> "FakeDatetime64":
            assert unit == "datetime64[us]"
            return self

        def item(self) -> dt.datetime:
            return dt.datetime(2026, 1, 2)

    FakeDatetime64.__name__ = "datetime64"
    assert ev(F.t == dt.datetime(2026, 1, 2), {"t": FakeDatetime64()}) is T


def test_indexing_an_object_without_length_is_missing() -> None:
    assert why(F.p[0] == 1, {"p": Plain()}) == "missing field p[0]"


def test_integers_beyond_64_bits_are_null() -> None:
    assert ev(F.x > 0, {"x": 2**70}) is U
    assert why(F.x > 0, {"x": 2**70}) == "integer overflow in x"


# --- Comparisons -----------------------------------------------------------------------


@pytest.mark.parametrize(
    ("condition", "expected"),
    [
        (F.n == 5, T),
        (F.n != 5, FALSE),
        (F.n < 6, T),
        (F.n <= 5, T),
        (F.n > 5, FALSE),
        (F.n >= 5.0, T),
        (F.s < "b", T),
        (F.s == "a", T),
        (F.d < dt.date(2026, 1, 2), T),
        (F.flag == True, T),  # noqa: E712 - builds a node, not a Python comparison
        (F.n == F.m, FALSE),
        (F.amount > 1000, T),
        (F.amount == 1000.5, T),
        (F.half == 0.5, T),
    ],
)
def test_comparisons_within_a_type_group(condition: Expr, expected: Truth) -> None:
    data = {
        "n": 5,
        "m": 6,
        "s": "a",
        "d": dt.date(2026, 1, 1),
        "flag": True,
        "amount": Decimal("1000.5"),
        "half": Fraction(1, 2),
    }
    assert ev(condition, data) is expected


def test_int_and_float_compare_exactly() -> None:
    assert ev(F.x > float(2**53), {"x": 2**53 + 1}) is T


@pytest.mark.parametrize(
    ("value", "condition", "expected"),
    [
        ("5", F.x == 5, "type mismatch: string vs number"),
        (5, F.x == "5", "type mismatch: number vs string"),
        (True, F.x > False, "cannot order booleans"),
        (True, F.x == 1, "type mismatch: boolean vs number"),
        (
            dt.datetime(2026, 1, 1),
            F.x > dt.date(2026, 1, 1),
            "type mismatch: naive datetime vs date",
        ),
        (
            dt.datetime(2026, 1, 1, tzinfo=dt.UTC),
            F.x > dt.datetime(2026, 1, 1),
            "type mismatch: aware datetime vs naive datetime",
        ),
        ([1], F.x == 1, "type mismatch: other (list) vs number"),
    ],
    ids=["str-int", "int-str", "bool-order", "bool-int", "datetime-date", "aware-naive", "list"],
)
def test_type_mismatches_are_unknown_with_a_reason(
    value: object, condition: Expr, expected: str
) -> None:
    assert ev(condition, {"x": value}) is U
    assert why(condition, {"x": value}) == expected


def test_mismatch_reasons_are_structured() -> None:
    reason = evaluate_tree(F.code == 1, {"code": "1"}).reason
    assert reason == Reason("type_mismatch", (("code",),), ("string", "number"))


def test_strict_mode_raises_on_type_mismatch_with_types_path_and_rule() -> None:
    with pytest.raises(TypeMismatchError) as info:
        ev(F.code == 1, {"code": "1"}, strict=True, rule="r1")
    err = info.value
    assert (err.types, err.path, err.rule) == (("string", "number"), ("code",), "r1")
    assert err.node is not None


def test_null_wins_over_type_checks() -> None:
    assert ev(F.x == "a", {"x": None}, strict=True) is U


# --- Connectives (Kleene, no short circuit) --------------------------------------------


def test_connectives_follow_kleene() -> None:
    data = {"a": 1, "b": None}
    assert ev((F.a > 0) & (F.b > 0), data) is U
    assert ev((F.a > 5) & (F.b > 0), data) is FALSE
    assert ev((F.a > 0) | (F.b > 0), data) is T
    assert ev(~(F.b > 0), data) is U


def test_reasons_only_explain_unknown() -> None:
    data = {"x": 5, "lo": "a"}
    assert evaluate_tree((F.missing > 1) | (F.x > 0), data).reason is None
    assert evaluate_tree(~((F.missing > 1) & (F.x > 5)), data).reason is None
    assert evaluate_tree(F.x.between(F.lo, 3), data).reason is None  # FALSE
    assert why((F.missing > 1) & (F.x > 0), data) == "missing field missing"


@pytest.mark.parametrize("first_false", [True, False])
def test_strict_errors_do_not_depend_on_operand_order(first_false: bool) -> None:
    false_cond, mismatch = F.a > 5, F.code == 1
    tree = And(false_cond, mismatch) if first_false else And(mismatch, false_cond)
    with pytest.raises(TypeMismatchError):
        ev(tree, {"a": 1, "code": "x"}, strict=True)


def test_strict_errors_are_raised_in_left_to_right_post_order() -> None:
    data = {"a": "s"}  # a: type mismatch; b: missing
    with pytest.raises(TypeMismatchError):
        ev((F.a > 1) & (F.b > 1), data, strict=True)
    with pytest.raises(MissingFieldError):
        ev((F.b > 1) & (F.a > 1), data, strict=True)


def test_fields_and_constants_as_conditions() -> None:
    assert ev(F.vip, {"vip": True}) is T
    assert ev(F.vip, {"vip": None}) is U
    assert ev(~F.vip, {"vip": False}) is T
    assert ev(And(F.vip, Const(True)), {"vip": True}) is T
    assert ev(Or(Const(None), Const(False)), {}) is U


def test_non_boolean_field_as_condition_is_a_mismatch() -> None:
    assert ev(F.vip, {"vip": 1}) is U
    assert why(F.vip, {"vip": 1}) == "type mismatch: number used as a condition"
    with pytest.raises(TypeMismatchError) as info:
        ev(F.vip, {"vip": "yes"}, strict=True)
    assert info.value.path == ("vip",)


# --- Predicates ------------------------------------------------------------------------


def test_in() -> None:
    assert ev(F.c.is_in({"ES", "PT"}), {"c": "ES"}) is T
    assert ev(F.c.is_in({"ES", "PT"}), {"c": "FR"}) is FALSE
    assert ev(F.n.is_in([1, 2]), {"n": 2.0}) is T
    assert ev(F.c.is_in({"ES"}), {"c": None}) is U
    assert ev(F.c.is_in({"ES"}), {"c": 3}) is U


@pytest.mark.parametrize(
    ("closed", "results"),
    [
        ("both", (T, T, T)),
        ("left", (T, T, FALSE)),
        ("right", (FALSE, T, T)),
        ("none", (FALSE, T, FALSE)),
    ],
)
def test_between_closed(closed: str, results: tuple[Truth, ...]) -> None:
    cond = F.x.between(1, 3, closed=closed)  # pyright: ignore[reportArgumentType]
    assert tuple(ev(cond, {"x": x}) for x in (1, 2, 3)) == results


def test_between_is_the_kleene_pair_of_comparisons() -> None:
    # low unknown but x above high: (x >= low) and (x <= high) = UNKNOWN and FALSE = FALSE
    assert ev(F.x.between(F.lo, 10), {"x": 20, "lo": None}) is FALSE
    assert ev(F.x.between(F.lo, 10), {"x": 5, "lo": None}) is U


def test_between_keeps_both_halves_for_explanations() -> None:
    result = evaluate_tree(F.x.between(1, 10), {"x": 20})
    low, high = result.children
    assert (repr(low.node), low.value) == ("F.x >= 1", T)
    assert (repr(high.node), high.value) == ("F.x <= 10", FALSE)


def test_is_null_is_never_unknown() -> None:
    assert ev(F.x.is_null(), {}) is T
    assert ev(F.x.is_null(), {"x": 0}) is FALSE


def test_is_null_on_missing_field_in_strict_mode_raises() -> None:
    with pytest.raises(MissingFieldError):
        ev(F.x.is_null(), {}, strict=True)


def test_matches_searches_and_supports_inline_flags() -> None:
    assert ev(F.s.matches(r"\d+"), {"s": "abc123"}) is T
    assert ev(F.s.matches(r"^\d+$"), {"s": "abc123"}) is FALSE
    assert ev(F.s.matches(r"(?i)ABC"), {"s": "abc"}) is T
    assert ev(F.s.matches("a"), {"s": 5}) is U


def test_string_tests_are_literal() -> None:
    data = {"s": "a.b"}
    assert ev(F.s.starts_with("a."), data) is T
    assert ev(F.s.ends_with(".b"), data) is T
    assert ev(F.s.contains("."), data) is T
    assert ev(F.s.contains("x"), data) is FALSE
    assert ev(F.s.contains("a"), {"s": ["a"]}) is U  # not list membership


# --- Arithmetic ------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("expr", "expected"),
    [
        (F.a + F.b, 7),
        (F.a - F.b, 3),
        (F.a * F.b, 10),
        (F.a / F.b, 2.5),
        (-F.a, -5),
        (abs(F.c), 1.5),
        (F.d + 1, 2.5),
        (-F.d, -1.5),
    ],
    ids=["add", "sub", "mul", "div", "neg", "abs", "decimal-add", "decimal-neg"],
)
def test_arithmetic(expr: Expr, expected: float) -> None:
    value = evaluate_value(expr, {"a": 5, "b": 2, "c": -1.5, "d": Decimal("1.5")})
    assert value == expected
    assert type(value) is type(expected)


def test_division_converts_operands_to_float64_first() -> None:
    a, b = 5258986265376043509, 132345
    assert evaluate_value(F.a / F.b, {"a": a, "b": b}) == float(a) / float(b)


@pytest.mark.parametrize(
    ("expr", "data", "expected"),
    [
        (F.a / F.b, {"a": 1, "b": 0}, "division by zero"),
        (F.a / F.b, {"a": 1.0, "b": 0.0}, "division by zero"),
        (F.a * F.b, {"a": 2**62, "b": 4}, "integer overflow in a, b"),
        (-F.a, {"a": -(2**63)}, "integer overflow in a"),
        (F.a - F.b, {"a": math.inf, "b": math.inf}, "NaN result"),
        (F.a + F.b, {"a": 1, "b": None}, "b is null"),
        (F.a + F.b, {"a": 1, "b": "x"}, "type mismatch: number vs string"),
        (-F.a, {"a": "x"}, "type mismatch: string vs number"),
        (abs(F.a), {"a": None}, "a is null"),
    ],
    ids=[
        "div-zero", "float-div-zero", "overflow", "neg-overflow", "nan", "null",
        "mismatch", "unary-mismatch", "unary-null",
    ],
)  # fmt: skip
def test_arithmetic_yields_null_with_a_reason(
    expr: Expr, data: dict[str, object], expected: str
) -> None:
    result = evaluate_tree(expr > 0, data)
    assert result.value is U
    assert str(result.reason) == expected


def test_value_root_is_rejected_by_evaluate() -> None:
    with pytest.raises(ExpressionError, match="value"):
        evaluate(F.a + 1, {"a": 1})


def test_condition_root_is_rejected_by_evaluate_value() -> None:
    with pytest.raises(ExpressionError, match="condition"):
        evaluate_value(F.a > 1, {"a": 1})


@pytest.mark.parametrize("bad", [True, 5, "F.x > 1"])
def test_non_nodes_are_rejected(bad: object) -> None:
    with pytest.raises(ExpressionError, match="not a node"):
        evaluate(bad, {})  # pyright: ignore[reportArgumentType, reportCallIssue]
    with pytest.raises(ExpressionError, match="not a node"):
        evaluate_value(bad, {})  # pyright: ignore[reportArgumentType]


def test_constant_null_reason() -> None:
    assert str(evaluate_tree(Const(None).is_null(), {}).children[0].reason) == "null value"


# --- Result tree -----------------------------------------------------------------------


def test_result_tree_mirrors_the_condition() -> None:
    cond = (F.a > 1) & ~(F.b == "x")
    result = evaluate_tree(cond, {"a": 2, "b": "y"})
    assert isinstance(result, NodeResult)
    assert result.node is cond
    assert result.value is T
    compare, negation = result.children
    assert [c.value for c in compare.children] == [2, 1]
    assert negation.children[0].value is FALSE


def test_field_used_as_condition_keeps_its_value_result() -> None:
    result = evaluate_tree(F.vip, {"vip": True})
    assert result.value is T
    assert result.children[0].value is True


def test_logic_can_be_passed_by_name_or_instance() -> None:
    assert ev(F.a > 1, {"a": 2}, logic="boolean") is T
    assert ev(F.a > 1, {"a": 2}, logic=BooleanLogic()) is T


# --- Properties ------------------------------------------------------------------------

data_dicts = st.dictionaries(st.text(min_size=1, max_size=8), scalars, max_size=6)


@given(conditions, data_dicts)
def test_non_strict_evaluation_never_raises(tree: Expr, data: dict[str, object]) -> None:
    assert isinstance(evaluate(tree, data), Truth)


@given(conditions, data_dicts)
def test_strict_evaluation_only_raises_evaluation_errors(
    tree: Expr, data: dict[str, object]
) -> None:
    with contextlib.suppress(EvaluationError):
        evaluate(tree, data, strict=True)


@given(conditions, data_dicts)
def test_reasons_never_accompany_true_or_false(tree: Expr, data: dict[str, object]) -> None:
    def check(result: NodeResult) -> None:
        if isinstance(result.value, Truth) and result.value is not U:
            assert result.reason is None
        for child in result.children:
            check(child)

    check(evaluate_tree(tree, data))


@given(conditions, conditions, data_dicts)
def test_de_morgan_holds_on_evaluation(a: Expr, b: Expr, data: dict[str, object]) -> None:
    assert ev(Not(And(a, b)), data) is ev(Or(Not(a), Not(b)), data)
    assert ev(Not(Or(a, b)), data) is ev(And(Not(a), Not(b)), data)


@given(conditions, data_dicts)
def test_a_condition_and_its_negation_are_never_both_true(
    tree: Expr, data: dict[str, object]
) -> None:
    assert ev(And(tree, Not(tree)), data) is not T


@given(st.lists(st.text(min_size=1, max_size=4), min_size=1, max_size=3), scalars)
def test_a_present_field_reads_back_its_value(path: list[str], value: object) -> None:
    data: object = value
    for segment in reversed(path):
        data = {segment: data}
    result = evaluate_tree(Field(*path).is_null(), data)
    assert result.value is (T if value is None else FALSE)


@pytest.mark.parametrize(
    ("value", "group"),
    [
        (1, "number"),
        (1.5, "number"),
        (Decimal("1"), "number"),
        (True, "boolean"),
        ("a", "string"),
        (dt.date(2020, 1, 1), "date"),
        (dt.datetime(2020, 1, 1), "naive datetime"),
        (dt.datetime(2020, 1, 1, tzinfo=dt.UTC), "aware datetime"),
        (object(), "other"),
        ([1], "other"),
    ],
    ids=repr,
)
def test_type_group(value: object, group: str) -> None:
    assert type_group(value) == group


def test_evaluate_value_tree_reports_why_and_names_the_rule() -> None:
    result = evaluate_value_tree(F.a * 2, {})
    assert result.value is None
    assert str(result.reason) == "missing field a"
    with pytest.raises(MissingFieldError) as info:
        evaluate_value_tree(F.a * 2, {}, strict=True, rule="r")
    assert info.value.rule == "r"


def test_signaling_decimal_nan_is_null() -> None:
    assert ev(F.x.is_null(), {"x": Decimal("sNaN")}) is T
