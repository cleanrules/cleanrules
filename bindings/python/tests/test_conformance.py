# The runner handles arbitrary JSON and calls the private JSON entry points.
# pyright: reportUnknownArgumentType=false, reportUnknownVariableType=false
# pyright: reportUnknownMemberType=false, reportPrivateUsage=false
"""The Python conformance runner (`conformance/README.md`, *Runners*; task C.5).

Every vector runs twice:

- `json`: through the binding's JSON entry points (private until task D.1),
  which checks that documents and errors cross into Python intact;
- `native`: the input is converted into native Python values (`Decimal`,
  `datetime`, `dict`, ...), conditions and rules are built with `F`, `Const`,
  operators and `Rule`, and the results come back through the Python API. This
  run tests the binding's own code: construction, data reading and results.

`read` vectors, about the document reader, have only the `json` run; rule
texts (`rules_text`, `spec/text.md`) are read with `cleanrules.loads` and
compared with their document and canonical text. Text renderings of
explanations and summaries (`text` in their vectors) arrive with task D.2b.

Bundles (`spec/bundles.md`, `format/bundles/*`) run on both: `read` vectors
through `_read_document` (`json`) and through `cleanrules.loads`, `dumps` and
the `Bundle` properties (`native`); `bundle` vectors make a bundle with
`_make_bundle` or `cleanrules.Bundle`, and `bundle_compare` vectors compare
with `_compare_bundles` or `Bundle.compare`. A `current_engine` vector, and
the expected document of a `bundle` vector, name this engine's version
(`cleanrules.__version__`).
"""

from __future__ import annotations

import copy
import datetime as dt
import functools
import json
import math
import operator
import zoneinfo
from collections.abc import Callable, Mapping
from decimal import Decimal
from pathlib import Path
from typing import Any, TypedDict, cast

import cleanrules
import pytest
from cleanrules import (
    CleanRulesError,
    Const,
    Expr,
    Field,
    Rule,
    RuleSet,
    _cleanrules,  # pyright: ignore[reportPrivateUsage]
    evaluate_tree,
    evaluate_value_tree,
)

VECTORS = Path(__file__).resolve().parents[3] / "conformance" / "vectors"


class Number(str):
    """A JSON number as written, so that `1.0`, `1` and `1e-7` keep their text."""

    __slots__ = ()


def load(text: str) -> Any:
    return json.loads(text, parse_int=Number, parse_float=Number)


def dumps(value: Any) -> str:
    """Compact JSON, numbers written exactly as they were read."""
    if isinstance(value, Number):
        return str.__str__(value)
    if isinstance(value, dict):
        items = cast(dict[str, Any], value).items()
        return (
            "{"
            + ",".join(f"{json.dumps(k, ensure_ascii=False)}:{dumps(v)}" for k, v in items)
            + "}"
        )
    if isinstance(value, list):
        return "[" + ",".join(dumps(item) for item in cast(list[Any], value)) + "]"
    return json.dumps(value, ensure_ascii=False)


def typed(value: Any) -> Any:
    """Numbers as `int` (no fraction, no exponent) or `float`, as `json-format.md` reads them."""
    if isinstance(value, Number):
        text = str.__str__(value)
        return float(text) if any(c in text for c in ".eE") else int(text)
    if isinstance(value, dict):
        return {k: typed(v) for k, v in cast(dict[str, Any], value).items()}
    if isinstance(value, list):
        return [typed(item) for item in cast(list[Any], value)]
    return value


# --- Comparing ------------------------------------------------------------------------


def strip_detail(value: Any) -> Any:
    if isinstance(value, dict):
        return {k: strip_detail(v) for k, v in cast(dict[str, Any], value).items() if k != "detail"}
    if isinstance(value, list):
        return [strip_detail(item) for item in cast(list[Any], value)]
    return value


