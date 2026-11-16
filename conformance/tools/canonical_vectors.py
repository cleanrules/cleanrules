"""Generate and check the vectors of canonical bytes and digests.

The expected bytes and digests come from an oracle independent of the core: it
follows `spec/json-format.md` (*Canonical bytes*, *Digests*) with Python's own
JSON parser (keeping each number's literal), float `repr` for the shortest
digits, `decimal.Decimal` for decimal text and `hashlib.sha256`. The documents
below are written in their canonical structure (defaults written, `in` values
sorted), so the oracle only canonicalizes values and lays the bytes out; the
few vectors that check the structure give their canonical form explicitly.

    python3 conformance/tools/canonical_vectors.py --write   # canonical.json, digests.json
    python3 conformance/tools/canonical_vectors.py --check   # CI: files up to date, and
                                                              # bytes and digests recomputed
"""

from __future__ import annotations

import decimal
import hashlib
import json
import math
import re
import sys
from pathlib import Path
from typing import Any

VECTORS = Path(__file__).resolve().parent.parent / "vectors"
SAFE = 2**53 - 1


class IntLit(str):
    """An integer literal, as written."""


class FloatLit(str):
    """A number literal with a fraction or an exponent, as written."""


def no_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    """An object's members, refusing a repeated key."""
    keys = [k for k, _ in pairs]
    if len(set(keys)) != len(keys):
        raise ValueError(f"duplicate keys in {keys}")
    return dict(pairs)


def load(text: str) -> Any:
    """Parse JSON, keeping number literals and rejecting duplicate keys."""
    return json.loads(text, parse_int=IntLit, parse_float=FloatLit, object_pairs_hook=no_duplicates)


def dump(value: Any, indent: int = 0) -> str:
    """Pretty JSON that writes number literals as they are."""
    pad = "  " * (indent + 1)
    if isinstance(value, (IntLit, FloatLit)):
        return str(value)
    if isinstance(value, dict):
        if not value:
            return "{}"
        items = [
            f"{pad}{json.dumps(k, ensure_ascii=False)}: {dump(v, indent + 1)}"
            for k, v in value.items()
        ]
        return "{\n" + ",\n".join(items) + "\n" + "  " * indent + "}"
    if isinstance(value, list):
        if not value:
            return "[]"
        items = [f"{pad}{dump(v, indent + 1)}" for v in value]
        return "[\n" + ",\n".join(items) + "\n" + "  " * indent + "]"
    return json.dumps(value, ensure_ascii=False)


# --- The oracle: canonical values and bytes -----------------------------------


def float_text(value: float) -> str:
    """A finite float's literal (*Canonical bytes*).

    Python's shortest digits (nearest, ties to even), plain from 1e-4 to below
    1e16, and `d.ddde-7` otherwise.
    """
    assert math.isfinite(value)
    text = repr(value)
    if "e" in text:
        mantissa, exponent = text.split("e")
        mantissa = mantissa.removesuffix(".0")
        return f"{mantissa}e{int(exponent)}"
    return text


def int_value(n: int) -> Any:
    """An int in canonical form: bare up to 2^53 - 1, typed beyond."""
    return IntLit(str(n)) if abs(n) <= SAFE else {"$int": str(n)}


def float_value(value: float) -> Any:
    """A float in canonical form."""
    if math.isinf(value):
        return {"$float": "inf" if value > 0 else "-inf"}
    return FloatLit(float_text(value))


def decimal_text(text: str) -> str:
    """A decimal's canonical text: plain, scale kept, zero unsigned."""
    if text in ("inf", "-inf"):
        return text
    plain = format(decimal.Decimal(text), "f")
    return plain.removeprefix("-") if decimal.Decimal(text) == 0 else plain


def datetime_text(text: str) -> str:
    """A datetime's text as writers write it (*Dates and datetimes*)."""
    match = re.fullmatch(
        r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{1,6}))?"
        r"(Z|[+-]\d{2}:\d{2}(?::\d{2})?)?(\[[^\]]+\])?",
        text,
    )
    assert match, text
    base, fraction, offset, zone = match.groups()
    out = base
    if fraction and int(fraction):
        out += "." + fraction.ljust(6, "0")
    if offset:
        if offset.endswith(":00") and offset.count(":") == 2:
            offset = offset[:-3]
        if offset in ("+00:00", "-00:00") and not zone:
            offset = "Z"
        out += offset
    return out + (zone or "")


