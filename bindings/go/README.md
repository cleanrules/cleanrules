# cleanrules for Go

The Go binding of cleanrules: the same engine and the same concepts as every
other language (`Rule`, `RuleSet`, `Decision`, `Explain`, `Summary`), on the C
ABI ([`crates/ffi`](../../crates/ffi/README.md)) through cgo. Go builds
conditions and rules in code, reads Go data into the engine's values, and gets
decisions, explanations and summaries back as Go values; the core checks,
evaluates, explains and summarizes everything (no rule logic in Go).

Go 1.24 or later, standard library only. Module
`github.com/cleanrules/cleanrules/bindings/go` (package `cleanrules`), not
published yet: task H.2 publishes it from its own repository,
`github.com/cleanrules/cleanrules-go`, with prebuilt libraries. This first
part (task H.1a) builds rules in code; documents (`loads`/`dumps`),
`RuleFromText`, decision tables, result trees and the prebuilt libraries come
with task H.1b.

## Building

The package links the static library that cargo builds in this repository,
by its full path (`link.go`), so build that first. From the repository's
root, on Linux and macOS:

```bash
cargo build -p cleanrules-ffi
(cd bindings/go && go test -count=1 ./...)
```

On Windows, cgo compiles and links with MinGW-w64's `gcc`, whose objects and C
runtime are GNU's; the library of Rust's default MSVC target expects
Microsoft's runtime and linker. So build the library for the GNU target (only
the static library, which needs no linker of its own):

```bash
rustup target add x86_64-pc-windows-gnu
cargo rustc -p cleanrules-ffi --lib --crate-type staticlib --target x86_64-pc-windows-gnu
(cd bindings/go && go test -count=1 ./...)
```

cgo needs a C compiler on the `PATH` (`CGO_ENABLED=1`, the default when one
is there).

**After rebuilding the library, test with `-count=1`** (or run
`go clean -testcache` first). Go's cache keys a test's result on the flags in
`#cgo LDFLAGS`, not on the bytes of the library they name, so a plain
`go test` after a change in Rust can report the earlier result as
`ok (cached)`. With `-count=1` the test binary is linked and run again; CI
always uses it.

### Linking another build

`link.go` links `target/debug` (or `target/x86_64-pc-windows-gnu/debug`) of
this repository. To link another build, such as a release one, build with the
tag `cleanrules_external` and give the library and the system libraries that
Rust's standard library needs in `CGO_LDFLAGS`. From the repository's root,
on macOS:

```bash
cargo build -p cleanrules-ffi --release
(cd bindings/go && CGO_LDFLAGS="$(git rev-parse --show-toplevel)/target/release/libcleanrules_ffi.a -lm" \
    go test -count=1 -tags cleanrules_external ./...)
```

The system libraries of each platform are those of `link.go`, which are the
ones `cargo rustc -p cleanrules-ffi --lib --crate-type staticlib -- --print native-static-libs`
prints with Rust 1.97 (without the C library, which the compiler links
anyway): Linux `-lgcc_s -lutil -lrt -lpthread -lm -ldl`, macOS `-lm`, Windows
`-lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp`. CI prints the list on each
platform and warns when it differs from `link.go`. Link the static library by
its path rather than with `-L` and `-l`: cargo writes the shared library next
to it, which the linkers of Linux and macOS would prefer, and which the
program then has to find when it runs.

## Conditions and rules in code

Go has no operator overloading, so conditions are functions and methods, as
fuzzytool's `&`, `|` and `~` become `And`, `Or` and `Not`:

```go
import cr "github.com/cleanrules/cleanrules/bindings/go"

adult := cr.Field("customer", "age").Ge(18)          // a path, never split at dots
cond := adult.And(cr.Field("country").IsIn("ES", "PT"))
fmt.Println(cond)                                    // customer.age >= 18 AND country IN ("ES", "PT")
cr.Not(cr.Field("blocked")).Or(cr.Field("vip"))
cr.Field("order", "total").Mul(0.9).Between(10, 100)
cr.Field("items", 0, "sku").StartsWith("A-")
```

| Go | Node |
|---|---|
| `Field("a", 0, "b")` | `field` |
| `Const(v)` (and any Go value given as an operand) | `const` |
| `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge` | comparisons |
| `And(...)`, `Or(...)`, `Not(x)`, and the methods `x.And(...)`, `x.Or(...)`, `x.Not()` | connectives |
| `IsIn(values...)` (or one slice of them), `Between(lo, hi)`, `BetweenClosed(lo, hi, ClosedLeft)` | `in`, `between` |
| `IsNull()`, `Matches(pattern)`, `StartsWith`, `EndsWith`, `Contains` | tests |
| `Add`, `Sub`, `Mul`, `Div`, `Neg()`, `Abs()` | arithmetic |

