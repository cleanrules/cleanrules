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
