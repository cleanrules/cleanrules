"""Tasks 2.7-2.9: Rule, RuleSet with DMN-style hit policies, and the sequential engine."""

import copy
import pickle
from collections.abc import Iterator, Mapping
from decimal import Decimal
from pathlib import Path
from typing import Any

import pytest
from hypothesis import given
from hypothesis import strategies as st

from rulepy import (
    ActionContext,
    ActionError,
    Decision,
    EvaluationError,
    F,
    HitPolicy,
    HitPolicyError,
    MissingFieldError,
    Rule,
    RuleDefinitionError,
    RuleNotFoundError,
    RuleSet,
    TypeMismatchError,
)
from rulepy.core.evaluate import evaluate
from rulepy.core.expr import Const, Expr
from rulepy.core.logic import BooleanLogic, Truth
from rulepy.core.rule import FrozenMap
from rulepy.core.ruleset import Aggregate
from tests.strategies import conditions


def payload_values(rule: Rule) -> dict[str, object]:
    """Constant payload values of a rule, for readable assertions."""
    assert rule.then is not None
    return {k: v.value for k, v in rule.then.items() if isinstance(v, Const)}


# --- Rule ------------------------------------------------------------------------------


def test_rule_keeps_its_definition() -> None:
    rule = Rule(
        "vip",
        when=F.years > 5,
        then={"discount": 0.15},
        priority=10,
        tags=["pricing", "pricing"],
        description="Loyal customers",
        metadata={"owner": "sales"},
    )
    assert (rule.name, rule.priority, rule.tags) == ("vip", 10, frozenset({"pricing"}))
    assert payload_values(rule) == {"discount": 0.15}
    assert rule.do is None
    assert rule.metadata["owner"] == "sales"
    assert rule.description == "Loyal customers"


def test_payload_constants_are_normalized_and_expressions_kept() -> None:
    rule = Rule("r", when=F.x > 1, then={"rate": 0.1, "amount": F.total * 0.1})
    assert rule.then is not None
    assert isinstance(rule.then["rate"], Const)
    assert rule.then["amount"].same_as(F.total * 0.1)


def test_rule_mappings_are_frozen_copies() -> None:
    payload: dict[str, object] = {"a": 1}
    meta: dict[str, object] = {"k": "v"}
    rule = Rule("r", when=F.x > 1, then=payload, metadata=meta)
    payload["a"] = 2
    meta["k"] = "changed"
    assert payload_values(rule) == {"a": 1}
    assert rule.metadata["k"] == "v"
    with pytest.raises(TypeError):
        rule.metadata["k"] = "x"  # pyright: ignore[reportIndexIssue]


def test_frozen_map_basics() -> None:
    frozen = FrozenMap({"a": 1})
    assert (len(frozen), list(frozen), repr(frozen)) == (1, ["a"], "FrozenMap({'a': 1})")
    with pytest.raises(TypeError):
        hash(frozen)


def test_fields_can_be_conditions() -> None:
    assert Rule("flag", when=F.is_vip).when.tag == "field"


def test_rule_can_have_both_payload_and_action() -> None:
    def notify(ctx: ActionContext) -> None: ...

    rule = Rule("alert", when=F.x > 1, then={"review": True}, do=notify)
    assert rule.do is notify
    assert payload_values(rule) == {"review": True}


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        ({"name": "", "when": F.x > 1}, "name"),
        ({"name": 5, "when": F.x > 1}, "name"),
        ({"name": "r", "when": True}, "condition"),
        ({"name": "r", "when": F.x + 1}, "value"),
        ({"name": "r", "when": F.x > 1, "priority": True}, "priority"),
        ({"name": "r", "when": F.x > 1, "priority": 1.5}, "priority"),
        ({"name": "r", "when": F.x > 1, "then": [1]}, "then"),
        ({"name": "r", "when": F.x > 1, "then": print}, "then"),
        ({"name": "r", "when": F.x > 1, "then": {"ok": F.x > 1}}, "condition"),
        ({"name": "r", "when": F.x > 1, "then": {1: "a"}}, "keys"),
        ({"name": "r", "when": F.x > 1, "then": {"v": [F.x]}}, "constant"),
        ({"name": "r", "when": F.x > 1, "then": {"v": object()}}, "constant"),
        ({"name": "r", "when": F.x > 1, "then": {"v": float("nan")}}, "NaN"),
        ({"name": "r", "when": F.x > 1, "do": "notify"}, "callable"),
        ({"name": "r", "when": F.x > 1, "tags": "pricing"}, "tags"),
        ({"name": "r", "when": F.x > 1, "tags": [1]}, "tags"),
    ],
    ids=[
        "empty-name", "non-str-name", "bool-when", "value-when", "bool-priority",
        "float-priority", "list-then", "callable-then", "condition-payload", "int-key",
        "node-in-list", "object-payload", "nan-payload", "str-do", "str-tags", "int-tags",
    ],
)  # fmt: skip
def test_invalid_rules_are_rejected(kwargs: dict[str, Any], match: str) -> None:
    with pytest.raises(RuleDefinitionError, match=match):
        Rule(**kwargs)


