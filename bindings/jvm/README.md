# cleanrules for Java

The Java binding of cleanrules: the same engine and the same concepts as every
other language (`RuleSet`, `Decision`, `explain`, `summary`), on the C ABI
([`crates/ffi`](../../crates/ffi/README.md)) through the Foreign Function &
Memory API of Java 25. There is no native code of its own: the bindings of
[`cleanrules.h`](../../crates/ffi/include/cleanrules.h) are written by hand in
Java, and Java data is read straight into the engine's values.

It has two layers. The idiomatic API (tasks G.1b and G.1d) builds conditions
and rules in code with static imports and methods, reads and writes them as
documents, builds decision tables, evaluates them, and gives decisions,
explanations, summaries and result trees as objects. The native layer (task
G.1a) reads and writes documents in every format and evaluates them on Java
data or JSON. The jar carries the native library of eight platforms (see
*Installing* below), and Scala 3 comes with G.2.

Java 25 or later. Module `io.github.cleanrules`, artifact
`io.github.cleanrules:cleanrules` (not published yet), one jar for every
platform, with two packages:
`io.github.cleanrules`, the idiomatic API and what every layer shares (the
errors, the enums, `Limits`, `CleanRules`), and `io.github.cleanrules.ffi`,
the native layer (`Document`, `RuleSet`, `Decision`, `Node`, `Action`,
`ActionCall`), as `cleanrules/native` is in JavaScript.

## Conditions and rules in code

```java
import static io.github.cleanrules.Conditions.*;

import io.github.cleanrules.*;
import java.math.BigDecimal;
import java.util.Map;

Rule vip = Rule.named("vip")
        .when(field("customer", "years").gt(5).and(field("order", "total").gt(1000)))
        .then("discount", new BigDecimal("0.15"))
        .priority(10)
        .tags("loyalty")
        .action("notify_sales");
Rule loyal = Rule.named("loyal")
        .when(field("customer", "years").gt(2).or(field("customer", "newsletter")))
        .then("discount", new BigDecimal("0.05"))
        .priority(5);
RuleSet rules = RuleSet.builder().add(vip, loyal).hitPolicy(HitPolicy.PRIORITY).build();

var order = Map.of(
        "customer", Map.of("years", 7, "newsletter", false),
        "order", Map.of("total", new BigDecimal("900.00")));
Decision decision = rules.evaluate(order);
decision.output();                    // Optional[{discount=0.05}], a BigDecimal
decision.fired();                     // [loyal]
System.out.println(decision.explain());
for (Cause cause : decision.whyNot("vip").causes()) {
    cause.counterfactual();           // Optional[Counterfactual[path=[order, total], op=gt, target=1000, gap=Optional[100.00]]]
}
rules.bind(Map.of("notify_sales", context -> System.out.println(context.output()))).run(order);
```

- **Conditions** (`Conditions`, `Expr`, `Field`, `Const`): `field(...)` takes
  the path's segments, keys as `String`s (never split at dots: a key may hold
  one) and indexes as `int`s (`field("items", 0, "price")`; `Field.at(...)`
  goes further down); `constant(value)`, and Java values next to a node become
  constants on their own (`ge(18)`); `and(...)`, `or(...)` and `not(x)`, also
  as methods (`a.and(b)`, `a.or(b)`, `a.not()`). Comparisons `eq`, `ne`, `lt`,
  `le`, `gt`, `ge`; `isIn(values...)` (or a collection), `between(low, high[,
  Closed])`, `isNull()`, `matches(pattern)`, `startsWith`, `endsWith`,
  `contains`; arithmetic `add`, `sub`, `mul`, `div`, `neg()`, `abs()`.
  The core builds every node and applies the construction rules of
  `spec/ast.md`: a node that breaks one, a NaN, an integer beyond 64 bits, a
  decimal beyond 38 digits, a datetime finer than a microsecond or a constant
  that is not a scalar is an `ExpressionException`. A node gives its `tag()`,
  `kind()` (`Kind`), `children()`, `walk()`, `fieldPaths()`, `toJson()` and,
  as `toString()`, its text in the rule syntax (`customer.years > 5 AND ...`).
  `equals` and `hashCode` are structural identity (`sameAs`): Java has no
  operator overloading, so `equals` never builds a node, and it is an
  equivalence relation (constants compare by type and value: `1`, `1.0` and
  `true` differ, `0.0` and `-0.0` do not).
