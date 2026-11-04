# cleanrules for Node.js

The `cleanrules` npm package is the Rust engine of cleanrules as a native Node
addon, built with [napi-rs](https://napi.rs): the same rules, decisions,
explanations and errors as in every other language. It reads rules as JSON,
YAML, the rule syntax or CSV decision tables, evaluates them on plain
JavaScript data, and explains every decision.

This is the native layer (task F.1a), in `cleanrules/native`. The idiomatic
TypeScript API (rule builders, the `F` proxy, operators) comes in task F.2, on
top of it, and the browser and Deno build (wasm) in F.1b.

## Example

```js
const { RuleSet } = require('cleanrules/native')
const { MissingFieldError } = require('cleanrules')

const rules = RuleSet.read(`
  HIT POLICY priority
  RULE vip PRIORITY 10
  IF customer.years > 5 AND order.total > 1000
  THEN discount = 0.15
  DO notify_sales
  RULE loyal PRIORITY 5
  IF customer.years > 2
  THEN discount = 0.05`, { format: 'text' })

const data = { customer: { years: 7 }, order: { total: 1500 } }
const decision = rules.evaluate(data)
decision.outputJson()   // '{"discount":{"$decimal":"0.15"}}'
decision.explain()      // why vip was selected and loyal was not

rules.run(data, { notify_sales: ({ ruleName, outputJson }) => console.log(ruleName, outputJson) })
```

[`docs/examples/discounts.js`](../../docs/examples/discounts.js) does more:
JSON input, which rules fired, an action, and an error.

## Entry points

- `cleanrules`: what is final today, the errors, the marker values
  (`decimal`, `float`, `date`, `datetime`), `FORMAT_VERSION` and `version`. The
  idiomatic API (task F.2) arrives here.
- `cleanrules/native`: the native layer, the classes below. F.2 builds on it,
  and it stays available.

Both are CommonJS with TypeScript declarations, and ES modules import them by
name (`import { RuleSet } from 'cleanrules/native'`).

## Installing and building

Users install `cleanrules` only (Node 22.12 or later). The addon ships in one
optional package per platform, `@cleanrules/node-<platform>`
([`npm/`](npm)), and npm installs the one for the machine: Linux (x64 and arm64,
glibc and musl), macOS (x64 and arm64) and Windows (x64 and arm64, MSVC).
Nothing is published yet (task M.2). The package refuses to load an addon of
another version, and says to reinstall it.

From the repository, with a Rust toolchain:

```sh
npm --prefix bindings/js install      # @napi-rs/cli and typescript
npm --prefix bindings/js test         # builds the addon (debug), node --test, tsc --noEmit
npm --prefix bindings/js run build:release
```

`npm run build` writes the addon (`cleanrules.<platform>.node`), the loader
napi-rs generates (`binding.js`) and its declarations (`binding.d.ts`); the
last two are committed, and CI checks that they are current and that every
version (package, platform packages, Cargo workspace) is the same
(`scripts/check-versions.js`).

## The native layer

| Name | What |
|---|---|
| `Document.read(text, { format, limits }?)` | a condition, rule, rule set or decision table; `kind`, `write(format?)` |
| `RuleSet.read(text, { format, limits }?)`, `RuleSet.fromDocument(doc)` | a rule set (a rule document is a rule set of that rule; a CSV table is compiled) |
| `ruleset.evaluate(data)`, `evaluateJson(text, limits?)` | one decision, from native data or from a JSON value |
| `ruleset.evaluateMany(rows)`, `evaluateManyJson(text, limits?)` | a batch; an error names its `row` |
| `ruleset.run(data, actions)` | evaluate, then call the selected rules' actions, in rule order |
| `ruleset.readDecision(text, { format, limits }?)` | a recorded decision, checked against the rule set |
| `ruleset.write(format?)`, `summary(format?)` | the document (JSON by default), and the summary (text by default, or JSON or YAML) |
| `ruleset.length`, `ruleNames()`, `hitPolicy`, `aggregate`, `logic`, `strict`, `fieldPaths()` | what it is and what it reads |
| `decision.length`, `ruleName(i)`, `firedAt(i)`, `truth(i)`, `selected()` | each rule's result |
| `decision.outputJson()`, `outputsJson()`, `ruleOutputJson(i)` | outputs, in the JSON format; `null` for none |
| `decision.write(format?)`, `explain(format?)` | the decision document (JSON by default), and its explanation (text by default, or JSON or YAML) |
| `Node.read(text, { format, limits }?)` | one condition or value node; `write`, `fieldPaths`, `evaluateTree(data, { logic, strict }?)`, `evaluateValueTree(data, { strict }?)`, and `evaluateTreeJson(text, { logic, strict, limits }?)` and `evaluateValueTreeJson(text, { strict, limits }?)` |

Formats are `"json"`, `"yaml"`, `"text"` (the rule syntax) and `"csv"`
(decision tables). Texts are strings or bytes (`Uint8Array`, `Buffer`); bytes
that are not UTF-8 are a `FormatError` at their line and column. `limits` is an
object with any of `maxSize` (bytes, for every format), `maxNesting`,
`maxDepth`, `maxTableSize`, `maxYamlSize` and `maxTextSize`. The defaults
(explanations and summaries as text) are those of C++.

Outputs, explanations and summaries come out as text in the JSON format
(`spec/json-format.md`), with `{"$decimal": ...}`, `{"$int": ...}` for integers
beyond 2⁵³ and so on. Native JavaScript values for outputs come with the
TypeScript API (F.2).

## Data

Native data follows `spec/data-model.md` (*JavaScript and TypeScript*). Only
the fields the rules read are read, each object once, so a getter runs at most
once per evaluation.

| JavaScript | Value |
|---|---|
| `null`, an Invalid `Date`, `NaN` | null; `undefined` is missing |
| `boolean`, `string` | boolean, string (a lone surrogate makes it opaque) |
| an integer `number` within ±(2⁵³ − 1) | int; `float(x)` for a float |
| any other `number`, `-0` and infinities included | float |
| `bigint` | int, or null with `int_overflow` beyond 64 bits |
| `decimal("0.10")` | decimal, or null with `decimal_overflow` beyond 38 digits |
| `float(x)` | float |
| `date(...)`, `Temporal.PlainDate` | date |
| `datetime(...)`, `Temporal.PlainDateTime` | naive datetime |
| `Date`, `Temporal.Instant` | aware datetime in `Z` |
| `Temporal.ZonedDateTime` | aware datetime in its zone, or with its offset when the zone id is one |
| `Array`, typed arrays except bytes | list; holes and `undefined` elements are null |
| `Uint8Array`, `Buffer`, `ArrayBuffer`, `DataView` | opaque, like bytes in other languages |
| `Map` | map, in insertion order; entries whose key is not a string are left out |
| plain objects, class instances | map of their own enumerable string keys and their classes' getters |

Anything else is opaque (`Set`, `RegExp`, `Error`, symbols, functions in a
`Map`...). A rule never reaches into the program: a string segment never reads
a symbol, a `#` field, a method, a prototype, `constructor` or `__proto__`, or
anything of a string, number or date; an integer segment only indexes arrays
and typed arrays. A getter that throws gives a null with reason `read_error`,
and in strict mode an `EvaluationError` whose `cause` is what it threw.
Temporal values are recognised by `Symbol.toStringTag`, so polyfills work, and
marker values by the brand `Symbol.for('cleanrules.marker')`, so markers made by
another copy of the package work too.

## Errors

Every error is a `CleanRulesError` (an `Error`), with the class of its code:

```text
CleanRulesError
├── FormatError                 format_error, unsupported_version, limit_exceeded, decision_mismatch
├── ExpressionError             expression_error
├── RuleDefinitionError         rule_definition_error
├── RuleSyntaxError             rule_syntax_error
├── RegistryError               registry_error
├── RuleNotFoundError           rule_not_found
├── EvaluationError             evaluation_error
│   ├── MissingFieldError       missing_field
│   ├── TypeMismatchError       type_mismatch
│   └── HitPolicyError          hit_policy_error
├── ActionError                 action_error
└── UnsupportedError            unsupported
```

Each has `code`, `message`, `hint` (or `null`), `context` (the fields that
apply: `rule`, `path`, `pointer`, `line`, `column`, `rules`, `types`, `row`,
`name`, `available`, `executed`, `node`, `operation`, `reason`; an object,
because an error's `name` is its class), `cause` when another error or value
caused it, and `toJSON()`, the error's JSON form. An `ActionError` also has the
`decision` whose actions were running. Arguments the binding cannot take (an
unknown format, a negative limit) are `TypeError`s and `RangeError`s.

## Actions

`run(data, actions)` takes an object or a `Map` from action names to functions.
Every action the rules name must be there (else a `RegistryError`, before
anything is read). Each selected rule's action is called, in rule order, with an
`ActionCall`: `{ ruleName, position, input, truth, outputJson, decision }` (the
rule, the input as given, the truth of its condition, its evaluated payload in
JSON, and the decision `run` returns). Actions are synchronous: what they
return is ignored, except a promise, which fails the run. If an action throws
or returns a promise, the run stops with an `ActionError` (whose `cause` is what
it threw).

## Licence

Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). cleanrules is written
by Jose L. Salmeron.
