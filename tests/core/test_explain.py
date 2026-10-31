"""Tasks 2.10-2.11: explanations of decisions (why each rule fired or not)."""

import datetime as dt
from collections.abc import Mapping
from decimal import Decimal
from typing import Any

import pytest
from hypothesis import given
from hypothesis import strategies as st

from rulepy import F, Rule, RuleNotFoundError, RuleSet
from rulepy.core.evaluate import NodeResult
from rulepy.core.explain import (
    Cause,
    CauseGroup,
    Counterfactual,
    Explanation,
    Requirement,
    RuleExplanation,
)
from rulepy.core.expr import And, Between, Compare, Const, Expr, Not, Or
from rulepy.core.logic import BooleanLogic, Truth
from rulepy.errors import UnsupportedOperation
from tests.snapshot import assert_snapshot
from tests.strategies import conditions


def why(condition: Expr, data: Mapping[str, object]) -> RuleExplanation[Truth]:
    return RuleSet([Rule("r", when=condition)]).evaluate(data).why_not("r")


def texts(entry: RuleExplanation[Truth]) -> list[str]:
    return [str(cause) for cause in entry.causes]


def shape(requirement: Requirement) -> object:
    """Compact structure of a requirement: node reprs inside ('all'|'any', [...])."""
    if isinstance(requirement, Cause):
        return repr(requirement.result.node)
    return (requirement.kind, [shape(item) for item in requirement.items])


# --- Status and selection notes ---------------------------------------------------------

small = Rule("small", when=F.total > 10, then={"discount": 0.05}, priority=1)
big = Rule("big", when=F.total > 100, then={"discount": 0.10}, priority=5)
vip = Rule("vip", when=F.vip, then={"discount": 0.20}, priority=5)


def test_statuses() -> None:
    decision = RuleSet([small, big, vip]).evaluate({"total": 50, "vip": None})
    assert [(e.name, e.status) for e in decision.explain().rules] == [
        ("small", "selected"),
        ("big", "not_fired"),
        ("vip", "unknown"),
    ]


def test_first_explains_why_a_fired_rule_was_not_selected() -> None:
    entry = RuleSet([small, big]).evaluate({"total": 500}).why_not("big")
    assert entry.status == "fired"
    assert entry.note == "fired, but FIRST selected 'small', which comes first"


def test_priority_notes() -> None:
    rules = RuleSet([small, big, vip], hit_policy="priority")
    decision = rules.evaluate({"total": 500, "vip": True})
    assert decision.why_not("small").note == "fired, but 'big' has a higher priority (5 > 1)"
    assert decision.why_not("vip").note == "fired, but 'big' has the same priority and comes first"
    assert decision.why_not("big").note is None


def test_any_note_says_the_outputs_agree() -> None:
    rules = [Rule("a", when=F.x > 0, then={"v": 1}), Rule("b", when=F.x > 0, then={"v": 1})]
    entry = RuleSet(rules, hit_policy="any").evaluate({"x": 1}).why_not("b")
    assert entry.note == "fired; its output agrees with 'a' (ANY reports the first)"


def test_multi_hit_policies_select_every_fired_rule() -> None:
    decision = RuleSet([small, big], hit_policy="rule_order").evaluate({"total": 500})
    assert [e.status for e in decision.explain().rules] == ["selected", "selected"]


def test_why_not_an_unknown_rule() -> None:
    decision = RuleSet([small]).evaluate({"total": 1})
    with pytest.raises(RuleNotFoundError):
        decision.why_not("nope")


def test_other_logics_are_rejected_instead_of_misexplained() -> None:
    class Graded(BooleanLogic):
        __slots__ = ()

        def from_bool(self, value: bool | None) -> Any:
            return 0.5

        def is_true(self, value: Any) -> bool:
            return False

    decision = RuleSet([Rule("r", when=F.x > 1)], logic=Graded()).evaluate({"x": 5})
    with pytest.raises(UnsupportedOperation, match="explain"):
        decision.explain()


# --- Requirements: all / any -----------------------------------------------------------


def test_or_of_ands_needs_any_one_fix() -> None:
    entry = why(((F.a > 1) & (F.b > 1)) | (F.c > 1), {"a": 0, "b": 5, "c": 0})
    assert shape(entry.requirement) == ("any", ["F.a > 1", "F.c > 1"])


