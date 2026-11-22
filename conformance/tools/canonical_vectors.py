"""Generate and check the vectors of canonical bytes and digests.

The expected bytes and digests come from an oracle independent of the core: it
follows `spec/json-format.md` (*Canonical bytes*, *Digests*) with Python's own
JSON parser (keeping each number's literal), float `repr` for the shortest
digits, `decimal.Decimal` for decimal text and `hashlib.sha256`. The documents
below are written in their canonical structure (defaults written, `in` values
sorted), so the oracle only canonicalizes values and lays the bytes out; the
few vectors that check the structure give their canonical form explicitly.

    python3 conformance/tools/canonical_vectors.py --write   # canonical.json, digests.json,
                                                              # bundles.json, cases.json
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


# --- Bundles ---------------------------------------------------------------------

BUNDLES = "bundles.md"
SEMVER_OTHER = "1.0.0-conformance.1"
SEMVER_OTHER_2 = "1.0.0-conformance.2"
LIMIT_KEYS = (
    "max_size",
    "max_nesting",
    "max_depth",
    "max_path_segments",
    "max_table_size",
    "max_values",
)
DEFAULT_LIMITS = dict(zip(LIMIT_KEYS, (16777216, 1024, 256, 256, 2097152, 1000000), strict=True))
NUMBER_OR_NULL = '{"anyOf": [{"type": "number"}, {"type": "null"}]}'
# The input schema of EXAMPLE's rules, derived by hand from `agents.md`
# (*Input schemas*): `years` is compared with an int, so it is a number.
EXAMPLE_INPUT = (
    '{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object",'
    ' "properties": {"years": {"description": "conditions of vip",'
    ' "anyOf": [{"type": "number"}, {"type": "null"}]}}}'
)
# That of the rules `format/tables/basic` compiles to.
TABLE_INPUT = (
    '{"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object",'
    ' "properties": {'
    '"customer": {"anyOf": [{"type": "object", "properties": {"years": {"description":'
    ' "conditions of vip, loyal", "anyOf": [{"type": "number"}, {"type": "null"}]}}},'
    ' {"type": "null"}]},'
    ' "order": {"anyOf": [{"type": "object", "properties": {"total": {"description":'
    ' "conditions of vip", "anyOf": [{"type": "number"}, {"type": "null"}]}}},'
    ' {"type": "null"}]}}}'
)


def raw_canonical(value: Any) -> Any:
    """The canonical form of JSON that is not a value (a contract): numbers only."""
    if isinstance(value, IntLit):
        return IntLit(str(int(value)))
    assert not isinstance(value, FloatLit), value
    if isinstance(value, list):
        return [raw_canonical(v) for v in value]
    if isinstance(value, dict):
        return {k: raw_canonical(v) for k, v in value.items()}
    return value


def ruleset_digest_of(content: Any) -> str:
    """The rule set digest of a rule set's content in canonical structure."""
    return digest(canonical_text(canonical_value(ruleset_document(content))))


def limits_json(changes: dict[str, int] | None = None) -> dict[str, Any]:
    """The six recorded limits, the defaults unless changed."""
    values = {**DEFAULT_LIMITS, **(changes or {})}
    return {key: IntLit(str(values[key])) for key in LIMIT_KEYS}


def engine(version: str = SEMVER_OTHER, **limits: int) -> dict[str, Any]:
    """An engine record (`bundles.md`, *Engine*)."""
    return {"name": "cleanrules", "version": version, "limits": limits_json(limits)}


def contract(input_text: str = '{"type": "object"}', keys: list[str] | None = None) -> Any:
    """A contract record (`bundles.md`, *Contract*)."""
    return {"input": load(input_text), "output": {"keys": keys or ["discount"]}}


def case(path: str, text: str) -> dict[str, Any]:
    """A case file reference to a file holding `text`."""
    return {"path": path, "digest": digest(text)}


def example_rules(threshold: int = 5) -> Any:
    """EXAMPLE's rule set content, with another threshold."""
    content = load(EXAMPLE)["ruleset"]
    content["rules"][0]["when"]["right"]["value"] = IntLit(str(threshold))
    return content


def bundle(
    *,
    id: str = "acme/discounts",
    revision: str = "1",
    ruleset: Any = None,
    table: Any = None,
    stated: str | None = None,
    engine_record: Any = None,
    contract_record: Any = None,
    cases: list[Any] | None = None,
) -> dict[str, Any]:
    """A bundle document in canonical structure, its rule set digest computed."""
    content: dict[str, Any] = {"id": id, "revision": revision}
    if table is not None:
        content["decision_table"] = table
        _, compiled = table_basic()
        content["ruleset_digest"] = stated or ruleset_digest_of(compiled)
    else:
        rules = example_rules() if ruleset is None else ruleset
        content["ruleset"] = rules
        content["ruleset_digest"] = stated or ruleset_digest_of(rules)
    content["engine"] = engine() if engine_record is None else engine_record
    content["contract"] = contract() if contract_record is None else contract_record
    if cases:
        content["cases"] = cases
    return {"version": IntLit("1"), "bundle": content}


def canonical_bundle(document: Any) -> Any:
    """A bundle document's canonical form.

    Values in the rules, numbers in the contract, sorted output keys, and no
    empty `cases`.
    """
    content = dict(document["bundle"])
    for key in ("ruleset", "decision_table"):
        if key in content:
            content[key] = canonical_value(content[key])
    content["contract"] = raw_canonical(content["contract"])
    content["contract"]["output"]["keys"] = sorted(content["contract"]["output"]["keys"])
    if not content.get("cases", True):
        del content["cases"]
    return {"version": IntLit("1"), "bundle": content}


def with_changes(document: Any, pointer: str, value: Any) -> Any:
    """A copy of a document with the value at a pointer replaced (removed for `REMOVE`)."""
    copy = load(dump(document))
    *parents, last = pointer.lstrip("/").split("/")
    node = copy
    for part in parents:
        node = node[int(part)] if isinstance(node, list) else node[part]
    if value is REMOVE:
        del node[last]
    elif isinstance(node, list):
        node[int(last)] = value
    else:
        node[last] = value
    return copy


REMOVE = object()


