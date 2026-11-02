"""Rules, rule sets, decisions, explanations and summaries (task C.2b).

The semantics are the core's and its conformance vectors check them; these tests
check the Python API: construction, validation, results and errors.
"""

import datetime as dt
from decimal import Decimal
from pathlib import Path
from typing import Any

import pytest
from cleanrules import (
    Aggregate,
    Cause,
    Const,
    Counterfactual,
    EvaluationError,
    F,
    FieldUsage,
    HitPolicy,
    HitPolicyError,
    MissingFieldError,
    Reason,
    RegistryError,
    Rule,
    RuleDefinitionError,
    RuleNotFoundError,
    RuleSet,
    RuleSummary,
    Status,
    Truth,
    TypeMismatchError,
)
from cleanrules.errors import ExpressionError


def shop(**options: Any) -> RuleSet:
    return RuleSet(
        [
            Rule(
                "vip",
                (F.customer.years > 5) & (F.order.total > 1000),
                {"discount": Decimal("0.15")},
                priority=10,
                tags=["gold"],
            ),
            Rule(
                "loyal",
                (F.customer.years > 2) | F.customer.newsletter,
                {"discount": Decimal("0.05")},
                priority=5,
                tags=["gold", "mail"],
            ),
            Rule("blocked", F.customer.blocked, {"discount": 0}, priority=99),
        ],
        **options,
    )


CUSTOMER = {"customer": {"years": 3, "newsletter": False}, "order": {"total": 800}}

# --- Rules ----------------------------------------------------------------------------


def test_rules_keep_their_parts() -> None:
    rule = Rule(
        "r",
        F.x > 1,
        {"a": 1, "b": F.x * 2},
        priority=3,
        tags=["t", "u"],
        description="d",
        metadata={"owner": "risk", "version": 2},
    )
    assert rule.name == "r"
    assert rule.when.same_as(F.x > 1)
    assert list(rule.then) == ["a", "b"]
    assert rule.then["b"].same_as(F.x * 2)
    assert (rule.priority, rule.tags, rule.description) == (3, ("t", "u"), "d")
    assert rule.metadata == {"owner": "risk", "version": 2}
    with pytest.raises(AttributeError):
        rule.name = "s"  # type: ignore[misc]


@pytest.mark.parametrize(
    ("build", "match"),
    [
        (lambda: Rule("", F.x > 1), "non-empty"),
        (lambda: Rule("r", "x > 1"), "condition node"),  # type: ignore[arg-type]
        (lambda: Rule("r", F.x > 1, priority=True), "int"),
        (lambda: Rule("r", F.x > 1, priority=2**63), "64 bits"),
        (lambda: Rule("r", F.x > 1, tags="t"), "not the string"),
        (lambda: Rule("r", F.x > 1, [1]), "mapping"),  # type: ignore[arg-type]
        (lambda: Rule("r", F.x > 1, {"a": F.y > 1}), "condition"),
    ],
)
def test_invalid_rules_are_rejected(build: Any, match: str) -> None:
    with pytest.raises(RuleDefinitionError, match=match):
        build()


def test_invalid_payload_constants_are_expression_errors() -> None:
    with pytest.raises(ExpressionError, match="NaN"):
        Rule("r", F.x > 1, {"a": float("nan")})


def test_a_value_is_not_a_condition() -> None:
    with pytest.raises((RuleDefinitionError, ExpressionError)):
        Rule("r", F.x + 1)


# --- Rule sets ------------------------------------------------------------------------


def test_rule_sets_are_sequences_by_position_and_name() -> None:
    rules = shop()
    assert len(rules) == 3
    assert rules[0].name == "vip"
    assert rules["loyal"] is rules[1]
    assert [r.name for r in rules[1:]] == ["loyal", "blocked"]
    assert "vip" in rules
    assert rules[0] in rules
    assert rules.get("nope") is None
    assert repr(rules) == "RuleSet([vip, loyal, blocked], hit_policy='first')"
    with pytest.raises(RuleNotFoundError) as info:
        _ = rules["nope"]
    assert isinstance(info.value, KeyError)
    assert info.value.available == ("blocked", "loyal", "vip")


