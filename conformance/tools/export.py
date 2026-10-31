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


def main() -> int:
    """Write every vector file and report what was written."""
    known = anchors()
    files: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    seen: set[str] = set()
    problems: list[str] = []
    skipped: list[str] = []
    for case in CASES:
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
    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    for (area, topic), vectors in sorted(files.items()):
        path = VECTORS / area / f"{topic}.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        document = {"version": 1, "area": area, "vectors": vectors}
        path.write_text(
            json.dumps(document, indent=2, ensure_ascii=False, allow_nan=False) + "\n",
            encoding="utf-8",
        )
    for line in skipped:
        print(f"skipped {line}", file=sys.stderr)
    total = len(CASES) - len(skipped) + len(READ_VECTORS)
    print(f"{total} vectors in {len(files)} files, {len(skipped)} skipped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