- **Rules** (`Rule`): `Rule.named(name).when(condition)`, then
  `then(key, value)` (a node or a Java constant; `then(map)` adds a map's
  entries, in order for a `SequencedMap` and sorted otherwise),
  `priority(n)`, `tags(...)`, `description(text)`, `metadata(map)`, and
  `action(name)` (data, bound later) or `action(context -> ...)` (code). Each
  step gives a new immutable rule, checked by the core
  (`RuleDefinitionException`). `tags()` and `metadata()` are what the core
  keeps (tags sorted, without repeats).
- **Rule sets** (`RuleSet`): `RuleSet.of(rules...)`, or
  `RuleSet.builder().add(...).hitPolicy(...).aggregate(...).logic(...).strict(...).build()`;
  an immutable `Iterable<Rule>` with `rules()`, `size()`, `get(name)` (an
  `Optional`), `rule(name)` (a `RuleNotFoundException` from the core, with the
  names there are), `contains(name)`, `hitPolicy()`, `aggregate()`, `logic()`,
  `isStrict()`, `fieldPaths()`, `evaluate(data[, limits])`,
  `evaluateMany(rows[, limits])`, `readDecision(text[, format])`,
  `bind(actions)`, `run(data[, limits])` and `summary()`.
- **Decisions** (`Decision`): `ruleSet()`, `hitPolicy()`, `results()` and
  `selected()` (`RuleResult`: `rule()`, `fired()`, `truth()`, `output()`),
  `fired()` (names), `outputs()`, `output()` and `aggregated()` (`Optional`
  maps; `output()` of a policy with several outputs is a
  `HitPolicyException`), `explain()`, `whyNot(name)`, `toJson()` and
  `write(format)`. A decision is `AutoCloseable`: `close()` frees its native
  memory at once, which is optional (the `Cleaner` frees it otherwise); what
  was already read stays readable.
- **Explanations** (`Explanation`, `RuleExplanation`, `Requirement`, `Cause`,
  `CauseGroup` with `GroupKind`, `Counterfactual`, `Note` with `NoteKind`,
  `Reason`, `Status`): the core's
  explanation as objects, with `rules()`, `selected()`, `forRule(name)`,
  `toJson()` (the explanation document) and, as `toString()`, its text; a
  rule's `status()`, `requirement()`, `causes()`, `note()`, `achievable()` and
  its part of the text; a cause's `node()`, `truth()`, `negated()`, `holds()`,
  `values()` (`Cause.FieldValue`: path and value), `reason()`,
  `counterfactual()` (a gap is a number, a `java.time.Period` of days between
  dates, or a `java.time.Duration` between datetimes) and its line of the
  text.
- **Summaries** (`Summary`, `RuleSummary`, `FieldUsage` with
  `FieldUsage.Payload`): `hitPolicy()`,
  `aggregate()`, `logic()`, `isStrict()`, `rules()`, `fields()`, `outputs()`,
  `tags()`, `toJson()` and the text; rules whose action is code are marked.
- **Actions** (`Action`, `ActionContext`): `run(data)` checks that every named
  action is bound (`bind(Map.of(name, action))`, a `RegistryException` from
  the core otherwise), evaluates, and calls the actions of the selected rules,
  in rule order, each with an `ActionContext` (`rule()`, `data()`, `result()`,
  `output()`, `decision()`: the object `run` returns). An action that throws
  stops the run with an `ActionException` whose cause is what it threw, with
  `rule()`, `executed()` and `decision()`; the core writes its message, hint
  and JSON form, as for every binding.

Rules, rule sets, decisions, nodes, decision tables and result trees are
immutable and may be shared between threads; a `Cleaner` frees their native
memory once they are unreachable, so they have no `close()` (decisions may be
closed early).
`Expr.hashCode()` is the core's, the same on every platform for one version of
the library: never store it. Values come back as in *Values given back*
below.

The same concepts in JavaScript, where Java's names differ:

| Java | JavaScript |
|---|---|
| `ruleSet.contains(name)` | `ruleSet.has(name)` |
| `ruleSet.get(name)` (an `Optional`) | `ruleSet.get(name)` (or `undefined`) |
| `Rule.named(name).when(c).then(k, v)...` | `new Rule(name, c, { k: v }, { ... })` |
| `Documents.loads(text, Format.YAML, actions)` | `loads(text, { format: 'yaml', actions })` |
| `DecisionTable.builder().header(...).row(...).build()` | `new DecisionTable(header, rows, { ... })` |
| `table.ruleSet()` | `table.ruleset` |
| `expr.evaluateTree(data)` | `evaluateTree(expr, data)` |
| `RuleSet.builder()...build()`, `RuleSet.of(...)` | `new RuleSet([...], { ... })` |
| `decision.output()` (an `Optional`) | `decision.output` (or `null`) |
| `explanation.forRule(name)` | `explanation.forRule(name)` |
| `expr.equals(other)`, `hashCode()` | `expr.sameAs(other)` |

[`docs/examples/DiscountRules.java`](../../docs/examples/DiscountRules.java)
builds the rules of `discounts.rules` in code and runs with the source
launcher, as its comment says; a test runs it.

## Documents, decision tables and result trees

```java
import static io.github.cleanrules.Conditions.field;
import static io.github.cleanrules.Documents.*;

if (load(Path.of("discounts.rules"), Map.of("notify_sales", notify)) instanceof RuleSet rules) {
    String yaml = dumps(rules, Format.YAML);
}
Rule vip = ruleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15");

DecisionTable table = DecisionTable.builder()
        .hitPolicy(TablePolicy.PRIORITY)
        .header("RULE", field("customer", "years"), "order.total", TableOutput.of("risk", "high", "low"))
        .row("new", field("customer", "years").lt(1), "> 1000", "'high'")
        .row("default", null, null, "'low'")
        .build();
ResultTree tree = vip.when().evaluateTree(Map.of("customer", Map.of("years", 7)));
```

- **Documents** (`Documents`, for static imports): `loads(text[, format[,
  limits], actions])` (a `String`, or UTF-8 bytes) and `load(path[, format,
  limits], actions)` read a condition, rule, rule set or decision table
  document as a `DocumentContent`, a sealed interface of `Expr`, `Rule`,
  `RuleSet` and `DecisionTable` to take apart with a `switch`; a file's format
  is its extension's (`.json`, `.yaml`, `.yml`, `.rules`, `.csv`) unless
  given, and a file beyond the size limit of its format is not read
  (`limit_exceeded`). `actions`, a registry, binds the rules read
  (`RuleSet.bind`); without it named actions stay unbound. `dumps(value[,
  format[, omitActions]])` and `dump(value, path[, format, omitActions])`
  write one in canonical form (a rule text ends with a line break in a file):
  a value node is an `ExpressionException`, and a rule whose action is code
  without a name an `UnsupportedException` at `/rule/do` or
  `/ruleset/rules/<i>/do`, unless `omitActions`. Rules and rule sets read
  keep the core's own rules, so they write back exactly.
  `ruleFromText(text[, action | actions | limits])` and
  `ruleFromText(text, limits, actions)` (both may be null; a bare `null` in
  the two-argument form matches all three and does not compile) read one rule
  in the rule syntax (fuzzytool's verb; `rule_1` without a `RULE` line; text
  naming an action binds the code to that name).
- **Decision tables** (`DecisionTable`, `TablePolicy`, `TableOutput`,
  `TableRow`): `DecisionTable.of(header, rows)` or `DecisionTable.builder()`
  (`header(...)`, `row(...)`, `rows(...)`, `hitPolicy`, `aggregate`, `logic`,
  `strict`, `limits`), as Python's and JavaScript's: header items are a CSV
  header field's text, an input `Expr` or a `TableOutput`; a `String` cell is
  read as the table's CSV writes it (`"> 5"`, `"-"`, `"'VIP'"` for the string),
  an `Expr` cell is an input's condition or an output's value, `null` and
  `Optional.empty()` are empty, and anything else is a constant. The core
  reads the grid one row at a time; its errors point at `/header/<c>` or
  `/rows/<r>/<c>` (also for a NaN, which Java rejects). A table gives
  `hitPolicy()`, `aggregate()`, `logic()`, `isStrict()`, `inputs()`,
  `outputs()`, `rows()` (each `TableRow`: `name()`, `priority()`,
  `description()`, `inputs()` and `outputs()` as `Optional<Expr>`s),
  `ruleSet()`, `evaluate`, `evaluateMany`, `summary()` and `toJson()`.
