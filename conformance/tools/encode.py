"""Encode Python v1 (`rulepy`) objects in the JSON format v1 (`spec/json-format.md`).

Temporary: used by the exporter until the v1 is retired (task A.4). Every function
returns plain JSON data (dicts, lists, str, int, float, bool, None) in the canonical
form, so the vectors are byte-for-byte reproducible.
"""

from __future__ import annotations

import datetime as dt
import math
from collections.abc import Mapping, Sequence
from decimal import Decimal, localcontext
from typing import Any
from zoneinfo import ZoneInfo

from rulepy.core.evaluate import NodeResult, Reason
from rulepy.core.explain import Cause, CauseGroup, Requirement
from rulepy.core.expr import (
    Abs,
    And,
    Arith,
    Between,
    Compare,
    Const,
    Expr,
    Field,
    In,
    IsNull,
    Matches,
    Neg,
    Not,
    Or,
    StringTest,
)
from rulepy.core.logic import Truth
from rulepy.core.rule import Rule
from rulepy.core.ruleset import Decision, RuleSet

Json = Any

_SAFE_INT = 2**53 - 1
_MAX_COEFFICIENT = 10**38


class Unencodable(ValueError):
    """A v1 value the format cannot hold; the exporter skips such cases."""


# --- Values ------------------------------------------------------------------------


def value(v: object) -> Json:
    """Encode a data value or a constant (`json-format.md`, *Values*)."""
    if v is None or isinstance(v, bool | str):
        return v
    if isinstance(v, int):
        if not -(2**63) <= v < 2**63:
            raise Unencodable(f"int out of range: {v}")
        return v if abs(v) <= _SAFE_INT else {"$int": str(v)}
    if isinstance(v, float):
        if math.isnan(v):
            raise Unencodable("NaN")
        if math.isinf(v):
            return {"$float": "inf" if v > 0 else "-inf"}
        return v
    if isinstance(v, Decimal):
        return {"$decimal": decimal_text(v)}
    if isinstance(v, dt.datetime):
        return {"$datetime": datetime_text(v)}
    if isinstance(v, dt.date):
        return {"$date": v.isoformat()}
    if isinstance(v, Mapping):
        items: dict[str, Json] = {}
        for key, item in v.items():  # pyright: ignore[reportUnknownVariableType]
            if not isinstance(key, str):
                raise Unencodable(f"map key {key!r}")
            items[key] = value(item)
        return {"$map": items} if any(k.startswith("$") for k in items) else items
    if isinstance(v, Sequence) and not isinstance(v, bytes | bytearray):
        return [value(item) for item in v]  # pyright: ignore[reportUnknownVariableType]
    return {"$opaque": type(v).__name__}


def input_value(v: object) -> Json:
    """Encode an evaluation input: only JSON-like data, never opaque values or records."""
    if isinstance(v, Mapping):
        if not all(isinstance(k, str) for k in v):  # pyright: ignore[reportUnknownVariableType]
            raise Unencodable("input map with a non-string key")
        items = {k: input_value(x) for k, x in v.items()}  # pyright: ignore[reportUnknownVariableType]
        return {"$map": items} if any(k.startswith("$") for k in items) else items
    if isinstance(v, list | tuple) and not hasattr(v, "_fields"):
        return [input_value(x) for x in v]  # pyright: ignore[reportUnknownVariableType]
    if v is None or isinstance(v, bool | int | float | str | Decimal | dt.date):
        return value(v)
    raise Unencodable(f"input of type {type(v).__name__}")


def decimal_text(d: Decimal) -> str:
    """The canonical text of a decimal: plain notation, scale kept, no exponent."""
    if d.is_nan():
        raise Unencodable("decimal NaN")
    if d.is_infinite():
        return "-inf" if d.is_signed() else "inf"
    sign, digits, exponent = d.as_tuple()
    assert isinstance(exponent, int)
    coefficient = int("".join(map(str, digits)) or "0")
    if exponent > 0:
        coefficient *= 10**exponent
        exponent = 0
    scale = -exponent
    if coefficient >= _MAX_COEFFICIENT or scale > 38:
        raise Unencodable(f"decimal does not fit in 38 digits: {d}")
    text = str(coefficient).rjust(scale + 1, "0")
    if scale:
        text = f"{text[:-scale]}.{text[-scale:]}"
    return f"-{text}" if sign and coefficient else text


