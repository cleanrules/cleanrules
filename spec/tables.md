# Decision tables

**Status:** draft, normative for the core and every binding.

A decision table writes a rule set as a grid, the way DMN and spreadsheets do:
one column per input, one per output, and one row per rule.

| RULE | customer.years | order.total | THEN discount | THEN label |
|---|---|---|---|---|
| vip | > 5 | > 1000 | 0.15 | 'VIP' |
| loyal | > 2 | - | 0.05 | 'loyal' |

A table is a model of its own: it is read and written as a table (as a JSON or
YAML document and as CSV, below), and it **compiles** into an ordinary rule set
(`rules.md`), which is what evaluates, explains and summarizes. Nothing about a
table is evaluated in any other way.

## Model

A decision table has:

- a **hit policy**: one of the policies of `rules.md` (`first`, `priority`,
  `unique`, `any`, `rule_order`, `collect`), or `output_order`; an
  **aggregate** under `collect`; a **logic** and **strictness**, as a rule set;
- **inputs**: one or more value expressions in the text syntax (`text.md`),
  such as `customer.years` or `order.total * 1.21`. A condition is not an input:
  `x > 5` as an input is an `expression_error`; write the column `x` and the
  cells `> 5`;
- **outputs**: zero or more payload keys, each optionally with its **output
  values**, a list of distinct scalars other than null, in priority order, the
  first the highest (DMN's output values). The keys are distinct;
- **rows**, in order: each with an optional **name** (not empty), an optional
  **priority** (an int), an optional **description** (an empty one is none),
  one **input cell** per input and one **output cell** per output.

### Input cells

An input cell tests its column's expression, which the cell leaves out. It is
written in the text syntax (`text.md`), as the part of a predicate that follows
its operand:

| Cell | Means, for the column `x` |
|---|---|
| `-`, or nothing | any value: the cell adds no condition |
| `> 5`, `<= order.limit`, `!= 'ES'` (any comparison) | `x > 5`, … |
| `IN ('ES', 'PT')`, `NOT IN (…)` | `x IN (…)`, `NOT x IN (…)` |
| `BETWEEN 1 AND 10 [CLOSED …]`, `NOT BETWEEN …` | `x BETWEEN 1 AND 10`, … |
| `IS NULL`, `IS NOT NULL` | `x IS NULL`, `NOT x IS NULL` |
| `MATCHES '…'`, `STARTS WITH '…'`, `ENDS WITH '…'`, `CONTAINS '…'`, each also after `NOT` | the string tests |
| a value expression: `'ES'`, `5`, `TRUE`, `DATE "2026-01-01"`, `order.limit` | equality: `x = 'ES'` |

Whitespace around a cell is ignored. The node a cell makes is the one the text
syntax makes for the whole predicate, with the column's expression as its
operand, under the construction rules of `ast.md`: `IN (1, 'a')` is an
`expression_error`.

### Output cells

An output cell is a value expression in the text syntax (`0.15`, `'VIP'`,
`order.total * 0.1`), or nothing, in which case the row's payload has no value
for that key. A condition is not an output value (`rules.md`). In a column with
output values, every cell holds a constant equal to one of them (equality as in
`rules.md`, *`ANY`*); anything else is a `rule_definition_error`.

## Compiling

A table compiles into a rule set with the table's aggregate, logic and
strictness, and one rule per row, in row order unless the policy is
`output_order`:

- the rule's **name** is the row's, or `row_<n>` for the `n`-th row, from 1.
  Names must be unique, as in any rule set (`rule_definition_error`);
- its **condition** is the conjunction (`and`) of the conditions of the input
  cells that are not `-`, in input order; one alone is itself; with none, the
  condition is `TRUE`;
- its **payload** has one value per output cell that is not empty, in output
  order;
- its **priority** and **description** are the row's (priority 0 when it has
  none), except under `priority` and `output_order` with output values, below.

### Output priority

The `priority` and `output_order` policies use the **output priority** of each
row:

- When at least one output has output values, a row's output priority is the
  list of the positions of its cells in their output's values, in output order
  (outputs without values are skipped). A row ranks above another when its list
  is smaller, compared position by position (DMN: the first output decides, the
  next breaks ties). A row then may not have a priority of its own
  (`rule_definition_error`), because the table would mean two things.
- Otherwise each row's priority (0 when it has none) is its output priority,
  higher first.

Under `priority`, the rule set's policy is `priority`. When outputs have values,
each rule's priority is `k − r`, where `k` is the number of distinct output
priorities and `r` the rank of the row's (0 for the highest), so equal rows get
equal priorities and ties go to rule order, as `rules.md` says. Under
`output_order`, the rows are sorted by output priority, highest first, keeping
row order between equals (a stable sort), and the rule set's policy is
`rule_order`: every row that fires is selected, in output priority order. Its
rules keep the priorities they would have under `priority`, so a summary shows
the ranking. With
neither output values nor row priorities, both policies give every row the same
priority, which is allowed: the table is then read as in row order.

## JSON

A table is a document with the content key `"decision_table"`
(`json-format.md`, *Documents*):

```json
{
  "version": 1,
  "decision_table": {
    "hit_policy": "priority",
    "logic": "boolean",
    "inputs": ["customer.years", "order.total"],
    "outputs": [
      { "name": "discount" },
      { "name": "risk", "values": ["high", "medium", "low"] }
    ],
    "rows": [
      { "name": "vip", "inputs": ["> 5", "> 1000"], "outputs": ["0.15", "'low'"] },
      { "inputs": ["-", "> 5000"], "outputs": [null, "'medium'"] }
    ]
  }
}
```

| Key | Required | JSON type | Default |
|---|---|---|---|
| `hit_policy` | no | one of `"first"`, `"priority"`, `"unique"`, `"any"`, `"rule_order"`, `"output_order"`, `"collect"` | `"first"` |
| `aggregate` | no | one of `"sum"`, `"min"`, `"max"`, `"count"`, only with `"collect"` | none |
| `logic` | no | string: the name of a logic | `"boolean"` |
| `strict` | no | boolean | `false` |
| `inputs` | yes | non-empty array of strings: value expressions in the text syntax | |
| `outputs` | yes | array of objects `{"name": <string>, "values": [<value>, …]}`; `values` is optional, its items are constants in the JSON encoding of values | |
| `rows` | yes | array of row objects | |

A row object has `inputs` (an array of strings, one per input: the input cells)
and `outputs` (an array, one per output, of strings or `null`: the output cells),
both required, and `name` (string), `priority` (int) and `description`
(string), all optional. An input cell may be `"-"` or `""`; an empty output cell
is `null`.

Readers check the shape first, in pre-order, as for every document, including
the number of cells of each row and that no output value is null
(`format_error`). Then the table, in this order: the inputs, then the outputs
(with their values), then each row's cells in row and column order, then the
logic, then the rest of *Model* and the rules of *Compiling*. Errors
point to what is wrong, such as `/decision_table/rows/1/inputs/0`; for an error
in the text of a cell or a header, the message gives its column within the
text. Writers write the keys in the order of the tables above, `name`,
`priority` and `description` of a row only when set, `values` only when not
empty, input cells in their canonical text (*Canonical text*) and `-` for any
value, and output cells in their canonical text or `null`.

## CSV

