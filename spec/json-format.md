# JSON format

**Status:** draft, format version `1`. This document covers documents, values
and conditions (task A.1a), rules and rule sets (A.1b), decisions and reasons
(A.1c), and explanations and summaries (A.1d).

The JSON format is the contract between languages and with users' files
(`CLAUDE.md`, invariant 6). A condition written in Java and read in Go is the
same tree, and evaluates the same way. The JSON Schemas in
[`schemas/v1/`](schemas/v1/) help editors and tools check a document, but this
document and [`ast.md`](ast.md) are normative. They define what a schema
cannot express (literals, ranges, duplicate keys, construction rules, limits)
and which error each problem gives.

## Documents

A document is a JSON text ([RFC 8259](https://www.rfc-editor.org/rfc/rfc8259))
encoded as UTF-8 without a byte order mark. Its top level is an object (any
other value is a `format_error` at the root) with:

- `"version"`: the format version, the integer literal `1`;
- exactly one **content key**, which says what the document holds:
  `"condition"`, `"rule"`, `"ruleset"`, `"decision"`, `"explanation"`,
  `"summary"` or `"decision_table"`;
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

- A document with no content key, or with more than one, is a `format_error`
  at the root (the pointer `""`).
- A key they do not know, anywhere, is a `format_error`. Ignoring it could
  silently change what a rule means.
- An object with a **duplicate key** is a `format_error`. JSON parsers disagree
  on which duplicate wins, so accepting one would make readers in different
  languages build different trees.
- Invalid UTF-8, a byte order mark, and a string escape for a lone surrogate
  (such as `"\ud800"`) are `format_error`s. Strings are never
  Unicode-normalized: they are kept exactly as written.

**Formats.** Documents are also read and written in YAML (*YAML*); the
documents of conditions, rules and rule sets in the rule syntax (`text.md`);
and those of decision tables in CSV (`tables.md`). Decisions are read and
written in JSON or YAML only, and explanations and summaries are written in
JSON, YAML or as their text (`explain.md`). Any other combination is
`unsupported`. Read from bytes, text that is not UTF-8 is a `format_error` at
the line and column of the first invalid byte, in every format.

**A rule set from a document.** Where an API asks for a rule set (the C ABI's
`cr_ruleset_read`, for one), a rule set document gives it, a decision table
gives the rule set it compiles to (`tables.md`), and a rule gives a rule set of
that one rule with the default settings (*Rule sets*: `first`, `boolean`, not
strict). A condition is a `format_error`: it is not a rule.

**Versioning.** Adding a node, a content key or an optional key keeps the
version, and so does accepting a new JSON form for an existing key, as long as
existing documents never use that form. Any other change (renaming, removing,
or changing what an existing key or form means) increases it. Optional keys added after version 1 are omitted by writers
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
| opaque (decisions and explanations only) | `{"$opaque": "bytes"}` | |
| list (data only) | `[1, 2]` | |
| map (data only) | `{"a": 1}` | `{"$map": {"a": 1}}` |

**Data** here means the input of an evaluation, the only place where a value
can be out of range: values recorded in a decision or an explanation were
already read, and metadata are written by the caller.

An **opaque** value (`data-model.md`, *Opaque values*) is a native value read
from the input that has no equivalent. It appears only in decisions and
explanations, written with its type name, which is informative; anywhere else
(a condition, a rule, an evaluation input) it is a `format_error` pointing to
the typed value.

Writers always use the canonical form. The other forms exist for writers that
cannot produce it. JavaScript and Go's default decoder lose integers beyond
2⁵³, which is why larger ones are written as text. `JSON.stringify` writes the
float `1.0` as `1`, which reads back as an int: such writers must use
`{"$float": 1}`, because int and float differ in arithmetic overflow and in
structural identity (`ast.md`).

### Numbers

- **int** is a signed 64-bit integer. A bare integer literal is read
  **exactly**, never through a float. One outside the signed 64-bit range is a
  `format_error` in a constant, and reads as null with reason `int_overflow` in
  data (`data-model.md`), so one bad value never rejects a whole input. `-0` is the int `0`. In `{"$int": ...}` the value is an
  integer literal (not `1.0` or `1e2`) or a string matching `-?[0-9]+`, leading
  zeros allowed (`"007"` is 7).
- **float** is an IEEE 754 binary64. A literal is rounded to the nearest float,
  ties to even, so a literal too small to represent, such as `1e-400`, reads
  as `0.0`. A literal that rounds to infinity, such as `1e400`, is a
  `format_error`: write `{"$float": "inf"}`. In `{"$float": ...}` any number
  literal is accepted, and `{"$float": -0}` is `-0.0`. NaN has no encoding:
  conditions reject it and data treats it as null (`ast.md`). Writers produce
  the shortest digits that read back as the same float, with a `.` or an
  exponent, and `-0.0` for negative zero. The layout (`100.0` or `1e2`) is not
  part of the contract.
- **decimal** is exact: a sign, a coefficient and an exponent. A reader accepts
  text matching `-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?`, or `inf` or `-inf`.
  The value is never rounded. One outside the 38 digits of `data-model.md`
  (*Decimals*) is a `format_error` in a constant, and reads as null with reason
  `decimal_overflow` in data. The **canonical text** is in
  plain notation, `-?(0|[1-9][0-9]*)(\.[0-9]+)?` (or `inf` and `-inf`), with no exponent and no
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
  operand of `and`, `or` or `not`, nor the condition of a `condition` document
  (for a rule's `when`, see *Rules*);
- `const` with `null` only as the operand of `is_null`, or as a whole payload
  value (*Rules*);
- arithmetic only accepts number constants;
- `and` and `or` need at least two operands **as written**. A nested `and`
  inside an `and` (or `or` inside `or`) is then flattened, so
  `{"op": "and", "operands": [{"op": "and", ...}]}` is rejected although the
  API accepts `And(And(a, b))`;
- `in` values are non-empty, non-null and from one type group. They are then
  normalized, sorted and deduplicated;
- `between` bounds, when both are constants, belong to one orderable group;
- a `matches` pattern is a valid regular expression (`ast.md`, *`matches`*).

The error points to the node whose construction fails: for a wrong operand,
the node that takes it (`{"op": "eq", ...}` with a condition as `left` points
to the `eq`, not to the condition), and otherwise the node itself.

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
exact value, so `1` and `1.0` differ and `100.0` equals `1e2`; the sign of a
float zero is compared too, so `-0.0` and `0.0` differ. Whitespace and number
layout are not part of the contract.

## Rules

A rule is an object with these keys (`rules.md`, *Rules*):

| Key | Required | JSON type | Default |
|---|---|---|---|
| `name` | yes | string | |
| `when` | yes | condition node | |
| `then` | no | object: payload key → value node | `{}` |
| `priority` | no | int | `0` |
| `tags` | no | array of strings | `[]` |
| `description` | no | string | none |
| `metadata` | no | object: key → data value | `{}` |
| `do` | no | string, not empty: the name of the rule's action | none |

```json
{
  "version": 1,
  "rule": {
    "name": "vip",
    "when": { "op": "gt", "left": { "op": "field", "path": ["customer", "years"] }, "right": { "op": "const", "value": 5 } },
    "then": { "discount": { "op": "const", "value": { "$decimal": "0.15" } } },
    "priority": 10,
    "tags": ["pricing"]
  }
}
```

- **`then`** is the payload. Each member value is a node of kind value or any,
  written like a condition operand; a `const` may hold `null` here. Keys are
  any strings, taken literally. Member order is significant: it is the order
  of the outputs' keys, and the order in which payload values are evaluated
  (`rules.md`, *When payloads are evaluated*). Readers keep it, and writers
  write it.
- **`priority`** is an int (*Values*), bare or as `$int`.
- **`tags`** are a set: readers drop duplicates, and writers write them sorted
  by Unicode code point.
- **`metadata`** is an object whose keys are taken literally (a key starting
  with `$` is an ordinary key here) and whose member values are data values
  (*Values*). Member order is kept.
- **`do`** names the rule's action, which a binding resolves in its action
  registry (`rules.md`, *Actions*). An action given as code has no name, and
  cannot be written (*Writing errors*).

**Member order.** JSON objects are unordered for many tools: Go's
`map[string]any`, PostgreSQL's `jsonb` and `jq -S` sort their keys, and
JavaScript objects put integer-like keys such as `"10"` first. A tool that
reorders the members of `then` changes the order of a decision's outputs and
which payload error strict mode reports first. Readers and writers of this
format, in every binding, must go through an order-preserving representation,
never through an unordered map.

Reading a rule checks its shape (`format_error`), builds its condition and
payload values with the rules of *Reading* (`expression_error`), and then
applies the rule's own construction rules, which give
`rule_definition_error`, in this order:

1. `name` is not empty (pointer to `…/name`);
2. `when` is not a node of kind value (pointer to `…/when`; a `const` holding
   `null` there is already an `expression_error`);
3. no payload value is a node of kind condition (pointer to `…/then/<key>`,
   with the key escaped as RFC 6901 requires, so `a/b` becomes `a~1b`), in
   member order.

Writers write the keys in the order of the table and omit every optional key
that holds its default.

## Rule sets

A rule set is an object with these keys (`rules.md`, *Evaluation*):

| Key | Required | JSON type | Default |
|---|---|---|---|
| `hit_policy` | no | one of `"first"`, `"priority"`, `"unique"`, `"any"`, `"rule_order"`, `"collect"` | `"first"` |
| `aggregate` | no | one of `"sum"`, `"min"`, `"max"`, `"count"` | none |
| `logic` | no | string: the name of a logic | `"boolean"` |
| `strict` | no | boolean | `false` |
| `rules` | yes | array of rules | |

```json
{
  "version": 1,
  "ruleset": {
    "hit_policy": "priority",
    "logic": "boolean",
    "rules": [
      { "name": "vip", "when": { "op": "field", "path": ["vip"] }, "then": { "discount": { "op": "const", "value": 0.15 } }, "priority": 10 },
      { "name": "default", "when": { "op": "const", "value": true }, "then": { "discount": { "op": "const", "value": 0.0 } } }
    ]
  }
}
```

- Policy and aggregate names are lowercase and matched exactly. Another string
  is a `format_error`.
- `logic` names a logic of the core. This version defines `"boolean"`, Kleene's
  three-valued logic (`ast.md`, *Truth values*), and the fuzzy logics add their
  names in phase I. A name the format does not define is a `registry_error`. A
  name it defines but this build of the core lacks (built without the fuzzy
  feature, for example) is `unsupported`, and the message names the Cargo
  feature.
- `rules` may be empty. Their order is the rule order of `rules.md`.

**Reserved for later versions.** Fuzzy logics take parameters, and graded
logics an activation threshold. Both will be added without changing the
version (*Versioning*): `"logic": "boolean"` will remain the short form of an
object `{"name": "boolean", ...parameters}`, and the threshold will be a
separate optional key `alpha`. Readers of this version reject both with
`format_error`.

After reading every rule, the rule set's own construction rules give an error
in this order:

1. rule names are unique: `rule_definition_error`, pointing to the `name` of
   the second rule with that name;
2. `aggregate` is only given with `"collect"`: `rule_definition_error`,
   pointing to `/ruleset/aggregate`;
3. `logic` is known: `registry_error` or `unsupported`, pointing to
   `/ruleset/logic`.

Writers write the keys in the order of the table and omit `aggregate` when
there is none and `strict` when it is `false`. They always write `hit_policy`
and `logic`, although they have defaults, because together they say how the
rule set decides, and a reader should not need to know the defaults.

## Decisions

A decision is what evaluating a rule set on one input produces (`rules.md`).
Its document records every rule's result tree, so it can be audited or
explained later without evaluating again. It holds the values the conditions
and payloads read from the input, which may include whole lists or maps and
personal data, so audit logs that store decisions must be treated like the
input. Rules are named, not copied: a decision is always read together with
the rule set that produced it (*Reading a decision*).

| Key | Required | JSON type |
|---|---|---|
| `hit_policy` | yes | the rule set's policy name |
| `aggregate` | with an aggregate | the aggregate name |
| `logic` | yes | the rule set's logic, in the same form |
| `results` | yes | array of rule results, one per rule, in rule order |
| `selected` | yes | array of the selected rules' names, in selection order (rule order for every policy of this version), without repetition |
| `aggregated` | with an aggregate | object: payload key → data value, or `null` when nothing was selected |

A **rule result** is an object with:

| Key | Required | JSON type |
|---|---|---|
| `rule` | yes | the rule's name |
| `fired` | yes | boolean: the truth value passed the logic's activation test |
| `when` | yes | the result tree of the rule's condition |
| `then` | when the payload was evaluated | object: payload key → result tree of that value, in the rule's member order |

`then` is present exactly for the rules whose payload was evaluated: the
selected rules, and under `"any"` every rule that fired (`rules.md`, *When
payloads are evaluated*). A rule without a payload whose payload was evaluated
has `"then": {}`.

`logic`, and the `alpha` that rule sets reserve, take the rule set's forms, so
graded logics extend decisions in the same way (*Rule sets*, *Reserved for
later versions*). Two optional keys are reserved too: `ruleset_digest`, a
digest of the rule set document that readers will verify, once a canonical
byte form exists to hash; and `metadata`, data values supplied by the caller,
such as a correlation id. The core never stamps times or ids itself, so
decisions stay deterministic. Repeated firings in forward chaining (phase K),
batches and the record of a `run` with its actions (task D.4) will use other
content keys or optional keys, not new meanings of these.

```json
{
  "version": 1,
  "decision": {
    "hit_policy": "first",
    "logic": "boolean",
    "results": [
      {
        "rule": "vip",
        "fired": true,
        "when": {
          "op": "gt",
          "truth": true,
          "children": [
            { "op": "field", "path": ["customer", "years"], "value": 7 },
            { "op": "const", "value": 5 }
          ]
        },
        "then": { "discount": { "op": "const", "value": { "$decimal": "0.15" } } }
      },
      {
        "rule": "blocked",
        "fired": false,
        "when": {
          "op": "field",
          "path": ["customer", "blocked"],
          "truth": null,
          "reason": { "code": "missing_field", "paths": [["customer", "blocked"]] },
          "children": [
            {
              "op": "field",
              "path": ["customer", "blocked"],
              "value": null,
              "reason": { "code": "missing_field", "paths": [["customer", "blocked"]] }
            }
          ]
        }
      }
    ],
    "selected": ["vip"]
  }
}
```

The outputs are not stored, because they follow from the document: a
rule's output maps each key of `then` to the `value` at the root of its tree;
`outputs` are the outputs of the `selected` rules in order; and `output` is the
winner's output for single-hit policies, or `aggregated` (`rules.md`,
*Outputs*). `fired` and `aggregated` could also be recomputed, but are stored
for consumers outside cleanrules: the aggregate's arithmetic (float
conversion, overflow) is not trivial. Readers recompute both and reject a
mismatch.

### Result trees

Every node of a result tree is an object with:

| Key | Required | JSON type |
|---|---|---|
| `op` | yes | the tag of the node evaluated |
| `path` | for `field` | the field's path, as in the condition |
| `truth` | for a node that has a truth value | truth value |
| `value` | for every other node | data value (*Values*) |
| `reason` | when `truth` is unknown or `value` is null | reason |
| `children` | when the node has children | array of result trees |

A node has `truth` when it is of kind condition (`ast.md`, *Node kinds*),
including the `between` halves, or when it is a field or constant in a
**condition position**: a rule's `when`, or an operand of `and`, `or` or
`not`. Every other node, including payload values, has `value`. A field or
constant in a condition position has one child: the result of reading it as a
value.

The children follow the node's fields in the order of *Conditions*, with two
exceptions, `between` and a field or constant in a condition position:

| Node | Children |
|---|---|
| `and`, `or` | one per operand |
| `not`, `in`, `is_null`, `matches`, `starts_with`, `ends_with`, `contains`, `abs`, `neg` | the operand |
| `eq` … `ge`, `add` … `div` | `left`, `right` |
| `between` | two half comparisons, `ge` or `gt` (operand, `low`) and `le` or `lt` (operand, `high`) according to `closed`, each with `truth` and its two children |
| `field`, `const` in a condition position | the value result |
| `field`, `const` in a value position | none |

The `between` halves let explanations say which bound failed (`ast.md`,
*Results and reasons*). The operand's result appears in both halves.

**Truth values.** Under Kleene logic, `truth` is `true`, `false`, or `null` for
unknown. Graded logics will add a number in `[0, 1]` and an interval form in
phase J, as new JSON forms of this key (*Versioning*).

**Reasons.** A `reason` says why a truth value is unknown or a value is null,
and appears only then (`ast.md`, *Results and reasons*):

| Key | Required | JSON type |
|---|---|---|
| `code` | yes | one of the reason codes of `ast.md` |
| `paths` | no, default `[]` | array of field paths involved; `[]` as a path means the input itself |
| `types` | no, default `[]` | array of type groups involved |
| `detail` | no | string, human-readable |

Type groups are written `number`, `string`, `boolean`, `date`,
`naive_datetime`, `aware_datetime` and `other` (lists, maps and opaque values).
The order of `paths` and `types`, and the duplicates they keep, are defined in
`ast.md` (*Results and reasons*). Implementations are compared on `code`,
`paths` and `types`, never on `detail`, whose wording may change between
releases. Writers omit empty `paths` and
`types`, and `detail` when there is none.

Writers write every key in the order of its table, and omit `aggregate`,
`aggregated`, `then`, `path`, `reason` and `children` when they do not apply.

### Reading a decision

A decision is read with the rule set that produced it, already read
(`read_decision(text, ruleset)` or its idiom in each language). After its
shape (`format_error`), the reader checks that the decision belongs to that
rule set and is consistent with it. The first failed check, in this order, is
a `decision_mismatch`, with a pointer to the offending value:

1. `hit_policy`, `aggregate` and `logic` are the rule set's;
2. `results` names exactly the rule set's rules, in rule order;
3. each rule's `when` tree, and each `then` tree, matches the rule's node
   by node, depth first: the same `op`, the same `path`, the number of children
   given above, `between` halves whose ops agree with `closed`, `const` values
   equal to the rule's constants, and both copies of a `between` operand equal;
4. `then` has exactly the payload's keys, in order, and is present exactly
   where *When payloads are evaluated* requires, judged from `selected` and
   `fired` as written (checks 6 and 7 verify those);
5. each node's `truth` or `value`, and its `reason`, are what evaluation gives
   from its children's (leaves, which hold what was read from the input, are
   taken as they are);
