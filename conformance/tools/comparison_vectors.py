"""Generate and check the vectors of the comparator (`spec/comparison.md`).

The vectors compare two versions of rules on test cases (`kind`
`test_comparison`): every kind of change, expected changes written as the new
version's expectations, cases on one side only, files that cannot be read,
partial sides, and two bundles. The expected comparisons are written by hand
below, from the spec; the script only lays them out and computes what a hand
cannot: the SHA-256 digests of case files and the rule set digests, with the
oracle of `canonical_vectors.py`.

    python3 conformance/tools/comparison_vectors.py --write   # vectors/comparison/*.json
    python3 conformance/tools/comparison_vectors.py --check   # CI: files up to date
"""

from __future__ import annotations

import sys
from typing import Any

from canonical_vectors import (
    DEFAULT_LIMITS,
    LIMIT_KEYS,
    VECTORS,
    IntLit,
    bundle,
    digest,
    dump,
    engine,
    ruleset_digest_of,
    table_basic,
)
from test_run_vectors import (
    ENGINE_PLACEHOLDER,
    case,
    case_file,
    cmp,
    numbers,
    rule,
    ruleset,
    text,
)

# --- Building blocks ----------------------------------------------------------------

RUNNING = "comparison.md#running-a-comparison"
RELATION = "comparison.md#relation"
BUNDLES = "comparison.md#bundles"
JOINING = "comparison.md#joining"
CHANGES = "comparison.md#changes"
EXPECTED = "comparison.md#expected-changes"
EXPLAINING = "comparison.md#explaining-changes"
INTERRUPTION = "comparison.md#interruption"
JSON = "comparison.md#json"

CHANGE_KINDS = (
    "same_selection_other_output",
    "other_selection_same_output",
    "other_selection_other_output",
    "other_unknown",
    "error",
)


def rules_digest(document: Any) -> str:
    """The rule set digest of a rule set, decision table or bundle document."""
    if "bundle" in document:
        return document["bundle"]["ruleset_digest"]
    if "decision_table" in document:
        _, compiled = table_basic()
        return ruleset_digest_of(compiled)
    return ruleset_digest_of(numbers(document)["ruleset"])


def side(
    document: Any,
    counts: tuple[int, int, int, Any],
    outcomes: tuple[int, int, int],
    *,
    files: list[tuple[str, str]] | None = None,
    cases: bool = False,
    limits: dict[str, int] | None = None,
    unreadable: int = 0,
) -> dict[str, Any]:
    """A side of a comparison: its run's header and summary, and its outcomes.

    `counts` is (passed, failed, error, interrupted), `interrupted` None or
    (reason, remaining); `outcomes` is (nothing_selected, with_unknown,
    error), and `decisions` is what remains of the results after the errors
    and the `unreadable` files.
    """
    if "bundle" in document:
        content = document["bundle"]
        source = "decision_table" if "decision_table" in content else "ruleset"
        rules = content.get("ruleset") or content["decision_table"]
        engine_limits = content["engine"]["limits"]
        bundle_ref: Any = {"id": content["id"], "revision": content["revision"]}
    else:
        source = "decision_table" if "decision_table" in document else "ruleset"
        rules = document.get("ruleset") or document["decision_table"]
        engine_limits = {key: DEFAULT_LIMITS[key] for key in LIMIT_KEYS}
        bundle_ref = None
    out: dict[str, Any] = {"source": source, "hit_policy": rules["hit_policy"]}
    if "aggregate" in rules:
        out["aggregate"] = rules["aggregate"]
    out["logic"] = rules.get("logic", "boolean")
    if rules.get("strict"):
        out["strict"] = True
    out["ruleset_digest"] = rules_digest(document)
    out["engine"] = {**engine(ENGINE_PLACEHOLDER), "limits": engine_limits}
    if bundle_ref is not None:
        out["bundle"] = bundle_ref
    out["files"] = [{}] if cases else [{"path": p, "digest": digest(t)} for p, t in files or []]
    out["limits"] = dict(limits or {})
    passed, failed, error, interrupted = counts
    summary: dict[str, Any] = {
        "total": passed + failed + error,
        "passed": passed,
        "failed": failed,
        "error": error,
        "partial": interrupted is not None,
    }
    if interrupted is not None:
        summary["interrupted"] = {"reason": interrupted[0], "remaining": interrupted[1]}
    out["summary"] = summary
    nothing, unknown, errors = outcomes
    out["outcomes"] = {
        "decisions": summary["total"] - errors - unreadable,
        "nothing_selected": nothing,
        "with_unknown": unknown,
        "error": errors,
    }
    return out


MISSING = object()


def decision(
    status: str,
    selected: list[str],
    outputs: list[Any],
    unknown: list[str] | None = None,
    output: Any = MISSING,
    explanation: str | None = None,
) -> dict[str, Any]:
    """What a side gave: a decision.

    `output` defaults to the single output of a single-hit policy (the first
    output, or null); pass None for a null output and `NO_SINGLE` for a
    decision without one.
    """
    out: dict[str, Any] = {"status": status, "selected": selected}
    if output is MISSING:
        output = outputs[0] if outputs else None
    if output is not NO_SINGLE:
        out["output"] = output
    out["outputs"] = outputs
    out["unknown"] = unknown or []
    if explanation is not None:
        out["explanation_text"] = explanation
    return out


NO_SINGLE = object()


