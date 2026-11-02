"""Decision tables read and written as CSV, JSON and YAML (task D.3a)."""

from decimal import Decimal
from pathlib import Path

import pytest
from cleanrules import (
    DecisionTable,
    F,
    FormatError,
    RuleDefinitionError,
    RuleSet,
    RuleSyntaxError,
    TableOutput,
    TablePolicy,
    UnsupportedError,
    dump,
    dumps,
    load,
    loads,
)

EXAMPLE = Path(__file__).resolve().parents[3] / "docs" / "examples" / "risk.csv"


def risk() -> DecisionTable:
    table = load(EXAMPLE)
    assert isinstance(table, DecisionTable)
    return table


def test_a_table_compiles_into_a_rule_set() -> None:
    table = risk()
    assert (table.hit_policy, table.aggregate, table.logic, table.strict) == (
        "priority",
        None,
        "boolean",
        False,
    )
    assert [str(i) for i in table.inputs] == ["customer.years", "order.total"]
    assert table.outputs[1] == TableOutput("risk", ("high", "medium", "low"))
    new = table.rows[0]
    assert (new.name, new.priority, new.description) == (
        "new",
        None,
        "New customers with large orders are checked first",
    )
    assert new.inputs[0] is not None and new.inputs[0].same_as(F.customer.years < 1)
    assert table.rows[2].inputs[0] is None
    assert isinstance(table.ruleset, RuleSet)
    # Output priorities become rule priorities: high, then medium, then low.
    assert [(r.name, r.priority) for r in table.ruleset] == [
        ("new", 3),
        ("vip", 1),
        ("big", 2),
        ("default", 1),
    ]
    decision = table.evaluate({"customer": {"years": 9}, "order": {"total": 6000}})
    assert decision.output == {"discount": Decimal("0.05"), "risk": "medium"}
    assert str(decision.explain()).startswith("HIT POLICY priority: selected big\n")
    assert len(table.rows) == 4
    assert repr(table) == "<DecisionTable priority: 4 rows, 4 columns>"


def test_the_example_is_canonical(tmp_path: Path) -> None:
    table = risk()
    assert EXAMPLE.read_text(encoding="utf-8") == dumps(table, format="csv")
    for name in ("risk.json", "risk.yaml", "risk.csv"):
        dump(table, tmp_path / name)
        back = load(tmp_path / name)
        assert isinstance(back, DecisionTable)
        assert dumps(back) == dumps(table)
    assert (tmp_path / "risk.csv").read_text(encoding="utf-8") == dumps(table, format="csv")


def test_csv_errors_have_line_and_column() -> None:
    with pytest.raises(RuleSyntaxError) as syntax:
        loads("RULE,x\na,>  \n", format="csv")
    assert (syntax.value.line, syntax.value.column) == (2, 6)
    with pytest.raises(FormatError) as shape:
        loads('x\n"1\n', format="csv")
    assert (shape.value.line, shape.value.column) == (2, 1)
    with pytest.raises(RuleDefinitionError) as values:
        loads("x,\"THEN y IN ('a')\"\n> 1,'b'\n", format="csv")
    assert (values.value.line, values.value.column) == (2, 5)
    assert "values" in (values.value.hint or "")


def test_json_errors_point_to_the_cell() -> None:
    document = (
        '{"version": 1, "decision_table": {"inputs": ["x"], "outputs": [],'
        ' "rows": [{"inputs": ["IN ("], "outputs": []}]}}'
    )
    with pytest.raises(RuleSyntaxError) as info:
        loads(document)
    assert info.value.pointer == "/decision_table/rows/0/inputs/0"
    assert "column" in str(info.value)


def test_only_tables_are_csv_and_tables_are_not_text() -> None:
    with pytest.raises(UnsupportedError, match="decision tables"):
        dumps(RuleSet([]), format="csv")
    with pytest.raises(UnsupportedError, match="no text form"):
        dumps(risk(), format="text")
    with pytest.raises(FormatError) as info:
        loads("x\n> 1\n", format="csv", max_size=3)
    assert info.value.code == "limit_exceeded"


def test_tables_are_immutable_and_pickle() -> None:
    import pickle

    table = risk()
    with pytest.raises(AttributeError, match="immutable"):
        table.hit_policy = TablePolicy.FIRST  # pyright: ignore[reportAttributeAccessIssue]
    with pytest.raises(AttributeError, match="immutable"):
        del table.inputs  # pyright: ignore[reportAttributeAccessIssue]
    with pytest.raises(TypeError, match="load"):
        DecisionTable()
    back = pickle.loads(pickle.dumps(table))
    assert isinstance(back, DecisionTable)
    assert dumps(back, format="csv") == dumps(table, format="csv")
    assert table.hit_policy is TablePolicy.PRIORITY
    assert table.hit_policy == "priority"


def test_tables_delegate_to_their_rule_set() -> None:
    table = risk()
    decisions = table.evaluate_many([{"customer": {"years": 9}}, {"order": {"total": 6000}}])
    assert [d.fired for d in decisions] == [("vip", "default"), ("big", "default")]
    assert [r.name for r in table.summary().rules] == ["new", "vip", "big", "default"]


def test_files_keep_their_line_breaks(tmp_path: Path) -> None:
    text = 'RULE,x,DESCRIPTION\na,> 1,"two\r\nlines"\n'
    path = tmp_path / "t.csv"
    path.write_bytes(text.encode())
    table = load(path)
    assert isinstance(table, DecisionTable)
    assert table.rows[0].description == "two\r\nlines"
    dump(table, tmp_path / "copy.csv")
    assert (tmp_path / "copy.csv").read_bytes() == dumps(table, format="csv").encode()


def test_the_readme_shows_the_example() -> None:
    readme = (Path(__file__).resolve().parents[1] / "README.md").read_text(encoding="utf-8")
    assert "```text\n" + EXAMPLE.read_text(encoding="utf-8") + "```" in readme
    decision = risk().evaluate({"customer": {"years": 0}, "order": {"total": 2000}})
    assert decision.output == {"discount": 0, "risk": "high"}