def test_and_needs_every_false_operand_fixed() -> None:
    entry = why((F.a > 1) & (F.b > 1) & (F.c > 1), {"a": 5, "b": 0, "c": 0})
    assert shape(entry.requirement) == ("all", ["F.b > 1", "F.c > 1"])


def test_true_and_needs_all_and_true_or_any() -> None:
    assert shape(why((F.a > 1) & (F.b > 1), {"a": 5, "b": 5}).requirement) == (
        "all",
        ["F.a > 1", "F.b > 1"],
    )
    assert shape(why((F.a > 1) | (F.b > 1) | (F.c > 1), {"a": 0, "b": 5, "c": 5}).requirement) == (
        "any",
        ["F.b > 1", "F.c > 1"],
    )


def test_false_or_needs_any_operand() -> None:
    entry = why((F.a > 1) | (F.b > 1), {"a": 0, "b": 0})
    assert shape(entry.requirement) == ("any", ["F.a > 1", "F.b > 1"])
    assert not any(c.holds for c in entry.causes)


def test_not_swaps_all_and_any() -> None:
    # not(a and b) is false when both hold: making either one false is enough.
    entry = why(~((F.a > 1) & (F.b > 1)), {"a": 5, "b": 5})
    assert shape(entry.requirement) == ("any", ["F.a > 1", "F.b > 1"])
    assert all(c.negated for c in entry.causes)
    assert str(entry.causes[0]) == (
        "F.a > 1 is true (a = 5), but the rule needs it false; needs a <= 1 (over by 4)"
    )


def test_not_between_needs_either_bound_broken() -> None:
    entry = why(~F.x.between(5, 10), {"x": 7})
    assert shape(entry.requirement) == ("any", ["F.x >= 5", "F.x <= 10"])


def test_repeated_conditions_are_merged_and_absorbed() -> None:
    entry = why((F.a > 1) & ((F.a > 1) | (F.b > 1)), {"a": 0, "b": 0})
    assert shape(entry.requirement) == "F.a > 1"


def test_false_and_also_lists_unknown_operands_that_must_become_true() -> None:
    entry = why((F.x > 1) & (F.y > 1) & (F.z > 1), {"x": 0, "z": 5})
    assert entry.status == "not_fired"
    assert shape(entry.requirement) == ("all", ["F.x > 1", "F.y > 1"])


def test_nested_groups_render_indented() -> None:
    entry = why(((F.a > 1) | (F.b > 1)) & (F.c > 1), {"a": 0, "b": 0, "c": 0})
    assert shape(entry.requirement) == ("all", [("any", ["F.a > 1", "F.b > 1"]), "F.c > 1"])
    assert str(entry).splitlines()[1:] == [
        "    would fire if all of these changed:",
        "    - any of:",
        "      - F.a > 1 is false (a = 0); short by 1",
        "      - F.b > 1 is false (b = 0); short by 1",
        "    - F.c > 1 is false (c = 0); short by 1",
    ]


def test_fixes_that_break_another_occurrence_are_not_offered() -> None:
    a = F.a > 1
    entry = why(a & (~a | (F.b > 1)), {"a": 5, "b": 0})
    assert entry.achievable
    assert shape(entry.requirement) == "F.b > 1"  # making a > 1 false would break the first a


def test_conflicting_requirements_are_flagged() -> None:
    a = F.a > 1
    entry = why(a & (~a | (F.b > 1)), {"a": 0, "b": 0})
    assert not entry.achievable
    assert "cannot fire by changing these conditions alone; they conflict:" in str(entry)
    assert not why(a & ~a, {"a": 5}).achievable


def test_a_condition_needed_both_ways_is_a_conflict() -> None:
    a = F.a > 1
    entry = why(a & ~a, {})  # unknown: a is missing, and the rule needs it true and false
    assert entry.status == "unknown"
    assert not entry.achievable


def _wide_and(n: int) -> Expr:
    return And(*(Or(F[f"a{i}"] > 1, F[f"b{i}"] > 1) for i in range(n)))


