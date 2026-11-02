"""Loading and dumping documents in JSON and YAML (task D.1) and text (task D.2a)."""

import json
from decimal import Decimal
from pathlib import Path

import cleanrules
import pytest
from cleanrules import (
    Const,
    Expr,
    ExpressionError,
    F,
    FormatError,
    Rule,
    RuleSet,
    UnsupportedError,
    dump,
    dumps,
    load,
    loads,
)
from hypothesis import given

from tests.strategies import conditions


def shop() -> RuleSet:
    return RuleSet(
        [
            Rule(
                "vip",
                (F.customer.years > 5) & F.since.is_null(),
                {"discount": Decimal("0.15"), "label": "VIP"},
                priority=10,
                tags=["gold"],
                description="long-standing customers",
                metadata={"owner": "risk", "reviewed": [2025, 2026]},
            ),
            Rule("loyal", F.customer.years > 2, {"discount": Decimal("0.05")}, priority=5),
        ],
        hit_policy="priority",
    )


@pytest.mark.parametrize("format", ["json", "yaml"])
def test_rule_sets_round_trip(format: cleanrules.Format) -> None:
    rules = shop()
    text = dumps(rules, format=format)
    back = loads(text, format=format)
    assert isinstance(back, RuleSet)
    assert dumps(back) == dumps(rules)
    vip = back["vip"]
    assert vip.when.same_as(rules["vip"].when)
    assert (vip.priority, vip.tags, vip.description) == (10, ("gold",), "long-standing customers")
    assert vip.metadata == {"owner": "risk", "reviewed": [2025, 2026]}
    data = {"customer": {"years": 9}}
    assert back.evaluate(data).output == rules.evaluate(data).output


@pytest.mark.parametrize("format", ["json", "yaml"])
@given(tree=conditions)
def test_conditions_round_trip(format: cleanrules.Format, tree: Expr) -> None:
    back = loads(dumps(tree, format=format), format=format)
    assert isinstance(back, Expr)
    assert back.same_as(tree)


def test_documents_are_canonical_json() -> None:
    text = dumps(F.x > 1)
    assert json.loads(text) == {
        "version": 1,
        "condition": {
            "op": "gt",
            "left": {"op": "field", "path": ["x"]},
            "right": {"op": "const", "value": 1},
        },
    }
    assert isinstance(loads(dumps(shop()["vip"])), Rule)


def test_yaml_follows_the_json_subset_of_the_core_schema() -> None:
    text = (
        "version: 1\ncondition:\n  op: eq\n"
        "  left: {op: field, path: [d]}\n  right: {op: const, value: 2026-10-31}\n"
    )
    condition = loads(text, format="yaml")
    assert repr(condition) == "F.d == '2026-10-31'"  # a string, never a date


@pytest.mark.parametrize(
    ("text", "line", "column"),
    [
        ("version: 1\ncondition: &c {op: const, value: true}\n", 2, 12),
        ("version: 1\ncondition: {op: const, value: true}\nversion: 1\n", 3, 1),
    ],
)
def test_yaml_errors_have_line_and_column(text: str, line: int, column: int) -> None:
    with pytest.raises(FormatError) as info:
        loads(text, format="yaml")
    assert (info.value.line, info.value.column) == (line, column)


def test_shape_errors_have_pointers() -> None:
    with pytest.raises(FormatError) as info:
        loads('{"version": 1, "condition": {"op": "const", "value": true, "note": 1}}')
    assert info.value.pointer == "/condition/note"


def test_limits() -> None:
    text = dumps(shop())
    with pytest.raises(FormatError) as info:
        loads(text, max_size=10)
    assert info.value.code == "limit_exceeded"
    assert isinstance(loads(text, max_size=len(text)), RuleSet)


def test_value_nodes_are_not_documents() -> None:
    with pytest.raises(ExpressionError, match="only a condition"):
        dumps(F.x + 1)


def record(context: object) -> None:
    pass


def test_actions_are_never_written() -> None:
    rules = RuleSet([Rule("a", F.x > 1), Rule("b", F.x > 2, do=record)])
    with pytest.raises(UnsupportedError) as info:
        dumps(rules)
    assert info.value.pointer == "/ruleset/rules/1/do"
    assert info.value.rule == "b"
    back = loads(dumps(rules, omit_actions=True))
    assert isinstance(back, RuleSet)
    assert back["b"].do is None
    with pytest.raises(UnsupportedError) as single:
        dumps(rules["b"])
    assert single.value.pointer == "/rule/do"


def test_files(tmp_path: Path) -> None:
    rules = shop()
    for name in ("rules.json", "rules.yaml", "rules.yml"):
        dump(rules, tmp_path / name)
        assert dumps(load(tmp_path / name)) == dumps(rules)
    assert (tmp_path / "rules.yaml").read_text().startswith("version: 1\nruleset:\n")
    with pytest.raises(ValueError, match="suffix"):
        dump(rules, tmp_path / "rules.txt")
    dump(rules, tmp_path / "rules.txt", format="yaml")
    assert isinstance(load(tmp_path / "rules.txt", format="yaml"), RuleSet)


def test_file_objects_bytes_and_limits(tmp_path: Path) -> None:
    import io

    text = dumps(shop(), format="yaml")
    assert isinstance(load(io.StringIO(text), format="yaml"), RuleSet)
    buffer = io.StringIO()
    dump(shop(), buffer, format="json")
    assert isinstance(loads(buffer.getvalue().encode()), RuleSet)
    with pytest.raises(FormatError, match="UTF-8"):
        loads(b"\xff\xfe")
    with pytest.raises(ValueError, match="max_size"):
        loads(text, max_size=-1)
    big = tmp_path / "big.yaml"
    big.write_text("# " + "x" * (1024 * 1024) + "\n")
    with pytest.raises(FormatError) as info:
        load(big)
    assert info.value.code == "limit_exceeded"


