# JSON format

**Status:** draft, format version `1`. This document covers documents, values
and conditions (task A.1a). Rules and rule sets (A.1b), decisions and reasons
(A.1c), and explanations and summaries (A.1d) are added in later tasks.

The JSON format is the contract between languages and with users' files
(`CLAUDE.md`, invariant 6). A condition written in Java and read in Go is the
same tree, and evaluates the same way. The JSON Schemas in
[`schemas/v1/`](schemas/v1/) help editors and tools check a document, but this
document and [`ast.md`](ast.md) are normative. They define what a schema
cannot express (literals, ranges, duplicate keys, construction rules, limits)
and which error each problem gives.

## Documents

A document is a JSON text ([RFC 8259](https://www.rfc-editor.org/rfc/rfc8259))
encoded as UTF-8 without a byte order mark. Its top level is an object with:

- `"version"`: the format version, the integer literal `1`;
- exactly one **content key**, which says what the document holds. In this
  version the only content key is `"condition"`;
- optionally `"$schema"`, a string that readers ignore, so editors can find the
  schema. Writers do not emit it.

Writers put `version` first and the content key second.

```json
{
  "version": 1,
  "condition": {
    "op": "and",
    "operands": [
      { "op": "gt", "left": { "op": "field", "path": ["customer", "years"] }, "right": { "op": "const", "value": 5 } },
      { "op": "gt", "left": { "op": "field", "path": ["order", "total"] }, "right": { "op": "const", "value": 1000 } }
    ]
  }
}
```

Readers are strict:

- A key they do not know, anywhere, is a `format_error`. Ignoring it could
  silently change what a rule means.
- An object with a **duplicate key** is a `format_error`. JSON parsers disagree
  on which duplicate wins, so accepting one would make readers in different
  languages build different trees.
- Invalid UTF-8, a byte order mark, and a string escape for a lone surrogate
  (such as `"\ud800"`) are `format_error`s. Strings are never
  Unicode-normalized: they are kept exactly as written.

**Versioning.** Adding a node, a content key or an optional key keeps the
version. Any other change (renaming, removing, or changing what an existing key
means) increases it. Optional keys added after version 1 are omitted by writers
when they hold their default, so documents that do not use a new feature stay
readable by older readers. A reader that meets a node or key added after it was
written rejects the document with `format_error`, and the message suggests that
the document may come from a newer cleanrules.

## Values

Constants in conditions, and the data that conditions read, use one encoding.
JSON has no integer, decimal or date types of its own, so each value is either
**bare** JSON, where JSON is unambiguous, or a **typed value**: an object with a
single key that starts with `$`.

| Type | Canonical form | Also accepted |
|---|---|---|
| null | `null` | |
| boolean | `true`, `false` | |
| string | `"text"` | |
| int, \|n\| ≤ 2⁵³ − 1 | `42`, `-7`: a number with no fraction and no exponent | `{"$int": 42}`, `{"$int": "42"}` |
| int, \|n\| > 2⁵³ − 1 | `{"$int": "9223372036854775807"}` | the bare number |
| float, finite | `42.0`, `1e-7`, `-0.0`: a number with a fraction or an exponent | `{"$float": 42}` |
| float, infinite | `{"$float": "inf"}`, `{"$float": "-inf"}` | |
| decimal | `{"$decimal": "0.10"}` | exponent forms such as `{"$decimal": "1.5e-3"}` |
| date | `{"$date": "2026-10-31"}` | |
| datetime | `{"$datetime": "2026-10-31T09:30:00+01:00"}` | |
| list (data only) | `[1, 2]` | |
| map (data only) | `{"a": 1}` | `{"$map": {"a": 1}}` |

Writers always use the canonical form. The other forms exist for writers that
cannot produce it. JavaScript and Go's default decoder lose integers beyond
2⁵³, which is why larger ones are written as text. `JSON.stringify` writes the
float `1.0` as `1`, which reads back as an int: such writers must use
`{"$float": 1}`, because int and float differ in arithmetic overflow and in
structural identity (`ast.md`).

### Numbers

- **int** is a signed 64-bit integer. A bare integer literal is read
  **exactly**, never through a float, and one outside the signed 64-bit range is
  a `format_error`. `-0` is the int `0`. In `{"$int": ...}` the value is an
  integer literal (not `1.0` or `1e2`) or a string matching `-?[0-9]+`, leading
  zeros allowed (`"007"` is 7).
- **float** is an IEEE 754 binary64. A literal is rounded to the nearest float,
  ties to even. A finite literal too large to represent, such as `1e400`, is a
  `format_error`: write `{"$float": "inf"}`. In `{"$float": ...}` any number
  literal is accepted, and `{"$float": -0}` is `-0.0`. NaN has no encoding:
  conditions reject it and data treats it as null (`ast.md`). Writers produce
  the shortest digits that read back as the same float, with a `.` or an
  exponent, and `-0.0` for negative zero. The layout (`100.0` or `1e2`) is not
  part of the contract.
- **decimal** is exact: a sign, a coefficient and an exponent. A reader accepts
  text matching `-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?`, or `inf` or `-inf`.
  The value is never rounded: one outside the precision and range set in
  `data-model.md` (task A.2) is a `format_error`. The **canonical text** is in
  plain notation, `-?(0|[1-9][0-9]*)(\.[0-9]+)?`, with no exponent and no
  leading zeros, and with as many fraction digits as the value's scale (the
  number of digits after the point, never negative). So `"1.00"` stays
  `"1.00"`, `"007.50"` becomes `"7.50"`, `"1.5e-3"` becomes `"0.0015"`, and
  `"1e2"` and `"1.5e3"` become `"100"` and `"1500"`. A zero is written without
  a sign (`"-0.00"` becomes `"0.00"`). `"1.00"` and `"1"` are different text
  for the same value and the same node (`ast.md`, *Structural identity*).

### Dates and datetimes

- **date**: `YYYY-MM-DD`, a valid day in the proleptic Gregorian calendar, years
  `0001` to `9999`.
- **datetime**: `YYYY-MM-DDTHH:MM:SS`, with an uppercase `T`, optionally
  followed by a fraction of one to six digits (microseconds; more digits are a
  `format_error`). Leap seconds (`:60`) are rejected. What follows says whether
  it is naive or aware:
  - nothing: a **naive** datetime;
  - an uppercase `Z`, or an offset `±HH:MM` or `±HH:MM:SS` of at most 18 hours:
    an **aware** datetime with that fixed UTC offset. `-00:00` is read as `Z`;
  - an offset (never `Z`) followed by an IANA zone name in brackets, as in
    `2026-10-31T09:30:00+01:00[Europe/Madrid]`
    ([RFC 9557](https://www.rfc-editor.org/rfc/rfc9557)): an aware datetime in
    that zone. The offset must be the zone's offset at that instant, which
    also picks the right time when a clock change repeats an hour.

  An aware datetime whose UTC instant falls outside years 0001 to 9999 is a
  `format_error`.

**Time zones.** Zone names are case-sensitive and kept exactly as written,
without resolving aliases (`Europe/Kiev` stays `Europe/Kiev`). An unknown zone
name, a zone whose offset does not match, a critical flag (`[!Europe/Madrid]`)
and RFC 9557 suffix tags (`[u-ca=japanese]`) are `format_error`s. The time zone
database is bundled with the core, not taken from the operating system, so a
document is valid or invalid in the same way on every machine. Its version is
part of each release, the API reports it, and an update is noted in the
changelog.

Writers omit the fraction when it is zero and otherwise write six digits. A
datetime with a fixed offset is written with that offset, adding seconds only
when they are not zero, and a zero offset as `Z`. A datetime in a named zone is
written with its numeric offset at that instant (`+00:00`, never `Z`) followed
by the zone name.

### Lists and maps

Lists and maps occur only in data, never as constants in a condition. A map's
keys are strings, kept in document order.

A JSON object with a key that starts with `$` is never a map: it is a typed
value, and that must be its only key. The types in the table above are the only
ones defined, and any other `$`-key is a `format_error`, so new types can be
added later without changing what existing documents mean. A map with a key
that starts with `$` is written inside `{"$map": ...}`. For example,
`{"$map": {"$ref": 1}}` is the map with the single key `$ref`. Writers use
`$map` only in that case.

## Conditions

A condition node is a JSON object with an `"op"` key, which holds the node's
tag (`ast.md`, *Catalogue*), and one key per field. The table gives each key's
JSON type. A missing key, an unknown key, an unknown `op` or a wrong JSON type
is a `format_error`.

| `op` | Keys | JSON types |
|---|---|---|
| `field` | `path` | array of strings and integer literals |
| `const` | `value` | a scalar value: null, boolean, string, int, float, decimal, date or datetime |
| `eq` `ne` `lt` `le` `gt` `ge` | `left`, `right` | nodes |
| `and` `or` | `operands` | array of nodes |
| `not` | `operand` | node |
| `in` | `operand`, `values` | node; array of scalar values |
| `between` | `operand`, `low`, `high`, `closed` (optional) | nodes; one of the strings `"both"`, `"left"`, `"right"` and `"none"`, `"both"` when omitted |
| `is_null` | `operand` | node |
| `matches` | `operand`, `pattern` | node; string |
| `starts_with` `ends_with` `contains` | `operand`, `text` | node; string |
| `add` `sub` `mul` `div` | `left`, `right` | nodes |
| `abs` `neg` | `operand` | node |

A path segment that is a string looks up a key, and one that is an integer
indexes a list, as described in `ast.md` (*Reading data*). `["items", 0,
"price"]` is `F.items[0].price`. An index is an integer literal in the int
range; one larger than the platform can address (on 32-bit WebAssembly, for
example) is simply missing when evaluated.

### Reading

Once the shape is right, a reader builds the tree with the construction rules
of `ast.md`, exactly as if a binding had built it. So a document can never hold
a tree that the API could not build. Breaking any of these rules is an
`expression_error`:

- the path rules: non-empty, no empty string segment, no negative index;
- node kinds: a condition is not a value operand, and a value is not an
  operand of `and`, `or` or `not`, nor a document's condition;
- `const` with `null` only as the operand of `is_null`;
- arithmetic only accepts number constants;
- `and` and `or` need at least two operands **as written**. A nested `and`
  inside an `and` (or `or` inside `or`) is then flattened, so
  `{"op": "and", "operands": [{"op": "and", ...}]}` is rejected although the
  API accepts `And(And(a, b))`;
- `in` values are non-empty, non-null and from one type group. They are then
  normalized, sorted and deduplicated;
- `between` bounds, when both are constants, belong to one orderable group;
- a `matches` pattern is a valid regular expression (`ast.md`, *`matches`*).

The JSON Schemas check most of these rules too, so editors can flag them. That
does not change their code, which is always `expression_error`. Messages come
from the core and are worded the same in every language.

### Writing

Writers produce the **canonical form**:

- keys in the order of the table above, `op` first;
- `closed` always written, even when it is `"both"`;
- `and` and `or` flattened;
- `in` values in normalized order: ascending, numbers by exact value, strings by
  Unicode code point, `false` before `true`, dates and datetimes in time order
  (aware ones by instant). Equal numbers of different types are all kept, with
  decimal before float before int. Of values that are the same node (`ast.md`,
  *Structural identity*), such as `0.0` and `-0.0`, or two aware datetimes for
  the same instant, the first in input order is kept;
- values in their canonical form.

**Comparing documents.** Conformance vectors compare documents as parsed JSON
trees, never as text. Object members compare in order. Numbers compare by
lexical class (integer literal, or literal with a fraction or exponent) and
exact value, so `1` and `1.0` differ and `100.0` equals `1e2`. Whitespace and
number layout are not part of the contract.

## Errors and limits

Every error carries a stable code and a location. A syntax error gives a 1-based
line and column. Every other error gives a JSON Pointer
([RFC 6901](https://www.rfc-editor.org/rfc/rfc6901)) into the document as
written, before any sorting or flattening, such as
`/condition/operands/1/right/value`.

| Code | Meaning |
|---|---|
| `format_error` | not valid JSON or UTF-8, a duplicate, unknown or missing key, a wrong JSON type, an unknown `op` or `$`-key, or a malformed value |
| `unsupported_version` | `version` is missing, is not an integer literal, or is not a version the reader knows |
| `expression_error` | the shape is valid, but a construction rule (see *Reading*) is broken |
| `limit_exceeded` | the document is larger, or nested deeper, than the reader allows |

A reader reports **one** error, the first it finds in this order:

1. size, JSON nesting and syntax (duplicate keys included), in text order;
2. `version`, before any other key, so a document from a newer format version
   gets `unsupported_version` even if it uses new keys;
3. shape (`format_error`), depth first, with an object's keys in the order of
   the table above, whatever their order in the text;
4. construction rules (`expression_error`) and node depth, in depth-first
   post-order: children before their parent, operands in order.

Readers enforce three limits, so a hostile file cannot exhaust memory or the
stack (`CLAUDE.md`, invariant 9):

- **document size**: at most **16 MiB** of JSON text;
- **JSON nesting**: arrays and objects at most **1024** deep, checked while
  parsing, before any tree is built. This also bounds lists and maps in data;
- **node depth**: a condition at most **256** nodes deep, counting the root as
  depth 1 and counting before `and`/`or` flattening.

A condition 256 nodes deep needs well under 1024 levels of JSON nesting, so
every condition within the node limit can be read. The caller can raise the
limits, but never disable them. A reader must not overflow its stack on any
input within its limits.