def datetime_text(v: dt.datetime) -> str:
    """The canonical text of a datetime, with its offset and IANA zone if aware."""
    text = v.replace(tzinfo=None).isoformat()
    if v.tzinfo is None:
        return text
    offset = v.utcoffset()
    if offset is None:
        return text
    zone = v.tzinfo.key if isinstance(v.tzinfo, ZoneInfo) else None
    if zone is None and offset == dt.timedelta(0):
        return f"{text}Z"
    return f"{text}{offset_text(offset)}" + (f"[{zone}]" if zone else "")


def offset_text(offset: dt.timedelta) -> str:
    """`±HH:MM`, with `:SS` only when the seconds are not zero."""
    if offset.microseconds:
        raise Unencodable("sub-second offset")
    seconds = int(offset.total_seconds())
    sign = "-" if seconds < 0 else "+"
    hours, rest = divmod(abs(seconds), 3600)
    minutes, secs = divmod(rest, 60)
    return f"{sign}{hours:02}:{minutes:02}" + (f":{secs:02}" if secs else "")


# --- Conditions and rules ----------------------------------------------------------------


def node(n: Expr) -> Json:
    """Encode a condition or value node in canonical form."""
    match n:
        case Field():
            return {"op": "field", "path": list(n.path)}
        case Const():
            return {"op": "const", "value": value(n.value)}
        case Compare():
            return {"op": n.op, "left": node(n.left), "right": node(n.right)}
        case And() | Or():
            return {"op": n.tag, "operands": [node(o) for o in n.operands]}
        case Not():
            return {"op": "not", "operand": node(n.operand)}
        case In():
            return {"op": "in", "operand": node(n.operand), "values": [value(v) for v in n.values]}
        case Between():
            return {
                "op": "between",
                "operand": node(n.operand),
                "low": node(n.low),
                "high": node(n.high),
                "closed": n.closed,
            }
        case IsNull():
            return {"op": "is_null", "operand": node(n.operand)}
        case Matches():
            return {"op": "matches", "operand": node(n.operand), "pattern": n.pattern}
        case StringTest():
            return {"op": n.op, "operand": node(n.operand), "text": n.text}
        case Arith():
            return {"op": n.op, "left": node(n.left), "right": node(n.right)}
        case Abs() | Neg():
            return {"op": n.tag, "operand": node(n.operand)}
        case _:
            raise Unencodable(f"node {n!r}")


def rule(r: Rule) -> Json:
    """Encode a rule, omitting optional keys at their default."""
    if r.do is not None:
        raise Unencodable("rule with an action")
    out: dict[str, Json] = {"name": r.name, "when": node(r.when)}
    if r.then:
        out["then"] = {key: node(v) for key, v in r.then.items()}
    if r.priority:
        out["priority"] = value(r.priority)
    if r.tags:
        out["tags"] = sorted(r.tags)
    if r.description is not None:
        out["description"] = r.description
    if r.metadata:
        out["metadata"] = {key: value(v) for key, v in r.metadata.items()}
    return out


def ruleset(rs: RuleSet) -> Json:
    """Encode a rule set's content (the object under `"ruleset"`)."""
    out: dict[str, Json] = {"hit_policy": rs.hit_policy.value}
    if rs.aggregate is not None:
        out["aggregate"] = rs.aggregate
    out["logic"] = rs.logic.name
    if rs.strict:
        out["strict"] = True
    out["rules"] = [rule(r) for r in rs]
    return out


# --- Results and decisions -------------------------------------------------------------


_TRUTH = {Truth.TRUE: True, Truth.FALSE: False, Truth.UNKNOWN: None}


def reason(r: Reason) -> Json:
    """Encode a reason; `detail` is left out, since it is never compared."""
    out: dict[str, Json] = {"code": r.code}
    if r.paths:
        out["paths"] = [list(p) for p in r.paths]
    if r.types:
        out["types"] = [t.replace(" ", "_") for t in r.types]
    return out