def test_configuration() -> None:
    rules = shop(hit_policy="collect", aggregate="sum", strict=True)
    assert rules.hit_policy is HitPolicy.COLLECT
    assert (rules.aggregate, rules.logic, rules.strict) == ("sum", "boolean", True)


def test_duplicate_names_are_rejected_by_the_core() -> None:
    with pytest.raises(RuleDefinitionError) as info:
        RuleSet([Rule("a", F.x > 1), Rule("a", F.x > 2)])
    assert info.value.rule == "a"
    assert info.value.code == "rule_definition_error"


def test_invalid_configurations() -> None:
    with pytest.raises(RuleDefinitionError, match="hit policy"):
        shop(hit_policy="best")
    with pytest.raises(RuleDefinitionError, match="aggregate"):
        shop(aggregate="mean")
    with pytest.raises(RuleDefinitionError):
        shop(aggregate="sum")  # without COLLECT
    with pytest.raises(RegistryError) as info:
        shop(logic="fuzzy")
    assert info.value.name == "fuzzy"
    assert "boolean" in info.value.available
    with pytest.raises(RuleDefinitionError, match="Rule objects"):
        RuleSet([F.x > 1])  # type: ignore[list-item]


# --- Decisions ------------------------------------------------------------------------


def test_first_selects_the_first_rule_that_fired() -> None:
    decision = shop().evaluate(CUSTOMER)
    assert [r.rule.name for r in decision.selected] == ["loyal"]
    assert decision.fired == ("loyal",)
    assert decision.output == {"discount": Decimal("0.05")}
    assert [r.truth for r in decision.results] == [Truth.FALSE, Truth.TRUE, Truth.UNKNOWN]
    assert decision.results[1].rule is decision.ruleset["loyal"]


def test_priority_prefers_the_highest_priority() -> None:
    data = {"customer": {"years": 9, "blocked": True}, "order": {"total": 2000}}
    decision = shop(hit_policy="priority").evaluate(data)
    assert decision.output == {"discount": 0}
    assert decision.fired == ("vip", "loyal", "blocked")


def test_nothing_selected_gives_no_output() -> None:
    decision = shop().evaluate({"customer": {"years": 0, "newsletter": False}})
    assert decision.selected == ()
    assert decision.output is None
    assert decision.outputs == ()


def test_unique_raises_when_several_fire() -> None:
    data = {"customer": {"years": 9}, "order": {"total": 2000}}
    with pytest.raises(HitPolicyError) as info:
        shop(hit_policy="unique").evaluate(data)
    assert info.value.rules == ("vip", "loyal")


def test_multi_hit_decisions_have_several_outputs() -> None:
    data = {"customer": {"years": 9}, "order": {"total": 2000}}
    decision = shop(hit_policy="rule_order").evaluate(data)
    assert decision.outputs == ({"discount": Decimal("0.15")}, {"discount": Decimal("0.05")})
    with pytest.raises(HitPolicyError, match="several outputs"):
        _ = decision.output


def test_collect_aggregates() -> None:
    data = {"customer": {"years": 9}, "order": {"total": 2000}}
    total = shop(hit_policy="collect", aggregate="sum").evaluate(data)
    # Aggregates add decimals as floats (spec/data-model.md, *Decimals*).
    assert total.output == total.aggregated == {"discount": pytest.approx(0.2)}
    count = shop(hit_policy="collect", aggregate="count").evaluate(data)
    assert count.output == {"discount": 2}
    nothing = shop(hit_policy="collect", aggregate="sum").evaluate({})
    assert nothing.output is None


def test_payload_values_are_evaluated() -> None:
    rules = RuleSet([Rule("r", F.total > 0, {"tax": F.total * Decimal("0.21"), "k": "v"})])
    assert rules.evaluate({"total": 100}).output == {"tax": pytest.approx(21.0), "k": "v"}


def test_strict_mode_raises_with_context() -> None:
    rules = shop(strict=True)
    with pytest.raises(MissingFieldError) as missing:
        rules.evaluate({"customer": {"years": 9}})
    assert missing.value.rule == "vip"
    assert missing.value.path == ("order", "total")
    with pytest.raises(TypeMismatchError) as mismatch:
        rules.evaluate({"customer": {"years": "nine"}, "order": {"total": 1}})
    assert mismatch.value.types == ("string", "number")


