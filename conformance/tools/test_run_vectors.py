"""Generate and check the vectors of the case runner (`spec/tests.md`).

The vectors run test cases against rules (`kind` `test_run`): what each case
gives (passed, failed with its mismatches, error), the case files of a
bundle, files that cannot be read, explained failures and interrupted runs.
The expected results are written by hand below, from the spec; the script
only lays them out and computes what a hand cannot: the SHA-256 digests of
case files and the rule set digests of bundles, with the oracle of
`canonical_vectors.py`.

    python3 conformance/tools/test_run_vectors.py --write   # conformance/vectors/tests/*.json
    python3 conformance/tools/test_run_vectors.py --check   # CI: files up to date
"""

from __future__ import annotations

import json
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
    load,
    table_basic,
)

# --- Building blocks ----------------------------------------------------------------

MATCHING = "tests.md#matching"
VALUES = "tests.md#values"
RUNNING = "tests.md#running-cases"
BUNDLES = "tests.md#case-files-of-a-bundle"
EXPLAINING = "tests.md#explaining-failures"
BATCHES = "tests.md#batches-and-interruption"
TEST_RUNS = "tests.md#test-runs"
RULES_IN = "tests.md#rules-in-expectations"


def field(*path: str) -> dict[str, Any]:
    """A field node."""
    return {"op": "field", "path": list(path)}


def const(value: Any) -> dict[str, Any]:
    """A constant node."""
    return {"op": "const", "value": value}


def cmp(op: str, path: str, value: Any) -> dict[str, Any]:
    """A comparison of a field with a constant."""
    return {"op": op, "left": field(*path.split(".")), "right": const(value)}


def rule(name: str, when: Any, then: dict[str, Any] | None = None, **extra: Any) -> Any:
    """A rule, its payload of constants."""
    out: dict[str, Any] = {"name": name, "when": when}
    if then:
        out["then"] = {k: const(v) for k, v in then.items()}
    out.update(extra)
    return out


def ruleset(policy: str, *rules: Any, **settings: Any) -> dict[str, Any]:
    """A rule set document."""
    content: dict[str, Any] = {"hit_policy": policy, "logic": "boolean", "rules": list(rules)}
    content.update(settings)
    return {"version": 1, "ruleset": content}


def dec(text: str) -> dict[str, str]:
    """A decimal value."""
    return {"$decimal": text}


def case(id: str, input: Any, expect: Any = None) -> dict[str, Any]:
    """A case."""
    out: dict[str, Any] = {"id": id, "input": input}
    if expect is not None:
        out["expect"] = expect
    return out


def case_file(*cases: Any) -> dict[str, Any]:
    """A case file document."""
    return {"version": 1, "cases": {"items": list(cases)}}


def text(document: Any) -> str:
    """A document's JSON text."""
    return json.dumps(document, ensure_ascii=False)


def summary(passed: int, failed: int, error: int, interrupted: Any = None) -> dict[str, Any]:
    """A run's summary."""
    out: dict[str, Any] = {
        "total": passed + failed + error,
        "passed": passed,
        "failed": failed,
        "error": error,
        "partial": interrupted is not None,
    }
    if interrupted is not None:
        out["interrupted"] = {"reason": interrupted[0], "remaining": interrupted[1]}
    return out


def run(
    policy: str,
    results: list[Any],
    counts: dict[str, Any],
    limits: dict[str, int] | None = None,
    **header: Any,
) -> dict[str, Any]:
    """A test run's content, explaining nothing unless its limits say so."""
    out: dict[str, Any] = {"source": header.pop("source", "ruleset"), "hit_policy": policy}
    if "aggregate" in header:
        out["aggregate"] = header.pop("aggregate")
    out["logic"] = "boolean"
    out.update(header)
    out["limits"] = {"max_explained": 0, **(limits or {})}
    out["summary"] = counts
    out["results"] = results
    return out


def passed(id: str, path: str | None = None) -> dict[str, Any]:
    """A passed result."""
    return {**({"path": path} if path else {}), "id": id, "status": "passed"}