def bundle_vectors() -> dict[str, Any]:
    """The vectors of `format/bundles.json`."""
    vectors: list[dict[str, Any]] = []
    table_document, compiled = table_basic()
    table = table_document["decision_table"]

    def add(name: str, description: str, spec: list[str], given: Any, expect: Any) -> None:
        vectors.append(
            {
                "id": f"format/bundles/{name}",
                "spec": [s if "#" in s else f"{BUNDLES}#{s}" for s in spec],
                "description": description,
                "given": given,
                "expect": expect,
            }
        )

    def read(
        name: str, description: str, spec: list[str], document: Any, *, current: bool = False
    ) -> None:
        given: dict[str, Any] = {"kind": "read", "document": document}
        if current:
            given["current_engine"] = True
        add(name, description, spec, given, {"document": canonical_bundle(document)})

    def refused(
        name: str,
        description: str,
        spec: list[str],
        document: Any,
        code: str,
        pointer: str,
        *,
        current: bool = False,
    ) -> None:
        given: dict[str, Any] = {"kind": "read", "document": document}
        if current:
            given["current_engine"] = True
        add(name, description, spec, given, {"error": {"code": code, "pointer": pointer}})

    base = bundle(cases=[case("cases/discounts.json", "[]\n")])

    # Reading and writing.
    read(
        "read-ruleset",
        "a bundle of a rule set reads, and is written back as it was",
        ["keys", "rules-and-their-digest", "reading", "json-format.md#bundles"],
        base,
    )
    read(
        "read-table",
        "a bundle of a decision table states the digest of the rule set the table compiles to",
        ["rules-and-their-digest"],
        bundle(table=table),
    )
    read(
        "read-without-cases",
        "without cases, a bundle is written without the key",
        ["keys", "cases"],
        bundle(),
    )
    empty = bundle()
    empty["bundle"]["cases"] = []
    read(
        "read-empty-cases",
        "an empty list of cases is written without the key",
        ["keys", "cases"],
        empty,
    )
    read(
        "read-output-keys-sorted",
        "output keys are written sorted by code point",
        ["contract"],
        bundle(contract_record=contract(keys=["\uff5a", "\U0001f600", "a"])),
    )
    read(
        "read-other-engine-contract-kept",
        "a bundle of another engine keeps its contract as recorded, though this engine"
        " would derive another one",
        ["contract", "engine"],
        bundle(contract_record=contract('{"anything": [1, {"x": true}]}', ["other"])),
    )
    read(
        "read-other-engine-name",
        "a bundle made by an engine of another name is read",
        ["engine"],
        bundle(engine_record={**engine(), "name": "other-engine"}),
    )
    read(
        "read-cases-in-order",
        "case files keep their order",
        ["cases"],
        bundle(cases=[case("b.json", "b"), case("a/x.json", "a")]),
    )
    read(
        "read-contract-integers",
        "integers in a contract are kept, up to 2^53 - 1",
        ["contract"],
        bundle(contract_record=contract('{"minItems": 9007199254740991, "n": -9007199254740991}')),
    )
    yaml_text = (
        "version: 1\nbundle:\n  id: acme/discounts\n  revision: '1'\n  ruleset:\n"
        "    rules:\n    - name: vip\n      when: {op: gt, left: {op: field, path: [years]},"
        " right: {op: const, value: 5}}\n      then:\n        discount: {op: const, value:"
        " {$decimal: '0.15'}}\n"
        f"  ruleset_digest: {ruleset_digest_of(example_rules())}\n"
        f"  engine:\n    name: cleanrules\n    version: {SEMVER_OTHER}\n    limits:\n"
        + "".join(f"      {k}: {DEFAULT_LIMITS[k]}\n" for k in LIMIT_KEYS)
        + "  contract:\n    input: {type: object}\n    output: {keys: [discount]}\n"
    )
    add(
        "read-yaml",
        "a bundle reads from YAML, settings of its rule set at their defaults",
        ["keys"],
        {"kind": "read", "yaml": yaml_text},
        {"document": canonical_bundle(bundle())},
    )

    # The rules and their digest.
    other = ruleset_digest_of(example_rules(6))
    refused(
        "digest-mismatch",
        "a rule set digest that is not that of the rules is a decision_mismatch",
        ["rules-and-their-digest", "reading"],
        bundle(stated=other),
        "decision_mismatch",
        "/bundle/ruleset_digest",
    )
    table_own = digest(canonical_text(canonical_value(table_document)))
    refused(
        "digest-of-the-table-document",
        "the digest of a table's own document is not its rule set digest",
        ["rules-and-their-digest"],
        bundle(table=table, stated=table_own),
        "decision_mismatch",
        "/bundle/ruleset_digest",
    )
    refused(
        "digest-uppercase",
        "a malformed rule set digest is a format_error",
        ["rules-and-their-digest"],
        bundle(stated=ruleset_digest_of(example_rules()).upper().replace("SHA256", "sha256")),
        "format_error",
        "/bundle/ruleset_digest",
    )
    refused(
        "digest-missing",
        "the rule set digest is required",
        ["keys"],
        with_changes(bundle(), "/bundle/ruleset_digest", REMOVE),
        "format_error",
        "/bundle/ruleset_digest",
    )
    refused(
        "no-rules",
        "a bundle without rules is a format_error at the bundle",
        ["keys"],
        with_changes(bundle(), "/bundle/ruleset", REMOVE),
        "format_error",
        "/bundle",
    )
    both = bundle()
    both["bundle"]["decision_table"] = table
    refused(
        "rules-and-table",
        "a bundle with a rule set and a table is a format_error",
        ["keys"],
        both,
        "format_error",
        "/bundle",
    )
    repeated = example_rules()
    repeated["rules"] = [repeated["rules"][0], repeated["rules"][0]]
    refused(
        "rules-construction-before-digest",
        "the rules' construction rules come before the digest",
        ["reading"],
        bundle(ruleset=repeated, stated=other),
        "rule_definition_error",
        "/bundle/ruleset/rules/1/name",
    )
    bad_shape = with_changes(bundle(), "/bundle/ruleset/hit_policy", "best")
    refused(
        "rules-shape",
        "the rules are read with the errors of their own documents",
        ["rules-and-their-digest"],
        bad_shape,
        "format_error",
        "/bundle/ruleset/hit_policy",
    )

    # Identifier and revision.
    for name, value in (
        ("id-uppercase", "Acme"),
        ("id-empty-segment", "acme//x"),
        ("id-leading-slash", "/acme"),
        ("id-too-long", "a" * 129),
    ):
        refused(
            name,
            f"the identifier {value[:20]!r} is a format_error",
            ["identifier-and-revision"],
            bundle(id=value),
            "format_error",
            "/bundle/id",
        )
    read(
        "id-segments",
        "an identifier of several segments, with '.', '_' and '-'",
        ["identifier-and-revision"],
        bundle(id="acme/pricing.v2/loan_approval-eu"),
    )
    read(
        "revision-forms",
        "a revision may hold letters of either case, digits, '.', '_', '+' and '-'",
        ["identifier-and-revision"],
        bundle(revision="V1.2.0+build_5-rc"),
    )
    for name, value in (
        ("revision-space", "1 2"),
        ("revision-leading-dot", ".1"),
        ("revision-empty", ""),
    ):
        refused(
            name,
            f"the revision {value!r} is a format_error",
            ["identifier-and-revision"],
            bundle(revision=value),
            "format_error",
            "/bundle/revision",
        )
    refused(
        "id-not-a-string",
        "an identifier that is not a string is a format_error",
        ["identifier-and-revision"],
        with_changes(bundle(), "/bundle/id", IntLit("1")),
        "format_error",
        "/bundle/id",
    )

    # Engine.
    for name, value in (
        ("version-two-parts", "1.0"),
        ("version-leading-zero", "01.0.0"),
        ("version-v", "v1.0.0"),
    ):
        refused(
            name,
            f"the engine version {value!r} is a format_error",
            ["engine"],
            with_changes(bundle(), "/bundle/engine/version", value),
            "format_error",
            "/bundle/engine/version",
        )
    read(
        "version-prerelease-build",
        "a version with pre-release and build metadata",
        ["engine"],
        bundle(engine_record=engine("2.0.0-rc.1+sha.5114f85")),
    )
    refused(
        "engine-name-uppercase",
        "an engine name with uppercase letters is a format_error",
        ["engine"],
        with_changes(bundle(), "/bundle/engine/name", "CleanRules"),
        "format_error",
        "/bundle/engine/name",
    )
    refused(
        "limit-zero",
        "a limit of 0 is a format_error",
        ["engine"],
        bundle(engine_record=engine(max_depth=0)),
        "format_error",
        "/bundle/engine/limits/max_depth",
    )
    refused(
        "limit-too-large",
        "a limit above 2^53 - 1 is a format_error",
        ["engine"],
        bundle(engine_record=engine(max_values=2**53)),
        "format_error",
        "/bundle/engine/limits/max_values",
    )
    refused(
        "limit-float",
        "a limit written as a float is a format_error",
        ["engine"],
        with_changes(bundle(), "/bundle/engine/limits/max_size", FloatLit("1.0")),
        "format_error",
        "/bundle/engine/limits/max_size",
    )
    refused(
        "limit-missing",
        "every limit is required",
        ["engine"],
        with_changes(bundle(), "/bundle/engine/limits/max_table_size", REMOVE),
        "format_error",
        "/bundle/engine/limits/max_table_size",
    )
    refused(
        "limit-unknown",
        "a limit that is not recorded is a format_error",
        ["engine"],
        with_changes(bundle(), "/bundle/engine/limits/max_yaml_size", IntLit("1")),
        "format_error",
        "/bundle/engine/limits/max_yaml_size",
    )
    read(
        "limits-recorded",
        "the limits are kept as recorded",
        ["engine"],
        bundle(engine_record=engine(max_size=1, max_values=9007199254740991)),
    )

    # Contract.
    refused(
        "contract-float",
        "a float in a contract is a format_error, at the number",
        ["contract"],
        bundle(contract_record=contract('{"properties": {"x": {"minimum": 0.5}}}')),
        "format_error",
        "/bundle/contract/input/properties/x/minimum",
    )
    refused(
        "contract-large-integer",
        "an integer beyond 2^53 - 1 in a contract is a format_error",
        ["contract"],
        bundle(contract_record=contract('{"minItems": 9007199254740992}')),
        "format_error",
        "/bundle/contract/input/minItems",
    )
    refused(
        "contract-input-not-object",
        "an input schema that is not an object is a format_error",
        ["contract"],
        with_changes(bundle(), "/bundle/contract/input", True),
        "format_error",
        "/bundle/contract/input",
    )
    refused(
        "output-key-repeated",
        "a repeated output key is a format_error",
        ["contract"],
        bundle(contract_record=contract(keys=["a", "b", "a"])),
        "format_error",
        "/bundle/contract/output/keys/2",
    )

    # Cases.
    for name, path in (
        ("case-parent", "../cases.json"),
        ("case-absolute", "/cases.json"),
        ("case-dot", "cases/./a.json"),
        ("case-backslash", "cases\\a.json"),
        ("case-uppercase", "cases/A.json"),
        ("case-trailing-dot", "cases/a."),
    ):
        refused(
            name,
            f"the case path {path!r} is a format_error",
            ["cases"],
            bundle(cases=[case(path, "x")]),
            "format_error",
            "/bundle/cases/0/path",
        )
    refused(
        "case-repeated",
        "the same case path twice is a format_error at the second",
        ["cases"],
        bundle(cases=[case("a.json", "1"), case("a.json", "2")]),
        "format_error",
        "/bundle/cases/1/path",
    )
    refused(
        "case-digest-malformed",
        "a malformed case digest is a format_error",
        ["cases"],
        bundle(cases=[{"path": "a.json", "digest": "sha1:00"}]),
        "format_error",
        "/bundle/cases/0/digest",
    )
    refused(
        "unknown-key",
        "an unknown key in a bundle is a format_error",
        ["keys"],
        with_changes(bundle(), "/bundle/digest", ruleset_digest_of(example_rules())),
        "format_error",
        "/bundle/digest",
    )
    two = bundle()
    two["version"] = IntLit("2")
    refused(
        "format-version-unknown",
        "only the format version refuses a bundle",
        ["engine"],
        two,
        "unsupported_version",
        "/version",
    )

    # This engine: the contract is verified.
    strict_vector = next(
        v
        for v in load((VECTORS / "agents" / "input-schemas.json").read_text(encoding="utf-8"))[
            "vectors"
        ]
        if v["id"] == "agents/input-schemas/strict"
    )
    strict_rules = strict_vector["given"]["rulesets"]["orders"]
    strict_input = strict_vector["expect"]["output"]["input_schema"]
    strict_keys = ["late", "note", "tier"]

    def current(**changes: Any) -> Any:
        record = {"input": strict_input, "output": {"keys": strict_keys}}
        record.update(changes)
        return bundle(
            id="acme/orders",
            ruleset=strict_rules,
            engine_record=engine("0.0.0"),
            contract_record=load(dump(record)),
        )

    read(
        "current-engine-verified",
        "a bundle of this engine whose contract is the one this engine derives reads",
        ["contract", "reading"],
        current(),
        current=True,
    )
    loose = with_changes(current(), "/bundle/contract/input/required", REMOVE)
    refused(
        "current-engine-input-mismatch",
        "a bundle of this engine whose input schema is not the derived one is a decision_mismatch",
        ["contract", "reading"],
        loose,
        "decision_mismatch",
        "/bundle/contract/input",
        current=True,
    )
    fewer = with_changes(current(), "/bundle/contract/output/keys", ["late", "note"])
    refused(
        "current-engine-output-mismatch",
        "a bundle of this engine whose output keys are"
        " not those of the rules is a decision_mismatch",
        ["contract"],
        fewer,
        "decision_mismatch",
        "/bundle/contract/output",
        current=True,
    )
    both_wrong = with_changes(fewer, "/bundle/ruleset_digest", other)
    refused(
        "current-engine-digest-first",
        "the rule set digest is verified before the contract",
        ["reading"],
        both_wrong,
        "decision_mismatch",
        "/bundle/ruleset_digest",
        current=True,
    )
    read(
        "other-engine-not-verified",
        "the same bundle made by another version of this engine is read, its contract as recorded",
        ["contract", "engine"],
        with_changes(loose, "/bundle/engine/version", SEMVER_OTHER),
    )

    # Canonical bytes and digests.
    def canonical_vector(name: str, description: str, document: Any, ruleset: str) -> None:
        form = canonical_bundle(document)
        text = canonical_text(form)
        add(
            name,
            description,
            ["digest-of-a-bundle"],
            {"kind": "canonical", "document": document},
            {"canonical": text, "digest": digest(text), "ruleset_digest": ruleset},
        )

    canonical_vector(
        "canonical-ruleset",
        "a bundle's canonical bytes sort every member,"
        " the contract's too, and its rule set digest is its rules'",
        bundle(
            contract_record=contract('{"z": -0, "a": {"$schema": "x"}}', ["b", "a"]),
            cases=[case("cases/a.json", "[]")],
        ),
        ruleset_digest_of(example_rules()),
    )
    canonical_vector(
        "canonical-table",
        "a bundle of a table has the rule set digest of the rule set the table compiles to",
        bundle(table=table),
        ruleset_digest_of(compiled),
    )

    # Making a bundle.
    def make(name: str, description: str, given: Any, expect: Any) -> None:
        add(name, description, ["making-a-bundle"], {"kind": "bundle", **given}, expect)

    made = bundle(
        engine_record=engine("0.0.0"),
        contract_record=contract(EXAMPLE_INPUT),
        cases=[case("cases/vip.json", '{"cases": []}\n'), case("cases/more.json", "")],
    )
    make(
        "make-ruleset",
        "a bundle made from a rule set records this engine, the default"
        " limits, the derived contract and each case file's digest",
        {
            "document": load(EXAMPLE),
            "id": "acme/discounts",
            "revision": "1",
            "cases": [
                {"path": "cases/vip.json", "text": '{"cases": []}\n'},
                {"path": "cases/more.json", "text": ""},
            ],
        },
        {"document": canonical_bundle(made)},
    )
    table_made = bundle(
        table=table,
        id="acme/table",
        revision="2026-11-16.1",
        engine_record=engine("0.0.0", max_depth=64, max_values=10),
        contract_record=contract(TABLE_INPUT, ["discount", "label"]),
    )
    make(
        "make-table",
        "a bundle made from a decision table, with limits",
        {
            "document": table_document,
            "id": "acme/table",
            "revision": "2026-11-16.1",
            "limits": {"max_depth": IntLit("64"), "max_values": IntLit("10")},
        },
        {"document": canonical_bundle(table_made)},
    )
    make(
        "make-bad-id",
        "making a bundle checks the identifier",
        {"document": load(EXAMPLE), "id": "Acme", "revision": "1"},
        {"error": {"code": "format_error"}},
    )
    make(
        "make-bad-revision",
        "making a bundle checks the revision",
        {"document": load(EXAMPLE), "id": "acme", "revision": "a b"},
        {"error": {"code": "format_error"}},
    )
    make(
        "make-bad-path",
        "making a bundle checks the case paths",
        {
            "document": load(EXAMPLE),
            "id": "acme",
            "revision": "1",
            "cases": [{"path": "../x.json", "text": ""}],
        },
        {"error": {"code": "format_error"}},
    )
    make(
        "make-repeated-path",
        "making a bundle refuses a case path given twice",
        {
            "document": load(EXAMPLE),
            "id": "acme",
            "revision": "1",
            "cases": [{"path": "x.json", "text": "1"}, {"path": "x.json", "text": "2"}],
        },
        {"error": {"code": "format_error"}},
    )
    make(
        "make-limit-zero",
        "a limit of 0, which a reader would refuse, is not recorded",
        {
            "document": load(EXAMPLE),
            "id": "acme",
            "revision": "1",
            "limits": {"max_depth": IntLit("0")},
        },
        {"error": {"code": "unsupported"}},
    )
    make(
        "make-limit-above-max",
        "a limit of 2^53, above the largest integer every language reads exactly, is not recorded",
        {
            "document": load(EXAMPLE),
            "id": "acme",
            "revision": "1",
            "limits": {"max_values": IntLit("9007199254740992")},
        },
        {"error": {"code": "unsupported"}},
    )

    # Comparing bundles.
    def compare(
        name: str, description: str, a: Any, b: Any, relation: str, differences: list[str]
    ) -> None:
        add(
            name,
            description,
            ["comparing-bundles"],
            {"kind": "bundle_compare", "a": a, "b": b},
            {
                "comparison": {
                    "relation": relation,
                    "identical": not differences,
                    "differences": differences,
                }
            },
        )

    a = bundle(cases=[case("cases/a.json", "[]")])
    compare(
        "compare-identical", "a bundle and itself are identical", a, a, "same_rules_same_engine", []
    )
    relaid = load(dump(a))
    relaid["bundle"]["ruleset"]["rules"][0]["then"]["discount"]["value"] = {"$decimal": "1.5e-1"}
    compare(
        "compare-identical-other-layout",
        "the same bundle written another way is identical",
        a,
        relaid,
        "same_rules_same_engine",
        [],
    )
    compare(
        "compare-other-engine-version",
        "the same rules under another engine version",
        a,
        with_changes(a, "/bundle/engine/version", SEMVER_OTHER_2),
        "same_rules_other_engine",
        ["engine.version"],
    )
    compare(
        "compare-other-limits",
        "the same rules under another configuration",
        a,
        bundle(cases=[case("cases/a.json", "[]")], engine_record=engine(max_values=10)),
        "same_rules_other_engine",
        ["engine.limits.max_values"],
    )
    compare(
        "compare-other-engine-name",
        "the same rules under an engine of another name",
        a,
        with_changes(a, "/bundle/engine/name", "other-engine"),
        "same_rules_other_engine",
        ["engine.name"],
    )
    b = bundle(revision="2", ruleset=example_rules(6), cases=[case("cases/a.json", "[]")])
    compare(
        "compare-other-rules",
        "other rules under the same engine",
        a,
        b,
        "other_rules_same_engine",
        ["revision", "rules"],
    )
    compare(
        "compare-other-rules-other-engine",
        "other rules under another engine, with another contract",
        a,
        with_changes(
            with_changes(b, "/bundle/engine/version", SEMVER_OTHER_2),
            "/bundle/contract/input",
            load('{"type": "object", "x": 1}'),
        ),
        "other_rules_other_engine",
        ["revision", "rules", "engine.version", "contract.input"],
    )
    compare(
        "compare-conflict",
        "the same identifier and revision with other rules is a conflict",
        a,
        with_changes(b, "/bundle/revision", "1"),
        "conflict",
        ["rules"],
    )
    compare(
        "compare-conflict-other-engine",
        "a conflict takes precedence over the engine",
        a,
        with_changes(
            with_changes(b, "/bundle/revision", "1"), "/bundle/engine/version", SEMVER_OTHER_2
        ),
        "conflict",
        ["rules", "engine.version"],
    )
    compare(
        "compare-table-and-its-ruleset",
        "a table and the rule set it compiles to are the same rules from another source",
        bundle(table=table, contract_record=contract(keys=["discount", "label"])),
        bundle(ruleset=compiled, contract_record=contract(keys=["discount", "label"])),
        "same_rules_same_engine",
        ["source"],
    )
    compare(
        "compare-cases-only",
        "the same rules on the same engine with other cases",
        a,
        bundle(cases=[case("cases/a.json", "[1]")]),
        "same_rules_same_engine",
        ["cases"],
    )
    compare(
        "compare-cases-order",
        "case files in another order differ",
        bundle(cases=[case("a.json", "1"), case("b.json", "2")]),
        bundle(cases=[case("b.json", "2"), case("a.json", "1")]),
        "same_rules_same_engine",
        ["cases"],
    )
    compare(
        "compare-other-id",
        "another identifier with the same rules",
        a,
        with_changes(a, "/bundle/id", "acme/other"),
        "same_rules_same_engine",
        ["id"],
    )
    compare(
        "compare-output-contract",
        "another output contract is a difference",
        a,
        bundle(cases=[case("cases/a.json", "[]")], contract_record=contract(keys=["x"])),
        "same_rules_same_engine",
        ["contract.output"],
    )
    return {"version": 1, "area": "format", "vectors": vectors}