def same(actual: Any, expected: Any) -> bool:
    """Equality of JSON trees as `json-format.md` (*Comparing documents*) defines it."""
    if isinstance(expected, dict) and "$opaque" in expected:
        return isinstance(actual, dict) and "$opaque" in actual
    if isinstance(expected, bool) or isinstance(actual, bool):
        return type(actual) is type(expected) and actual == expected
    if isinstance(expected, float) or isinstance(actual, float):
        return (
            type(actual) is float
            and type(expected) is float
            and actual == expected
            and math.copysign(1, actual) == math.copysign(1, expected)
        )
    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            return False
        a, e = cast(dict[str, Any], actual), cast(dict[str, Any], expected)
        # Members have no order (`json-format.md`, *Objects*).
        return a.keys() == e.keys() and all(same(a[k], e[k]) for k in e)
    if isinstance(expected, list):
        if not isinstance(actual, list):
            return False
        a, e = cast(list[Any], actual), cast(list[Any], expected)
        return len(a) == len(e) and all(same(x, y) for x, y in zip(a, e, strict=True))
    return type(actual) is type(expected) and actual == expected


# `name` for `limit_exceeded` beyond `max_values`, which evaluation raises.
COMPARED = ("rule", "path", "types", "rules", "row", "name")


def error_dict(error: CleanRulesError, fields: tuple[str, ...]) -> dict[str, Any]:
    """The code and the compared context fields of an error (`spec/errors.md`)."""
    out: dict[str, Any] = {"code": error.code}
    for field in fields:
        value = getattr(error, field)
        if value is None or value == ():
            continue
        out[field] = list(value) if isinstance(value, tuple) else value
    return out


def check_error(error: CleanRulesError, expected: dict[str, Any], *, read: bool) -> None:
    if read:
        # Reading errors compare the code and the location given.
        actual = error_dict(error, ("pointer", "line", "column", "name"))
        for key, value in expected.items():
            assert same(actual.get(key), value), f"{key}: got {actual}, expected {expected}"
        return
    actual = error_dict(error, COMPARED)
    assert set(actual) == set(expected), f"got {actual}, expected {expected}"
    for key, value in expected.items():
        assert same(actual[key], value), f"{key}: got {actual}, expected {expected}"


def expect_result(
    produce: Callable[[], Any], expect: dict[str, Any], key: str, read: bool = False
) -> None:
    error = expect.get("error")
    try:
        actual = produce()
    except CleanRulesError as raised:
        if error is None:
            raise
        check_error(raised, typed(error), read=read)
        return
    assert error is None, f"expected the error {error}, got {actual}"
    wanted = strip_detail(typed(expect[key]))
    got = strip_detail(actual)
    assert same(got, wanted), f"\ngot  {json.dumps(got)}\nwant {json.dumps(wanted)}"


# --- Native values and nodes ----------------------------------------------------------


def datetime_value(text: str) -> dt.datetime:
    """A `$datetime`: naive, with an offset, or with an RFC 9557 zone name."""
    zone = None
    if text.endswith("]"):
        text, zone = text[:-1].split("[")
    value = dt.datetime.fromisoformat(text)
    if zone is None or value.tzinfo is None:
        return value
    try:
        named = value.astimezone(zoneinfo.ZoneInfo(zone))
    except zoneinfo.ZoneInfoNotFoundError:
        return value
    # The zone is kept only where its offset agrees, as the core does.
    return named if named.utcoffset() == value.utcoffset() else value


def native(value: Any) -> Any:
    """A JSON value of the format as a native Python value (`spec/data-model.md`)."""
    if isinstance(value, Number):
        return typed(value)
    if isinstance(value, list):
        return [native(item) for item in cast(list[Any], value)]
    if not isinstance(value, dict):
        return value
    data = cast(dict[str, Any], value)
    if len(data) == 1:
        ((key, inner),) = data.items()
        if key == "$int":
            return int(inner)
        if key == "$float":
            return float(inner)
        if key == "$decimal":
            return Decimal(inner)
        if key == "$date":
            return dt.date.fromisoformat(inner)
        if key == "$datetime":
            return datetime_value(inner)
        if key == "$map":
            return {k: native(v) for k, v in cast(dict[str, Any], inner).items()}
    return {k: native(v) for k, v in data.items()}