def failed(id: str, *mismatches: Any, path: str | None = None, **extra: Any) -> dict[str, Any]:
    """A failed result."""
    return {
        **({"path": path} if path else {}),
        "id": id,
        "status": "failed",
        "mismatches": list(mismatches),
        **extra,
    }


def errored(error: Any, id: str | None = None, path: str | None = None) -> dict[str, Any]:
    """An error result."""
    out: dict[str, Any] = {}
    if path:
        out["path"] = path
    if id:
        out["id"] = id
    out["status"] = "error"
    out["error"] = error
    return out


def miss(key: str, expected: Any, actual: Any) -> dict[str, Any]:
    """A mismatch."""
    return {"key": key, "expected": expected, "actual": actual}


def vector(
    name: str,
    spec: list[str],
    description: str,
    document: Any,
    expected: Any,
    *,
    cases: Any = None,
    files: list[tuple[str, str]] | None = None,
    limits: dict[str, int] | None = None,
    cancel_after: int | None = None,
    ruleset_digest: bool = False,
    topic: str,
) -> dict[str, Any]:
    """A `test_run` vector, its header's `engine`, `bundle` and `files` derived from what is given.

    `engine.version` is a placeholder: runners set it to their own engine's.
    """
    expected = with_header(expected, document, cases, files)
    given: dict[str, Any] = {"kind": "test_run", "document": document}
    if cases is not None:
        given["cases"] = cases
    if files is not None:
        given["files"] = [{"path": p, "text": t} for p, t in files]
    given["limits"] = {"max_explained": 0, **(limits or {})}
    if cancel_after is not None:
        given["cancel_after"] = cancel_after
    if ruleset_digest:
        given["ruleset_digest"] = True
    return {
        "id": f"tests/{topic}/{name}",
        "spec": spec,
        "description": description,
        "given": given,
        "expect": {"test_run": expected},
    }


def with_header(expected: Any, document: Any, cases: Any, files: Any) -> Any:
    """A test run with `engine`, `bundle` and `files` after `ruleset_digest`."""
    if "bundle" in document:
        limits = document["bundle"]["engine"]["limits"]
        source: Any = {"id": document["bundle"]["id"], "revision": document["bundle"]["revision"]}
    else:
        limits = {key: DEFAULT_LIMITS[key] for key in LIMIT_KEYS}
        source = None
    listed = [{}] if cases is not None else [{"path": p, "digest": digest(t)} for p, t in files]
    out: dict[str, Any] = {}
    for key, value in expected.items():
        if key == "limits":
            out["engine"] = {**engine(ENGINE_PLACEHOLDER), "limits": limits}
            if source is not None:
                out["bundle"] = source
            out["files"] = listed
        out[key] = value
    return out


ENGINE_PLACEHOLDER = "0.0.0-this-engine"

# --- The example rules ----------------------------------------------------------------

EXAMPLE_RULES = load((VECTORS.parent.parent / "docs/examples/discounts.json").read_text())
EXAMPLE_CASES = (VECTORS.parent.parent / "docs/examples/cases/discounts.json").read_text()

# Three rules under rule_order, the second with an id.
ORDERED = ruleset(
    "rule_order",
    rule("a", cmp("gt", "x", 0), {"v": 1, "w": "a"}),
    rule("b", cmp("gt", "x", 1), {"v": 2}, id="B-2"),
    rule("c", cmp("gt", "y", 0), {"v": 3}),
)
FIRST = ruleset(
    "first",
    rule("a", cmp("gt", "x", 0), {"v": 1, "w": "a"}),
    rule("b", cmp("gt", "x", 1), {"v": 2}, id="B-2"),
    rule("c", cmp("gt", "y", 0), {"v": 3}),
)
STRICT = ruleset(
    "unique",
    rule("a", cmp("gt", "x", 1), {"v": 1}),
    rule("b", cmp("gt", "x", 2), {"v": 2}),
    strict=True,
)