TESTS = "tests.md"
I64 = 2**63


def data_value(value: Any) -> Any:
    """The canonical form of a data value (*Values*).

    An int outside the signed 64-bit range, or a decimal that does not fit,
    reads as null, as in any input.
    """
    if isinstance(value, IntLit) and not -I64 <= int(value) < I64:
        return None
    if isinstance(value, dict) and set(value) == {"$int"} and not -I64 <= int(value["$int"]) < I64:
        return None
    if (
        isinstance(value, dict)
        and set(value) == {"$decimal"}
        and not decimal_fits(value["$decimal"])
    ):
        return None
    if isinstance(value, list):
        return [data_value(v) for v in value]
    if isinstance(value, dict):
        typed = [k for k in value if k.startswith("$")]
        if typed:
            if typed == ["$map"]:
                inner = {k: data_value(v) for k, v in value["$map"].items()}
                return canonical_value({"$map": inner})
            return canonical_value(value)
        return {k: data_value(v) for k, v in value.items()}
    return canonical_value(value)


def decimal_fits(text: str) -> bool:
    """Whether a decimal's text fits the data model (*Decimals*): coefficient below 10^38."""
    if text in ("inf", "-inf"):
        return True
    _, digits, exponent = decimal.Decimal(text).as_tuple()
    assert isinstance(exponent, int)
    coefficient = int("".join(map(str, digits)) or "0")
    if exponent > 0:
        coefficient *= 10**exponent
    return coefficient < 10**38 and -exponent <= 38