6. `fired` agrees with `when` under the logic's activation test;
7. `selected` is what the hit policy selects from the fired rules;
8. `aggregated` is what the aggregate gives from the selected outputs.

Recorded values are compared after reading, in their canonical form
(*Values*): every accepted form of a value matches it, while an int and a
float differ, and so do decimals of different scale and float zeros of
different sign. The exception is check 3's `const` values, which match the
rule's constants by structural identity (`ast.md`). Reasons compare on `code`,
`paths` and `types` (never `detail`), and the members of a result tree in any
order.

So a decision that a reader accepts is exactly one the core could have
produced for some input.

## Explanations

An explanation document is the JSON form of `explain()` (`explain.md`). It is
**output only**: the core writes it, and readers reject it with `unsupported`,
because it follows deterministically from a decision and its rule set, which
are the documents to store. Its schema is for consumers, such as user
interfaces.

What an explanation contains (the option cap, simplification, the
counterfactual rules of `explain.md`) is behaviour of the core, governed by
SemVer, the changelog and the conformance vectors, not by the format version.
To reproduce a past explanation, store the decision, the rule set and the core
version, not the explanation. The *Versioning* rules apply to output-only
documents too, so consumers should expect new optional keys.

| Key | Required | JSON type |
|---|---|---|
| `hit_policy` | yes | the decision's policy |
| `logic` | yes | the decision's logic, in the same form |
| `selected` | yes | the decision's `selected` |
| `rules` | yes | array of rule explanations, one per rule, in rule order |

