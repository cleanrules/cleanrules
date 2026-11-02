"""The decision table of `risk.csv`, built in Python from rows and from a data frame.

    python docs/examples/risk_table.py

Cells are read as in the CSV file: a `str` is the text syntax, so the string
high is written "'high'"; any other value is a constant, and `None` is an empty
cell. See `spec/tables.md`, *Grids*.
"""

from decimal import Decimal
from pathlib import Path

import cleanrules
from cleanrules import DecisionTable, TableOutput

HEADER = [
    "RULE",
    "customer.years",
    "order.total",
    "THEN discount",
    TableOutput("risk", ("high", "medium", "low")),
    "DESCRIPTION",
]
ROWS = [
    ["new", "< 1", "> 1000", 0, "'high'", "New customers with large orders are checked first"],
    ["vip", "> 5", None, Decimal("0.15"), "'low'", "Long-standing customers"],
    ["big", None, "> 5000", Decimal("0.05"), "'medium'"],
    ["default", None, None, 0, "'low'"],
]


def from_rows() -> DecisionTable:
    """The table from its header and rows."""
    return DecisionTable(HEADER, ROWS, hit_policy="priority")


def from_dataframe() -> DecisionTable:
    """The same table from a pandas DataFrame, whose column names are the header."""
    import pandas

    frame = pandas.DataFrame(
        # A frame's rows all have every column.
        [row + [None] * (len(HEADER) - len(row)) for row in ROWS],
        columns=[
            "RULE",
            "customer.years",
            "order.total",
            "THEN discount",
            "THEN risk IN ('high', 'medium', 'low')",
            "DESCRIPTION",
        ],
    )
    return DecisionTable.from_dataframe(frame, hit_policy="priority")


if __name__ == "__main__":
    table = from_rows()
    print(cleanrules.dumps(table, format="csv"), end="")
    same = Path(__file__).with_name("risk.csv").read_text(encoding="utf-8")
    assert cleanrules.dumps(table, format="csv") == same
    print(table.evaluate({"customer": {"years": 0}, "order": {"total": 2000}}).output)