- **Result trees** (`ResultTree`): `expr.evaluateTree(data[, logic, strict])`
  and `expr.evaluateValueTree(data[, strict])` evaluate one node, and
  `ActionContext.tree()` gives the tree of the rule's condition in an action;
  each result has `node()`, `truth()` (an `Optional`, empty for a value),
  `value()`, `reason()`, `children()` and `toJson()` (the core's JSON result
  tree of that result).

[`docs/examples/RiskTable.java`](../../docs/examples/RiskTable.java) does what
`risk.mjs` does in JavaScript (the table of `risk.csv`, built again, a rule
from text, a result tree, a rule set written and read back with its action
bound) and runs like `DiscountRules.java`, with `docs/examples/risk.csv` as its
argument; a test runs it.

## The native layer

```java
import io.github.cleanrules.Format;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.RuleSet;
import java.math.BigDecimal;
import java.util.Map;

record Customer(int years, boolean newsletter) {}

try (RuleSet rules = RuleSet.read("""
        HIT POLICY priority
        RULE vip PRIORITY 10
        IF customer.years > 5 AND order.total > 1000
        THEN discount = 0.15
        DO notify_sales
        """, Format.TEXT)) {
    var order = Map.of(
            "customer", new Customer(7, false),
            "order", Map.of("total", new BigDecimal("1500.00")));
    try (Decision decision = rules.evaluate(order)) {
        Map<?, ?> output = (Map<?, ?>) decision.output();  // {discount=0.15}, a BigDecimal
        System.out.println(decision.explain());
    }
    rules.run(order, Map.of("notify_sales", call ->
            System.out.println("notify " + call.ruleName() + ": " + call.output()))).close();
}
```

[`docs/examples/Discounts.java`](../../docs/examples/Discounts.java) runs the
rules of `discounts.rules` with the source launcher, on the module path:

```bash
(cd bindings/jvm && ./gradlew jar)    # the jar, with this platform's library inside
java --enable-native-access=io.github.cleanrules \
     --module-path bindings/jvm/java/build/libs --add-modules io.github.cleanrules \
     docs/examples/Discounts.java docs/examples/discounts.rules
```

A test runs it there and on the class path (with the library the tests
build, `-Dcleanrules.library.path=target/debug`).

## Building and testing

You need a JDK 25 or later and Rust (the C ABI is built with cargo).

```bash
cd bindings/jvm
./gradlew test    # builds the C ABI (cargo build -p cleanrules-ffi), then the JUnit tests
./gradlew check   # also the javadoc, with -Xdoclint:all -Werror
./gradlew jar     # java/build/libs/cleanrules-<version>.jar, with this platform's library
```

The build is Gradle (Kotlin DSL) with the wrapper in the repository, one build
for Java and, later, Scala (`settings.gradle.kts`). It compiles with
`--release 25` and `-Xlint:all -Werror` whatever JDK runs it, and has no
dependencies; the tests use JUnit 5. `-Pcleanrules.library.path=<file>` tests
a library you built yourself, without cargo; cargo's `CARGO_TARGET_DIR` is
honoured.

`./gradlew jar` builds the library of the platform it runs on with
`cargo build --release -p cleanrules-ffi --target <its triple>` (on musl
with `RUSTFLAGS="-C target-feature=-crt-static"`, so that Rust builds a
shared library) and puts it in the jar; the target must be installed
(`rustup target add <triple>`), and the error says which.
`-Pcleanrules.natives.dir=<dir>` puts instead the prebuilt libraries of
`<dir>/<platform>/<file>` (`darwin-arm64/libcleanrules_ffi.dylib`,
`linux-x64-gnu/libcleanrules_ffi.so`, `win32-x64-msvc/cleanrules_ffi.dll`...),
without cargo; that is how CI builds the jar of all eight (the "JVM natives"
workflow, `.github/workflows/jvm-natives.yml`), then checks it with
`python3 scripts/check-jar.py <jar>` (`--platforms <list>` for fewer: exactly
those libraries under `native/`, each a binary of its system and processor
with its SHA-256, glibc 2.28 at most for the gnu ones) and runs
`scripts/JarSmoke.java` from the jar on each platform. The manifest's
`Cleanrules-Native-Platforms` lists the platforms a jar holds.