The core checks each node as it is built (`spec/ast.md`, *Construction
rules*). A node that breaks a rule, such as `Field("x").Add("a")` or a NaN
constant, carries an `*Error`; every node built on it carries the same error,
which `Err()` gives and `NewRule` returns. So an expression is written in one
chain and checked once. `BetweenClosed` takes `ClosedBoth`, `ClosedLeft`,
`ClosedRight` or `ClosedNone`; another value is an `expression_error`. `String()` is the core's text of the node; `Tag()`,
`Kind()`, `Children()`, `FieldPaths()`, `Equal()` (structural identity: `Eq`
builds a node, `Equal` compares) and `Hash()` read it.

Rules and rule sets take **functional options**, applied in order, and return
an error:

```go
vip, err := cr.NewRule("vip", cond,
    cr.Then("discount", cr.MustDecimal("0.15")),    // a constant or an *Expr
    cr.WithPriority(10),
    cr.WithTags("loyalty"),
    cr.WithDescription("loyal customers"),
    cr.WithMetadata(map[string]any{"owner": "sales"}),
    cr.WithAction("notify_sales"))
rules, err := cr.NewRuleSet([]*cr.Rule{vip, loyal},
    cr.WithHitPolicy(cr.HitPolicyPriority),           // HitPolicyFirst (default), ...Priority, ...Unique, ...Any, ...RuleOrder, ...Collect
    cr.WithAggregate(cr.AggregateSum),                // with HitPolicyCollect: AggregateSum, ...Min, ...Max, ...Count
    cr.WithLogic("boolean"),
    cr.WithStrict(true))
```

`rule.With(options...)` gives a changed copy; a rule never changes. For rules
written in the program, `cr.Must(...)` panics on an error, as
`template.Must` does; check the error where rules come from users. A rule set
gives `Len`, `Rules`, `Rule(name)` (a `rule_not_found` error for an unknown
name), `HitPolicy`, `Aggregate`, `Logic`, `Strict` and `FieldPaths`.

## Deciding on Go data

```go
type Customer struct {
    Years      int  `json:"years"`
    Newsletter bool `cleanrules:"newsletter"`
    notes      string                                 // never read
}
decision, err := rules.Evaluate(map[string]any{
    "customer": Customer{Years: 7},
    "order":    map[string]any{"total": cr.MustDecimal("1500.00")},
})
out, err := decision.Output()                         // map[discount:0.15], a cr.Decimal
decision.Fired()                                      // [vip loyal]
decision.Results()                                    // each rule: Rule, Fired, Truth, Output
explanation, err := decision.Explain()
fmt.Println(explanation)                              // the core's text
why, err := decision.WhyNot("vip")
for _, cause := range why.Causes() {
    fmt.Println(cause, cause.Counterfactual)          // what it read, what it needed
}
summary, err := rules.Summary()
text, err := decision.JSON()                          // the decision document
explanation.JSON()                                    // the explanation document, whose
                                                      // Cause.At points into decision.JSON()
```