def test_callable_then_points_to_do() -> None:
    with pytest.raises(RuleDefinitionError) as info:
        Rule("r", when=F.x > 1, then=print)  # pyright: ignore[reportArgumentType]
    assert "do=" in (info.value.hint or "")


def test_rules_compare_by_identity() -> None:
    a = Rule("vip", when=F.years > 5, then={"discount": 0.15})
    b = Rule("vip", when=F.years > 5, then={"discount": 0.15})
    assert a != b  # comparing structurally would compare `when` nodes
    assert len({a, b}) == 2


@pytest.mark.parametrize(
    ("rule", "text"),
    [
        (Rule("r", when=F.x > 1), "Rule('r', when=F.x > 1)"),
        (
            Rule("vip", when=F.years > 5, then={"discount": 0.15, "amt": F.t * 0.1}, priority=2),
            "Rule('vip', when=F.years > 5, then={'discount': 0.15, 'amt': F.t * 0.1}, priority=2)",
        ),
        (Rule("t", when=F.x > 1, tags=["b", "a"]), "Rule('t', when=F.x > 1, tags=['a', 'b'])"),
    ],
    ids=["minimal", "payload", "tags"],
)
def test_rule_repr(rule: Rule, text: str) -> None:
    assert repr(rule) == text


def test_rule_repr_names_the_action() -> None:
    def notify(ctx: ActionContext) -> None: ...

    assert repr(Rule("r", when=F.x > 1, do=notify)).endswith(
        "do=test_rule_repr_names_the_action.<locals>.notify)"
    )


def _module_level_action(ctx: ActionContext) -> None:
    pass


def test_rules_and_rule_sets_pickle_and_deepcopy() -> None:
    rule = Rule(
        "vip",
        when=(F.years > 5) & F.country.is_in({"ES"}),
        then={"discount": 0.15, "amount": F.total * 0.1},
        do=_module_level_action,
        tags=["x"],
        metadata={"owner": "sales"},
    )
    ruleset = RuleSet([rule], hit_policy="priority", strict=True)
    for clone in (pickle.loads(pickle.dumps(ruleset)), copy.deepcopy(ruleset)):
        cloned = clone["vip"]
        assert cloned.when.same_as(rule.when)
        assert cloned.then is not None
        assert cloned.then["amount"].same_as(F.total * 0.1)
        assert cloned.do is _module_level_action
        assert (cloned.tags, dict(cloned.metadata)) == (rule.tags, {"owner": "sales"})
        assert (clone.hit_policy, clone.strict) == (HitPolicy.PRIORITY, True)
    decision = ruleset.evaluate({"years": 7, "country": "ES", "total": 100})
    assert pickle.loads(pickle.dumps(decision)).output == {"discount": 0.15, "amount": 10.0}


# --- RuleSet construction --------------------------------------------------------------


def test_rule_names_must_be_unique() -> None:
    with pytest.raises(RuleDefinitionError, match="'a'") as info:
        RuleSet([Rule("a", when=F.x > 1), Rule("a", when=F.x > 2)])
    assert info.value.rule == "a"


def test_ruleset_is_a_read_only_sequence_with_lookup_by_name() -> None:
    a, b = Rule("a", when=F.x > 1), Rule("b", when=F.x > 2)
    rules = RuleSet([a, b])
    assert len(rules) == 2
    assert list(rules) == [a, b]
    assert rules["b"] is b
    assert rules[0] is a
    assert list(rules[1:]) == [b]
    assert rules.get("a") is a
    assert rules.get("zz") is None
    assert "a" in rules
    assert "zz" not in rules
    assert b in rules
    assert Rule("b", when=F.x > 2) not in rules  # identity, not name
    assert repr(rules) == "RuleSet(['a', 'b'], hit_policy='first')"


def test_unknown_rule_name_is_a_key_error_with_the_names() -> None:
    rules = RuleSet([Rule("a", when=F.x > 1)])
    with pytest.raises(KeyError) as info:
        _ = rules["c"]
    assert isinstance(info.value, RuleNotFoundError)
    assert "'a'" in (info.value.hint or "")


