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
"""

from __future__ import annotations

import datetime as dt
import functools
import json
import math
import operator
import zoneinfo
from collections.abc import Callable
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
        return list(a) == list(e) and all(same(a[k], e[k]) for k in e)
    if isinstance(expected, list):
        if not isinstance(actual, list):
            return False
        a, e = cast(list[Any], actual), cast(list[Any], expected)
        return len(a) == len(e) and all(same(x, y) for x, y in zip(a, e, strict=True))
    return type(actual) is type(expected) and actual == expected


COMPARED = ("rule", "path", "types", "rules", "row")


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
    raise AssertionError(f"unknown op {op}")


def build_ruleset(content: dict[str, Any]) -> RuleSet:
    rules = [
        Rule(
            rule["name"],
            build(rule["when"]),
            {key: build(value) for key, value in rule.get("then", {}).items()},
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


def run_json(vector: dict[str, Any]) -> None:
    given, expect = vector["given"], vector["expect"]
    kind = given["kind"]
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


def run_native(vector: dict[str, Any]) -> None:
    given, expect = vector["given"], vector["expect"]
    kind = given["kind"]
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
        if "inputs" in given:
            expect_result(
                lambda: [
                    d.to_dict()["decision"]
                    for d in build_ruleset(given["ruleset"]).evaluate_many(native(given["inputs"]))
                ],
                expect,
                "decisions",
            )
        else:
            expect_result(
                lambda: (
                    build_ruleset(given["ruleset"])
                    .evaluate(native(given["input"]))
                    .to_dict()["decision"]
                ),
                expect,
                "decision",
            )
    elif kind == "explain":

        def decision() -> cleanrules.Decision:
            return build_ruleset(given["ruleset"]).evaluate(native(given["input"]))

        expect_result(lambda: decision().to_dict()["decision"], expect, "decision")
        expect_result(lambda: decision().explain().to_dict()["explanation"], expect, "explanation")
    elif kind == "summary":
        expect_result(
            lambda: build_ruleset(given["ruleset"]).summary().to_dict()["summary"],
            expect,
            "summary",
        )


def collect() -> list[Any]:
    params: list[Any] = []
    for path in sorted(VECTORS.rglob("*.json")):
        for vector in load(path.read_text(encoding="utf-8"))["vectors"]:
            params.append(pytest.param(vector, "json", id=f"json:{vector['id']}"))
            if vector["given"]["kind"] != "read":
                params.append(pytest.param(vector, "native", id=f"native:{vector['id']}"))
    return params


@pytest.mark.parametrize(("vector", "run"), collect())
def test_vector(vector: dict[str, Any], run: str) -> None:
    (run_json if run == "json" else run_native)(vector)


def test_every_vector_is_collected() -> None:
    # 2,135 vectors at the end of phase B; vectors are only ever added.
    assert len([p for p in collect() if p.id.startswith("json:")]) >= 2401