COMPARE: dict[str, Callable[[Any, Any], Expr]] = {
    "eq": operator.eq,
    "ne": operator.ne,
    "lt": operator.lt,
    "le": operator.le,
    "gt": operator.gt,
    "ge": operator.ge,
    "add": operator.add,
    "sub": operator.sub,
    "mul": operator.mul,
    "div": operator.truediv,
}


def build(node: dict[str, Any]) -> Expr:
    """A condition or value node of the format, built with the Python API."""
    op = node["op"]
    if op == "field":
        return Field(*typed(node["path"]))
    if op == "const":
        return Const(native(node["value"]))
    if op in COMPARE:
        return COMPARE[op](build(node["left"]), build(node["right"]))
    if op in ("and", "or"):
        combine = operator.and_ if op == "and" else operator.or_
        return functools.reduce(combine, [build(item) for item in node["operands"]])
    operand = build(node["operand"])
    if op == "not":
        return ~operand
    if op == "in":
        return operand.is_in([native(v) for v in node["values"]])
    if op == "between":
        return operand.between(
            build(node["low"]), build(node["high"]), closed=node.get("closed", "both")
        )
    if op == "is_null":
        return operand.is_null()
    if op == "matches":
        return operand.matches(node["pattern"])
    if op in ("starts_with", "ends_with", "contains"):
        return getattr(operand, op)(node["text"])
    if op == "abs":
        return abs(operand)
    if op == "neg":
        return -operand
    if op == "round":
        mode, scale = node.get("mode", "half_even"), typed(node["scale"])
        return round(operand, scale) if mode == "half_even" else operand.round(scale, mode)
    raise AssertionError(f"unknown op {op}")


def build_ruleset(content: dict[str, Any]) -> RuleSet:
    rules = [
        Rule(
            rule["name"],
            build(rule["when"]),
            {key: build(value) for key, value in rule.get("then", {}).items()},
            id=rule.get("id"),
            priority=typed(rule.get("priority", Number("0"))),
            tags=rule.get("tags", ()),
            description=rule.get("description"),
            metadata=native(rule.get("metadata", {})),
            do=rule.get("do"),
        )
        for rule in content["rules"]
    ]
    return RuleSet(
        rules,
        hit_policy=content.get("hit_policy", "first"),
        aggregate=content.get("aggregate"),
        logic=content.get("logic", "boolean"),
        strict=bool(content.get("strict", False)),
    )


# --- Running ---------------------------------------------------------------------------


class Limits(TypedDict, total=False):
    """A vector's limits, as `loads` takes them."""

    max_size: int
    max_nesting: int
    max_depth: int
    max_table_size: int
    max_path_segments: int


def run_text(given: dict[str, Any], expect: dict[str, Any], limits: Limits) -> None:
    """Read a rule text, or write a document as text (`spec/text.md`)."""
    source, fmt = (
        (given["rules_text"], "text")
        if "rules_text" in given
        else (dumps(given["document"]), "json")
    )

    def read_and_write() -> dict[str, Any]:
        read = cleanrules.loads(source, format=fmt, **limits)
        out: dict[str, Any] = {}
        if "document" in expect:
            out["document"] = json.loads(cleanrules.dumps(read))
        if "printed" in expect or "write" in given:
            out["printed"] = cleanrules.dumps(read, format="text")
        return out

    wanted = {key: expect[key] for key in ("document", "printed") if key in expect}
    expect_result(
        read_and_write,
        {"error": expect["error"]} if "error" in expect else {"read": wanted},
        "read",
        read=True,
    )


