"""Loading and dumping documents in JSON and YAML (task D.1)."""

import json
from decimal import Decimal
from pathlib import Path

import cleanrules
import pytest
from cleanrules import (
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
