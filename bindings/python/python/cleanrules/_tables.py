"""Decision tables (`spec/tables.md`): a grid of inputs, outputs and rows.

    table = cleanrules.load("pricing.csv")
    table = DecisionTable(
        ["RULE", "customer.years", "order.total", "THEN discount"],
        [["vip", "> 5", "> 1000", 0.15], ["loyal", "> 2", "-", 0.05]],
    )
    decision = table.evaluate({"customer": {"years": 9}, "order": {"total": 1500}})

A table is read and written as CSV, JSON or YAML, built from rows or from a
data frame, and compiles into an ordinary `RuleSet` (`table.ruleset`), which
evaluates, explains and summarizes it. The core reads, checks and compiles
tables, so a table means the same in every language.
"""

from __future__ import annotations

import enum
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from typing import Any, NoReturn, TypeAlias

from cleanrules import _cleanrules as _native
from cleanrules._expr import Expr, _wrap  # pyright: ignore[reportPrivateUsage]
from cleanrules._rules import Aggregate, Decision, RuleSet, Summary
from cleanrules.errors import RuleDefinitionError

__all__ = ["DecisionTable", "HeaderItem", "TableOutput", "TablePolicy", "TableRow"]


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
    """An output column: its payload key and its output values, highest priority first.

    In a header, `TableOutput("risk", ("high", "low"))` is the column
    `THEN risk IN ('high', 'low')`: the values are Python constants, not text.
    """

    name: str
    values: tuple[object, ...] = ()


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


HeaderItem: TypeAlias = str | Expr | TableOutput
"""A column of a table's header: its CSV text, an input `Expr`, or a `TableOutput`."""