def run_table(given: dict[str, Any], expect: dict[str, Any], limits: Limits) -> None:
    """Read or build a decision table and write it back as a document, as CSV and compiled."""
    if "table_csv" in given:
        source, fmt = given["table_csv"], "csv"
    elif "yaml" in given:
        source, fmt = given["yaml"], "yaml"
    else:
        source, fmt = dumps(given.get("document", {})), "json"

    def table() -> cleanrules.DecisionTable:
        grid = given.get("table_grid")
        if grid is None:
            read = cleanrules.loads(source, format=fmt, **limits)
            assert isinstance(read, cleanrules.DecisionTable)
            return read
        # Strings are text and other items values (`spec/tables.md`, *Grids*).
        settings = {
            key: grid[key] for key in ("hit_policy", "aggregate", "logic", "strict") if key in grid
        }
        return cleanrules.DecisionTable(
            grid["header"],
            [[native(cell) for cell in row] for row in grid["rows"]],
            **settings,
            max_table_size=limits.get("max_table_size"),
            max_path_segments=limits.get("max_path_segments"),
        )

    def read_and_write() -> dict[str, Any]:
        table_read = table()
        out: dict[str, Any] = {}
        if "document" in expect:
            out["document"] = json.loads(cleanrules.dumps(table_read))
        if "csv" in expect:
            out["csv"] = cleanrules.dumps(table_read, format="csv")
        if "compiled" in expect:
            out["compiled"] = json.loads(cleanrules.dumps(table_read.ruleset))["ruleset"]
        return out

    wanted = {key: expect[key] for key in ("document", "csv", "compiled") if key in expect}
    expect_result(
        read_and_write,
        {"error": expect["error"]} if "error" in expect else {"read": wanted},
        "read",
        read=True,
    )


# Members of an agent error not compared (`conformance/README.md`, *Comparing*).
AGENT_FREE = ("message", "hint", "cause", "node", "operation", "reason")


def agent_error(error: dict[str, Any]) -> dict[str, Any]:
    """An agent error without its free text, after checking its hint (`spec/agents.md`)."""
    assert isinstance(error.get("hint"), str) and error["hint"], f"no hint in {error}"
    return {k: v for k, v in error.items() if k not in AGENT_FREE}


def run_tool(vector: dict[str, Any], route: str) -> None:
    """Run a `tool` vector with `cleanrules.Catalog`.

    Rule sets are read from JSON (`json`) or built with the Python API
    (`native`), and the arguments are JSON text (`json`) or Python data (`native`).
    """
    given, expect = vector["given"], vector["expect"]
    contents = given.get("rulesets", {})
    if route == "json":
        rulesets = {
            name: cast(
                RuleSet, cleanrules.loads(dumps({"version": Number("1"), "ruleset": content}))
            )
            for name, content in contents.items()
        }
        arguments: Any = given.get("arguments_text", None)
        if arguments is None:
            arguments = dumps(given["arguments"])
    else:
        rulesets = {name: build_ruleset(content) for name, content in contents.items()}
        arguments = given.get("arguments_text", None)
        if arguments is None:
            # Python data where it is a mapping; anything else only JSON text holds.
            given_arguments = given["arguments"]
            is_mapping = isinstance(given_arguments, dict)
            arguments = typed(given_arguments) if is_mapping else dumps(given_arguments)
    result = cleanrules.Catalog(rulesets).call(given["tool"], arguments)
    if "error" in expect:
        assert result.is_error, f"expected the error {expect['error']}, got {result.content}"
        actual = agent_error(result.content["error"])
        wanted = typed(expect["error"])
        assert set(actual) == set(wanted), f"got {actual}, expected {wanted}"
        assert all(same(actual[k], wanted[k]) for k in wanted), f"got {actual}, expected {wanted}"
        return
    assert not result.is_error, f"unexpected tool error {result.content}"
    got = dict(result.content)
    if "error" in got:
        got["error"] = agent_error(got["error"])
    wanted = typed(expect["output"])
    assert same(got, wanted), f"\ngot  {json.dumps(got)}\nwant {json.dumps(wanted)}"


