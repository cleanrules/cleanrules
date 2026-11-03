# cleanrules

An explainable rules engine with **one Rust core** and the same semantics in
**Rust, C/C++, Python, JavaScript, Java/Scala, Go and Erlang**. One engine for
four jobs: **business rules**, **expert systems** (forward chaining), **data
validation** and **explaining ML models**, and a deterministic, explainable
tool and guardrail for **AI agents** (through MCP and adapters for agent
frameworks). Crisp (three-valued Kleene) and fuzzy logic share the same engine
(fuzzy norms follow [fuzzytool](https://fuzzytool.github.io)), and every result
explains itself.

Website: <https://cleanrules.github.io>

> **Status:** pre-alpha, nothing is published yet. The semantics are written
> down as a **specification** and checked by **conformance vectors**. The Rust
> core passes all of them; the bindings come next. See the
> [roadmap](docs/PLAN.md) (in Spanish).

## What is here

| Path | Contents |
|---|---|
| [`spec/`](spec/) | The normative specification: condition AST, rules and hit policies, the data model, explanations, errors, and the JSON format (version 1) with its JSON Schemas |
| [`conformance/`](conformance/) | 2,000+ language-neutral test vectors that every implementation must pass, and their checker |
| [`crates/core/`](crates/core/) | The Rust core (crate `cleanrules`) |
| [`benchmarks/`](benchmarks/) | Benchmark suites, results, and the comparison with zen-engine |
| [`docs/PLAN.md`](docs/PLAN.md) | The roadmap and its decisions (in Spanish) |

A rule as the engine reads it, in the JSON format:

```json
{
  "version": 1,
  "rule": {
    "name": "vip",
    "when": {
      "op": "and",
      "operands": [
        { "op": "gt", "left": { "op": "field", "path": ["customer", "years"] }, "right": { "op": "const", "value": 5 } },
        { "op": "gt", "left": { "op": "field", "path": ["order", "total"] }, "right": { "op": "const", "value": 1000 } }
      ]
    },
    "then": { "discount": { "op": "const", "value": { "$decimal": "0.15" } } }
  }
}
```

Each language builds such rules idiomatically: in Python,
`(F.customer.years > 5) & (F.order.total > 1000)`.

## Citing

If you use cleanrules in academic work, please cite it. GitHub's **"Cite this
repository"** button (from [`CITATION.cff`](CITATION.cff)) produces ready-made
APA and BibTeX entries. A Zenodo DOI will come with the first release; until
then, cite the repository:

```bibtex
@software{salmeron_cleanrules,
  author = {Salmeron, Jose L.},
  title  = {{cleanrules}: An explainable rules engine with a Rust core},
  year   = {2026},
  url    = {https://github.com/cleanrules/cleanrules}
}
```

## License

Apache License 2.0, created by Jose L. Salmeron. See [LICENSE](LICENSE) and
[NOTICE](NOTICE): anyone who redistributes cleanrules, or a work derived from
it, must keep the NOTICE with its attribution of authorship.