def raised(status: str, error: dict[str, Any]) -> dict[str, Any]:
    """What a side gave: an error, by its code and compared fields."""
    return {"status": status, "error": error}


def item(
    change: str,
    a: Any = None,
    b: Any = None,
    *,
    id: str | None = None,
    path: str | None = None,
    expected: bool | None = None,
    differences: list[str] | None = None,
    unstated: list[str] | None = None,
) -> dict[str, Any]:
    """A listed item.

    An unexpected change says why (`unstated`): unless given, `status` when
    side `b` gave a result that did not pass its case, and otherwise every
    part that differs (a case without expectations, or a case file, which
    has none).
    """
    out: dict[str, Any] = {}
    if path is not None:
        out["path"] = path
    if id is not None:
        out["id"] = id
    out["change"] = change
    if change in CHANGE_KINDS:
        out["expected"] = bool(expected)
        out["differences"] = differences or []
        if not expected:
            if unstated is None:
                case_b = id is not None and b is not None and b["status"] != "passed"
                unstated = ["status"] if case_b else out["differences"]
            out["unstated"] = unstated
    if a is not None:
        out["a"] = a
    if b is not None:
        out["b"] = b
    return out


def comparison(
    relation: str,
    a: dict[str, Any],
    b: dict[str, Any],
    items: list[dict[str, Any]],
    *,
    unchanged: int = 0,
    not_compared: int = 0,
    max_explained: int = 0,
    bundles: Any = None,
) -> dict[str, Any]:
    """A comparison's content, its summary counted from its items."""
    by_change = {kind: 0 for kind in CHANGE_KINDS}
    expected = unexpected = added = removed = 0
    for listed in items:
        kind = listed["change"]
        if kind == "added":
            added += 1
        elif kind == "removed":
            removed += 1
        else:
            by_change[kind] += 1
            if listed["expected"]:
                expected += 1
            else:
                unexpected += 1
    changed = expected + unexpected
    out: dict[str, Any] = {"relation": relation}
    if bundles is not None:
        out["bundles"] = bundles
    out["a"] = a
    out["b"] = b
    out["limits"] = {"max_explained": max_explained}
    out["summary"] = {
        "total": unchanged + changed + added + removed + not_compared,
        "unchanged": unchanged,
        "changed": changed,
        "expected": expected,
        "unexpected": unexpected,
        "added": added,
        "removed": removed,
        "not_compared": not_compared,
        "partial": a["summary"]["partial"] or b["summary"]["partial"],
        "by_change": by_change,
    }
    out["items"] = items
    return out


def given_side(
    document: Any,
    files: list[tuple[str, str]] | None = None,
    cases: Any = None,
    limits: dict[str, int] | None = None,
) -> dict[str, Any]:
    """What a vector gives for one side."""
    out: dict[str, Any] = {"document": document}
    if cases is not None:
        out["cases"] = cases
    if files is not None:
        out["files"] = [{"path": p, "text": t} for p, t in files]
    if limits:
        out["limits"] = limits
    return out


def vector(
    name: str,
    spec: list[str],
    description: str,
    a: dict[str, Any],
    b: dict[str, Any],
    expected: Any,
    *,
    topic: str,
    max_explained: int = 0,
    cancel_after: int | None = None,
) -> dict[str, Any]:
    """A `test_comparison` vector: a comparison, or an error (`{"error": …}`)."""
    given: dict[str, Any] = {"kind": "test_comparison", "a": a, "b": b}
    given["max_explained"] = max_explained
    if cancel_after is not None:
        given["cancel_after"] = cancel_after
    expect = expected if "error" in expected else {"test_comparison": expected}
    return {
        "id": f"comparison/{topic}/{name}",
        "spec": spec,
        "description": description,
        "given": given,
        "expect": expect,
    }


# --- The rules and cases ------------------------------------------------------------

BASE = ruleset(
    "first",
    rule("a", cmp("gt", "x", 0), {"v": 1}),
    rule("b", cmp("gt", "x", 1), {"v": 2}),
)
# a outputs 3: the same rules selected, other outputs.
OUT = ruleset(
    "first",
    rule("a", cmp("gt", "x", 0), {"v": 3}),
    rule("b", cmp("gt", "x", 1), {"v": 2}),
)
# b first, with a's output: other rules selected for x = 2, the same output.
SWAP_SAME = ruleset(
    "first",
    rule("b", cmp("gt", "x", 1), {"v": 1}),
    rule("a", cmp("gt", "x", 0), {"v": 1}),
)
# b first, with its own output: other rules, other outputs.
SWAP_OTHER = ruleset(
    "first",
    rule("b", cmp("gt", "x", 1), {"v": 2}),
    rule("a", cmp("gt", "x", 0), {"v": 1}),
)
# A third rule on y, which no case has: only the unknown rules change.
WITH_C = ruleset(
    "first",
    rule("a", cmp("gt", "x", 0), {"v": 1}),
    rule("b", cmp("gt", "x", 1), {"v": 2}),
    rule("c", cmp("gt", "y", 0), {"v": 9}),
)
UNIQUE = ruleset(
    "unique",
    rule("a", cmp("gt", "x", 0), {"v": 1}),
    rule("b", cmp("gt", "x", 1), {"v": 2}),
)
ORDERED = ruleset(
    "rule_order",
    rule("a", cmp("gt", "x", 0), {"v": 1}),
    rule("b", cmp("gt", "x", 1), {"v": 2}),
)
ORDERED_OUT = ruleset(
    "rule_order",
    rule("a", cmp("gt", "x", 0), {"v": 1}),
    rule("b", cmp("gt", "x", 1), {"v": 3}),
)
STRICT = ruleset("first", rule("a", cmp("gt", "x", 0), {"v": 1}), strict=True)
STRICT_DESCRIBED = ruleset(
    "first", rule("a", cmp("gt", "x", 0), {"v": 1}, description="other words"), strict=True
)
STRICT_Z = ruleset("first", rule("z", cmp("gt", "x", 0), {"v": 1}), strict=True)
ONE = ruleset("first", rule("a", cmp("gt", "x", 0), {"v": 1}))
TIERS = ruleset("first", rule("a", cmp("gt", "x", 0), {"discount": 1, "tier": "gold"}))
TIERS_SILVER = ruleset("first", rule("a", cmp("gt", "x", 0), {"discount": 1, "tier": "silver"}))
TIERS_BOTH = ruleset("first", rule("a", cmp("gt", "x", 0), {"discount": 3, "tier": "silver"}))
ONE_OUT = ruleset("first", rule("a", cmp("gt", "x", 0), {"v": 2}))