A **rule explanation** is an object with:

| Key | Required | JSON type |
|---|---|---|
| `rule` | yes | the rule's name |
| `status` | yes | `"selected"`, `"fired"`, `"not_fired"` or `"unknown"` |
| `note` | for `"fired"` | object: `kind` (`"lower_priority"`, `"same_priority"`, `"any_agrees"` or `"earlier_rule"`) and `winner` (a rule name); for the two priority kinds also `priority` and `winner_priority` (ints) |
| `achievable` | for `"not_fired"` and `"unknown"` | boolean |
| `requirement` | yes | requirement |

A **requirement** is a cause, or a group written `{"all": [...]}` or
`{"any": [...]}` with at least two requirements. A **cause** is an object with:

| Key | Required | JSON type |
|---|---|---|
| `condition` | yes | the cause's node, as a condition node (*Conditions*); a `between` half is written as the comparison it is |
| `at` | yes | JSON Pointer to the result node in the decision document that the cause was built from: always in this rule's `when` tree, and always a node with `truth`. When repeated causes merge (`explain.md`, *Simplifying*), it is that of the item kept |
| `truth` | yes | truth value |
| `negated` | yes | boolean |
| `values` | yes | array of `{"path": ..., "value": ...}`, possibly empty |
| `reason` | when `truth` is unknown | reason (*Result trees*) |
| `counterfactual` | when there is one | counterfactual |