def matching() -> list[Any]:
    """The vectors of *Matching*."""
    t = "matching"
    return [
        vector(
            "example",
            [MATCHING, RUNNING, "tests.md#identity"],
            "the example case file passes against the example rules: every key of each case"
            " matches",
            EXAMPLE_RULES,
            run(
                "priority",
                [
                    passed(i, "cases/discounts.json")
                    for i in (
                        "vip-gets-15",
                        "blocked-gets-nothing",
                        "newsletter-is-loyal",
                        "new-customer-gets-nothing",
                    )
                ],
                summary(4, 0, 0),
            ),
            files=[("cases/discounts.json", EXAMPLE_CASES)],
            topic=t,
        ),
        vector(
            "selected-in-selection-order",
            [MATCHING, TEST_RUNS],
            "selected matches the effective ids in selection order: another order fails, and"
            " what was given is listed",
            ORDERED,
            run(
                "rule_order",
                [
                    passed("in-order"),
                    failed("reversed", miss("selected", ["B-2", "a"], ["a", "B-2"])),
                    failed("by-name", miss("selected", ["a", "b"], ["a", "B-2"])),
                    passed("none"),
                    failed("not-a-rule", miss("selected", ["z"], [])),
                ],
                summary(2, 3, 0),
            ),
            cases=case_file(
                case("in-order", {"x": 2}, {"selected": ["a", "B-2"]}),
                case("reversed", {"x": 2}, {"selected": ["B-2", "a"]}),
                case("by-name", {"x": 2}, {"selected": ["a", "b"]}),
                case("none", {"x": 0, "y": 0}, {"selected": []}),
                case("not-a-rule", {"x": 0}, {"selected": ["z"]}),
            ),
            topic=t,
        ),
        vector(
            "output-keys-and-null",
            [MATCHING, TEST_RUNS],
            "output matches when the output has the expected keys with equal values, the others"
            " unchecked; null expects no rule selected; {} expects one",
            FIRST,
            run(
                "first",
                [
                    passed("subset"),
                    passed("all-keys"),
                    failed("wrong-value", miss("output", {"v": 2}, {"v": 1, "w": "a"})),
                    failed("missing-key", miss("output", {"z": None}, {"v": 1, "w": "a"})),
                    passed("null"),
                    failed("null-but-selected", miss("output", None, {"v": 1, "w": "a"})),
                    passed("empty"),
                    failed("empty-but-nothing", miss("output", {}, None)),
                ],
                summary(4, 4, 0),
            ),
            cases=case_file(
                case("subset", {"x": 1}, {"output": {"v": 1}}),
                case("all-keys", {"x": 1}, {"output": {"v": 1, "w": "a"}}),
                case("wrong-value", {"x": 1}, {"output": {"v": 2}}),
                case("missing-key", {"x": 1}, {"output": {"z": None}}),
                case("null", {}, {"output": None}),
                case("null-but-selected", {"x": 1}, {"output": None}),
                case("empty", {"x": 1}, {"output": {}}),
                case("empty-but-nothing", {}, {"output": {}}),
            ),
            topic=t,
        ),
        vector(
            "output-of-a-multi-hit-decision",
            [MATCHING, TEST_RUNS],
            "a decision with no single output matches no output, null included: the key fails"
            " with its reason and no actual",
            ORDERED,
            run(
                "rule_order",
                [
                    failed(
                        "object",
                        {
                            "key": "output",
                            "expected": {"v": 1},
                            "reason": "the decision has no single output (rule_order)",
                        },
                    ),
                    failed(
                        "null",
                        {
                            "key": "output",
                            "expected": None,
                            "reason": "the decision has no single output (rule_order)",
                        },
                    ),
                ],
                summary(0, 2, 0),
            ),
            cases=case_file(
                case("object", {"x": 1}, {"output": {"v": 1}}),
                case("null", {}, {"output": None}),
            ),
            topic=t,
        ),
        vector(
            "output-of-an-aggregate",
            [MATCHING],
            "under collect with an aggregate, output is the aggregate, null when nothing is"
            " selected; outputs are the selected rules' own",
            ruleset(
                "collect",
                rule("a", cmp("gt", "x", 0), {"v": 1}),
                rule("b", cmp("gt", "x", 1), {"v": 2}),
                aggregate="sum",
            ),
            run(
                "collect",
                [
                    passed("sum"),
                    passed("nothing"),
                    failed("not-the-sum", miss("output", {"v": 2}, {"v": 3})),
                ],
                summary(2, 1, 0),
                aggregate="sum",
            ),
            cases=case_file(
                case("sum", {"x": 2}, {"output": {"v": 3}, "outputs": [{"v": 1}, {"v": 2}]}),
                case("nothing", {"x": 0}, {"output": None, "outputs": []}),
                case("not-the-sum", {"x": 2}, {"output": {"v": 2}}),
            ),
            topic=t,
        ),
        vector(
            "outputs",
            [MATCHING, TEST_RUNS],
            "outputs matches item by item, each as an output; a different count fails, and what"
            " was given has every key",
            ORDERED,
            run(
                "rule_order",
                [
                    passed("both"),
                    failed(
                        "one-missing",
                        miss("outputs", [{"v": 1}], [{"v": 1, "w": "a"}, {"v": 2}]),
                    ),
                    failed(
                        "other-order",
                        miss("outputs", [{"v": 2}, {"v": 1}], [{"v": 1, "w": "a"}, {"v": 2}]),
                    ),
                    passed("none"),
                ],
                summary(2, 2, 0),
            ),
            cases=case_file(
                case("both", {"x": 2}, {"outputs": [{"w": "a"}, {"v": 2}]}),
                case("one-missing", {"x": 2}, {"outputs": [{"v": 1}]}),
                case("other-order", {"x": 2}, {"outputs": [{"v": 2}, {"v": 1}]}),
                case("none", {}, {"outputs": []}),
            ),
            topic=t,
        ),
        vector(
            "unknown",
            [MATCHING, RULES_IN, TEST_RUNS],
            "unknown matches the effective ids of the rules whose condition is unknown, as a set;"
            " given sorted by code point",
            FIRST,
            run(
                "first",
                [
                    passed("all-unknown"),
                    passed("none-unknown"),
                    failed("not-all", miss("unknown", ["a"], ["B-2", "a", "c"])),
                    failed("y-unknown", miss("unknown", [], ["c"])),
                ],
                summary(2, 2, 0),
            ),
            cases=case_file(
                case("all-unknown", {}, {"unknown": ["c", "B-2", "a"]}),
                case("none-unknown", {"x": 0, "y": 0}, {"unknown": []}),
                case("not-all", {}, {"unknown": ["a"]}),
                case("y-unknown", {"x": 0}, {"unknown": []}),
            ),
            topic=t,
        ),
        vector(
            "every-key-in-order",
            [MATCHING, TEST_RUNS],
            "a failure lists every key that does not match, in the order of the expectations"
            " table, whatever their order in the file",
            FIRST,
            run(
                "first",
                [
                    failed(
                        "all-wrong",
                        miss("selected", ["c"], ["a"]),
                        miss("output", {"v": 3}, {"v": 1, "w": "a"}),
                        miss("outputs", [{"v": 3}], [{"v": 1, "w": "a"}]),
                        miss("unknown", [], ["c"]),
                    )
                ],
                summary(0, 1, 0),
            ),
            cases=case_file(
                case(
                    "all-wrong",
                    {"x": 1},
                    {"unknown": [], "outputs": [{"v": 3}], "output": {"v": 3}, "selected": ["c"]},
                )
            ),
            topic=t,
        ),
        vector(
            "values",
            [VALUES],
            "expected values match by canonical bytes: an int is not a float, decimals keep their"
            " scale, floats their sign of zero, and nulls match whatever their reason",
            ruleset(
                "first",
                {
                    "name": "r",
                    "when": const(True),
                    "then": {
                        "i": const(1),
                        "d": const(dec("0.150")),
                        "f": field("f"),
                        "n": {"op": "div", "left": const(1), "right": const(0)},
                    },
                },
            ),
            run(
                "first",
                [
                    passed("exact"),
                    failed("int-is-not-float", miss("output", {"i": 1.0}, ACTUAL_VALUES)),
                    failed("scale", miss("output", {"d": dec("0.15")}, ACTUAL_VALUES)),
                    failed("zero-sign", miss("output", {"f": 0.0}, ACTUAL_VALUES)),
                ],
                summary(1, 3, 0),
            ),
            cases=case_file(
                case(
                    "exact",
                    {"f": -0.0},
                    {"output": {"i": 1, "d": dec("0.150"), "f": -0.0, "n": None}},
                ),
                case("int-is-not-float", {"f": -0.0}, {"output": {"i": 1.0}}),
                case("scale", {"f": -0.0}, {"output": {"d": dec("0.15")}}),
                case("zero-sign", {"f": -0.0}, {"output": {"f": 0.0}}),
            ),
            topic=t,
        ),
    ]