def expected_output(output: Any) -> Any:
    """An expected output: payload keys taken literally, data values (never out of range)."""
    return {k: data_value(v) for k, v in output.items()}


def canonical_cases(document: Any) -> Any:
    """A case file's canonical form (`tests.md`, *Reading and writing*)."""
    content = document["cases"]
    cases = []
    for case in content["items"]:
        out: dict[str, Any] = {"id": case["id"]}
        if "description" in case:
            out["description"] = case["description"]
        out["input"] = data_value(case["input"])
        expect = dict(case.get("expect", {}))
        written: dict[str, Any] = {}
        if "selected" in expect:
            written["selected"] = list(expect["selected"])
        if "output" in expect:
            output = expect["output"]
            written["output"] = None if output is None else expected_output(output)
        if "outputs" in expect:
            written["outputs"] = [expected_output(o) for o in expect["outputs"]]
        if "unknown" in expect:
            # Python compares strings by code point.
            written["unknown"] = sorted(expect["unknown"])
        if "error" in expect:
            written["error"] = {"code": expect["error"]["code"]}
        if written:
            out["expect"] = written
        cases.append(out)
    file: dict[str, Any] = {}
    if "description" in content:
        file["description"] = content["description"]
    if content.get("metadata"):
        file["metadata"] = {k: data_value(v) for k, v in content["metadata"].items()}
    file["items"] = cases
    return {"version": IntLit("1"), "cases": file}


