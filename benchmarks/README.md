# Benchmarks

The criterion suites of the Rust core live in `crates/core/benches/`:

- `eval`: one input and batches of 1,000 under three hit policies, one condition
  tree, and reading and writing a rule set as JSON;
- `explain`: explaining 1,000 decisions, with and without writing them as JSON,
  evaluating and explaining together, and the summary of a rule set.

Both suites share one workload (`crates/core/benches/common`): a loan screening
rule set of 12 rules that uses every family of node, and 1,000 deterministic
applicants, some with missing or null fields.

```bash
cargo bench -p cleanrules --bench eval
cargo bench -p cleanrules --bench explain
cargo bench -p cleanrules -- --save-baseline <date>   # then --baseline <date>
```

`results/` keeps the measurements, one JSON file per run, with the machine, the
compiler and criterion's confidence interval for every case. Compare runs on the
same machine only.

## Rivals

`zen/` compares cleanrules with [zen-engine](https://crates.io/crates/zen-engine)
on the same workload: the 12 rules become one zen decision table, compiled and
evaluated without trace. It is a separate crate outside the workspace, so the
core never depends on zen-engine, and it is not built in CI.

```bash
cargo bench --manifest-path benchmarks/zen/Cargo.toml
```

Before measuring, the benchmark checks that both engines select the same rules
for every applicant, under `first` and `collect`. The engines do not do the
same work: cleanrules evaluates every rule and keeps every result tree so that
the decision can be explained, while zen's first hit stops at the first
matching row and keeps no trace.

First run (2026-11-01, Apple M5, rustc 1.97.1, zen-engine 2.1.6), 1,000
applicants, criterion's 95 % confidence interval:

| Case | cleanrules | zen-engine | Factor |
|---|---|---|---|
| `first` | 7.22 ms (7.22–7.23) | 46.4 ms (46.4–46.5) | 6.4× |
| `collect` | 8.51 ms (8.50–8.52) | 56.7 ms (56.6–56.8) | 6.7× |

One workload on one machine; the result says nothing yet about other rule sets
or about the bindings.
