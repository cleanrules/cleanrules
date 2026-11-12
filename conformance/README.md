# Conformance vectors

The vectors in this directory are the executable form of the specification
(`spec/`). The Rust core and every binding run all of them, and must produce
exactly the expected output (`CLAUDE.md`, invariant 5). When a vector and an
implementation disagree, the spec decides which one is wrong.

```text
conformance/
  vectors/<area>/<topic>.json   the vectors, grouped by spec area
  tools/check.py                the vector checker
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
| `read` | `text` (a JSON text, for lexical cases), `yaml` (a YAML text, `json-format.md`, *YAML*) or `document`; `ruleset` when reading a decision; optional `limits` (`max_size` in bytes, `max_nesting`, `max_depth`) | the document reader | `document`: the canonical form that is read back, or `error` |
| `condition` | `condition` (a condition node), `input` (a data value), optional `logic` and `strict` | `evaluate_tree` (`spec/ast.md`, *Evaluating one node*) | `result`: its result tree, or `error` |
| `value` | `value` (a value node), `input`, optional `strict` | `evaluate_value_tree` | `result`: its result tree, or `error` |
| `evaluate` | `ruleset` (a rule set document's content), and `input` or `inputs` (a batch); optional `limits` (`max_values`, `spec/data-model.md`, *Reading native data*, *Limits*) | `evaluate`, or its batch form | `decision`, or `decisions` for a batch, or `error` |
| `explain` | `ruleset`, `input` | `evaluate`, then `explain` | `decision` (which `at` pointers refer to) and `explanation`, optionally `text` (the exact text of `explain()`), or `error` |
| `summary` | `ruleset` | `summary` | `summary`, and optionally `text` |
| `draw` | `format` (`dot`, `mermaid`, `html` or `html_fragment`), `document` (a condition, rule, rule set or decision table document); optional `input`, with a rule set or a table; optional `limits` (`max_diagram_size`) | `draw_document` (`spec/visual.md`); with an `input`, the rule set (or the table's rule set) evaluates it, and the runner draws the decision's explanation for a rule set (`draw_explanation`), and the table with the decision for a table (`draw_document` with the decision) | `diagram`: the exact text, compared as a string, or `error` (`code` and `name`) |

Optional flags such as `strict` are omitted when false. An `evaluate` vector
with `limits` checks `max_values`, which bounds what is read from native data
only: runners run it with the language's native data (`native:<id>`) and skip
it in their JSON runs, and the core, C ABI and C++ runners, which have no
native data, skip it and count the skips. An `error` holds the
error's `code` and the compared context fields that apply, such as
`{"code": "missing_field", "rule": "vip", "path": ["customer", "age"]}`;
runners compare errors as `spec/errors.md` (*Comparing errors*) says.

## Comparing

Runners compare the actual output with `expect` as parsed JSON trees, as
`spec/json-format.md` (*Comparing documents*) defines: object members in
order, numbers by lexical class and exact value, and the sign of a float zero
(use `copysign` or its equivalent: `-0.0 == 0.0` in most languages). Some parts are not compared,
because the spec says that they are informative:

- `detail` in reasons is removed from both sides before comparing;
- any two opaque values (`{"$opaque": ...}`) are equal, whatever their type
  name;
- errors are compared by code and compared fields only (`spec/errors.md`).

Runners must parse the vectors without losing what is compared. A generic
parser that turns `1.0` into `1`, reads integers through floats, or reorders
object members cannot be used as is: JavaScript needs a lossless parser (or
`JSON.parse` with access to the source text) and an ordered representation of
objects, because plain objects move integer-like keys such as `"10"` first; Go
needs a token-stream decoder with `UseNumber`, because `map[string]any` loses
member order.

A runner reports every failing vector with its id, the expected and the actual
output, never stopping at the first one.

## Runners

- **Core**: `cargo test -p cleanrules --test conformance` reads the documents
  and data with the core's own JSON reader, and runs the `draw` vectors.
- **Python**: `cd bindings/python && uv run pytest tests/test_conformance.py`
  runs every vector through the binding's JSON entry points (`json:<id>`) and,
  except the `read` vectors, with native Python data and the Python API
  (`native:<id>`).
- **C ABI**: `cargo test -p cleanrules-ffi --test conformance` runs every
  vector through the C functions, from JSON and from native `cr_value`s;
  `draw` vectors through `cr_document_draw` and the function of the
  document's content (`cr_node_draw`, `cr_rule_draw`, `cr_ruleset_draw`,
  `cr_decision_draw`).
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
  (`spec/data-model.md`, which gives a map back without order, the zone `UTC`
  as `time.UTC` and a zone Go does not know at its offset) and as their JSON
  forms. `read` vectors go through `Loads`, `Dumps`, `RuleFromText`,
  `NewDecisionTable` and `ReadDecision`. Grids are Go data, so `table_grid`
  vectors have no JSON run, and the vector with a limit of 0 has no Go run (0
  keeps the default in Go); skips are counted and listed in
  `bindings/go/build/reports/conformance.txt`. It compares with a lossless
  JSON reader of its own (`bindings/go/conformance_json_test.go`).
- **Each binding** runs every vector twice: once through its JSON entry points,
  and once converting `input` into the language's native data (with the
  conversions of `spec/data-model.md`: floats as floats, with `cr.float` in
  JavaScript, decimals as the language's decimal, maps as an ordered map) and
  calling the idiomatic API. The second run tests the binding's own code, its
  data conversion. In a language whose maps have no order (Go), maps given
  back are compared without regard to member order in that run; Go's runner
  gives a map whose keys are not sorted as a struct, whose fields keep their
  order, so the JSON forms of its results are still compared exactly.

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
exists and that every normative section is cited, and sanity-checks the error
locations of `read` vectors.

## Where the vectors come from

The vectors in `vectors/generated/` (one vector per line) come from
hand-picked cases and cases generated with hypothesis; the others were written
from the spec. Vectors are added by hand, from the spec, and from the core's
property tests: a counterexample that `proptest` finds becomes a new vector.