def canonical_value(value: Any) -> Any:
    """The canonical form of the values in a document in canonical structure.

    Numbers, typed values and `$map` (*Values*); `$schema` is dropped.
    """
    if isinstance(value, IntLit):
        return int_value(int(value))
    if isinstance(value, FloatLit):
        return float_value(float(value))
    if isinstance(value, list):
        return [canonical_value(v) for v in value]
    if isinstance(value, dict):
        typed = [k for k in value if k.startswith("$")]
        if len(value) == 1 and typed:
            (tag,) = typed
            inner = value[tag]
            if tag == "$int":
                return int_value(int(inner))
            if tag == "$float":
                if inner in ("inf", "-inf"):
                    return {"$float": inner}
                return float_value(float(inner))
            if tag == "$decimal":
                return {"$decimal": decimal_text(inner)}
            if tag == "$datetime":
                return {"$datetime": datetime_text(inner)}
            if tag == "$map":
                members = {k: canonical_value(v) for k, v in inner.items()}
                return {"$map": members} if any(k.startswith("$") for k in inner) else members
            return dict(value)
        return {k: canonical_value(v) for k, v in value.items() if k != "$schema"}
    return value


def string_bytes(text: str) -> str:
    """A string with the escapes of *Canonical bytes* (RFC 8785's)."""
    out = ['"']
    for c in text:
        if c == '"':
            out.append('\\"')
        elif c == "\\":
            out.append("\\\\")
        elif c in "\b\t\n\f\r":
            out.append({"\b": "\\b", "\t": "\\t", "\n": "\\n", "\f": "\\f", "\r": "\\r"}[c])
        elif ord(c) < 0x20:
            out.append(f"\\u{ord(c):04x}")
        else:
            out.append(c)
    out.append('"')
    return "".join(out)


def canonical_text(value: Any) -> str:
    """The canonical JSON text of a canonical form (*Canonical bytes*)."""
    if value is None:
        return "null"
    if value is True:
        return "true"
    if value is False:
        return "false"
    if isinstance(value, IntLit):
        n = int(value)
        assert str(n) == value, value
        return value
    if isinstance(value, FloatLit):
        assert float_text(float(value)) == value, value
        return value
    if isinstance(value, str):
        return string_bytes(value)
    if isinstance(value, list):
        return "[" + ",".join(canonical_text(v) for v in value) + "]"
    assert isinstance(value, dict), value
    # Python compares strings by code point.
    members = sorted(value.items())
    return "{" + ",".join(f"{string_bytes(k)}:{canonical_text(v)}" for k, v in members) + "}"


def digest(text: str) -> str:
    """The digest of canonical text (*Digests*)."""
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def ruleset_document(content: Any) -> dict[str, Any]:
    """The rule set document of a rule set's content."""
    return {"version": IntLit("1"), "ruleset": content}


def default_ruleset(rule: Any) -> Any:
    """The rule set of one rule with the default settings."""
    return {"hit_policy": "first", "logic": "boolean", "rules": [rule]}


# --- The documents --------------------------------------------------------------

FIELD_X = '{"op": "field", "path": ["x"]}'
EXAMPLE = """{
  "version": 1,
  "ruleset": {
    "hit_policy": "first",
    "logic": "boolean",
    "rules": [
      { "name": "vip",
        "when": { "op": "gt", "left": { "op": "field", "path": ["years"] },
                  "right": { "op": "const", "value": 5 } },
        "then": { "discount": { "op": "const", "value": { "$decimal": "0.15" } } } }
    ]
  }
}"""


def payload_ruleset(values: dict[str, str], name: str = "r") -> str:
    """A rule set of one rule whose payload holds each value as a constant."""
    then = ", ".join(
        f'{json.dumps(key)}: {{"op": "const", "value": {value}}}' for key, value in values.items()
    )
    return (
        '{"version": 1, "ruleset": {"hit_policy": "first", "logic": "boolean", "rules": ['
        f'{{"name": "{name}", "when": {{"op": "const", "value": true}}, "then": {{{then}}}}}]}}}}'
    )


def condition(node: str) -> str:
    """A condition document's text."""
    return f'{{"version": 1, "condition": {node}}}'


