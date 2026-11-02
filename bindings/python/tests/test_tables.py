"""Decision tables: read and written as CSV, JSON and YAML (D.3a), built from rows (D.3b)."""

import datetime as dt
from collections.abc import Iterator
from decimal import Decimal
from pathlib import Path
from typing import Any

import pytest
from cleanrules import (
    DecisionTable,
    ExpressionError,
    F,
    FormatError,
    RegistryError,
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


def test_rows_build_the_table_their_csv_would() -> None:
    header = [
        "RULE",
        "PRIORITY",
        "customer.years",
        "order.total",
        "THEN discount",
        "THEN risk IN ('high', 'medium', 'low')",
        "DESCRIPTION",
    ]
    table = DecisionTable(
        header,
        [
            [
                "new",
                None,
                "< 1",
                "> 1000",
                0,
                "'high'",
                "New customers with large orders are checked first",
            ],
            ["vip", None, "> 5", "-", Decimal("0.15"), "'low'", " Long-standing customers "],
            ["big", "", "-", "> 5000", "0.05", "'medium'"],
            [None, None, None, None, "0", "'low'"],
        ],
        hit_policy="PRIORITY",
    )
    built = dumps(table, format="csv")
    assert built == dumps(risk(), format="csv").replace("default,", ",")
    assert table.hit_policy is TablePolicy.PRIORITY
    assert [r.name for r in table.ruleset] == ["new", "vip", "big", "row_4"]
    assert repr(table) == "<DecisionTable priority: 4 rows, 4 columns>"


def test_cells_may_be_values_and_expressions() -> None:
    table = DecisionTable(
        [F.age, "since", TableOutput("band", ("minor", "adult")), "THEN note"],
        [
            [F.age < 18, None, "'minor'"],
            [18, dt.date(2026, 1, 1), "'adult'", F.age * 2],
            [True, None, "'adult'", "'x'", None, "  "],
        ],
        aggregate=None,
    )
    assert table.outputs == (TableOutput("band", ("minor", "adult")), TableOutput("note"))
    assert [str(i) for i in table.inputs] == ["age", "since"]
    cells = [[None if c is None else str(c) for c in row.inputs] for row in table.rows]
    assert cells == [
        ["age < 18", None],
        ["age = 18", 'since = DATE "2026-01-01"'],
        ["age = TRUE", None],
    ]
    assert not table.strict and table.rows[1].outputs[1] is not None
    assert table.evaluate({"age": 18, "since": dt.date(2026, 1, 1)}).output == {
        "band": "adult",
        "note": 36,
    }


def test_text_cells_are_the_text_syntax() -> None:
    # "VIP" is the field VIP: the string is written between single quotes.
    with pytest.raises(RuleDefinitionError) as info:
        DecisionTable(["x", "THEN risk IN ('high', 'low')"], [["1", "'low'"], ["2", "high"]])
    assert info.value.pointer == "/rows/1/1"
    assert info.value.hint == "high is a field; the string is written between single quotes: 'high'"
    with pytest.raises(RuleSyntaxError) as syntax:
        DecisionTable(["x"], [["> )"]])
    assert syntax.value.pointer == "/rows/0/0"
    assert "(column 3 of the text)" in str(syntax.value)


def test_grid_errors_point_into_the_grid() -> None:
    with pytest.raises(FormatError) as twice:
        DecisionTable(["RULE", "x", "rule"], [])
    assert twice.value.pointer == "/header/2"
    with pytest.raises(FormatError) as extra:
        DecisionTable(["x"], [["1", None, "2"]])
    assert extra.value.pointer == "/rows/0/2"
    with pytest.raises(FormatError) as priority:
        DecisionTable(["x", "PRIORITY"], [["1", 1.5]])
    assert priority.value.pointer == "/rows/0/1"
    with pytest.raises(UnsupportedError) as other_column:
        DecisionTable(["x"], [[F.y > 1]])
    assert other_column.value.pointer == "/rows/0/0"
    # Values the binding cannot convert are located too.
    with pytest.raises(ExpressionError) as nan:
        DecisionTable(["x"], [[1], [float("nan")]])
    assert nan.value.pointer == "/rows/1/0"
    with pytest.raises(ExpressionError) as value:
        DecisionTable(["x", TableOutput("y", (object(),))], [])
    assert value.value.pointer == "/header/1"


def test_settings_and_shapes_are_checked() -> None:
    with pytest.raises(RuleDefinitionError, match="hit policy"):
        DecisionTable(["x"], [], hit_policy="sometimes")
    with pytest.raises(RuleDefinitionError, match="aggregate"):
        DecisionTable(["x"], [], hit_policy="collect", aggregate="mean")
    with pytest.raises(RuleDefinitionError, match="strict"):
        DecisionTable(["x"], [], strict=1)  # pyright: ignore[reportArgumentType]
    with pytest.raises(RegistryError):
        DecisionTable(["x"], [], logic="nope")
    with pytest.raises(ValueError, match="max_table_size"):
        DecisionTable(["x"], [], max_table_size=0)
    with pytest.raises(TypeError, match="not a str"):
        DecisionTable("x", [])
    with pytest.raises(TypeError, match="header item"):
        DecisionTable(["x", 3], [])  # pyright: ignore[reportArgumentType]
    with pytest.raises(TypeError, match="row 0 is a str"):
        DecisionTable(["x"], ["> 1"])
    with pytest.raises(TypeError):
        DecisionTable()  # pyright: ignore[reportCallIssue]
    table = DecisionTable(
        ["x", "THEN y"], [["> 1", 1]], hit_policy=TablePolicy.COLLECT, aggregate="SUM", strict=True
    )
    assert (table.hit_policy, table.aggregate, table.strict) == ("collect", "sum", True)


def test_rows_are_read_one_at_a_time() -> None:
    def rows() -> Iterator[list[object]]:
        yield ["> 1"]
        raise LookupError("the source failed")

    with pytest.raises(LookupError, match="the source failed"):
        DecisionTable(["x"], rows())
    with pytest.raises(FormatError) as info:
        DecisionTable(["x"], (["> 1"] for _ in range(10)), max_table_size=7)
    assert (info.value.code, info.value.pointer) == ("limit_exceeded", "/rows/1/0")


def test_the_table_size_limit_can_be_raised() -> None:
    text = "x\n" + "> 1\n" * 3
    with pytest.raises(FormatError) as info:
        loads(text, format="csv", max_table_size=7)
    assert info.value.code == "limit_exceeded"
    assert isinstance(loads(text, format="csv", max_table_size=12), DecisionTable)


def test_built_tables_pickle() -> None:
    import pickle

    table = DecisionTable(["x", "THEN y"], [["> 1", "'a'"]])
    back = pickle.loads(pickle.dumps(table))
    assert dumps(back, format="csv") == dumps(table, format="csv")


def test_a_pandas_frame_is_read_as_its_csv() -> None:
    pandas = pytest.importorskip("pandas")
    frame = pandas.DataFrame(
        {
            "RULE": ["new", "vip", None],
            "PRIORITY": [2, None, 1],
            "customer.years": ["< 1", "> 5", float("nan")],
            "THEN discount": [0, 0.15, None],
            "THEN since": [pandas.NaT, pandas.Timestamp("2026-01-01"), pandas.NA],
        },
        index=[10, 20, 30],
    )
    table = DecisionTable.from_dataframe(frame, hit_policy="priority")
    assert dumps(table, format="csv") == (
        "HIT POLICY,priority\n"
        "RULE,PRIORITY,customer.years,THEN discount,THEN since\n"
        "new,2,< 1,0e0,\n"
        'vip,,> 5,0.15e0,"DATETIME ""2026-01-01T00:00:00"""\n'
        ",1,-,,\n"
    )
    with pytest.raises(TypeError, match="column names"):
        DecisionTable.from_dataframe(pandas.DataFrame({0: ["> 1"]}))
    with pytest.raises(TypeError, match="pandas or polars"):
        DecisionTable.from_dataframe([["x"], ["> 1"]])


class _PolarsLike:
    """What `from_dataframe` reads of a polars frame, without polars."""

    __module__ = "polars.dataframe.frame"

    columns = ("RULE", "x", "THEN y")

    def iter_rows(self) -> Iterator[tuple[Any, ...]]:
        yield ("a", "> 1", 1)
        yield (None, None, float("nan"))


def test_a_polars_frame_is_read_by_its_rows() -> None:
    table = DecisionTable.from_dataframe(_PolarsLike())
    assert dumps(table, format="csv") == "HIT POLICY,first\nRULE,x,THEN y\na,> 1,1\n,-,\n"


def test_the_python_example_builds_the_csv_example() -> None:
    import runpy

    example = runpy.run_path(str(EXAMPLE.with_name("risk_table.py")))
    expected = EXAMPLE.read_text(encoding="utf-8")
    assert dumps(example["from_rows"](), format="csv") == expected
    pytest.importorskip("pandas")
    assert dumps(example["from_dataframe"](), format="csv") == expected


def test_a_cell_text_has_the_text_size_limit() -> None:
    with pytest.raises(FormatError) as info:
        DecisionTable(["x"], [["> 1000000"]], max_size=5)
    assert (info.value.code, info.value.pointer) == ("limit_exceeded", "/rows/0/0")
    assert DecisionTable(["x"], [["> 1000000"]], max_size=9).rows[0].inputs[0] is not None
