# Agents

**Status:** draft, normative for the core, the MCP server (`crates/mcp`) and
every binding that offers these tools. JSON Schema:
[`schemas/v1/agents.schema.json`](schemas/v1/agents.schema.json).

AI agents use cleanrules through **tools**: they list the rule sets they may
use, read what a rule set needs and decides, evaluate it on an input, ask why,
and check the rules they write. The engine has no model inside, and the tools
never run actions, never change a rule set and never read anything their
caller did not configure (`CLAUDE.md`, invariant 12). The same call on the same
rule sets always gives the same result (invariant 7).

The tools are implemented once, in the core: a **catalog** of named rule sets
answers a **tool call**, a tool name and its arguments as JSON, with a result
or an error, as JSON. The MCP server and the adapters for agent frameworks
pass both through, so every language gives the same answer to the same call.
Values in arguments and results use the encoding of the JSON format
(`json-format.md`, *Values*): an input with a decimal is written
`{"$decimal": "0.15"}`, and a date `{"$date": "2026-11-11"}`.

## Tools

| Tool | Arguments | Result |
|---|---|---|
| `list_rulesets` | none | the rule sets of the catalog |
| `summary` | `ruleset` | what a rule set reads and decides, and the schema of its input |
| `evaluate` | `ruleset`, `input` | the decision: the rules selected and their outputs |
| `explain` | `ruleset`, `input`, optionally `budget` | the decision and a compact explanation |
| `validate` | `document`, and `format` for a text | whether a document reads, and what it holds |

A **catalog** maps names to rule sets. Names are strings, compared exactly, and
listed sorted by Unicode code point. How a server names its rule sets, such as
by their file names, is its own choice. A rule's action is never run: it is
data, which `summary` shows. A binding that adds a rule set whose rules hold
actions given as code tells the catalog which rules have one, so its summary
marks them as `summary()` does in that language (`explain.md`, *Summaries*).
A catalog offers every tool unless it is built to offer only some, such as a
server that offers no `validate`; the others are then unknown to it.

### Arguments

The arguments of a call are a JSON object with these members:

| Member | Tools | JSON type | Required | Meaning |
|---|---|---|---|---|
| `ruleset` | `summary`, `evaluate`, `explain` | string | yes | the name of a rule set of the catalog |
| `input` | `evaluate`, `explain` | any | yes | the input, a data value (`json-format.md`, *Values*) |
| `budget` | `explain` | an integral number from 256 to 1048576 (`2000.0` is `2000`) | no, `2000` | the size of the explanation (*Compact explanations*) |
| `document` | `validate` | object or string | yes | an object is a JSON document; a string is the text of a document in `format` |
| `format` | `validate` | `"json"`, `"yaml"`, `"text"` or `"csv"` | when `document` is a string | the syntax of the text (`json-format.md`, *Documents*, *Formats*) |

They are read as strictly as documents are (`json-format.md`, *Documents*), and
a call reports **one** error, the first it finds in this order:

1. arguments that are not a JSON object: `format_error` at the root (`""`);
2. the tool's members in the order of the table: a required member that is
   missing, a member of the wrong JSON type, or a `budget` out of its range,
   is a `format_error` at the member (`/budget`);
3. members the tool does not take, and a member given twice, in the order they
   are written: `format_error` at the member;
