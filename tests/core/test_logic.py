"""Tasks 2.4-2.5: the Logic protocol, the Truth values and Kleene BooleanLogic."""

import itertools
import math
import pickle
from decimal import Decimal

import pytest

from rulepy.core._compat import is_null
from rulepy.core.logic import (
    BooleanLogic,
    GradedLogic,
    Logic,
    Truth,
    get_logic,
    logic_names,
    register_logic,
    unregister_logic,
)
from rulepy.errors import RegistryError, RulePyError, TypeMismatchError, UnsupportedOperation

T, F_, U = Truth.TRUE, Truth.FALSE, Truth.UNKNOWN
kleene = BooleanLogic()


# --- Truth -----------------------------------------------------------------------------


def test_truth_cannot_be_used_as_python_bool() -> None:
    # Otherwise UNKNOWN would silently count as true in `if truth:`.
    for value in Truth:
        with pytest.raises(TypeError, match="is_true"):
            bool(value)


def test_truth_is_ordered_false_unknown_true() -> None:
    assert F_ < U < T
    assert max([F_, T, U]) is T
    assert sorted([T, F_, U]) == [F_, U, T]


def test_truth_does_not_compare_with_numbers() -> None:
    with pytest.raises(TypeError):
        _ = T < 3  # pyright: ignore[reportOperatorIssue]
    assert T != 2


def test_truth_reads_well_and_pickles() -> None:
    assert [str(v) for v in Truth] == ["FALSE", "UNKNOWN", "TRUE"]
    assert repr(U) == "Truth.UNKNOWN"
    assert pickle.loads(pickle.dumps(U)) is U


# --- Kleene tables (spec/ast.md) ---------------------------------------------------

AND_OR_TABLE = [
    (T, T, T, T),
    (T, F_, F_, T),
    (T, U, U, T),
    (F_, F_, F_, F_),
    (F_, U, F_, U),
    (U, U, U, U),
]


@pytest.mark.parametrize(("a", "b", "conj", "disj"), AND_OR_TABLE)
def test_kleene_and_or_match_the_spec_table(a: Truth, b: Truth, conj: Truth, disj: Truth) -> None:
    for x, y in ((a, b), (b, a)):  # the spec lists each unordered pair once
        assert kleene.and_(x, y) is conj
        assert kleene.or_(x, y) is disj


def test_kleene_not() -> None:
    assert [kleene.not_(v) for v in (T, F_, U)] == [F_, T, U]


def test_n_ary_helpers_take_evaluated_sequences() -> None:
    # A generator would let early stopping skip evaluations (and errors), making the
    # result depend on operand order; the contract is a Sequence of computed values.
    assert kleene.all_((T, T)) is T
    assert kleene.any_([F_, F_]) is F_


def test_n_ary_helpers_fold_and_have_neutral_elements() -> None:
    assert kleene.all_([]) is T
    assert kleene.any_([]) is F_
    assert kleene.all_([T, U, T]) is U
    assert kleene.all_([T, U, F_]) is F_
    assert kleene.any_([F_, U]) is U
    assert kleene.any_([U, T, F_]) is T


def test_only_true_fires() -> None:
    assert [kleene.is_true(v) for v in (T, F_, U)] == [True, False, False]


# --- Converting data values ------------------------------------------------------------


class _PandasNA:  # stand-ins detected by module and name, like the real ones
    __module__ = "pandas._libs.missing"


_PandasNA.__name__ = "NAType"


@pytest.mark.parametrize(
    ("value", "truth"),
    [(True, T), (False, F_), (None, U), (math.nan, U), (_PandasNA(), U)],
    ids=["true", "false", "none", "nan", "pd.NA"],
)
def test_from_value_maps_booleans_and_missing(value: object, truth: Truth) -> None:
    assert kleene.from_value(value) is truth


def test_from_value_accepts_numpy_bool_without_numpy() -> None:
    def falsy(_: object) -> bool:
        return False

    fake = type("bool_", (), {"__module__": "numpy", "__bool__": falsy})
    assert kleene.from_value(fake()) is F_