def test_ruleset_configuration_is_read_only() -> None:
    rules = RuleSet([Rule("a", when=F.x > 1)])
    for attribute in ("rules", "hit_policy", "aggregate", "logic", "strict"):
        with pytest.raises(AttributeError):
            setattr(rules, attribute, None)


@pytest.mark.parametrize("policy", ["first", "FIRST", "rule_order", HitPolicy.COLLECT])
def test_hit_policy_accepts_names(policy: str | HitPolicy) -> None:
    assert isinstance(RuleSet([], hit_policy=policy).hit_policy, HitPolicy)


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        ({"hit_policy": "best"}, "hit policy"),
        ({"hit_policy": "first", "aggregate": "sum"}, "COLLECT"),
        ({"hit_policy": "collect", "aggregate": "median"}, "aggregate"),
        ({"rules": [object()]}, "Rule"),
    ],
    ids=["unknown-policy", "aggregate-without-collect", "unknown-aggregate", "not-a-rule"],
)
def test_invalid_rulesets_are_rejected(kwargs: dict[str, Any], match: str) -> None:
    kwargs.setdefault("rules", [])
    with pytest.raises(RuleDefinitionError, match=match):
        RuleSet(**kwargs)


# --- Hit policies ----------------------------------------------------------------------

small = Rule("small", when=F.total > 10, then={"discount": 0.05, "label": "s"}, priority=1)
big = Rule("big", when=F.total > 100, then={"discount": 0.10, "label": "b"}, priority=5)
vip = Rule("vip", when=F.vip, then={"discount": 0.20, "label": "v"}, priority=5)
RULES = [small, big, vip]


def decide(policy: str, data: Mapping[str, object], **kwargs: Any) -> Decision[Truth]:
    return RuleSet(RULES, hit_policy=policy, **kwargs).evaluate(data)


def test_first_returns_the_first_firing_rule() -> None:
    decision = decide("first", {"total": 500, "vip": True})
    assert decision.output == {"discount": 0.05, "label": "s"}
    assert decision.outputs == ({"discount": 0.05, "label": "s"},)
    assert [r.rule.name for r in decision.selected] == ["small"]
    assert decision.fired == ("small", "big", "vip")


def test_no_rule_fires_gives_no_output() -> None:
    decision = decide("first", {"total": 1, "vip": False})
    assert decision.output is None
    assert decision.outputs == ()
    assert decision.selected == ()


def test_unknown_never_fires() -> None:
    decision = decide("rule_order", {"vip": None})  # total missing, vip null
    assert decision.outputs == ()
    assert [r.truth for r in decision.results] == [Truth.UNKNOWN] * 3


def test_priority_picks_the_highest_and_breaks_ties_by_order() -> None:
    decision = decide("priority", {"total": 500, "vip": True})
    assert [r.rule.name for r in decision.selected] == ["big"]


def test_priority_with_nothing_fired() -> None:
    assert decide("priority", {"total": 0, "vip": False}).output is None


def test_unique_allows_one_and_rejects_several() -> None:
    assert decide("unique", {"total": 50, "vip": False}).output == {"discount": 0.05, "label": "s"}
    with pytest.raises(HitPolicyError) as info:
        decide("unique", {"total": 500, "vip": False})
    assert info.value.rules == ("small", "big")


def test_any_requires_identical_outputs() -> None:
    same = [Rule("a", when=F.x > 0, then={"o": 1}), Rule("b", when=F.x > 1, then={"o": 1.0})]
    assert RuleSet(same, hit_policy="any").evaluate({"x": 5}).output == {"o": 1}
    with pytest.raises(HitPolicyError, match="different outputs") as info:
        decide("any", {"total": 500, "vip": False})
    assert info.value.rules == ("small", "big")


@pytest.mark.parametrize(
    ("first", "second"), [(1, True), ("1", 1), (1, 2)], ids=["int-bool", "str-int", "values"]
)
def test_any_equality_uses_type_groups(first: object, second: object) -> None:
    rules = [
        Rule("a", when=F.x > 0, then={"o": first}),
        Rule("b", when=F.x > 0, then={"o": second}),
    ]
    with pytest.raises(HitPolicyError):
        RuleSet(rules, hit_policy="any").evaluate({"x": 1})


def test_any_compares_payloads_with_expressions_safely() -> None:
    rules = [
        Rule("a", when=F.x > 0, then={"v": F.x * 2}),
        Rule("b", when=F.x > 0, then={"v": F.x + F.x}),
    ]
    assert RuleSet(rules, hit_policy="any").evaluate({"x": 3}).output == {"v": 6}