def test_batches_name_the_failing_row() -> None:
    rules = shop(strict=True)
    good = {"customer": {"years": 1, "newsletter": True, "blocked": False}, "order": {"total": 5}}
    decisions = rules.evaluate_many([good, good])
    assert [d.output for d in decisions] == [{"discount": Decimal("0.05")}] * 2
    with pytest.raises(MissingFieldError) as info:
        rules.evaluate_many([good, {"customer": {}}])
    assert info.value.row == 1
    assert "(input 1)" in str(info.value)


# --- Data -----------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("value", "truth"),
    [
        (5, Truth.TRUE),
        (Decimal("1.0000000000000000000000000001"), Truth.TRUE),  # exact: a float says 1.0
        (2**70, Truth.UNKNOWN),  # int_overflow
        (float("nan"), Truth.UNKNOWN),
        (b"bytes", Truth.UNKNOWN),  # opaque: a type mismatch
        (None, Truth.UNKNOWN),
    ],
)
def test_input_values(value: object, truth: Truth) -> None:
    rules = RuleSet([Rule("r", F.x > 1)])
    assert rules.evaluate({"x": value}).results[0].truth is truth


def test_paths_read_lists_and_dates() -> None:
    madrid = dt.timezone(dt.timedelta(hours=1))
    rules = RuleSet(
        [
            Rule("r", (F.orders[1].total > 10) & (F.at >= dt.datetime(2026, 1, 1, tzinfo=dt.UTC))),
        ]
    )
    data = {
        "orders": [{"total": 1}, {"total": 20}],
        "at": dt.datetime(2026, 1, 1, 1, tzinfo=madrid),
    }
    assert rules.evaluate(data).fired == ("r",)


# --- Explanations and summaries -------------------------------------------------------


def test_explanations_give_statuses_and_causes() -> None:
    decision = shop(hit_policy="priority").evaluate(CUSTOMER)
    explanation = decision.explain()
    assert explanation.selected == ("loyal",)
    statuses = {entry.rule: entry.status for entry in explanation.rules}
    assert statuses == {
        "vip": Status.NOT_FIRED,
        "loyal": Status.SELECTED,
        "blocked": Status.UNKNOWN,
    }
    vip = decision.why_not("vip")
    assert vip.achievable is True
    assert vip.causes[0].node.same_as(F.customer.years > 5)
    assert [(c.truth, c.holds) for c in vip.causes] == [(Truth.FALSE, False)] * 2
    assert [c.values for c in vip.causes] == [
        ((("customer", "years"), 3),),
        ((("order", "total"), 800),),
    ]
    assert [c.counterfactual for c in vip.causes if c.counterfactual] == [
        Counterfactual(("customer", "years"), "gt", 5, 2),
        Counterfactual(("order", "total"), "gt", 1000, 200),
    ]
    blocked = explanation.for_rule("blocked").causes[0]
    assert blocked.reason == Reason("missing_field", (("customer", "blocked"),), (), None)
    with pytest.raises(RuleNotFoundError):
        decision.why_not("nope")


def _resolve(document: dict[str, Any], pointer: str) -> Any:
    node: Any = document
    for token in pointer.split("/")[1:]:
        node = node[int(token)] if isinstance(node, list) else node[token]  # pyright: ignore[reportUnknownVariableType]
    return node  # pyright: ignore[reportUnknownVariableType]


def test_explanation_pointers_resolve_in_the_decision_document() -> None:
    decision = shop().evaluate(CUSTOMER)
    document = decision.to_dict()
    assert document["version"] == 1
    explanation = decision.explain().to_dict()["explanation"]
    stack = [entry["requirement"] for entry in explanation["rules"]]
    while stack:
        item = stack.pop()
        group = item.get("all", item.get("any"))
        if group is None:
            assert _resolve(document, item["at"])["truth"] == item["truth"]
        else:
            stack.extend(group)


def test_documents() -> None:
    decision = shop().evaluate(CUSTOMER)
    assert decision.to_dict()["decision"]["selected"] == ["loyal"]
    assert set(decision.explain().to_dict()) == {"version", "explanation"}
    assert set(shop().summary().to_dict()) == {"version", "summary"}