def test_wide_requirements_are_kept_without_checking() -> None:
    data = {f"{x}{i}": 0 for x in "ab" for i in range(8)}
    entry = why(_wide_and(8), data)  # 2**8 ways to fix it: too many to replay
    assert entry.achievable
    assert isinstance(entry.requirement, CauseGroup)
    assert len(entry.causes) == 16
    nested = why(Or(_wide_and(8), F.c > 1), {**data, "c": 0})
    assert nested.achievable
    many = why(Or(*(F[f"x{i}"] > 1 for i in range(70))), {f"x{i}": 0 for i in range(70)})
    assert many.achievable
    assert len(many.causes) == 70


def test_unknown_needs_the_unknown_operands() -> None:
    entry = why((F.a > 1) & (F.missing > 1), {"a": 5})
    assert entry.status == "unknown"
    assert texts(entry) == ["F.missing > 1 is unknown: missing field missing"]


def test_between_is_explained_by_the_failing_half() -> None:
    assert texts(why(F.x.between(1, 10), {"x": 20})) == ["F.x <= 10 is false (x = 20); over by 10"]


def test_field_used_as_a_condition_shows_its_value() -> None:
    assert texts(why(F.vip, {"vip": False})) == ["F.vip is false (vip = False)"]


def test_values_are_listed_once_in_reading_order() -> None:
    entry = why(F.a + F.b > F.a * 2, {"a": 1, "b": 0})
    assert entry.causes[0].values == ((("a",), 1), (("b",), 0))


def test_constant_conditions_have_no_values() -> None:
    entry = why(Const(False) & F.x.is_null(), {"x": 1})
    assert [c.values for c in entry.causes] == [(), ((("x",), 1),)]


# --- Counterfactuals -------------------------------------------------------------------


@pytest.mark.parametrize(
    ("condition", "data", "hint"),
    [
        (F.total > 1000, {"total": 800}, "short by 200"),
        (F.total >= 1000, {"total": 800.5}, "short by 199.5"),
        (F.total > 1000, {"total": 800.1}, "short by 199.9"),
        (F.total < 10, {"total": 25}, "over by 15"),
        (Compare("lt", Const(1000), F.total), {"total": 800}, "short by 200"),
        (F.d < dt.date(2026, 1, 1), {"d": dt.date(2026, 1, 31)}, "over by 30 days"),
        (F.d >= dt.date(2026, 1, 2), {"d": dt.date(2026, 1, 1)}, "short by 1 day"),
        (F.t < dt.datetime(2026, 1, 1), {"t": dt.datetime(2026, 1, 1, 6)}, "over by 6:00:00"),
        (F.amount > 10, {"amount": Decimal("7.5")}, "short by 2.5"),
        (F.amount > Decimal("10.00"), {"amount": Decimal("7.5")}, "short by 2.50"),
        (F.total > 1000, {"total": 1000}, "at the limit (needs total > 1000)"),
        (F.total < 10, {"total": 10}, "at the limit (needs total < 10)"),
    ],
    ids=[
        "gt", "ge-float", "float-noise", "lt", "flipped", "date", "date-singular",
        "datetime", "decimal-int", "decimal-decimal", "limit-gt", "limit-lt",
    ],
)  # fmt: skip
def test_ordering_hints_say_how_far_the_value_is(
    condition: Expr, data: dict[str, object], hint: str
) -> None:
    assert str(why(condition, data).causes[0].hint) == hint


def test_hints_are_structured() -> None:
    hint = why(~(F.total > 1000), {"total": 1300}).causes[0].hint
    assert hint == Counterfactual(("total",), "le", 1000, 300, negated=True)
    assert str(hint) == "needs total <= 1000 (over by 300)"


@pytest.mark.parametrize(
    ("condition", "data"),
    [
        (F.code == 5, {"code": 7}),
        (F.a > F.b, {"a": 1, "b": 2}),
        (F.x > 10, {"x": None}),
        (F.amount > 10.0, {"amount": Decimal("7.5")}),
        (F.d > dt.date(2026, 1, 1), {"d": dt.datetime(2025, 1, 1)}),
        (F.x.is_in([1, 2]), {"x": 5}),
        (F.x > float("inf"), {"x": float("inf")}),
        (F.x > 0, {"x": float("-inf")}),
        (F.x > Const(Decimal("Infinity")), {"x": Decimal("Infinity")}),
        (F.x < Const(Decimal("-Infinity")), {"x": Decimal("-Infinity")}),
        (F.x < Const(Decimal("-9E+999999")), {"x": Decimal("9E+999999")}),
    ],
    ids=[
        "equality", "two-fields", "unknown", "decimal-vs-float", "date-vs-datetime",
        "not-compare", "inf-inf", "minus-inf", "decimal-inf", "decimal-minus-inf",
        "decimal-overflow",
    ],
)  # fmt: skip
def test_no_hint_when_none_is_meaningful(condition: Expr, data: dict[str, object]) -> None:
    assert why(condition, data).causes[0].hint is None