def test_rule_order_and_collect_list_every_fired_output() -> None:
    data = {"total": 500, "vip": True}
    expected = (
        {"discount": 0.05, "label": "s"},
        {"discount": 0.10, "label": "b"},
        {"discount": 0.20, "label": "v"},
    )
    assert decide("rule_order", data).outputs == expected
    assert decide("collect", data).outputs == expected


@pytest.mark.parametrize("policy", ["rule_order", "collect"])
def test_multi_hit_output_points_to_outputs(policy: str) -> None:
    with pytest.raises(HitPolicyError) as info:
        _ = decide(policy, {"total": 500}).output
    assert "outputs" in (info.value.hint or "")


@pytest.mark.parametrize(
    ("aggregate", "expected"),
    [
        ("sum", {"discount": pytest.approx(0.35)}),
        ("min", {"discount": 0.05}),
        ("max", {"discount": 0.20}),
        ("count", {"discount": 3}),
    ],
)
def test_collect_aggregates_per_key(aggregate: Aggregate, expected: object) -> None:
    rules = [
        Rule(r.name, when=r.when, then={"discount": payload_values(r)["discount"]}) for r in RULES
    ]
    decision = RuleSet(rules, hit_policy="collect", aggregate=aggregate).evaluate(
        {"total": 500, "vip": True}
    )
    assert decision.output == expected


def test_collect_aggregate_of_nothing_is_null() -> None:
    assert (
        RuleSet([small], hit_policy="collect", aggregate="sum").evaluate({"total": 0}).output
        is None
    )
    assert RuleSet([small], hit_policy="collect", aggregate="count").evaluate({}).output is None


def test_collect_aggregates_skip_nulls() -> None:
    rules = [
        Rule("a", when=F.x > 0, then={"v": F.missing}),
        Rule("b", when=F.x > 0, then={"v": 2}),
        Rule("c", when=F.x > 0, then={"w": F.missing}),
    ]
    summed = RuleSet(rules, hit_policy="collect", aggregate="sum").evaluate({"x": 1})
    assert summed.output == {"v": 2, "w": None}
    counted = RuleSet(rules, hit_policy="collect", aggregate="count").evaluate({"x": 1})
    assert counted.output == {"v": 1, "w": 0}


def test_collect_sum_mixes_decimal_and_float() -> None:
    rules = [
        Rule("a", when=F.x > 0, then={"v": F.d}),
        Rule("b", when=F.x > 0, then={"v": 0.5}),
    ]
    decision = RuleSet(rules, hit_policy="collect", aggregate="sum").evaluate(
        {"x": 1, "d": Decimal("1.5")}
    )
    assert decision.output == {"v": 2.0}


def test_collect_aggregate_rejects_non_numbers() -> None:
    with pytest.raises(HitPolicyError, match="label"):
        decide("collect", {"total": 500, "vip": True}, aggregate="sum")


# --- Payloads and results --------------------------------------------------------------


def test_payload_expressions_are_computed_from_the_data() -> None:
    rule = Rule("pct", when=F.total > 0, then={"discount": F.total * 0.1, "who": F.name})
    decision = RuleSet([rule]).evaluate({"total": 200, "name": "Ana"})
    assert decision.output == {"discount": 20.0, "who": "Ana"}


def test_rules_without_payload_output_an_empty_mapping() -> None:
    assert RuleSet([Rule("r", when=F.x > 0)]).evaluate({"x": 1}).output == {}


def test_null_payload_values_keep_their_reason() -> None:
    decision = RuleSet([Rule("r", when=F.x > 0, then={"v": F.rate * 2})]).evaluate({"x": 1})
    result = decision.selected[0]
    assert result.output == {"v": None}
    assert str(result.output_trees["v"].reason) == "missing field rate"


def test_payloads_are_evaluated_only_for_selected_rules() -> None:
    discarded = Rule("b", when=F.x > 0, then={"v": F.missing})
    rules = RuleSet([Rule("a", when=F.x > 0, then={"v": 1}), discarded], strict=True)
    decision = rules.evaluate({"x": 1})  # FIRST picks "a"; "b" fired but is discarded
    assert decision.output == {"v": 1}
    assert decision.results[1].fired
    assert decision.results[1].output is None


def test_every_rule_result_is_kept_with_its_tree() -> None:
    decision = decide("first", {"total": 50, "vip": None})
    assert [(r.rule.name, r.truth, r.fired) for r in decision.results] == [
        ("small", Truth.TRUE, True),
        ("big", Truth.FALSE, False),
        ("vip", Truth.UNKNOWN, False),
    ]
    assert decision.results[2].tree.reason is not None
    assert decision.results[0].output == {"discount": 0.05, "label": "s"}
    assert decision.results[1].output is None
    assert decision.fired == ("small",)


