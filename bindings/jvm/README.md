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
data or JSON. A Scala 3 layer (task G.2, [below](#scala-3)) wraps the
idiomatic API with fuzzytool's operators, case classes and Scala types. The
native libraries inside the jar come with G.1c.

Java 25 or later. Module `io.github.cleanrules`, artifact
`io.github.cleanrules:cleanrules` (not published yet), with two packages:
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

## Scala 3

The Scala layer (task G.2) is a thin layer over the idiomatic Java API, for
Scala 3.3 LTS or later: artifact `io.github.cleanrules:cleanrules-scala_3`
(not published yet), package `io.github.cleanrules.scaladsl` (not `.scala`,
which `import io.github.cleanrules.*` would bring into scope, hiding the root
`scala` package). The core still builds, checks, evaluates and explains; the
layer converts Scala values and delegates. Every Scala object keeps the Java
one in `asJava`, and each type has `fromJava` for the other way.

```scala
import io.github.cleanrules.{HitPolicy, Truth}
import io.github.cleanrules.scaladsl.*

case class Customer(years: Int, newsletter: Boolean)
case class Purchase(total: BigDecimal)
case class Order(customer: Customer, order: Purchase)

val vip = Rule(
  "vip",
  F.customer.years > 5 & F.order.total > 1000,
  payload = Map("discount" -> BigDecimal("0.15")),
  priority = 10,
  tags = Seq("loyalty"),
  actionName = Some("notify_sales"),
)
val loyal = Rule("loyal", F.customer.years > 2 | F.customer.newsletter, payload = Map("discount" -> BigDecimal("0.05")))
val rules = RuleSet(Seq(vip, loyal), hitPolicy = HitPolicy.PRIORITY)

val decision = rules.evaluate(Order(Customer(7, false), Purchase(BigDecimal("900"))))
decision.output                        // Some(VectorMap(discount -> 0.05))
decision.whyNot("vip").causes.head match
  case Cause(node, Truth.FALSE, _, _, _, _, Some(Counterfactual(_, op, _, Some(gap)))) =>
    s"$node needs $op, short by $gap"  // order.total > 1000 needs gt, short by 100
  case other => other.toString
println(decision.explain)              // the text, the same in every language
```

**Conditions.** `Expr` (with `Field` and `Const`) wraps the Java node, with
fuzzytool's operators and Scala's precedence (comparisons above `&`, `&` above
`|`, so `F.a > 1 & F.b < 2 | F.c` needs no parentheses):

| Scala | Node | Java |
|---|---|---|
| `a & b`, `a \| b`, `~a` | `and`, `or` (flattened), `not` | `and`, `or`, `not` |
| `a === b`, `a =!= b` | `eq`, `ne` | `eq`, `ne` |
| `a < b`, `a <= b`, `a > b`, `a >= b` | `lt`, `le`, `gt`, `ge` | `lt`, `le`, `gt`, `ge` |
| `a + b`, `a - b`, `a * b`, `a / b`, `-a`, `a.abs` | arithmetic | `add`, `sub`, `mul`, `div`, `neg`, `abs` |
| `a.isIn("ES", "PT")`, `a.isIn(set)` | `in` | `isIn` |
| `a.between(1, 10[, Closed.LEFT])` | `between` | `between` |
| `a.isNull`, `a.matches(re)`, `a.startsWith(s)`, `a.endsWith(s)`, `a.contains(s)` | tests | the same |

`==` is Scala's own equality: on nodes, the structural identity of
`spec/ast.md`, as `equals` in Java, so nodes work in sets and maps. Constants
go on the right and are typed (`Constant`: booleans, numbers, `BigDecimal`
and `BigInt`, strings, `java.time` dates and datetimes, Java enums and Scala 3
enum cases); on the left, `Const(1000) < F.total`. `toString` is the core's
text, `toJson` the JSON node, `fieldPaths` the paths read, and
`evaluateTree(data[, logic, strict])` and `evaluateValueTree(data[, strict])`
give a `ResultTree`; the rest is on `asJava` (`tag()`, `kind()`,
`children()`...).

**Field paths.** `F.customer.age`, `F.items(0).price`, `F("customer", "age")`
and `F.customer("age")` (`F` and `Field` are `Dynamic`: `selectDynamic` and
`applyDynamic`, because any key must work and `Selectable` needs the keys in a
type). Keys are never split at dots. A key that is a member of a field (the
operators, `isIn`, `between`, `isNull`, `matches`, `startsWith`, `endsWith`,
`contains`, `abs`, `evaluateTree`, `evaluateValueTree`, `fieldPaths`,
`toJson`, `asJava`, `apply`, or a method every Scala object has, such as
`equals`, `toString` or `Predef`'s `->` and `ensuring`) is reached with `apply`:
`F.customer("contains")`. At the top only `apply` and the methods of every
object clash, so `F.contains` is the field `contains`. A key that is not an
identifier takes backquotes (`` F.`type` ``) or `apply`.

**Rules and rule sets.** `Rule(name, when, payload, priority, tags,
description, metadata, actionName, action)`, with every argument after `when`
named and optional, and the steps `withPayload`, `withPriority`, `withTags`,
`withDescription`, `withMetadata`, `withActionName` and `withAction`, each a
new rule (`then` is a keyword in Scala 3, hence `payload`). A payload in a
`SeqMap` (`ListMap`, `VectorMap`) keeps its order; any other map is sorted by
key. A rule gives back `name`, `when`, `payload` (a `SeqMap` of nodes),
`priority`, `tags`, `description` and `actionName` (`Option`s), `metadata`
and `action` (the very function). `RuleSet(rules*)`, or `RuleSet(rules,
hitPolicy, aggregate, logic, strict)` with named arguments, gives `rules`,
`size`, `get(name)` (`Option`), `apply(name)` (`RuleNotFoundException`),
`contains`, `hitPolicy`, `aggregate`, `logic`, `isStrict`, `fieldPaths`,
`evaluate`, `evaluateMany`, `readDecision`, `bind`, `run` and `summary`.
`ruleFromText(text[, actions])` reads a rule of the rule syntax. Rules, rule
sets and decisions compare by identity, that of their Java objects.

**Decisions.** `fired`, `results` and `selected` (`RuleResult`, a case class
of `rule`, `fired`, `truth` and `output`), `outputs`, `output` and
`aggregated` (`Option`s of `SeqMap`), `explain` (cached), `whyNot(name)`,
`toJson`, `write(format)` and `close()`. Explanations, summaries and result
trees are case classes to match on: `Explanation(selected, rules)`,
`RuleExplanation(rule, status, requirement, note, achievable)` with
`causes`, the sealed `Requirement` (`Cause(node, truth, negated, holds,
values, reason, counterfactual)` and `CauseGroup(kind, items)`),
`FieldValue`, `Counterfactual(path, op, target, gap)`, `Note`, `Reason`,
`Summary`, `RuleSummary`, `FieldUsage` (with `FieldUsage.Payload(rule,
key)`) and `ResultTree`; `toString` is the core's text and `asJava` the Java
object. The enums are Java's (`Truth`, `Status`, `HitPolicy`, `GroupKind`...),
which match as `case Truth.FALSE =>`.

**Data.** Scala data is read as `spec/data-model.md` says (*Java and Scala*),
lazily: the layer hands the Java reader views, so only the paths the rules
read are read, each object once.

| Scala | Value |
|---|---|
| `None`; `Some(x)` | null (the key is there: `null`, not `missing_field`); `x` |
| `Int`, `Long`, `Short`, `Byte`, `BigInt` | int; beyond 64 bits, a null with reason `int_overflow` |
| `Double`, `Float` | float |
| `BigDecimal` | decimal, exact; beyond 38 digits, a null with reason `decimal_overflow` |
| `String`, `Boolean`, `java.time` | as in Java |
| `Seq` (`List`, `Vector`, `Range`...) and arrays (not `Array[Byte]`) | list |
| `Map` with `String` keys | map: a `SeqMap` (`ListMap`, `VectorMap`) or `SortedMap` keeps its order, any other map is sorted by key |
| case classes, and `enum` cases with parameters | map of their fields by name, in their order; `private` fields are left out |
| Scala 3 `enum` cases without parameters | string: the case's name |
| Java lists and maps | as in Java, with Scala values inside read as Scala values |
| sets, tuples, functions, `LazyList` (never forced), iterators and every other object of the Scala library; the binding's objects | opaque, given back as the very object |
| anything else | as the Java binding reads it (records, JavaBeans...); a Scala value inside a Java record or bean is opaque |

`None` is null and not missing because the field is there, holding nothing,
as with Java's `Optional.empty()`. An action's `data` is the very object given
to `run`. Values come back as Scala values: null as `None`, int `Long`, float
`Double`, decimal `BigDecimal` (exact, with an unlimited `MathContext`), list
`Vector`, map `VectorMap`, `java.time` as in Java and an opaque value as the
very object; a gap is a `Long`, `Double`, `BigDecimal`, `Period` or
`Duration`.

**Actions** are Scala functions, `type Action = ActionContext => Unit`: in a
rule (`action = Some(ctx => ...)`, `withAction`) or bound to names
(`rules.bind(Map("notify_sales" -> notify))`). The context gives `rule`,
`data`, `result`, `output`, `tree` and `decision` (equal to what `run` gives
back).

**Errors** are the Java binding's exceptions, raised as they are
(`ExpressionException`, `RuleDefinitionException`, `MissingFieldException`,
`ActionException`...; [Errors](#errors)): Scala has no checked exceptions, so
wrapping would only hide the hierarchy that `spec/errors.md` shares with every
language, and `scala.util.Try` takes them as they are. What an action throws is
the cause of the `ActionException`, whose decision is
`Decision.fromJava(error.decision().orElseThrow())`.

[`docs/examples/DiscountRules.scala`](../../docs/examples/DiscountRules.scala)
is `DiscountRules.java` in Scala; the Scala tests compile and run it.

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
./gradlew test    # builds the C ABI (cargo build -p cleanrules-ffi), then the JUnit and MUnit tests
./gradlew check   # also the javadoc, with -Xdoclint:all -Werror, and the Scaladoc
./gradlew jar     # java/build/libs/cleanrules-<version>.jar, scala/build/libs/cleanrules-scala_3-<version>.jar
```

The build is Gradle (Kotlin DSL) with the wrapper in the repository, one build
for Java (`java`) and Scala (`scala`, with Gradle's own `scala` plugin;
`settings.gradle.kts`). Java compiles with `--release 25` and
`-Xlint:all -Werror` whatever JDK runs it, and has no dependencies; the tests
use JUnit 5. Scala 3.3 LTS compiles with `-release 25 -feature -Wunused:all
-Wvalue-discard -Werror`, depends on the Scala 3 library only, and its tests
use MUnit (on JUnit 4). `-Pcleanrules.library.path=<file>` tests
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
| sets, `byte[]`, lambdas, threads, class loaders, classes, any other object of the Java platform or of the Scala library (`scala.*`), and the binding's own objects (its Scala layer's included) | opaque: never read through, compared by identity |

Dates outside years 1 to 9999 are opaque. An index beyond `Integer.MAX_VALUE`
(`a[3000000000]`, a `Long` in `fieldPaths()`) is beyond any Java list: it is
missing. A getter that throws gives a null
with reason `read_error`; in strict mode the `EvaluationException` has the
exception as its cause (also in a batch, for the row that failed). Nothing but
public data is read: no private member, no method with parameters, no static
member, and nothing that a class of the platform or of the Scala library
declares (`getClass()`), so a
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