def test_yaml_has_its_own_size_limit() -> None:
    text = "version: 1\ncondition: {op: const, value: true}\n# " + "x" * (1024 * 1024) + "\n"
    with pytest.raises(FormatError) as info:
        loads(text, format="yaml")
    assert info.value.code == "limit_exceeded"
    assert isinstance(loads(text, format="yaml", max_size=2 * 1024 * 1024), Expr)


def test_yaml_errors_say_how_to_fix_them() -> None:
    with pytest.raises(FormatError) as info:
        loads("version: 1\ncondition: {op: const, value: !!str x}\n", format="yaml")
    assert "$date" in (info.value.hint or "")
    with pytest.raises(FormatError) as directive:
        loads("%YAML 1.1\n---\nversion: 1\n", format="yaml")
    assert (directive.value.line, directive.value.column) == (1, 1)


def test_the_documented_examples_load() -> None:
    examples = Path(__file__).resolve().parents[3] / "docs" / "examples"
    from_yaml, from_json = load(examples / "discounts.yaml"), load(examples / "discounts.json")
    assert isinstance(from_yaml, RuleSet)
    assert dumps(from_yaml) == dumps(from_json)
    assert (examples / "discounts.yaml").read_text() == dumps(from_yaml, format="yaml")
    from_text = load(examples / "discounts.rules")
    assert dumps(from_text) == dumps(from_json)
    assert (examples / "discounts.rules").read_text() == dumps(from_text, format="text") + "\n"


# --- Text (task D.2a) -------------------------------------------------------------------

SHOP_TEXT = """HIT POLICY priority

RULE vip PRIORITY 10 TAGS gold
IF customer.years > 5 AND since IS NULL
THEN discount = 0.15, label = "VIP"

RULE loyal PRIORITY 5
IF customer.years > 2
THEN discount = 0.05"""


def test_text_round_trip() -> None:
    rules = loads(SHOP_TEXT, format="text")
    assert isinstance(rules, RuleSet)
    assert dumps(rules, format="text") == SHOP_TEXT
    assert rules["vip"].then["discount"].same_as(Const(Decimal("0.15")))
    decision = rules.evaluate({"customer": {"years": 9}, "since": None})
    assert decision.output == {"discount": Decimal("0.15"), "label": "VIP"}
    # The same model as the JSON document, without metadata and description.
    plain = RuleSet(
        [Rule(r.name, r.when, r.then, priority=r.priority, tags=r.tags) for r in shop()],
        hit_policy="priority",
    )
    assert dumps(loads(dumps(plain, format="text"), format="text")) == dumps(plain)


@given(tree=conditions)
def test_conditions_round_trip_through_text(tree: Expr) -> None:
    text = dumps(tree, format="text")
    assert str(tree) == text
    back = loads(text, format="text")
    assert isinstance(back, Expr)
    assert back.same_as(tree), text


def test_rule_from_text() -> None:
    rule = cleanrules.rule_from_text("if total > 100 then discount = 0.1")
    assert (rule.name, rule.priority, rule.do) == ("rule_1", 0, None)
    assert rule.when.same_as(F.total > 100)
    with_action = cleanrules.rule_from_text("RULE big TAGS a IF total > 100", do=record)
    assert (with_action.name, with_action.tags, with_action.do) == ("big", ("a",), record)
    with pytest.raises(cleanrules.RuleSyntaxError, match="expected RULE or IF") as condition:
        cleanrules.rule_from_text("total > 100")
    assert (condition.value.line, condition.value.column) == (1, 1)
    with pytest.raises(cleanrules.RuleSyntaxError, match="end of the text") as two:
        cleanrules.rule_from_text(b"IF a\nIF b")
    assert (two.value.line, two.value.column) == (2, 1)
    assert "rule set" in (two.value.hint or "")


def test_text_errors_have_line_and_column() -> None:
    with pytest.raises(cleanrules.RuleSyntaxError) as info:
        loads("RULE r\nIF x >\nTHEN y = 1", format="text")
    assert (info.value.line, info.value.column) == (3, 1)
    assert "expected a field" in str(info.value)
    with pytest.raises(ExpressionError) as expression:
        loads('a AND\n  x MATCHES "("', format="text")
    assert (expression.value.line, expression.value.column) == (2, 3)
    with pytest.raises(FormatError) as limit:
        loads("NOT NOT NOT x", format="text", max_depth=2)
    assert limit.value.code == "limit_exceeded"


def test_text_cannot_hold_metadata_or_actions() -> None:
    with pytest.raises(UnsupportedError, match="metadata"):
        dumps(shop()["vip"], format="text")
    rules = RuleSet([Rule("a", F.x > 1, do=record)])
    with pytest.raises(UnsupportedError, match="action"):
        dumps(rules, format="text")
    assert dumps(rules, format="text", omit_actions=True) == "HIT POLICY first\n\nRULE a\nIF x > 1"


def test_text_files(tmp_path: Path) -> None:
    path = tmp_path / "pricing.rules"
    path.write_text(SHOP_TEXT, encoding="utf-8")
    assert dumps(load(path), format="text") == SHOP_TEXT
    dump(load(path), tmp_path / "copy.rules")
    assert (tmp_path / "copy.rules").read_text(encoding="utf-8") == SHOP_TEXT + "\n"
    with pytest.raises(ValueError, match="format='text'"):
        load(tmp_path / "pricing.txt")
