# RulePy

A rules engine for Python with one core for four jobs:

- **Business rules**: stateless decisions with hit policies and decision tables.
- **Expert systems**: forward chaining over a working memory, with truth
  maintenance.
- **Data validation**: the same rules, vectorized over DataFrames.
- **ML explanations**: rules extracted from models, plus guardrails around them.

Conditions are data, not opaque functions. One rule can be serialized to YAML,
explained clause by clause, compiled for polars, audited for contradictions and
dead rules, and run by an optional Rust core. Crisp and fuzzy logic share the
engine; fuzzy support comes from [fuzzytool](https://fuzzytool.github.io).

!!! warning "Pre-alpha"
    Nothing is published yet and the API below is the planned one. It will
    change.

```python
from rulepy import F, Rule, RuleSet

vip = Rule(
    "vip",
    when=(F.customer.years > 5) & (F.order.total > 1000),
    then={"discount": 0.15},
)

decision = RuleSet([vip]).evaluate(data)
print(decision.explain())  # which rules fired, which did not, and why
```

## What exists today

- The [exception hierarchy](reference/errors.md) every component reports through.
- The [specification](spec/README.md) conventions that the Python, polars and
  Rust backends share.