`WhyNot(name)` is `Explain().ForRule(name)`: it needs no call to the rule
set, so a decision explains itself after its rule set is closed. The maps and
slices a decision gives (`Results`, `Selected`, `Outputs`, `Output`,
`Aggregated`, and an action's `Output`) are copies, except opaque values,
which are the native objects themselves.

Go data is read as [`spec/data-model.md`](../../spec/data-model.md) (*Go*)
says, and only along the paths the rule set reads (`FieldPaths`): maps by
their string keys (a `map[any]any`, as YAML decoders give, by the keys that
are strings; other keys are left out), slices and arrays, and structs by
their exported fields, as `encoding/json` chooses them (promoted fields of
embedded structs included), named by a `cleanrules:"name"` tag, else a
`json` tag, else the field's name; a tag `"-"` leaves a field out, and a
`cleanrules` tag, when there is one, is the only tag read. Unexported fields
are never read. Pointers and interfaces are followed, and named types are
read by their kind. Where a path ends, a container is read whole, once per
input even when the data shares it in several places; one that contains
itself, or is nested more than 256 deep, is opaque there.

| Go | Value | Given back as |
|---|---|---|
| `nil` | null | `nil` |
| `bool` | boolean | `bool` |
| integers, `big.Int` (beyond 64 bits: null with `int_overflow`) | int | `int64` |
| `float32`, `float64` (NaN: null) | float | `float64` |
| `cr.Decimal` (`ParseDecimal`, `MustDecimal`) | decimal | `cr.Decimal` |
| `string` | string | `string` |
| `cr.Date` | date | `cr.Date` |
| `cr.DateTime` (no zone) | naive datetime | `cr.DateTime` |
| `time.Time` (`time.UTC` is `Z`; `time.Local` and `time.FixedZone`, their offset; a loaded location, its zone) | aware datetime | `time.Time`, in its zone when Go knows it |
| slices, arrays | list | `[]any` |
| maps (their string keys), structs | map | `map[string]any` |
| `[]byte`, functions, channels, `time.Duration`, … | opaque | the same value |

A counterfactual's gap is an `int64`, `float64` or `cr.Decimal`, a
`cr.GapDays` between dates or a `cr.GapMicroseconds` between datetimes.

`Decimal`, `Date`, `DateTime` and `Truth` implement `encoding.TextMarshaler`
and `TextUnmarshaler`, so `encoding/json` writes them as their text (`"0.15"`,
`"2026-10-31"`, `"2026-10-31T09:30:00"`, `"true"`) and reads them back.
Decimals compare with `==` by their canonical text: `cr.Decimal{}` is 0 and
equals `cr.MustDecimal("0")`; `"1.0"` and `"1.00"` keep their scale and
differ.

## Actions

A rule names its action (`WithAction`), and the program binds names to Go
functions; nothing else is ever called:

```go
bound, err := rules.Bind(cr.Actions{
    "notify_sales": func(ctx *cr.ActionContext) error {
        return mail(ctx.Rule().Name(), ctx.Output()["discount"])
    },
})
decision, err := bound.Run(order)
```

`Bind` checks that every name a rule gives is registered (a `registry_error`
otherwise; a nil function, and the name `""`, which no rule can give, are
left out). `Run` evaluates, then calls the actions of the selected rules in
rule order, each with an `ActionContext` (`Rule`, `Position`, `Data`,
`Result`, `Output`, `Decision`, the same decision `Run` gives back). Actions
run through cgo: the library calls one trampoline (`//export`), which finds
the Go function through a `cgo.Handle` held in C memory, so the library never
holds a Go pointer. The trampoline runs each action on a goroutine of its own
and waits for it, so nothing the action does can unwind through the core's
frames. An action's error stops the run with an `action_error` that wraps it
(`errors.Is(err, yourErr)` holds), with `Executed` and the `Decision`. A
panic, or `runtime.Goexit` (what `t.FailNow` and `t.Fatal` call), stops the
run too, and `Run` then panics again with the same value, or calls
`runtime.Goexit`, on its own goroutine.

## Errors

Errors are values: every error of cleanrules is an `*cr.Error` (except
`ErrClosed`, below) with its `Code` (`cr.CodeMissingField`...), `Message` and
`Hint` written by the core, the context (`Pointer`, `Line`, `Column`, `Rule`,
`Rules`, `Path`, `Types`, `Row`, `Name`, `Available`, `Executed`, `Node`,
`Operation`, `Reason`, `Cause`) and `JSON`, its JSON form
([`spec/errors.md`](../../spec/errors.md)). A field that does not apply is
empty: `Pointer` is nil (`""` is the document's root), `Line` and `Column`
are 0, and `Row` is -1 outside a batch.

```go
_, err := strict.Evaluate(data)
var e *cr.Error
if errors.As(err, &e) {
    fmt.Println(e.Code, e.Rule, e.Path)    // missing_field adult [customer age]
}
errors.Is(err, cr.ErrMissingField)         // true
errors.Is(err, cr.ErrEvaluation)          // true: its parent
```

Each code has a sentinel (`ErrFormat`, `ErrExpression`, `ErrRuleDefinition`,
`ErrRegistry`, `ErrRuleNotFound`, `ErrEvaluation`, `ErrMissingField`,
`ErrTypeMismatch`, `ErrHitPolicy`, `ErrAction`, `ErrUnsupported`, ...), which
`errors.Is` matches for the code and its descendants. `ErrInvalidArgument`
(an argument the C ABI refuses, which the package checks first) and
`ErrInternal` are bugs: please report them. `ErrClosed`, which is not an
`*Error`, is a rule set or decision used after `Close`, or a nil or zero one.

The core writes the messages. The package writes these few itself, as the
other bindings do: a NaN constant and a time finer than a microsecond (which
a value cannot carry), an unknown `Closed`, a nil or zero `*Expr`, `*Rule` or
option, and `rule_not_found` from `WhyNot` and `Explanation.ForRule` (in the
core's words, from the names a decision keeps). A Go program never sees the
name of a C function in a message.

## Goroutines and memory

`Expr`, `Rule`, `RuleSet` and `Decision` never change once made: any number
of goroutines may use one at once. Nil and zero values give errors, never
panics. `RuleSet.Close` and `Decision.Close` free
the native memory now (when the calls using it return, even on other
goroutines); later calls give `ErrClosed`, and decisions outlive their rule
set. Without `Close`, the memory is freed once the Go object is unreachable
(`runtime.AddCleanup`). `Bind` shares the native rule set: closing either
closes both. An explanation is made once per decision and kept.

## Tests

From the repository's root:

```bash
cargo build -p cleanrules-ffi
cd bindings/go
test -z "$(gofmt -l .)" && go vet ./... && go test -count=1 ./...
go test -count=1 -race ./...                         # also clean
GOEXPERIMENT=cgocheck2 go test -count=1 ./...        # every pointer passed to C checked
```

The tests cover every node and its text, constants and the errors of
construction, rules and rule sets, each Go kind both ways, structs, opaque
values, errors with their codes and context, actions (an error, an error
whose `Error` panics, a panic, `runtime.Goexit`, nested runs, goroutines),
`Close`, cleanups and shared ownership, nil and zero values, concurrency, the
example
[`docs/examples/discounts.go`](../../docs/examples/discounts.go) and a smoke
pass over 623 conformance vectors (`rules/`, `explain/`, and the generated
rule sets and explanations) built with this API and Go data. The full runner
is task H.2.