## Installing

One jar, `io.github.cleanrules:cleanrules`, for every platform (not on Maven
Central yet: G.3). It holds the native library (the C ABI, `cleanrules_ffi`)
of each of these, the platforms of the npm packages, and nothing needs
installing besides it:

| Platform | System | Processor | In the jar |
|---|---|---|---|
| `darwin-x64`, `darwin-arm64` | macOS | x64, arm64 (Apple silicon) | `native/darwin-*/libcleanrules_ffi.dylib` |
| `linux-x64-gnu`, `linux-arm64-gnu` | Linux with glibc 2.28 or later (built on manylinux_2_28: RHEL 8, Debian 10, Ubuntu 20.04 and later) | x64, arm64 | `native/linux-*-gnu/libcleanrules_ffi.so` |
| `linux-x64-musl`, `linux-arm64-musl` | Linux with musl (Alpine), with `libgcc` (`apk add libgcc`) | x64, arm64 | `native/linux-*-musl/libcleanrules_ffi.so` |
| `win32-x64-msvc`, `win32-arm64-msvc` | Windows 10 or later | x64, arm64 | `native/win32-*/cleanrules_ffi.dll` |

Each library has its SHA-256 next to it (`<file>.sha256`, as `sha256sum`
writes it), and the manifest's `Cleanrules-Native-Platforms` lists the
platforms. Elsewhere (another system or processor), build `cleanrules-ffi`
yourself and give its path (below).

## Loading the native library

The binding loads `cleanrules_ffi` the first time it needs it (or at
`CleanRules.load()`), from the first of:

1. the system property `cleanrules.library.path`, or else the environment
   variable `CLEANRULES_LIBRARY_PATH`: the library's file
   (`libcleanrules_ffi.so`, `libcleanrules_ffi.dylib` or `cleanrules_ffi.dll`,
   built by `cargo build --release -p cleanrules-ffi` into `target/release`),
   or a directory that holds it;
2. the jar, which holds this platform's library: the binding extracts it to a
   temporary directory and loads it from there (below);
3. only when the jar has no library for this platform: `java.library.path`
   (`System.loadLibrary`), then the system's dynamic loader
   (`LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH`, `PATH`).

A library of the jar that cannot be extracted or loaded is an error, not a
reason to look further, so which library runs never depends on a failure.

The platform is the JVM's: `os.name` and `os.arch` (an x64 JVM under Rosetta
2 takes the x64 library), and on Linux the C library the JVM itself runs on,
read from the files mapped into the process (`/proc/self/maps`: musl's
`ld-musl-<arch>.so.1` or glibc's `libc.so.6`), or else from the program
interpreter of the `java` launcher. That is the library that counts: a glibc
system with musl installed takes the glibc library, and a glibc JDK on Alpine
(through `gcompat`) runs on musl and takes the musl one.

**The temporary directory.** Each JVM extracts its own copy into a new
directory `cleanrules-native-<random>` under the system property
`cleanrules.tmpdir`, or else `java.io.tmpdir`. Only its user may read, write
or enter it (`rwx------`, and the same for the file, where the file system
has POSIX permissions; on Windows it inherits the access control lists of
the directory it is in, which keep it private in the default per-user
`%TEMP%` but not in a shared directory given as `cleanrules.tmpdir`). The file is created new there, never replaced, so no other
process can swap it, and its size and SHA-256 are checked against the jar's
before it is loaded. On Linux and macOS the file and its directory are
deleted as soon as the library is loaded (the loaded library stays mapped),
so nothing is left behind unless the JVM dies while loading it. Windows
cannot delete a loaded DLL: the directory stays while the JVM runs, and a
later JVM deletes every `cleanrules-native-*` directory of its own user that
is more than an hour old and holds nothing but a library no process has
loaded (never through a symbolic link or a junction). `CleanRules.libraryPath()` gives
the file it was loaded from (already deleted, but on Windows).
Where the temporary directory does not allow programs (mounted `noexec`),
set `-Dcleanrules.tmpdir=<dir>` to one that does, or give the library's path.

The binding calls restricted methods of `java.lang.foreign`. Allow them with
`--enable-native-access=io.github.cleanrules` (on the module path) or
`--enable-native-access=ALL-UNNAMED` (on the class path); otherwise Java
prints a warning when the library loads, and with
`--illegal-native-access=deny` loading fails with an `UnsatisfiedLinkError`
("native access is not enabled for io.github.cleanrules: run with
--enable-native-access=..."). A library that cannot be found, extracted or
loaded, or whose ABI version (`CR_ABI_VERSION`, `CleanRules.ABI_VERSION`) or
format version (`CleanRules.FORMAT_VERSION`) is not the binding's, is an
`UnsatisfiedLinkError` that says which and why, every time it is used:

- a platform the jar has no library for: "cleanrules bundles no library for
  linux-riscv64-gnu (os.name Linux, os.arch riscv64; the jar has darwin-x64,
  ...), and there is no libcleanrules_ffi.so on java.library.path or the
  system's library path: build it for this platform (cargo build --release -p
  cleanrules-ffi) and set the system property cleanrules.library.path or the
  environment variable CLEANRULES_LIBRARY_PATH to its path" (or "this jar has
  no cleanrules library for linux-x64-musl (it has ...)" for one of the eight
  that a jar was built without);
