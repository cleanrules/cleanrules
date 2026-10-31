# RulePy

A rules engine for Python with one core for four jobs: **business rules**,
**expert systems** (forward chaining), **data validation** and **explaining ML
models**. Crisp and fuzzy logic share the same engine (fuzzy support via
[fuzzytool](https://fuzzytool.github.io)), every result explains itself, and
rule sets can be audited for contradictions, dead rules and coverage gaps.

> **Status:** pre-alpha. Conditions, the reference evaluator and rule sets with
> hit policies work; nothing is published yet and the API may still change. See
> the [roadmap](docs/PLAN.md) (in Spanish).

```python
from rulepy import F, Rule, RuleSet

vip = Rule("vip", when=(F.customer.years > 5) & (F.order.total > 1000), then={"discount": 0.15})

data = {"customer": {"years": 7}, "order": {"total": 1500}}
decision = RuleSet([vip]).evaluate(data)
print(decision.output)  # {'discount': 0.15}
```

## License

MIT. See [LICENSE](LICENSE).
