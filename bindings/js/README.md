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
  `not`, `Expr`, `Rule`, `RuleSet`, `Decision`, `DecisionTable`, `Bundle`, `CaseFileRef`, `loads`,
  `dumps`, `load`, `dump`, `canonicalBytes`, `digest`, `ruleFromText`,
  `evaluateTree`, `evaluateValueTree`, and the classes of explanations,
  summaries, result trees and tables), the errors, the marker values
  (`decimal`, `float`, `date`, `datetime`, and `ZonedDateTime` given back),
  `FORMAT_VERSION` and `version`.
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

Users install `cleanrules` only (Node 22.12 or later). On Node, the addon
ships in one optional package per platform, `@cleanrules/node-<platform>`
([`npm/`](npm)), and npm installs the one whose `os`, `cpu` and `libc` fit the
machine:

| Platform package | System |
|---|---|
| `linux-x64-gnu`, `linux-arm64-gnu` | Linux with glibc 2.28 or later (built on manylinux_2_28, the floor of Node 22's own binaries: RHEL 8, Debian 10, Ubuntu 20.04 and later) |
| `linux-x64-musl`, `linux-arm64-musl` | Linux with musl (Alpine) |
| `darwin-x64`, `darwin-arm64` | macOS on Intel and Apple silicon |
| `win32-x64-msvc`, `win32-arm64-msvc` | Windows on x64 and arm64 |

The package refuses to load an addon of another version, and says to
reinstall it. Where no platform package fits (another system or processor), or
when optional dependencies are left out (`npm install --omit=optional`),
`cleanrules` cannot load an addon on Node and says so; the WebAssembly build,
which is in the package itself, runs there instead with `node
--conditions=browser`, or through `cleanrules/wasm` (*Initialising the
WebAssembly module yourself*, above). Nothing is published yet (task M.2).

### Packing

`npm pack` packs the main package with the WebAssembly module, and refuses
without it or with one older than the Rust sources (`scripts/prepack.js`). CI
(`.github/workflows/node-binaries.yml`) builds every addon (the glibc ones in
manylinux_2_28 containers, checked to need glibc 2.28 at most), packs the nine
packages, checks what each tarball holds, and installs the tarballs into a new
project on every platform. The same scripts run by hand, from `bindings/js`:

```sh
npm run build:wasm
cp cleanrules.darwin-arm64.node npm/darwin-arm64/    # this machine's addon (npm run build:release)
node scripts/check-pack.js --platforms darwin-arm64              # npm pack --dry-run, and the checks
node scripts/check-pack.js --platforms darwin-arm64 --out dist   # the tarballs, checked
node scripts/install-packed.js dist                              # a new project from them, and the smoke scripts
```

`scripts/check-pack.js` checks each package against an explicit list: the
main package holds its entries and declarations, `binding.cjs`, `lib/`,
`wasm/cleanrules.js` and `wasm/cleanrules_bg.wasm`, README, LICENSE and NOTICE,
and no tests, sources, addons or source maps; each platform package holds
exactly its addon (a binary of its system and processor; for glibc, needing
2.28 at most), README, LICENSE and NOTICE; every package has one version.
Without `--platforms` it checks all eight platform packages.
`scripts/install-packed.js` installs the tarballs as a user would, without the
registry: `cleanrules` as a `file:` dependency and each platform package
through `overrides` with a `file:` spec, under `npm install --offline` with a
cache of its own (npm still picks the platform package by `os`, `cpu` and
`libc`). Then it imports and requires `cleanrules` and `cleanrules/native`,
evaluates a rule, checks that the addon came from the installed platform
package and from nowhere else, and that `node --conditions=browser` runs the
WebAssembly build (`scripts/smoke/`).

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
| `Document.read(text, { format, limits }?)` | a condition, rule, rule set, decision table or bundle; `kind`, `write(format?)`, `canonicalBytes()`, `digest()` |
| `Document.makeBundle(rules, id, revision, cases?, limits?)`, `doc.compareBundle(other)` | a bundle of a rule set or table document, with `cases` an array of `{ path, content }`; the comparison of two bundles as JSON (*Bundles*, below) |
| `RuleSet.read(text, { format, limits }?)`, `RuleSet.fromDocument(doc)` | a rule set (a rule document is a rule set of that rule; a CSV table is compiled) |
| `ruleset.evaluate(data, limits?)`, `evaluateJson(text, limits?)` | one decision, from native data (at most `limits.maxValues` values of it) or from a JSON value |
| `ruleset.evaluateMany(rows, limits?)`, `evaluateManyJson(text, limits?)` | a batch; an error names its `row` |
| `ruleset.run(data, actions, limits?)` | evaluate, then call the selected rules' actions, in rule order |
| `ruleset.readDecision(text, { format, limits }?)` | a recorded decision, checked against the rule set |
| `ruleset.write(format?)`, `summary(format?)` | the document (JSON by default), and the summary (text by default, or JSON or YAML) |
| `ruleset.canonicalBytes()`, `digest()` | the canonical bytes of its document (a `Uint8Array`) and the rule set digest (*Canonical bytes and digests*, below) |
| `ruleset.length`, `ruleNames()`, `hitPolicy`, `aggregate`, `logic`, `strict`, `fieldPaths()` | what it is and what it reads |
| `decision.length`, `ruleName(i)`, `firedAt(i)`, `truth(i)`, `selected()` | each rule's result |
| `decision.outputJson()`, `outputsJson()`, `ruleOutputJson(i)` | outputs, in the JSON format; `null` for none |
| `decision.write(format?, { rulesetDigest }?)`, `explain(format?)` | the decision document (JSON by default; with `{ rulesetDigest: true }`, its rule set's digest), and its explanation (text by default, or JSON or YAML) |
| `Node.read(text, { format, limits }?)` | one condition or value node; `write`, `fieldPaths`, `evaluateTree(data, { logic, strict, limits }?)`, `evaluateValueTree(data, { strict, limits }?)`, and `evaluateTreeJson(text, { logic, strict, limits }?)` and `evaluateValueTreeJson(text, { strict, limits }?)` |

Formats are `"json"`, `"yaml"`, `"text"` (the rule syntax) and `"csv"`
(decision tables). Texts are strings or bytes (`Uint8Array`, `Buffer`); bytes
that are not UTF-8 are a `FormatError` at their line and column. `limits` is an
object with any of `maxSize` (bytes, for every format), `maxNesting`,
`maxDepth`, `maxTableSize`, `maxYamlSize`, `maxTextSize`, `maxPathSegments`
and `maxValues`. The defaults (explanations and summaries as text) are those
of C++. `maxPathSegments` (256 by default) bounds the segments of a field path
in every format, so the data read along a path stays shallow; `F` and `field`
check the default, and beyond it the error is a `FormatError` with code
`limit_exceeded` and `context.name` `max_path_segments`. `maxValues`
(1,000,000 by default) bounds what is read from native data, the only limit
that applies to it: each value placed where a path ends or stops counts, with
everything inside it, an object referenced from several places each time, and
a list along the paths counts its length, so data shared as a "diamond" or an
array with a huge `length` cannot exhaust memory. An input beyond it is
refused, not evaluated, in every mode: a `FormatError` with the code
`limit_exceeded`, `context.name` `'max_values'` and, in a batch,
`context.row` (`spec/data-model.md`, *Reading native data*, *Limits*).
Constants and rule metadata are converted under the default limit too, and a
larger one is the same error, with the `rule` when there is one.

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
| `round(scale, { mode })` | an int or a decimal rounded to `scale` places (0 to 38), as a `Decimal`; `mode` is `'half_even'` (default), `'half_up'`, `'half_down'`, `'up'`, `'down'`, `'ceiling'` or `'floor'` |
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
- A key named like a method (`and`, `path`, `tag`, `round`...) or like a member of
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
new Rule(name, when, then?, { id, priority, tags, description, metadata, do }?)
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

`id` identifies a rule across versions of its rule set
([`spec/rules.md`](../../spec/rules.md), *Identity*): a non-empty string,
taken literally (`RuleDefinitionError` otherwise). A rule without one is
identified by its name; `rule.effectiveId` is the id, or the name. Effective
ids are unique within a rule set, so `new RuleSet` refuses two rules with the
same id, or an id equal to the name of a rule without one, with a
`RuleDefinitionError` naming the second rule (two rules may swap ids: `a` with
id `b` and `b` with id `a`). The id changes nothing else: decisions,
explanations and summaries name rules by name, and `get(name)` and
`rule(name)` look them up by name. A renamed rule needs an explicit id (its
old name, if it had none) to stay the same rule: comparing two versions of a
rule set, which comes in a later release, shows a renamed rule without an id
as one rule removed and another added.

```js
// Renamed from `vip`: the id keeps it the same rule.
const gold = new Rule('gold_customer', F.customer.years.gt(5), { discount: 0.15 }, { id: 'vip' })
gold.id           // 'vip'
gold.effectiveId  // 'vip'
const loyal = new Rule('loyal', F.customer.years.gt(2), { discount: 0.05 })
loyal.id           // null
loyal.effectiveId  // 'loyal'
new RuleSet([gold, new Rule('vip', F.customer.vip)])  // RuleDefinitionError: rule "vip", which has no id, repeats the id of rule "gold_customer"
```

Documents keep the id: the key `id` right after `name` in JSON and YAML, and
`ID` after the rule's name in the text syntax (`RULE gold_customer ID vip
PRIORITY 10`).

| Name | What |
|---|---|
| `rule.name`, `id`, `effectiveId`, `when`, `then`, `priority`, `tags`, `description`, `metadata`, `do`, `actionName` | the rule, as the core keeps it (tags sorted, without repeats; `id` is `null` without one) |
| `ruleset.rules`, `length`, iteration, `get(name)`, `has(name)`, `rule(name)` | the rules; `get` gives `undefined` for an unknown name, like `Map`, and `rule` throws a `RuleNotFoundError` listing them |
| `ruleset.hitPolicy`, `aggregate`, `logic`, `strict`, `fieldPaths()` | its configuration and what it reads |
| `ruleset.evaluate(data, { limits }?)`, `evaluateMany(rows, { limits }?)`, `readDecision(text, { format, limits }?)` | decisions; `limits.maxValues` bounds the values read from each input (above) |
| `ruleset.bind(actions)`, `run(data, { limits }?)` | actions (below) |
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
| `loads(text, { format, limits, actions }?)` | an `Expr`, `Rule`, `RuleSet`, `DecisionTable` or `Bundle` from a document (a string or UTF-8 bytes); typed `DecisionTable` for `format: 'csv'` and `Expr \| Rule \| RuleSet` for `'text'` |
| `dumps(value, { format, omitActions }?)`, `dumps(decision, { format, rulesetDigest }?)` | the canonical document of a condition, rule, rule set, table, bundle or decision; `rulesetDigest` for decisions only (a `TypeError` otherwise, and a type error in TypeScript) |
| `load(path, { format, limits, actions }?)` | `loads` of a file: a promise; the format comes from the extension |
| `dump(value, path, options?)` | `dumps` to a file: a promise; a rule text ends with a line break |
| `ruleFromText(text, { do, actions, limits }?)` | one rule in the text syntax (fuzzytool's verb); `rule_1` without a `RULE` line |

Formats are `'json'` (the default for `loads` and `dumps`), `'yaml'`, `'text'`
(the rule syntax, `spec/text.md`) and `'csv'` (decision tables,
`spec/tables.md`); a file's extension (`.json`, `.yaml`, `.yml`, `.rules`,
`.csv`) gives its format unless `format` says, and an unknown one is a
`RangeError`. Reading is the core's, strict and bounded: errors have a JSON
Pointer or a line and column, and `limits` are the native layer's (`maxSize`,
`maxNesting`, `maxDepth`, `maxTableSize`, `maxYamlSize`, `maxTextSize`,
`maxPathSegments`). A file larger than its format's limit is not read. A rule's action is the name
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
`RuleSet.readDecision` read the text back as the same document. One thing a
plain object cannot hold is lost: the sign of a float zero
(`JSON.stringify(-0)` is `0`). Plain objects also list integer-like keys
first (a payload `{ b, "10" }` is written `{ "10", b }`), which changes
nothing: the members of a JSON object have no order, and payloads and maps
come back sorted by key, by Unicode code point. `dumps` writes conditions, rules, rule
sets, tables, bundles and decisions exactly, and so does the native layer's
`explain('json')` for explanations.

## Canonical bytes and digests

```js
import { canonicalBytes, digest } from 'cleanrules'

const bytes = canonicalBytes(rules)   // a Uint8Array
const id = digest(rules)              // 'sha256:5a0e590c…', the rule set digest
digest(table)                         // the table's own document
digest(table.ruleset)                 // the rule set digest of the table
```

| Name | What |
|---|---|
| `canonicalBytes(value, { omitActions }?)` | the canonical bytes of the document of a condition, rule, rule set, table or bundle: a `Uint8Array` |
| `digest(value, { omitActions }?)` | their SHA-256 digest, `sha256:` and 64 lowercase hexadecimal digits |

The canonical bytes (`spec/json-format.md`, *Canonical bytes*) are the UTF-8
JSON text of the canonical form that `dumps` writes, with every object's
members sorted by key, by Unicode code point, no whitespace, minimal string
escapes and one layout for each number; they are the same in every language,
and `loads` reads them back. The digest (*Digests*) is the SHA-256 hash of
those bytes, a string that compares equal across languages. They take the
same values and give the same errors as `dumps`: a value such as `F.x.add(1)`
is not a document (`ExpressionError`), and a rule whose action is code is an
`UnsupportedError` with the rule's `pointer` unless `omitActions`, which gives
those of the document written without it.

A digest identifies **one concrete canonical artifact**; it does not say that
two rule sets decide alike. The same document in another layout, member order,
number layout (`1e2`, `100.0`) or format (JSON, YAML, the rule syntax) has
the same digest; anything else gives another one, even when the rules decide
the same: `x > 5` and `5 < x`, the decimals `1.0` and `1.00`, or another
description. Telling whether two versions decide alike is the work of tests,
not digests.

The **rule set digest** (*Rule set digests*) is `digest(ruleset)`: that of a
rule is `digest(new RuleSet([rule]))`, and that of a decision table
`digest(table.ruleset)`, the rule set it compiles to, while `digest(table)`
is the table's own document. A decision may carry it, under
`ruleset_digest`: `ruleset.readDecision` checks it against the rule set (a
`FormatError` with code `decision_mismatch` at `/decision/ruleset_digest`
when it differs) and keeps it, so the decision writes back with it. A decision
made in JavaScript carries it when it is written with `dumps(decision, {
rulesetDigest: true })` (or `dump`), so a log of decisions is tied to the rules
that made them, and to the bundle that holds them, by a string comparison;
`toJSON()` writes it only for a decision read with one. Such a document is
read back with `ruleset.readDecision`, not `loads`: a decision is read with
its rule set. In the native layer, `Document` and `RuleSet`
have `canonicalBytes()` and `digest()`, so `RuleSet.fromDocument(doc).digest()`
is the rule set digest of any document but a condition.

## Bundles

```js
import { Bundle, dump, load } from 'cleanrules'

const bundle = new Bundle(rules, {
  id: 'acme/discounts',
  revision: '2026-11-16.1',
  cases: [{ path: 'cases/vip.json', content: await readFile('cases/vip.json') }],
})
await dump(bundle, 'discounts.bundle.json')
const kept = await load('discounts.bundle.json')
kept.compare(bundle)        // { relation: 'same_rules_same_engine', identical: true, differences: [] }
kept.caseDigestMatches('cases/vip.json', await readFile('cases/vip.json'))  // true
```

A bundle (`spec/bundles.md`) is a reproducible package of rules: the rules
themselves (a rule set or a decision table), the identifier and revision their
author gives them, their rule set digest, the engine and limits that made it,
the contract of their input and output, and the digests of their files of test
cases. Making the same bundle twice gives the same bytes and the same
`digest(bundle)`: the engine never invents an identifier or a revision, and
reads no clock.

| Name | What |
|---|---|
| `new Bundle(rules, { id, revision, cases, omitActions, limits })` | a bundle of a `RuleSet` or a `DecisionTable` with this engine; `cases` an iterable of `{ path, content }` (a string or a `Uint8Array`), whose digests it records in order; `limits` those the rules run with, which it records (`maxSize`, `maxNesting`, `maxDepth`, `maxPathSegments`, `maxTableSize`, `maxValues`; the defaults for those left out) |
| `bundle.id`, `revision` | the identifier, such as `acme/discounts`, and the revision, such as `2026-11-16.1` |
| `bundle.rules`, `ruleset`, `rulesetDigest` | the rules it holds (a `RuleSet` or a `DecisionTable`, actions as unbound names), the rule set it gives, and that rule set's digest |
| `bundle.engine` | `{ name, version, limits, isCurrent }`, frozen: the engine that made it, its exact version, the six limits by the names of the `limits` option, and whether it is this engine (same name and version) |
| `bundle.contract` | `{ input, output: { keys } }`, frozen: the JSON Schema of the input and the sorted output keys |
| `bundle.cases` | its `CaseFileRef`s, frozen, in order: each with `path`, `digest` and `digestMatches(content)` |
| `bundle.caseDigestMatches(path, content)` | whether `content` is the case file the bundle was made with (a `RangeError` for a path it does not refer to); the engine never opens files |
| `new CaseFileRef(path, digest)` | a reference to a case file, its path and digest checked by the core (`FormatError`); `caseFileRef.digestMatches(content)` |
| `bundle.compare(other)` | `{ relation, identical, differences }`, frozen (*Comparing bundles*) |
| `loads`, `load`, `dumps`, `dump`, `canonicalBytes`, `digest`, `toJSON()` | as for any document, in JSON or YAML |

An `id` is one or more `/`-separated segments of lowercase letters, digits,
`.`, `_` and `-`; a `revision` letters, digits, `.`, `_`, `+` and `-`; a case
path stays inside the bundle's directory. Anything else is a `FormatError`, as
is a case path given twice. Rules whose actions are code are refused
(`UnsupportedError`) unless `omitActions`, as `dumps` refuses them. A limit is
a whole number from 1 to 2^53 − 1 (`Number.MAX_SAFE_INTEGER`): 0, or one above it, is an
`UnsupportedError`, since a bundle cannot record it, and a negative or
fractional one a `RangeError`; giving `maxYamlSize` or `maxTextSize`, which
bound the reading of texts and not decisions, is a `TypeError`.

A case file is a document of test cases (`spec/tests.md`,
`docs/examples/cases/discounts.json`): each case has an id, an input, and what
evaluating it must give (the selected rules, the output, the rules left
unknown, or the error). The bundle records the digest of its bytes; `loads`
does not load case files yet (`UnsupportedError`), while `Document.read` of
`cleanrules/native` reads, writes and hashes one as a document.

Reading a bundle checks its rule set digest against its rules, and, when it
names this engine (`engine.isCurrent`), its contract against the one the
engine derives (a `FormatError` with code `decision_mismatch` either way). A
bundle of another engine or version is read as it is, never refused: `version`
is the version of this engine.

`compare` tells the two questions apart: `same_rules_other_engine` (the same
rule set digest, another engine version or other limits) and
`other_rules_same_engine`; `conflict` is one `id` and `revision` naming two
sets of rules; the comparison is symmetric. `differences` names what
differs, in the specification's order (`id`, `revision`, `rules`, `source`,
`engine.name`, `engine.version`, `engine.limits.max_depth`...,
`contract.input`, `contract.output`, `cases`; the type `BundleDifference`).
Difference names are data, as relation names are, so they keep the format's
spelling in every language: the difference `engine.limits.max_depth` is about
the limit that `limits` and `bundle.engine.limits` call `maxDepth`. Two
bundles are the same exactly when `digest(a) === digest(b)`.

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
table.evaluate({ customer: { years: 9 }, order: { total: 200 } }).output   // { discount: Decimal { text: '0.15' }, risk: 'low' }
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

`evaluateTree(condition, data, { logic, strict, limits }?)` and
`evaluateValueTree(value, data, { strict, limits }?)` evaluate one node on native data
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
| map | a plain object, its keys added by Unicode code point; a key such as `__proto__` is an own property |
| opaque | the very object read from the input; `null` in a decision read from a document (`readDecision`), which keeps only the type's name |

Arithmetic keeps the kind its operands give it (`spec/ast.md`,
*Arithmetic*): with a `decimal(...)` operand and no float, the result is a
`Decimal`, not a `number`, so `F.price.mul(decimal('0.85'))` on
`decimal('19.99')` gives `Decimal` `16.9915`, and `.round(2)` gives `16.99`.
A plain JavaScript number with a fraction, such as `0.85`, is a float, and
makes the result a `number`; `round` on a float is unknown with a type
mismatch. Read a `Decimal` with `String(d)` or `d.toJSON()`, never with
`Number(d)` where cents matter.

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
| `Map` | map (its order means nothing); entries whose key is not a string are left out |
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