class DecisionTable:
    """A decision table and the rule set it compiles into (`spec/tables.md`).

    Build one from a header and rows, as a spreadsheet holds it:

        table = DecisionTable(
            ["RULE", "customer.years", "order.total", "THEN discount", "THEN label"],
            [
                ["vip", "> 5", "> 1000", 0.15, "'VIP'"],
                ["loyal", "> 2", "-", 0.05, "'loyal'"],
            ],
        )

    Each header item is the text of a CSV header field (`"RULE"`,
    `"PRIORITY"`, `"DESCRIPTION"`, `"THEN key"`, `"THEN key IN (...)"` or an
    input expression), an input `Expr`, or a `TableOutput`. A `str` cell is
    read as in the table's CSV, in the text syntax: `"> 5"`, `"-"`, and
    `"'VIP'"` for the string, since `"VIP"` is the field named `VIP`. So under
    an input `country`, the cell `"ES"` is the test `country = ES`, a
    comparison with the field `ES`: write `"'ES'"`, or `IN ('ES', 'PT')`. Any
    other cell is a value: under an input `x`, the test `x == value`; under an
    output, the value; `None` is an empty cell. An `Expr` cell is an input's
    condition, which tests its column (`F.age < 18` under `age`), or an
    output's value. Rows may be shorter than the header.

    Or read one with `cleanrules.load("table.csv")`, `cleanrules.loads(text,
    format="csv")` or `DecisionTable.from_dataframe(frame)`; write it with
    `dump` and `dumps` as CSV, JSON or YAML. Tables are immutable and pickle as
    their JSON document.

    Args:
        header: The columns.
        rows: The rows, each a sequence of cells in column order; read one at
            a time.
        hit_policy: A `TablePolicy` or its name.
        aggregate: With `COLLECT`, `"sum"`, `"min"`, `"max"` or `"count"`.
        logic: The logic's name; `"boolean"` by default.
        strict: Raise instead of returning unknown when evaluating.
        max_size: The longest text of a header or cell, in bytes (4 MiB).
        max_depth: The deepest header or cell, in nodes (256).
        max_table_size: The most cells and condition nodes the table may hold,
            column copies included (2,097,152; `spec/tables.md`, *Limits*).

    Raises:
        FormatError: For an empty or repeated column, a row with more cells
            than the header, or a name, priority or description that is not
            one.
        RuleSyntaxError: For a header or cell whose text does not follow the
            syntax.
        ExpressionError: For an input that is a condition, or a cell that
            breaks a construction rule.
        RuleDefinitionError: For repeated output keys or rule names, an output
            cell that is not one of its column's values, an unknown hit policy
            or aggregate, and the other rules of `spec/tables.md`.
        UnsupportedError: For an `Expr` cell that does not test its column.
        RegistryError: For an unknown logic.

    Errors carry a `pointer` into the grid: `/header/<c>` or `/rows/<r>/<c>`,
    counting from 0.

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

    def __init__(
        self,
        header: Sequence[HeaderItem],
        rows: Iterable[Sequence[object]],
        *,
        hit_policy: TablePolicy | str = TablePolicy.FIRST,
        aggregate: Aggregate | str | None = None,
        logic: str = "boolean",
        strict: bool = False,
        max_size: int | None = None,
        max_depth: int | None = None,
        max_table_size: int | None = None,
    ) -> None:
        native = _grid(
            header,
            rows,
            hit_policy=hit_policy,
            aggregate=aggregate,
            logic=logic,
            strict=strict,
            max_size=max_size,
            max_depth=max_depth,
            max_table_size=max_table_size,
        )
        self._set(native)

    @classmethod
    def from_dataframe(
        cls,
        frame: Any,
        *,
        hit_policy: TablePolicy | str = TablePolicy.FIRST,
        aggregate: Aggregate | str | None = None,
        logic: str = "boolean",
        strict: bool = False,
        max_size: int | None = None,
        max_depth: int | None = None,
        max_table_size: int | None = None,
    ) -> DecisionTable:
        """Build a table from a pandas or polars `DataFrame`.

            frame = pandas.read_excel("pricing.xlsx", dtype=str, keep_default_na=False)
            table = DecisionTable.from_dataframe(frame, hit_policy="priority")

        The column names are the header and each row a row of cells, read as
        `DecisionTable` reads them: a `str` cell is text (`"'VIP'"` for the
        string), any other a value. Missing values (`None`, NaN, `pandas.NA`,
        `NaT`) are empty cells, and the index is ignored. Neither library is
        imported: the frame is read through its own methods.

        A frame keeps the types its library gave each column, so it builds the
        table of its CSV file only when read as text: pandas and polars read
        `0.15` as a float (a decimal in the CSV) and a `RULE` column of digits
        as ints, which are not names. Read table files with `dtype=str,
        keep_default_na=False` (pandas) or `infer_schema=False` (polars), or
        with `cleanrules.load`.

        Args:
            frame: A `pandas.DataFrame` or a `polars.DataFrame`.
            hit_policy: As `DecisionTable`.
            aggregate: As `DecisionTable`.
            logic: As `DecisionTable`.
            strict: As `DecisionTable`.
            max_size: As `DecisionTable`.
            max_depth: As `DecisionTable`.
            max_table_size: As `DecisionTable`.

        Raises:
            TypeError: For a frame of another kind, or a column whose name is
                not a `str`.
            CleanRulesError: As `DecisionTable`.
        """
        library = type(frame).__module__.partition(".")[0]
        if library not in ("pandas", "polars") or not hasattr(frame, "columns"):
            raise TypeError(
                f"from_dataframe takes a pandas or polars DataFrame, got {type(frame).__name__}"
            )
        header: list[HeaderItem] = []
        for name in frame.columns:
            if not isinstance(name, str):
                raise TypeError(f"column names are the header's text, got {name!r}")
            header.append(name)
        if library == "pandas":
            rows: Iterable[Sequence[object]] = frame.itertuples(index=False, name=None)
        else:
            rows = frame.iter_rows()
        table = object.__new__(cls)
        table._set(
            _grid(
                header,
                (tuple(_present(cell) for cell in row) for row in rows),
                hit_policy=hit_policy,
                aggregate=aggregate,
                logic=logic,
                strict=strict,
                max_size=max_size,
                max_depth=max_depth,
                max_table_size=max_table_size,
            )
        )
        return table

    @classmethod
    def _from_native(cls, native: _native.DecisionTable) -> DecisionTable:
        table = object.__new__(cls)
        table._set(native)
        return table

    def _set(self, native: _native.DecisionTable) -> None:
        table = self
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

    def evaluate(self, data: object, *, max_values: int | None = None) -> Decision:
        """Evaluate one input with the compiled rule set (`RuleSet.evaluate`).

        Raises:
            FormatError: For an input beyond `max_values` (`limit_exceeded`).
        """
        return self.ruleset.evaluate(data, max_values=max_values)

    def evaluate_many(
        self, rows: Iterable[object], *, max_values: int | None = None
    ) -> list[Decision]:
        """Evaluate a batch with the compiled rule set (`RuleSet.evaluate_many`)."""
        return self.ruleset.evaluate_many(rows, max_values=max_values)

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


def _present(cell: object) -> object:
    """A data frame's cell, with its missing values as `None` (NaN is not equal to itself)."""
    if isinstance(cell, float) and cell != cell:
        return None
    return cell