def run_canonical(given: dict[str, Any], expect: dict[str, Any], *, built: bool) -> None:
    """Canonical bytes and digests (`spec/json-format.md`, *Canonical bytes*, *Digests*).

    The document is read with `loads`, or, on the native run, built in code
    from its content (a decision table is read: it is built from grids only).
    """
    document = given.get("document") or load(given["text"])
    if built and "condition" in document:
        value: Any = build(document["condition"])
    elif built and "rule" in document:
        value = build_ruleset({"rules": [document["rule"]]}).rules[0]
    elif built and "ruleset" in document:
        value = build_ruleset(document["ruleset"])
    else:
        value = cleanrules.loads(given.get("text") or dumps(document))
    canonical = cleanrules.canonical_bytes(value)
    assert canonical.decode("utf-8") == expect["canonical"], canonical
    assert cleanrules.digest(value) == expect["digest"]
    # The bytes read back as the same document.
    assert cleanrules.canonical_bytes(cleanrules.loads(canonical)) == canonical
    if isinstance(value, cleanrules.Bundle):
        assert value.ruleset_digest == expect["ruleset_digest"]
        ruleset: cleanrules.RuleSet | None = value.ruleset
    elif isinstance(value, cleanrules.RuleSet):
        ruleset = value
    elif isinstance(value, cleanrules.Rule):
        ruleset = cleanrules.RuleSet([value])
    elif isinstance(value, cleanrules.DecisionTable):
        ruleset = value.ruleset
    else:
        ruleset = None
    wanted = expect.get("ruleset_digest")
    assert (None if ruleset is None else cleanrules.digest(ruleset)) == wanted


# --- Bundles ----------------------------------------------------------------------------


def is_bundle_vector(vector: dict[str, Any]) -> bool:
    return str(vector["id"]).startswith("format/bundles/")


def this_engine(document: Any) -> Any:
    """A copy of a bundle document that names this engine's version (`current_engine`)."""
    fixed = copy.deepcopy(document)
    engine = fixed.get("bundle", {}).get("engine") if isinstance(fixed, dict) else None
    if isinstance(engine, dict) and "version" in engine:
        engine["version"] = cleanrules.__version__
    return fixed


def bundle_limits(given: dict[str, Any]) -> dict[str, int]:
    return {name: int(value) for name, value in given.get("limits", {}).items()}


def check_bundle(bundle: cleanrules.Bundle, document: dict[str, Any]) -> None:
    """The properties of a bundle agree with its document as written."""
    content = document["bundle"]
    assert bundle.id == content["id"]
    assert bundle.revision == content["revision"]
    assert bundle.ruleset_digest == content["ruleset_digest"]
    assert bundle.ruleset_digest == cleanrules.digest(bundle.ruleset)
    held = "decision_table" if "decision_table" in content else "ruleset"
    assert isinstance(
        bundle.rules, cleanrules.DecisionTable if held == "decision_table" else cleanrules.RuleSet
    )
    engine = bundle.engine
    assert engine.name == content["engine"]["name"]
    assert engine.version == content["engine"]["version"]
    assert engine.limits.to_dict() == content["engine"]["limits"]
    assert engine.is_current == (
        engine.name == "cleanrules" and engine.version == cleanrules.__version__
    )
    assert same(bundle.contract, content["contract"])
    cases = [{"path": case.path, "digest": case.digest} for case in bundle.cases]
    assert cases == content.get("cases", [])
    # The bytes read back as the same bundle, in JSON and YAML.
    assert cleanrules.loads(cleanrules.canonical_bytes(bundle)) == bundle
    assert cleanrules.loads(cleanrules.dumps(bundle, format="yaml"), format="yaml") == bundle