Whether a cause holds is not stored: under Kleene logic it holds when `truth`
is `true` and `negated` is `false`, or `truth` is `false` and `negated` is
`true`. Graded logics will define it with their activation test.

A **counterfactual** is an object with `path`, `op` (a comparison tag, already
flipped and negated as `explain.md` says), `target` (a scalar value) and `gap`
when there is one. Whether it is negated is the cause's `negated`. A `gap` is a
number (*Values*), `{"days": n}` between dates, or `{"microseconds": n}`
between datetimes, with `n` an int value (*Values*), so a large one is written
`{"$int": "..."}`.

```json
{
  "version": 1,
  "explanation": {
    "hit_policy": "first",
    "logic": "boolean",
    "selected": [],
    "rules": [
      {
        "rule": "big",
        "status": "not_fired",
        "achievable": true,
        "requirement": {
          "condition": { "op": "gt", "left": { "op": "field", "path": ["x"] }, "right": { "op": "const", "value": 10 } },
          "at": "/decision/results/0/when",
          "truth": false,
          "negated": false,
          "values": [{ "path": ["x"], "value": 8 }],
          "counterfactual": { "path": ["x"], "op": "gt", "target": 10, "gap": 2 }
        }
      }
    ]
  }
}
```

The text of `explain()` is generated from the same structure (`explain.md`,
*Text*); it is not part of the JSON document.