def test_strict_mode_names_the_rule() -> None:
    with pytest.raises(MissingFieldError) as info:
        RuleSet([small], strict=True).evaluate({})
    assert info.value.rule == "small"


# --- Actions ---------------------------------------------------------------------------


def test_evaluate_never_runs_actions() -> None:
    def boom(ctx: ActionContext) -> None:
        raise AssertionError("must not run")

    rules = RuleSet([Rule("a", when=F.x > 0, do=boom)])
    rules.evaluate({"x": 1})
    list(rules.evaluate_many([{"x": 1}]))


def test_run_calls_actions_of_selected_rules_in_order() -> None:
    calls: list[tuple[str, object, Mapping[str, object]]] = []

    def record(ctx: ActionContext) -> None:
        calls.append((ctx.rule.name, ctx.data, ctx.output))

    rules = [
        Rule("a", when=F.x > 0, then={"n": 1}, do=record),
        Rule("b", when=F.x > 1, do=record),
        Rule("c", when=F.x > 1),
    ]
    RuleSet(rules).run({"x": 5})  # FIRST: only "a"
    RuleSet(rules, hit_policy="rule_order").run({"x": 5})
    assert calls == [("a", {"x": 5}, {"n": 1}), ("a", {"x": 5}, {"n": 1}), ("b", {"x": 5}, {})]


def test_action_failures_carry_the_decision_and_what_already_ran() -> None:
    ran: list[str] = []

    def ok(ctx: ActionContext) -> None:
        ran.append(ctx.rule.name)

    def boom(ctx: ActionContext) -> None:
        raise ValueError("smtp down")

    rules = [
        Rule("first", when=F.x > 0, do=ok),
        Rule("notify", when=F.x > 0, do=boom),
        Rule("later", when=F.x > 0, do=ok),
    ]
    with pytest.raises(ActionError, match="smtp down") as info:
        RuleSet(rules, hit_policy="rule_order").run({"x": 1})
    err = info.value
    assert (err.rule, err.executed) == ("notify", ("first",))
    assert isinstance(err.decision, Decision)
    assert isinstance(err.__cause__, ValueError)
    assert ran == ["first"]  # actions stop at the first failure


# --- Batches ---------------------------------------------------------------------------


def test_evaluate_many_is_lazy() -> None:
    rules = RuleSet([small])
    seen: list[int] = []

    def rows() -> Iterator[dict[str, int]]:
        for total in (5, 50):
            seen.append(total)
            yield {"total": total}

    decisions = rules.evaluate_many(rows())
    assert seen == []
    assert [d.fired for d in decisions] == [(), ("small",)]


def test_evaluate_many_errors_name_the_row() -> None:
    rows = [{"total": 5}, {}]
    with pytest.raises(MissingFieldError) as info:
        list(RuleSet([small], strict=True).evaluate_many(rows))
    assert info.value.row == 1
    assert str(info.value).startswith("row 1: rule 'small': missing field total")


# --- README ----------------------------------------------------------------------------


def test_readme_example_runs_verbatim() -> None:
    readme = (Path(__file__).parents[2] / "README.md").read_text(encoding="utf-8")
    code = readme.split("```python\n", 1)[1].split("```", 1)[0]
    namespace: dict[str, Any] = {}
    exec(code, namespace)  # our own README
    assert namespace["decision"].output == {"discount": 0.15}


# --- Properties ------------------------------------------------------------------------

data_dicts = st.dictionaries(st.sampled_from(["a", "b", "x"]), st.integers(-3, 3), max_size=3)


@given(st.lists(conditions, min_size=1, max_size=4), data_dicts)
def test_first_policy_agrees_with_the_evaluator(whens: list[Expr], data: dict[str, int]) -> None:
    rules = [Rule(f"r{i}", when=w, then={"i": i}) for i, w in enumerate(whens)]
    decision = RuleSet(rules).evaluate(data)
    firing = [i for i, w in enumerate(whens) if evaluate(w, data) is Truth.TRUE]
    assert decision.output == ({"i": firing[0]} if firing else None)
    assert decision.fired == tuple(f"r{i}" for i in firing)
    assert [r.rule.name for r in decision.selected] == [f"r{i}" for i in firing[:1]]


@given(st.lists(conditions, min_size=1, max_size=4), data_dicts)
def test_evaluate_is_pure_and_repeatable(whens: list[Expr], data: dict[str, int]) -> None:
    rules = RuleSet(
        [Rule(f"r{i}", when=w, then={"i": i}) for i, w in enumerate(whens)], hit_policy="collect"
    )
    assert rules.evaluate(data).outputs == rules.evaluate(data).outputs