def run_read_bundle(vector: dict[str, Any], route: str) -> None:
    """A `read` vector of a bundle: read, then written back."""
    given, expect = vector["given"], vector["expect"]
    current = bool(given.get("current_engine", False))
    fmt: Any = "yaml" if "yaml" in given else "json"
    if "yaml" in given:
        source = given["yaml"]
    else:
        document = given["document"]
        source = dumps(this_engine(document) if current else document)
    wanted = dict(expect)
    if current and "document" in wanted:
        wanted["document"] = this_engine(wanted["document"])

    def read() -> Any:
        if route == "json":
            return json.loads(_cleanrules._read_document(source, format=fmt))  # pyright: ignore[reportPrivateUsage]
        bundle = cleanrules.loads(source, format=fmt)
        assert isinstance(bundle, cleanrules.Bundle), f"read {bundle!r}"
        written = json.loads(cleanrules.dumps(bundle))
        check_bundle(bundle, written)
        return written

    expect_result(read, wanted, "document", read=True)


def run_make_bundle(vector: dict[str, Any], route: str) -> None:
    """A `bundle` vector: make a bundle with this engine, write it, read it back."""
    given, expect = vector["given"], vector["expect"]
    limits = bundle_limits(given)
    cases = [(case["path"], case["text"]) for case in given.get("cases", [])]
    wanted = dict(expect)
    if "document" in wanted:
        wanted["document"] = this_engine(wanted["document"])

    def make() -> Any:
        if route == "json":
            text = _cleanrules._make_bundle(  # pyright: ignore[reportPrivateUsage]
                dumps(given["document"]), given["id"], given["revision"], cases, **limits
            )
            return json.loads(text)
        content = given["document"]
        rules: Any = (
            build_ruleset(content["ruleset"])
            if "ruleset" in content
            else cleanrules.loads(dumps(content))
        )
        bundle = cleanrules.Bundle(
            rules,
            id=given["id"],
            revision=given["revision"],
            cases=cases,
            max_size=limits.get("max_size"),
            max_nesting=limits.get("max_nesting"),
            max_depth=limits.get("max_depth"),
            max_path_segments=limits.get("max_path_segments"),
            max_table_size=limits.get("max_table_size"),
            max_values=limits.get("max_values"),
        )
        written = json.loads(cleanrules.dumps(bundle))
        # This engine reads what it made, verifying its digest and contract.
        again = cleanrules.loads(cleanrules.dumps(bundle))
        assert again == bundle and json.loads(cleanrules.dumps(again)) == written
        check_bundle(bundle, written)
        for path, text in cases:
            assert bundle.case_matches(path, text)
        return written

    # Errors are compared by their code only.
    expect_result(make, wanted, "document", read=True)


def run_compare_bundles(vector: dict[str, Any], route: str) -> None:
    """A `bundle_compare` vector: read both bundles, compare `a` with `b`."""
    given, expect = vector["given"], vector["expect"]
    a, b = dumps(given["a"]), dumps(given["b"])
    if route == "json":
        forth = json.loads(_cleanrules._compare_bundles(a, b))  # pyright: ignore[reportPrivateUsage]
        back = json.loads(_cleanrules._compare_bundles(b, a))  # pyright: ignore[reportPrivateUsage]
    else:
        first, second = cleanrules.loads(a), cleanrules.loads(b)
        assert isinstance(first, cleanrules.Bundle) and isinstance(second, cleanrules.Bundle)
        comparison = first.compare(second)
        assert comparison.identical == (first == second)
        forth, back = comparison.to_dict(), second.compare(first).to_dict()
    # Symmetric, but for which bundle is first.
    assert back == forth, f"not symmetric: {forth} and {back}"
    expect_result(lambda: forth, expect, "comparison")


def run_bundle(vector: dict[str, Any], route: str) -> bool:
    """Run a vector of `format/bundles`; whether it was one that this runs."""
    kind = vector["given"]["kind"]
    if kind == "read" and is_bundle_vector(vector):
        run_read_bundle(vector, route)
    elif kind == "bundle":
        run_make_bundle(vector, route)
    elif kind == "bundle_compare":
        run_compare_bundles(vector, route)
    else:
        return False
    return True


