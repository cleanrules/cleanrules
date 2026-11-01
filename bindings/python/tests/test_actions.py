"""Actions, `run`, and pickling of rules and rule sets (task C.4)."""

import copy
import pickle
from decimal import Decimal
from typing import Any

import pytest
from cleanrules import (
    ActionContext,
    ActionError,
    F,
    Rule,
    RuleDefinitionError,
    RuleSet,
    Truth,
)

calls: list[tuple[str, object]] = []


def record(context: ActionContext) -> None:
    calls.append((context.rule.name, dict(context.output)))


def fail(context: ActionContext) -> None:
    raise ValueError(f"cannot handle {context.rule.name}")


def shop(do_vip: Any = record, do_loyal: Any = record, **options: Any) -> RuleSet:
    return RuleSet(
        [
            Rule("vip", F.years > 5, {"discount": Decimal("0.15")}, do=do_vip),
            Rule("loyal", F.years > 2, {"discount": Decimal("0.05")}, do=do_loyal),
            Rule("quiet", F.years > 0),
        ],
        **options,
    )


@pytest.fixture(autouse=True)
def _clear() -> None:
    calls.clear()


def test_evaluate_never_runs_actions() -> None:
    shop().evaluate({"years": 9})
    assert calls == []


def test_run_calls_the_actions_of_the_selected_rules_in_order() -> None:
    decision = shop(hit_policy="rule_order").run({"years": 9})
    assert decision.fired == ("vip", "loyal", "quiet")
    assert calls == [
        ("vip", {"discount": Decimal("0.15")}),
        ("loyal", {"discount": Decimal("0.05")}),
    ]
    calls.clear()
    shop().run({"years": 9})  # FIRST selects only vip
    assert calls == [("vip", {"discount": Decimal("0.15")})]


def test_actions_receive_their_context() -> None:
    seen: list[ActionContext] = []
    data = {"years": 3}
    decision = shop(do_loyal=seen.append).run(data)
    (context,) = seen
    assert context.rule is decision.ruleset["loyal"]
    assert context.data is data
    assert context.result is decision.selected[0]
    assert context.result.truth is Truth.TRUE
    assert context.decision is decision


def test_a_failing_action_stops_the_run() -> None:
    with pytest.raises(ActionError) as info:
        shop(do_vip=record, do_loyal=fail, hit_policy="rule_order").run({"years": 9})
    error = info.value
    assert error.rule == "loyal"
    assert error.executed == ("vip",)
    assert isinstance(error.__cause__, ValueError)
    assert error.decision is not None
    assert error.decision.fired == ("vip", "loyal", "quiet")
    assert calls == [("vip", {"discount": Decimal("0.15")})]
    clone = pickle.loads(pickle.dumps(error))  # the decision is not pickled
    assert (clone.rule, clone.executed, clone.decision) == ("loyal", ("vip",), None)


def test_actions_must_be_callable() -> None:
    with pytest.raises(RuleDefinitionError, match="callable"):
        Rule("r", F.x > 1, do="print")  # type: ignore[arg-type]


def test_rules_and_rule_sets_pickle_and_copy() -> None:
    rules = shop(hit_policy="Collect", aggregate="sum", strict=False, do_vip=record, do_loyal=None)
    rule = Rule(
        "r",
        F.x.between(1, 5) & F.s.matches("^a"),
        {"k": F.x * 2},
        priority=3,
        tags=["t"],
        description="d",
        metadata={"owner": "risk", "n": [1, 2]},
        do=record,
    )
    for clone in (pickle.loads(pickle.dumps(rule)), copy.deepcopy(rule), copy.copy(rule)):
        assert clone.name == rule.name
        assert clone.when.same_as(rule.when)
        assert clone.then["k"].same_as(rule.then["k"])
        assert (clone.priority, clone.tags, clone.description) == (3, ("t",), "d")
        assert clone.metadata == {"owner": "risk", "n": [1, 2]}
        assert clone.do is record
    restored = pickle.loads(pickle.dumps(rules))
    assert [r.name for r in restored] == ["vip", "loyal", "quiet"]
    assert (restored.hit_policy, restored.aggregate, restored.strict) == (
        rules.hit_policy,
        rules.aggregate,
        rules.strict,
    )
    data = {"years": 9}
    assert restored.evaluate(data).output == rules.evaluate(data).output
    restored.run(data)
    assert calls == [("vip", {"discount": Decimal("0.15")})]


def test_rules_with_lambdas_do_not_pickle() -> None:
    rule = Rule("r", F.x > 1, do=lambda context: None)
    with pytest.raises((pickle.PicklingError, AttributeError, TypeError)):
        pickle.dumps(rule)


def test_actions_receive_the_condition_tree() -> None:
    seen: list[ActionContext] = []
    shop(do_loyal=seen.append).run({"years": 3})
    tree = seen[0].tree
    assert tree.truth is Truth.TRUE
    assert tree.node.same_as(F.years > 2)
    assert tree.children[0].value == 3


def test_action_errors_name_the_rule_once() -> None:
    with pytest.raises(ActionError) as info:
        shop(do_vip=fail).run({"years": 9})
    assert str(info.value).count("vip") == 2  # the rule, and the action's own message
    assert str(info.value).startswith("rule 'vip': the action raised ValueError")