ACTUAL_VALUES = {"d": dec("0.150"), "f": -0.0, "i": 1, "n": None}


def errors() -> list[Any]:
    """The vectors of errors, expected or not."""
    t = "errors"
    return [
        vector(
            "unexpected",
            [MATCHING, RUNNING, TEST_RUNS],
            "evaluation raised an error the case does not expect: the result is error, with the"
            " error, and the run goes on",
            STRICT,
            run(
                "unique",
                [
                    errored({"code": "missing_field", "rule": "a", "path": ["x"]}, id="missing"),
                    passed("next"),
                ],
                summary(1, 0, 1),
                strict=True,
            ),
            cases=case_file(
                case("missing", {}, {"selected": []}),
                case("next", {"x": 2}, {"selected": ["a"]}),
            ),
            topic=t,
        ),
        vector(
            "expected",
            [MATCHING, "tests.md#errors"],
            "an error with the expected code passes; another code fails the error key, with the"
            " error given",
            STRICT,
            run(
                "unique",
                [
                    passed("missing-field"),
                    passed("both-fire"),
                    failed(
                        "other-code",
                        miss(
                            "error",
                            {"code": "type_mismatch"},
                            {"code": "missing_field", "rule": "a", "path": ["x"]},
                        ),
                    ),
                ],
                summary(2, 1, 0),
                strict=True,
            ),
            cases=case_file(
                case("missing-field", {}, {"error": {"code": "missing_field"}}),
                case("both-fire", {"x": 3}, {"error": {"code": "hit_policy_error"}}),
                case("other-code", {}, {"error": {"code": "type_mismatch"}}),
            ),
            topic=t,
        ),
        vector(
            "expected-but-none",
            [MATCHING, EXPLAINING],
            "an expected error that evaluation does not raise fails the error key with null, and"
            " the decision is explained",
            STRICT,
            run(
                "unique",
                [
                    failed(
                        "no-error",
                        miss("error", {"code": "hit_policy_error"}, None),
                        explanation_text=(
                            "HIT POLICY unique: selected a\n"
                            "[x] a (selected): x > 1\n"
                            "    - x > 1 is true (x = 2)\n"
                            "[ ] b (not fired): x > 2\n"
                            "    - x > 2 is false (x = 2); at the limit (needs x > 2)"
                        ),
                    )
                ],
                summary(0, 1, 0),
                {"max_explained": 5},
                strict=True,
            ),
            cases=case_file(case("no-error", {"x": 2}, {"error": {"code": "hit_policy_error"}})),
            limits={"max_explained": 5},
            topic=t,
        ),
        vector(
            "policy-error-is-a-result",
            [MATCHING],
            "hit_policy_error under unique is a case's error, naming the rules that fired",
            ruleset("unique", rule("a", cmp("gt", "x", 0)), rule("b", cmp("gt", "x", 1))),
            run(
                "unique",
                [errored({"code": "hit_policy_error", "rules": ["a", "b"]}, id="both")],
                summary(0, 0, 1),
            ),
            cases=case_file(case("both", {"x": 2})),
            topic=t,
        ),
    ]