def run_json(vector: dict[str, Any]) -> None:
    given, expect = vector["given"], vector["expect"]
    kind = given["kind"]
    if run_bundle(vector, "json"):
        return
    if kind == "canonical":
        run_canonical(given, expect, built=False)
        return
    if kind == "read":
        limits = cast(Limits, {k: int(v) for k, v in given.get("limits", {}).items()})
        if "rules_text" in given or given.get("write") == "text":
            run_text(given, expect, limits)
            return
        if (
            "table_csv" in given
            or "table_grid" in given
            or given.get("write") == "csv"
            or "csv" in expect
            or "compiled" in expect
        ):
            run_table(given, expect, limits)
            return
        if "text" in given:
            text = given["text"]
        elif "yaml" in given:
            text = given["yaml"]
        else:
            text = dumps(given["document"])
        if "ruleset" in given:

            def decision() -> Any:
                content = json.loads(
                    _cleanrules._read_decision(
                        dumps(given["ruleset"]),
                        text,
                        "yaml" if "yaml" in given else "json",
                        max_size=limits.get("max_size"),
                        max_nesting=limits.get("max_nesting"),
                        max_depth=limits.get("max_depth"),
                    )
                )  # pyright: ignore[reportPrivateUsage]
                return {"version": 1, "decision": content}

            expect_result(decision, expect, "document", read=True)
        else:
            # Read with the public API: the Python objects are rebuilt and written back.
            fmt: Any = "yaml" if "yaml" in given else "json"
            source = given.get("yaml", text)
            expect_result(
                lambda: json.loads(
                    cleanrules.dumps(cleanrules.loads(source, format=fmt, **limits))
                ),
                expect,
                "document",
                read=True,
            )
    elif kind in ("condition", "value"):
        expect_result(
            lambda: json.loads(
                _cleanrules._evaluate_node(  # pyright: ignore[reportPrivateUsage]
                    dumps(given[kind]),
                    dumps(given["input"]),
                    given.get("logic", "boolean"),
                    bool(given.get("strict", False)),
                    kind == "value",
                )
            ),
            expect,
            "result",
        )
    elif kind == "evaluate":
        ruleset = dumps(given["ruleset"])
        if "inputs" in given:
            expect_result(
                lambda: json.loads(_cleanrules._evaluate_many(ruleset, dumps(given["inputs"]))),  # pyright: ignore[reportPrivateUsage]
                expect,
                "decisions",
            )
        else:
            expect_result(
                lambda: json.loads(_cleanrules._evaluate(ruleset, dumps(given["input"]))),  # pyright: ignore[reportPrivateUsage]
                expect,
                "decision",
            )
    elif kind == "explain":
        pair = lambda: _cleanrules._explain(dumps(given["ruleset"]), dumps(given["input"]))  # pyright: ignore[reportPrivateUsage]  # noqa: E731
        expect_result(lambda: json.loads(pair()[0]), expect, "decision")
        expect_result(lambda: json.loads(pair()[1]), expect, "explanation")
    elif kind == "summary":
        expect_result(
            lambda: json.loads(_cleanrules._summary(dumps(given["ruleset"]))),  # pyright: ignore[reportPrivateUsage]
            expect,
            "summary",
        )
    else:
        raise AssertionError(f"unknown kind {kind}")


def by_key(value: Any, where: str) -> None:
    """Every dict in `value` lists its keys by code point (data-model.md, *Lists and maps*)."""
    if isinstance(value, Mapping):
        mapping = cast(Mapping[Any, Any], value)
        keys = list(mapping)
        assert keys == sorted(keys), f"{where}: keys {keys} are not by code point"
        for item in mapping.values():
            by_key(item, where)
    elif isinstance(value, (list, tuple)):
        for item in cast(list[Any], value):
            by_key(item, where)


def given_by_key(decision: cleanrules.Decision) -> cleanrules.Decision:
    """The maps a decision and its rules give back come by key, by code point."""
    by_key(decision.outputs, "outputs")
    by_key(decision.aggregated, "aggregated")
    for rule in decision.ruleset:
        by_key(dict(rule.then), f"{rule.name}.then")
        by_key(dict(rule.metadata), f"{rule.name}.metadata")
    return decision


