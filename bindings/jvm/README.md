# cleanrules for Java

The Java binding of cleanrules: the same engine and the same concepts as every
other language (`RuleSet`, `Decision`, `explain`, `summary`), on the C ABI
([`crates/ffi`](../../crates/ffi/README.md)) through the Foreign Function &
Memory API of Java 25. There is no native code of its own: the bindings of
[`cleanrules.h`](../../crates/ffi/include/cleanrules.h) are written by hand in
Java, and Java data is read straight into the engine's values.

This is the native layer (task G.1a): documents, rule sets, decisions, single
nodes, Java data in and Java values out, errors as exceptions and actions as
lambdas. Building conditions and rules in code, and explanations and summaries
as objects, come with the idiomatic API (G.1b); the native libraries inside the
jar with G.1c; Scala 3 with G.2.

Java 25 or later. Module `io.github.cleanrules`, artifact
`io.github.cleanrules:cleanrules` (not published yet), with two packages:
`io.github.cleanrules`, what every layer shares (the errors, the enums,
`Limits`, `CleanRules`), and `io.github.cleanrules.ffi`, the native layer
(`Document`, `RuleSet`, `Decision`, `Node`, `Action`, `ActionCall`), as
`cleanrules/native` is in JavaScript. The idiomatic API will build on both.

## Example

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
cargo build -p cleanrules-ffi
(cd bindings/jvm && ./gradlew jar)
java --enable-native-access=io.github.cleanrules -Dcleanrules.library.path=target/debug \
     --module-path bindings/jvm/java/build/libs --add-modules io.github.cleanrules \
     docs/examples/Discounts.java docs/examples/discounts.rules
```

A test runs it there and on the class path.

## Building and testing

You need a JDK 25 or later and Rust (the C ABI is built with cargo).

```bash
cd bindings/jvm
./gradlew test    # builds the C ABI (cargo build -p cleanrules-ffi), then the JUnit tests
./gradlew check   # also the javadoc, with -Xdoclint:all -Werror
./gradlew jar     # java/build/libs/cleanrules-<version>.jar
```

The build is Gradle (Kotlin DSL) with the wrapper in the repository, one build
for Java and, later, Scala (`settings.gradle.kts`). It compiles with
`--release 25` and `-Xlint:all -Werror` whatever JDK runs it, and has no
dependencies; the tests use JUnit 5. `-Pcleanrules.library.path=<file>` tests
a library you built yourself, without cargo; cargo's `CARGO_TARGET_DIR` is
honoured.

## Loading the native library

Until G.1c puts the library inside the jar, the binding loads
`cleanrules_ffi` (`libcleanrules_ffi.so`, `libcleanrules_ffi.dylib` or
`cleanrules_ffi.dll`, built by `cargo build -p cleanrules-ffi` into
`target/debug` or `target/release`) the first time it needs it, from:

1. the system property `cleanrules.library.path`, or else the environment
   variable `CLEANRULES_LIBRARY_PATH`: the library's file, or a directory that
   holds it;
2. `java.library.path` (`System.loadLibrary`);
3. the system's dynamic loader (`LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH`,
   `PATH`).

The binding calls restricted methods of `java.lang.foreign`. Allow them with
`--enable-native-access=io.github.cleanrules` (on the module path) or
`--enable-native-access=ALL-UNNAMED` (on the class path); otherwise Java
prints a warning when the library loads, and with
`--illegal-native-access=deny` loading fails with an `UnsatisfiedLinkError`
("native access is not enabled for io.github.cleanrules: run with
--enable-native-access=..."). A library that cannot be found or loaded, or
whose ABI version (`CR_ABI_VERSION`, `CleanRules.ABI_VERSION`) or format
version (`CleanRules.FORMAT_VERSION`) is not the binding's, is an
`UnsatisfiedLinkError` that says which and why, every time it is used.
`CleanRules.load()` loads it at once, `CleanRules.version()` gives its
version, and `CleanRules.libraryPath()` where it came from. The binding needs
a 64-bit platform.

## The API

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

Outputs, payloads and aggregates come back as Java values (*Values given
back* in [`spec/data-model.md`](../../spec/data-model.md)):

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
decision when serialized). Misuses are Java's own exceptions:
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
cause is what it threw, with `rule()`, `executed()` and the `decision()`; an
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
