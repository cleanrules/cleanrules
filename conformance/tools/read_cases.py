"""Vectors for reading documents, written from the spec (the v1 has no reader).

Each vector gives a document, as JSON data or as text for lexical cases, and the
canonical document read back or the error. The expectations come from
`spec/json-format.md`, not from running anything, except the valid decisions,
which are produced by the v1 and then altered.
"""

from __future__ import annotations

import json
from typing import Any

import encode

from rulepy import F, Rule, RuleSet

Json = Any
VECTORS: list[dict[str, Json]] = []

DOCUMENTS = "json-format.md#documents"
NUMBERS = "json-format.md#numbers"
DATES = "json-format.md#dates-and-datetimes"
LISTS = "json-format.md#lists-and-maps"
VALUES = "json-format.md#values"
CONDITIONS = "json-format.md#conditions"
READING = "json-format.md#reading"
WRITING = "json-format.md#writing"
RULES = "json-format.md#rules"
RULESETS = "json-format.md#rule-sets"
READING_DECISION = "json-format.md#reading-a-decision"
ERRORS = "json-format.md#errors-and-limits"


def text(data: Json) -> str:
    """Compact JSON text, so that columns are easy to compute."""
    return json.dumps(data, separators=(",", ":"), ensure_ascii=False)


def read(
    name: str,
    spec: tuple[str, ...],
    description: str,
    *,
    document: Json = None,
    source: str | None = None,
    expect: Json = None,
    error: Json = None,
    ruleset: Json = None,
    limits: Json = None,
) -> None:
    """Add a `read` vector: `document` or `source` text in, `expect` or `error` out."""
    given: dict[str, Json] = {"kind": "read"}
    if source is not None:
        given["text"] = source
    else:
        given["document"] = document
    if ruleset is not None:
        given["ruleset"] = ruleset
    if limits is not None:
        given["limits"] = limits
    VECTORS.append(
        {
            "id": f"format/{name}",
            "spec": list(spec),
            "description": description,
            "given": given,
            "expect": {"error": error} if error is not None else {"document": expect},
        }
    )


def field(*path: Json) -> Json:
    """A field node."""
    return {"op": "field", "path": list(path)}


def const(v: Json) -> Json:
    """A const node."""
    return {"op": "const", "value": v}


def eq(v: Json) -> Json:
    """`a == v`, the condition used to test how a constant is read."""
    return {"op": "eq", "left": field("a"), "right": const(v)}


def cond(c: Json) -> Json:
    """A condition document."""
    return {"version": 1, "condition": c}


def column(source: str, needle: str, occurrence: int = 1) -> int:
    """1-based column (in code points) of the `occurrence`-th `needle` in a one-line text."""
    index = -1
    for _ in range(occurrence):
        index = source.index(needle, index + 1)
    return index + 1


VALUE_AT = "/condition/right/value"

# --- Documents ----------------------------------------------------------------------

simple = cond(field("a"))
read(
    "documents/schema-key-ignored",
    (DOCUMENTS,),
    "readers ignore $schema",
    document={"$schema": "https://example.com/s.json", **simple},
    expect=simple,
)
read(
    "documents/missing-version",
    (DOCUMENTS, ERRORS),
    "a missing version",
    document={"condition": field("a")},
    error={"code": "unsupported_version", "pointer": "/version"},
)
read(
    "documents/version-string",
    (DOCUMENTS,),
    "a version that is a string",
    document={"version": "1", "condition": field("a")},
    error={"code": "unsupported_version", "pointer": "/version"},
)
read(
    "documents/version-float",
    (DOCUMENTS,),
    "a version written 1.0 is not an integer literal",
    source='{"version":1.0,"condition":{"op":"field","path":["a"]}}',
    error={"code": "unsupported_version", "pointer": "/version"},
)
read(
    "documents/newer-version-first",
    (DOCUMENTS, ERRORS),
    "the version is checked before unknown keys",
    document={"version": 2, "condition": field("a"), "news": 1},
    error={"code": "unsupported_version", "pointer": "/version"},
)
read(
    "documents/unknown-key",
    (DOCUMENTS,),
    "an unknown top-level key",
    document={**simple, "comment": "x"},
    error={"code": "format_error", "pointer": "/comment"},
)
read(
    "documents/no-content-key",
    (DOCUMENTS,),
    "a document without a content key",
    document={"version": 1},
    error={"code": "format_error", "pointer": ""},
)
read(
    "documents/two-content-keys",
    (DOCUMENTS,),
    "a document with two content keys",
    document={**simple, "rule": {"name": "r", "when": field("a")}},
    error={"code": "format_error", "pointer": ""},
)
dup = '{"version":1,"version":1,"condition":{"op":"field","path":["a"]}}'
read(
    "documents/duplicate-key",
    (DOCUMENTS, ERRORS),
    "a duplicate key points to its second occurrence",
    source=dup,
    error={"code": "format_error", "line": 1, "column": column(dup, '"version"', 2)},
)
dup_inner = '{"version":1,"condition":{"op":"field","path":["a"],"path":["b"]}}'
read(
    "documents/duplicate-key-in-node",
    (DOCUMENTS, ERRORS),
    "a duplicate key inside a node",
    source=dup_inner,
    error={"code": "format_error", "line": 1, "column": column(dup_inner, '"path"', 2)},
)
read(
    "documents/byte-order-mark",
    (DOCUMENTS, ERRORS),
    "a byte order mark is rejected",
    source="\ufeff" + text(simple),
    error={"code": "format_error", "line": 1, "column": 1},
)
trailing = text(simple) + " x"
read(
    "documents/trailing-garbage",
    (ERRORS,),
    "text after the document is a syntax error",
    source=trailing,
    error={"code": "format_error", "line": 1, "column": len(trailing)},
)
multiline = '{\n  "version": 1,\n  "condition": {"op": "field", "path": ["a"],}\n}'
read(
    "documents/syntax-error-line",
    (ERRORS,),
    "a syntax error on the third line",
    source=multiline,
    error={"code": "format_error", "line": 3, "column": multiline.splitlines()[2].index(",}") + 2},
)
read(
    "documents/explanation-is-output-only",
    (DOCUMENTS,),
    "explanations are output only",
    document={
        "version": 1,
        "explanation": {"hit_policy": "first", "logic": "boolean", "selected": [], "rules": []},
    },
    error={"code": "unsupported"},
)
read(
    "documents/summary-is-output-only",
    (DOCUMENTS,),
    "summaries are output only",
    document={
        "version": 1,
        "summary": {
            "hit_policy": "first",
            "logic": "boolean",
            "rules": [],
            "fields": [],
            "outputs": [],
            "tags": [],
        },
    },
    error={"code": "unsupported"},
)

