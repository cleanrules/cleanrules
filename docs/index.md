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
    Conditions, evaluation and rule sets work, but nothing is published yet and
    the API may still change.

```python
from rulepy import F, Rule, RuleSet

vip = Rule(
    "vip",
    when=(F.customer.years > 5) & (F.order.total > 1000),
    then={"discount": F.order.total * 0.1},
)

decision = RuleSet([vip]).evaluate({"customer": {"years": 7}, "order": {"total": 1500}})
decision.output  # {'discount': 150.0}
decision.fired  # ('vip',)
```

## What exists today

- [Conditions](reference/expr.md), written with `F` (see the
  [common pitfalls](pitfalls.md)).
- The [reference evaluator](reference/evaluate.md), which reports *why* a value
  is unknown.
- [Rules and rule sets](reference/rules.md) with DMN-style hit policies.
- The [specification](spec/README.md) that every backend shares.
