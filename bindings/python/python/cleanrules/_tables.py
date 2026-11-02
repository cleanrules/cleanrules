"""Decision tables (`spec/tables.md`): a grid of inputs, outputs and rows.

    table = cleanrules.load("pricing.csv")
    decision = table.evaluate({"customer": {"years": 9}, "order": {"total": 1500}})

A table is read and written as CSV, JSON or YAML, and compiles into an ordinary
`RuleSet` (`table.ruleset`), which evaluates, explains and summarizes it. The
core reads, checks and compiles tables, so a table means the same in every
language.
"""

from __future__ import annotations

import enum
from collections.abc import Iterable
from dataclasses import dataclass
from typing import Any, NoReturn

from cleanrules import _cleanrules as _native
from cleanrules._expr import Expr, _wrap  # pyright: ignore[reportPrivateUsage]
from cleanrules._rules import Aggregate, Decision, RuleSet, Summary

__all__ = ["DecisionTable", "TableOutput", "TablePolicy", "TableRow"]


class TablePolicy(enum.StrEnum):
    """A decision table's hit policy: those of `HitPolicy` and `OUTPUT_ORDER`."""

    FIRST = "first"
    """The first row that fires."""
    PRIORITY = "priority"
    """The row with the highest output priority; ties go to row order."""
    UNIQUE = "unique"
    """The only row that fires; more than one is a `HitPolicyError`."""
    ANY = "any"
    """The first row that fires; the rows that fire must agree on their outputs."""
    RULE_ORDER = "rule_order"
    """Every row that fires, in row order."""
    OUTPUT_ORDER = "output_order"
    """Every row that fires, in output priority order (DMN's O)."""
    COLLECT = "collect"
    """Every row that fires, in row order, optionally combined by an aggregate."""


@dataclass(frozen=True, slots=True)
class TableOutput:
    """An output column: its payload key and its output values, highest priority first."""

    name: str
    values: tuple[object, ...]


@dataclass(frozen=True, slots=True, eq=False)
class TableRow:
    """A row of a table: one rule.

    Attributes:
        name: The rule's name, or `None` for `row_<n>`.
        priority: The rule's own priority, if any.
        description: The rule's description, if any.
        inputs: One condition per input, testing that input; `None` for any value.
        outputs: One value per output; `None` for no value.
    """

    name: str | None
    priority: int | None
    description: str | None
    inputs: tuple[Expr | None, ...]
    outputs: tuple[Expr | None, ...]


class DecisionTable:
    """A decision table and the rule set it compiles into (`spec/tables.md`).

    Read one with `cleanrules.load("table.csv")` or `cleanrules.loads(text,
    format="csv")`; write it with `dump` and `dumps` as CSV, JSON or YAML.
    Tables are immutable and pickle as their JSON document.

    Attributes:
        hit_policy: The hit policy.
        aggregate: The `COLLECT` aggregate, if any.
        logic: The logic's name.
        strict: Whether evaluation is strict.
        inputs: The input expressions, in column order.
        outputs: The output columns.
    """

    __slots__ = (
        "_native",
        "_rows",
        "_ruleset",
        "aggregate",
        "hit_policy",
        "inputs",
        "logic",
        "outputs",
        "strict",
    )

    hit_policy: TablePolicy
    aggregate: Aggregate | None
    logic: str
    strict: bool
    inputs: tuple[Expr, ...]
    outputs: tuple[TableOutput, ...]
    _rows: tuple[TableRow, ...] | None
    _ruleset: RuleSet | None

    def __init__(self) -> None:
        raise TypeError("decision tables are read with cleanrules.load or cleanrules.loads")

    @classmethod
    def _from_native(cls, native: _native.DecisionTable) -> DecisionTable:
        table = object.__new__(cls)
        set_ = object.__setattr__
        set_(table, "_native", native)
        hit_policy, aggregate, logic, strict = native.config()
        set_(table, "hit_policy", TablePolicy(hit_policy))
        set_(table, "aggregate", None if aggregate is None else Aggregate(aggregate))
        set_(table, "logic", logic or "boolean")
        set_(table, "strict", strict)
        set_(table, "inputs", tuple(_wrap(node) for node in native.inputs()))
        set_(table, "outputs", tuple(TableOutput(n, v) for n, v in native.outputs()))
        # Rows and the rule set are built when first asked for: a table may be
        # large, and read only to be written again.
        set_(table, "_rows", None)
        set_(table, "_ruleset", None)
        return table

    @property
    def rows(self) -> tuple[TableRow, ...]:
        """The rows, in order."""
        if self._rows is None:

            def cells(nodes: list[_native.Node | None]) -> tuple[Expr | None, ...]:
                return tuple(None if node is None else _wrap(node) for node in nodes)

            rows = tuple(
                TableRow(name, priority, description, cells(inputs), cells(outputs))
                for name, priority, description, inputs, outputs in self._native.rows()
            )
            object.__setattr__(self, "_rows", rows)
            return rows
        return self._rows

    @property
    def ruleset(self) -> RuleSet:
        """The rule set the table compiles into: one rule per row (`spec/tables.md`)."""
        if self._ruleset is None:
            from cleanrules._documents import _ruleset  # pyright: ignore[reportPrivateUsage]

            ruleset = _ruleset(self._native.compiled())
            object.__setattr__(self, "_ruleset", ruleset)
            return ruleset
        return self._ruleset

    def evaluate(self, data: object) -> Decision:
        """Evaluate one input with the compiled rule set (`RuleSet.evaluate`)."""
        return self.ruleset.evaluate(data)

    def evaluate_many(self, rows: Iterable[object]) -> list[Decision]:
        """Evaluate a batch with the compiled rule set (`RuleSet.evaluate_many`)."""
        return self.ruleset.evaluate_many(rows)

    def summary(self) -> Summary:
        """The summary of the compiled rule set (`RuleSet.summary`)."""
        return self.ruleset.summary()

    def __setattr__(self, name: str, value: object) -> NoReturn:
        raise AttributeError(f"decision tables are immutable: cannot set {name!r}")

    def __delattr__(self, name: str) -> NoReturn:
        raise AttributeError(f"decision tables are immutable: cannot delete {name!r}")

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        from cleanrules._documents import loads

        return (loads, (_native.dump_table(self._native, "json"),))

    def __repr__(self) -> str:
        columns = len(self.inputs) + len(self.outputs)
        rows = self._native.row_count()
        return f"<DecisionTable {self.hit_policy.value}: {rows} rows, {columns} columns>"
