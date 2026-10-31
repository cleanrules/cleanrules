"""Export conformance vectors by running the Python v1 on the cases of cases.py.

Temporary (phase A): deleted with the v1 in task A.4. The output is deterministic,
so running it twice leaves the vectors unchanged.

    uv run python conformance/tools/export.py
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

import encode
from cases import CASES, Case
from generated import generated_cases
from read_cases import VECTORS as READ_VECTORS

from rulepy.core.evaluate import evaluate_tree, evaluate_value_tree
from rulepy.errors import RulePyError

ROOT = Path(__file__).resolve().parents[2]
VECTORS = ROOT / "conformance" / "vectors"
SPEC = ROOT / "spec"


def anchors() -> set[str]:
    """Every `file.md#anchor` of the spec, with GitHub's heading anchors."""
    found: set[str] = set()
    for path in SPEC.glob("*.md"):
        for line in path.read_text(encoding="utf-8").splitlines():
            if heading := re.match(r"^#{1,6} (.+)$", line):
                text = re.sub(r"[^\w\- ]", "", heading[1].lower()).replace(" ", "-")
                found.add(f"{path.name}#{text}")
    return found


def dumps(data: Any, indent: int | None) -> str:
    """JSON text as written in the vector files."""
    return json.dumps(data, indent=indent, ensure_ascii=False, allow_nan=False)


def run(case: Case) -> tuple[dict[str, Any], dict[str, Any]]:
    """Run one case on the v1 and return its `given` and `expect`."""
    given: dict[str, Any] = {"kind": case.kind}
    expect: dict[str, Any]
    if case.kind in ("condition", "value"):
        assert case.expr is not None
        given["condition" if case.kind == "condition" else "value"] = encode.node(case.expr)
        given["input"] = encode.input_value(case.input)
        if case.strict:
            given["strict"] = True
        evaluator = evaluate_tree if case.kind == "condition" else evaluate_value_tree
        try:
            expect = {"result": encode.result(evaluator(case.expr, case.input, strict=case.strict))}
        except RulePyError as exc:
            expect = {"error": encode.error(exc)}
        return given, expect
    assert case.ruleset is not None
    rs = case.ruleset()
    given["ruleset"] = encode.ruleset(rs)
    if case.kind == "summary":
        return given, {"summary": encode.summary(rs)}
    try:
        if case.inputs is not None:
            given["inputs"] = [encode.input_value(item) for item in case.inputs]
            expect = {"decisions": [encode.decision(d) for d in rs.evaluate_many(case.inputs)]}
        else:
            given["input"] = encode.input_value(case.input)
            decision = rs.evaluate(case.input)
            if case.kind == "explain":
                expect = {
                    "decision": encode.decision(decision),
                    "explanation": encode.explanation(decision),
                }
            else:
                expect = {"decision": encode.decision(decision)}
    except RulePyError as exc:
        expect = {"error": encode.error(exc)}
    return given, expect


# Spec sections each group of vectors also checks (task A.3d), by id pattern.
EXTRA_SPEC: list[tuple[str, tuple[str, ...]]] = [
    (
        r"^(ast/|generated/(conditions|strict|values)/)",
        ("ast.md#evaluating-one-node", "json-format.md#result-trees"),
    ),
    (r"^generated/conditions/", ("ast.md#catalogue",)),
    (
        r"^(generated/strict/|ast/.*/strict|rules/(strict|batch|policies/(unique|any)-|aggregates/sum-not))",
        ("errors.md#comparing-errors",),
    ),
    (
        r"^(rules/strict/conditions-first|rules/batch/row|format/.*(order|before|first))",
        ("errors.md#which-error-comes-first",),
    ),
    (r"^rules/payloads/", ("rules.md#rules", "data-model.md#lists-and-maps")),
    (r"^(rules/aggregates/|rules/policies/(collect|priority))", ("rules.md#deviations-from-dmn",)),
    (r"^rules/policies/any-agree", ("data-model.md#equality",)),
    (
        r"^format/conditions/(and-one|in-mixed|in-null|in-naive|between-bound|between-boolean|null-|arithmetic|regex|path-(negative|empty))",
        ("ast.md#construction-rules",),
    ),
    (
        r"^format/(conditions/(condition-as|value-as|value-under)|rules/value-as-when)",
        ("ast.md#node-kinds",),
    ),
    (r"^format/conditions/(unknown-op|canonical-key-order)", ("ast.md#catalogue",)),
    (r"^format/conditions/in-(same-node|number-types|aware)", ("ast.md#structural-identity",)),
    (r"^format/values/decimal-", ("data-model.md#decimals",)),
    (r"^format/values/float-", ("data-model.md#floats",)),
    (r"^format/values/datetime-", ("data-model.md#datetimes",)),
    (
        r"^format/values/(int-overflow|int-underflow|decimal-39|decimal-1e38|float-nan)",
        ("data-model.md#constants",),
    ),
]


