# cleanrules for JavaScript

The `cleanrules` npm package is the Rust engine of cleanrules for JavaScript:
the same rules, decisions, explanations and errors as in every other language.
It reads rules as JSON, YAML, the rule syntax or CSV decision tables, evaluates
them on plain JavaScript data, and explains every decision. It comes in two
builds with the same API: a native Node addon built with
[napi-rs](https://napi.rs) (task F.1a), and a WebAssembly module built with
[wasm-bindgen](https://wasm-bindgen.github.io/wasm-bindgen/) for browsers,
Deno and other runtimes (task F.1b).

The package root, `cleanrules`, is the idiomatic API (tasks F.2a and F.2b):
conditions built with the `F` proxy and methods, rules and rule sets built in
code or read from documents (`loads`, `load`, `ruleFromText`) and written back
(`dumps`, `dump`), decision tables, result trees of one condition
(`evaluateTree`), decisions with native JavaScript values, and explanations
and summaries as objects. The native layer underneath stays available in
`cleanrules/native`, and `cleanrules/wasm` is the WebAssembly build
initialised by hand.

## Example

```js
import { F, Rule, RuleSet, decimal } from 'cleanrules'

const rules = new RuleSet(
  [
    new Rule('vip', F.customer.years.gt(5).and(F.order.total.gt(1000)), { discount: decimal('0.15') }, {
      priority: 10,
      do: ({ rule, output }) => console.log(`${rule.name}: ${output.discount}`),
    }),
    new Rule('loyal', F.customer.years.gt(2), { discount: decimal('0.05') }, { priority: 5 }),
  ],
  { hitPolicy: 'priority' },
)

const decision = rules.evaluate({ customer: { years: 7 }, order: { total: 900 } })
decision.output              // { discount: Decimal { text: '0.05' } }
decision.whyNot('vip').causes[0].counterfactual   // { path: ['order', 'total'], op: 'gt', target: 1000, gap: 100 }
String(decision.explain())   // the explanation's text, the same in every language
rules.run({ customer: { years: 7 }, order: { total: 1500 } })   // calls vip's action
```

[`docs/examples/discounts.mjs`](../../docs/examples/discounts.mjs) does more,
in Node and Deno: why a rule did not fire, the summary, a named action bound
to a function, and an error. The same rules read from the rule syntax, with the
native layer:

```js
import { RuleSet } from 'cleanrules/native'
import { MissingFieldError } from 'cleanrules'

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

The same code runs in Node, Deno (`import { RuleSet } from
'npm:cleanrules/native'`) and browsers. On Node, CommonJS works too
(`const { RuleSet } = require('cleanrules/native')`).
[`docs/examples/discounts.js`](../../docs/examples/discounts.js) (Node,
CommonJS) and [`docs/examples/discounts.deno.js`](../../docs/examples/discounts.deno.js)
(Deno) do more: JSON input, which rules fired, an action, and an error.

## Entry points

- `cleanrules`: the idiomatic API (`F`, `field`, `constant`, `and`, `or`,
  `not`, `Expr`, `Rule`, `RuleSet`, `Decision`, `DecisionTable`, `loads`,
  `dumps`, `load`, `dump`, `ruleFromText`, `evaluateTree`,
  `evaluateValueTree`, and the classes of explanations, summaries, result
  trees and tables), the errors, the marker values (`decimal`, `float`,
  `date`, `datetime`, and `ZonedDateTime` given back), `FORMAT_VERSION` and
  `version`.
- `cleanrules/native`: the native layer, the classes below. The idiomatic API
  builds on it, and it stays available.
- `cleanrules/wasm`: the WebAssembly build initialised by hand (*Initialising
  the WebAssembly module yourself*, below), in every runtime.

Each comes in both builds, and `package.json` `exports` picks one by
condition:

| Runtime | Condition | Build | Files |
|---|---|---|---|
| Node 22.12 or later | `node` | the native addon | `index.js`, `native.js` |
| Deno 2.6 or later | `deno` | WebAssembly, imported as a module source | `index.deno.js`, `native.deno.js` |
| browsers and bundlers | `browser` | WebAssembly, fetched by URL | `index.wasm.js`, `native.wasm.js` |
| any other runtime | `default` | WebAssembly, fetched by URL | the same |

The modules are ES modules, one copy of the shared code (`lib/`). On Node,
CommonJS works too: `require('cleanrules')` and `require('cleanrules/native')`
load them through `require(esm)`. TypeScript compiling to CommonJS needs
`"module": "nodenext"` to import them (under `node16` it reports TS1471).

The WebAssembly entries instantiate the module with top-level `await`, so they
are imported, not required. In browsers (and other runtimes) they use no Node
API and fetch `wasm/cleanrules_bg.wasm` from `new URL('./wasm/cleanrules_bg.wasm',
import.meta.url)`; in Node they read it from disk. Deno's entries import it as
a module source (`import source module from '...wasm'`, Deno 2.6 or later),
so loading the package needs no permission. A worker instantiates its own
module.

With bundlers, the module is a separate file that the bundle must ship
(`exports` names it, `cleanrules/wasm/cleanrules_bg.wasm`):

- webpack 5 handles `new URL('...', import.meta.url)` and copies the file.
- Vite copies it in builds; in development, add `optimizeDeps: { exclude:
  ['cleanrules'] }` so that the pre-bundler does not move the JavaScript away
  from it.
- esbuild does not follow `new URL(...)`: copy `wasm/cleanrules_bg.wasm` next
  to the bundled module (or serve it at the URL the module computes).

Where a bundler or a CDN serves the module from a URL of its own, use
`cleanrules/wasm` and pass that URL (below).

On Node, where no prebuilt addon exists for the platform, `node
--conditions=browser` runs the WebAssembly build instead.

### What differs between the builds

The same classes, methods, options, errors (classes and messages), markers and
`ActionCall`, and the same idiomatic API (each build makes its own `Expr`,
`Rule`, `RuleSet` and `Decision` classes, over its own native objects, so
objects of one build do not mix with the other's: passing one to the other
build is a `TypeError` that says so): the whole test suite runs on
each build, in Node and in Deno, and compares their members, the errors of a
list of wrong calls, and what decisions, explanations and summaries give back.
Only these differ:

- The WebAssembly build's objects have wasm-bindgen's `free()` (and its
  internal `__destroy_into_raw()` and static `__wrap()`). `free()` and
  `[Symbol.dispose]()` release an object early; a second call does nothing,
  and using a released object is a `TypeError`. JavaScript releases them anyway
  when it collects them. The addon's `[Symbol.dispose]()` does nothing, so
  `using` works with both.
- The addon checks that it is the package's version; the module ships inside
  the package.
- Built-in objects of another realm (a `vm` context, an iframe) are told apart
  by brand everywhere, but where `util.types` does not exist (browsers) an
  `Error` of another realm is recognised only with `Error.isError`.

### Initialising the WebAssembly module yourself

`cleanrules/wasm` has the exports of `cleanrules`, the native classes in
`native` (`native.RuleSet.read(...)`: its `RuleSet` and `Decision` are not the
API's), and `init(source)`, which loads the module from where you say:

```js
import { init, F, Rule, RuleSet } from 'cleanrules/wasm'

// A URL (a string or a URL), a Request, a Response or a promise of one, the
// bytes (an ArrayBuffer or a typed array) or a compiled WebAssembly.Module.
await init(new URL('cleanrules_bg.wasm', import.meta.url))
```

- With **Vite**: `import wasmUrl from 'cleanrules/wasm/cleanrules_bg.wasm?url'`
  and `await init(wasmUrl)`.
- With **webpack 5**: an asset rule (`{ test: /cleanrules_bg\.wasm$/, type:
  'asset/resource' }`) makes `import wasmUrl from
  'cleanrules/wasm/cleanrules_bg.wasm'` the file's URL; then `await
  init(wasmUrl)`.
- With **esbuild** or any bundler that does not copy it: copy
  `node_modules/cleanrules/wasm/cleanrules_bg.wasm` to your assets and pass its
  URL.
- From a **CDN**, the module next to the package's files: `await
  init('https://cdn.jsdelivr.net/npm/cleanrules@<version>/wasm/cleanrules_bg.wasm')`
  (served as `application/wasm`, it compiles while it downloads).
- In **Node** and **Deno**, `await init()` loads the file next to the package
  (Node reads it from disk; Deno fetches it, with `--allow-read`), and bytes
  work everywhere: `await init(await readFile(path))`.

Importing the entry loads nothing (no top-level `await`, no runtime module).
Until `init` resolves, using what needs the module (the API of the build,
`decimal` and `native`: calling it, constructing it, or reading, listing or
changing its properties) throws an `Error` whose `code` is
`'ERR_CLEANRULES_NOT_INITIALISED'` and whose message says to call `await
init(source)` first, and `version` and `FORMAT_VERSION` are `undefined` (typed
`string | undefined` and `number | undefined`); the error classes, the marker
classes and the classes of results work at once. The exports become the real
values when `init` resolves, and a copy taken before (`const { F } = await
import('cleanrules/wasm')`, or with `require`) forwards to them. `init`
returns the same promise when called again (a rejected one can be retried),
so calling it twice is harmless. In one realm, `cleanrules/wasm` and
`cleanrules` on WebAssembly share the module and the API's classes. The error
is a plain `Error`, not a `CleanRulesError`: like the addon's version check,
it is a loading mistake, not one of the engine's error codes
(`spec/errors.md`), hence a code in Node's style.

## Installing and building

Users install `cleanrules` only. On Node, the addon ships in one optional
package per platform, `@cleanrules/node-<platform>` ([`npm/`](npm)), and npm
installs the one for the machine: Linux (x64 and arm64, glibc and musl), macOS
(x64 and arm64) and Windows (x64 and arm64, MSVC). The package refuses to load
an addon of another version, and says to reinstall it. The WebAssembly module
is in the package itself (`npm pack` refuses to pack without it, or with one older
than the Rust sources: `scripts/prepack.js`). Nothing is published yet (task
M.2).

From the repository, with a Rust toolchain:

```sh
npm --prefix bindings/js install          # @napi-rs/cli and typescript
npm --prefix bindings/js test             # the addon (debug): node --test with coverage, tsc --noEmit
npm --prefix bindings/js run test:wasm    # the WebAssembly module: the same suite in Node
npm --prefix bindings/js run test:deno    # the same suite in Deno, on the module
npm --prefix bindings/js run test:conformance   # the conformance runner alone, on the addon
npm --prefix bindings/js run build:release
```

Each suite runs the conformance runner (`test/conformance.test.js`): every
vector of [`conformance/`](../../conformance) through JSON on the native layer
(`json:<id>`) and through native JavaScript data on the idiomatic API
(`native:<id>`), compared with a lossless JSON reader of its own. It prints
how many passed each way, and lists the vectors a route skips (grids, which
are native data, have no JSON route) and the native inputs no JavaScript form
holds exactly (without Temporal, an aware datetime with microseconds is given
as the `ZonedDateTime` the engine gives back). On the module:
`CLEANRULES_BUILD=wasm node --test test/conformance.test.js` from
`bindings/js`, after `npm run build:wasm`.

`npm test` measures the coverage of the JavaScript half (`lib/` and the entry
files) with Node's own (`--experimental-test-coverage`), and fails under 90 %
of lines.

`npm run build` writes the addon (`cleanrules.<platform>.node`), the loader
napi-rs generates (`binding.cjs`) and its declarations (`binding.d.ts`, the
types of both builds); the last two are committed, and CI checks that they are
current and that every version agrees (`scripts/check-versions.js`).

`npm run build:wasm` (`scripts/build-wasm.js`) builds the module into `wasm/`
with the `wasm` profile of the workspace (size-optimised: `opt-level = "s"`,
LTO) and wasm-bindgen's `--target web`. It needs the `wasm32-unknown-unknown`
target (`rustup target add wasm32-unknown-unknown`) and the `wasm-bindgen` CLI
at the version of the `wasm-bindgen` crate, which `bindings/js/Cargo.toml` pins
(`cargo install --locked wasm-bindgen-cli --version 0.2.130`); the script
checks that they match. The module is 1.9 MiB, 676 KiB gzipped (the core with
its time zone database, regular expressions and the YAML reader). Binaryen's
`wasm-opt -Oz` shaves 5 % off the raw size and nothing off the gzipped size,
so the build does not use it.

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

In this layer, outputs, explanations and summaries come out as text in the
JSON format (`spec/json-format.md`), with `{"$decimal": ...}`, `{"$int": ...}`
for integers beyond 2⁵³ and so on. The idiomatic API gives native values
(*Values given back*, below).

## Conditions

A condition is data, a tree of nodes the core evaluates, never a JavaScript
function, so a rule can be explained, written and audited. Nodes are built
with the `F` proxy and methods; fuzzytool's operators `&`, `|` and `~` are
`.and()`, `.or()` and `.not()`:

```js
import { F, and, constant, decimal, field, not, or } from 'cleanrules'

F.customer.age.ge(18).and(F.country.isIn(['ES', 'PT']), not(F.blocked))
F.price.mul(F.quantity).sub(F.discount).gt(decimal('100.00'))
F.age.between(18, 65, { closed: 'left' })   // 'both' (default), 'left', 'right', 'none'
F.email.matches('^[^@]+@example\\.com$')       // the Rust regex syntax: linear time
or(F.vip, F.total.gt(1000))
```

| Method | Node |
|---|---|
| `eq`, `ne`, `lt`, `le`, `gt`, `ge` | comparison with a node or a constant |
| `and(...)`, `or(...)`, `not()`; `and(...)`, `or(...)`, `not(x)` as functions | connectives (at least two operands) |
| `add`, `sub`, `mul`, `div`, `neg()`, `abs()` | arithmetic |
| `isIn(values)` | membership in constants of one type group |
| `between(low, high, { closed })` | a range |
| `isNull()` | missing or null; never unknown |
| `matches(pattern)`, `startsWith(text)`, `endsWith(text)`, `contains(text)` | string tests |

Every node is built by the core's constructors, so a node it refuses fails at
once with an `ExpressionError` (`F.x.gt(NaN)`, `F.x.isIn([1, 'a'])`, `and(F.a)`).
Values next to a node become constants (`F.x.gt(5)`); `constant(value)` makes
one where no node is involved. Comparing with `null` is refused: test missing
values with `isNull()`. A node is immutable, with `tag`, `kind`
(`'condition'`, `'value'` or `'any'`), `children()`, `fieldPaths()`,
`sameAs(other)` (structural identity), `toString()` (the canonical text, as
explanations write it) and `toJSON()` (its JSON form). Fields have `path`,
constants `value`; `walk()` yields a node and its descendants in pre-order.
Nodes are immutable: assigning to one throws a `TypeError`, in strict and
sloppy code alike.

Every property of `F` (and of a field) is a key, except a field's own members.
Three rules keep the proxy safe:

- `then` is never a field, so `await` and promises never take a field for a
  promise; symbols, `toJSON`, Node's and Deno's inspection and the methods are
  the real members.
- A key named like a method (`and`, `path`, `tag`...) or like a member of
  `Object.prototype` (`constructor`, `valueOf`, `toString`, `hasOwnProperty`...)
  is reached with `F.x.at('and')`, or `field('x', 'and')`.
- An index is ambiguous as a property (JavaScript property names are strings,
  and a string key never reads an array): `F.items[0]` throws, with the fix.
  Write `F.items.at(0).price` or `field('items', 0, 'price')`; `.at('0')` is the
  key `"0"`.

In TypeScript, `F` lets any key through. Typed after the input, it checks the
keys below the first and completes them, with nothing possibly `undefined`
(also under `noUncheckedIndexedAccess`, where every key of the untyped `F` is
possibly `undefined`, as for any index signature):

```ts
import { F, type Fields } from 'cleanrules'

interface Order { customer: { years: number }; items: { price: number }[] }
const O = F as Fields<Order>               // types only: at run time it is F
O.customer.years.gt(5).and(O.items.at(0).price.gt(10))
```

JavaScript operators are refused rather than silently wrong: `F.age >= 18` and
`F.age + 1` throw an `ExpressionError` (a node is not a number), while
`` `${node}` `` and `String(node)` give its text. Like `==` in other bindings,
`===` compares objects: use `sameAs`.

## Rules, rule sets and decisions

```js
new Rule(name, when, then?, { priority, tags, description, metadata, do }?)
new RuleSet(rules, { hitPolicy, aggregate, logic, strict }?)
```

The third argument is the payload and the fourth the options: `new Rule('vip',
cond, { priority: 10 })` makes a payload with the output `priority`, which is
rarely what was meant (`new Rule('vip', cond, null, { priority: 10 })`).
`then` is an object or a `Map` of payload values (nodes or constants), in
order. `do` is the action: a function, called by `run`, or the name of one
(`do: 'notify'`), which is data and `bind` resolves. `metadata` holds data
only. Rules and rule sets are immutable and checked as they are built
(`RuleDefinitionError`, `ExpressionError` or `RegistryError`, as the core
reports them); an unknown option, here and in `between` and `readDecision`, is
a `TypeError`. Hit policies and aggregates are named as in the format, in any
case. `toJSON()` of a rule or rule set is its document of the JSON format
(`{ version, rule }`, `{ version, ruleset }`), as `dumps` writes it: an action
given as a name is written as its name, and one given as code is an
`UnsupportedError` (pass `{ omitActions: true }` to `dumps`).

| Name | What |
|---|---|
| `rule.name`, `when`, `then`, `priority`, `tags`, `description`, `metadata`, `do`, `actionName` | the rule, as the core keeps it (tags sorted, without repeats) |
| `ruleset.rules`, `length`, iteration, `get(name)`, `has(name)`, `rule(name)` | the rules; `get` gives `undefined` for an unknown name, like `Map`, and `rule` throws a `RuleNotFoundError` listing them |
| `ruleset.hitPolicy`, `aggregate`, `logic`, `strict`, `fieldPaths()` | its configuration and what it reads |
| `ruleset.evaluate(data)`, `evaluateMany(rows)`, `readDecision(text, { format, limits }?)` | decisions |
| `ruleset.bind(actions)`, `run(data)` | actions (below) |
| `ruleset.summary()` | a `Summary`: `hitPolicy`, `rules`, `fields`, `outputs`, `tags`; `toString()` and `toJSON()` |
| `decision.ruleset`, `results`, `selected`, `fired`, `hitPolicy` | each rule's `RuleResult` (`rule`, `fired`, `truth`, `output`) |
| `decision.output`, `outputs`, `aggregated` | native values; `output` throws a `HitPolicyError` for `rule_order`, and `collect` without an aggregate |
| `decision.explain()`, `whyNot(name)`, `toJSON()` | an `Explanation`, one rule's (a `RuleNotFoundError` for an unknown name), and the decision document |

An `Explanation` has `rules` (a `RuleExplanation` each: `rule`, `status`,
`requirement`, `note`, `achievable`, `causes`), `selected`, `forRule(name)`,
`toString()` (the text every language shares) and `toJSON()`. A requirement is
a `Cause` (`node`, `truth`, `negated`, `holds`, `values`, `reason`,
`counterfactual`, and its line as `toString()`) or a `CauseGroup` (`kind`
`'all'` or `'any'`, `items`); a `Counterfactual` has `path`, `op`, `target` and
`gap` (a number, a `Decimal`, `{ days }` or `{ microseconds }`); a `Note` says
why a rule that fired was not selected. Everything comes from the core:
evaluation, selection, explanation and their text.

## Documents

```js
import { dump, dumps, load, loads, ruleFromText } from 'cleanrules'

const rules = await load('pricing.yaml', { actions: { notify } })   // Node and Deno
const text = dumps(rules, { format: 'yaml' })
const same = loads(text, { format: 'yaml' })
const vip = ruleFromText('RULE vip IF customer.years > 5 THEN discount = 0.15')
await dump(rules, 'pricing.rules')
```

| Name | What |
|---|---|
| `loads(text, { format, limits, actions }?)` | an `Expr`, `Rule`, `RuleSet` or `DecisionTable` from a document (a string or UTF-8 bytes); typed `DecisionTable` for `format: 'csv'` and `Expr \| Rule \| RuleSet` for `'text'` |
| `dumps(value, { format, omitActions }?)` | the canonical document of a condition, rule, rule set or table |
| `load(path, { format, limits, actions }?)` | `loads` of a file: a promise; the format comes from the extension |
| `dump(value, path, { format, omitActions }?)` | `dumps` to a file: a promise; a rule text ends with a line break |
| `ruleFromText(text, { do, actions, limits }?)` | one rule in the text syntax (fuzzytool's verb); `rule_1` without a `RULE` line |

Formats are `'json'` (the default for `loads` and `dumps`), `'yaml'`, `'text'`
(the rule syntax, `spec/text.md`) and `'csv'` (decision tables,
`spec/tables.md`); a file's extension (`.json`, `.yaml`, `.yml`, `.rules`,
`.csv`) gives its format unless `format` says, and an unknown one is a
`RangeError`. Reading is the core's, strict and bounded: errors have a JSON
Pointer or a line and column, and `limits` are the native layer's (`maxSize`,
`maxNesting`, `maxDepth`, `maxTableSize`, `maxYamlSize`, `maxTextSize`). A
file larger than its format's limit is not read. A rule's action is the name
the document gives it, unbound unless `actions` (an object or a `Map` of
functions) binds it, as `RuleSet.bind` does; `ruleFromText` also takes `do`, a
function, which a name in the text (`DO notify`) is bound to. Rules and rule
sets read from documents are the core's own, so they write back exactly (a
float `1.0` in metadata stays a float). A rule whose action is code cannot be
written (`UnsupportedError`, with the rule's `pointer`) unless `omitActions`
(`null` is `false`);
the text syntax holds neither metadata nor code.

[`docs/examples/risk.mjs`](../../docs/examples/risk.mjs) reads the decision
table of `risk.csv`, builds it again from rows, and reads and writes rules as
documents and text, in Node and Deno.

`load` and `dump` work in Node (through `process.getBuiltinModule`, so the
package imports no Node module) and Deno (`Deno.readFile`, `Deno.writeTextFile`,
with `--allow-read` and `--allow-write`) and take a path or a `file:` URL. In
browsers there is no file system: they reject with an `UnsupportedError` that
names `loads` and `dumps`.

`toJSON()` of a condition, rule, rule set, decision table, decision,
explanation, summary, result tree or error gives its JSON form as an object
for `JSON.stringify`. JavaScript writes the number `1.0` as `1`, which reads
back as an int, so a float that holds an integer is `{"$float": 1}` there, as
`spec/json-format.md` (*Values*) asks of such writers (ints beyond 2⁵³ are
already `{"$int": "…"}` and infinities `{"$float": "inf"}`): `loads` and
`RuleSet.readDecision` read the text back as the same document. Two things a
plain object cannot hold are lost: the sign of a float zero
(`JSON.stringify(-0)` is `0`), and the order of integer-like keys, which come
first (a payload `{ b, "10" }` becomes `{ "10", b }`, and a decision with that
payload no longer matches its rule). `dumps` writes conditions, rules, rule
sets and tables exactly, and so do the native layer's `write()` and
`explain('json')` for decisions and explanations.

## Decision tables

```js
import { DecisionTable, F, TableOutput, decimal } from 'cleanrules'

const table = new DecisionTable(
  ['RULE', 'customer.years', F.order.total, new TableOutput('risk', ['high', 'low']), 'THEN discount'],
  [
    ['vip', '> 5', null, "'low'", decimal('0.15')],
    ['new', '< 1', F.order.total.gt(1000), "'high'", 0],
  ],
  { hitPolicy: 'priority' },
)
table.evaluate({ customer: { years: 9 }, order: { total: 200 } }).output   // { risk: 'low', discount: Decimal { text: '0.15' } }
```

A header item is a CSV header field's text (`'RULE'`, `'PRIORITY'`,
`'DESCRIPTION'`, `'THEN key'`, `"THEN key IN (...)"` or an input expression),
an input `Expr`, or a `TableOutput` (a key and its output values, as
constants). A string cell is read as in the table's CSV, in the text syntax:
`'> 5'`, `'-'`, and `"'VIP'"` for the string, since `'VIP'` is the field named
`VIP`. An `Expr` cell is an input's condition, which must test its column
(`F.age.lt(18)` under `age`), or an output's value; `null` and `undefined` are
empty; any other value is a constant (under an input `x`, the test `x =
value`; under `PRIORITY`, an integer). Rows may be shorter than the header,
and any iterable, read one at a time. Options: `hitPolicy` (a rule set's, or
`'output_order'`), `aggregate` (`null` or left out for none; `''` is an
unknown aggregate, as in rule sets), `logic`, `strict`, and `limits` (`maxSize` for
each text, `maxDepth`, `maxTableSize`). Errors point into the grid
(`context.pointer` `/header/<c>` or `/rows/<r>/<c>`, from 0), also for values
JavaScript cannot give (`NaN`).

A table has `hitPolicy`, `aggregate`, `logic`, `strict`, `inputs` (`Expr`s),
`outputs` (`TableOutput`s), `rows` (`TableRow`s: `name`, `priority`,
`description`, `inputs` and `outputs`, an `Expr` or `null` each), `ruleset`
(the `RuleSet` it compiles into), `evaluate`, `evaluateMany`, `summary` and
`toJSON()`; it is read and written as CSV, JSON or YAML with
`loads`/`dumps`. Python's `DecisionTable.from_dataframe` has no equivalent:
JavaScript has no standard data frame (pass its rows).

## Result trees

```js
import { F, evaluateTree, evaluateValueTree } from 'cleanrules'

const tree = evaluateTree(F.age.ge(18).and(F.country.isIn(['ES'])), { age: 20 })
tree.truth                       // 'unknown'
tree.children[1].reason.code     // 'missing_field'
evaluateValueTree(F.price.mul(F.quantity), { price: 2.5, quantity: 4 }).value   // 10
```

`evaluateTree(condition, data, { logic, strict }?)` and
`evaluateValueTree(value, data, { strict }?)` evaluate one node on native data
and give its `ResultTree` (`evaluateTree` refuses a value node with an
`ExpressionError`): `node` (an `Expr`; a `between` has its two half
comparisons as children), `truth` (for a condition) or `value` (for a value,
given back as above), `reason`, `children`, and `toJSON()`, the result tree of
the JSON format. An action's `ActionContext` has the `tree` of its rule's
condition, made from the input already read.

## Values given back

Outputs, payloads, aggregates, metadata, constants (`Const.value`) and the
values in an explanation (the values a cause read, a counterfactual's target)
come back as `spec/data-model.md` says (*Values given back*), the same in Node,
Deno and browsers:

| Value | JavaScript |
|---|---|
| null, whatever its reason | `null` |
| int within ±(2⁵³ − 1) | `number`; beyond, `bigint` |
| float | `number` (`-0` and infinities included) |
| decimal | `Decimal`, with the canonical text (`inf` and `-inf` included) |
| date, naive datetime | `LocalDate`, `NaiveDateTime` |
| aware datetime | `ZonedDateTime` |
| list | `Array` |
| map | a plain object; a key such as `__proto__` is an own property |
| opaque | the very object read from the input; `null` in a decision read from a document (`readDecision`), which keeps only the type's name |

`ZonedDateTime` holds the local `year`, `month`, `day`, `hour`, `minute`,
`second` and `microsecond`, `offsetSeconds`, `timeZone` (an IANA name, or
`null` for a fixed offset) and `epochMicroseconds` (a `bigint`); `toDate()`,
`toTemporal()` (in its IANA zone or at its offset; a `TypeError` where the
runtime has no Temporal, such as Node 22, and a `RangeError` for an offset with
seconds, since Temporal's offset zones have whole minutes; in a named zone the
Temporal value's local fields follow the runtime's zone database), and
`toString()` and `toJSON()`, the RFC 9557 text of the JSON format
(`2026-10-31T09:30:00+01:00[Europe/Madrid]`). Only the engine makes them.

A counterfactual's gap is a `number`, `bigint` or `Decimal`, like the values
above; between dates a frozen `{ days }`, between datetimes a frozen
`{ microseconds }`, each an int by the rule above. Priorities in notes and
summaries are ints by the same rule.

These classes are frozen, and payloads and outputs are frozen shallowly (the
arrays and objects inside them are not); results and explanations are frozen
too. Each value reads back as the same value, so an output can be fed to
another evaluation, with three exceptions: a plain object never yields the
keys `__proto__` and `constructor` (pass such a map as a `Map`); an opaque value
given back as `null` reads as null; and a float that holds an integer comes
back as a `number`, which reads back as an int (wrap it in `float(x)` where it
matters). What the core cannot hold (a forged marker, an instant beyond its
range) is opaque, and comes back as the object itself.

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

With the idiomatic API, a rule's action is `do`: a function, or a name.
`ruleset.bind(actions)` (an object or a `Map` of functions) returns a rule set
whose named actions are resolved; every named action is checked, whether or
not its rule fires (`RegistryError`), and actions given as functions are kept.
`ruleset.run(data)` refuses unbound names before reading anything, evaluates,
and calls the selected rules' actions in rule order with an `ActionContext`
(`rule`, `data`, `result`, `tree`, `output`, `decision`). `evaluate` never calls
actions. Actions are synchronous: what they return is ignored, except a
promise, which fails the run (and its rejection is handled). An action that
throws or returns a promise stops the run with an `ActionError` whose `cause`
is what it threw, `context.executed` the rules whose actions ran, and
`decision` the decision.

In the native layer, `run(data, actions)` takes an object or a `Map` from action names to functions.
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
