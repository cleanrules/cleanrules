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
`evaluate_value_tree`).

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
