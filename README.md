# cleanrules

An explainable rules engine with **one Rust core** and the same semantics in
**Rust, C/C++, Python, JavaScript, Java/Scala and Go**. One engine for four
jobs: **business rules**, **expert systems** (forward chaining), **data
validation** and **explaining ML models**. Crisp (three-valued Kleene) and
fuzzy logic share the same engine (fuzzy norms follow
[fuzzytool](https://fuzzytool.github.io)), and every result explains itself.

Website: <https://cleanrules.github.io>

> **Status:** pre-alpha, nothing is published yet. This repository currently
> holds the **Python reference implementation** (package `rulepy`), which
> defines the semantics. Its knowledge is being turned into a written
> specification and cross-language conformance vectors; the Rust core and the
> bindings are built against them, and the Python prototype is then retired.
> See the [roadmap](docs/PLAN.md) (in Spanish).

```python
from rulepy import F, Rule, RuleSet

vip = Rule("vip", when=(F.customer.years > 5) & (F.order.total > 1000), then={"discount": 0.15})

data = {"customer": {"years": 7}, "order": {"total": 1500}}
decision = RuleSet([vip]).evaluate(data)
print(decision.output)  # {'discount': 0.15}
print(decision.explain())  # which rules fired, which did not, and why
```

## License

MIT. See [LICENSE](LICENSE).