SIMPLE = ruleset("first", rule("a", cmp("gt", "x", 0), {"v": 1}))
GOOD = text(case_file(case("one", {"x": 1}, {"selected": ["a"]}), case("two", {"x": 0})))
OTHER = text(case_file(case("one", {"x": 0}, {"selected": []})))


def files() -> list[Any]:
    """The vectors of case files."""
    t = "files"
    return [
        vector(
            "in-order-with-paths",
            [RUNNING, "tests.md#identity", TEST_RUNS],
            "case files run in the order given, each case with its file's path; two files may"
            " hold the same id, and an empty file gives no result",
            SIMPLE,
            run(
                "first",
                [
                    passed("one", "cases/a.json"),
                    passed("two", "cases/a.json"),
                    passed("one", "cases/b.json"),
                ],
                summary(3, 0, 0),
            ),
            files=[
                ("cases/a.json", GOOD),
                ("cases/empty.json", text(case_file())),
                ("cases/b.json", OTHER),
            ],
            topic=t,
        ),
        vector(
            "unreadable",
            [RUNNING, TEST_RUNS],
            "a file that cannot be read as a case file is one error with its path and no id, and"
            " the run goes on: invalid JSON, another document, a repeated id, another version",
            SIMPLE,
            run(
                "first",
                [
                    errored(
                        {"code": "format_error", "line": 1, "column": 2}, path="cases/syntax.json"
                    ),
                    errored({"code": "format_error", "pointer": ""}, path="cases/rules.json"),
                    errored(
                        {"code": "format_error", "pointer": "/cases/items/1/id"},
                        path="cases/twice.json",
                    ),
                    passed("one", "cases/good.json"),
                    passed("two", "cases/good.json"),
                    errored(
                        {"code": "unsupported_version", "pointer": "/version"},
                        path="cases/version.json",
                    ),
                ],
                summary(2, 0, 4),
            ),
            files=[
                ("cases/syntax.json", "{"),
                ("cases/rules.json", text(SIMPLE)),
                ("cases/twice.json", text(case_file(case("a", {}), case("a", {})))),
                ("cases/good.json", GOOD),
                ("cases/version.json", '{"version": 2, "cases": {"items": []}}'),
            ],
            topic=t,
        ),
        vector(
            "repeated-path",
            [RUNNING, "tests.md#identity"],
            "a case is its path and its id in a run: a file whose path repeats an earlier"
            " file's is one format_error, without an id, and none of its cases runs",
            SIMPLE,
            run(
                "first",
                [
                    passed("one", "cases/a.json"),
                    passed("two", "cases/a.json"),
                    errored({"code": "format_error"}, path="cases/a.json"),
                    passed("one", "cases/b.json"),
                ],
                summary(3, 0, 1),
            ),
            files=[("cases/a.json", GOOD), ("cases/a.json", OTHER), ("cases/b.json", OTHER)],
            topic=t,
        ),
        vector(
            "decision-table",
            [RUNNING, MATCHING, TEST_RUNS],
            "a decision table is tested through the rule set it compiles to; the run names the"
            " table's own policy, here in the reason of a missing single output",
            {
                "version": 1,
                "decision_table": {
                    "hit_policy": "collect",
                    "logic": "boolean",
                    "inputs": ["x"],
                    "outputs": [{"name": "v"}],
                    "rows": [
                        {"name": "low", "inputs": ["> 0"], "outputs": ["1"]},
                        {"inputs": ["> 1"], "outputs": ["2"]},
                    ],
                },
            },
            run(
                "collect",
                [
                    passed("both"),
                    failed(
                        "single",
                        {
                            "key": "output",
                            "expected": {"v": 1},
                            "reason": "the decision has no single output (collect)",
                        },
                    ),
                ],
                summary(1, 1, 0),
                source="decision_table",
            ),
            cases=case_file(
                case(
                    "both",
                    {"x": 2},
                    {"selected": ["low", "row_2"], "outputs": [{"v": 1}, {"v": 2}]},
                ),
                case("single", {"x": 1}, {"output": {"v": 1}}),
            ),
            topic=t,
        ),
    ]


