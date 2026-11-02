"""Actions, `run`, and pickling of rules and rule sets (task C.4); named actions (D.4)."""

import copy
import pickle
from decimal import Decimal
from typing import Any

import pytest
from cleanrules import (
    ActionContext,
    ActionError,
    F,
    MissingFieldError,
    RegistryError,
    Rule,
    RuleDefinitionError,
    RuleSet,
    Truth,
    UnsupportedError,
    dumps,
    load,
    loads,
    rule_from_text,
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


def test_actions_must_be_callable_or_named() -> None:
    with pytest.raises(RuleDefinitionError, match="callable"):
        Rule("r", F.x > 1, do=5)  # type: ignore[arg-type]
    # A string is a name for a registry, never something to import.
    named = Rule("r", F.x > 1, do="print")
    assert (named.action_name, named.do) == ("print", None)
    assert repr(named) == "Rule('r', F.x > 1, do='print')"
    with pytest.raises(RuleDefinitionError, match="empty"):
        Rule("r", F.x > 1, do="")


TEXT = """HIT POLICY rule_order
RULE vip IF years > 5 THEN discount = 0.15 DO notify
RULE loyal IF years > 2 THEN discount = 0.05
RULE audit IF years > 0 DO log"""


def test_named_actions_are_data() -> None:
    rules = loads(TEXT, format="text")
    assert isinstance(rules, RuleSet)
    assert [(r.action_name, r.do) for r in rules] == [("notify", None), (None, None), ("log", None)]
    assert dumps(rules, format="text") == (
        "HIT POLICY rule_order\n\n"
        "RULE vip\nIF years > 5\nTHEN discount = 0.15\nDO notify\n\n"
        "RULE loyal\nIF years > 2\nTHEN discount = 0.05\n\n"
        "RULE audit\nIF years > 0\nDO log"
    )
    document = dumps(rules)
    assert '"do":"notify"' in document
    assert dumps(loads(document), format="text") == dumps(rules, format="text")
    # Unbound, the rules still evaluate and summarize.
    assert rules.evaluate({"years": 9}).fired == ("vip", "loyal", "audit")
    summary = rules.summary()
    assert [(r.action, r.action_name) for r in summary.rules] == [
        (True, "notify"),
        (False, None),
        (True, "log"),
    ]
    assert "discount DO notify" in str(summary)


def test_bind_resolves_names_in_a_registry() -> None:
    rules = loads(TEXT, format="text")
    assert isinstance(rules, RuleSet)
    bound = rules.bind({"notify": record, "log": record, "unused": fail})
    assert bound is not rules and rules["vip"].do is None
    assert [r.do for r in bound] == [record, None, record]
    bound.run({"years": 9})
    assert calls == [("vip", {"discount": Decimal("0.15")}), ("audit", {})]
    # Every named action is checked, whether or not its rule fires.
    with pytest.raises(RegistryError) as info:
        rules.bind({"notify": record})
    error = info.value
    assert (error.name, error.rule, error.available) == ("log", "audit", ("notify",))
    assert str(error).startswith('the action "log" of rule "audit" is not registered')
    with pytest.raises(TypeError, match="not callable"):
        rules.bind({"notify": record, "log": "record"})  # type: ignore[dict-item]
    with pytest.raises(TypeError, match="names are str"):
        rules.bind({"notify": record, 1: record})  # type: ignore[dict-item]
    with pytest.raises(TypeError, match="mapping"):
        rules.bind([("notify", record)])  # type: ignore[arg-type]
    # Code actions are kept; rebinding replaces named ones.
    mixed = RuleSet([Rule("a", F.x > 1, do=fail), Rule("b", F.x > 1, do="log")])
    rebound = mixed.bind({"log": record}).bind({"log": fail})
    assert [r.do for r in rebound] == [fail, fail]


def test_run_refuses_unbound_names_before_evaluating() -> None:
    rules = loads(TEXT, format="text")
    assert isinstance(rules, RuleSet)
    strict = RuleSet(list(rules), strict=True)
    # The missing field would raise in strict mode: the registry is checked first.
    with pytest.raises(RegistryError) as info:
        strict.run({})
    assert (info.value.name, info.value.rule) == ("notify", "vip")
    with pytest.raises(MissingFieldError):
        strict.bind({"notify": record, "log": record}).run({})
    assert calls == []


def test_documents_bind_when_they_are_read(tmp_path: Any) -> None:
    actions = {"notify": record, "log": record}
    rules = loads(TEXT, format="text", actions=actions)
    assert isinstance(rules, RuleSet) and rules["audit"].do is record
    path = tmp_path / "rules.yaml"
    path.write_text(dumps(rules, format="yaml"), encoding="utf-8")
    from_file = load(path, actions=actions)
    assert isinstance(from_file, RuleSet) and from_file["vip"].do is record
    with pytest.raises(RegistryError):
        load(path, actions={})
    one = loads(dumps(rules["vip"]), actions=actions)
    assert isinstance(one, Rule) and one.do is record
    assert rule_from_text("IF x > 1 DO notify", actions=actions).do is record
    # `do` is the code of the action the text names.
    given = rule_from_text("IF x > 1 DO notify", do=fail)
    assert (given.action_name, given.do) == ("notify", fail)
    with pytest.raises(RegistryError):
        rule_from_text("IF x > 1 DO notify", actions={})
    with pytest.raises(TypeError, match="not both"):
        rule_from_text("IF x > 1 DO notify", do=fail, actions=actions)


def test_only_code_without_a_name_cannot_be_written() -> None:
    named = RuleSet([Rule("a", F.x > 1, do="notify")]).bind({"notify": record})
    assert '"do":"notify"' in dumps(named)
    with pytest.raises(UnsupportedError) as info:
        dumps(RuleSet([Rule("a", F.x > 1, do=record)]))
    assert info.value.pointer == "/ruleset/rules/0/do"
    assert '"do"' not in dumps(RuleSet([Rule("a", F.x > 1, do=record)]), omit_actions=True)


def test_bound_named_actions_pickle() -> None:
    bound = RuleSet([Rule("a", F.x > 1, do="notify")]).bind({"notify": record})
    clone = pickle.loads(pickle.dumps(bound))
    assert (clone["a"].action_name, clone["a"].do) == ("notify", record)
    unbound = pickle.loads(pickle.dumps(Rule("a", F.x > 1, do="notify")))
    assert (unbound.action_name, unbound.do) == ("notify", None)


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