- a directory it cannot write to: "cannot extract the cleanrules library for
  linux-x64-gnu to /tmp: ...; set the system property cleanrules.tmpdir to a
  writable directory; or set ... cleanrules.library.path ...";
- a library that does not load (an older glibc, no `libgcc` on Alpine, a
  `noexec` directory): "cannot load the cleanrules library for linux-x64-gnu
  (it needs glibc 2.28 or later), extracted from the jar to ...: ...",
  naming `cleanrules.tmpdir` and `cleanrules.library.path`;
- a damaged jar: "the jar's /native/linux-x64-gnu/libcleanrules_ffi.so has
  SHA-256 ..., and its ....sha256 says ...: the jar is damaged".

`CleanRules.load()` loads it at once, `CleanRules.version()` gives its
version, and `CleanRules.libraryPath()` where it came from. The binding needs
a 64-bit platform.

## Restricted environments

- **Read-only containers.** The library is extracted under
  `java.io.tmpdir`; where that is read-only or mounted `noexec`, point
  `cleanrules.tmpdir` at a writable directory that allows programs (a
  `tmpfs` without `noexec`, for example). In a container that is easiest
  through the environment: `JAVA_TOOL_OPTIONS=-Dcleanrules.tmpdir=/run/cleanrules`.
  Or set `CLEANRULES_LIBRARY_PATH` to a library installed in the image, and
  nothing is extracted.
- **GraalVM native image** is not supported until G.3: the image holds only
  the resources registered for it, so the jar's libraries are missing (the
  error then says "native image: register the resources ... or set
  cleanrules.library.path"). Give the library's path with
  `cleanrules.library.path` or `CLEANRULES_LIBRARY_PATH` and ship it next to
  the image.
- **macOS hardened runtime.** An application signed with the hardened
  runtime loads only libraries signed by its own team (or Apple), so it
  refuses the extracted library. Either add the entitlement
  `com.apple.security.cs.disable-library-validation`, or sign
  `libcleanrules_ffi.dylib` with the application, ship it inside the bundle
  and point `cleanrules.library.path` at it.
- **jlink and jpackage** work: the module is an ordinary named module, and
  the image carries the jar's libraries for all eight platforms (as
  resources of the module), of which only this platform's is extracted.
- **Profilers and crash logs.** On Linux and macOS the extracted file is
  deleted once loaded, so `/proc/<pid>/maps`, `perf`, async-profiler and
  `hs_err` files show it as `(deleted)` and may not resolve its symbols.
  When profiling, extract it once (`unzip` the jar's
  `native/<platform>/<file>`) and give its path with `cleanrules.library.path`.

## The native layer's API

