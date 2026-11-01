# Text syntax

**Status:** draft, normative for the core and every binding.

Rules can be written as text, the way people write them on paper, in a
configuration field, a spreadsheet cell or a language model's answer:

```text
HIT POLICY priority

RULE vip PRIORITY 10 TAGS gold, loyalty
IF customer.years > 5 AND order.total > 1000
THEN discount = 0.15, label = "VIP"

RULE loyal PRIORITY 5
IF customer.years > 2 OR customer.newsletter
THEN discount = 0.05
```

A text is read into exactly the model of the other documents: a condition, a
rule or a rule set, which then behave as if read from JSON (`json-format.md`).
The syntax follows `fuzzytool.rule_from_text` where the two meet: `IF … THEN …`,
`AND`, `OR`, `NOT`, `&`, `|`, `~`, case-insensitive keywords. Two forms are
reserved for fuzzy rules (phase J): `IS` followed by a term name
(`score IS poor`), where `IS NULL` keeps its meaning even if a term is named
`null`, and a rule weight after the payload (`THEN … WITH 0.8`). Here `IS` only
appears in `IS NULL` and `IS NOT NULL`, and `WITH` only in `STARTS WITH` and
`ENDS WITH`.

Rule files written as text use the suffix `.rules`.

## Lexical structure

- Whitespace (spaces, tabs and line breaks, nothing else) separates tokens and
  is otherwise insignificant; a `#` starts a comment that runs to the end of the
  line. A byte order mark may start the text.
- **Keywords** are case-insensitive: `IF`, `THEN`, `RULE`, `PRIORITY`, `TAGS`,
  `DESCRIPTION`, `HIT`, `POLICY`, `AGGREGATE`, `LOGIC`, `STRICT`, `AND`, `OR`,
  `NOT`, `IN`, `BETWEEN`, `CLOSED`, `IS`, `NULL`, `TRUE`, `FALSE`, `MATCHES`,
  `STARTS`, `ENDS`, `WITH`, `CONTAINS`, `ABS`, `DATE`, `DATETIME`, `DECIMAL`,
  `FLOAT`.
- **Names** are ASCII identifiers (`[A-Za-z_][A-Za-z0-9_]*`) that are not
  keywords, or any text between backticks (`` `first name` ``, `` `and` ``), in
  which a backtick is written twice. Names are case-sensitive.
- **Strings** are written between double quotes, with the escapes of JSON
  strings (`\"`, `\\`, `\n`, `\u00e9`...), or between single quotes, in
  which a single quote is written twice and nothing else is escaped.
- Neither strings nor names hold raw control characters (U+0000 to U+001F):
  in strings they are written with JSON escapes, between double quotes.
- **Numbers**: digits with no fraction or exponent are an **int** (`42`); with a
  fraction and no exponent, an exact **decimal** (`0.15`, `2.50`, scale kept);
  with an exponent, a **float** (`1.5e0`, `1e-7`). As in JSON, a number does
  not start with `0` followed by digits (`007`). Where an operand starts, a `-`
  directly before a number makes a negative constant (`x > -1`); elsewhere it
  subtracts (`x -1 > 0` is `x - 1 > 0`). `DECIMAL "…"` and `FLOAT "…"` write any
  decimal or float, infinities included (`FLOAT "inf"`, `DECIMAL "-inf"`), with
  the text of their `$decimal` and `$float` forms (`json-format.md`, *Values*).
- **Dates and datetimes** are written `DATE "2026-10-31"` and
  `DATETIME "2026-10-31T09:30:00+01:00"`, with the text of their `$date` and
  `$datetime` forms. An unquoted `2026-10-31` is arithmetic, never a date.
- `TRUE`, `FALSE` and `NULL` are the boolean and null constants.

## Conditions and values

From the loosest binding to the tightest:

| Level | Syntax | Node (`ast.md`) |
|---|---|---|
| or | `a OR b`, `a \| b` | `or`, flattened |
| and | `a AND b`, `a & b` | `and`, flattened |
| not | `NOT a`, `~a` | `not` |
| predicate | `x = y`, `x == y`, `x != y`, `x <> y`, `x < y`, `x <= y`, `x > y`, `x >= y` | `eq`, `ne`, `lt`, `le`, `gt`, `ge` |
| | `x IN (1, 2, 3)`, `x NOT IN (…)` | `in`, `not` of `in`; constants only |
| | `x BETWEEN low AND high [CLOSED BOTH\|LEFT\|RIGHT\|NONE]`, `x NOT BETWEEN …` | `between` (`closed` is `both` when omitted) |
| | `x IS NULL`, `x IS NOT NULL` | `is_null`, `not` of `is_null` |
| | `x MATCHES "pattern"`, `x STARTS WITH "a"`, `x ENDS WITH "a"`, `x CONTAINS "a"`, each also with `NOT` before the keyword | `matches`, `starts_with`, `ends_with`, `contains` |
| sum | `a + b`, `a - b` | `add`, `sub` |
| product | `a * b`, `a / b` | `mul`, `div` |
| unary | `-a` | `neg` (a negative number is a constant instead) |
| primary | a constant, a field, `ABS(a)` (`a` a sum), `( … )` | `const`, `field`, `abs` |