def _cells(r: int, row: Sequence[object]) -> list[object]:
    """A row's cells for the core: `Expr` cells as their nodes."""
    if isinstance(row, (str, bytes)):
        raise TypeError(f"row {r} is a {type(row).__name__}; a row is a sequence of cells")
    return [cell._node if isinstance(cell, Expr) else cell for cell in row]  # pyright: ignore[reportPrivateUsage]


def _grid(
    header: Sequence[HeaderItem],
    rows: Iterable[Sequence[object]],
    *,
    hit_policy: TablePolicy | str,
    aggregate: Aggregate | str | None,
    logic: str,
    strict: bool,
    max_size: int | None,
    max_depth: int | None,
    max_table_size: int | None,
) -> _native.DecisionTable:
    """Check the settings and hand the grid to the core (`spec/tables.md`, *Grids*)."""
    try:
        policy = TablePolicy(str(hit_policy).lower())
    except ValueError:
        raise RuleDefinitionError(
            f"unknown hit policy {hit_policy!r}",
            hint=f"use one of: {', '.join(p.value for p in TablePolicy)}",
        ) from None
    chosen: Aggregate | None = None
    if aggregate is not None:
        try:
            chosen = Aggregate(str(aggregate).lower())
        except ValueError:
            raise RuleDefinitionError(
                f"unknown aggregate {aggregate!r}", hint="use one of: count, max, min, sum"
            ) from None
    if type(strict) is not bool:
        raise RuleDefinitionError(f"strict must be a bool, got {strict!r}")
    limits = (("max_size", max_size), ("max_depth", max_depth), ("max_table_size", max_table_size))
    for name, limit in limits:
        if limit is not None and limit < 1:
            raise ValueError(f"{name} must be at least 1, got {limit}")
    if isinstance(header, str):
        raise TypeError("the header is a sequence of columns, not a str")
    items: list[object] = []
    for item in header:
        if isinstance(item, Expr):
            items.append(item._node)  # pyright: ignore[reportPrivateUsage]
        elif isinstance(item, TableOutput):
            items.append((item.name, list(item.values)))
        elif isinstance(item, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            items.append(item)
        else:
            raise TypeError(
                "a header item is a str, an Expr or a TableOutput, "
                f"got {type(item).__name__} {item!r}"
            )
    cells = (_cells(r, row) for r, row in enumerate(rows))
    return _native.table_from_grid(
        items,
        cells,
        policy.value,
        None if chosen is None else chosen.value,
        None if logic == "boolean" else logic,
        strict,
        max_size,
        max_depth,
        max_table_size,
    )