4. the members' values, in the order of the table:
   - a `ruleset` that the catalog does not have: `registry_error` with `name`,
     `available` (the catalog's names, sorted) and the pointer `/ruleset`;
   - the `input`, read as data: its `format_error`s point into it
     (`/input/order/total`);
   - a `format` that is not one of the four, or that is not `"json"` for a
     `document` that is an object: `format_error` at `/format`.

A tool the catalog does not offer is a `registry_error` with the tool's `name`
and the names of the tools it offers, sorted, as `available`, without a
pointer.

### Results

A result is a JSON object whose members are written in the order of its table.
Optional members are left out when they do not apply, and arrays are written
even when they are empty.

#### `list_rulesets`

One member, `rulesets`: an array with one object per rule set, in catalog
order:

| Member | Required | JSON type |
|---|---|---|
| `name` | yes | the rule set's name |
| `rules` | yes | int: how many rules it has |
| `hit_policy` | yes | its policy |
| `aggregate` | with an aggregate | its aggregate |
| `logic` | yes | its logic |
| `strict` | when it is strict | `true` |
| `outputs` | yes | its payload keys, as its summary lists them |

#### `summary`

| Member | Required | JSON type |
|---|---|---|
| `ruleset` | yes | the rule set's name |
| `summary` | yes | its summary: the content of a summary document (`json-format.md`, *Summaries*) |
| `input_schema` | yes | the schema of its input (*Input schemas*) |

#### `evaluate`

The rule set evaluates the input (`rules.md`, *Evaluation*), and the result
says what it decided:

| Member | Required | JSON type |
|---|---|---|
| `ruleset` | yes | the rule set's name |
| `selected` | yes | the names of the selected rules, in selection order |
| `unknown` | yes | the names of the rules whose condition is unknown, in rule order |
| `output` | for single-hit policies and aggregates | a data value: the winner's payload as a map, the aggregate's map, or `null` when nothing was selected (`rules.md`, *Outputs*) |
| `outputs` | yes | an array of maps: the payloads of the selected rules, in rule order |

`unknown` lets an agent see that missing or mistyped data kept rules from
firing. An evaluation that fails, in strict mode or because the hit policy
cannot decide, is a tool error (*Errors*).

#### `explain`

The members of `evaluate`'s result, then `explanation`, the decision's compact
explanation (*Compact explanations*): an object with `text`, a string, and
`complete`, a boolean.

#### `validate`

The document is read as a reader of the format reads it, with the default
limits (`json-format.md`, *Errors and limits*): an object as a JSON document,
and a string in its `format`. Decisions are read with their rule set and are
not documents a rule set comes from, so reading one here is `unsupported`, as
reading an explanation or a summary is.

| Member | Required | JSON type |
|---|---|---|
| `valid` | yes | boolean: whether the document reads |
| `content` | when it reads | its content key: `"condition"`, `"rule"`, `"ruleset"` or `"decision_table"` |
| `summary` | when it gives a rule set | the summary of the rule set it gives (`json-format.md`, *A rule set from a document*) |
| `input_schema` | with `summary` | that rule set's input schema |
| `error` | when it does not read | the reader's error, as an agent error (*Errors*) |

A document that does not read is a result, not a tool error, because it
answers the question asked. Its error is located in the document: a pointer
into it, as for any document (`/ruleset/rules/0/when`), or a line and a column
in its text. Only the arguments themselves give tool errors.

### Errors

A tool error is the result `{"error": <agent error>}`. An **agent error** is
the JSON form of the error (`errors.md`, *JSON form*), written so that a model
can correct its call:

- it always has a `hint`: the core's own, or else the default of its code
  (below);
- its `pointer` locates the problem in the call's arguments: the member, or a
  place inside `input`. An evaluation error with a `path` (`missing_field`,
  `type_mismatch`, `evaluation_error`) gets the pointer to the value at that
  path as the input is written, through `$map` where the input uses it, or to
  where it would be when it is missing: `/input/customer/age` for the path
  `["customer", "age"]`;
- the error of a `validate` result is located in the document instead
  (*`validate`*).

**In MCP.** A tool call gives a JSON object and whether it is a tool error
(`content` and `is_error` in the core's result). An MCP server returns a result
as the call's structured content (`structuredContent`), with its JSON text as
the text content (`content`), as MCP recommends for clients that read only
text. It returns a tool error as a result with `isError: true` whose text
content is the JSON of `{"error": ...}`, without structured content, which the
tool's output schema does not describe; and a tool the catalog does not offer
as a protocol error.

#### Default hints

*Informative: hints are free text for people and models, whose wording may
change; that every agent error has one is checked by the core's tests.*

| Code | Default hint |
|---|---|
| `format_error` | fix the value at the pointer so that it has the form the format (or the tool's input schema) gives |
| `unsupported_version` | write "version": 1 |
| `limit_exceeded` | send a smaller document or input |
| `decision_mismatch` | read the decision with the rule set that made it |
| `expression_error` | fix the condition at the pointer: compare values, and join conditions with and, or and not |
| `rule_definition_error` | fix the rule at the pointer |
| `rule_syntax_error` | fix the text at the line and column |
| `registry_error` | use one of the available names |
| `rule_not_found` | use one of the available names |
| `evaluation_error` | fix the value at the pointer in the input |
| `missing_field` | add the field at the pointer to the input; the rule set's input schema lists what it reads |
| `type_mismatch` | give the field at the pointer a value of the type the rule set's input schema gives |
| `hit_policy_error` | the rule set cannot decide on this input: change the input, or ask the rule set's owner |
| `action_error` | tools never run actions; report this as a bug |
| `unsupported` | this build or this version cannot do it; try another format or tool |

### Versioning

*Not checked by vectors: a rule for changes to this contract, which reviews
and the format version enforce.*

The tools follow the JSON format's version (`json-format.md`, *Versioning*):
their arguments and results hold documents, values and errors of format
version 1, and `schemas/v1/agents.schema.json` describes them. Changes are
additive only: a new tool, a new optional argument, or a new member of a
result, which callers must expect, as they must expect new members in errors.
Any other change (renaming or removing a tool, an argument or a member, or
changing what one means) comes with the next version of the format, in
`schemas/v2/`. What a result contains within its shape (which rules a compact
explanation keeps, the wording of a hint or a description) is behaviour of the
core, governed by SemVer, the changelog and the conformance vectors, as for
explanations (`json-format.md`, *Explanations*).

## Input schemas

The input schema of a rule set is a JSON Schema (draft 2020-12) of the input it
reads, built from its summary (`explain.md`, *Summaries*): the fields it reads,
in the summary's order, each with the types its conditions and payloads expect.
It tells an agent what to send. It is advisory: the engine never checks an
input against it, an input that matches it can still leave conditions
unknown, and one that does not is still evaluated as the specification says.

### Types of a field

Each place where a rule reads a field expects a type, or none, from the node
around it:

| The field is | Types |
|---|---|
| a condition: a rule's `when`, or an operand of `and`, `or` or `not` | `boolean`; under a graded logic also `number` |
| a side of a comparison (`eq` … `ge`) whose other side is a constant | the constant's type |
| a side of a comparison whose other side is arithmetic (`add` … `neg`) | `number` |
| the operand of `between`, compared with each bound; or a bound, compared with the operand | as for comparisons, each half on its own (`ast.md`, *Node semantics*) |
| the operand of `in` | the types of its values, in their order |
| the operand of `matches`, `starts_with`, `ends_with` or `contains` | `string` |
| an operand of `add`, `sub`, `mul`, `div`, `abs` or `neg` | `number` |
| anywhere else: the operand of `is_null`, a payload value, a side of a comparison with another field | none |

A constant's type is `boolean`, `number` (an int or a float), `decimal`,
`string`, `date`, `naive_datetime` or `aware_datetime`. Decimals have their own
type because an agent that sends `0.1` sends a float, which never equals the
decimal `0.1` (`ast.md`, *Types*).

A field's types are those of every place that reads it, in the order the
summary visits them (rules in rule order; the condition, then the payload
values in member order; each node in depth-first pre-order), each once. Under
the logic `boolean`, a field used as a condition is `boolean`; the graded
logics of phase J also accept numbers there (`ast.md`, *Truth values*).

### Building the schema

The paths of the summary's fields form a tree: the input is its root, and each
path's segments lead from it to the field, the children of a node in the order
their paths first appear among the fields. Each node gets a schema:

1. Its **alternatives**: when it is a field, one schema per type, in order:

   | Type | Schema |
   |---|---|
   | `boolean` | `{"type": "boolean"}` |
   | `number` | `{"type": "number"}` |
   | `decimal`, `date`, `naive_datetime`, `aware_datetime` | the value's typed form, as `value.schema.json` defines it (`decimal`, `date`, `naiveDatetime`, `awareDatetime`) |
   | `string` | `{"type": "string"}` |

   Then, when it has children reached by keys, a map: `{"type": "object",
   "properties": {...}}` with one property per key, in order, and the
   `required` keys (*Required members*) when there are any. A map with a key
   that starts with `$` is written `{"$map": ...}` (`json-format.md`, *Lists
   and maps*), so then the map's schema is instead `{"type": "object",
   "properties": {"$map": <the map's schema>}, "required": ["$map"],
   "additionalProperties": false}`. Then, when it has children reached by
   indices, a list: `{"type": "array", "prefixItems": [...]}`, with an item
   for every index up to the largest, `{}` for those not read, and `minItems`
   when items are required (*Required members*). When the largest index is
   256 or more, the list has no `prefixItems`, so that one rule cannot make a
   huge schema.
2. The **root** is the object `{"$schema":
   "https://json-schema.org/draft/2020-12/schema"}` with the members of its one
   alternative, or `anyOf` with its alternatives when it has several, or
   nothing more when it has none (a rule set that reads no field).
3. Any **other node** is the object with `description` when it is a field, and
   `anyOf` with its alternatives followed by `{"type": "null"}`, when it has
   alternatives. A field without alternatives (read only by `is_null` or as a
   payload value, say) accepts anything: it has the `description` alone.

The `description` says where the field is read, as the summary's text does
(`explain.md`, *Summaries*): `conditions of vip, loyal; payloads of loyal
(discount)`. Nothing has `additionalProperties`, so an input may hold more
than the rule set reads; and every field, and every map or list on the way to
one, may be null, because null is data: it makes a condition unknown, never an
error (`ast.md`, *Missing values*).

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "customer": {
      "anyOf": [
        {
          "type": "object",
          "properties": {
            "years": { "description": "conditions of vip, loyal", "anyOf": [{ "type": "number" }, { "type": "null" }] }
          }
        },
        { "type": "null" }
      ]
    },
    "since": { "description": "conditions of vip" }
  }
}
```

(The rule set `RULE vip IF customer.years > 5 AND since IS NULL` and `RULE
loyal IF customer.years > 2`.)

### Required members

Outside strict mode a missing field is null, so nothing is required. In strict
mode every condition is evaluated, and a missing field raises `missing_field`
(`rules.md`, *Strict mode and errors*): a map lists as `required` each key that
leads to a field read by a condition, and a list has `minItems` one more than
the largest such index. Payload values are evaluated only for the rules
selected, so the fields they read alone are not required. A required member
may still be null: strict mode raises for missing fields, not for nulls.

## Compact explanations

A compact explanation is the text of the decision's explanation (`explain.md`,
*Text*) cut to a **budget**: at most that many Unicode code points, counting
the line breaks between lines, so that it fits in a model's context. The
budget is 2000 by default, at least 256 and at most 1048576 (*Arguments*); a
budget out of that range is the `format_error` at `/budget` of *Arguments*,
also when a binding asks for a compact explanation directly.

1. **Lines.** The explanation's text is split into lines: the header, then
   each rule's **block**, its own line followed by its other lines. A line
   longer than **160** code points is cut to its first 157 and `...`.
2. **Everything.** When all the lines fit in the budget, the text is all of
   them, in order.
3. **Choosing rules.** Otherwise the header is kept, and the rules are taken
   by rank: selected rules first, then those that are unknown, then those that
   fired and were not selected, then those that did not fire and could
   (achievable), and last those that did not fire and cannot; within a rank, in
   rule order. A rule's whole block is taken when it fits in what is left of
   the budget; else its own line alone, when that fits; else nothing. What is
   left keeps room for the last line, measured as if every rule were left out.
4. **The text.** The header, the lines taken, in rule order, and a last line
   `... <k> of <n> rules shortened or omitted to fit the budget`, where `k`
   counts the rules not taken whole.

`complete` is true when the text is the explanation's whole text: no line was
cut and every rule was taken whole.

So the rules that decided come first, then what the input lacked, then what
another input could change. Explanations of graded logics (phase J) are
`unsupported` for now, and so is `explain` on a rule set that uses one.
