# cleanrules for Python

The Python binding of [cleanrules](https://github.com/cleanrules/cleanrules), an
explainable rules engine with one Rust core and the same semantics in Rust,
C/C++, Python, JavaScript, Java/Scala, Go and Erlang.

> **Pre-alpha.** Nothing is published yet. Input data can be dicts, lists,
> dataclasses, Pydantic models, namedtuples, pandas rows, numpy values and other
> objects: only the fields the rules read are read. Rules can carry actions,
> run by `RuleSet.run`. Loading rules from files comes next in the roadmap.

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
```

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