**Reserved for later versions.** Graded explanations (phase J) will add the
rule set's `alpha` to the explanation, a rule's degree, and graded fields on
causes (such as the degree reached and the degree needed), as optional keys.

## Summaries

A summary document is the JSON form of `summary()` (`explain.md`,
*Summaries*). Like an explanation, it is output only.

| Key | Required | JSON type |
|---|---|---|
| `hit_policy` | yes | the rule set's policy |
| `aggregate` | with an aggregate | the aggregate |
| `logic` | yes | the rule set's logic |
| `strict` | when `true` | `true` |
| `rules` | yes | array of `{"name", "priority", "outputs"}` in rule order, plus `"action": true` for a rule with an action and `"do"` with its name when it has one |
| `fields` | yes | array of `{"path", "conditions", "payloads"}`: `conditions` is an array of rule names, and `payloads` an array of `{"rule", "key"}` |
| `outputs` | yes | array of payload keys |
| `tags` | yes | array of `{"tag", "rules"}`, sorted by tag |

Every array is written even when it is empty.

## Decision tables

A decision table document has the content key `"decision_table"`. Its keys,
the order in which it is checked and how it is written are in `tables.md`
(*JSON*): its headers and cells are strings in the text syntax, and it compiles
into a rule set. It was added to version 1 (*Versioning*): older readers reject
it with `format_error`.

