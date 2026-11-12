"""The tools for agents (`spec/agents.md`) from Python.

`Catalog`, input schemas and compact explanations. The conformance vectors
check the results themselves; these tests check the Python API around them.
"""

from __future__ import annotations

import datetime as dt
from decimal import Decimal
from types import MappingProxyType
from typing import Any

import cleanrules
import pytest
from cleanrules import Catalog, F, FormatError, Rule, RuleSet, Tool


def pricing() -> RuleSet:
    return RuleSet(
        [
            Rule("vip", F.customer.years > 5, {"discount": Decimal("0.15")}, priority=10),
            Rule("base", F.customer.years >= 0, {"discount": Decimal("0.00")}),
        ],
        hit_policy="priority",
    )


def test_tools_are_those_of_the_core() -> None:
    assert cleanrules.TOOLS == ("evaluate", "explain", "list_rulesets", "summary", "validate")
    assert tuple(Tool) == cleanrules.TOOLS
    assert (cleanrules.MIN_BUDGET, cleanrules.DEFAULT_BUDGET, cleanrules.MAX_BUDGET) == (
        256,
        2000,
        1 << 20,
    )


def test_a_catalog_holds_rule_sets_by_name() -> None:
    catalog = Catalog(MappingProxyType({"pricing": pricing(), "Base": pricing()}))
    assert catalog.names() == ("Base", "pricing")
    assert len(catalog) == 2
    assert "pricing" in catalog
    assert "nope" not in catalog
    assert catalog.tools == tuple(Tool)
    assert repr(catalog) == "<Catalog: Base, pricing>"
    assert len(Catalog()) == 0


def test_calls_take_python_data() -> None:
    arguments = {"ruleset": "pricing", "input": {"customer": {"years": 7}}}
    result = Catalog({"pricing": pricing()}).call(Tool.EVALUATE, arguments)
    assert not result.is_error
    assert result.content["selected"] == ["vip"]
    assert result.content["output"] == {"discount": {"$decimal": "0.15"}}


def test_native_values_are_written_in_the_format() -> None:
    rules = RuleSet(
        [
            Rule("cheap", F.price < Decimal("10.5"), {"ok": True}),
            Rule("old", F.day < dt.date(2026, 1, 1), {"ok": True}),
            Rule("late", F.at > dt.datetime(2026, 1, 1, tzinfo=dt.UTC), {"ok": True}),
        ],
        hit_policy="collect",
    )
    catalog = Catalog({"r": rules})
    data: dict[str, Any] = {
        "price": Decimal("10.25"),
        "day": dt.date(2025, 5, 1),
        "at": dt.datetime(2026, 2, 1, 9, tzinfo=dt.UTC),
    }
    result = catalog.call("evaluate", {"ruleset": "r", "input": data})
    assert result.content["selected"] == ["cheap", "old", "late"]
    # Typed values written as JSON keep their meaning.
    typed = {"price": {"$decimal": "10.25"}, "day": {"$date": "2025-05-01"}, "at": None}
    assert catalog.call("evaluate", {"ruleset": "r", "input": typed}).content["selected"] == [
        "cheap",
        "old",
    ]
    infinite = catalog.call("evaluate", {"ruleset": "r", "input": {"price": float("-inf")}})
    assert infinite.content["selected"] == ["cheap"]


def test_calls_take_json_text() -> None:
    result = Catalog({"pricing": pricing()}).call("list_rulesets", "{}")
    assert result.content["rulesets"][0]["name"] == "pricing"


def test_a_tool_error_is_a_result_with_a_hint() -> None:
    result = Catalog({"pricing": pricing()}).call("evaluate", {"ruleset": "prices", "input": {}})
    assert result.is_error
    error = result.content["error"]
    assert error["code"] == "registry_error"
    assert error["pointer"] == "/ruleset"
    assert error["available"] == ["pricing"]
    assert error["hint"]


def test_a_catalog_offers_the_tools_it_is_given() -> None:
    catalog = Catalog({"pricing": pricing()}, tools=["summary", Tool.EVALUATE])
    assert catalog.tools == (Tool.EVALUATE, Tool.SUMMARY)
    result = catalog.call("validate", {"document": "x", "format": "text"})
    assert result.is_error
    assert result.content["error"]["available"] == ["evaluate", "summary"]
    with pytest.raises(ValueError, match="delete"):
        Catalog(tools=["delete"])


def test_actions_given_as_code_show_in_summaries() -> None:
    rules = RuleSet([Rule("a", F.x > 1, do=lambda context: None), Rule("b", F.x > 2)])
    summary = Catalog({"r": rules}).call("summary", {"ruleset": "r"}).content["summary"]
    assert [rule.get("action") for rule in summary["rules"]] == [True, None]
    assert summary == rules.summary().to_dict()["summary"]


def test_rule_sets_and_names_are_checked() -> None:
    with pytest.raises(TypeError, match="not a RuleSet"):
        Catalog({"x": "RULE a IF b"})  # pyright: ignore[reportArgumentType]
    with pytest.raises(TypeError, match="names are str"):
        Catalog({1: pricing()})  # pyright: ignore[reportArgumentType]


@pytest.mark.parametrize(
    ("arguments", "error"),
    [
        ({"x": object()}, TypeError),
        ({"x": {1, 2}}, TypeError),
        ({"x": {1: "a"}}, TypeError),
        ({"x": float("nan")}, ValueError),
        ({"x": Decimal("NaN")}, ValueError),
        (["ruleset"], TypeError),
    ],
)
def test_arguments_must_be_json_data(arguments: Any, error: type[Exception]) -> None:
    with pytest.raises(error):
        Catalog().call("list_rulesets", arguments)


def test_arguments_cannot_contain_themselves() -> None:
    looped: dict[str, Any] = {}
    looped["self"] = looped
    with pytest.raises(ValueError, match="contain themselves"):
        Catalog().call("list_rulesets", {"x": looped})
    shared = [1]
    result = Catalog().call("list_rulesets", {"a": shared, "b": shared})
    assert result.content["error"]["pointer"] == "/a"


def test_input_schema() -> None:
    schema = pricing().input_schema()
    assert schema["$schema"] == "https://json-schema.org/draft/2020-12/schema"
    years = schema["properties"]["customer"]["anyOf"][0]["properties"]["years"]
    assert years["description"] == "conditions of vip, base"
    assert years["anyOf"] == [{"type": "number"}, {"type": "null"}]


def test_compact_explanation() -> None:
    explanation = pricing().evaluate({"customer": {"years": 3}}).explain()
    compact = explanation.compact()
    assert isinstance(compact, cleanrules.CompactExplanation)
    assert compact.complete
    assert compact.text == str(explanation)
    assert str(compact) == compact.text
    assert len(explanation.compact(256).text) <= 256


@pytest.mark.parametrize("budget", [255, 1048577, -1, 10**30])
def test_a_budget_out_of_range_is_the_tools_error(budget: int) -> None:
    explanation = pricing().evaluate({}).explain()
    with pytest.raises(FormatError) as raised:
        explanation.compact(budget)
    assert raised.value.code == "format_error"
    assert raised.value.pointer == "/budget"


@pytest.mark.parametrize("budget", [True, 300.0, "300"])
def test_a_budget_is_an_int(budget: object) -> None:
    explanation = pricing().evaluate({}).explain()
    with pytest.raises(TypeError, match="budget"):
        explanation.compact(budget)  # pyright: ignore[reportArgumentType]