def bundles() -> list[Any]:
    """The vectors of a bundle's case files."""
    t = "bundles"
    table, _ = table_basic()
    good = text(
        case_file(
            case(
                "vip",
                {"customer": {"years": 7}, "order": {"total": 2000}},
                {"selected": ["vip"], "output": {"discount": dec("0.15"), "label": "VIP"}},
            )
        )
    )
    other = text(case_file(case("new", {"customer": {"years": 1}}, {"output": None})))
    refs = [
        {"path": "cases/good.json", "digest": digest(good)},
        {"path": "cases/other.json", "digest": digest(other)},
    ]
    document = bundle(table=table["decision_table"], cases=refs)
    stated = document["bundle"]["ruleset_digest"]
    return [
        vector(
            "case-files",
            [BUNDLES, RUNNING, TEST_RUNS],
            "a bundle's case files run in its order, each checked against its digest and its"
            " cases named by path and id; asked for, the run gives the rule set digest",
            document,
            run(
                "first",
                [passed("vip", "cases/good.json"), passed("new", "cases/other.json")],
                summary(2, 0, 0),
                source="decision_table",
                ruleset_digest=stated,
            ),
            files=[("cases/good.json", good), ("cases/other.json", other)],
            ruleset_digest=True,
            topic=t,
        ),
        vector(
            "digest-mismatch",
            [BUNDLES],
            "a case file whose bytes are not the ones the bundle records is a decision_mismatch at"
            " its digest, and none of its cases runs",
            document,
            run(
                "first",
                [
                    passed("vip", "cases/good.json"),
                    errored(
                        {"code": "decision_mismatch", "pointer": "/bundle/cases/1/digest"},
                        path="cases/other.json",
                    ),
                ],
                summary(1, 0, 1),
                source="decision_table",
            ),
            files=[("cases/good.json", good), ("cases/other.json", other + "\n")],
            topic=t,
        ),
        vector(
            "not-a-case-file",
            [BUNDLES, RUNNING],
            "a bundle's file whose digest matches but which is not a case file is one error,"
            " the error of reading it",
            bundle(
                table=table["decision_table"],
                cases=[{"path": "cases/rules.json", "digest": digest(text(SIMPLE))}],
            ),
            run(
                "first",
                [errored({"code": "format_error", "pointer": ""}, path="cases/rules.json")],
                summary(0, 0, 1),
                source="decision_table",
            ),
            files=[("cases/rules.json", text(SIMPLE))],
            topic=t,
        ),
        vector(
            "recorded-limits",
            [BUNDLES, TEST_RUNS],
            "a bundle's case files are read under the limits the bundle records: a file larger"
            " than its max_size is a limit_exceeded, and the run records those limits",
            bundle(
                table=table["decision_table"],
                cases=[{"path": "cases/good.json", "digest": digest(good)}],
                engine_record=engine(max_size=64),
            ),
            run(
                "first",
                [errored({"code": "limit_exceeded"}, path="cases/good.json")],
                summary(0, 0, 1),
                source="decision_table",
            ),
            files=[("cases/good.json", good)],
            topic=t,
        ),
    ]