## Writing errors

*Not checked by vectors: they need objects (actions, native metadata) that a
vector cannot hold, so each binding's tests check them.*

A writer fails with `unsupported` when it is given something this version of
the format cannot hold. It reports the first problem found in rule order, and
within a rule in the order of the table, with a JSON Pointer to where the value
would have been written:

- a rule with an action given as code, without a name (pointer to
  `…/rules/<i>/do`), unless the caller passes the option `omit_actions`, which
  every binding names the same way in its language's case (`omitActions` in
  JavaScript); a named action is written as its name;
- a metadata value that is not a data value, or a float NaN in metadata, which
  would otherwise silently become null;
- a logic that is not one of the core's named logics, or one configured with
  parameters.

Decisions and explanations can always be written: a native value with no
equivalent is recorded as an opaque value (*Values*).

## YAML

Rule files may also be written in YAML 1.2. A YAML document is read into
exactly the JSON model above, and then read as that JSON document would be:
same keys, same errors, same pointers, same limits. Mapping order is kept.

**What a YAML file may hold.** One document (an optional `---` start and `...`
end), block and flow mappings and sequences, plain, single-quoted,
double-quoted, literal (`|`) and folded (`>`) scalars, comments, and a byte
order mark at the start, as YAML allows. Directives (`%YAML`, `%TAG`), anchors
(`&a`), aliases (`*a`), tags (`!!str`, `!x`), more than one document, mapping
keys that are not scalars, and raw control characters other than tab and line
breaks (anywhere, quoted or not) are `format_error`s located at their line and
column.

