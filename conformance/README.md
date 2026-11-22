# Conformance vectors

The vectors in this directory are the executable form of the specification
(`spec/`). The Rust core and every binding run all of them, and must produce
exactly the expected output (`CLAUDE.md`, invariant 5). When a vector and an
implementation disagree, the spec decides which one is wrong.

```text
conformance/
  vectors/<area>/<topic>.json   the vectors, grouped by spec area
  tools/check.py                the vector checker
  tools/decimal_vectors.py      the decimal oracle: writes and checks vectors
  tools/canonical_vectors.py    the canonical-bytes oracle: writes and checks vectors
```

## Vector files

A vector file is a JSON document:

```json
{
  "version": 1,
  "area": "ast",
  "vectors": [
    {
      "id": "ast/truth/and-false-unknown",
      "spec": ["ast.md#truth-values"],
      "description": "FALSE and UNKNOWN is FALSE, without a reason",
      "given": { "kind": "condition", "condition": { "...": "..." }, "input": { "a": false } },
      "expect": { "result": { "...": "..." } }
    }
  ]
}
```

- `version`: the version of this vector format, `1`. It is independent of the
  JSON format version, which appears in the documents inside the vectors.
- `id`: unique across all files, `<area>/<topic>/<name>`, lowercase with
  hyphens. An id never changes meaning; a vector that becomes wrong is fixed or
  removed, and its id is not reused.