def test_summaries() -> None:
    summary = shop().summary()
    assert [field.path for field in summary.fields] == [
        ("customer", "years"),
        ("order", "total"),
        ("customer", "newsletter"),
        ("customer", "blocked"),
    ]
    assert summary.fields[0] == FieldUsage(("customer", "years"), ("vip", "loyal"), ())
    assert summary.rules[0] == RuleSummary("vip", 10, ("discount",))
    assert (summary.hit_policy, summary.aggregate, summary.logic, summary.strict) == (
        HitPolicy.FIRST,
        None,
        "boolean",
        False,
    )
    assert summary.outputs == ("discount",)
    assert summary.tags == {"gold": ("vip", "loyal"), "mail": ("loyal",)}


def test_explanations_and_summaries_have_text() -> None:
    decision = shop(hit_policy="priority").evaluate(CUSTOMER)
    explanation = decision.explain()
    lines = str(explanation).splitlines()
    assert lines[0] == "HIT POLICY priority: selected loyal"
    vip = explanation.for_rule("vip")
    assert str(vip).splitlines() == lines[1:5]
    assert lines[1] == "[ ] vip (not fired): customer.years > 5 AND order.total > 1000"
    assert str(vip.causes[0]) == "customer.years > 5 is false (customer.years = 3); short by 2"
    assert "    - customer.blocked is unknown: missing field customer.blocked" in lines
    assert str(shop().summary()).splitlines()[:3] == [
        "Rule set: 3 rules, hit policy first, logic boolean",
        "  priority  name     condition                                  outputs",
        "        10  vip      customer.years > 5 AND order.total > 1000  discount",
    ]


def test_the_readme_shows_the_real_explanation() -> None:
    readme = (Path(__file__).resolve().parents[1] / "README.md").read_text(encoding="utf-8")
    shown = readme.split("```text\n", 1)[1].split("```", 1)[0]
    assert shown == str(shop(hit_policy="priority").evaluate(CUSTOMER).explain()) + "\n"


def test_explanation_text_writes_python_values_as_constants() -> None:
    rules = RuleSet([Rule("cheap", (F.rate < 0.035) & (F.day >= dt.date(2026, 11, 1)))])
    text = str(rules.evaluate({"rate": 0.04, "day": dt.date(2026, 10, 29)}).explain())
    assert text.splitlines()[2:] == [
        "    would fire if all of these changed:",
        "    - rate < 0.035e0 is false (rate = 0.04e0); over by 0.005",
        '    - day >= DATE "2026-11-01" is false (day = DATE "2026-10-29"); short by 3 days',
    ]


def test_causes_have_text_and_hand_made_ones_fall_back_to_repr() -> None:
    import dataclasses

    rules = RuleSet([Rule("a", ~(F.y < 3) & F.missing)])
    explanation = rules.evaluate({"y": 1}).explain()
    negated, unknown = explanation.rules[0].causes
    assert str(negated) == (
        "y < 3 is true (y = 1), but the rule needs it false; needs y >= 3 (short by 2)"
    )
    assert str(unknown) == "missing is unknown: missing field missing"
    assert "_text" not in Cause.__match_args__
    changed = dataclasses.replace(negated, holds=True)
    assert str(changed) == repr(changed)
    rule = explanation.rules[0]
    assert str(dataclasses.replace(rule, note=None)) == repr(dataclasses.replace(rule, note=None))


def test_summaries_say_which_rules_have_actions() -> None:
    rules = RuleSet([Rule("a", F.x > 1, {"y": 1}), Rule("b", F.x > 2, do=lambda context: None)])
    summary = rules.summary()
    assert [rule.action for rule in summary.rules] == [False, True]
    assert [rule.get("action") for rule in summary.to_dict()["summary"]["rules"]] == [None, True]
    assert str(summary).splitlines()[3] == "         0  b     x > 2      +action"


