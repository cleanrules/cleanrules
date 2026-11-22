# Migrating to the 1.0 contract

This guide is for anyone with rule files or code written against the draft of
cleanrules: the JSON format and the APIs as they stood before the 1.0
contract. Nothing had been published, so the format keeps `"version": 1`. Some
of the changes are still incompatible, though: a file may read differently,
give other results, or be written differently. Each section below says what
changed, whether you need to act, and shows the change in JSON, Python and
JavaScript. Rust and the C ABI are covered where they differ.

| Change | Files still read? | Results can change? | What to do |
|---|---|---|---|
| [Field paths of at most 256 segments](#field-paths-of-at-most-256-segments) | not those with longer paths | no | raise `max_path_segments` for trusted rules |
| [Objects without member order](#objects-without-member-order) | yes | the order of payload keys and maps | stop relying on member order |
| [Exact decimal arithmetic](#exact-decimal-arithmetic) | yes, except a field named `round` in text | yes, for arithmetic on decimals and `sum` | expect decimals where you got floats; use `round` |
| [Canonical bytes and digests](#canonical-bytes-and-digests) | yes | no | nothing, unless you compare written floats |
| [Rule ids](#rule-ids) | yes | no | give an `id` before renaming a rule |
| [Bundles](#bundles) | yes (new document) | no | optional: pin your rules with a bundle |

## What stays the same

- The format version is still `1`, and every document starts with
  `"version": 1`. The JSON Schemas live in `spec/schemas/v1/`.
- Conditions, rules, rule sets, decision tables, hit policies, logics,
  explanations and summaries keep their structure. Phase R added keys
  (`id`, `ruleset_digest`, the `round` node, the `bundle` document) and
  renamed none.
- Comparisons, null rules, int and float arithmetic, and `min`, `max` and
  `count` on ints and floats give the same results.
- The C ABI stays at version 1 (`CR_ABI_VERSION`).
- Error codes keep their names. `decision_mismatch` and `decimal_overflow`
  cover more cases than before (see below).

## Checking that your files still read

Read every rule file once with the new engine. A file that no longer reads
fails with a `CleanRulesError` whose `code` and pointer (or line and column)
say where and why.

```python
from pathlib import Path

import cleanrules

for path in sorted(Path("rules").rglob("*")):
    if path.suffix in {".json", ".yaml", ".yml", ".rules", ".csv"}:
        cleanrules.load(path)  # raises with the code, pointer and a hint
```

```js
import { readdir } from 'node:fs/promises'
import { join } from 'node:path'
import { load } from 'cleanrules'

for (const name of (await readdir('rules')).sort()) {
  if (/\.(json|ya?ml|rules|csv)$/.test(name)) await load(join('rules', name))
}
```

AI agents can do the same through the `validate` tool of the MCP server
(`spec/agents.md`).

A file that reads can still decide differently. Two changes can alter a
result: [decimal arithmetic](#exact-decimal-arithmetic) and the order of
[payload keys and maps](#objects-without-member-order). To find out, run your
known inputs through the old and new engines and compare the outputs as JSON
values, ignoring member order. Then pin the new results with a
[bundle](#bundles).

## Field paths of at most 256 segments

A field path has at most `max_path_segments` segments, 256 by default. This
covers conditions, payload values, and the headers and cells of decision
tables, in every format. A longer path is `limit_exceeded` (name
`max_path_segments`), reported at its JSON Pointer, or at its line and column
in text and CSV. Field constructors check the default too. The limit keeps a
few hundred KiB of untrusted rules from overflowing a thread's stack.

Before, this condition read; now it is refused:

```json
{ "version": 1, "condition": { "op": "field", "path": ["k", "k", "…300 segments…"] } }
```

For rules you trust, raise the limit when you read them:

```python
cleanrules.loads(text, max_path_segments=1024)
```

```js
loads(text, { limits: { maxPathSegments: 1024 } })
```

In Rust, set `Limits::max_path_segments` and build long paths with
`Node::field_within(path, &limits)`. In the C ABI, the limit is
`CR_LIMIT_MAX_PATH_SEGMENTS`.

## Objects without member order

JSON objects have no significant member order. Two documents whose objects
hold the same members in other orders are the same document, with the same
results, errors, explanations and diagrams. Arrays, and the order of rules,
keep their meaning.

**Payloads are associations.** A payload (`then`) keeps its keys sorted by
Unicode code point, whatever order they were written or built in. Rules are
evaluated, explained, drawn, written and given back in that order. If a key
is given twice, its last value wins. Before, a payload kept the order of its
definition.

```json
{ "name": "vip", "when": { "op": "field", "path": ["vip"] },
  "then": { "zeta": { "op": "const", "value": 1 }, "alpha": { "op": "const", "value": 2 } } }
```

The draft wrote this payload back as `zeta, alpha` and gave the output in that
order. Now it is written as `alpha, zeta`, and the output is
`{"alpha": 2, "zeta": 1}`.

```python
rule = Rule("vip", F.vip, {"zeta": 1, "alpha": 2})
list(rule.then)  # ['alpha', 'zeta']; the draft gave ['zeta', 'alpha']
```

```js
const rule = new Rule('vip', F.vip, { zeta: 1, alpha: 2 })
Object.keys(rule.then) // ['alpha', 'zeta']; integer-like keys still come first in any plain object
```

**Maps are compared without order.** Maps in input data, in metadata and in a
decision's `aggregated` member are compared without regard to order and given
back sorted by key, by code point, in every language. In JavaScript, a `Map`
no longer keeps its order. On the JVM, a `SequencedMap` or a Scala `SeqMap`
no longer keeps its order either. The conformance vector that asserted the
old behaviour, `rules/payloads/map-order-kept`, is now
`rules/payloads/map-keys-without-order`, which asserts the opposite. A record
(a named tuple, a pandas row, a C ABI record) still reads integer segments by
position.

**The first error follows key order.** When several keys of one object are
wrong, the error reported is the one for the first key by code point. This
applies to unknown keys, payload keys, metadata keys, `$`-keys of typed values
and agent tool arguments. Before, it was the first key in the text. Keys that
a decision table defines are still checked in the table's order. If your tests
match error pointers, they may need updating.

**Decisions read in any order.** `read_decision` and `readDecision` accept a
decision's `then` with its keys in any order. Before, that was
`decision_mismatch`.

**Agent input schemas** list a map's `properties` and `required` keys by code
point, not in order of first appearance.

C ABI: `cr_value_map_entry`, `cr_rule_payload_key` and `cr_rule_payload_value`
count positions among the keys sorted. Use `cr_tree_to_json` for the canonical
JSON of a result tree.

## Exact decimal arithmetic

`add`, `sub`, `mul`, `div`, `abs` and `neg` on decimals, or on a decimal and
an int, now give an exact decimal. Before, they converted to float64. A float
among the operands still gives a float, two ints still give an int, and `div`
of two ints is still a float. A result keeps its ideal scale (`19.99 × 0.85`
is `16.9915`) and is exact while it fits in 38 digits. Beyond that it is
rounded half to even to 38 significant digits (`1 / 3.0` is
`0.33333333333333333333333333333333333333`). A result that cannot fit at any
scale is null with the reason `decimal_overflow`. Comparisons are unchanged.

```python
price = F.price * Decimal("0.85")
RuleSet([Rule("p", F.ok, {"price": price})]).evaluate(
    {"ok": True, "price": Decimal("19.99")}
).output
# {'price': Decimal('16.9915')}; the draft gave the float 16.9915
```

```js
// A Decimal now, where the draft gave a number: read it with String(d), not Number(d).
evaluateValueTree(F.price.mul(decimal('0.85')), { price: decimal('19.99') }).value
```

Java and Scala get a `BigDecimal`, Go a `cr.Decimal`, and Erlang a
`cleanrules_decimal` tuple where they got floats.

**`round` is a new node.** It rounds an int or a decimal to `scale` digits
after the point (0 to 38). The `mode` defaults to `half_even`; the others are
`half_up`, `half_down`, `up`, `down`, `ceiling` and `floor`. Rounding a float
is a type mismatch, so floats are never rounded implicitly.

```json
{ "op": "round", "operand": { "op": "field", "path": ["price"] }, "scale": 2, "mode": "half_up" }
```

| Language | Rounding `price × 0.85` to cents |
|---|---|
| Text | `ROUND(price * 0.85, 2, HALF_UP)` |
| Python | `round(F.price * Decimal("0.85"), 2)` or `.round(2, "half_up")` |
| JavaScript | `F.price.mul(decimal('0.85')).round(2, { mode: 'half_up' })` |
| Rust | `(field("price") * d).round_with(2, RoundingMode::HalfUp)` |

**`ROUND` is a keyword of the text syntax.** Wherever rules are read as text,
a field, key or column named `round` (in any case) must be written between
backticks as its first segment: `` `round` > 1 ``. `order.round` still reads.
This applies to rule files, `rule_from_text`/`ruleFromText`, and the headers
and cells of decision tables. In the builders, a field named `round` after a
dot is now `F.x["round"]` in Python and `F.x.at('round')` in JavaScript.

**`collect` with `sum` adds exactly.** It adds every value exactly and rounds
once, so the result no longer depends on rule order. Ints give an int. Ints
and decimals give a decimal with the largest scale (`0.1 + 0.2` is `0.3`).
Any float gives the nearest float, so `1 + 1 + 1e16` and `1e16 + 1 + 1` are
both `1e16 + 2`; before, the second was `1e16`. `min` and `max` return a
decimal unchanged, compared exactly.

**Explanations** of an arithmetic overflow read
`decimal overflow in <paths>`, which was `decimal out of range in <path>`.

**Agent input schemas** type an operand of arithmetic as `number` or
`decimal` (it was `number`) and the operand of `round` as `decimal`.

## Canonical bytes and digests

Every document now has one sequence of **canonical bytes**: the UTF-8 JSON of
its canonical form, with every object's members sorted by key, no whitespace,
and fixed layouts for numbers, decimals and strings. Its **digest** is
`sha256:` followed by the SHA-256 of those bytes in hexadecimal. The **rule set
digest** of a rule, rule set or decision table is the digest of the rule set
it gives. A digest identifies one concrete artifact. It does not promise that
two rule sets decide alike: `x > 5` and `5 < x` have different digests.

```python
cleanrules.digest(rules)  # 'sha256:…'
cleanrules.canonical_bytes(rules)
```

```js
digest(rules) // 'sha256:…'
canonicalBytes(rules) // a Uint8Array
```

**One float is written differently.** Floats are written from their shortest
round-trip digits, and an exact tie now goes to the even last digit, as in
ECMAScript, Java and Python. So `201409170638417.125` is written
`201409170638417.12`, where the draft wrote `…17.13`. The layout of a float
(plain or exponent notation) is now part of the format. Readers still accept
any layout, so this only matters if you compare written text.

**Decisions can carry their rule set digest.** A decision may hold
`ruleset_digest`, which ties an audit log to the exact rules that made each
decision. Writers add it only on request. Readers check it before anything
else (`decision_mismatch` at `/decision/ruleset_digest`) and keep it.

```python
cleanrules.dumps(decision, ruleset_digest=True)  # or decision.to_dict(ruleset_digest=True)
```

```js
dumps(decision, { rulesetDigest: true })
```

In Rust, use `decision.with_ruleset_digest(true)`. In the C ABI, use
`cr_decision_write_with` with `CR_WRITE_RULESET_DIGEST`. Read such a decision
back with `read_decision`/`readDecision` on its rule set.

## Rule ids

A rule may have an optional `id`, a non-empty string. Its **effective id** is
the `id`, or the name when it has none. Effective ids are unique within a rule
set (`rule_definition_error`). The id changes nothing in evaluation.
Decisions, explanations, summaries and diagrams still name rules by name.
Rules without an id keep their bytes and digest.

The id matters when two versions of a rule set are compared, as bundles and
the test tools of the next phase do. A renamed rule without an id is a
different rule: one rule removed and another added. To rename a rule and keep
it the same rule, give it an explicit id equal to its old name.

```json
{ "name": "gold_customer", "id": "vip", "when": { "op": "field", "path": ["gold"] } }
```

```text
RULE gold_customer ID vip IF gold THEN discount = 0.15
```

```python
Rule("gold_customer", F.gold, {"discount": Decimal("0.15")}, id="vip")
```

```js
new Rule('gold_customer', F.gold, { discount: decimal('0.15') }, { id: 'vip' })
```

## Bundles

A **bundle** is a new kind of document, `{"version": 1, "bundle": {…}}`. It
holds:

- the rules (a rule set or a decision table);
- the `id` and `revision` you give them;
- their `ruleset_digest`;
- the `engine` that made it: its name, exact version and the six limits that
  can change a result;
- the `contract`: the input schema and the output keys;
- the paths and digests of your test case files.

Making the same bundle twice gives the same bytes. Reading one checks its rule
set digest and, for a bundle made by this engine version, its contract. A
bundle of another engine is read as it is, never refused.

**Pinning results.** Make a bundle of the rules you have checked, keep it next
to its case files, and compare later bundles with it. The comparison tells a
change of engine from a change of rules:

```python
bundle = cleanrules.Bundle(
    rules,
    id="acme/discounts",
    revision="2026-11-16.1",
    cases=[("cases/vip.json", Path("cases/vip.json").read_bytes())],
)
cleanrules.dump(bundle, "discounts.bundle.json")

pinned = cleanrules.load("discounts.bundle.json")
comparison = pinned.compare(cleanrules.Bundle(rules, id="acme/discounts", revision="2026-11-16.1"))
comparison.relation  # BundleRelation.SAME_RULES_SAME_ENGINE, or e.g. SAME_RULES_OTHER_ENGINE after an upgrade
comparison.differences  # (BundleDifference.CASES,): no case files given this time
pinned.case_matches("cases/vip.json", Path("cases/vip.json").read_bytes())  # the file is unchanged
```

```js
const bundle = new Bundle(rules, {
  id: 'acme/discounts',
  revision: '2026-11-16.1',
  cases: [{ path: 'cases/vip.json', content: await readFile('cases/vip.json') }],
})
await dump(bundle, 'discounts.bundle.json')

const pinned = await load('discounts.bundle.json')
pinned.compare(bundle) // { relation: 'same_rules_same_engine', identical: true, differences: [] }
pinned.cases[0].matches(await readFile('cases/vip.json')) // true
```

```rust
let bundle = Bundle::new(rules, BundleId::new("acme/discounts")?, Revision::new("2026-11-16.1")?)
    .with_cases([CaseFile::new("cases/vip.json", &bytes)?])?
    .with_limits(&limits)?;
```

The relations are `same_rules_same_engine`, `same_rules_other_engine`,
`other_rules_same_engine`, `other_rules_other_engine`, and `conflict` (one
`id` and `revision` naming two rule sets). "The same rules" means the same rule
set digest, so editing a description gives other rules.

Difference names keep the format's spelling in every language, for example
`engine.limits.max_depth`. The limit itself is `max_depth` in Python and Rust
and `maxDepth` in JavaScript.

A limit of 0, or above 2⁵³ − 1, cannot be recorded, and is `unsupported` in
every language. In JavaScript, a negative or fractional limit is a
`RangeError`, and `maxYamlSize` or `maxTextSize`, which bound reading rather
than decisions, are a `TypeError`.

### Names across languages

| Rust (`cleanrules::bundle`) | Python | JavaScript / TypeScript | C ABI |
|---|---|---|---|
| `Bundle::new(rules, BundleId, Revision)`, `.with_cases`, `.with_limits` | `Bundle(rules, *, id, revision, cases, max_…)` | `new Bundle(rules, { id, revision, cases, limits })` | `cr_document_new_bundle` |
| `Engine` | `BundleEngine` | `BundleEngine` | (JSON) |
| `EngineLimits`, `EngineLimit` | `BundleLimits` | `BundleLimits`, `BundleLimitName` | (JSON) |
| `Contract` | `BundleContract` | `BundleContract` | (JSON) |
| `Relation` | `BundleRelation` | `BundleRelation` | (JSON) |
| `Difference` | `BundleDifference` | `BundleDifference` | (JSON) |
| `Comparison` | `BundleComparison` | `BundleComparison` | `cr_document_compare_bundle` |
| `CaseFile` (path and digest), `.matches(bytes)` | `CaseFile`, `.matches(content)` | `CaseFile`, `.matches(content)` | — |
| `CaseFile::new(path, bytes)` | `(path, content)` pairs | `BundleCaseInput` `{ path, content }` | `struct cr_case_input` |
| `Bundle::case_matches(path, bytes)` | `Bundle.case_matches(path, content)` | `bundle.caseMatches(path, content)` | — |
| `PartialEq` by digest | `==` by digest | `digest(a) === digest(b)` | `cr_document_digest` |

Java, Scala, Go and C++ (beta) have no bundle type yet. They read, write and
hash a bundle as a document, and their `loads` refuses it with `unsupported`.