- `spec`: the spec sections the vector checks, as `<file>#<heading anchor>`
  (GitHub's anchors: lowercase, spaces as hyphens, punctuation dropped).
- `description`: one sentence saying what the vector shows.
- `given` and `expect`: below. All values inside them use the JSON format's
  encoding (`spec/json-format.md`, *Values*).

## What is given

`given.kind` says what the runner does:

| Kind | Given | Runner calls | Expected |
|---|---|---|---|
| `read` | `text` (a JSON text, for lexical cases), `yaml` (a YAML text, `json-format.md`, *YAML*) or `document`; `ruleset` when reading a decision; optional `limits` (`max_size` in bytes, `max_nesting`, `max_depth`, `max_table_size`, `max_path_segments`); optional `current_engine` (below) | the document reader | `document`: the canonical form that is read back, or `error` |
| `condition` | `condition` (a condition node), `input` (a data value), optional `logic` and `strict` | `evaluate_tree` (`spec/ast.md`, *Evaluating one node*) | `result`: its result tree, or `error` |
| `value` | `value` (a value node), `input`, optional `strict` | `evaluate_value_tree` | `result`: its result tree, or `error` |
| `evaluate` | `ruleset` (a rule set document's content), and `input` or `inputs` (a batch); optional `limits` (`max_values`, `spec/data-model.md`, *Reading native data*, *Limits*) | `evaluate`, or its batch form | `decision`, or `decisions` for a batch, or `error` |
| `explain` | `ruleset`, `input` | `evaluate`, then `explain` | `decision` (which `at` pointers refer to) and `explanation`, optionally `text` (the exact text of `explain()`), or `error` |
| `summary` | `ruleset` | `summary` | `summary`, and optionally `text` |
| `tool` | `rulesets` (an object: each name with a rule set document's content; none for an empty catalog), `tool`, and `arguments` (JSON) or `arguments_text` (JSON text, for texts a vector cannot hold, such as repeated keys) | the tool call on a catalog of those rule sets (`spec/agents.md`) | `output`: the result, or `error`: the tool error |
| `canonical` | `document` (a condition, rule, rule set or decision table document), or `text` (its JSON text, for lexical cases) | the reader, then the document's canonical bytes and digest, and the digest of the rule set it gives (`spec/json-format.md`, *Canonical bytes*, *Digests*) | `canonical`: the exact canonical bytes as UTF-8 text, compared as a string; `digest`; `ruleset_digest`, absent for a condition |
| `bundle` | `document` (a rule set or decision table document: the rules), `id`, `revision`; optional `cases` (an array of `{"path", "text"}`: each case file's path and content as UTF-8 text) and `limits` (any of the six that a bundle records, `spec/bundles.md`, *Engine*) | make a bundle of the rules with this engine (`spec/bundles.md`, *Making a bundle*), write it, and read it back | `document`: the bundle, whose `engine.version` the runner first sets to its own engine's; or `error` (its `code` only) |
| `bundle_compare` | `a`, `b`: two bundle documents | read both, then compare `a` with `b` (`spec/bundles.md`, *Comparing bundles*) | `comparison`: `relation`, `identical` and `differences` |
| `audit` | `document` (a rule set or decision table document); optional `limits` (`max_terms`, `max_steps`, `spec/audit.md`, *Limits*) and `ruleset_digest` (`true`: ask for the digest in the report) | `audit` or `audit_table` (`spec/audit.md`) | `audit`: the report without the `message` and `witness` of each diagnostic (below) |
| `draw` | `format` (`dot`, `mermaid`, `html` or `html_fragment`), `document` (a condition, rule, rule set or decision table document); optional `input`, with a rule set or a table; optional `limits` (`max_diagram_size`) | `draw_document` (`spec/visual.md`); with an `input`, the rule set (or the table's rule set) evaluates it, and the runner draws the decision's explanation for a rule set (`draw_explanation`), and the table with the decision for a table (`draw_document` with the decision) | `diagram`: the exact text, compared as a string, or `error` (`code` and `name`) |

Optional flags such as `strict` are omitted when false. A bundle's `engine.version`
says which engine made it, and a reader verifies the contract of a bundle of
its own engine only (`spec/bundles.md`, *Contract*), so the vectors never name
a real version: the bundles of other engines name pre-release versions such as
`1.0.0-conformance.1`, which no build has, and a `read` vector with
`"current_engine": true` holds a bundle of the engine that runs it: the runner
sets `engine.version` of `given.document` and of `expect.document` to its
engine's version (`cr_version()`, `cleanrules.__version__`, ...) before
running it. A `bundle` vector does the same with `expect.document`. A `condition` vector's
`logic` is a logic in its JSON form (`spec/json-format.md`, *Logics*): a name,
or an object with the name, a parameter and a complement. Until the bindings
evaluate one node under a graded logic (task J.1b), only the core runs the
vectors whose `logic` is not `"boolean"` (`ast/logics.json`); every binding's
runner, the C ABI's and the C++ one skip them on every route, counting and
listing them. An `evaluate` vector
with `limits` checks `max_values`, which bounds what is read from native data
only: runners run it with the language's native data (`native:<id>`) and skip
it in their JSON runs, and the core, C ABI and C++ runners, which have no
native data, skip it and count the skips. An `error` holds the
error's `code` and the compared context fields that apply, such as
`{"code": "missing_field", "rule": "vip", "path": ["customer", "age"]}`;
runners compare errors as `spec/errors.md` (*Comparing errors*) says.

## Comparing

Runners compare the actual output with `expect` as parsed JSON trees, as
`spec/json-format.md` (*Comparing documents*) defines: objects by their
members whatever their order (*Objects*), arrays item by item in order, numbers
by lexical class and exact value, and the sign of a float zero
(use `copysign` or its equivalent: `-0.0 == 0.0` in most languages). Some parts are not compared,
because the spec says that they are informative:

- `detail` in reasons is removed from both sides before comparing;
- any two opaque values (`{"$opaque": ...}`) are equal, whatever their type
  name;
- errors are compared by code and compared fields only (`spec/errors.md`).
- in `audit` vectors, the `message` and the `witness` of each diagnostic are
  removed from the actual report before comparing. The runner checks each
  witness instead, with the rule set's own evaluation, as `spec/audit.md`
  (*Witnesses*) says: an `overlap` of severity `error` raises
  `hit_policy_error` naming both its rules; any other `overlap` gives a
  decision in which both fire; a `gap` gives a decision in which no rule
  fires; and those two codes, and only those, have one;
- in `tool` vectors, an agent error (a tool error, or the `error` of a
  `validate` result) must have a non-empty `hint`, and is compared on every
  member of its JSON form except `message`, `hint`, `cause`, `node`,
  `operation` and `reason`, which are dropped from the actual error: so
  `pointer`, `name` and `available` are compared too.

Runners must parse the vectors without losing what is compared. A generic
parser that turns `1.0` into `1` or reads integers through floats cannot be
used as is: JavaScript needs a lossless parser (or `JSON.parse` with access to
the source text), and Go a decoder with `UseNumber`. Member order is never
compared, so a parser may reorder object members (plain JavaScript objects
move integer-like keys such as `"10"` first, and Go's `map[string]any` has no
order).

A runner reports every failing vector with its id, the expected and the actual
output, never stopping at the first one.

## Runners

- **Core**: `cargo test -p cleanrules --test conformance` reads the documents
  and data with the core's own JSON reader, and runs the `draw` vectors.
- **Python**: `cd bindings/python && uv run pytest tests/test_conformance.py`
  runs every vector through the binding's JSON entry points (`json:<id>`) and,
  except the `read` vectors other than those of bundles, with native Python data and the Python API
  (`native:<id>`).
- **C ABI**: `cargo test -p cleanrules-ffi --test conformance` runs every
  vector through the C functions, from JSON and from native `cr_value`s;
  `draw` vectors through `cr_document_draw` and the function of the
  document's content (`cr_node_draw`, `cr_rule_draw`, `cr_ruleset_draw`,
  `cr_decision_draw`).
- **Canonical bytes**: every runner (core, C ABI, Python, C++, JavaScript in
  Node, wasm and Deno, Java, Scala and Go) runs the `canonical` vectors: the
  bytes are compared exactly, as a string, after a strict UTF-8 decoding. The
  JSON route reads the document with the language's reader; where a binding
  builds documents in code, its native route also builds the condition, rule
  or rule set from the vector's document (a decision table is read). Erlang
  has no conformance runner yet (task N.4).
- **Bundles** (`spec/bundles.md`): the `read` and `canonical` vectors of
  bundles go through each runner's document reader like the others, and the
  core, C ABI, Python and JavaScript runners also make bundles (`bundle`) and
  compare them (`bundle_compare`). The C++, Java, Scala and Go APIs have no
  bundles of their own yet (they are beta, `docs/PLAN.md`, R.5): their
  `Loads`/`loads` refuse a bundle with `unsupported` once the core has read
  it, so the vectors that read a bundle with an error still run there. C++
  reads bundles as `cr::document` and Java's native layer as
  `ffi.Document`, so they also run the `read` vectors of bundles (C++ with
  `current_engine` too) and Java and Scala every `canonical` vector of a
  bundle through `ffi.Document`. What they cannot run (making and comparing
  bundles; the vectors with `current_engine` in Go, Java and Scala; reading a
  bundle without an error in Go and in the idiomatic Java and Scala routes) is
  counted and skipped with the reason `bundle`.
- **Audits** (`spec/audit.md`): the core's runner runs the `audit` vectors.
  The bindings audit from task T.9 (Python and JavaScript; Java, Scala, Go
  and C++ are beta): until then the C ABI, Python, C++, JavaScript, Java,
  Scala and Go runners skip them in every route, count them and say why
  (`audit` in the JavaScript, Go and JVM reports).
- **Diagrams in the bindings**: until each binding draws diagrams (phase V.2),
  the Python, C++, JavaScript, Java, Scala and Go runners skip the `draw`
  vectors, count them and state the reason (`diagram` in the JavaScript, Go
  and JVM reports); the text they would give is the core's, which the core and
  C ABI runners check.
- **C++**: `ctest --test-dir build/cpp -R conformance`, after configuring
  `bindings/cpp` with CMake, runs every vector through `cleanrules.hpp` with
  the JSON entry points and the overloads that throw (`json:<id>`; `read`
  vectors also with `error_info`) and, except the `read` vectors, with input
  built by the value constructors and the `error_info` overloads
  (`native:<id>`). Rule sets and nodes are read from their JSON in both runs:
  the C++ API builds them from documents and text only. It compares with a
  lossless JSON parser of its own (`bindings/cpp/tests/json.hpp`).
- **JavaScript**: `npm --prefix bindings/js test` (the Node addon),
  `run test:wasm` and `run test:deno` (the WebAssembly module, in Node and
  Deno) run every vector through the native layer's JSON entry points
  (`json:<id>`; decisions also through its readers) and through the idiomatic
  API with native JavaScript data (`native:<id>`): conditions, rules and rule
  sets are built in code and checked against the core's reading of the same
  JSON, results are compared as the values given back to JavaScript
  (`spec/data-model.md`, which lets a float holding an integer come back as an
  int, and a map's integer-like keys come first), and the same native data
  also goes through the native layer, whose JSON is compared exactly. `read`
  vectors go through `loads`, `dumps`, `ruleFromText`, `DecisionTable` and
  `readDecision`. `JSON.stringify` of what `toJSON()` gives is checked too,
  and read back with `loads` and `readDecision`. Grids are native data, so
  `table_grid` vectors have no JSON run; skips and inexact native inputs are counted and listed. It compares
  with a lossless JSON reader of its own (`bindings/js/test/support/json.js`).
- **Java**: `cd bindings/jvm && ./gradlew test` (alone:
  `--tests '*ConformanceTest'`) runs every vector through the native layer
  (`io.github.cleanrules.ffi`) with JSON text (`json:<id>`; decisions also
  through its readers) and through the idiomatic API with Java data
  (`native:<id>`): conditions, rules and rule sets are built in code and
  checked against the core's reading of the same JSON, inputs are Java values
  (`Long`, `Integer`, `Short` or `BigInteger` in turn, `Double`, `BigDecimal`,
  `LocalDate`, `LocalDateTime`, `Instant`, `OffsetDateTime` or `ZonedDateTime`
  in turn for `Z`, `ZonedDateTime` in a zone, lists, `LinkedHashMap`), results
  are compared as the values given back to Java (`spec/data-model.md`, which
  gives a decimal infinity back as a `Double` infinity and a datetime in a
  zone Java does not know at its offset) and as their `toJson()`, and the same
  Java data also goes through the native layer, whose JSON is compared
  exactly. `read` vectors go through `Documents.loads`, `dumps`,
  `ruleFromText`, `DecisionTable` and `RuleSet.readDecision`. Grids are Java
  data, so `table_grid` vectors have no JSON run; skips are counted and listed
  in `java/build/reports/conformance/conformance.txt`. It compares with a
  lossless JSON reader of its own (`bindings/jvm/java/src/testFixtures/java/io/github/cleanrules/tests/TestJson.java`).
- **Scala**: `cd bindings/jvm && ./gradlew test` also runs every vector
  through the Scala layer (`scala:<id>`; alone:
  `./gradlew :scala:test --tests '*ScalaConformanceSuite'`), with the Java
  runner's engine (the Java project's test fixtures): conditions, rules and
  rule sets built with `F`, the operators, `Rule(...)` and `RuleSet(...)` and
  checked against the core's reading of the same JSON; inputs as Scala data
  (`Long`, `Int` or `BigInt` in turn, `Double`, `BigDecimal`, `java.time`,
  `None` for null, `Some`, `Vector`, `List`, `Array`, `VectorMap`, `ListMap`,
  `SeqMap`, a plain `Map` when the keys are sorted, case classes); results
  compared as the Scala values given back (`spec/data-model.md`) through the
  layer's case classes and as their `toJson`. `read` vectors go through
  `loads`, `dumps`, `ruleFromText`, `DecisionTable(...)` with Scala cells
  (grids included) and `readDecision`. Skips are counted and listed in
  `scala/build/reports/conformance/conformance.txt`.
- **Go**: `cd bindings/go && go test -count=1 ./...` (alone:
  `go test -count=1 -run '^TestConformance' .`) runs every vector with JSON
  text (`json:<id>`: `Loads`, `Dumps`, `ReadDecision`, and the package's
  unexported JSON entry points for inputs, as Python's private ones; decisions
  also through their methods) and with Go data (`native:<id>`): conditions,
  rules and rule sets are built in code and checked against the core's
  reading of the same JSON; inputs are Go values (every integer type,
  `big.Int`, `float32` and `float64`, `Decimal`, `Date`, `DateTime`,
  `time.Time`, slices, arrays, maps and structs, named types and pointers, in
  turn); results are compared as the Go values given back
  (`spec/data-model.md`, which gives a map back as a `map[string]any`, the zone `UTC`
  as `time.UTC` and a zone Go does not know at its offset) and as their JSON
  forms. `read` vectors go through `Loads`, `Dumps`, `RuleFromText`,
  `NewDecisionTable` and `ReadDecision`. Grids are Go data, so `table_grid`
  vectors have no JSON run, and the vector with a limit of 0 has no Go run (0
  keeps the default in Go); skips are counted and listed in
  `bindings/go/build/reports/conformance.txt`. It compares with a lossless
  JSON reader of its own (`bindings/go/conformance_json_test.go`).
- **`tool` vectors** (the tools for agents, `spec/agents.md`) run in the core
  and in Python (`cleanrules.Catalog`, with rule sets read from JSON and
  arguments as JSON text in `json:<id>`, and with rule sets built with the
  Python API and arguments as Python data in `native:<id>`). The C ABI, C++,
  JavaScript, Java, Scala and Go runners do not have the tools yet: they skip
  these vectors in every route, count them and say why (`docs/PLAN.md`, O.1).
- **Each binding** runs every vector twice: once through its JSON entry points,
  and once converting `input` into the language's native data (with the
  conversions of `spec/data-model.md`: floats as floats, with `cr.float` in
  JavaScript, decimals as the language's decimal, maps as the language's map)
  and calling the idiomatic API. The second run tests the binding's own code,
  its data conversion. Maps, like every object, are compared without regard to
  member order.

## Coverage

Every section of a normative spec document is cited by at least one vector,
directly or through one of its subsections. Exempt are the sections whose first
line is marked *Informative* or *Not checked by vectors* with the reason
(binding tests, the core's snapshot tests, a later phase); the exemption covers
their subsections. `check.py` checks this, and CI runs it.

## Tools

```bash
uv run --no-project --with jsonschema python conformance/tools/check.py
```

`check.py` validates every vector file against `vectors.schema.json` and the
spec's JSON Schemas, checks that ids are unique, that every cited spec section
exists and that every normative section is cited, sanity-checks the error
locations of `read` vectors, and validates the expected result of each `tool`
vector without an agent error against `spec/schemas/v1/agents.schema.json`.

## Where the vectors come from

The vectors in `vectors/generated/` (one vector per line) come from
hand-picked cases and cases generated with hypothesis; the others were written
from the spec. `vectors/ast/logics.json` is generated by `tools/fuzzy_vectors.py`
from fuzzytool, the reference of the fuzzy norms, with the rules the spec adds
(exact boundary conditions and intervals): `uv run --no-project --with
fuzzytool==0.7.0 python conformance/tools/fuzzy_vectors.py >
conformance/vectors/ast/logics.json`. `vectors/ast/decimals.json` and `vectors/ast/rounding.json` are generated by
`tools/decimal_vectors.py`, whose oracle follows `spec/ast.md` (*Decimal
arithmetic*, *Rounding*) and `spec/rules.md` (*`COLLECT` aggregates*) with
Python's exact rationals, independently of the core: `python3
conformance/tools/decimal_vectors.py --write`. With `--check`, which CI runs
next to `check.py`, it also recomputes every arithmetic node of every result
tree (`round` from its result's scale) and every aggregate of every decision
in all the vectors, so a hand-written or generated vector cannot disagree with
the arithmetic of the spec. `vectors/format/canonical.json`,
`vectors/format/digests.json` and `vectors/format/bundles.json` are generated by `tools/canonical_vectors.py`,
whose oracle lays out the canonical bytes of `spec/json-format.md`
(*Canonical bytes*) with Python's own JSON parser, float `repr` and
`decimal`, and hashes them with `hashlib`, independently of the core: `python3
conformance/tools/canonical_vectors.py --write`. With `--check`, which CI runs
too, it regenerates both files and recomputes every canonical text and digest
they hold. Vectors are added by hand, from the spec, and from the core's
property tests: a counterexample that `proptest` finds becomes a new vector.