# --- Numbers --------------------------------------------------------------------------


def value_text(literal: str) -> str:
    """The text of a condition document comparing `a` with a literal."""
    return (
        '{"version":1,"condition":{"op":"eq","left":{"op":"field","path":["a"]},"right":{"op":"const","value":'
        + literal
        + "}}}"
    )


surrogate = value_text('"x\\ud800"')
read(
    "documents/lone-surrogate",
    (DOCUMENTS, ERRORS),
    "a lone surrogate escape is rejected",
    source=surrogate,
    error={"code": "format_error", "line": 1, "column": column(surrogate, "\\ud800")},
)


read(
    "values/int-negative-zero",
    (NUMBERS,),
    "-0 is the int 0",
    source=value_text("-0"),
    expect=cond(eq(0)),
)
read(
    "values/int-max",
    (NUMBERS, VALUES),
    "the largest int is written as $int text",
    source=value_text("9223372036854775807"),
    expect=cond(eq({"$int": "9223372036854775807"})),
)
read(
    "values/int-safe",
    (NUMBERS, VALUES),
    "2^53 - 1 stays bare",
    source=value_text("9007199254740991"),
    expect=cond(eq(9007199254740991)),
)
read(
    "values/int-above-safe",
    (NUMBERS, VALUES),
    "2^53 is written as $int text",
    source=value_text("9007199254740992"),
    expect=cond(eq({"$int": "9007199254740992"})),
)
read(
    "values/int-overflow",
    (NUMBERS,),
    "an int constant outside 64 bits",
    source=value_text("9223372036854775808"),
    error={"code": "format_error", "pointer": VALUE_AT},
)
read(
    "values/int-underflow",
    (NUMBERS,),
    "an int constant below -2^63",
    source=value_text("-9223372036854775809"),
    error={"code": "format_error", "pointer": VALUE_AT},
)
read(
    "values/int-min",
    (NUMBERS,),
    "-2^63 is an int",
    source=value_text("-9223372036854775808"),
    expect=cond(eq({"$int": "-9223372036854775808"})),
)
read(
    "values/int-tagged-leading-zeros",
    (NUMBERS,),
    "$int text may have leading zeros",
    document=cond(eq({"$int": "007"})),
    expect=cond(eq(7)),
)
read(
    "values/int-tagged-number",
    (NUMBERS,),
    "$int with a number",
    document=cond(eq({"$int": 42})),
    expect=cond(eq(42)),
)
read(
    "values/int-tagged-float-literal",
    (NUMBERS,),
    "$int with 1.0 is not an integer literal",
    source=value_text('{"$int":1.0}'),
    error={"code": "format_error", "pointer": VALUE_AT + "/$int"},
)
read(
    "values/int-tagged-exponent",
    (NUMBERS,),
    "$int with 1e2 is not an integer literal",
    source=value_text('{"$int":1e2}'),
    error={"code": "format_error", "pointer": VALUE_AT + "/$int"},
)
read(
    "values/float-tagged-integer",
    (NUMBERS, VALUES),
    "$float with an integer literal is a float",
    source=value_text('{"$float":1}'),
    expect=cond(eq(1.0)),
)
read(
    "values/float-tagged-negative-zero",
    (NUMBERS,),
    "$float -0 is -0.0",
    source=value_text('{"$float":-0}'),
    expect=cond(eq(-0.0)),
)
read(
    "values/float-negative-zero",
    (NUMBERS,),
    "-0.0 is kept",
    source=value_text("-0.0"),
    expect=cond(eq(-0.0)),
)
read(
    "values/float-exponent",
    (NUMBERS,),
    "1e2 is the float 100",
    source=value_text("1e2"),
    expect=cond(eq(100.0)),
)
read(
    "values/float-overflow",
    (NUMBERS,),
    "a finite literal too large for a float",
    source=value_text("1e400"),
    error={"code": "format_error", "pointer": VALUE_AT},
)
read(
    "values/float-infinity",
    (NUMBERS,),
    "infinities are typed values",
    document=cond(eq({"$float": "-inf"})),
    expect=cond(eq({"$float": "-inf"})),
)
read(
    "values/float-rounding",
    (NUMBERS,),
    "a literal is rounded to the nearest float, ties to even",
    source=value_text("9007199254740993.0"),
    expect=cond(eq(9007199254740992.0)),
)
for name, raw, canonical in (
    ("leading-zeros", "007.50", "7.50"),
    ("negative-exponent", "1.5e-3", "0.0015"),
    ("positive-exponent", "1e2", "100"),
    ("positive-exponent-with-fraction", "1.5e3", "1500"),
    ("negative-zero", "-0.00", "0.00"),
    ("scale-kept", "1.00", "1.00"),
    ("negative-infinity", "-inf", "-inf"),
):
    read(
        f"values/decimal-{name}",
        (NUMBERS,),
        f"the decimal {raw} is written {canonical}",
        document=cond(eq({"$decimal": raw})),
        expect=cond(eq({"$decimal": canonical})),
    )
