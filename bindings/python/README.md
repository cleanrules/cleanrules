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

To evaluate one condition and see every step, use `evaluate_tree`:

```python
from cleanrules import evaluate_tree

tree = evaluate_tree((F.age >= 18) & F.country.is_in(["ES"]), {"age": 20})
tree.truth  # Truth.UNKNOWN
tree.children[1].reason.code  # 'missing_field': there is no country
```

Wheels: one `abi3` wheel for CPython 3.11 and later, a version-specific wheel for
free-threaded CPython 3.14, and one `abi3t` wheel for CPython 3.15 and later,
free-threaded or not.

## Development

```bash
cd bindings/python
uv run pytest          # builds the extension with maturin, then runs the tests
uv run pyright
```

MIT licensed.