XY = text(case_file(case("x1", {"x": 1}), case("x2", {"x": 2}), case("none", {})))
XY_FILES = [("c.json", XY)]

# What BASE gives each case of XY.
BASE_X1 = decision("passed", ["a"], [{"v": 1}])
BASE_X2 = decision("passed", ["a"], [{"v": 1}])
BASE_NONE = decision("passed", [], [], ["a", "b"])


def changes() -> list[Any]:
    """The vectors of *Changes* and *Relation*."""
    t = "changes"
    base = given_side(BASE, XY_FILES)
    base_side = side(BASE, (3, 0, 0, None), (1, 1, 0), files=XY_FILES)
    return [
        vector(
            "identical",
            [RUNNING, RELATION, CHANGES, JSON],
            "a version compared with itself: the same rules on the same engine, every case"
            " unchanged and none listed",
            base,
            base,
            comparison("same_rules_same_engine", base_side, base_side, [], unchanged=3),
            topic=t,
        ),
        vector(
            "same-selection-other-output",
            [CHANGES, RELATION, JSON],
            "the same rule selected with another output; a case without expectations makes"
            " the change unexpected, and an unchanged case is only counted",
            base,
            given_side(OUT, XY_FILES),
            comparison(
                "other_rules_same_engine",
                base_side,
                side(OUT, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                [
                    item(
                        "same_selection_other_output",
                        BASE_X1,
                        decision("passed", ["a"], [{"v": 3}]),
                        path="c.json",
                        id="x1",
                        differences=["output"],
                    ),
                    item(
                        "same_selection_other_output",
                        BASE_X2,
                        decision("passed", ["a"], [{"v": 3}]),
                        path="c.json",
                        id="x2",
                        differences=["output"],
                    ),
                ],
                unchanged=1,
            ),
            topic=t,
        ),
        vector(
            "other-selection-same-output",
            [CHANGES],
            "another rule selected with the same output",
            base,
            given_side(SWAP_SAME, XY_FILES),
            comparison(
                "other_rules_same_engine",
                base_side,
                side(SWAP_SAME, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                [
                    item(
                        "other_selection_same_output",
                        BASE_X2,
                        decision("passed", ["b"], [{"v": 1}]),
                        path="c.json",
                        id="x2",
                        differences=["selected"],
                    ),
                ],
                unchanged=2,
            ),
            topic=t,
        ),
        vector(
            "other-selection-other-output",
            [CHANGES],
            "another rule selected with another output",
            base,
            given_side(SWAP_OTHER, XY_FILES),
            comparison(
                "other_rules_same_engine",
                base_side,
                side(SWAP_OTHER, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                [
                    item(
                        "other_selection_other_output",
                        BASE_X2,
                        decision("passed", ["b"], [{"v": 2}]),
                        path="c.json",
                        id="x2",
                        differences=["selected", "output"],
                    ),
                ],
                unchanged=2,
            ),
            topic=t,
        ),
        vector(
            "other-unknown",
            [CHANGES],
            "the same selection and outputs with other unknown rules; unknown rules are a set,"
            " compared sorted",
            base,
            given_side(WITH_C, XY_FILES),
            comparison(
                "other_rules_same_engine",
                base_side,
                side(WITH_C, (3, 0, 0, None), (1, 3, 0), files=XY_FILES),
                [
                    item(
                        "other_unknown",
                        BASE_X1,
                        decision("passed", ["a"], [{"v": 1}], ["c"]),
                        path="c.json",
                        id="x1",
                        differences=["unknown"],
                    ),
                    item(
                        "other_unknown",
                        BASE_X2,
                        decision("passed", ["a"], [{"v": 1}], ["c"]),
                        path="c.json",
                        id="x2",
                        differences=["unknown"],
                    ),
                    item(
                        "other_unknown",
                        BASE_NONE,
                        decision("passed", [], [], ["a", "b", "c"]),
                        path="c.json",
                        id="none",
                        differences=["unknown"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "new-error",
            [CHANGES, JSON],
            "a case that now raises an error: only the error is compared, and the side's"
            " status is error",
            base,
            given_side(UNIQUE, XY_FILES),
            comparison(
                "other_rules_same_engine",
                base_side,
                side(UNIQUE, (2, 0, 1, None), (1, 1, 1), files=XY_FILES),
                [
                    item(
                        "error",
                        BASE_X2,
                        raised("error", {"code": "hit_policy_error", "rules": ["a", "b"]}),
                        path="c.json",
                        id="x2",
                        differences=["error"],
                    ),
                ],
                unchanged=2,
            ),
            topic=t,
        ),
        vector(
            "same-error-unchanged",
            [CHANGES, RELATION],
            "errors with the same code and compared fields are the same, whatever the rules'"
            " descriptions: other rules, no change",
            given_side(STRICT, [("s.json", STRICT_CASES)]),
            given_side(STRICT_DESCRIBED, [("s.json", STRICT_CASES)]),
            comparison(
                "other_rules_same_engine",
                side(STRICT, (0, 0, 2, None), (0, 0, 2), files=[("s.json", STRICT_CASES)]),
                side(
                    STRICT_DESCRIBED,
                    (0, 0, 2, None),
                    (0, 0, 2),
                    files=[("s.json", STRICT_CASES)],
                ),
                [],
                unchanged=2,
            ),
            topic=t,
        ),
        vector(
            "other-error",
            [CHANGES],
            "errors of the same code that differ in a compared field (the rule) are a change",
            given_side(STRICT, [("s.json", STRICT_CASES)]),
            given_side(STRICT_Z, [("s.json", STRICT_CASES)]),
            comparison(
                "other_rules_same_engine",
                side(STRICT, (0, 0, 2, None), (0, 0, 2), files=[("s.json", STRICT_CASES)]),
                side(STRICT_Z, (0, 0, 2, None), (0, 0, 2), files=[("s.json", STRICT_CASES)]),
                [
                    item(
                        "error",
                        raised("error", missing_field("a")),
                        raised("error", missing_field("z")),
                        path="s.json",
                        id="missing",
                        differences=["error"],
                    ),
                    item(
                        "error",
                        raised("error", type_mismatch("a")),
                        raised("error", type_mismatch("z")),
                        path="s.json",
                        id="text",
                        differences=["error"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "no-single-output",
            [CHANGES, JSON],
            "a decision without a single output has no output key, and its outputs are"
            " compared position by position",
            given_side(ORDERED, XY_FILES),
            given_side(ORDERED_OUT, XY_FILES),
            comparison(
                "other_rules_same_engine",
                side(ORDERED, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                side(ORDERED_OUT, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                [
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a", "b"], [{"v": 1}, {"v": 2}], output=NO_SINGLE),
                        decision("passed", ["a", "b"], [{"v": 1}, {"v": 3}], output=NO_SINGLE),
                        path="c.json",
                        id="x2",
                        differences=["output"],
                    ),
                ],
                unchanged=2,
            ),
            topic=t,
        ),
        vector(
            "single-output-gained",
            [CHANGES],
            "a decision that gains a single output under another policy changes its output,"
            " though its outputs are the same",
            given_side(ORDERED, XY_FILES),
            base,
            comparison(
                "other_rules_same_engine",
                side(ORDERED, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                base_side,
                [
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a"], [{"v": 1}], output=NO_SINGLE),
                        BASE_X1,
                        path="c.json",
                        id="x1",
                        differences=["output"],
                    ),
                    item(
                        "other_selection_other_output",
                        decision("passed", ["a", "b"], [{"v": 1}, {"v": 2}], output=NO_SINGLE),
                        BASE_X2,
                        path="c.json",
                        id="x2",
                        differences=["selected", "output"],
                    ),
                    item(
                        "same_selection_other_output",
                        decision("passed", [], [], ["a", "b"], output=NO_SINGLE),
                        BASE_NONE,
                        path="c.json",
                        id="none",
                        differences=["output"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "table-and-its-rule-set",
            [RELATION],
            "a decision table and the rule set it compiles to are the same rules",
            given_side(TABLE, [("t.json", TABLE_CASES)]),
            given_side(COMPILED, [("t.json", TABLE_CASES)]),
            comparison(
                "same_rules_same_engine",
                side(TABLE, (1, 0, 0, None), (0, 0, 0), files=[("t.json", TABLE_CASES)]),
                side(COMPILED, (1, 0, 0, None), (0, 0, 0), files=[("t.json", TABLE_CASES)]),
                [],
                unchanged=1,
            ),
            topic=t,
        ),
        vector(
            "explained",
            [EXPLAINING, JSON],
            "max_explained changes are explained, each side with a decision: the unexpected"
            " changes first, in comparison order, then the expected ones",
            given_side(ONE, [("one.json", TWO_CASES)]),
            given_side(ONE_OUT, [("one.json", TWO_CASES_B)]),
            comparison(
                "other_rules_same_engine",
                side(ONE, (2, 0, 0, None), (0, 0, 0), files=[("one.json", TWO_CASES)]),
                side(ONE_OUT, (2, 0, 0, None), (0, 0, 0), files=[("one.json", TWO_CASES_B)]),
                [
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a"], [{"v": 1}]),
                        decision("passed", ["a"], [{"v": 2}]),
                        path="one.json",
                        id="first",
                        expected=True,
                        differences=["output"],
                    ),
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a"], [{"v": 1}], explanation=EXPLAINED_X1),
                        decision("passed", ["a"], [{"v": 2}], explanation=EXPLAINED_X1),
                        path="one.json",
                        id="second",
                        differences=["output"],
                    ),
                ],
                max_explained=1,
            ),
            topic=t,
            max_explained=1,
        ),
    ]


STRICT_CASES = text(case_file(case("missing", {}), case("text", {"x": "a"})))
TWO_CASES = text(case_file(case("first", {"x": 1}), case("second", {"x": 1})))
TWO_CASES_B = text(
    case_file(case("first", {"x": 1}, {"output": {"v": 2}}), case("second", {"x": 1}))
)
EXPLAINED_X1 = "HIT POLICY first: selected a\n[x] a (selected): x > 0\n    - x > 0 is true (x = 1)"
TABLE, _COMPILED = table_basic()
COMPILED = {"version": IntLit("1"), "ruleset": _COMPILED}
TABLE_CASES = text(case_file(case("vip", {"customer": {"years": 7}, "order": {"total": 2000}})))


def missing_field(rule_name: str) -> dict[str, Any]:
    """A missing_field error of x, by its compared fields."""
    return {"code": "missing_field", "rule": rule_name, "path": ["x"]}


def type_mismatch(rule_name: str) -> dict[str, Any]:
    """A type_mismatch error of x, by its compared fields."""
    return {
        "code": "type_mismatch",
        "rule": rule_name,
        "path": ["x"],
        "types": ["string", "number"],
    }


def expected() -> list[Any]:
    """The vectors of *Expected changes*."""
    t = "expected"
    plain = given_side(BASE, XY_FILES)
    stated = text(
        case_file(
            case("x1", {"x": 1}, {"selected": ["a"], "output": {"v": 3}}),
            case("x2", {"x": 2}, {"output": {"v": 3}}),
            case("none", {}),
        )
    )
    parts_a = text(case_file(case("only-selected", {"x": 2}), case("both", {"x": 2})))
    parts_b = text(
        case_file(
            case("only-selected", {"x": 2}, {"selected": ["b"]}),
            case("both", {"x": 2}, {"selected": ["b"], "outputs": [{"v": 2}]}),
        )
    )
    fails_b = text(case_file(case("fails", {"x": 2}, {"selected": ["a"], "output": {"v": 2}})))
    fails_a = text(case_file(case("fails", {"x": 2})))
    fixed_a = text(
        case_file(case("empty", {"x": 2}), case("unknown-only", {"x": 2}), case("stated", {"x": 2}))
    )
    fixed_b = text(
        case_file(
            case("empty", {"x": 2}),
            case("unknown-only", {"x": 2}, {"unknown": []}),
            case("stated", {"x": 2}, {"selected": ["a"]}),
        )
    )
    error_a = text(case_file(case("x2", {"x": 2})))
    error_b = text(case_file(case("x2", {"x": 2}, {"error": {"code": "hit_policy_error"}})))
    x2_b = decision("passed", ["b"], [{"v": 2}])
    keys_a = text(case_file(case("tier", {"x": 1}), case("both", {"x": 1})))
    keys_b = text(
        case_file(
            case("tier", {"x": 1}, {"output": {"discount": 1}}),
            case("both", {"x": 1}, {"output": {"discount": 3, "tier": "silver"}}),
        )
    )
    named_b = text(
        case_file(
            case("tier", {"x": 1}, {"output": {"tier": "silver"}}),
            case("both", {"x": 1}, {"outputs": [{"tier": "silver", "discount": 3}]}),
        )
    )
    gold = {"discount": 1, "tier": "gold"}
    return [
        vector(
            "unnamed-key-is-unexpected",
            [EXPECTED, CHANGES],
            "an expected output states the keys it names: a change of a key it does not name"
            " is unexpected, though the case passes",
            given_side(TIERS, [("k.json", keys_a)]),
            given_side(TIERS_SILVER, [("k.json", keys_b)]),
            comparison(
                "other_rules_same_engine",
                side(TIERS, (2, 0, 0, None), (0, 0, 0), files=[("k.json", keys_a)]),
                side(TIERS_SILVER, (1, 1, 0, None), (0, 0, 0), files=[("k.json", keys_b)]),
                [
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a"], [gold]),
                        decision("passed", ["a"], [{"discount": 1, "tier": "silver"}]),
                        path="k.json",
                        id="tier",
                        differences=["output"],
                        unstated=["output"],
                    ),
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a"], [gold]),
                        decision("failed", ["a"], [{"discount": 1, "tier": "silver"}]),
                        path="k.json",
                        id="both",
                        differences=["output"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "every-changed-key-named",
            [EXPECTED, CHANGES],
            "a change of an output is expected when the case names every key that differs, in"
            " output or at its position in outputs; naming only some of them leaves it"
            " unexpected, though the case passes",
            given_side(TIERS, [("k.json", keys_a)]),
            given_side(TIERS_BOTH, [("k.json", named_b)]),
            comparison(
                "other_rules_same_engine",
                side(TIERS, (2, 0, 0, None), (0, 0, 0), files=[("k.json", keys_a)]),
                side(TIERS_BOTH, (2, 0, 0, None), (0, 0, 0), files=[("k.json", named_b)]),
                [
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a"], [gold]),
                        decision("passed", ["a"], [{"discount": 3, "tier": "silver"}]),
                        path="k.json",
                        id="tier",
                        differences=["output"],
                        unstated=["output"],
                    ),
                    item(
                        "same_selection_other_output",
                        decision("passed", ["a"], [gold]),
                        decision("passed", ["a"], [{"discount": 3, "tier": "silver"}]),
                        path="k.json",
                        id="both",
                        expected=True,
                        differences=["output"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "stated-by-b",
            [EXPECTED, JOINING],
            "the new version's cases state the new output: the changes are expected, though"
            " one states only the part that differs; the old version fails them",
            plain,
            given_side(OUT, [("c.json", stated)]),
            comparison(
                "other_rules_same_engine",
                side(BASE, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                side(OUT, (3, 0, 0, None), (1, 1, 0), files=[("c.json", stated)]),
                [
                    item(
                        "same_selection_other_output",
                        BASE_X1,
                        decision("passed", ["a"], [{"v": 3}]),
                        path="c.json",
                        id="x1",
                        expected=True,
                        differences=["output"],
                    ),
                    item(
                        "same_selection_other_output",
                        BASE_X2,
                        decision("passed", ["a"], [{"v": 3}]),
                        path="c.json",
                        id="x2",
                        expected=True,
                        differences=["output"],
                    ),
                ],
                unchanged=1,
            ),
            topic=t,
        ),
        vector(
            "every-part-checked",
            [EXPECTED],
            "a change is expected only when the case checks every part that differs: the"
            " selection alone does not cover another output",
            given_side(BASE, [("p.json", parts_a)]),
            given_side(SWAP_OTHER, [("p.json", parts_b)]),
            comparison(
                "other_rules_same_engine",
                side(BASE, (2, 0, 0, None), (0, 0, 0), files=[("p.json", parts_a)]),
                side(SWAP_OTHER, (2, 0, 0, None), (0, 0, 0), files=[("p.json", parts_b)]),
                [
                    item(
                        "other_selection_other_output",
                        BASE_X2,
                        x2_b,
                        path="p.json",
                        id="only-selected",
                        expected=False,
                        differences=["selected", "output"],
                        unstated=["output"],
                    ),
                    item(
                        "other_selection_other_output",
                        BASE_X2,
                        x2_b,
                        path="p.json",
                        id="both",
                        expected=True,
                        differences=["selected", "output"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "b-fails",
            [EXPECTED],
            "a change is expected only when the new version passes its case",
            given_side(BASE, [("f.json", fails_a)]),
            given_side(SWAP_OTHER, [("f.json", fails_b)]),
            comparison(
                "other_rules_same_engine",
                side(BASE, (1, 0, 0, None), (0, 0, 0), files=[("f.json", fails_a)]),
                side(SWAP_OTHER, (0, 1, 0, None), (0, 0, 0), files=[("f.json", fails_b)]),
                [
                    item(
                        "other_selection_other_output",
                        BASE_X2,
                        decision("failed", ["b"], [{"v": 2}]),
                        path="f.json",
                        id="fails",
                        expected=False,
                        differences=["selected", "output"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "error-fixed",
            [EXPECTED],
            "an error that is gone is expected only by an expectation that states the new"
            " decision (its selection or outputs): not by an empty one, nor by unknown alone",
            given_side(UNIQUE, [("e.json", fixed_a)]),
            given_side(BASE, [("e.json", fixed_b)]),
            comparison(
                "other_rules_same_engine",
                side(UNIQUE, (0, 0, 3, None), (0, 0, 3), files=[("e.json", fixed_a)]),
                side(BASE, (3, 0, 0, None), (0, 0, 0), files=[("e.json", fixed_b)]),
                [
                    item(
                        "error",
                        raised("error", {"code": "hit_policy_error", "rules": ["a", "b"]}),
                        BASE_X2,
                        path="e.json",
                        id="empty",
                        expected=False,
                        differences=["error"],
                    ),
                    item(
                        "error",
                        raised("error", {"code": "hit_policy_error", "rules": ["a", "b"]}),
                        BASE_X2,
                        path="e.json",
                        id="unknown-only",
                        expected=False,
                        differences=["error"],
                    ),
                    item(
                        "error",
                        raised("error", {"code": "hit_policy_error", "rules": ["a", "b"]}),
                        BASE_X2,
                        path="e.json",
                        id="stated",
                        expected=True,
                        differences=["error"],
                    ),
                ],
            ),
            topic=t,
        ),
        vector(
            "error-expected",
            [EXPECTED, JSON],
            "a new error that the new version's case expects: the change is expected, and the"
            " side passed with its error",
            given_side(BASE, [("e.json", error_a)]),
            given_side(UNIQUE, [("e.json", error_b)]),
            comparison(
                "other_rules_same_engine",
                side(BASE, (1, 0, 0, None), (0, 0, 0), files=[("e.json", error_a)]),
                side(UNIQUE, (1, 0, 0, None), (0, 0, 1), files=[("e.json", error_b)]),
                [
                    item(
                        "error",
                        BASE_X2,
                        raised("passed", {"code": "hit_policy_error", "rules": ["a", "b"]}),
                        path="e.json",
                        id="x2",
                        expected=True,
                        differences=["error"],
                    ),
                ],
            ),
            topic=t,
        ),
    ]


def joining() -> list[Any]:
    """The vectors of *Joining*."""
    t = "joining"
    one_a = text(case_file(case("kept", {"x": 1}), case("gone", {"x": 1})))
    one_b = text(case_file(case("new", {"x": 0}), case("kept", {"x": 1})))
    other = text(case_file(case("t", {"x": 1})))
    files_a = [("one.json", one_a)]
    files_b = [("one.json", one_b), ("other.json", other)]
    readable = text(case_file(case("c", {"x": 1})))
    syntax = {"code": "format_error", "line": 1, "column": 2}
    return [
        vector(
            "added-and-removed",
            [JOINING],
            "cases are joined by path and id: a case only the new files hold is added, one"
            " only the old files hold is removed, each with its side's result, and they come"
            " in comparison order",
            given_side(BASE, files_a),
            given_side(BASE, files_b),
            comparison(
                "same_rules_same_engine",
                side(BASE, (2, 0, 0, None), (0, 0, 0), files=files_a),
                side(BASE, (3, 0, 0, None), (1, 0, 0), files=files_b),
                [
                    item(
                        "removed", decision("passed", ["a"], [{"v": 1}]), path="one.json", id="gone"
                    ),
                    item(
                        "added",
                        None,
                        decision("passed", [], []),
                        path="one.json",
                        id="new",
                    ),
                    item(
                        "added",
                        None,
                        decision("passed", ["a"], [{"v": 1}]),
                        path="other.json",
                        id="t",
                    ),
                ],
                unchanged=1,
            ),
            topic=t,
        ),
        vector(
            "unreadable-on-one-side",
            [JOINING, CHANGES],
            "a file one side cannot read is an error change against no result, and the cases"
            " the other side reads in it are not compared",
            given_side(BASE, [("one.json", readable)]),
            given_side(BASE, [("one.json", "not json")]),
            comparison(
                "same_rules_same_engine",
                side(BASE, (1, 0, 0, None), (0, 0, 0), files=[("one.json", readable)]),
                side(
                    BASE,
                    (0, 0, 1, None),
                    (0, 0, 0),
                    files=[("one.json", "not json")],
                    unreadable=1,
                ),
                [
                    item(
                        "error",
                        None,
                        raised("error", syntax),
                        path="one.json",
                        differences=["error"],
                    ),
                ],
                not_compared=1,
            ),
            topic=t,
        ),
        vector(
            "unreadable-on-both-sides",
            [JOINING, CHANGES],
            "a file both sides cannot read with the same error is not compared, since none of its"
            " cases is known; with other errors it is a change; a file that repeats a path is"
            " joined with the other side's",
            given_side(
                BASE,
                [
                    ("p.json", "not json"),
                    ("q.json", "{"),
                    ("r.json", readable),
                    ("r.json", readable),
                ],
            ),
            given_side(
                BASE,
                [
                    ("p.json", "not json"),
                    ("q.json", "[]"),
                    ("r.json", readable),
                    ("r.json", readable),
                ],
            ),
            comparison(
                "same_rules_same_engine",
                side(
                    BASE,
                    (1, 0, 3, None),
                    (0, 0, 0),
                    files=[
                        ("p.json", "not json"),
                        ("q.json", "{"),
                        ("r.json", readable),
                        ("r.json", readable),
                    ],
                    unreadable=3,
                ),
                side(
                    BASE,
                    (1, 0, 3, None),
                    (0, 0, 0),
                    files=[
                        ("p.json", "not json"),
                        ("q.json", "[]"),
                        ("r.json", readable),
                        ("r.json", readable),
                    ],
                    unreadable=3,
                ),
                [
                    item(
                        "error",
                        raised("error", {"code": "format_error", "line": 1, "column": 2}),
                        raised("error", {"code": "format_error", "pointer": ""}),
                        path="q.json",
                        differences=["error"],
                    ),
                ],
                unchanged=1,
                not_compared=2,
            ),
            topic=t,
        ),
    ]


def interruption() -> list[Any]:
    """The vectors of *Interruption*."""
    t = "interruption"
    base = given_side(BASE, XY_FILES)
    out_side = side(OUT, (3, 0, 0, None), (1, 1, 0), files=XY_FILES)
    x1 = item(
        "same_selection_other_output",
        BASE_X1,
        decision("passed", ["a"], [{"v": 3}]),
        path="c.json",
        id="x1",
        differences=["output"],
    )
    return [
        vector(
            "side-b-max-results",
            [INTERRUPTION, JSON],
            "side b stops at its max_results: the items it did not reach are not compared, and"
            " the comparison is partial",
            base,
            given_side(OUT, XY_FILES, limits={"max_results": 1}),
            comparison(
                "other_rules_same_engine",
                side(BASE, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                side(
                    OUT,
                    (1, 0, 0, ("max_results", 2)),
                    (0, 0, 0),
                    files=XY_FILES,
                    limits={"max_results": 1},
                ),
                [x1],
                not_compared=2,
            ),
            topic=t,
        ),
        vector(
            "side-a-max-steps",
            [INTERRUPTION],
            "side a stops at its max_steps, one step per rule and case: the cases it did not"
            " run are not compared, never unchanged",
            given_side(BASE, XY_FILES, limits={"max_steps": 3}),
            given_side(OUT, XY_FILES),
            comparison(
                "other_rules_same_engine",
                side(
                    BASE,
                    (1, 0, 0, ("max_steps", 2)),
                    (0, 0, 0),
                    files=XY_FILES,
                    limits={"max_steps": 3},
                ),
                out_side,
                [x1],
                not_compared=2,
            ),
            topic=t,
        ),
        vector(
            "cancelled",
            [INTERRUPTION],
            "the caller's callback is asked before each item of side a, then of side b; once it"
            " cancels, side b stops too",
            base,
            given_side(OUT, XY_FILES),
            comparison(
                "other_rules_same_engine",
                side(BASE, (3, 0, 0, None), (1, 1, 0), files=XY_FILES),
                side(OUT, (1, 0, 0, ("cancelled", 2)), (0, 0, 0), files=XY_FILES),
                [x1],
                not_compared=2,
            ),
            topic=t,
            cancel_after=4,
        ),
        vector(
            "cancelled-in-side-a",
            [INTERRUPTION],
            "a cancel during side a stops side b before its first item: both sides are partial",
            base,
            given_side(OUT, XY_FILES),
            comparison(
                "other_rules_same_engine",
                side(BASE, (1, 0, 0, ("cancelled", 2)), (0, 0, 0), files=XY_FILES),
                side(OUT, (0, 0, 0, ("cancelled", 3)), (0, 0, 0), files=XY_FILES),
                [],
                not_compared=3,
            ),
            topic=t,
            cancel_after=1,
        ),
    ]


def bundles() -> list[Any]:
    """The vectors of *Bundles*."""
    t = "bundles"
    stated = text(
        case_file(
            case("x1", {"x": 1}, {"output": {"v": 3}}),
            case("x2", {"x": 2}),
            case("none", {}),
        )
    )
    old = bundle(
        revision="1", ruleset=numbers(BASE)["ruleset"], cases=[case_ref("cases/c.json", XY)]
    )
    new = bundle(
        revision="2", ruleset=numbers(OUT)["ruleset"], cases=[case_ref("cases/c.json", stated)]
    )
    deeper = bundle(
        revision="1",
        ruleset=numbers(BASE)["ruleset"],
        engine_record=engine(max_depth=100),
        cases=[case_ref("cases/c.json", XY)],
    )
    conflicting = bundle(revision="1", ruleset=numbers(OUT)["ruleset"])
    files_old = [("cases/c.json", XY)]
    files_new = [("cases/c.json", stated)]
    return [
        vector(
            "two-bundles",
            [BUNDLES, RELATION, EXPECTED],
            "two bundles run their own case files under the limits they record, and the"
            " comparison holds the comparison of the two bundles",
            given_side(old, files_old),
            given_side(new, files_new),
            comparison(
                "other_rules_same_engine",
                side(old, (3, 0, 0, None), (1, 1, 0), files=files_old),
                side(new, (3, 0, 0, None), (1, 1, 0), files=files_new),
                [
                    item(
                        "same_selection_other_output",
                        BASE_X1,
                        decision("passed", ["a"], [{"v": 3}]),
                        path="cases/c.json",
                        id="x1",
                        expected=True,
                        differences=["output"],
                    ),
                    item(
                        "same_selection_other_output",
                        BASE_X2,
                        decision("passed", ["a"], [{"v": 3}]),
                        path="cases/c.json",
                        id="x2",
                        differences=["output"],
                    ),
                ],
                unchanged=1,
                bundles={
                    "relation": "other_rules_same_engine",
                    "identical": False,
                    "differences": ["revision", "rules", "cases"],
                },
            ),
            topic=t,
        ),
        vector(
            "same-rules-other-limits",
            [BUNDLES, RELATION],
            "the same rules under other recorded limits: the same rules on another engine,"
            " for the runs as for the bundles",
            given_side(old, files_old),
            given_side(deeper, files_old),
            comparison(
                "same_rules_other_engine",
                side(old, (3, 0, 0, None), (1, 1, 0), files=files_old),
                side(deeper, (3, 0, 0, None), (1, 1, 0), files=files_old),
                [],
                unchanged=3,
                bundles={
                    "relation": "same_rules_other_engine",
                    "identical": False,
                    "differences": ["engine.limits.max_depth"],
                },
            ),
            topic=t,
        ),
        vector(
            "conflict",
            [BUNDLES],
            "two bundles of one id and revision with other rules are a conflict: refused, and"
            " nothing runs",
            given_side(old, files_old),
            given_side(conflicting, []),
            {"error": {"code": "decision_mismatch"}},
            topic=t,
        ),
    ]


def case_ref(path: str, content: str) -> dict[str, Any]:
    """A bundle's reference to a case file holding `content`."""
    return {"path": path, "digest": digest(content)}


FILES = {
    "changes.json": changes,
    "expected.json": expected,
    "joining.json": joining,
    "interruption.json": interruption,
    "bundles.json": bundles,
}


def build(name: str) -> str:
    """A vector file's text."""
    vectors = [numbers(v) for v in FILES[name]()]
    return dump({"version": IntLit("1"), "area": "comparison", "vectors": vectors}) + "\n"


def main() -> int:
    """Write or check the files; return the exit code."""
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    folder = VECTORS / "comparison"
    problems: list[str] = []
    for name in FILES:
        path = folder / name
        written = build(name)
        if mode == "--write":
            folder.mkdir(exist_ok=True)
            path.write_text(written, encoding="utf-8")
        elif not path.exists() or path.read_text(encoding="utf-8") != written:
            problems.append(f"{path}: out of date; run with --write")
    for problem in problems:
        print(problem, file=sys.stderr)
    print(f"{len(FILES)} files, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