@pytest.mark.parametrize("value", [1, 0, "yes", 1.5, [True]], ids=repr)
def test_from_value_rejects_other_types_as_mismatch(value: object) -> None:
    with pytest.raises(TypeMismatchError, match="boolean") as info:
        kleene.from_value(value)
    assert info.value.types == ("boolean", type(value).__name__)


@pytest.mark.parametrize(
    "value",
    [None, math.nan, float("nan"), Decimal("NaN"), _PandasNA()],
    ids=["none", "nan", "float-nan", "decimal-nan", "pd.NA"],
)
def test_is_null_recognizes_missing_values(value: object) -> None:
    assert is_null(value)


@pytest.mark.parametrize(
    "value", [0, 0.0, "", False, math.inf, Decimal("1"), [], object()], ids=repr
)
def test_is_null_rejects_real_values(value: object) -> None:
    assert not is_null(value)


def test_is_null_never_trusts_a_non_bool_inequality() -> None:
    class Weird:
        def __ne__(self, other: object) -> object:  # pyright: ignore[reportIncompatibleMethodOverride]
            return "yes"  # truthy, but not a bool: must not count as NaN

    assert not is_null(Weird())


def test_is_null_treats_a_failing_inequality_as_a_value() -> None:
    class Strict:
        def __ne__(self, other: object) -> bool:
            raise ValueError("not comparable")

    assert not is_null(Strict())


def test_from_bool() -> None:
    assert (kleene.from_bool(True), kleene.from_bool(False), kleene.from_bool(None)) == (T, F_, U)


# --- Laws, run against every registered logic ------------------------------------------


def test_boolean_logic_is_graded() -> None:
    assert isinstance(kleene, GradedLogic)
    assert [kleene.degree(v) for v in (T, F_, U)] == [1.0, 0.0, None]
    assert (kleene.from_degree(1.0), kleene.from_degree(0.0)) == (T, F_)


def test_boolean_logic_rejects_intermediate_degrees() -> None:
    with pytest.raises(UnsupportedOperation, match="boolean") as info:
        kleene.from_degree(0.4)
    assert "fuzzy" in (info.value.hint or "")


class _MinimalKleene(Logic[Truth]):
    """Implements only the required members; all_/any_ come from the protocol."""

    name = "minimal"
    TRUE, FALSE, UNKNOWN = Truth.TRUE, Truth.FALSE, Truth.UNKNOWN

    def and_(self, a: Truth, b: Truth) -> Truth:
        return min(a, b)

    def or_(self, a: Truth, b: Truth) -> Truth:
        return max(a, b)

    def not_(self, a: Truth) -> Truth:
        return kleene.not_(a)

    def from_bool(self, value: bool | None) -> Truth:
        return kleene.from_bool(value)

    def from_value(self, value: object) -> Truth:
        return kleene.from_value(value)

    def is_true(self, value: Truth) -> bool:
        return value is Truth.TRUE


def test_protocol_defaults_for_n_ary_helpers_match_kleene() -> None:
    minimal = _MinimalKleene()
    for values in itertools.chain.from_iterable(
        itertools.product(list(Truth), repeat=n) for n in range(4)
    ):
        assert minimal.all_(values) is kleene.all_(values)
        assert minimal.any_(values) is kleene.any_(values)


def _values(logic: Logic[object]) -> list[object]:
    return [logic.FALSE, logic.UNKNOWN, logic.TRUE]


@pytest.fixture(params=sorted(logic_names()))
def logic(request: pytest.FixtureRequest) -> Logic[object]:
    return get_logic(request.param)


def test_conjunction_and_disjunction_are_commutative_and_associative(logic: Logic[object]) -> None:
    for a, b, c in itertools.product(_values(logic), repeat=3):
        assert logic.and_(a, b) == logic.and_(b, a)
        assert logic.or_(a, b) == logic.or_(b, a)
        assert logic.and_(logic.and_(a, b), c) == logic.and_(a, logic.and_(b, c))
        assert logic.or_(logic.or_(a, b), c) == logic.or_(a, logic.or_(b, c))