def test_no_hint_for_conditions_that_hold() -> None:
    assert why(F.total > 10, {"total": 50}).causes[0].hint is None


def test_negated_equality_hint() -> None:
    assert str(why(~(F.code == 5), {"code": 5}).causes[0].hint) == "needs code != 5"


def test_hint_constants_render_like_the_expression() -> None:
    hint = why(~(F.x >= float("inf")), {"x": float("inf")}).causes[0].hint
    assert str(hint) == "needs x < float('inf')"


# --- Rendering -------------------------------------------------------------------------


class Opaque:
    pass


def test_values_render_bounded_and_without_addresses() -> None:
    cause = why(F.x.is_null(), {"x": Opaque()}).causes[0]
    assert str(cause) == "F.x.is_null() is false (x = <tests.core.test_explain.Opaque>)"
    long = why(F.x.is_null(), {"x": list(range(40))}).causes[0]
    assert "..." in str(long)
    assert len(str(long)) < 120


def test_odd_names_are_quoted_and_descriptions_indented() -> None:
    rule = Rule("weird name\nx", when=F.x > 1, description="line1\nline2 [x] fake")
    text = str(RuleSet([rule]).evaluate({"x": 0}).explain())
    assert "[ ] 'weird name\\nx' (not fired)" in text
    assert "\n    line1\n    line2 [x] fake\n" in text


def test_text_is_ascii_for_plain_data() -> None:
    text = str(RuleSet([small, big, vip]).evaluate({"total": 50, "vip": None}).explain())
    assert text.isascii()


def test_explanation_structure_and_lookup() -> None:
    decision = RuleSet([small, big]).evaluate({"total": 50})
    explanation = decision.explain()
    assert isinstance(explanation, Explanation)
    assert explanation.decision is decision
    assert explanation.for_rule("big").name == "big"
    assert isinstance(explanation.for_rule("big").requirement, Cause)
    group = why((F.a > 1) & (F.b > 1), {"a": 0, "b": 0}).requirement
    assert isinstance(group, CauseGroup)
    assert len(group.causes()) == 2


def test_explanation_snapshot_priority() -> None:
    rules = RuleSet(
        [
            Rule("small", when=F.total > 10, then={"discount": 0.05}, priority=1,
                 description="Any real order"),
            Rule("big", when=(F.total > 1000) & ~F.blocked, then={"discount": 0.10}, priority=5),
            Rule("vip", when=F.customer.years.between(5, 50) & F.country.is_in({"ES", "PT"}),
                 then={"discount": 0.20}, priority=5),
            Rule("gold", when=(F.points >= 500) | (F.tier == "gold"), then={"discount": 0.3}),
        ],
        hit_policy="priority",
    )  # fmt: skip
    data = {"total": 800, "blocked": False, "customer": {"years": 3}, "country": "FR", "tier": "x"}
    assert_snapshot("explain_priority", str(rules.evaluate(data).explain()))


def test_explanation_snapshot_first_with_fired_rules() -> None:
    rules = RuleSet(
        [
            Rule("adult", when=F.age >= 18),
            Rule("senior", when=F.age >= 65, then={"tier": "senior"}),
            Rule("not_blocked", when=~F.blocked),
            Rule("deadline", when=F.sent < dt.date(2026, 1, 1)),
            Rule("promo", when=((F.age < 25) & F.student) | (F.coupon == "X")),
        ]
    )
    data = {"age": 70, "blocked": True, "sent": dt.date(2026, 3, 1), "student": True, "coupon": "Y"}
    assert_snapshot("explain_first", str(rules.evaluate(data).explain()))


# --- Properties ------------------------------------------------------------------------

_K = BooleanLogic()