CASE_EXAMPLE = """{
  "version": 1,
  "cases": {
    "description": "the cases of the discount rules",
    "items": [
      {
        "id": "vip-gets-15",
        "description": "a customer of more than five years gets 15%",
        "input": { "years": 7 },
        "expect": {
          "selected": ["vip"],
          "output": { "discount": { "$decimal": "0.15" } }
        }
      },
      {
        "id": "years-unknown",
        "input": {},
        "expect": { "selected": [], "output": null, "unknown": ["vip"] }
      }
    ]
  }
}"""


def case_file(*cases: str) -> Any:
    """A case file of cases given as JSON texts."""
    return load('{"version": 1, "cases": {"items": [' + ", ".join(cases) + "]}}")


def case_file_vectors() -> dict[str, Any]:
    """The vectors of `format/cases.json` (`tests.md`)."""
    vectors: list[dict[str, Any]] = []

    def add(name: str, description: str, spec: list[str], given: Any, expect: Any) -> None:
        vectors.append(
            {
                "id": f"format/cases/{name}",
                "spec": [s if "#" in s else f"{TESTS}#{s}" for s in spec],
                "description": description,
                "given": given,
                "expect": expect,
            }
        )

    def read(name: str, description: str, spec: list[str], document: Any) -> None:
        add(
            name,
            description,
            spec,
            {"kind": "read", "document": document},
            {"document": canonical_cases(document)},
        )

    def refused(name: str, description: str, spec: list[str], document: Any, pointer: str) -> None:
        add(
            name,
            description,
            spec,
            {"kind": "read", "document": document},
            {"error": {"code": "format_error", "pointer": pointer}},
        )

    # Reading and writing.
    example = load(CASE_EXAMPLE)
    read(
        "read-example",
        "a case file reads, and is written back as it was",
        ["case-files", "cases", "expectations", "json-format.md#test-cases"],
        example,
    )
    read("read-empty", "a case file may hold no cases", ["case-files"], case_file())
    read(
        "read-description-and-metadata",
        "a case file has a description and metadata, whose keys are literal and whose values"
        " are data, an int out of range reading as null as in rule metadata",
        ["case-files"],
        load(
            '{"version": 1, "cases": {"items": [], "metadata": {"$owner": "risk",'
            ' "reviewed": {"$date": "2026-11-22"}, "n": 99999999999999999999,'
            ' "m": {"$map": {"$k": 1}}}, "description": "d"}}'
        ),
    )
    read(
        "read-empty-metadata",
        "empty metadata is written without the key",
        ["case-files"],
        load('{"version": 1, "cases": {"metadata": {}, "items": []}}'),
    )
    read(
        "read-no-expectation",
        "a case without an expectation, or with an empty one, is written without it",
        ["cases", "expectations"],
        case_file('{"id": "a", "input": {"x": 1}}', '{"id": "b", "input": {"x": 2}, "expect": {}}'),
    )
    read(
        "read-member-order",
        "the members of a case and of its expectation are read in any order",
        ["reading-and-writing"],
        case_file(
            '{"expect": {"unknown": [], "output": {"b": 2, "a": 1}, "selected": ["r"]},'
            ' "input": {"y": 1, "x": 2}, "description": "d", "id": "a"}'
        ),
    )
    read(
        "read-unknown-sorted",
        "unknown is a set, written sorted by code point",
        ["expectations", "rules-in-expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"unknown": ["b", "\\u00e9", "a", "Z"]}}'),
    )
    read(
        "read-selected-order-kept",
        "selected keeps its order, the selection order",
        ["expectations", "rules-in-expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"selected": ["b", "a", "PRICING-12"]}}'),
    )
    read(
        "read-output-null",
        "an output of null expects that no rule is selected",
        ["expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"selected": [], "output": null}}'),
    )
    read(
        "read-outputs",
        "outputs hold one object per selected rule, whose values may be lists and maps",
        ["expectations"],
        case_file(
            '{"id": "a", "input": {"order": {"items": [1, 2], "total": 1200}},'
            ' "expect": {"outputs": [{"shipping": 0},'
            ' {"tags": ["a", "b"], "limits": {"max": 3}}, {}]}}'
        ),
    )
    read(
        "read-output-literal-keys",
        "the keys of an expected output are payload keys, taken literally, even with a $",
        ["expectations"],
        case_file(
            '{"id": "a", "input": {"$map": {"$ref": 1}},'
            ' "expect": {"output": {"$decimal": "1", "$ref": {"$map": {"$x": 2}}}}}'
        ),
    )
    read(
        "read-values-canonical",
        "inputs and expected values are written in their canonical form, decimals keeping"
        " their scale",
        ["inputs", "expectations", "json-format.md#values"],
        case_file(
            '{"id": "a", "input": {"i": {"$int": "007"}, "f": 1e2, "d": {"$decimal": "1.5e-3"},'
            ' "s": {"$decimal": "1.00"}, "t": {"$datetime": "2026-10-31T09:30:00.500+01:00"}},'
            ' "expect": {"output": {"total": {"$float": 1}, "price": {"$decimal": "19.990"},'
            ' "big": {"$int": "9007199254740993"}, "day": {"$date": "2026-10-31"}}}}'
        ),
    )
    read(
        "read-int-overflow",
        "an int outside the signed 64-bit range reads as null in an input, as in any input",
        ["inputs"],
        case_file(
            '{"id": "a", "input": {"n": 99999999999999999999,'
            ' "m": {"$int": "-99999999999999999999"},'
            f' "d": {{"$decimal": "1{"0" * 38}"}}}}}}'
        ),
    )
    read(
        "read-errors",
        "an expected error is an object with its code, one of the three evaluation can raise",
        ["errors"],
        case_file(
            '{"id": "a", "input": {}, "expect": {"error": {"code": "missing_field"}}}',
            '{"id": "b", "input": {}, "expect": {"error": {"code": "type_mismatch"}}}',
            '{"id": "c", "input": {}, "expect": {"error": {"code": "hit_policy_error"}}}',
        ),
    )
    read(
        "read-ids-literal",
        "ids are any non-empty strings, taken literally: ids that differ only in case differ",
        ["identity"],
        case_file(
            '{"id": "Vip \\u2713", "input": {}}',
            '{"id": "vip \\u2713", "input": {}}',
            '{"id": "a/b", "input": {}}',
            '{"id": " ", "input": {}}',
        ),
    )
    read(
        "read-scalar-input",
        "an input may be any data value, not only a map",
        ["inputs"],
        case_file('{"id": "a", "input": [1, "x", null]}', '{"id": "b", "input": 5}'),
    )
    add(
        "read-yaml",
        "a case file reads from YAML",
        ["case-files"],
        {
            "kind": "read",
            "yaml": (
                "version: 1\n"
                "cases:\n"
                "  items:\n"
                "  - id: vip-gets-15\n"
                "    input: {years: 7}\n"
                "    expect:\n"
                "      selected: [vip]\n"
                "      output:\n"
                "        discount: {$decimal: '0.15'}\n"
                "  - id: years-unknown\n"
                "    input: {}\n"
                "    expect: {selected: [], output: null, unknown: [vip]}\n"
            ),
        },
        {
            "document": canonical_cases(
                case_file(
                    '{"id": "vip-gets-15", "input": {"years": 7}, "expect": {"selected": ["vip"],'
                    ' "output": {"discount": {"$decimal": "0.15"}}}}',
                    '{"id": "years-unknown", "input": {}, "expect": {"selected": [],'
                    ' "output": null, "unknown": ["vip"]}}',
                )
            )
        },
    )
    for name, write in (("write-text", "text"), ("write-csv", "csv")):
        add(
            name,
            f"a case file has no {write} form",
            ["case-files"],
            {"kind": "read", "document": example, "write": write},
            {"error": {"code": "unsupported"}},
        )

    # Errors.
    refused(
        "content-not-object",
        "the content of a case file is an object, not the array of its cases",
        ["case-files"],
        load('{"version": 1, "cases": [{"id": "a", "input": {}}]}'),
        "/cases",
    )
    refused(
        "items-missing",
        "a case file holds its cases under items",
        ["case-files"],
        load('{"version": 1, "cases": {"description": "none"}}'),
        "/cases/items",
    )
    refused(
        "items-not-array",
        "items is an array of cases",
        ["case-files"],
        load('{"version": 1, "cases": {"items": {}}}'),
        "/cases/items",
    )
    refused(
        "file-description-not-string",
        "a case file's description is a string",
        ["case-files"],
        load('{"version": 1, "cases": {"description": 1, "items": []}}'),
        "/cases/description",
    )
    refused(
        "metadata-before-items",
        "a case file's metadata is read before its items, and is an object",
        ["case-files", "reading-and-writing"],
        load('{"version": 1, "cases": {"items": [{"id": ""}], "metadata": []}}'),
        "/cases/metadata",
    )
    refused(
        "file-unknown-key",
        "a case file has no other keys: tags are not one in this version",
        ["case-files"],
        load('{"version": 1, "cases": {"items": [], "tags": ["a"]}}'),
        "/cases/tags",
    )
    refused(
        "case-not-object",
        "a case is an object",
        ["cases"],
        case_file('["a", {}]'),
        "/cases/items/0",
    )
    refused(
        "id-missing", "a case has an id", ["cases"], case_file('{"input": {}}'), "/cases/items/0/id"
    )
    refused(
        "id-not-string",
        "an id is a string",
        ["identity"],
        case_file('{"id": 1, "input": {}}'),
        "/cases/items/0/id",
    )
    refused(
        "id-empty",
        "an id is not empty",
        ["identity"],
        case_file('{"id": "", "input": {}}'),
        "/cases/items/0/id",
    )
    refused(
        "id-repeated",
        "a case whose id repeats an earlier case's is refused at its id",
        ["identity", "reading-and-writing"],
        case_file(
            '{"id": "a", "input": {}}', '{"id": "b", "input": {}}', '{"id": "a", "input": {}}'
        ),
        "/cases/items/2/id",
    )
    refused(
        "description-not-string",
        "a description is a string",
        ["cases"],
        case_file('{"id": "a", "description": 1, "input": {}}'),
        "/cases/items/0/description",
    )
    refused(
        "input-missing",
        "a case has an input, checked before unknown keys",
        ["cases", "reading-and-writing"],
        case_file('{"id": "a", "name": "a"}'),
        "/cases/items/0/input",
    )
    refused(
        "input-opaque",
        "an opaque value is not data",
        ["inputs"],
        case_file('{"id": "a", "input": {"x": {"$opaque": "bytes"}}}'),
        "/cases/items/0/input/x",
    )
    refused(
        "unknown-key",
        "a case has no other keys",
        ["cases"],
        case_file('{"id": "a", "input": {}, "name": "a"}'),
        "/cases/items/0/name",
    )
    refused(
        "expect-not-object",
        "an expectation is an object",
        ["cases"],
        case_file('{"id": "a", "input": {}, "expect": []}'),
        "/cases/items/0/expect",
    )
    refused(
        "expect-unknown-key",
        "an expectation has no other keys: fired is not one in this version",
        ["expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"fired": ["vip"]}}'),
        "/cases/items/0/expect/fired",
    )
    refused(
        "selected-not-array",
        "selected is an array",
        ["expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"selected": "vip"}}'),
        "/cases/items/0/expect/selected",
    )
    refused(
        "selected-not-string",
        "a rule is named by a string",
        ["rules-in-expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"selected": [1]}}'),
        "/cases/items/0/expect/selected/0",
    )
    refused(
        "selected-empty",
        "an empty string names no rule",
        ["rules-in-expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"selected": ["vip", ""]}}'),
        "/cases/items/0/expect/selected/1",
    )
    refused(
        "selected-repeated",
        "a rule is named once in selected",
        ["rules-in-expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"selected": ["a", "b", "a", 1]}}'),
        "/cases/items/0/expect/selected/2",
    )
    refused(
        "unknown-repeated",
        "a rule is named once in unknown",
        ["rules-in-expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"unknown": ["a", "a"]}}'),
        "/cases/items/0/expect/unknown/1",
    )
    refused(
        "output-not-object",
        "an output is an object or null",
        ["expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"output": "x"}}'),
        "/cases/items/0/expect/output",
    )
    refused(
        "outputs-not-array",
        "outputs is an array",
        ["expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"outputs": {}}}'),
        "/cases/items/0/expect/outputs",
    )
    refused(
        "outputs-null-item",
        "each item of outputs is an object: a selected rule always has an output",
        ["expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"outputs": [{}, null]}}'),
        "/cases/items/0/expect/outputs/1",
    )
    refused(
        "output-value-opaque",
        "an expected value is a data value",
        ["expectations"],
        case_file('{"id": "a", "input": {}, "expect": {"output": {"x": {"$opaque": "bytes"}}}}'),
        "/cases/items/0/expect/output/x",
    )
    refused(
        "output-int-overflow",
        "an expected int outside the signed 64-bit range is refused, not read as null",
        ["expectations", "reading-and-writing"],
        case_file('{"id": "a", "input": {}, "expect": {"output": {"n": 99999999999999999999}}}'),
        "/cases/items/0/expect/output/n",
    )
    refused(
        "output-typed-int-overflow",
        "an expected $int outside the signed 64-bit range is refused at its typed value",
        ["expectations"],
        case_file(
            '{"id": "a", "input": {},'
            ' "expect": {"output": {"n": {"$int": "-99999999999999999999"}}}}'
        ),
        "/cases/items/0/expect/output/n/$int",
    )
    refused(
        "output-decimal-overflow",
        "an expected decimal that does not fit is refused, not read as null",
        ["expectations"],
        case_file(
            '{"id": "a", "input": {},'
            f' "expect": {{"output": {{"d": {{"$decimal": "1{"0" * 38}"}}}}}}}}'
        ),
        "/cases/items/0/expect/output/d/$decimal",
    )
    refused(
        "outputs-int-overflow",
        "an int out of range is refused in outputs too",
        ["expectations"],
        case_file(
            '{"id": "a", "input": {},'
            ' "expect": {"outputs": [{}, {"n": [1, 99999999999999999999]}]}}'
        ),
        "/cases/items/0/expect/outputs/1/n/1",
    )
    refused(
        "error-as-string",
        "an expected error is an object with its code",
        ["errors"],
        case_file('{"id": "a", "input": {}, "expect": {"error": "missing_field"}}'),
        "/cases/items/0/expect/error",
    )
    refused(
        "error-code-missing",
        "an expected error has a code",
        ["errors"],
        case_file('{"id": "a", "input": {}, "expect": {"error": {}}}'),
        "/cases/items/0/expect/error/code",
    )
    for name, code, why in (
        ("error-code-format", "format_error", "a code that evaluation never raises"),
        (
            "error-code-parent",
            "evaluation_error",
            "a parent code, which this version does not match",
        ),
        ("error-code-unknown", "oops", "a string that is not a code"),
    ):
        refused(
            name,
            f"an expected error is not {why}",
            ["errors"],
            case_file(f'{{"id": "a", "input": {{}}, "expect": {{"error": {{"code": "{code}"}}}}}}'),
            "/cases/items/0/expect/error/code",
        )
    refused(
        "error-unknown-key",
        "an expected error holds its code only in this version",
        ["errors"],
        case_file(
            '{"id": "a", "input": {},'
            ' "expect": {"error": {"code": "missing_field", "rule": "vip"}}}'
        ),
        "/cases/items/0/expect/error/rule",
    )
    refused(
        "error-with-others",
        "an expected error is given alone",
        ["expectations", "reading-and-writing"],
        case_file(
            '{"id": "a", "input": {},'
            ' "expect": {"output": null, "error": {"code": "missing_field"}}}'
        ),
        "/cases/items/0/expect/error",
    )
    refused(
        "error-alone-after-keys",
        "an error given with others is checked after the expectation's unknown keys",
        ["reading-and-writing"],
        case_file(
            '{"id": "a", "input": {}, "expect": {"selected": [], "fired": [],'
            ' "error": {"code": "missing_field"}}}'
        ),
        "/cases/items/0/expect/fired",
    )
    refused(
        "first-case-first",
        "cases are read in order: the first case's error is reported",
        ["reading-and-writing"],
        case_file('{"id": "a"}', '{"input": {}}'),
        "/cases/items/0/input",
    )
    refused(
        "content-before-top-level",
        "the content's shape is checked before the document's unknown keys",
        ["reading-and-writing", "json-format.md#errors-and-limits"],
        load('{"version": 1, "x": 1, "cases": {"items": [{"input": {}}]}}'),
        "/cases/items/0/id",
    )
    refused(
        "top-level-unknown-key",
        "a case file has no other top-level keys",
        ["case-files"],
        load('{"version": 1, "cases": {"items": []}, "x": 1}'),
        "/x",
    )
    add(
        "version-unknown",
        "a case file of another format version is refused",
        ["case-files", "json-format.md#documents"],
        {"kind": "read", "document": load('{"version": 2, "cases": {"items": []}}')},
        {"error": {"code": "unsupported_version", "pointer": "/version"}},
    )

    # Canonical bytes and digests.
    def canonical_vector(name: str, description: str, document: Any) -> None:
        text = canonical_text(canonical_cases(document))
        add(
            name,
            description,
            ["reading-and-writing", DIGESTS],
            {"kind": "canonical", "document": document},
            {"canonical": text, "digest": digest(text)},
        )

    canonical_vector(
        "canonical-example",
        "a case file has canonical bytes and a digest, and gives no rule set",
        example,
    )
    canonical_vector(
        "canonical-sorted",
        "canonical bytes sort every member and the unknown rules, and keep selected in order",
        case_file(
            '{"input": {"b": 1e2, "a": {"$decimal": "1.50"}}, "id": "z",'
            ' "expect": {"unknown": ["y", "x"], "selected": ["y", "x"],'
            ' "outputs": [{"$k": 1, "a": null}]}}'
        ),
    )
    return {"version": 1, "area": "format", "vectors": vectors}


# --- Writing and checking ---------------------------------------------------------


FILES = {
    "canonical.json": canonical_vectors,
    "digests.json": decision_vectors,
    "bundles.json": bundle_vectors,
    "cases.json": case_file_vectors,
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