def test_identity_and_annihilator_elements(logic: Logic[object]) -> None:
    for a in _values(logic):
        assert logic.and_(a, logic.TRUE) == a
        assert logic.or_(a, logic.FALSE) == a
        assert logic.and_(a, logic.FALSE) == logic.FALSE
        assert logic.or_(a, logic.TRUE) == logic.TRUE


def test_de_morgan_and_double_negation(logic: Logic[object]) -> None:
    for a, b in itertools.product(_values(logic), repeat=2):
        assert logic.not_(logic.and_(a, b)) == logic.or_(logic.not_(a), logic.not_(b))
        assert logic.not_(logic.or_(a, b)) == logic.and_(logic.not_(a), logic.not_(b))
        assert logic.not_(logic.not_(a)) == a


def test_kleene_is_idempotent_and_distributive() -> None:
    # Not every logic has these (product t-norms are not idempotent), so Kleene only.
    for a, b, c in itertools.product(list(Truth), repeat=3):
        assert kleene.and_(a, a) is a
        assert kleene.or_(a, a) is a
        assert kleene.and_(a, kleene.or_(b, c)) is kleene.or_(kleene.and_(a, b), kleene.and_(a, c))


def test_excluded_middle_does_not_hold_for_unknown() -> None:
    # The defining difference from classical logic: x | ~x is UNKNOWN when x is.
    assert kleene.or_(U, kleene.not_(U)) is U


# --- Registry --------------------------------------------------------------------------


def test_boolean_logic_is_registered_and_satisfies_the_protocol() -> None:
    assert "boolean" in logic_names()
    assert isinstance(get_logic("boolean"), BooleanLogic)
    assert isinstance(kleene, Logic)
    assert kleene.name == "boolean"


def test_boolean_logic_repr() -> None:
    assert repr(kleene) == "BooleanLogic()"


def test_logics_of_the_same_type_and_parameters_are_equal() -> None:
    assert BooleanLogic() == BooleanLogic()
    assert hash(BooleanLogic()) == hash(BooleanLogic())
    assert get_logic("boolean") == kleene


def test_get_logic_forwards_parameters_to_the_factory() -> None:
    seen: dict[str, object] = {}

    def factory(**params: object) -> BooleanLogic:
        seen.update(params)
        return BooleanLogic()

    register_logic("param-test", factory)
    try:
        get_logic("param-test", tnorm="prod")
    finally:
        unregister_logic("param-test")
    assert seen == {"tnorm": "prod"}


def test_parameters_require_a_name() -> None:
    with pytest.raises(RegistryError, match="parameters"):
        get_logic(kleene, tnorm="prod")


def test_get_logic_passes_instances_through() -> None:
    assert get_logic(kleene) is kleene


def test_unknown_logic_name_lists_the_options() -> None:
    with pytest.raises(RegistryError, match="'fuzzzy'") as info:
        get_logic("fuzzzy")
    assert "boolean" in (info.value.hint or "")
    assert isinstance(info.value, LookupError)
    assert isinstance(info.value, RulePyError)


def test_register_refuses_to_overwrite_silently() -> None:
    register_logic("dup-test", BooleanLogic)
    try:
        with pytest.raises(RegistryError, match="already registered"):
            register_logic("dup-test", BooleanLogic)
    finally:
        unregister_logic("dup-test")


def test_builtin_logics_cannot_be_replaced() -> None:
    with pytest.raises(RegistryError, match="built-in"):
        register_logic("boolean", BooleanLogic, overwrite=True)


def test_register_and_overwrite() -> None:
    register_logic("kleene-test", BooleanLogic)
    try:
        assert isinstance(get_logic("kleene-test"), BooleanLogic)
        register_logic("kleene-test", BooleanLogic, overwrite=True)
    finally:
        unregister_logic("kleene-test")
    assert "kleene-test" not in logic_names()


def test_builtin_logics_cannot_be_unregistered() -> None:
    with pytest.raises(RegistryError, match="built-in"):
        unregister_logic("boolean")
    assert "boolean" in logic_names()


def test_unregistering_an_unknown_name_is_an_error() -> None:
    with pytest.raises(RegistryError, match="unknown"):
        unregister_logic("never-registered")