**Scalars.** Quoted, literal and folded scalars are always strings. A plain
scalar is resolved with the JSON subset of the YAML 1.2 core schema:

| Plain scalar | Value |
|---|---|
| empty, `null` or `~` | null |
| `true`, `false` | that boolean |
| text in the JSON number syntax (`42`, `-0.5`, `1e-7`) | that number, an integer literal or a float by the JSON rules (*Values*) |
| anything else | a string |

So `yes`, `True`, `0x1F`, `.inf`, `1_000` and an unquoted `2026-10-31` are
strings: dates are written `{"$date": "2026-10-31"}` as in JSON. A mapping key
is always a string, the text of its scalar (`1: x` has the key `"1"`).

**Errors and limits.** A syntax error, a forbidden construct or a duplicate key
gives the 1-based line and column, in code points, of where it starts (`\r\n`,
`\r` and `\n` are line breaks); every later error gives a JSON Pointer, as in
JSON. YAML texts have their own size limit, **1 MiB** by default: YAML parsers
hold a flow collection whole before reading it, so a large flow sequence costs
far more memory than the same JSON. The caller can raise it like the others.
The nesting limit counts mappings and sequences as JSON counts objects and
arrays; flow collections, in addition, nest at most **255** deep
(`limit_exceeded`), so deeper data is written in block style.