def rule_with(extra: str) -> str:
    """A rule document's text, with extra keys."""
    return (
        '{"version": 1, "rule": {"name": "r", "when": {"op": "gt", "left": '
        f'{FIELD_X}, "right": {{"op": "const", "value": 1}}}}{extra}}}}}'
    )


def table_basic() -> tuple[Any, Any]:
    """The table of `format/tables/basic`, and the rule set it compiles to."""
    file = json.loads((VECTORS / "format" / "tables.json").read_text(encoding="utf-8"))
    (vector,) = [v for v in file["vectors"] if v["id"] == "format/tables/basic"]
    document = load(json.dumps(vector["expect"]["document"]))
    compiled = load(json.dumps(vector["expect"]["compiled"]))
    return document, compiled


# (name, description, given text, explicit canonical form or None, spec sections)
CANONICAL = "json-format.md#canonical-bytes"
DIGESTS = "json-format.md#digests"
RULESET_DIGESTS = "json-format.md#rule-set-digests"

CASES: list[tuple[str, str, str, Any, list[str]]] = [
    (
        "ruleset-example",
        "the rule set of the spec's example: members sorted by code point at every level",
        EXAMPLE,
        None,
        [CANONICAL, DIGESTS],
    ),
    (
        "layout-and-member-order",
        "the same rule set with other whitespace, member order, escapes and number layout:"
        " the same bytes and digest",
        '{ "ruleset" : {"rules":[{"then":{"discount":{"value":{"$decimal":"1.5e-1"},'
        '"op":"const"}},\n"when":{"right":{"value":{"$int":"005"},"op":"const"},"op":"gt",'
        '"left":{"path":["y\\u0065ars"],"op":"field"}},"name":"\\u0076ip"}],\r\n'
        '"logic":"boolean","hit_policy":"first"},"$schema":"x","version":1}',
        load(EXAMPLE),
        [CANONICAL, DIGESTS],
    ),
    (
        "condition",
        "a condition document: op is not first, and a condition gives no rule set",
        condition('{"op": "gt", "left": ' + FIELD_X + ', "right": {"op": "const", "value": 1}}'),
        None,
        [CANONICAL, DIGESTS],
    ),
    (
        "rule",
        "a rule document with every optional key: its digest, and the rule set digest"
        " of a rule set of that rule with the default settings",
        rule_with(
            ', "then": {"y": {"op": "const", "value": "z"}}, "priority": 10,'
            ' "tags": ["b", "a"], "description": "d", "metadata": {"owner": "ops"},'
            ' "do": "notify"'
        ),
        load(
            rule_with(
                ', "then": {"y": {"op": "const", "value": "z"}}, "priority": 10,'
                ' "tags": ["a", "b"], "description": "d", "metadata": {"owner": "ops"},'
                ' "do": "notify"'
            )
        ),
        [CANONICAL, RULESET_DIGESTS],
    ),
    (
        "rule-description",
        "the same rule with another description: another digest",
        rule_with(
            ', "then": {"y": {"op": "const", "value": "z"}}, "priority": 10,'
            ' "tags": ["a", "b"], "description": "e", "metadata": {"owner": "ops"},'
            ' "do": "notify"'
        ),
        None,
        [DIGESTS],
    ),
    (
        "decision-table",
        "a decision table: the digest of its own document, and the rule set digest of"
        " the rule set it compiles to",
        dump(table_basic()[0]),
        None,
        [CANONICAL, RULESET_DIGESTS],
    ),
    (
        "flipped-comparison",
        "x > 1 written as 1 < x decides alike but is another artifact: another digest",
        condition('{"op": "lt", "left": {"op": "const", "value": 1}, "right": ' + FIELD_X + "}"),
        None,
        [DIGESTS],
    ),
    (
        "in-normalized",
        "in values are normalized before the bytes are written: duplicates and order"
        " of the text do not change the digest",
        condition(
            '{"op": "in", "operand": ' + FIELD_X + ', "values": [3, 1, 2, 1, {"$int": "2"}]}'
        ),
        load(condition('{"op": "in", "operand": ' + FIELD_X + ', "values": [1, 2, 3]}')),
        [CANONICAL],
    ),
    (
        "ints",
        "ints: bare up to 2^53 - 1, typed beyond, no leading zeros, no -0",
        condition(
            '{"op": "in", "operand": ' + FIELD_X + ', "values": [{"$int": "-9223372036854775808"},'
            ' -9007199254740992, -9007199254740991, -7, -0, {"$int": "007"},'
            ' 9007199254740991, 9007199254740992, {"$int": "9223372036854775807"}]}'
        ),
        load(
            condition(
                '{"op": "in", "operand": ' + FIELD_X + ', "values": ['
                '{"$int": "-9223372036854775808"}, {"$int": "-9007199254740992"},'
                ' -9007199254740991, -7, 0, 7, 9007199254740991, {"$int": "9007199254740992"},'
                ' {"$int": "9223372036854775807"}]}'
            )
        ),
        [CANONICAL],
    ),
    (
        "priority-beyond-2-53",
        "a priority beyond 2^53 - 1 is a typed int",
        rule_with(', "priority": 9007199254740993'),
        None,
        [CANONICAL],
    ),
    (
        "floats-plain",
        "floats with an exponent from -4 to 15 in plain notation, with a point",
        payload_ruleset(
            {
                "a": '{"$float": 1}',
                "b": "1e2",
                "c": "0.1",
                "d": "0.0001",
                "e": "-7.5",
                "f": "1e15",
                "g": "9999999999999998.0",
                "h": "123.456",
                "i": "0.0",
                "j": "-0.0",
            }
        ),
        None,
        [CANONICAL],
    ),
    (
        "floats-exponent",
        "floats with an exponent below -4 or above 15 in exponent notation, without + or"
        " leading zeros",
        payload_ruleset(
            {
                "a": "0.00001",
                "b": "1.5E-7",
                "c": "1e16",
                "d": "1.5e+16",
                "e": "1.7976931348623157e308",
                "f": "5e-324",
                "g": "2.2250738585072014e-308",
                "h": "-1e-05",
                "i": "123456789012345678.0",
            }
        ),
        None,
        [CANONICAL],
    ),
    (
        "floats-shortest",
        "the shortest digits that read back, and of two equally near the even one",
        payload_ruleset(
            {
                "a": "0.30000000000000004",
                "b": "0.1000000000000000055511151231257827",
                "c": "201409170638417.125",
                "d": "-881490291707823.25",
                "e": "2.5e-5",
            }
        ),
        None,
        [CANONICAL],
    ),
    (
        "floats-infinite",
        "infinite floats are typed values",
        payload_ruleset({"a": '{"$float": "inf"}', "b": '{"$float": "-inf"}'}),
        None,
        [CANONICAL],
    ),
    (
        "decimals",
        "decimals in their canonical text: plain, no leading zeros, scale kept, zero unsigned",
        payload_ruleset(
            {
                "a": '{"$decimal": "007.50"}',
                "b": '{"$decimal": "1.5e-3"}',
                "c": '{"$decimal": "1e2"}',
                "d": '{"$decimal": "-0.00"}',
                "e": '{"$decimal": "inf"}',
                "f": '{"$decimal": "-inf"}',
                "g": '{"$decimal": "-12345678901234567890.123456789012345678"}',
                "h": '{"$decimal": "1.5E+3"}',
            }
        ),
        None,
        [CANONICAL],
    ),
    (
        "decimal-scale-1",
        "the decimal 1.0",
        payload_ruleset({"price": '{"$decimal": "1.0"}'}),
        None,
        [DIGESTS],
    ),
    (
        "decimal-scale-2",
        "the decimal 1.00, equal to 1.0 but of another scale: other bytes, another digest",
        payload_ruleset({"price": '{"$decimal": "1.00"}'}),
        None,
        [DIGESTS],
    ),
    (
        "dates-and-datetimes",
        "dates and datetimes as writers write them: six fraction digits or none, Z for a"
        " zero offset, offset seconds only when not zero, the zone name",
        payload_ruleset(
            {
                "a": '{"$date": "2026-10-31"}',
                "b": '{"$datetime": "2026-10-31T09:30:00"}',
                "c": '{"$datetime": "2026-10-31T09:30:00.5"}',
                "d": '{"$datetime": "2026-10-31T09:30:00.000"}',
                "e": '{"$datetime": "2026-10-31T09:30:00-00:00"}',
                "f": '{"$datetime": "2026-10-31T09:30:00+00:00"}',
                "g": '{"$datetime": "2026-10-31T09:30:00.123456+05:30:15"}',
                "h": '{"$datetime": "2026-10-31T09:30:00+01:00:00"}',
                "i": '{"$datetime": "2026-10-31T09:30:00+01:00[Europe/Madrid]"}',
                "j": '{"$datetime": "2026-01-15T12:00:00+00:00[Europe/London]"}',
            }
        ),
        load(
            payload_ruleset(
                {
                    "a": '{"$date": "2026-10-31"}',
                    "b": '{"$datetime": "2026-10-31T09:30:00"}',
                    "c": '{"$datetime": "2026-10-31T09:30:00.500000"}',
                    "d": '{"$datetime": "2026-10-31T09:30:00"}',
                    "e": '{"$datetime": "2026-10-31T09:30:00Z"}',
                    "f": '{"$datetime": "2026-10-31T09:30:00Z"}',
                    "g": '{"$datetime": "2026-10-31T09:30:00.123456+05:30:15"}',
                    "h": '{"$datetime": "2026-10-31T09:30:00+01:00"}',
                    "i": '{"$datetime": "2026-10-31T09:30:00+01:00[Europe/Madrid]"}',
                    "j": '{"$datetime": "2026-01-15T12:00:00+00:00[Europe/London]"}',
                }
            )
        ),
        [CANONICAL],
    ),
    (
        "strings-escapes",
        "strings escape only quote, backslash and the characters below U+0020;"
        " slash, DEL, U+2028, U+2029 and noncharacters are their own UTF-8 bytes",
        payload_ruleset(
            {
                "a": '"quote \\" backslash \\\\ slash \\/ /"',
                "b": '"\\u0000\\u0001\\b\\t\\n\\u000b\\f\\r\\u000e\\u001f\\u0020"',
                "c": '"\\u007f\\u0080\\u2028\\u2029\\uffff\\ufffe\\ud83d\\ude00"',
                "d": '"\\u00e9 \\u0065\\u0301 caf\u00e9 \u65e5\u672c"',
            }
        ),
        None,
        [CANONICAL],
    ),
    (
        "string-precomposed",
        "a rule named with a precomposed é (U+00E9)",
        payload_ruleset({"a": "1"}, name="caf\u00e9"),
        None,
        [DIGESTS],
    ),
    (
        "string-decomposed",
        "the same name with e and a combining acute accent: never normalized, another digest",
        payload_ruleset({"a": "1"}, name="cafe\u0301"),
        None,
        [DIGESTS],
    ),
    (
        "keys-by-code-point",
        "keys sorted by code point, not by UTF-16 code unit: U+FF5A before U+1F600",
        rule_with(
            ', "then": {"\\ud83d\\ude00": {"op": "const", "value": 1},'
            ' "\\uff5a": {"op": "const", "value": 2}, "Z": {"op": "const", "value": 3},'
            ' "a": {"op": "const", "value": 4}, "\\u00e9": {"op": "const", "value": 5}},'
            ' "metadata": {"\\ud83d\\ude00": 1, "\\uff5a": 2, "a b": 3, "a": 4}'
        ),
        None,
        [CANONICAL],
    ),
    (
        "data-in-metadata",
        "lists and maps in metadata: lists in order, maps sorted, $map only for a"
        " key that starts with $",
        rule_with(
            ', "metadata": {"list": [3, 1.0, "a", null, true, {"$decimal": "1.10"}, []],'
            ' "map": {"b": {"$map": {"y": 1, "x": 2}}, "a": {}},'
            ' "dollar": {"$map": {"$ref": 1, "b": {"$map": {"$id": "x"}}}}}'
        ),
        None,
        [CANONICAL],
    ),
    (
        "paths-and-round",
        "path indexes and round's scale are integer literals; round's mode is written",
        condition(
            '{"op": "gt", "left": {"op": "round", "operand": {"op": "field", "path":'
            ' ["items", 0, "price"]}, "scale": 2}, "right": {"op": "const", "value":'
            ' {"$decimal": "9.99"}}}'
        ),
        load(
            condition(
                '{"op": "gt", "left": {"op": "round", "operand": {"op": "field", "path":'
                ' ["items", 0, "price"]}, "scale": 2, "mode": "half_even"}, "right":'
                ' {"op": "const", "value": {"$decimal": "9.99"}}}'
            )
        ),
        [CANONICAL],
    ),
    (
        "rule-with-id",
        "a rule with an id: the key id sorts like any other key, before name",
        '{"version": 1, "rule": {"name": "gold_customer", "id": "vip", "when":'
        ' {"op": "field", "path": ["vip"]}}}',
        None,
        [CANONICAL, RULESET_DIGESTS, "rules.md#identity"],
    ),
    (
        "rule-id-equal-to-name",
        "an explicit id equal to the name is written, so the digest differs from the"
        " same rule without an id, whose effective id is the same",
        '{"version": 1, "rule": {"name": "vip", "id": "vip", "when":'
        ' {"op": "field", "path": ["vip"]}}}',
        None,
        [DIGESTS, "rules.md#identity"],
    ),
    (
        "rule-without-id",
        "the same rule without an id: no id key, as before ids existed",
        '{"version": 1, "rule": {"name": "vip", "when": {"op": "field", "path": ["vip"]}}}',
        None,
        [DIGESTS, "rules.md#identity"],
    ),
    (
        "ruleset-ids-mixed",
        "a rule set of a rule with an id and one without: id is written only where given",
        '{"version": 1, "ruleset": {"hit_policy": "first", "logic": "boolean", "rules": ['
        '{"name": "gold_customer", "id": "vip", "when": {"op": "field", "path": ["vip"]},'
        ' "then": {"discount": {"op": "const", "value": {"$decimal": "0.15"}}}},'
        ' {"name": "default", "when": {"op": "const", "value": true},'
        ' "then": {"discount": {"op": "const", "value": {"$decimal": "0"}}}}]}}',
        None,
        [CANONICAL, RULESET_DIGESTS, "rules.md#identity"],
    ),
]