A predicate has one comparison at most: `0 < x < 10` is a syntax error (write
`x BETWEEN 0 AND 10 CLOSED NONE`). In `BETWEEN low AND high`, the `AND` belongs
to the `BETWEEN`. A field or a boolean constant can stand alone as a condition
(`customer.newsletter`).

**Fields** are paths: a name or an `[index]`, then any number of `.name` and
`[index]` segments, where an index is a non-negative integer
(`orders[0].total`, `[0].total` when the input is a list). Only a key can
follow a `.`, so there any identifier is a key, keywords included
(`order.date`, `item.priority`). A key that is not an identifier, or a first
key that is a keyword, is written between backticks (`` order.`unit price` ``,
`` `date` ``).

The construction rules of `ast.md` apply to what is read, with the same errors
(`expression_error`), located by the line and column where the node starts
(its opening parenthesis, when it has one): `x IN (1, 'a')` and `(a > 1) = TRUE`
are errors at column 1. Like the reader of JSON, the reader flattens nested `and`
and `or`, and sorts `in` values (`ast.md`), so `a AND (b AND c)` reads as one
`and` of three operands.

## Rules and rule sets

```text
RULE <name> [PRIORITY <int>] [TAGS <name or string>, …] [DESCRIPTION <string>]
IF <condition>
[THEN <key> = <value>, …]
```

- The name is a name or a string. Without a `RULE` header, a rule is named
  `rule_<n>` by its position in the text, from 1: `IF a > 1 THEN x = 1` alone
  is the rule `rule_1`.
- The payload is optional; its keys are names or strings, in order, and its
  values are value expressions (`THEN tax = order.total * 0.21`). A condition
  there is a `rule_definition_error`, as in JSON (`rules.md`), located at the
  value; so is a key written twice, located at its second occurrence.
- Metadata cannot be written as text: rules with metadata are written as JSON or
  YAML.

A text holds:

- a **condition**, when it starts with neither `RULE`, `IF` nor a rule set
  header (`customer.years > 5 AND NOT blocked`);
- a **rule**, when it holds one rule and no rule set header;
- a **rule set**, when it starts with a header, or holds more than one rule. The
  header is any of these items, in this order, usually one per line:
  `HIT POLICY <policy>`, `AGGREGATE <aggregate>`, `LOGIC <logic>`, `STRICT`
  (the names of `rules.md`).

A reader asked for one rule (`rule_from_text`) refuses a condition or a rule
set with a `rule_syntax_error` where the rule should start or the text end.

## Errors and limits

A text that does not follow this syntax is a `rule_syntax_error` with the line
and column (1-based, in code points; `\r\n`, `\r` and `\n` are line breaks) of
the first token that cannot be accepted, or one past the last character for an
unexpected end. The error says what was expected.

Texts are read in order, and the first error found is reported: a syntax error
where it is met; the construction errors of a rule once the rule is read, before
the next rule; those of a condition once it is read.

Texts have the limits of JSON documents (`json-format.md`, *Errors and
limits*):

- at most **4 MiB** by default, less than JSON: a text holds up to one node in
  every two bytes (`a|a|…`), and readers hold its tree whole;
- conditions and payload values at most 256 nodes deep, counted as there. The
  reader checks each node as it completes it, and stops with `limit_exceeded`
  located where the first node that goes deeper starts;
- parentheses, `NOT`, unary `-` and `ABS` nested at most `2 × max_depth + 2`
  deep (514), beyond which the reader stops with `limit_exceeded` at the token
  that goes deeper. Each node costs at most two levels, so a canonical text
  within the depth limit is never refused.

## Printing

Every condition, rule and rule set without metadata has a **canonical text**,
which reads back as the same model:

- keywords in upper case; one space around binary operators and after commas;
- the fewest parentheses that keep the tree, by the levels above; `NOT` before
  a predicate needs none (`NOT x > 5`);
- `=` and `!=` for equality and inequality, `x NOT IN (…)` never: the printer
  writes `NOT x IN (…)`, as the tree is;
- constants as in *Lexical structure*: ints as digits; decimals in plain
  notation with their scale, and `DECIMAL "…"` when the scale is 0 or the value
  infinite; floats in exponent notation with the shortest digits that read back
  (`1.5e0`, `1e-7`, `-0e0`), and `FLOAT "…"` when infinite; a negative constant
  with its sign (`-5`), and the negation of a number constant with parentheses
  (`-(5)`, `-(-5)`); strings between double quotes with JSON escapes;
- field keys, rule names, tags and payload keys as names when they are names,
  and keys after a `.` when they are identifiers; field keys between backticks
  and the others as strings otherwise;
- a rule as its `RULE` header (with `PRIORITY` when it is not 0, `TAGS` and
  `DESCRIPTION` when there are any), its `IF` line and its `THEN` line, each on
  its own line; a rule set as its header lines (`HIT POLICY`, then `AGGREGATE`,
  `LOGIC` when it is not `boolean`, and `STRICT` when set), a blank line, and its
  rules separated by blank lines;
- no line break at the end; a file ends with one.

A field key with a control character has no text (a name cannot hold it):
writing it is `unsupported`, like a rule with metadata.

The text of explanations and summaries (`explain.md`) writes conditions in this
canonical form.