def _replay(result: NodeResult, forced: Mapping[object, Truth], *, linked: bool) -> Truth:
    """Re-combine a result tree with some conditions forced to other truth values.

    `linked`: forced values are the condition's own truth, keyed by structure, so
    every occurrence changes together (as when the data changes). Otherwise they are
    keyed by structure and polarity, treating occurrences as independent.
    """
    return _replay_at(result, forced, linked=linked, negated=False)


def _replay_at(
    result: NodeResult, forced: Mapping[object, Truth], *, linked: bool, negated: bool
) -> Truth:
    node = result.node
    if isinstance(node, Not):
        return _K.not_(_replay_at(result.children[0], forced, linked=linked, negated=not negated))
    if isinstance(node, And | Between):
        return _K.all_(
            [_replay_at(c, forced, linked=linked, negated=negated) for c in result.children]
        )
    if isinstance(node, Or):
        return _K.any_(
            [_replay_at(c, forced, linked=linked, negated=negated) for c in result.children]
        )
    key = node.structural_key() if linked else (node.structural_key(), negated)
    value = forced.get(key)
    return result.value if value is None else value


def _needed(cause: Cause, fired: bool) -> Truth:
    """The value to force: what the rule needs (to fire), or its opposite (to break)."""
    # The rule needs the leaf true unless it is negated; to break a fired rule, force
    # the opposite of what it needs.
    force_true = cause.negated is fired
    return Truth.TRUE if force_true else Truth.FALSE


def _options(requirement: Requirement, *, breaking: bool = False) -> list[list[Cause]]:
    """Every minimal set of causes to change: to fix a rule, one per `any` branch;
    to break a fired rule, the dual (all of an `any`, one of an `all`)."""
    if isinstance(requirement, Cause):
        return [[requirement]]
    one_of = (requirement.kind == "any") is not breaking
    if one_of:
        return [opt for item in requirement.items for opt in _options(item, breaking=breaking)]
    combos: list[list[Cause]] = [[]]
    for item in requirement.items:
        combos = [c + o for c in combos for o in _options(item, breaking=breaking)][:16]
    return combos


data_dicts = st.dictionaries(st.sampled_from(["a", "b", "x"]), st.integers(-3, 3) | st.none())


@given(st.lists(conditions, min_size=1, max_size=3), data_dicts)
def test_causes_point_the_right_way(whens: list[Expr], data: dict[str, object]) -> None:
    rules = RuleSet([Rule(f"r{i}", when=w) for i, w in enumerate(whens)], hit_policy="rule_order")
    for entry in rules.evaluate(data).explain().rules:
        assert entry.causes, "every outcome has at least one cause"
        if entry.status == "selected":
            assert all(c.holds for c in entry.causes)
        elif entry.status == "not_fired":
            assert not any(c.holds for c in entry.causes)
        else:
            assert entry.status == "unknown"
            assert all(c.result.value is Truth.UNKNOWN for c in entry.causes)


@given(st.lists(conditions, min_size=1, max_size=3), data_dicts)
def test_requirements_are_sufficient(whens: list[Expr], data: dict[str, object]) -> None:
    rules = RuleSet([Rule(f"r{i}", when=w) for i, w in enumerate(whens)], hit_policy="rule_order")
    for entry in rules.evaluate(data).explain().rules:
        fired = entry.status == "selected"
        if not entry.achievable:
            # Flagged as conflicting: indeed no option fixes the rule.
            for option in _options(entry.requirement):
                linked: dict[object, Truth] = {
                    c.result.node.structural_key(): _needed(c, False) for c in option
                }
                assert _replay(entry.result.tree, linked, linked=True) is not Truth.TRUE
            continue
        for option in _options(entry.requirement, breaking=fired):
            if fired:
                # Support is reported per occurrence: break each one independently.
                forced: dict[object, Truth] = {
                    (c.result.node.structural_key(), c.negated): _needed(c, fired) for c in option
                }
            else:
                # Fixes are promised for real data: occurrences change together.
                forced = {c.result.node.structural_key(): _needed(c, fired) for c in option}
            replayed = _replay(entry.result.tree, forced, linked=not fired)
            if fired:
                assert replayed is not Truth.TRUE, "breaking what it needs stops the rule"
            else:
                assert replayed is Truth.TRUE, "fixing any one option makes the rule fire"