# --- Verification round (multi-lens review of 2.7-2.9) ---------------------------------


def test_any_compares_non_scalar_payload_values_without_hashing() -> None:
    same = [
        Rule("a", when=F.x > 0, then={"v": F.items}),
        Rule("b", when=F.x > 0, then={"v": F.items}),
    ]
    assert RuleSet(same, hit_policy="any").evaluate({"x": 1, "items": [1, 2]}).output == {
        "v": [1, 2]
    }
    mixed = [Rule("a", when=F.x > 0, then={"v": F.items}), Rule("b", when=F.x > 0, then={"v": 2})]
    with pytest.raises(HitPolicyError):
        RuleSet(mixed, hit_policy="any").evaluate({"x": 1, "items": [1, 2]})
    rows = [{"x": 1, "items": {"k": 1}}]
    assert [d.output for d in RuleSet(same, hit_policy="any").evaluate_many(rows)] == [
        {"v": {"k": 1}}
    ]


def test_any_null_payload_values_are_equal_and_differ_from_values() -> None:
    nulls = [
        Rule("a", when=F.x > 0, then={"v": F.nope}),
        Rule("b", when=F.x > 0, then={"v": F.gone}),
    ]
    assert RuleSet(nulls, hit_policy="any").evaluate({"x": 1}).output == {"v": None}
    with pytest.raises(HitPolicyError):
        RuleSet(
            [Rule("a", when=F.x > 0, then={"v": F.nope}), Rule("b", when=F.x > 0, then={"v": 1})],
            hit_policy="any",
        ).evaluate({"x": 1})


def test_any_rejects_different_key_sets() -> None:
    rules = [Rule("a", when=F.x > 0, then={"v": 1}), Rule("b", when=F.x > 0, then={"w": 1})]
    with pytest.raises(HitPolicyError) as info:
        RuleSet(rules, hit_policy="any").evaluate({"x": 1})
    assert info.value.rules == ("a", "b")


def test_strict_any_evaluates_the_payload_of_every_fired_rule() -> None:
    rules = [Rule("a", when=F.x > 0, then={"v": 1}), Rule("b", when=F.x > 0, then={"v": F.nope})]
    with pytest.raises(MissingFieldError) as info:
        RuleSet(rules, hit_policy="any", strict=True).evaluate({"x": 1})
    assert info.value.rule == "b"


def test_strict_mode_raises_on_the_selected_payload() -> None:
    rules = RuleSet([Rule("r", when=F.x > 0, then={"ok": 1, "v": F.nope})], strict=True)
    with pytest.raises(MissingFieldError) as info:
        rules.evaluate({"x": 1})
    assert (info.value.rule, info.value.path) == ("r", ("nope",))


def test_strict_order_conditions_then_payloads_then_policy() -> None:
    conditions_first = [
        Rule("a", when=F.x > 0, then={"v": F.nope}),
        Rule("b", when=F.missing > 0),
    ]
    with pytest.raises(MissingFieldError) as info:
        RuleSet(conditions_first, hit_policy="rule_order", strict=True).evaluate({"x": 1})
    assert info.value.path == ("missing",)  # rule b's condition, before rule a's payload
    payload_before_policy = [
        Rule("a", when=F.x > 0, then={"v": F.nope}),
        Rule("b", when=F.x > 0, then={"v": 1}),
    ]
    with pytest.raises(MissingFieldError):
        RuleSet(payload_before_policy, hit_policy="any", strict=True).evaluate({"x": 1})


def test_unique_raises_before_evaluating_payloads() -> None:
    rules = [Rule("a", when=F.x > 0, then={"v": F.nope}), Rule("b", when=F.x > 0)]
    with pytest.raises(HitPolicyError):
        RuleSet(rules, hit_policy="unique", strict=True).evaluate({"x": 1})


def test_priority_ignores_rules_that_did_not_fire_and_allows_negatives() -> None:
    lo = Rule("lo", when=F.x > 0, then={"v": "lo"}, priority=-5)
    neg = Rule("neg", when=F.x > 0, then={"v": "neg"}, priority=-9)
    hi = Rule("hi", when=F.x > 100, then={"v": "hi"}, priority=9)
    decision = RuleSet([neg, lo, hi], hit_policy="priority").evaluate({"x": 5})
    assert decision.output == {"v": "lo"}


def test_payload_keys_keep_definition_order() -> None:
    rule = Rule("r", when=F.x > 0, then={"b": 1, "a": 2, "c": F.x})
    output = RuleSet([rule]).evaluate({"x": 3}).output
    assert output is not None
    assert list(output) == ["b", "a", "c"]