# Vectors given as JSON text, for what a parsed document cannot hold.
LEXICAL = {"layout-and-member-order"}


def canonical_vectors() -> dict[str, Any]:
    """The vectors of `format/canonical.json`."""
    table_document, compiled = table_basic()
    vectors = []
    for name, description, text, form, spec in CASES:
        given = load(text)
        expected_form = canonical_value(form if form is not None else given)
        canonical = canonical_text(expected_form)
        expect: dict[str, Any] = {"canonical": canonical, "digest": digest(canonical)}
        if "ruleset" in expected_form:
            expect["ruleset_digest"] = expect["digest"]
        elif "rule" in expected_form:
            content = default_ruleset(expected_form["rule"])
            expect["ruleset_digest"] = digest(canonical_text(ruleset_document(content)))
        elif "decision_table" in expected_form:
            assert expected_form == canonical_value(table_document)
            content = canonical_value(compiled)
            expect["ruleset_digest"] = digest(canonical_text(ruleset_document(content)))
        given_json: dict[str, Any] = {"kind": "canonical"}
        if name in LEXICAL:
            given_json["text"] = text
        else:
            given_json["document"] = given
        vectors.append(
            {
                "id": f"format/canonical/{name}",
                "spec": spec,
                "description": description,
                "given": given_json,
                "expect": expect,
            }
        )
    return {"version": 1, "area": "format", "vectors": vectors}