def result(r: NodeResult) -> Json:
    """Encode a result tree (`json-format.md`, *Result trees*)."""
    out: dict[str, Json] = {"op": r.node.tag}
    if isinstance(r.node, Field):
        out["path"] = list(r.node.path)
    if isinstance(r.value, Truth):
        out["truth"] = _TRUTH[r.value]
    else:
        out["value"] = value(r.value)
    if r.reason is not None:
        out["reason"] = reason(r.reason)
    if r.children:
        out["children"] = [result(c) for c in r.children]
    return out


def decision(d: Decision[Any]) -> Json:
    """Encode a decision's content (the object under `"decision"`)."""
    out: dict[str, Json] = {"hit_policy": d.hit_policy.value}
    if d.aggregate is not None:
        out["aggregate"] = d.aggregate
    out["logic"] = d.logic.name if d.logic is not None else "boolean"
    results: list[Json] = []
    for res in d.results:
        entry: dict[str, Json] = {
            "rule": res.rule.name,
            "fired": res.fired,
            "when": result(res.tree),
        }
        if res.output is not None:
            entry["then"] = {key: result(tree) for key, tree in res.output_trees.items()}
        results.append(entry)
    out["results"] = results
    out["selected"] = [res.rule.name for res in d.selected]
    if d.aggregate is not None:
        aggregated = d.aggregated
        out["aggregated"] = (
            None if aggregated is None else {k: value(v) for k, v in aggregated.items()}
        )
    return out


# --- Explanations and summaries ----------------------------------------------------------


def _pointers(d: Decision[Any]) -> dict[int, str]:
    found: dict[int, str] = {}

    def walk(r: NodeResult, pointer: str) -> None:
        found.setdefault(id(r), pointer)
        for i, child in enumerate(r.children):
            walk(child, f"{pointer}/children/{i}")

    for i, res in enumerate(d.results):
        walk(res.tree, f"/decision/results/{i}/when")
    return found