# Five results: two cases (two rules each), an unreadable file, two cases.
FIVE = [
    ("one.json", text(case_file(case("a", {"x": 1}), case("b", {"x": 0})))),
    ("bad.json", "[]"),
    ("two.json", text(case_file(case("c", {"x": 1}), case("d", {"x": 1}, {"selected": []})))),
]
TWO_RULES = ruleset("first", rule("a", cmp("gt", "x", 0)), rule("b", cmp("gt", "x", 1)))
FIVE_RESULTS = [
    passed("a", "one.json"),
    passed("b", "one.json"),
    errored({"code": "format_error", "pointer": ""}, path="bad.json"),
    passed("c", "two.json"),
    failed("d", miss("selected", [], ["a"]), path="two.json"),
]


def interruption() -> list[Any]:
    """The vectors of interrupted runs."""
    t = "interruption"
    return [
        vector(
            "max-results",
            [BATCHES, TEST_RUNS],
            "max_results stops the run after that many results: a prefix of the full run, partial,"
            " with the results not given",
            TWO_RULES,
            run(
                "first", FIVE_RESULTS[:2], summary(2, 0, 0, ("max_results", 3)), {"max_results": 2}
            ),
            files=FIVE,
            limits={"max_results": 2},
            topic=t,
        ),
        vector(
            "max-steps",
            [BATCHES],
            "each case costs one step per rule and an unreadable file none: with 4 steps, two"
            " cases and the file run, and the next case would go past the limit",
            TWO_RULES,
            run("first", FIVE_RESULTS[:3], summary(2, 0, 1, ("max_steps", 2)), {"max_steps": 4}),
            files=FIVE,
            limits={"max_steps": 4},
            topic=t,
        ),
        vector(
            "max-steps-zero",
            [BATCHES],
            "with no step, no case runs, and the run is partial with every result remaining",
            TWO_RULES,
            run("first", [], summary(0, 0, 0, ("max_steps", 5)), {"max_steps": 0}),
            files=FIVE,
            limits={"max_steps": 0},
            topic=t,
        ),
        vector(
            "limits-in-order",
            [BATCHES],
            "max_results is checked before max_steps",
            TWO_RULES,
            run(
                "first",
                FIVE_RESULTS[:1],
                summary(1, 0, 0, ("max_results", 4)),
                {"max_results": 1, "max_steps": 2},
            ),
            files=FIVE,
            limits={"max_results": 1, "max_steps": 2},
            topic=t,
        ),
        vector(
            "cancelled",
            [BATCHES],
            "the caller's callback, asked before each result, cancels the run after three",
            TWO_RULES,
            run("first", FIVE_RESULTS[:3], summary(2, 0, 1, ("cancelled", 2))),
            files=FIVE,
            cancel_after=3,
            topic=t,
        ),
        vector(
            "complete-at-the-limit",
            [BATCHES],
            "a run that has given its last result is complete, never partial, even at its limits"
            " and with a callback that would cancel",
            TWO_RULES,
            run("first", FIVE_RESULTS, summary(3, 1, 1), {"max_results": 5, "max_steps": 8}),
            files=FIVE,
            limits={"max_results": 5, "max_steps": 8},
            cancel_after=5,
            topic=t,
        ),
        vector(
            "explained-up-to-the-limit",
            [EXPLAINING],
            "only the first max_explained failures are explained, in run order",
            SIMPLE,
            run(
                "first",
                [
                    failed(
                        "first",
                        miss("selected", [], ["a"]),
                        explanation_text=(
                            "HIT POLICY first: selected a\n"
                            "[x] a (selected): x > 0\n"
                            "    - x > 0 is true (x = 1)"
                        ),
                    ),
                    failed("second", miss("selected", [], ["a"])),
                ],
                summary(0, 2, 0),
                {"max_explained": 1},
            ),
            cases=case_file(
                case("first", {"x": 1}, {"selected": []}),
                case("second", {"x": 2}, {"selected": []}),
            ),
            limits={"max_explained": 1},
            topic=t,
        ),
    ]


FILES = {
    "matching.json": matching,
    "errors.json": errors,
    "files.json": files,
    "bundles.json": bundles,
    "interruption.json": interruption,
}


def numbers(value: Any) -> Any:
    """Python values to the literals `dump` writes: ints and floats kept exact."""
    if isinstance(value, bool) or value is None or isinstance(value, str):
        return value
    if isinstance(value, int):
        return IntLit(str(value))
    if isinstance(value, float):
        return load(json.dumps(value))
    if isinstance(value, list):
        return [numbers(v) for v in value]
    return {k: numbers(v) for k, v in value.items()}


def build(name: str) -> str:
    """A vector file's text."""
    vectors = [numbers(v) for v in FILES[name]()]
    return dump({"version": IntLit("1"), "area": "tests", "vectors": vectors}) + "\n"


def main() -> int:
    """Write or check the files; return the exit code."""
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    folder = VECTORS / "tests"
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