@pytest.mark.parametrize("aggregate", ["sum", "min", "max"])
def test_collect_aggregates_reject_booleans(aggregate: Aggregate) -> None:
    rules = [Rule("a", when=F.x > 0, then={"v": True}), Rule("b", when=F.x > 0, then={"v": False})]
    with pytest.raises(HitPolicyError, match="non-numeric"):
        RuleSet(rules, hit_policy="collect", aggregate=aggregate).evaluate({"x": 1})


def test_collect_count_accepts_any_type() -> None:
    rules = [Rule("a", when=F.x > 0, then={"v": "s"}), Rule("b", when=F.x > 0, then={"v": True})]
    assert RuleSet(rules, hit_policy="collect", aggregate="count").evaluate({"x": 1}).output == {
        "v": 2
    }


@pytest.mark.parametrize(
    ("values", "expected"),
    [
        ([2**63 - 1, 2**63 - 1], None),
        ([float("inf"), float("-inf")], None),
        ([Decimal("1.1"), 1], pytest.approx(2.1)),
        ([Decimal("1E+999999"), Decimal("1E+999999")], float("inf")),
    ],
    ids=["int-overflow", "nan", "decimal-is-float", "decimal-huge"],
)
def test_collect_sum_follows_arithmetic_rules(values: list[object], expected: object) -> None:
    rules = [Rule(f"r{i}", when=F.x > 0, then={"v": F[f"v{i}"]}) for i in range(len(values))]
    data: dict[str, object] = {"x": 1, **{f"v{i}": v for i, v in enumerate(values)}}
    output = RuleSet(rules, hit_policy="collect", aggregate="sum").evaluate(data).output
    assert output == {"v": expected}


def test_decisions_are_read_only_even_from_actions() -> None:
    def tamper(ctx: ActionContext) -> None:
        ctx.output["discount"] = 99  # pyright: ignore[reportIndexIssue]

    rules = RuleSet([Rule("r", when=F.x > 0, then={"discount": 0.1}, do=tamper)])
    with pytest.raises(ActionError) as info:
        rules.run({"x": 1})
    decision = info.value.decision
    assert isinstance(decision, Decision)
    assert decision.output == {"discount": 0.1}
    agg = RuleSet([Rule("r", when=F.x > 0, then={"v": 1})], hit_policy="collect", aggregate="sum")
    aggregated = agg.evaluate({"x": 1}).output
    assert aggregated is not None
    with pytest.raises(TypeError):
        aggregated["v"] = 2  # pyright: ignore[reportIndexIssue]


def test_run_returns_the_decision_and_actions_receive_the_tree() -> None:
    seen: list[ActionContext] = []
    rule = Rule("a", when=F.x > 0, then={"n": 1}, do=seen.append)
    decision = RuleSet([rule]).run({"x": 5})
    assert isinstance(decision, Decision)
    assert decision.output == {"n": 1}
    assert seen[0].tree.value is Truth.TRUE
    assert seen[0].tree.node.same_as(rule.when)


def test_executed_lists_only_actions_that_ran() -> None:
    def boom(ctx: ActionContext) -> None:
        raise ValueError("x")

    rules = [
        Rule("first", when=F.x > 0, do=lambda ctx: None),
        Rule("plain", when=F.x > 0),
        Rule("notify", when=F.x > 0, do=boom),
    ]
    with pytest.raises(ActionError) as info:
        RuleSet(rules, hit_policy="rule_order").run({"x": 1})
    assert info.value.executed == ("first",)


@pytest.mark.parametrize(
    ("rules", "rows", "error"),
    [
        (
            [Rule("a", when=F.x > 0), Rule("b", when=F.x > 0)],
            [{"x": -1}, {"x": 1}],
            HitPolicyError,
        ),
        ([Rule("a", when=F.x > 0)], [{"x": 1}, {"x": "s"}], TypeMismatchError),
    ],
    ids=["hit-policy", "type-mismatch"],
)
def test_evaluate_many_rows_for_every_evaluation_error(
    rules: list[Rule], rows: list[dict[str, object]], error: type[Exception]
) -> None:
    with pytest.raises(error) as info:
        list(RuleSet(rules, hit_policy="unique", strict=True).evaluate_many(rows))
    assert getattr(info.value, "row", None) == 1


@pytest.mark.parametrize("rows", ["abc", b"abc", {"x": 1}], ids=["str", "bytes", "mapping"])
def test_evaluate_many_rejects_non_row_iterables_immediately(rows: object) -> None:
    with pytest.raises(EvaluationError, match="iterable of inputs"):
        RuleSet([small]).evaluate_many(rows)  # pyright: ignore[reportArgumentType]