def test_one_rule_set_is_shared_by_threads() -> None:
    # On free-threaded Python the threads run in parallel.
    from concurrent.futures import ThreadPoolExecutor

    rules = shop(hit_policy="collect", aggregate="sum")
    rows = [{"customer": {"years": i % 10}, "order": {"total": i * 100}} for i in range(400)]
    expected = [rules.evaluate(row).output for row in rows]

    def output(row: dict[str, Any]) -> object:
        return rules.evaluate(row).output

    with ThreadPoolExecutor(max_workers=8) as pool:
        for _ in range(5):
            assert list(pool.map(output, rows)) == expected


# --- Review fixes ---------------------------------------------------------------------


def test_self_containing_and_very_deep_data_do_not_crash() -> None:
    rules = RuleSet([Rule("r", F.x.y > 1)])
    looped: dict[str, Any] = {"y": 5}
    looped["self"] = looped
    assert rules.evaluate({"x": looped}).fired == ("r",)
    deep: list[Any] = []
    for _ in range(200_000):
        deep = [deep]
    assert rules.evaluate({"x": {"y": 2}, "deep": deep}).fired == ("r",)


@pytest.mark.parametrize("rows", ["ab", b"ab", {"x": 1}])
def test_batches_must_be_collections_of_inputs(rows: Any) -> None:
    with pytest.raises(EvaluationError, match="iterable of inputs"):
        shop().evaluate_many(rows)


@pytest.mark.parametrize(
    ("metadata", "match"),
    [
        ({"k": float("nan")}, "NaN"),
        ({"k": b"x"}, "not a data value"),
        ({"k": {"n": [1, {2: "a"}]}}, "not a string"),
    ],
)
def test_metadata_holds_data_only(metadata: dict[str, Any], match: str) -> None:
    with pytest.raises(RuleDefinitionError, match=match) as info:
        Rule("r", F.x > 1, metadata=metadata)
    assert info.value.rule == "r"


def test_self_containing_metadata_is_rejected() -> None:
    looped: dict[str, Any] = {}
    looped["self"] = looped
    with pytest.raises(RuleDefinitionError, match="contains itself"):
        Rule("r", F.x > 1, metadata=looped)


def test_metadata_is_what_the_core_holds() -> None:
    rule = Rule("r", F.x > 1, metadata={"when": dt.date(2026, 1, 1), "n": [1, 2.5]})
    assert rule.metadata == {"when": dt.date(2026, 1, 1), "n": [1, 2.5]}
    with pytest.raises(TypeError):
        rule.metadata["n"] = 3  # type: ignore[index]


def test_payload_errors_name_the_rule_and_key() -> None:
    with pytest.raises(ExpressionError) as info:
        Rule("r", F.x > 0, {"k": float("nan")})
    assert info.value.rule == "r"
    assert str(info.value).startswith("rule 'r': then['k']: a constant cannot be NaN")
    assert Rule("r", F.x > 0, {"": 1}).then[""].same_as(Const(1))  # the format allows it


def test_outputs_and_decisions_are_read_only() -> None:
    decision = shop().evaluate(CUSTOMER)
    with pytest.raises(TypeError):
        decision.results[1].output["discount"] = 1  # type: ignore[index]
    with pytest.raises(TypeError):
        decision.output["discount"] = 1  # type: ignore[index]
    with pytest.raises(AttributeError):
        decision.selected = ()  # type: ignore[misc]


def test_names_of_policies_and_aggregates_ignore_case() -> None:
    rules = shop(hit_policy="COLLECT", aggregate="Sum")
    assert (rules.hit_policy, rules.aggregate) == (HitPolicy.COLLECT, Aggregate.SUM)


def test_gaps_are_python_values() -> None:
    rules = RuleSet([Rule("r", (F.total > Decimal("1000.50")) & (F.since <= dt.date(2026, 1, 1)))])
    decision = rules.evaluate({"total": Decimal("951"), "since": dt.date(2026, 1, 10)})
    gaps = [c.counterfactual.gap for c in decision.why_not("r").causes if c.counterfactual]
    assert gaps == [Decimal("49.50"), dt.timedelta(days=9)]


def test_messages_name_the_rule_once() -> None:
    with pytest.raises(RuleDefinitionError) as info:
        RuleSet([Rule("a", F.x > 1), Rule("a", F.x > 2)])
    assert str(info.value).count("a") == str(info.value).replace("rule 'a': ", "").count("a")