**Writing.** The canonical YAML of a document is block style with two-space
indentation: mapping keys in the document's order, written plain when they are
made of ASCII letters, digits, `_`, `$`, `.` and `-`, start with a letter, `_`
or `$`, and are not `null`, `true` or `false`, and as JSON strings otherwise; a
key longer than 1024 characters as written (the most YAML allows for an
implicit key) as an explicit key, `? key` and then `: value` on the next line;
every string value as a JSON string (double quotes, JSON escapes, which YAML
accepts); numbers, booleans and null as in canonical JSON; `[]` and `{}` for an
empty sequence or mapping. Reading the canonical YAML of a document gives the
same document.

## Errors and limits

Every error carries a stable code and a location. A syntax error gives a 1-based
line and column, counted in Unicode code points: that of the first character
that cannot be accepted, of the opening quote of a duplicate key's second
occurrence, or of the backslash of a lone surrogate escape. An unexpected end
of the text is one past its last character. Lines end at `\n`, and `\r\n` is
one line ending. Every other error gives a JSON Pointer
([RFC 6901](https://www.rfc-editor.org/rfc/rfc6901)) into the document as
written, before any sorting or flattening, such as
`/condition/operands/1/right/value`. A missing key is located by the pointer
it would have (`/condition/right`), and an unknown or extra key by its own
(`/condition/note`, `/condition/right/value/zone`).

| Code | Meaning |
|---|---|
| `format_error` | not valid JSON or UTF-8, a duplicate, unknown or missing key, a wrong JSON type, an unknown `op`, `$`-key or name, or a malformed value |
| `unsupported_version` | `version` is missing, is not an integer literal, or is not a version the reader knows |
| `expression_error` | the shape is valid, but a construction rule of a node (see *Reading*) is broken |
| `rule_definition_error` | the shape is valid, but a construction rule of a rule or rule set is broken |
| `registry_error` | a name that should be registered (a logic) is not |
| `decision_mismatch` | a decision does not belong to, or is not consistent with, the rule set it is read with |
| `unsupported` | this build of the core, or this version of the format, cannot do the operation: a logic built out by a Cargo feature, a writer given an action, reading an output-only document, or explaining a decision made with a graded logic |
| `limit_exceeded` | the document is larger, or nested deeper, than the reader allows |

A reader reports **one** error, the first it finds in this order:

1. size, JSON nesting and syntax (duplicate keys included), in text order;
2. `version`, before any other key, so a document from a newer format version
   gets `unsupported_version` even if it uses new keys;
3. shape (`format_error`) and node depth (`limit_exceeded`), depth first in
   pre-order: an object's known keys in the order of that object's table
   (*Conditions*, *Rules*, *Rule sets*, *Decisions*, *Result trees*), whatever
   their order in the text, then its unknown keys in text order. The node too
   deep is the first node at depth limit + 1 met this way. A document whose content
   key is `explanation` or `summary` is rejected with `unsupported` after the
   top-level keys are checked and before its content;
4. construction rules, in depth-first post-order: children
   before their parent, operands in order. In a rule, the condition comes
   first, then the payload values in order (`expression_error`), then the
   rule's own rules. In a rule set, the rules come first, in order, then the
   rule set's own rules. A decision has no construction rules; its checks
   against the rule set come here, as listed in *Reading a decision*.

Readers enforce three limits, so a hostile file cannot exhaust memory or the
stack (`CLAUDE.md`, invariant 9):

- **document size**: at most **16 MiB** of JSON text;
- **JSON nesting**: arrays and objects at most **1024** deep, counting the
  top-level object as 1, checked while parsing, before any tree is built. This also bounds lists and maps in data;
- **node depth**: a condition or payload value at most **256** nodes deep,
  counting the root as depth 1 and counting before `and`/`or` flattening. A
  result tree may be one level deeper, for the `between` halves.

A condition 256 nodes deep needs well under 1024 levels of JSON nesting, so
every condition within the node limit can be read. Data in a decision can
nest deeper, and a decision can be larger than its rule set, so writers do not
enforce these limits, and callers that read large decisions raise them. The
caller can raise the limits, but never disable them. A reader must not overflow its stack on any
input within its limits.