def _gap(gap: object, op: str, target: object, current: object) -> Json:
    """Encode a gap, checking it against the spec's arithmetic (`explain.md`).

    The v1 rounds decimal gaps to 28 digits and subtracts wall-clock times of aware
    datetimes in the same zone; such cases are not exported (`Unencodable`).
    """
    if current is not None:
        a, b = (target, current) if op in ("gt", "ge") else (current, target)
        exact: object
        if isinstance(a, dt.datetime) and isinstance(b, dt.datetime) and a.tzinfo is not None:
            exact = a.astimezone(dt.UTC) - b.astimezone(dt.UTC)
        elif isinstance(a, Decimal) or isinstance(b, Decimal):
            with localcontext(prec=200):
                exact = Decimal(a) - Decimal(b)  # pyright: ignore[reportArgumentType]
        else:
            exact = gap
        if exact != gap or (isinstance(exact, Decimal) and str(exact) != str(gap)):
            raise Unencodable(f"v1 gap {gap!r} differs from the spec's {exact!r}")
    if isinstance(target, dt.datetime):
        assert isinstance(gap, dt.timedelta)
        return {"microseconds": value(gap // dt.timedelta(microseconds=1))}
    if isinstance(target, dt.date):
        assert isinstance(gap, dt.timedelta)
        return {"days": value(gap.days)}
    return value(gap)


def _requirement(req: Requirement, pointers: dict[int, str]) -> Json:
    if isinstance(req, CauseGroup):
        return {req.kind: [_requirement(item, pointers) for item in req.items]}
    cause: Cause = req
    out: dict[str, Json] = {
        "condition": node(cause.result.node),
        "at": pointers[id(cause.result)],
        "truth": _TRUTH[cause.result.value],
        "negated": cause.negated,
        "values": [{"path": list(p), "value": value(v)} for p, v in cause.values],
    }
    if cause.result.value is Truth.UNKNOWN and cause.result.reason is not None:
        out["reason"] = reason(cause.result.reason)
    if cause.hint is not None:
        hint = cause.hint
        counterfactual: dict[str, Json] = {
            "path": list(hint.path),
            "op": hint.op,
            "target": value(hint.target),
        }
        if hint.gap is not None:
            current = next((v for p, v in cause.values if p == hint.path), None)
            counterfactual["gap"] = _gap(hint.gap, hint.op, hint.target, current)
        out["counterfactual"] = counterfactual
    return out


_NOTES = (
    ("higher priority", "lower_priority"),
    ("same priority", "same_priority"),
    ("ANY reports", "any_agrees"),
    ("comes first", "earlier_rule"),
)


def explanation(d: Decision[Any]) -> Json:
    """Encode `d.explain()`'s content (the object under `"explanation"`)."""
    pointers = _pointers(d)
    rules: list[Json] = []
    for entry in d.explain().rules:
        out: dict[str, Json] = {"rule": entry.name, "status": entry.status}
        if entry.status == "fired":
            assert entry.note is not None
            winner = d.selected[0].rule
            kind = next(k for text, k in _NOTES if text in entry.note)
            note: dict[str, Json] = {"kind": kind, "winner": winner.name}
            if kind.endswith("priority"):
                note["priority"] = value(entry.result.rule.priority)
                note["winner_priority"] = value(winner.priority)
            out["note"] = note
        if entry.status in ("not_fired", "unknown"):
            out["achievable"] = entry.achievable
        out["requirement"] = _requirement(entry.requirement, pointers)
        rules.append(out)
    return {
        "hit_policy": d.hit_policy.value,
        "logic": d.logic.name if d.logic is not None else "boolean",
        "selected": [res.rule.name for res in d.selected],
        "rules": rules,
    }


def summary(rs: RuleSet) -> Json:
    """Encode `rs.summary()`'s content (the object under `"summary"`)."""
    s = rs.summary()
    out: dict[str, Json] = {"hit_policy": rs.hit_policy.value}
    if rs.aggregate is not None:
        out["aggregate"] = rs.aggregate
    out["logic"] = rs.logic.name
    if rs.strict:
        out["strict"] = True
    out["rules"] = [
        {
            "name": r.name,
            "priority": value(r.priority),
            "outputs": list(r.then or {}),
            **({"action": True} if r.do is not None else {}),
        }
        for r in rs
    ]
    out["fields"] = [
        {
            "path": list(f.path),
            "conditions": list(f.in_conditions),
            "payloads": [{"rule": name, "key": key} for name, key in f.in_payloads],
        }
        for f in s.fields
    ]
    out["outputs"] = list(s.outputs)
    out["tags"] = [{"tag": tag, "rules": list(names)} for tag, names in s.tags.items()]
    return out


# --- Errors ------------------------------------------------------------------------


def error(exc: Exception) -> Json:
    """Encode a v1 error as its code and the context `spec/errors.md` compares."""
    from rulepy.core.expr import Field as _Field
    from rulepy.errors import (
        EvaluationError,
        HitPolicyError,
        MissingFieldError,
        TypeMismatchError,
    )

    out: dict[str, Json]
    if isinstance(exc, MissingFieldError):
        out = {"code": "missing_field", "rule": exc.rule, "path": list(exc.path)}
    elif isinstance(exc, TypeMismatchError):
        out = {"code": "type_mismatch", "rule": exc.rule}
        if exc.path is not None:
            out["path"] = list(exc.path)
        out["types"] = [t.replace(" ", "_") for t in exc.types]
    elif isinstance(exc, HitPolicyError):
        out = {"code": "hit_policy_error", "rules": list(exc.rules)}
    elif isinstance(exc, EvaluationError):
        out = {"code": "evaluation_error", "rule": exc.rule}
        if isinstance(exc.node, _Field):
            out["path"] = list(exc.node.path)
    else:
        raise Unencodable(f"error {type(exc).__name__}: {exc}")
    if getattr(exc, "row", None) is not None:
        out["row"] = exc.row  # pyright: ignore[reportAttributeAccessIssue]
    return {k: v for k, v in out.items() if v is not None}