def test_evaluate_many_rejects_dataframes() -> None:
    pd = pytest.importorskip("pandas")
    with pytest.raises(EvaluationError, match="column labels") as info:
        RuleSet([small]).evaluate_many(pd.DataFrame({"total": [1]}))
    assert "to_dict" in (info.value.hint or "")


def test_evaluate_many_rejects_dataframe_lookalikes() -> None:
    class DataFrame:  # stand-in detected by module name and `columns`
        __module__ = "pandas.core.frame"
        columns = ("total",)

        def __iter__(self) -> Iterator[str]:
            return iter(self.columns)

    with pytest.raises(EvaluationError, match="column labels"):
        RuleSet([small]).evaluate_many(DataFrame())


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        ({"tags": 5}, "tags"),
        ({"metadata": [1]}, "metadata"),
        ({"metadata": {1: "a"}}, "metadata keys"),
        ({"description": 5}, "description"),
        ({"priority": 2**70}, "64 bits"),
    ],
    ids=[
        "tags-not-iterable",
        "metadata-not-mapping",
        "metadata-int-key",
        "description",
        "huge-priority",
    ],
)
def test_more_invalid_rule_arguments(kwargs: dict[str, Any], match: str) -> None:
    with pytest.raises(RuleDefinitionError, match=match):
        Rule("r", when=F.x > 1, **kwargs)


@pytest.mark.parametrize(
    ("kwargs", "match"),
    [
        ({"rules": 5}, "iterable"),
        ({"rules": [], "hit_policy": "collect", "aggregate": ["sum"]}, "aggregate"),
        ({"rules": [], "logic": object()}, "logic"),
        ({"rules": [], "strict": "no"}, "strict"),
    ],
    ids=["rules-not-iterable", "unhashable-aggregate", "non-logic", "str-strict"],
)
def test_more_invalid_ruleset_arguments(kwargs: dict[str, Any], match: str) -> None:
    with pytest.raises(RuleDefinitionError, match=match):
        RuleSet(**kwargs)


class _FiresOnUnknown(BooleanLogic):
    """A logic whose activation test also accepts UNKNOWN."""

    __slots__ = ()

    def is_true(self, value: Truth) -> bool:
        return value is not Truth.FALSE


def test_rule_set_uses_its_logic_for_the_activation_test() -> None:
    logic = _FiresOnUnknown()
    rules = RuleSet([Rule("a", when=F.missing > 1, then={"v": 1})], logic=logic)
    assert rules.logic is logic
    decision = rules.evaluate({})
    assert decision.results[0].truth is Truth.UNKNOWN
    assert decision.fired == ("a",)
    assert decision.output == {"v": 1}


def test_aggregate_survives_pickling() -> None:
    agg = RuleSet([Rule("a", when=F.x > 0, then={"v": F.x})], hit_policy="collect", aggregate="sum")
    for clone in (pickle.loads(pickle.dumps(agg)), copy.deepcopy(agg)):
        assert (clone.hit_policy, clone.aggregate) == (HitPolicy.COLLECT, "sum")
        assert clone.evaluate({"x": 3}).output == {"v": 3}


def test_rule_pickles_through_its_constructor() -> None:
    rule = Rule("r", when=F.x > 1, then={"v": 1}, priority=-2, description="d")
    reduced = rule.__reduce__()
    assert reduced[0].__name__ == "_rebuild_rule"
    clone = pickle.loads(pickle.dumps(rule))
    assert (clone.priority, clone.description) == (-2, "d")


def test_small_api_details() -> None:
    a = Rule("a", when=F.x > 1)
    rules = RuleSet([a])
    assert rules.get("zz", a) is a
    assert repr(Rule("n", when=F.x > 1, priority=-3)) == "Rule('n', when=F.x > 1, priority=-3)"


def test_any_with_nothing_fired_has_no_output() -> None:
    rules = [Rule("a", when=F.x > 5, then={"v": 1}), Rule("b", when=F.x > 5, then={"v": 2})]
    assert RuleSet(rules, hit_policy="any").evaluate({"x": 1}).output is None


class _Incomparable:
    def __eq__(self, other: object) -> bool:
        raise RuntimeError("cannot compare")

    __hash__ = object.__hash__


def test_any_treats_incomparable_values_as_different() -> None:
    rules = [Rule("a", when=F.x > 0, then={"v": F.o}), Rule("b", when=F.x > 0, then={"v": F.o})]
    with pytest.raises(HitPolicyError):
        RuleSet(rules, hit_policy="any").evaluate({"x": 1, "o": _Incomparable()})