def run_native(vector: dict[str, Any]) -> None:
    given, expect = vector["given"], vector["expect"]
    kind = given["kind"]
    if run_bundle(vector, "native"):
        return
    if kind == "canonical":
        run_canonical(given, expect, built=True)
        return
    if kind in ("condition", "value"):
        strict = bool(given.get("strict", False))

        def tree() -> Any:
            node = build(given[kind])
            data = native(given["input"])
            if kind == "value":
                return evaluate_value_tree(node, data, strict=strict).to_dict()
            return evaluate_tree(
                node, data, logic=given.get("logic", "boolean"), strict=strict
            ).to_dict()

        expect_result(tree, expect, "result")
    elif kind == "evaluate":
        wanted = given.get("limits", {}).get("max_values")
        max_values = None if wanted is None else int(wanted)
        if "inputs" in given:
            expect_result(
                lambda: [
                    d.to_dict()["decision"]
                    for d in build_ruleset(given["ruleset"]).evaluate_many(
                        native(given["inputs"]), max_values=max_values
                    )
                ],
                expect,
                "decisions",
            )
        else:
            expect_result(
                lambda: given_by_key(
                    build_ruleset(given["ruleset"]).evaluate(
                        native(given["input"]), max_values=max_values
                    )
                ).to_dict()["decision"],
                expect,
                "decision",
            )
    elif kind == "explain":

        def decision() -> cleanrules.Decision:
            return given_by_key(build_ruleset(given["ruleset"]).evaluate(native(given["input"])))

        expect_result(lambda: decision().to_dict()["decision"], expect, "decision")
        expect_result(lambda: decision().explain().to_dict()["explanation"], expect, "explanation")
    elif kind == "summary":
        expect_result(
            lambda: build_ruleset(given["ruleset"]).summary().to_dict()["summary"],
            expect,
            "summary",
        )
    else:
        raise AssertionError(f"unknown kind {kind}")


DRAW_SKIP = (
    "diagrams (spec/visual.md) are drawn by the core and checked by its runner and the C ABI's;"
    " the binding draws them from phase V.2"
)


def collect() -> list[Any]:
    params: list[Any] = []
    for path in sorted(VECTORS.rglob("*.json")):
        for vector in load(path.read_text(encoding="utf-8"))["vectors"]:
            given = vector["given"]
            if given["kind"] == "draw":
                # Diagrams are written by the core and checked through the C ABI;
                # the Python API to draw them comes with V.2.
                params.append(
                    pytest.param(
                        vector,
                        "json",
                        id=f"json:{vector['id']}",
                        marks=pytest.mark.skip(reason=DRAW_SKIP),
                    )
                )
                continue
            if given["kind"] == "evaluate" and "limits" in given:
                # `max_values` bounds native data only: no JSON run.
                params.append(pytest.param(vector, "native", id=f"native:{vector['id']}"))
                continue
            if given.get("logic", "boolean") != "boolean":
                # Conditions under a graded logic: the binding evaluates one node
                # under `boolean` only until task J.1b.
                for run in ("json", "native"):
                    params.append(
                        pytest.param(
                            vector,
                            run,
                            id=f"{run}:{vector['id']}",
                            marks=pytest.mark.skip(reason="graded logics in the binding: J.1b"),
                        )
                    )
                continue
            params.append(pytest.param(vector, "json", id=f"json:{vector['id']}"))
            if vector["given"]["kind"] != "read" or is_bundle_vector(vector):
                params.append(pytest.param(vector, "native", id=f"native:{vector['id']}"))
    return params


@pytest.mark.parametrize(("vector", "run"), collect())
def test_vector(vector: dict[str, Any], run: str) -> None:
    if vector["given"]["kind"] == "tool":
        run_tool(vector, run)
        return
    (run_json if run == "json" else run_native)(vector)


def test_every_vector_is_collected() -> None:
    # 2,135 vectors at the end of phase B; vectors are only ever added.
    assert len([p for p in collect() if p.id.startswith("json:")]) >= 2937