| Class (package) | What it is |
|---|---|
| `Document` (`ffi`) | a document in JSON, YAML, the rule syntax or CSV: `read(text, format[, limits])` (a `String`, or UTF-8 bytes), `kind()`, `write([format])` |
| `RuleSet` (`ffi`) | `read(text, format[, limits])`, `fromDocument(document)`, `write([format])`, `summary([format])` (text by default), `size()`, `ruleName(i)`, `ruleNames()`, `hitPolicy()`, `logic()`, `aggregate()`, `isStrict()`, `fieldPaths()`, `evaluate(data[, limits])`, `evaluateJson(json[, limits])` (a `String` or bytes), `evaluateMany(inputs[, limits])` (any `Iterable`), `evaluateManyJson(json[, limits])`, `readDecision(text[, format[, limits]])`, `checkActions(names)`, `run(data, actions[, limits])` |
| `Decision` (`ffi`) | `size()`, `ruleName(i)`, `firedAt(i)`, `truth(i)`, `selected()`, `output()`, `outputs()`, `ruleOutput(i)`, `outputJson()`, `outputsJson()`, `write([format])` (JSON by default), `explain([format])` (text by default) |
| `Node` (`ffi`) | one condition or value: `read(text, format[, limits])`, `write([format])`, `fieldPaths()`, `evaluateTree(data[, logic, strict])`, `evaluateValueTree(data[, strict])` and their `...Json` forms, which give the result tree as JSON |
| `Limits` | immutable limits: `Limits.defaults().withMaxSize(n)`, `withMaxNesting`, `withMaxDepth`, `withMaxTableSize`, `withMaxYamlSize`, `withMaxTextSize` |
| `Action`, `ActionCall` (`ffi`) | an action (`call -> ...`, which may throw) and its call: `ruleName()`, `position()`, `truth()`, `output()`, `data()`, `decision()` |
| `Format`, `Truth`, `DocumentKind`, `HitPolicy`, `Aggregate` | enums; all but `Format` have `jsonName()` and `fromJsonName(name)` |
| `CleanRules` | `FORMAT_VERSION`, `ABI_VERSION`, `version()`, `load()`, `libraryPath()` |

Rule positions are `int`s from 0; a position out of the rule set is an
`IndexOutOfBoundsException`. `evaluate` never runs actions.

## Data

`evaluate`, `evaluateMany`, `run` and the node evaluations take Java data, and
read only the paths that the rule set or node reads (`fieldPaths()`), each
object once, as [`spec/data-model.md`](../../spec/data-model.md) says (*Java
and Scala*):

| Java | Value |
|---|---|
| `null`, `Optional.empty()` | null; `Optional.of(x)` is `x` |
| `Boolean` | boolean |
| `Byte`, `Short`, `Integer`, `Long`, `BigInteger` | int; beyond 64 bits, a null with reason `int_overflow` |
| `Float`, `Double` | float; NaN is null |
| `BigDecimal` | decimal; beyond 38 digits, a null with reason `decimal_overflow` |
| `String`, `CharSequence` | string (with a lone surrogate, opaque) |
| enum constants, `DayOfWeek` and other enums of the platform included | string: the constant's `name()` |
| `LocalDate`, `java.sql.Date` | date |
| `LocalDateTime` | naive datetime, truncated to the microsecond |
| `Instant`, `java.util.Date`, `java.sql.Timestamp` | aware datetime in `Z` |
| `OffsetDateTime` | aware datetime at its offset |
| `ZonedDateTime` | aware datetime in its zone, or at its offset for a `ZoneOffset` or a zone the engine does not know |
| `List`, arrays (not `byte[]`) | list |
| `Map` with `String` keys | map: a `SequencedMap` (`LinkedHashMap`, `TreeMap`) keeps its order, any other map is sorted by key; other keys are left out |
| records | map of their components |
| other objects | map of their public instance fields and JavaBean accessors (`getAge()`, `isActive()`), sorted by name |
| sets, `byte[]`, lambdas, threads, class loaders, classes, any other object of the Java platform, and the binding's own objects | opaque: never read through, compared by identity |

Dates outside years 1 to 9999 are opaque. An index beyond `Integer.MAX_VALUE`
(`a[3000000000]`, a `Long` in `fieldPaths()`) is beyond any Java list: it is
missing. A getter that throws gives a null
with reason `read_error`; in strict mode the `EvaluationException` has the
exception as its cause (also in a batch, for the row that failed). Nothing but
public data is read: no private member, no method with parameters, no static
member, and nothing that a class of the platform declares (`getClass()`), so a
rule loaded from a file cannot reach a `Class`, a `ClassLoader` or a `Thread`.
A public accessor of a class that is not public (a private nested record) is
read where its package is open to the binding, which it always is on the class
path, or else through a public supertype that declares it. A getter that
throws an error of the virtual machine (`StackOverflowError`,
`OutOfMemoryError`) is never a read error: the error goes on as it is.

## Values given back