A table is also a CSV text ([RFC 4180](https://www.rfc-editor.org/rfc/rfc4180)),
which spreadsheets read and write. Table files have the suffix `.csv`.

```text
HIT POLICY,priority
RULE,customer.years,order.total,THEN discount,"THEN risk IN ('high', 'medium', 'low')"
vip,> 5,> 1000,0.15,'low'
,-,> 5000,,'medium'
```

- **Fields** are separated by commas, semicolons or tabs, whichever comes first
  outside double quotes and parentheses, in the first line that has one:
  spreadsheets in many languages write semicolons. A field may be enclosed in double quotes, inside
  which a double quote is written twice and the separator and line breaks are
  text. Lines end with `\r\n`, `\n` or `\r`, a byte order mark may start the
  text, and lines whose fields are all empty or space are skipped.
- **Settings**: the text may start with lines whose first field is a keyword,
  and whose other fields after the second are empty; keywords and values are
  case-insensitive:
  `HIT POLICY` with the policy in the second field, `AGGREGATE` with the
  aggregate, `LOGIC` with the logic, and `STRICT` alone. A policy may also be
  written as DMN's letter: `U` (`unique`), `A` (`any`), `P` (`priority`),
  `F` (`first`), `R` (`rule_order`), `O` (`output_order`), `C` (`collect`), and
  `C+`, `C<`, `C>`, `C#` for `collect` with the aggregate `sum`, `min`, `max`
  or `count`. A setting given twice, or an aggregate given both by `AGGREGATE`
  and by a DMN letter, is a `format_error`.
- Fields follow RFC 4180 strictly: a field holding the separator, such as
  `IN ('ES', 'PT')` or an output header with values in a comma-separated file,
  is quoted (`"IN ('ES', 'PT')"`), as spreadsheets do when they save.
- A field that starts with a double quote is a quoted CSV field, so a string in
  a cell is best written between single quotes (`'ES'`): `"ES"` alone is the
  CSV field `ES`, the field named `ES`. Writers quote such fields (`"""ES"""`).
- **Header**: the next line names the columns, and empty fields at its end are
  ignored. `RULE`, `PRIORITY` and
  `DESCRIPTION` (case-insensitive, each at most once) hold the rows' names,
  priorities and descriptions. A field that starts with `THEN` is an output:
  `THEN <key>`, or `THEN <key> IN (<constant>, …)` with its output values, where
  the key is a name or a string. Any other field is an input, a value expression.
- **Rows**: every other line is a row. A row may have fewer fields than the
  header (spreadsheets drop empty trailing fields), which are then empty, and
  more only when the extra fields are empty. Names and descriptions are taken without the space around them,
  unless they are quoted; a priority is an int, or empty.

Errors are reported in text order, the inputs and outputs before any row, and
the rules of *Compiling* last. They give the line and column (1-based, in code
points) of the field, or of the position in it when the field is not quoted and
the error is within its text: CSV that is not well formed and a misplaced or repeated setting or column
are `format_error`s; the text of a header or a cell follows `text.md`, *Errors
and limits*. A CSV text has the size limit of texts (`text.md`: 4 MiB by
default), and the limits below.

Writers write `HIT POLICY` with the policy's name, then `AGGREGATE` when there is
one, `LOGIC` when it is not `boolean` and `STRICT` when set, each on its own
line; then the header, with `RULE` first when a row has a name, `PRIORITY` next
when a row has a priority, the inputs, the outputs, and `DESCRIPTION` last when a
row has a description; then the rows. Fields are separated by commas and
enclosed in double quotes only when they hold a comma, a double quote, a line
break, or space at either end. Every line ends with `\n`.

## Grids

A table can also be built from a **grid**: a header and rows of cells, as a
spreadsheet or a data frame holds them in memory. A grid is the CSV of a table
without its text layer. The settings are given apart, as in JSON (hit policy,
aggregate, logic and strictness), and each cell is text, read as a CSV field,
or a value. Bindings read their language's strings as text (in Erlang,
binaries; in Go, values of type `string` itself), so `'VIP'` is a string and
`VIP` the field named `VIP`, as in the CSV file. An enumeration constant is a
value, never text: Java's enum constants and Scala's enum cases are their
name, and a value of a Go named string type (`type Level string`, Go's
enums) is its string, so `Level("high")` is the string `high`, not the field
`high`. A data frame keeps the types its library gave each column, so it
builds the table of the CSV file it was read from only when it was read as
text: a library that reads `0.15` as a float makes it a float, where CSV reads
a decimal.

- **Header**: one item per column. A text item is read as a CSV header field
  (`RULE`, `PRIORITY`, `DESCRIPTION`, `THEN <key> [IN (…)]`, or an input
  expression); an empty one is a `format_error`. A host language may also give
  an input as an expression it built, and an output as a key with its output
  values.
- **Rows**: each a list of cells in column order. A row may have fewer cells
  than the header, which are then empty, and more only when the extra cells
  are empty.
- **Cells**: text is read as an unquoted CSV field: `-`, `> 5`, `'VIP'`, names
  and descriptions without the space around them, and nothing or only space
  for an empty cell. Null is an empty cell (in JavaScript, `null`, `undefined`
  and holes; in Java, `null` and `Optional.empty()`; in Go, `nil` and a nil
  pointer, except a nil `*Expr`, which is an `expression_error` as everywhere
  else in Go). Any other value `v` is, under an input `x`, the condition
  `x = v`; under an output, the value `v`; under `RULE` or `DESCRIPTION`, a
  string value is the name or description (its text, without the space around
  it); and under `PRIORITY`, an int, or a float with an integral value (data
  frames store ints with missing values as floats), is the priority. Any other
  value under `RULE`, `PRIORITY` or `DESCRIPTION` is a `format_error`; under
  `PRIORITY` that includes a string value, even one that spells a number,
  since only text is read as a number. So an enumeration constant (a string
  value) is a constant under an input or an output, a name or description
  under `RULE` or `DESCRIPTION`, and an error under `PRIORITY`; the core
  decides by the column, and the C ABI takes such cells with their kind
  (`cr_grid_push_row_cells`, `CR_CELL_VALUE`). A host language may also
  give a cell as a node it built: under an input, the cell's condition, which
  must test its column (*Canonical text*); under an output, its value.

Errors are reported as for CSV: the header first, its columns checked before
any row is read, then each row in order, and the rules of *Compiling* last.
They point into the grid, counting from 0: `/header` for the whole header
(a table without inputs), `/header/<c>` for a column,
`/rows/<r>/<c>` for a cell, `/rows/<r>` for a whole row, and `/aggregate` for
an aggregate without `collect`. The settings are not part of the grid: an
unknown logic, policy or aggregate has no pointer. For an error in the text of a header or a cell, the message
gives its column within the text, as in JSON. Each text has the size limit of
texts, and the grid the limits below.

## Limits

Tables have the limits of documents (`json-format.md`, *Errors and limits*):
size, nesting, and the depth of every header and cell. A table also expands as
it is read: every row has a cell per column, even when CSV leaves them out, and
the condition of every input cell that is not `-` holds a copy of its column's
expression. So readers count, as they read, one for every cell and the nodes of
every cell's condition or value, copies included, and stop with
`limit_exceeded` once the count goes over **2,097,152** (2²¹) by default: at the
row (JSON), the row or cell (grids) or the field (CSV) where it does. The limit keeps the memory a table
takes to about that of the largest JSON document; a long input expression
copied into many rows reaches it sooner.

## Canonical text

Headers and cells are written in the canonical text of `text.md` (*Printing*),
with two differences that keep CSV readable: strings are written between single
quotes when they hold no control character (`'VIP'`), and an input cell leaves
out its column's expression: a comparison is written as its operator and right
side (`> 5`), except equality, written as the bare value (`'ES'`); a predicate
keyword is written as in a predicate (`IN ('ES', 'PT')`, `BETWEEN 1 AND 10`,
`IS NULL`), and the negation of an `in`, `between`, `is_null` or string test is
written with `NOT` in the predicate (`NOT IN (…)`, `IS NOT NULL`). An input
cell whose node has none of these forms, or a header or cell with a field key
holding a control character (`text.md`, *Printing*), has no canonical text.
Both are possible only in a table built in a host language, which refuses them
with `unsupported`, so that every table can be written.

## Differences from DMN

- Cells use the text syntax of `text.md`, not FEEL. `[1..10]` is
  `BETWEEN 1 AND 10`, `"a","b"` is `IN ('a', 'b')`, and `not("a")` is `!= 'a'`.
- `PRIORITY` with equal output priorities selects the first such row; DMN
  leaves it to the engine.
- `COLLECT` follows `rules.md` (*Deviations from DMN*): aggregates are per
  output key.
