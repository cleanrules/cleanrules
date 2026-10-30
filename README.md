# RulePy

A rules engine for Python with one core for four jobs: **business rules**,
**expert systems** (forward chaining), **data validation** and **explaining ML
models**. Crisp and fuzzy logic share the same engine (fuzzy support via
[fuzzytool](https://fuzzytool.github.io)), every result explains itself, and
rule sets can be audited for contradictions, dead rules and coverage gaps.

> **Status:** early development (pre-alpha). Nothing is published yet and the
> API will change. See the [roadmap](docs/PLAN.md) (in Spanish).

```python
from rulepy import Rule, RuleSet, F  # planned API

vip = Rule("vip", when=(F.customer.years > 5) & (F.order.total > 1000), then={"discount": 0.15})

decision = RuleSet([vip]).evaluate(data)
print(decision.explain())  # which rules fired, which did not, and why
```

## License

MIT. See [LICENSE](LICENSE).