def with_extra_spec(vector: dict[str, Any]) -> dict[str, Any]:
    """Add the sections of EXTRA_SPEC that match the vector's id, keeping order."""
    refs = list(vector["spec"])
    for pattern, extra in EXTRA_SPEC:
        if re.search(pattern, vector["id"]):
            refs.extend(ref for ref in extra if ref not in refs)
    return {**vector, "spec": refs}


def decimal_overflow_vector(int_vector: dict[str, Any]) -> dict[str, Any]:
    """The decimal twin of the int overflow vector, written from the spec.

    The v1 has unbounded decimals, so it cannot produce it (`data-model.md`,
    *Differences from the Python v1*); the result is the int vector's with the code
    `decimal_overflow`.
    """
    text = json.dumps(int_vector).replace('"int_overflow"', '"decimal_overflow"')
    vector = json.loads(text)
    vector["id"] = "ast/reading/decimal-overflow-in-data"
    vector["description"] = (
        "a decimal that does not fit in the data is null with reason decimal_overflow"
    )
    vector["given"]["input"] = {"x": {"$decimal": "1" + "0" * 38}}
    return vector


def main() -> int:
    """Write every vector file and report what was written."""
    known = anchors()
    files: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    seen: set[str] = set()
    problems: list[str] = []
    skipped: list[str] = []
    all_cases = [*CASES, *generated_cases()]
    for case in all_cases:
        if case.id in seen:
            problems.append(f"{case.id}: duplicate id")
        seen.add(case.id)
        problems.extend(
            f"{case.id}: unknown spec section {ref}" for ref in case.spec if ref not in known
        )
        try:
            given, expect = run(case)
        except encode.Unencodable as exc:
            skipped.append(f"{case.id}: {exc}")
            continue
        area, topic, *_ = case.id.split("/")
        files[(area, topic)].append(
            {
                "id": case.id,
                "spec": list(case.spec),
                "description": case.description,
                "given": given,
                "expect": expect,
            }
        )
    for vector in READ_VECTORS:
        if vector["id"] in seen:
            problems.append(f"{vector['id']}: duplicate id")
        seen.add(vector["id"])
        problems.extend(
            f"{vector['id']}: unknown spec section {ref}"
            for ref in vector["spec"]
            if ref not in known
        )
        area, topic, *_ = vector["id"].split("/")
        files[(area, topic)].append(vector)
    for (area, topic), vectors in files.items():
        for vector in list(vectors):
            if vector["id"] == "ast/reading/int-overflow-in-data":
                vectors.append(decimal_overflow_vector(vector))
        files[(area, topic)] = [with_extra_spec(v) for v in vectors]
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    for (area, topic), vectors in sorted(files.items()):
        path = VECTORS / area / f"{topic}.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        if area == "generated":
            # Thousands of vectors: one per line keeps the files small and diffable.
            lines = ",\n".join(dumps(v, indent=None) for v in vectors)
            body = f'{{"version": 1, "area": "{area}", "vectors": [\n{lines}\n]}}\n'
        else:
            body = dumps({"version": 1, "area": area, "vectors": vectors}, indent=2) + "\n"
        path.write_text(body, encoding="utf-8")
    for line in skipped:
        print(f"skipped {line}", file=sys.stderr)
    total = len(all_cases) - len(skipped) + len(READ_VECTORS)
    print(f"{total} vectors in {len(files)} files, {len(skipped)} skipped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