Outputs, payloads, aggregates, a table's output values and a result tree's
values come back as Java values (*Values given back* in
[`spec/data-model.md`](../../spec/data-model.md)):

| Value | Java |
|---|---|
| null, whatever its reason | `null` |
| boolean | `Boolean` |
| int | `Long` |
| float | `Double` |
| decimal | `BigDecimal`, with its scale; the infinities as `Double`'s |
| string | `String` |
| date | `LocalDate` |
| naive datetime | `LocalDateTime` |
| aware datetime | `ZonedDateTime`, in its IANA zone (whose local fields follow Java's zone database), or at a `ZoneOffset` |
| list | an unmodifiable `List` |
| map | an unmodifiable `SequencedMap`, in the engine's order |
| opaque | the very object that was read; `null` in a decision read from a document |

Each reads back as the same value, except a decimal infinity (a float) and an
opaque `null`.

## Errors

Unchecked exceptions with the hierarchy of
[`spec/errors.md`](../../spec/errors.md), named with `Exception`:
`CleanRulesException` and its subclasses `FormatException` (also for
`unsupported_version`, `limit_exceeded` and `decision_mismatch`),
`ExpressionException`, `RuleDefinitionException`, `RuleSyntaxException`,
`RegistryException`, `RuleNotFoundException`, `EvaluationException` (with
`MissingFieldException`, `TypeMismatchException` and `HitPolicyException`),
`ActionException` and `UnsupportedException`. Each has `code()`, the message
and `hint()` the engine writes, the context fields that apply (`pointer()`,
`line()`, `column()` and `row()` as `OptionalInt`, `rule()`, `rules()`, `path()`
with `String` keys and `Integer` indexes (`Long` beyond them), `types()`, `name()`, `available()`,
`executed()`, `node()` as JSON, `operation()`, `reason()`: empty where they do
not apply) and `json()`, the error's JSON form. The cause is `getCause()`.
Exceptions are immutable and serializable (an `ActionException` loses its
decisions when serialized). Misuses are Java's own exceptions:
`NullPointerException`; `IllegalArgumentException` (a negative limit, an
empty or repeated action name, anything the C ABI calls
`CR_INVALID_ARGUMENT`); `IllegalStateException` (a closed object, an action
call used after it ended, or `CR_INTERNAL_ERROR`, a bug in cleanrules to
report); and `IndexOutOfBoundsException`.

## Actions

`run(data, actions)` takes the actions by the names the rules give them
(`DO notify_sales`), checks that every name the rules use is there before it
reads anything (`RegistryException`), evaluates, and calls the actions of the
selected rules in rule order, on the calling thread, each with an
`ActionCall`. An action fails by throwing: the run stops with an `ActionException` whose
cause is what it threw, with `rule()`, `executed()` and the decision
(`nativeDecision()`; `decision()` is the idiomatic `run`'s); an
error of the virtual machine (`StackOverflowError`...) is thrown by `run` as
it is. Nothing an action throws ever crosses the native frames: the binding
catches it in the upcall and the C ABI reports the failure. An action may run
rule sets again. An `ActionCall` is valid during its call only, from any
thread: afterwards each method throws `IllegalStateException`, and a method
another thread is running when the call ends finishes first.
`call.decision()` gives a copy of the decision that you own, made once per
run and shared by its actions; it is not the object `run` returns.

## Memory and threads

`Document`, `RuleSet`, `Decision` and `Node` own native memory. They are
`AutoCloseable`: close them (`try`-with-resources), or a `Cleaner` frees them
once they are unreachable. Closing twice does nothing; using a closed object
is an `IllegalStateException` ("this RuleSet is closed"). A decision keeps
what it needs of its rule set, which may be closed first. Everything else
(`Limits`, the exceptions, the values given back) holds no native memory, and
the memory of each call (texts, out-parameters, the input read from Java data)
lives in a confined arena freed when the call returns.

These four never change once made, so each may be used from several threads at
once: one rule set may evaluate on many threads. Closing one while another
thread uses it is safe: it is freed when the last call using it returns. An
`ActionCall` belongs to its call. `Limits` and the exceptions are immutable.

## License

Apache-2.0, with the [NOTICE](../../NOTICE) that every redistribution keeps;
the jar carries both in `META-INF`. cleanrules is written by Jose L. Salmeron.