# --- Decisions that carry a rule set digest ---------------------------------------

DECISION_RULESET = load(
    '{"hit_policy": "first", "logic": "boolean", "rules": ['
    '{"name": "a", "when": {"op": "gt", "left": {"op": "field", "path": ["x"]},'
    ' "right": {"op": "const", "value": 1}}, "then": {"y": {"op": "const", "value": 1}}},'
    ' {"name": "b", "when": {"op": "gt", "left": {"op": "field", "path": ["x"]},'
    ' "right": {"op": "const", "value": 5}}}]}'
)
DECISION = load(
    '{"hit_policy": "first", "logic": "boolean", "results": ['
    '{"rule": "a", "fired": true, "when": {"op": "gt", "truth": true, "children": ['
    '{"op": "field", "path": ["x"], "value": 2}, {"op": "const", "value": 1}]},'
    ' "then": {"y": {"op": "const", "value": 1}}},'
    ' {"rule": "b", "fired": false, "when": {"op": "gt", "truth": false, "children": ['
    '{"op": "field", "path": ["x"], "value": 2}, {"op": "const", "value": 5}]}}],'
    ' "selected": ["a"]}'
)
READING = "json-format.md#reading-a-decision"
IN_DECISIONS = "json-format.md#in-decisions"


def with_description(ruleset: Any, description: str) -> Any:
    """A copy of a rule set whose first rule has a description."""
    copy = load(dump(ruleset))
    copy["rules"][0]["description"] = description
    return copy


