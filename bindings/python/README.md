# cleanrules for Python

The Python binding of [cleanrules](https://github.com/cleanrules/cleanrules), an
explainable rules engine with one Rust core and the same semantics in Rust,
C/C++, Python, JavaScript, Java/Scala, Go and Erlang.

> **Pre-alpha.** Nothing is published yet. Input data can be dicts, lists,
> dataclasses, Pydantic models, namedtuples, pandas rows, numpy values and other
> objects: only the fields the rules read are read. Rules can carry actions,
> run by `RuleSet.run`, and are read from JSON, YAML, plain text or CSV decision
> tables.

```python
from decimal import Decimal
from cleanrules import F, Rule, RuleSet

rules = RuleSet(
    [
        Rule(
            "vip",
            (F.customer.years > 5) & (F.order.total > 1000),
            {"discount": Decimal("0.15")},
            priority=10,
        ),
        Rule(
            "loyal",
            (F.customer.years > 2) | F.customer.newsletter,
            {"discount": Decimal("0.05")},
            priority=5,
        ),
        Rule("blocked", F.customer.blocked, {"discount": 0}, priority=99),
    ],
    hit_policy="priority",
)
decision = rules.evaluate({"customer": {"years": 3, "newsletter": False}, "order": {"total": 800}})

decision.output
# {'discount': Decimal('0.05')}
[(r.rule.name, r.truth.value) for r in decision.results]
# [('vip', 'false'), ('loyal', 'true'), ('blocked', 'unknown')]

why = decision.why_not("vip")
why.status
# 'not_fired'
[(c.counterfactual.path, c.counterfactual.gap) for c in why.causes]
# [(('customer', 'years'), 2), (('order', 'total'), 200)]: short by 2 and by 200

print(decision.explain())
```

```text
HIT POLICY priority: selected loyal
[ ] vip (not fired): customer.years > 5 AND order.total > 1000
    would fire if all of these changed:
    - customer.years > 5 is false (customer.years = 3); short by 2
    - order.total > 1000 is false (order.total = 800); short by 200
[x] loyal (selected): customer.years > 2 OR customer.newsletter
    - customer.years > 2 is true (customer.years = 3)
[?] blocked (unknown): customer.blocked
    - customer.blocked is unknown: missing field customer.blocked
```

The text is the same in every language, and writes conditions in the text
syntax below; `str()` of a condition gives that text too, and `repr()` the Python
that builds it. `print(rules.summary())` describes the rule set without
evaluating it: its rules, the fields they read, their outputs and tags.

A decision can be kept for audits and explained later without the input:
`json.dumps(decision.to_dict())` records it, and `rules.read_decision(text)`
reads it back, checked node by node against the rule set that made it (a
decision from another rule set is refused with `decision_mismatch`).

`blocked` is unknown, not false: the input has no `customer.blocked`, and
cleanrules uses three-valued logic instead of guessing. Pass `strict=True` to
`RuleSet` to raise a `MissingFieldError` instead. Conditions are data, so some
Python idioms do not work on them; see the [common pitfalls](../../docs/pitfalls.md).

Rules are data, so they can be written to and read from JSON or YAML files,
which every language reads the same way:

```python
import cleanrules

cleanrules.dump(rules, "pricing.yaml")
rules = cleanrules.load("pricing.yaml")
```

They can also be written as text, the way people write rules on paper; errors
give the line and column:

```python
rules = cleanrules.loads(
    """
    HIT POLICY priority

    RULE vip PRIORITY 10
    IF customer.years > 5 AND order.total > 1000
    THEN discount = 0.15

    RULE loyal PRIORITY 5
    IF customer.years > 2 OR customer.newsletter
    THEN discount = 0.05
    """,
    format="text",
)
vip = cleanrules.rule_from_text("RULE vip IF customer.years > 5 THEN discount = 0.15")
print(cleanrules.dumps(rules["loyal"], format="text"))
# RULE loyal PRIORITY 5
# IF customer.years > 2 OR customer.newsletter
# THEN discount = 0.05
```

A rule can carry an `id`, which identifies it across versions of its rule set;
without one, its name does (`rule.effective_id`). Effective ids are unique
within a rule set, as names are, and the id changes nothing in evaluation:
decisions, explanations and summaries name rules by name. Renaming a rule
without an id makes it another rule when two versions are compared (comparison
arrives in a later release), so give a renamed rule its old name as id, or give
rules whose names may change an id from the start, such as a ticket:

```python
gold = Rule("gold_customer", F.vip, id="vip")  # was Rule("vip", F.vip)
gold.id, gold.effective_id
# ('vip', 'vip')
print(cleanrules.dumps(gold, format="text"))
# RULE gold_customer ID vip
# IF vip
```

Each document has one canonical form as bytes, and a digest of them that
identifies that exact artifact, whatever its layout, member order or format
([`spec/json-format.md`](../../spec/json-format.md), *Canonical bytes* and
*Digests*):

```python
cleanrules.canonical_bytes(rules)  # b'{"ruleset":{"hit_policy":"priority",...},"version":1}'
cleanrules.digest(rules)  # 'sha256:...', the rule set digest
cleanrules.digest(table.ruleset)  # a decision table's rule set digest
```

A digest does not say that two rule sets decide alike: `x > 5` and `5 < x`, or
the decimals `1.0` and `1.00`, give different digests. A decision document
that carries `ruleset_digest` is checked against it by `read_decision` and
keeps it; `cleanrules.dumps(decision, ruleset_digest=True)` (or
`decision.to_dict(ruleset_digest=True)`) writes one, so that a log of
decisions names the exact rules that made them (it is read back with
`rules.read_decision`, not `loads`, since a decision is read with its rule
set). Actions given as code are not
part of a document: the digest that decisions of such rules are checked
against is `digest(rules, omit_actions=True)`.

### Bundles

A bundle is a reproducible package of rules
([`spec/bundles.md`](../../spec/bundles.md)): the rule set or decision table,
an identifier and a revision that you give them, their rule set digest, the
engine version and limits that made it, the contract of their input and
output, and the digest of each file of test cases. It is a JSON or YAML
document, and two bundles compare to tell a change of engine from a change of
rules:

```python
from pathlib import Path

bundle = cleanrules.Bundle(
    rules,
    id="acme/discounts",
    revision="2026-11-16.1",
    cases=[("cases/discounts.json", Path("cases/discounts.json").read_bytes())],
)
cleanrules.dump(bundle, "discounts.bundle.json")
bundle.ruleset_digest  # 'sha256:...', as cleanrules.digest(rules)
bundle.engine.version  # cleanrules.__version__
bundle.contract["output"]  # {'keys': ['discount']}

old = cleanrules.load("discounts-1.bundle.json")
comparison = old.compare(bundle)
comparison.relation  # e.g. 'other_rules_same_engine': the rules changed
comparison.differences  # e.g. (BundleDifference.REVISION, BundleDifference.RULES)
bundle.engine.limits.max_depth  # 256
bundle.case_digest_matches("cases/discounts.json", Path("cases/discounts.json").read_bytes())
```

A bundle holds no code: rules with actions given as code are refused unless
you pass `omit_actions=True`, and named actions come back unbound. Reading a
bundle checks its `ruleset_digest`, and, when this engine and version made it,
its contract (`FormatError` with the code `decision_mismatch`); a bundle made
by another version is read as it was recorded.

A case file is a document of test cases
([`spec/tests.md`](../../spec/tests.md),
[`docs/examples/cases/discounts.json`](../../docs/examples/cases/discounts.json)):
each case has an id, an input, and what evaluating it must give (the
selected rules, the output, the rules left unknown, or the error). The bundle
records the digest of its bytes; *Test cases*, below, runs them.

### Test cases

A case file loads as `Cases` (`load`, `loads`; `dump`, `dumps`,
`canonical_bytes` and `digest` take it), and `RuleSet.test` runs case files
against the rules ([`spec/tests.md`](../../spec/tests.md), *Running cases*):

```python
from decimal import Decimal
from cleanrules import Case, CaseFile, Cases, Expectation, ExpectedOutput

rules = cleanrules.load("docs/examples/discounts.json")
run = rules.test(["docs/examples/cases/discounts.json"])  # paths are read from disk
run.all_passed  # True: every case ran and passed
print(run)  # the core's text: each failure, explained, and the summary

cases = Cases(
    [
        Case(
            "vip-gets-15",
            {"customer": {"years": 7}, "order": {"total": 1500}},
            expect=Expectation(selected=["vip"], output={"discount": Decimal("0.15")}),
        ),
        Case(
            "nobody",
            {"customer": {"years": 1}},
            expect=Expectation(output=ExpectedOutput.NOTHING_SELECTED),
        ),
    ],
    description="discounts",
)
cleanrules.dumps(cases)  # {"version":1,"cases":{"description":"discounts","items":[...]}}

for result in rules.iter_test([CaseFile(cases, path="cases/vip.json")], max_results=100):
    print(result.status, result.id, [m.key for m in result.mismatches])
```

Three names, the same in every language: a `CaseFileRef` is a bundle's
reference to a case file; a `Cases` is the content of a case file; a
`CaseFile` is one file given to a run, its `path`, the `digest` of the bytes
it was read from, and its `cases` or the `error` that keeps it from being read
(`CaseFile(cases, path=...)`, `CaseFile.read(content, path=..., format=...)`,
which reads YAML when the path ends in `.yaml` or `.yml`, and
`CaseFile.load(path)`). The constructors of `Expectation`, `Case` and `Cases`
go through the core, which refuses what a case file's reader refuses
(`FormatError`). Their data (`input`, `output`, `outputs`, `metadata`, a
mismatch's `expected` and `actual`) is a fresh copy on each access.

`test()` gives a `TestRun` (immutable): `summary` (`total`, `passed`,
`failed`, `error`, `partial`, `interrupted`), `results` in run order
(`CaseResult`: `path`, `id`, `status`, `mismatches`, `error`,
`explanation_text`), the header (`source`, `hit_policy`, `engine`, `files`,
`bundle`, `limits`, `ruleset_digest` when asked for), `all_passed`,
`to_dict()` (the test run document) and `str()` (the core's text). A case
never stops the run: an error is that case's result, and a file that cannot
be read is one `error` result. `max_results` and `max_steps` (one step per
rule for each case) stop the run, partial; `max_explained` (the core's 5 when
left out) bounds the failures explained; `cancel`, a callable asked between
results (never while a case is evaluated: `threading.Event().is_set`, a
deadline check), stops it too, and so does Ctrl-C during `test()`. A
mismatch of `output` says `ExpectedOutput.NOTHING_SELECTED` where nothing
was selected, and `None` only where nothing was given (with its `reason`).
`iter_test()` gives the same run one result at a time (`CaseRun`, an
iterator with `cancel()`, `summary`, `remaining`, `is_finished` and
`test_run()`).

| Leaving the loop early | Python | JavaScript |
|---|---|---|
| `break` | leaves the run where it is: it can go on, and is partial once `test_run()` or `cancel()` stops it | calls `return()`, which stops the run: it is partial (`cancelled`) |
| `test_run()` / `testRun()` | stops the run, then gives it partial | gives the partial run |

The `cancel` callable must not read its own run: there, `summary`,
`remaining`, `is_finished`, `test_run()` and `next()` raise `TypeError`, and
`run.cancel()` asks the run to stop, as returning `True` does.
`bundle.test(directory_or_reader)` runs a bundle's own case files: the core
picks how many bytes to load (a file in the directory larger than that is a
`limit_exceeded` result, not read), checks each against its digest
(`decision_mismatch` otherwise) and reads it under the bundle's limits. The
GIL is released while each case is evaluated. `CaseStatus` is closed;
`ExpectationKey` and `Interruption` may gain members, which arrive as plain
strings. Comparing two versions of the rules on the same cases comes with
task T.4.

### Auditing rules

`rules.audit()` (and `table.audit()`) examines a rule set or a decision table
without any input ([`spec/audit.md`](../../spec/audit.md)) and reports tests
whose types never match, conditions that are never true, rules that are never
selected, rules that fire together and inputs for which no rule fires. Each
finding that something can happen comes with a witness, an input the rules
themselves have evaluated:

```python
rules = RuleSet(
    [
        Rule("adult", F.age >= 18, {"band": "adult"}),
        Rule("senior", F.age >= 65, {"band": "senior"}),
    ],
    hit_policy="unique",
)
report = rules.audit()
report.max_severity  # Severity.ERROR
report.at_least(Severity.WARNING)  # True: compare severities this way
overlap = report.diagnostics[0]
overlap.code, overlap.rules  # (DiagnosticCode.OVERLAP, ('adult', 'senior'))
rules.evaluate(overlap.witness)  # raises HitPolicyError: both rules fire
print(report)  # the text, the same in every language
report.to_dict()  # {"version": 1, "audit": {...}}, witnesses in JSON
```

The audit is sound but incomplete: every diagnostic is true, and what it
could not decide is listed (`report.undecided`, `report.not_modelled`, and
each check's status in `report.checks`). It is bounded by `max_terms` and
`max_steps`, and `ruleset_digest=True` carries the rule set digest in the
report. A diagnostic has a `code`, a `severity` (`Severity`: `INFO`,
`WARNING` or `ERROR`, which takes the hit policy into account: an overlap is an
error under `unique` and a fact under `first`; the core orders them so, while
the members compare as strings, so ask `report.at_least(severity)`), the `rules` it names, a
`message`, and, where it applies, a `pointer` into the rules' document, the
field's `path` and `groups`, and the `witness` as Python data; `str()` of a
diagnostic is its text in the audit's text (the core's). Codes, checks,
reasons and limits come as `StrEnum` members (`DiagnosticCode`,
`AuditCheck`, `UndecidedReason`, `AuditLimit`); a later version may add
codes, which arrive as plain strings, so compare with `==` and keep the ones
you do not know. Reports are immutable, and output only: `dumps` refuses them
(use `json.dumps(report.to_dict())` or `str(report)`). The GIL is released
while the core audits.

`evaluate` never has side effects; `run` evaluates and then calls the actions
of the selected rules. An action is a function given to `Rule(do=...)`, or a
name that a rule file gives (`DO notify_sales` in text, `"do"` in JSON) and the
program resolves in a registry of its own. Names are looked up only there,
never imported, so a file can only call what the program registered
([`docs/examples/discounts.rules`](../../docs/examples/discounts.rules)):

```python
def notify_sales(context: cleanrules.ActionContext) -> None:
    print(f"{context.rule.name}: {dict(context.output)}")


rules = cleanrules.load("docs/examples/discounts.rules", actions={"notify_sales": notify_sales})
rules.run({"customer": {"years": 9}, "order": {"total": 2000}})
# vip: {'discount': Decimal('0.15')}
```

Unbound, the rules still evaluate, explain and write; `run` raises a
`RegistryError`, as `rules.bind(actions)` does for a name the registry lacks.

Prices stay exact: arithmetic on `Decimal` values and ints gives a `Decimal`
(a Python float makes it a float), and `round()` gives it a number of places,
rounded once on the exact value (`spec/ast.md`, *Rounding*):

```python
price = round(F.order.total * (1 - F.discount), 2)  # or .round(2, "half_up")
rules = RuleSet([Rule("vip", F.customer.years > 5, {"discount": Decimal("0.15"), "price": price})])
rules.evaluate(
    {"customer": {"years": 9}, "order": {"total": Decimal("19.99")}, "discount": Decimal("0.15")}
).output
# {'discount': Decimal('0.15'), 'price': Decimal('16.99')}
```

Rules can also be a decision table, read from CSV as spreadsheets write it (or
from JSON or YAML). Cells use the same syntax, without their column; output
values in priority order rank the rows, as in DMN. This is
[`docs/examples/risk.csv`](../../docs/examples/risk.csv):

```text
HIT POLICY,priority
RULE,customer.years,order.total,THEN discount,"THEN risk IN ('high', 'medium', 'low')",DESCRIPTION
new,< 1,> 1000,0,'high',New customers with large orders are checked first
vip,> 5,-,0.15,'low',Long-standing customers
big,-,> 5000,0.05,'medium',
default,-,-,0,'low',
```

```python
table = cleanrules.load("docs/examples/risk.csv")
table.evaluate({"customer": {"years": 0}, "order": {"total": 2000}}).output
# {'discount': 0, 'risk': 'high'}
table.ruleset  # the RuleSet it compiles into: one rule per row
```

A table can also be built from rows, or from a pandas or polars `DataFrame`
whose column names are the header. Cells are read as in the CSV file: a `str`
is the text syntax (`"> 5"`, `"-"`, and `"'VIP'"` for the string, since `"VIP"`
is the field named `VIP`), any other value is a constant, and `None` is an
empty cell
([`docs/examples/risk_table.py`](../../docs/examples/risk_table.py)):

```python
from cleanrules import DecisionTable

table = DecisionTable(
    ["RULE", "customer.years", "order.total", "THEN discount", "THEN label"],
    [
        ["vip", "> 5", "> 1000", 0.15, "'VIP'"],
        ["loyal", "> 2", None, 0.05, "'loyal'"],
    ],
)
table = DecisionTable.from_dataframe(frame, hit_policy="priority")
```

To evaluate one condition and see every step, use `evaluate_tree`:

```python
from cleanrules import evaluate_tree

tree = evaluate_tree((F.age >= 18) & F.country.is_in(["ES"]), {"age": 20})
tree.truth  # Truth.UNKNOWN
tree.children[1].reason.code  # 'missing_field': there is no country
```

The binding reads only the paths the rules read, and converts what it finds
there. Data that shares objects (the same list or object referenced from
several places) is converted once per reference, so the values read from one
input are bounded by `max_values`, 1,000,000 by default (a list along the
paths counts its length): an input beyond it is refused, not evaluated, in
every mode, with a `FormatError` whose code is `limit_exceeded` and whose
`name` is `max_values` (`row` in a batch). Rule metadata is converted under
the default limit too, naming the rule. Raise it
where large inputs are expected: `rules.evaluate(data, max_values=5_000_000)`
(also `evaluate_many`, `run`, `DecisionTable.evaluate`, `evaluate_tree` and
`evaluate_value_tree`). A field path has at most `max_path_segments` segments,
256 by default, so the data read along it stays shallow: `F` and `Field` check
the default, and `loads`, `load`, `rule_from_text` and `DecisionTable` take
`max_path_segments=` (beyond it, a `FormatError` with code `limit_exceeded`
and `name` `max_path_segments`). Raise it only for trusted rules:
`cleanrules.load("deep.rules", max_path_segments=1024)`.

For AI agents, a `cleanrules.Catalog` of rule sets by name answers the tools
of [`spec/agents.md`](../../spec/agents.md) (`list_rulesets`, `summary`,
`evaluate`, `explain`, `validate`), with JSON arguments and a JSON result that
are the same in every language and in the MCP server. Tools never run actions,
and a mistake comes back as a tool error with a pointer and a hint that a model
can act on. Arguments may be JSON text, or Python data whose `Decimal`, `date`
and `datetime` values are written in the JSON format's encoding:

```python
catalog = cleanrules.Catalog({"discounts": rules})
arguments = {"ruleset": "discounts", "input": {"customer": {"years": 3}}, "budget": 600}
result = catalog.call(cleanrules.Tool.EXPLAIN, arguments)
result.is_error, result.content["selected"], result.content["explanation"]["text"]
```

`rules.input_schema()` is the JSON Schema of the input a rule set reads, and
`decision.explain().compact(budget=2000)` the explanation's text cut to fit a
model's context. A runnable example is in
[`docs/examples/agent_tools.py`](../../docs/examples/agent_tools.py).

Wheels: one `abi3` wheel for CPython 3.11 and later, a version-specific wheel for
free-threaded CPython 3.14, and one `abi3t` wheel for CPython 3.15 and later,
free-threaded or not.

## Development

```bash
cd bindings/python
uv run pytest          # builds the extension with maturin, then runs the tests
uv run pyright
```

Apache License 2.0, created by Jose L. Salmeron: redistributions keep the
[NOTICE](https://github.com/cleanrules/cleanrules/blob/main/NOTICE) with its
attribution of authorship.