read(
    "values/decimal-38-digits",
    (NUMBERS,),
    "38 significant digits fit",
    document=cond(eq({"$decimal": "9" * 38})),
    expect=cond(eq({"$decimal": "9" * 38})),
)
read(
    "values/decimal-39-digits",
    (NUMBERS,),
    "39 significant digits do not fit",
    document=cond(eq({"$decimal": "1" + "0" * 38})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$decimal"},
)
read(
    "values/decimal-1e37",
    (NUMBERS,),
    "1e37 fits",
    document=cond(eq({"$decimal": "1e37"})),
    expect=cond(eq({"$decimal": "1" + "0" * 37})),
)
read(
    "values/decimal-1e38",
    (NUMBERS,),
    "1e38 does not fit",
    document=cond(eq({"$decimal": "1e38"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$decimal"},
)
read(
    "values/decimal-scale-39",
    (NUMBERS,),
    "a scale of 39 does not fit",
    document=cond(eq({"$decimal": "0." + "0" * 38 + "1"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$decimal"},
)
read(
    "values/decimal-trailing-zeros",
    (NUMBERS,),
    "trailing zeros are dropped only as far as needed: 1 fits with scale 37",
    document=cond(eq({"$decimal": "1." + "0" * 40})),
    expect=cond(eq({"$decimal": "1." + "0" * 37})),
)
read(
    "values/decimal-comma",
    (NUMBERS,),
    "a decimal comma is malformed",
    document=cond(eq({"$decimal": "1,5"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$decimal"},
)
read(
    "values/decimal-leading-point",
    (NUMBERS,),
    "a decimal needs a digit before the point",
    document=cond(eq({"$decimal": ".5"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$decimal"},
)

# --- Dates and datetimes ----------------------------------------------------------------

read(
    "values/date",
    (DATES,),
    "a date",
    document=cond(eq({"$date": "2026-10-31"})),
    expect=cond(eq({"$date": "2026-10-31"})),
)
read(
    "values/date-february-30",
    (DATES,),
    "a day that does not exist",
    document=cond(eq({"$date": "2026-02-30"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$date"},
)
read(
    "values/date-leap-day",
    (DATES,),
    "29 February in a leap year",
    document=cond(eq({"$date": "2028-02-29"})),
    expect=cond(eq({"$date": "2028-02-29"})),
)
read(
    "values/date-year-zero",
    (DATES,),
    "year 0000 is out of range",
    document=cond(eq({"$date": "0000-01-01"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$date"},
)
for slug, raw, canonical, what in (
    ("naive", "2026-10-31T09:30:00", "2026-10-31T09:30:00", "a naive datetime"),
    (
        "zero-fraction",
        "2026-10-31T09:30:00.000000",
        "2026-10-31T09:30:00",
        "a zero fraction is omitted",
    ),
    (
        "fraction-six-digits",
        "2026-10-31T09:30:00.5",
        "2026-10-31T09:30:00.500000",
        "a fraction is written with six digits",
    ),
    (
        "zero-offset",
        "2026-10-31T09:30:00+00:00",
        "2026-10-31T09:30:00Z",
        "a zero offset is written Z",
    ),
    (
        "negative-zero-offset",
        "2026-10-31T09:30:00-00:00",
        "2026-10-31T09:30:00Z",
        "-00:00 is read as Z",
    ),
    (
        "fixed-offset",
        "2026-10-31T09:30:00+05:30",
        "2026-10-31T09:30:00+05:30",
        "a fixed offset is kept",
    ),
    (
        "offset-seconds",
        "1900-01-01T00:00:00-00:14:44",
        "1900-01-01T00:00:00-00:14:44",
        "an offset with seconds",
    ),
    (
        "named-zone",
        "2026-10-31T09:30:00+01:00[Europe/Madrid]",
        "2026-10-31T09:30:00+01:00[Europe/Madrid]",
        "a datetime in a named zone",
    ),
    (
        "repeated-hour-first",
        "2026-10-25T02:30:00+02:00[Europe/Madrid]",
        "2026-10-25T02:30:00+02:00[Europe/Madrid]",
        "the first 02:30 of a repeated hour",
    ),
    (
        "repeated-hour-second",
        "2026-10-25T02:30:00+01:00[Europe/Madrid]",
        "2026-10-25T02:30:00+01:00[Europe/Madrid]",
        "the second 02:30 of a repeated hour",
    ),
    (
        "zero-offset-in-named-zone",
        "2026-10-31T09:30:00+00:00[Europe/London]",
        "2026-10-31T09:30:00+00:00[Europe/London]",
        "a zero offset in a named zone is not Z",
    ),
):
    read(
        f"values/datetime-{slug}",
        (DATES,),
        what,
        document=cond(eq({"$datetime": raw})),
        expect=cond(eq({"$datetime": canonical})),
    )
for slug, raw, what in (
    ("offset-beyond-18-hours", "2026-10-31T09:30:00+19:00", "an offset beyond 18 hours"),
    (
        "offset-18-hours-and-a-second",
        "2026-10-31T09:30:00+18:00:01",
        "an offset of 18 hours and a second",
    ),
    ("leap-second", "2026-10-31T23:59:60", "a leap second"),
    ("seven-fraction-digits", "2026-10-31T09:30:00.1234567", "seven fraction digits"),
    (
        "offset-not-the-zones",
        "2026-07-01T12:00:00+01:00[Europe/Madrid]",
        "an offset that does not match the zone",
    ),
    (
        "skipped-hour",
        "2026-03-29T02:30:00+01:00[Europe/Madrid]",
        "a time skipped by a clock change",
    ),
    ("unknown-zone", "2026-10-31T09:30:00+01:00[Mars/Olympus]", "an unknown zone"),
    ("zone-case", "2026-10-31T09:30:00+01:00[europe/madrid]", "zone names are case-sensitive"),
    ("zone-after-z", "2026-10-31T09:30:00Z[Europe/Madrid]", "a zone after Z"),
    ("zone-without-offset", "2026-10-31T09:30:00[Europe/Madrid]", "a zone without an offset"),
    ("critical-flag", "2026-10-31T09:30:00+01:00[!Europe/Madrid]", "a critical flag"),
    ("suffix-tag", "2026-10-31T09:30:00+01:00[Europe/Madrid][u-ca=japanese]", "a suffix tag"),
    ("space-separator", "2026-10-31 09:30:00", "a space instead of T"),
    ("instant-before-year-1", "0001-01-01T00:30:00+01:00", "an instant before year 1"),
    ("instant-after-year-9999", "9999-12-31T23:30:00-01:00", "an instant after year 9999"),
):
    read(
        f"values/datetime-invalid-{slug}",
        (DATES,),
        what,
        document=cond(eq({"$datetime": raw})),
        error={"code": "format_error", "pointer": VALUE_AT + "/$datetime"},
    )

# --- Typed values, lists and maps ------------------------------------------------------------

read(
    "values/unknown-typed-value",
    (LISTS,),
    "an unknown $-key points to that key",
    document=cond(eq({"$time": "09:30"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$time"},
)
read(
    "values/opaque-in-constant",
    (VALUES,),
    "an opaque value is never a constant",
    document=cond(eq({"$opaque": "bytes"})),
    error={"code": "format_error", "pointer": VALUE_AT},
)
read(
    "values/list-constant",
    (CONDITIONS,),
    "a list is not a scalar constant",
    document=cond(eq([1, 2])),
    error={"code": "format_error", "pointer": VALUE_AT},
)
read(
    "values/typed-with-extra-key",
    (LISTS,),
    "a typed value has a single key; the extra key is located",
    document=cond(eq({"$date": "2026-10-31", "zone": "UTC"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/zone"},
)

# --- Conditions ------------------------------------------------------------------------

a, b, c = (field(x) for x in "abc")
read(
    "conditions/and-flattened",
    (READING, WRITING),
    "a nested and is flattened",
    document=cond({"op": "and", "operands": [{"op": "and", "operands": [a, b]}, c]}),
    expect=cond({"op": "and", "operands": [a, b, c]}),
)
read(
    "conditions/and-one-operand-as-written",
    (READING,),
    "and needs two operands as written, before flattening",
    document=cond({"op": "and", "operands": [{"op": "and", "operands": [a, b]}]}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/or-not-flattened-into-and",
    (READING, WRITING),
    "an or inside an and is kept",
    document=cond({"op": "and", "operands": [{"op": "or", "operands": [a, b]}, c]}),
    expect=cond({"op": "and", "operands": [{"op": "or", "operands": [a, b]}, c]}),
)
read(
    "conditions/in-sorted",
    (READING, WRITING),
    "in values are sorted and deduplicated",
    document=cond({"op": "in", "operand": a, "values": ["b", "a", "b"]}),
    expect=cond({"op": "in", "operand": a, "values": ["a", "b"]}),
)
read(
    "conditions/in-number-types",
    (WRITING,),
    "equal numbers of different types are kept, decimal before float before int",
    source='{"version":1,"condition":{"op":"in","operand":{"op":"field","path":["a"]},"values":[2,1,1.0,{"$decimal":"1"}]}}',
    expect=cond({"op": "in", "operand": a, "values": [{"$decimal": "1"}, 1.0, 1, 2]}),
)
read(
    "conditions/in-same-node-first-kept",
    (WRITING,),
    "of two values that are the same node, the first is kept",
    source='{"version":1,"condition":{"op":"in","operand":{"op":"field","path":["a"]},"values":[-0.0,0.0,{"$decimal":"1.00"},{"$decimal":"1.0"}]}}',
    expect=cond({"op": "in", "operand": a, "values": [-0.0, {"$decimal": "1.00"}]}),
)
read(
    "conditions/in-strings-code-point",
    (WRITING,),
    "strings sort by code point",
    document=cond({"op": "in", "operand": a, "values": ["é", "z", "Z", "😀", "\uff5a"]}),
    expect=cond({"op": "in", "operand": a, "values": ["Z", "z", "é", "\uff5a", "😀"]}),
)
read(
    "conditions/in-mixed-groups",
    (READING,),
    "in values from two groups",
    document=cond({"op": "in", "operand": a, "values": [1, "1"]}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/in-null",
    (READING,),
    "a null in values",
    document=cond({"op": "in", "operand": a, "values": [None]}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/between-default-closed",
    (WRITING,),
    "closed is written even when omitted",
    document=cond({"op": "between", "operand": a, "low": const(1), "high": const(2)}),
    expect=cond(
        {"op": "between", "operand": a, "low": const(1), "high": const(2), "closed": "both"}
    ),
)
read(
    "conditions/between-bound-groups",
    (READING,),
    "constant bounds from two groups",
    document=cond(
        {"op": "between", "operand": a, "low": const("a"), "high": const(2), "closed": "both"}
    ),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/condition-as-operand",
    (READING,),
    "a condition as an operand of eq",
    document=cond(
        {"op": "eq", "left": {"op": "gt", "left": a, "right": const(1)}, "right": const(True)}
    ),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/value-as-condition",
    (READING,),
    "a value as the document's condition",
    document=cond(const(5)),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/null-in-compare",
    (READING,),
    "a null constant in a comparison",
    document=cond(eq(None)),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/null-under-not",
    (READING,),
    "a null constant under not",
    document=cond({"op": "not", "operand": const(None)}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/null-under-is-null",
    (READING,),
    "a null constant under is_null",
    document=cond({"op": "is_null", "operand": const(None)}),
    expect=cond({"op": "is_null", "operand": const(None)}),
)
read(
    "conditions/arithmetic-string",
    (READING,),
    "arithmetic with a string constant",
    document=cond(
        {
            "op": "gt",
            "left": {"op": "add", "left": const("a"), "right": const(1)},
            "right": const(0),
        }
    ),
    error={"code": "expression_error", "pointer": "/condition/left"},
)
read(
    "conditions/regex-invalid",
    (READING,),
    "a pattern that does not parse",
    document=cond({"op": "matches", "operand": a, "pattern": "("}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/regex-lookahead",
    (READING,),
    "lookaround is not in the syntax",
    document=cond({"op": "matches", "operand": a, "pattern": "a(?=b)"}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/regex-backreference",
    (READING,),
    "backreferences are not in the syntax",
    document=cond({"op": "matches", "operand": a, "pattern": "(a)\\1"}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/path-negative-index",
    (READING,),
    "a negative index",
    document=cond({"op": "eq", "left": field("a", -1), "right": const(1)}),
    error={"code": "expression_error", "pointer": "/condition/left"},
)
read(
    "conditions/path-empty",
    (READING,),
    "an empty path",
    document=cond({"op": "eq", "left": field(), "right": const(1)}),
    error={"code": "expression_error", "pointer": "/condition/left"},
)
read(
    "conditions/path-empty-segment",
    (READING,),
    "an empty string segment",
    document=cond(field("a", "")),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/path-float-index",
    (CONDITIONS,),
    "an index written 1.0 is not an integer literal",
    source='{"version":1,"condition":{"op":"field","path":["a",1.0]}}',
    error={"code": "format_error", "pointer": "/condition/path/1"},
)
read(
    "conditions/unknown-op",
    (CONDITIONS,),
    "an unknown op",
    document=cond({"op": "xor", "operands": [a, b]}),
    error={"code": "format_error", "pointer": "/condition/op"},
)
read(
    "conditions/missing-key",
    (CONDITIONS,),
    "a missing key",
    document=cond({"op": "eq", "left": a}),
    error={"code": "format_error", "pointer": "/condition/right"},
)
read(
    "conditions/unknown-key",
    (CONDITIONS,),
    "an unknown key in a node",
    document=cond({"op": "field", "path": ["a"], "note": "x"}),
    error={"code": "format_error", "pointer": "/condition/note"},
)
read(
    "conditions/shape-before-construction",
    (ERRORS,),
    "shape errors come before construction errors",
    document=cond({"op": "and", "operands": [{"op": "and", "operands": [a]}, {"op": "bad"}]}),
    error={"code": "format_error", "pointer": "/condition/operands/1/op"},
)
read(
    "conditions/keys-in-table-order",
    (ERRORS,),
    "shape errors follow the table's key order, not the text's",
    source='{"version":1,"condition":{"op":"eq","right":{"op":"bad"},"left":{"op":"bad"}}}',
    error={"code": "format_error", "pointer": "/condition/left/op"},
)
read(
    "conditions/post-order",
    (ERRORS,),
    "construction errors in post-order",
    document=cond(
        {
            "op": "or",
            "operands": [
                {"op": "and", "operands": [a]},
                {"op": "in", "operand": b, "values": [1, "1"]},
            ],
        }
    ),
    error={"code": "expression_error", "pointer": "/condition/operands/0"},
)
read(
    "conditions/canonical-key-order",
    (WRITING,),
    "keys are written in table order",
    source='{"condition":{"right":{"value":1,"op":"const"},"left":{"path":["a"],"op":"field"},"op":"eq"},"version":1}',
    expect=cond(eq(1)),
)

# --- Rules and rule sets ---------------------------------------------------------------------


def rule_doc(r: Json) -> Json:
    """A rule document."""
    return {"version": 1, "rule": r}


def set_doc(s: Json) -> Json:
    """A rule set document."""
    return {"version": 1, "ruleset": s}


when = {"op": "gt", "left": a, "right": const(1)}
read(
    "rules/defaults-omitted",
    (RULES,),
    "optional keys at their default are omitted",
    document=rule_doc(
        {"name": "r", "when": when, "then": {}, "priority": 0, "tags": [], "metadata": {}}
    ),
    expect=rule_doc({"name": "r", "when": when}),
)
read(
    "rules/tags-are-a-set",
    (RULES,),
    "tags are deduplicated and sorted",
    document=rule_doc({"name": "r", "when": when, "tags": ["b", "a", "b"]}),
    expect=rule_doc({"name": "r", "when": when, "tags": ["a", "b"]}),
)
read(
    "rules/priority-typed",
    (RULES,),
    "a priority as $int",
    document=rule_doc({"name": "r", "when": when, "priority": {"$int": "-5"}}),
    expect=rule_doc({"name": "r", "when": when, "priority": -5}),
)
read(
    "rules/key-order",
    (RULES,),
    "rule keys are written in table order",
    document=rule_doc({"tags": ["t"], "when": when, "name": "r"}),
    expect=rule_doc({"name": "r", "when": when, "tags": ["t"]}),
)
read(
    "rules/payload-order",
    (RULES,),
    "payload members keep their order",
    document=rule_doc(
        {
            "name": "r",
            "when": when,
            "then": {"b": const(1), "a": const(2), "10": const(3), "2": const(4)},
        }
    ),
    expect=rule_doc(
        {
            "name": "r",
            "when": when,
            "then": {"b": const(1), "a": const(2), "10": const(3), "2": const(4)},
        }
    ),
)
read(
    "rules/payload-null",
    (RULES,),
    "a null constant as a payload value",
    document=rule_doc({"name": "r", "when": when, "then": {"v": const(None)}}),
    expect=rule_doc({"name": "r", "when": when, "then": {"v": const(None)}}),
)
read(
    "rules/metadata-literal-keys",
    (RULES,),
    "metadata keys starting with $ are ordinary",
    document=rule_doc(
        {
            "name": "r",
            "when": when,
            "metadata": {"$owner": "sales", "since": {"$date": "2026-01-01"}},
        }
    ),
    expect=rule_doc(
        {
            "name": "r",
            "when": when,
            "metadata": {"$owner": "sales", "since": {"$date": "2026-01-01"}},
        }
    ),
)
read(
    "rules/empty-name",
    (RULES,),
    "an empty rule name",
    document=rule_doc({"name": "", "when": when}),
    error={"code": "rule_definition_error", "pointer": "/rule/name"},
)
read(
    "rules/value-as-when",
    (RULES,),
    "a value as a rule's condition",
    document=rule_doc({"name": "r", "when": {"op": "add", "left": a, "right": const(1)}}),
    error={"code": "rule_definition_error", "pointer": "/rule/when"},
)
read(
    "rules/null-as-when",
    (RULES,),
    "a null constant as a rule's condition",
    document=rule_doc({"name": "r", "when": const(None)}),
    error={"code": "expression_error", "pointer": "/rule/when"},
)
read(
    "rules/condition-in-payload",
    (RULES,),
    "a condition as a payload value",
    document=rule_doc({"name": "r", "when": when, "then": {"a/b": when}}),
    error={"code": "rule_definition_error", "pointer": "/rule/then/a~1b"},
)
read(
    "rules/action-key",
    (RULES,),
    "actions are not part of the format",
    document=rule_doc({"name": "r", "when": when, "do": "notify"}),
    error={"code": "format_error", "pointer": "/rule/do"},
)
read(
    "rules/bare-payload-scalar",
    (RULES,),
    "a payload value must be a node",
    document=rule_doc({"name": "r", "when": when, "then": {"d": 0.1}}),
    error={"code": "format_error", "pointer": "/rule/then/d"},
)
r1 = {"name": "r", "when": when}
read(
    "rulesets/defaults-written",
    (RULESETS,),
    "hit_policy and logic are always written",
    document=set_doc({"rules": []}),
    expect=set_doc({"hit_policy": "first", "logic": "boolean", "rules": []}),
)
read(
    "rulesets/strict-false-omitted",
    (RULESETS,),
    "strict false is omitted",
    document=set_doc({"strict": False, "rules": [r1]}),
    expect=set_doc({"hit_policy": "first", "logic": "boolean", "rules": [r1]}),
)
read(
    "rulesets/collect-aggregate",
    (RULESETS,),
    "an aggregate with collect",
    document=set_doc({"rules": [r1], "aggregate": "sum", "hit_policy": "collect", "strict": True}),
    expect=set_doc(
        {
            "hit_policy": "collect",
            "aggregate": "sum",
            "logic": "boolean",
            "strict": True,
            "rules": [r1],
        }
    ),
)
read(
    "rulesets/duplicate-names",
    (RULESETS,),
    "duplicate rule names point to the second",
    document=set_doc({"rules": [r1, r1]}),
    error={"code": "rule_definition_error", "pointer": "/ruleset/rules/1/name"},
)
read(
    "rulesets/aggregate-without-collect",
    (RULESETS,),
    "an aggregate without collect",
    document=set_doc({"hit_policy": "first", "aggregate": "sum", "rules": []}),
    error={"code": "rule_definition_error", "pointer": "/ruleset/aggregate"},
)
read(
    "rulesets/duplicates-before-aggregate",
    (RULESETS, ERRORS),
    "duplicate names are checked before the aggregate",
    document=set_doc({"aggregate": "sum", "rules": [r1, r1]}),
    error={"code": "rule_definition_error", "pointer": "/ruleset/rules/1/name"},
)
read(
    "rulesets/unknown-logic",
    (RULESETS,),
    "an unknown logic name",
    document=set_doc({"logic": "magic", "rules": []}),
    error={"code": "registry_error", "name": "magic", "pointer": "/ruleset/logic"},
)
read(
    "rulesets/uppercase-policy",
    (RULESETS,),
    "policy names are lowercase",
    document=set_doc({"hit_policy": "FIRST", "rules": []}),
    error={"code": "format_error", "pointer": "/ruleset/hit_policy"},
)
read(
    "rulesets/logic-object-reserved",
    (RULESETS,),
    "the object form of logic is reserved",
    document=set_doc({"logic": {"name": "boolean"}, "rules": []}),
    error={"code": "format_error", "pointer": "/ruleset/logic"},
)
read(
    "rulesets/alpha-reserved",
    (RULESETS,),
    "alpha is reserved",
    document=set_doc({"alpha": 0.5, "rules": []}),
    error={"code": "format_error", "pointer": "/ruleset/alpha"},
)

# --- Decisions --------------------------------------------------------------------------------

_rs = RuleSet([Rule("a", F.x > 1, then={"y": 1}), Rule("b", F.x > 5)])
_RS = encode.ruleset(_rs)
_DECISION = encode.decision(_rs.evaluate({"x": 2}))


def decision(change: Any = None) -> Json:
    """A valid decision document, optionally altered."""
    content = json.loads(json.dumps(_DECISION))
    if change is not None:
        change(content)
    return {"version": 1, "decision": content}


read(
    "decisions/valid",
    (READING_DECISION,),
    "a decision the core could produce",
    document=decision(),
    ruleset=_RS,
    expect=decision(),
)
read(
    "decisions/other-policy",
    (READING_DECISION,),
    "a decision with another policy",
    document=decision(lambda d: d.update(hit_policy="priority")),
    ruleset=_RS,
    error={"code": "decision_mismatch", "pointer": "/decision/hit_policy"},
)
read(
    "decisions/other-rule",
    (READING_DECISION,),
    "a decision naming another rule",
    document=decision(lambda d: d["results"][1].update(rule="c")),
    ruleset=_RS,
    error={"code": "decision_mismatch", "pointer": "/decision/results/1/rule"},
)
read(
    "decisions/other-constant",
    (READING_DECISION,),
    "a tree with another constant",
    document=decision(lambda d: d["results"][0]["when"]["children"][1].update(value=2)),
    ruleset=_RS,
    error={"code": "decision_mismatch", "pointer": "/decision/results/0/when/children/1/value"},
)
read(
    "decisions/wrong-truth",
    (READING_DECISION,),
    "a truth value evaluation would not give",
    document=decision(lambda d: d["results"][1]["when"].update(truth=True)),
    ruleset=_RS,
    error={"code": "decision_mismatch", "pointer": "/decision/results/1/when/truth"},
)
read(
    "decisions/wrong-fired",
    (READING_DECISION,),
    "fired disagrees with the truth value",
    document=decision(lambda d: d["results"][1].update(fired=True)),
    ruleset=_RS,
    error={"code": "decision_mismatch", "pointer": "/decision/results/1/fired"},
)
read(
    "decisions/missing-then",
    (READING_DECISION,),
    "the selected rule's payload is missing",
    document=decision(lambda d: d["results"][0].pop("then")),
    ruleset=_RS,
    error={"code": "decision_mismatch", "pointer": "/decision/results/0/then"},
)
read(
    "decisions/unselected-with-then",
    (READING_DECISION,),
    "then is judged from selected as written: an unselected rule must not have it",
    document=decision(lambda d: d.update(selected=[])),
    ruleset=_RS,
    error={"code": "decision_mismatch", "pointer": "/decision/results/0/then"},
)

# --- Limits ---------------------------------------------------------------------------------


def nots(n: int) -> Json:
    """`n` nested nots around a field."""
    node: Json = a
    for _ in range(n):
        node = {"op": "not", "operand": node}
    return node


read(
    "limits/depth-at-limit",
    (ERRORS,),
    "a condition exactly 256 nodes deep is read",
    document=cond(nots(255)),
    expect=cond(nots(255)),
)
read(
    "limits/depth-over-limit",
    (ERRORS,),
    "a condition 257 nodes deep is rejected at its deepest node",
    document=cond(nots(256)),
    error={"code": "limit_exceeded", "pointer": "/condition" + "/operand" * 256},
)
read(
    "limits/depth-custom",
    (ERRORS,),
    "the caller can lower the node depth",
    document=cond(nots(3)),
    limits={"max_depth": 3},
    error={"code": "limit_exceeded", "pointer": "/condition/operand/operand/operand"},
)
small = text(cond(field("a")))
read(
    "limits/nesting-at-limit",
    (ERRORS,),
    "JSON nesting exactly at the limit",
    source=small,
    limits={"max_nesting": 3},
    expect=cond(field("a")),
)
read(
    "limits/nesting-over-limit",
    (ERRORS,),
    "the bracket that exceeds the nesting limit",
    source=small,
    limits={"max_nesting": 2},
    error={"code": "limit_exceeded", "line": 1, "column": column(small, "[")},
)
read(
    "limits/size",
    (ERRORS,),
    "a document larger than the size limit",
    source=small,
    limits={"max_size": 10},
    error={"code": "limit_exceeded"},
)


# --- Added after review: precedence, ordering, positions, edges ---------------------------

_ANY = encode.ruleset(
    RuleSet(
        [Rule("a", F.x > 1, then={"y": 1}), Rule("b", F.x > 0, then={"y": 1})], hit_policy="any"
    )
)
_ANY_DECISION = encode.decision(
    RuleSet(
        [Rule("a", F.x > 1, then={"y": 1}), Rule("b", F.x > 0, then={"y": 1})], hit_policy="any"
    ).evaluate({"x": 2})
)
_bad_any = json.loads(json.dumps(_ANY_DECISION))
_bad_any["selected"] = ["b"]
read(
    "decisions/wrong-selection",
    (READING_DECISION,),
    "a selection the policy would not make",
    document={"version": 1, "decision": _bad_any},
    ruleset=_ANY,
    error={"code": "decision_mismatch", "pointer": "/decision/selected"},
)

emoji = '{"version":1,"condition":{"op":"field","path":["\U0001f600\u00e9",]}}'
read(
    "documents/column-in-code-points",
    (ERRORS,),
    "columns count code points, not bytes or UTF-16 units",
    source=emoji,
    error={"code": "format_error", "line": 1, "column": column(emoji, ",]") + 1},
)
cut = '{"version":1,'
read(
    "documents/unexpected-end",
    (ERRORS,),
    "an unexpected end is one past the last character",
    source=cut,
    error={"code": "format_error", "line": 1, "column": len(cut) + 1},
)
crlf = '{\r\n"version":1,\r\n"condition":{"op":"field","path":["a"],}\r\n}'
read(
    "documents/crlf-lines",
    (ERRORS,),
    "a CRLF is one line ending",
    source=crlf,
    error={"code": "format_error", "line": 3, "column": crlf.split("\n")[2].index(",}") + 2},
)
read(
    "documents/top-level-array",
    (DOCUMENTS,),
    "a top level that is not an object",
    source="[1]",
    error={"code": "format_error", "pointer": ""},
)
read(
    "documents/explanation-content-not-checked",
    (DOCUMENTS,),
    "an output-only document is rejected before its content is read",
    document={"version": 1, "explanation": {"nonsense": True}},
    error={"code": "unsupported"},
)

read(
    "values/float-nan-text",
    (NUMBERS,),
    "NaN has no encoding",
    document=cond(eq({"$float": "nan"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$float"},
)
read(
    "values/float-tagged-string",
    (NUMBERS,),
    "$float text other than inf",
    document=cond(eq({"$float": "1.5"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$float"},
)
read(
    "values/float-underflow",
    (NUMBERS,),
    "a literal too small reads as 0.0",
    source=value_text("1e-400"),
    expect=cond(eq(0.0)),
)
read(
    "values/float-largest",
    (NUMBERS,),
    "a literal that rounds to the largest float",
    source=value_text("1.7976931348623158e308"),
    expect=cond(eq(1.7976931348623157e308)),
)
read(
    "values/float-rounds-to-infinity",
    (NUMBERS,),
    "a literal that rounds to infinity",
    source=value_text("1.7976931348623159e308"),
    error={"code": "format_error", "pointer": VALUE_AT},
)
read(
    "values/int-negative-safe-edge",
    (NUMBERS, VALUES),
    "-2^53 is written as $int text",
    source=value_text("-9007199254740992"),
    expect=cond(eq({"$int": "-9007199254740992"})),
)
read(
    "values/int-tagged-overflow",
    (NUMBERS,),
    "$int text outside 64 bits",
    document=cond(eq({"$int": "9223372036854775808"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$int"},
)
read(
    "values/int-tagged-plus-sign",
    (NUMBERS,),
    "$int text with a plus sign",
    document=cond(eq({"$int": "+5"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$int"},
)
read(
    "values/decimal-plus-infinity",
    (NUMBERS,),
    "a decimal written +inf",
    document=cond(eq({"$decimal": "+inf"})),
    error={"code": "format_error", "pointer": VALUE_AT + "/$decimal"},
)
read(
    "values/datetime-offset-18-hours",
    (DATES,),
    "an offset of exactly 18 hours",
    document=cond(eq({"$datetime": "2026-10-31T09:30:00-18:00"})),
    expect=cond(eq({"$datetime": "2026-10-31T09:30:00-18:00"})),
)
read(
    "values/datetime-alias-kept",
    (DATES,),
    "zone aliases are kept as written",
    document=cond(eq({"$datetime": "2026-10-31T09:30:00+05:30[Asia/Calcutta]"})),
    expect=cond(eq({"$datetime": "2026-10-31T09:30:00+05:30[Asia/Calcutta]"})),
)
read(
    "values/datetime-zone-utc",
    (DATES,),
    "the named zone UTC is not written Z",
    document=cond(eq({"$datetime": "2026-10-31T09:30:00+00:00[UTC]"})),
    expect=cond(eq({"$datetime": "2026-10-31T09:30:00+00:00[UTC]"})),
)

madrid, z930, z900 = (
    {"$datetime": t}
    for t in (
        "2026-10-31T10:30:00+01:00[Europe/Madrid]",
        "2026-10-31T09:30:00Z",
        "2026-10-31T09:00:00Z",
    )
)
read(
    "conditions/in-aware-by-instant",
    (WRITING,),
    "aware datetimes sort by instant; of equal instants the first is kept",
    document=cond({"op": "in", "operand": a, "values": [madrid, z930, z900]}),
    expect=cond({"op": "in", "operand": a, "values": [z900, madrid]}),
)
read(
    "conditions/in-booleans",
    (WRITING,),
    "false before true",
    document=cond({"op": "in", "operand": a, "values": [True, False, True]}),
    expect=cond({"op": "in", "operand": a, "values": [False, True]}),
)
read(
    "conditions/in-dates",
    (WRITING,),
    "dates in time order",
    document=cond(
        {"op": "in", "operand": a, "values": [{"$date": "2026-10-31"}, {"$date": "2026-01-01"}]}
    ),
    expect=cond(
        {"op": "in", "operand": a, "values": [{"$date": "2026-01-01"}, {"$date": "2026-10-31"}]}
    ),
)
read(
    "conditions/in-naive-and-aware",
    (READING,),
    "naive and aware datetimes are two groups",
    document=cond(
        {"op": "in", "operand": a, "values": [{"$datetime": "2026-10-31T09:30:00"}, z930]}
    ),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/value-under-and",
    (READING,),
    "a value as an operand of and",
    document=cond({"op": "and", "operands": [a, {"op": "add", "left": b, "right": const(1)}]}),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/between-boolean-bounds",
    (READING,),
    "booleans are not orderable bounds",
    document=cond(
        {"op": "between", "operand": a, "low": const(False), "high": const(True), "closed": "both"}
    ),
    error={"code": "expression_error", "pointer": "/condition"},
)
read(
    "conditions/between-invalid-closed",
    (CONDITIONS,),
    "an invalid closed",
    document=cond(
        {"op": "between", "operand": a, "low": const(1), "high": const(2), "closed": "open"}
    ),
    error={"code": "format_error", "pointer": "/condition/closed"},
)
read(
    "conditions/path-index-out-of-range",
    (CONDITIONS,),
    "a path index outside the int range",
    source='{"version":1,"condition":{"op":"field","path":["a",9223372036854775808]}}',
    error={"code": "format_error", "pointer": "/condition/path/1"},
)
read(
    "conditions/path-boolean-segment",
    (CONDITIONS,),
    "a boolean path segment",
    document=cond(field("a", True)),
    error={"code": "format_error", "pointer": "/condition/path/1"},
)
read(
    "conditions/known-keys-before-unknown",
    (ERRORS,),
    "an object's known keys are checked before its unknown keys",
    source='{"version":1,"condition":{"note":1,"op":"field","path":["a",1.0]}}',
    error={"code": "format_error", "pointer": "/condition/path/1"},
)
read(
    "conditions/missing-before-bad-sibling",
    (ERRORS,),
    "a missing key is reported at its place in the table",
    document=cond({"op": "eq", "right": {"op": "bad"}}),
    error={"code": "format_error", "pointer": "/condition/left"},
)
read(
    "rules/construction-before-rule-rules",
    (ERRORS,),
    "a node's construction error comes before the rule's own rules",
    document=rule_doc({"name": "", "when": const(None)}),
    error={"code": "expression_error", "pointer": "/rule/when"},
)
read(
    "rules/name-before-when",
    (ERRORS,),
    "the rule's own rules in order: the name first",
    document=rule_doc({"name": "", "when": {"op": "add", "left": a, "right": const(1)}}),
    error={"code": "rule_definition_error", "pointer": "/rule/name"},
)
read(
    "rules/tags-code-point-order",
    (RULES,),
    "tags sort by code point",
    document=rule_doc(
        {"name": "r", "when": when, "tags": ["\u00e9", "z", "Z", "\U0001f600", "\uff5a"]}
    ),
    expect=rule_doc(
        {"name": "r", "when": when, "tags": ["Z", "z", "\u00e9", "\uff5a", "\U0001f600"]}
    ),
)
read(
    "rulesets/rule-errors-first",
    (ERRORS,),
    "a rule's errors come before the rule set's",
    document=set_doc({"aggregate": "sum", "rules": [{"name": "", "when": when}]}),
    error={"code": "rule_definition_error", "pointer": "/ruleset/rules/0/name"},
)
read(
    "rulesets/aggregate-before-logic",
    (RULESETS,),
    "the aggregate is checked before the logic",
    document=set_doc({"aggregate": "sum", "logic": "magic", "rules": []}),
    error={"code": "rule_definition_error", "pointer": "/ruleset/aggregate"},
)
read(
    "limits/depth-first-too-deep",
    (ERRORS,),
    "the node too deep is the first at depth limit + 1 in pre-order",
    document=cond(nots(260)),
    error={"code": "limit_exceeded", "pointer": "/condition" + "/operand" * 256},
)
deep = (
    '{"version":1,"condition":{"op":"in","operand":{"op":"field","path":["a"]},"values":'
    + "[" * 1023
    + "1"
    + "]" * 1023
    + "}}"
)
read(
    "limits/nesting-default",
    (ERRORS,),
    "the default JSON nesting limit is 1024",
    source=deep,
    error={"code": "limit_exceeded", "line": 1, "column": deep.index("[" * 1023) + 1023},
)