def decision_vectors() -> dict[str, Any]:
    """The vectors of `format/digests.json`: decisions with a digest."""
    own = digest(canonical_text(ruleset_document(DECISION_RULESET)))
    described = with_description(DECISION_RULESET, "edited")
    other = digest(canonical_text(ruleset_document(described)))
    assert own != other

    def document(**changes: Any) -> dict[str, Any]:
        decision = dict(DECISION)
        decision.update(changes)
        return {"version": IntLit("1"), "decision": decision}

    def vector(
        name: str, description: str, given: Any, ruleset: Any, expect: Any, spec: list[str]
    ) -> dict[str, Any]:
        return {
            "id": f"format/digests/{name}",
            "spec": spec,
            "description": description,
            "given": {"kind": "read", "document": given, "ruleset": ruleset},
            "expect": expect,
        }

    def error(code: str) -> dict[str, Any]:
        return {"error": {"code": code, "pointer": "/decision/ruleset_digest"}}

    kept = document(ruleset_digest=own)
    return {
        "version": 1,
        "area": "format",
        "vectors": [
            vector(
                "kept",
                "a decision with its rule set's digest is accepted, and written back with it",
                kept,
                DECISION_RULESET,
                {"document": kept},
                [IN_DECISIONS, READING],
            ),
            vector(
                "other-ruleset",
                "the digest of another rule set is a decision_mismatch, though the rule sets"
                " differ only in a description, which no other check sees",
                kept,
                described,
                error("decision_mismatch"),
                [IN_DECISIONS],
            ),
            vector(
                "without-digest",
                "without a digest, the same decision is accepted with that rule set",
                document(),
                described,
                {"document": document()},
                [READING],
            ),
            vector(
                "first-check",
                "the digest is verified before every other check against the rule set",
                document(ruleset_digest=other, hit_policy="priority"),
                DECISION_RULESET,
                error("decision_mismatch"),
                [IN_DECISIONS, READING],
            ),
            vector(
                "uppercase",
                "a digest in uppercase hexadecimal is a format_error",
                document(ruleset_digest=own.upper().replace("SHA256:", "sha256:")),
                DECISION_RULESET,
                error("format_error"),
                [IN_DECISIONS],
            ),
            vector(
                "other-function",
                "a digest of another hash function is a format_error",
                document(ruleset_digest="sha512:" + own.removeprefix("sha256:")),
                DECISION_RULESET,
                error("format_error"),
                [IN_DECISIONS],
            ),
            vector(
                "too-short",
                "a digest with 63 hexadecimal digits is a format_error",
                document(ruleset_digest=own[:-1]),
                DECISION_RULESET,
                error("format_error"),
                [IN_DECISIONS],
            ),
            vector(
                "not-a-string",
                "a digest that is not a string is a format_error",
                document(ruleset_digest=None),
                DECISION_RULESET,
                error("format_error"),
                [IN_DECISIONS],
            ),
        ],
    }


# --- Writing and checking ---------------------------------------------------------


FILES = {
    "canonical.json": canonical_vectors,
    "digests.json": decision_vectors,
}


def check_file(path: Path) -> list[str]:
    """Recompute every canonical text and digest of a written file."""
    problems = []
    for vector in load(path.read_text(encoding="utf-8"))["vectors"]:
        expect = vector["expect"]
        if "canonical" in expect:
            canonical = expect["canonical"]
            if canonical_text(load(canonical)) != canonical:
                problems.append(f"{vector['id']}: not canonical")
            if digest(canonical) != expect["digest"]:
                problems.append(f"{vector['id']}: wrong digest")
    return problems


def main() -> int:
    """Write or check the files; return the exit code."""
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    problems: list[str] = []
    for name, build in FILES.items():
        path = VECTORS / "format" / name
        text = dump(build()) + "\n"
        if mode == "--write":
            path.write_text(text, encoding="utf-8")
        elif not path.exists() or path.read_text(encoding="utf-8") != text:
            problems.append(f"{path}: out of date; run with --write")
        problems.extend(check_file(path))
    for problem in problems:
        print(problem, file=sys.stderr)
    print(f"{len(FILES)} files, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
