# cleanrules for Go

The Go binding of cleanrules: the same engine and the same concepts as every
other language (`Rule`, `RuleSet`, `Decision`, `Explain`, `Summary`), on the C
ABI ([`crates/ffi`](../../crates/ffi/README.md)) through cgo. Go builds
conditions, rules and decision tables in code or reads them from documents
(JSON, YAML, the rule syntax, CSV), reads Go data into the engine's values,
and gets decisions, explanations, summaries and result trees back as Go
values; the core reads, checks, evaluates, explains and summarizes everything
(no rule logic in Go).

Go 1.24 or later, standard library only; cgo, so a C compiler (see
*Platforms*). Module `github.com/cleanrules/cleanrules-go` (package
`cleanrules`), not published yet: it is published from its own repository,
`github.com/cleanrules/cleanrules-go`, with the prebuilt libraries below, so
that `go get` needs no Rust. This directory is where the package is written
and tested, under the same module path; *Go module* below says how the
published tree is assembled and checked.

## Installing

The module Go users get holds a prebuilt static library for each platform
(`lib/<os>_<arch>[_musl]/libcleanrules_ffi.a`) and the C ABI's header
(`include/`), and links the library of the platform it builds for: nothing to
build, no Rust. Once it is published:

```bash
go get github.com/cleanrules/cleanrules-go
```

Until then, CI assembles it as the artifact `go-module` of the workflow "Go
libraries" (`.github/workflows/go-libs.yml`; *Go module* below); use it with a
`replace` directive:

```text
require github.com/cleanrules/cleanrules-go v0.0.0
replace github.com/cleanrules/cleanrules-go => ../go-module
```

### Platforms

| Platform | Library | C compiler cgo needs |
|---|---|---|
| macOS x64, arm64 | `darwin_amd64`, `darwin_arm64` (macOS 11.0 or later, as Go 1.24) | Xcode's command line tools |
| Linux x64, arm64, glibc | `linux_amd64`, `linux_arm64` (glibc 2.28 or later) | gcc or clang |
| Linux x64, arm64, musl (Alpine) | `linux_amd64_musl`, `linux_arm64_musl`, with **`-tags cleanrules_musl`** | `apk add build-base` |
| Windows x64 | `windows_amd64`, for MinGW-w64's GCC (Rust's `x86_64-pc-windows-gnu`) | MinGW-w64 gcc (MSYS2, w64devkit...) |
| Windows arm64 | `windows_arm64`, for llvm-mingw (Rust's `aarch64-pc-windows-gnullvm`) | [llvm-mingw](https://github.com/mstorsjo/llvm-mingw), with `CC=aarch64-w64-mingw32-clang` |

The package needs cgo: `CGO_ENABLED=1` (Go's default when it finds a C
compiler; cross-compiling turns it off unless set) and the C compiler of the
table. Without cgo the build stops with `undefined:
cleanrules_needs_cgo__set_CGO_ENABLED_1_and_install_a_C_compiler__see_README_md_Platforms`.

Go has no GOOS for musl, so a program for Alpine (or any musl system) builds
with `go build -tags cleanrules_musl`; without it, the glibc library fails to
link there. On Windows, cgo's C compiler decides: GCC's MinGW-w64 on x64, and
on arm64 llvm-mingw, since GCC has no Windows on arm64 target (the libraries
of Rust's MSVC targets expect Microsoft's linker and C runtime instead). On
any other platform, Android and iOS included (whose builds also take the
files of Linux and macOS), the build stops with the error `undefined:
cleanrules_has_no_prebuilt_library_for_this_GOOS_and_GOARCH__see_README_md_Platforms`:
build the library there from the repository and link it as *Linking another
build* says.

The libraries are release builds with fat link-time optimization and one
codegen unit, stripped of debug information (but Windows on arm64's, which
`llvm-strip` cannot rewrite): 5 to 8 MB each, 12 MB on Windows on arm64,
about 60 MB for the eight (built with Rust 1.97 on macOS; CI builds them on
each platform). Each one links the system libraries that
`rustc --print native-static-libs` asks for on its target with Rust 1.97
(`link_*.go`, which CI checks against rustc, pinned to that version). `go mod vendor` works: the library and header
directories hold empty packages that the module imports, so that they are
copied.

## Building in this repository

In this repository the package links the static library that cargo builds,
by its full path (`link.go`), so contributors need no prebuilt library: build
that first. From the repository's root, on Linux and macOS:

```bash
cargo build -p cleanrules-ffi
(cd bindings/go && go test -count=1 ./...)
```

On Windows, build the library for the GNU target (only the static library,
which needs no linker of its own): `x86_64-pc-windows-gnu` on x64, and
`aarch64-pc-windows-gnullvm` on arm64 (with llvm-mingw as cgo's compiler):

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

### The prebuilt libraries

The tag `cleanrules_prebuilt` links the prebuilt libraries instead
(`link_<os>_<arch>.go`, `link_musl_linux_<arch>.go` and
`include_prebuilt.go`), from `lib/` and `include/` in `bindings/go`
(ignored by git). To build them and the module, as CI does, from the
repository's root:

```bash
rustup component add llvm-tools
bash bindings/go/scripts/build-lib.sh aarch64-apple-darwin darwin_arm64 go-libs   # one per platform
python3 bindings/go/scripts/assemble.py go-libs/libs go-module                    # needs all eight
python3 bindings/go/scripts/check-module.py go-module                             # --probes go-libs/probes for Linux
bash bindings/go/scripts/smoke.sh go-module /tmp/smoke                            # [cleanrules_musl]
```

`build-lib.sh` builds a platform's library in release (`cargo rustc
--crate-type staticlib --target ...`, which needs no linker of the target, so
every library can be built on any host that has its Rust target), checks
that its link file links every system library rustc asks for, strips it and,
with `--probe` on Linux, links and runs `scripts/probe.c` with it.
`assemble.py` writes the module: the package's Go files as they build with
`cleanrules_prebuilt` (the tag removed; `link.go` and `include.go` left out),
the header, the libraries and their empty packages, and the rest of *Go
module* below. `check-libs.py` checks its libraries: exactly the eight, each
an archive of objects of its system and processor, macOS 11.0 at most,
glibc's own symbols in the glibc ones and none in the musl ones, and, from the
probes CI links in manylinux_2_28 and Alpine, glibc 2.28 at most or musl's
libc; `check-module.py` runs those checks and the module's own. `smoke.sh`
uses the module from a clean module (also vendored), then runs this
package's tests with the tag. The workflow "Go libraries" runs all of it on
every platform, and publishes nothing.

### Linking another build

To link another build, such as a release one, build with the tag
`cleanrules_external` and give the library and the system libraries that
Rust's standard library needs in `CGO_LDFLAGS`. From the repository's root,
on macOS:

```bash
cargo build -p cleanrules-ffi --release
(cd bindings/go && CGO_LDFLAGS="$(git rev-parse --show-toplevel)/target/release/libcleanrules_ffi.a -lm" \
    go test -count=1 -tags cleanrules_external ./...)
```

The system libraries of each platform are those of `link.go` and of the
`link_*.go` files, which are the ones
`cargo rustc -p cleanrules-ffi --lib --crate-type staticlib -- --print native-static-libs`
prints with Rust 1.97 (without the C library, which the compiler links
anyway): Linux with glibc `-lgcc_s -lutil -lrt -lpthread -lm -ldl`, with musl
`-lgcc_s`, macOS `-lm`, Windows x64 `-lkernel32 -lntdll -luserenv -lws2_32
-ldbghelp`, and Windows arm64 the same and `-lunwind`. Link the static
library by its path rather than with `-L` and `-l`: cargo writes the shared
library next to it, which the linkers of Linux and macOS would prefer, and
which the program then has to find when it runs.

## Go module

Go users get the package from its own repository,
`github.com/cleanrules/cleanrules-go`, whose tagged commits hold the eight
prebuilt libraries (as go-duckdb does), so that `go get` needs no Rust. This
directory has the same module path, so the examples and tests here import
what Go users import. Nothing is published yet: the workflow "Go libraries"
assembles the tree that repository will hold, checks it and resolves it from
a clean module; the repository, its pushes and its tags come with the first
release. From the repository's root, with the eight libraries built as *The
prebuilt libraries* says:

```bash
python3 bindings/go/scripts/assemble.py go-libs/libs go-module
python3 bindings/go/scripts/check-module.py go-module [--probes go-libs/probes] [--release 0.1.0]
bash bindings/go/scripts/smoke.sh go-module /tmp/smoke [cleanrules_musl]
```

| Path | What it is |
|---|---|
| `go.mod` | `module github.com/cleanrules/cleanrules-go` and the go directive of this directory's `go.mod` (1.24); nothing else: the module needs no other |
| `*.go` | the package's files but its tests, as they build with `cleanrules_prebuilt` (the tag removed; `link.go` and `include.go` left out) |
| `vendor.go`, `include/include.go`, `lib/<platform>/lib.go` | empty packages, imported so that `go mod vendor` copies the header and the libraries |
| `include/cleanrules.h` | the C ABI's header |
| `lib/<os>_<arch>[_musl]/libcleanrules_ffi.a` | the eight prebuilt libraries (*Platforms*) |
| `README.md` | [`scripts/README.module.md`](scripts/README.module.md): for Go users, with absolute links (installing, an example, platforms, tags, versions) |
| `LICENSE`, `NOTICE` | the repository's |
| `VERSION` | the workspace's version (`Cargo.toml`), that of the tag `v<VERSION>` and the one `Version()` reports |

The tree is about 56 MiB, nearly all of it the libraries (4.8 to 7.9 MB each,
11.9 MB on Windows on arm64); about 15 MiB compressed, as the module zip that
Go's proxy serves.

`check-module.py` (standard library only; it loads `assemble.py` and
`check-libs.py`) checks the tree against this repository and against what Go,
its module proxy and GitHub require:

- exactly the files above, nothing else: each Go file the same text that
  `assemble.py` writes from this directory, no tests, no symbolic links or
  empty directories, names that a module zip and every file system take,
  distinct when case is ignored;
- Go's limits for a module (500 MiB, 16 MiB for `go.mod` and `LICENSE`) and
  GitHub's for a file (100 MiB, and a warning past 50 MiB);
- `go.mod` with only the module path and go directive (no `require`,
  `replace`, `exclude`, `retract`, `toolchain`, `godebug` or `tool`);
- `VERSION`, the workspace's version, a semantic version as Go's (and from
  2.0.0 on, a module path ending in `/v<major>`, which Go would require);
  with `--release <version>`, that version and not the placeholder 0.0.0,
  which is otherwise a warning;
- `README.md`: only absolute https links, those into
  `github.com/cleanrules/cleanrules` to files, directories and headings
  that exist on `main`; `go get` and the import of the module path; a
  *Platforms* section (the build errors of `link_unsupported.go` and
  `cgo_required.go` point there) whose table is this README's, the same text;
  the tags `cleanrules_musl` and `cleanrules_external`; an example with
  `package main`; no task of the plan;
- `LICENSE`, `NOTICE` and the header, the repository's byte for byte; no
  text file naming the package's former module path, a university or an
  affiliation;
- the libraries, as `check-libs.py` checks them (and the probes, with
  `--probes`).

`smoke.sh` makes a clean module outside the repository whose `go.mod`
requires `github.com/cleanrules/cleanrules-go v0.0.0` and replaces it with the
tree, under `GOPROXY=off`, `GOSUMDB=off`, `GOFLAGS=-mod=mod`,
`GOTOOLCHAIN=local` and `GOWORK=off`, so nothing can be fetched: `go list -m
all` must show the replacement and no other module, and no `go.sum` is
written. It runs `go vet`, builds and runs `scripts/smoke/main.go` (which
checks that the linked library reports the tree's `VERSION`), runs it again
with `go run`, and runs the README's example; then again from `vendor/`
(`go mod vendor` must copy the header and every library), and this package's
tests with `cleanrules_prebuilt`. CI ("Go libraries") assembles the tree from
the libraries built on each platform, checks it with the probes (and
`--release` on tags, so a tag with the version 0.0.0 fails), uploads it as
`go-module`, and runs `smoke.sh` on Linux, macOS and Windows, on x64 and
arm64, and in Alpine with `cleanrules_musl`. Nothing is pushed, tagged or
published.

## Conditions and rules in code

Go has no operator overloading, so conditions are functions and methods, as
fuzzytool's `&`, `|` and `~` become `And`, `Or` and `Not`:

```go
import cr "github.com/cleanrules/cleanrules-go"

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

### How much Go data is read

Data can share objects (the same slice, map or pointer in many places), and
a value of the core shares nothing, so each place is read (or copied) again:
a "diamond" that holds one object twice at each of 30 levels would be 2^31
values. So what an evaluation reads from the data of one input is counted
against `Limits.MaxValues`, 1,000,000 values by default
([`spec/data-model.md`](../../spec/data-model.md), *Reading native data*,
*Limits*):

- each value placed where a path ends, or where it stops early (a scalar, a
  `nil`, an opaque value, a null with a reason), with everything inside it: a
  slice, map or struct counts one, and each element, entry value or field
  one more;
- each position of a slice or an array the paths go through (its length:
  the core holds a value at every position), checked before anything is
  allocated, so `make([]struct{}, math.MaxInt)` is refused at once;
- a shared object each time it is placed, and so does each copy of it, whole;
- not the maps and structs the paths go through, their keys, or what is
  missing.

Beyond the limit the input is refused, not evaluated, in strict mode and
outside it: the evaluation gives a `limit_exceeded` error (`ErrLimitExceeded`,
an `ErrFormat`) whose `Name` is `"max_values"`, with the `Row` in a batch
(the first row beyond it, before any error of the evaluation, since every
row is read before any is evaluated). `WithLimits` sets it for
`RuleSet.Evaluate`, `EvaluateMany` and `Run`, `DecisionTable.Evaluate` and
`EvaluateMany`, and `EvaluateTree` and `EvaluateValueTree`; Go data has a
limit of its own, so there only `MaxValues` acts (the other fields bound
documents):

```go
decision, err := rules.Evaluate(data, cr.WithLimits(cr.Limits{MaxValues: 10_000}))
if errors.Is(err, cr.ErrLimitExceeded) {
    // too much data for one input: refused
}
```

A limit is never off, and 0 is the default, as for every field of `Limits`.
Constants (`Const`, the Go values of comparisons and `Then`, `IsIn`), tags
and metadata are read under the default: beyond it, building the node or
rule gives the same `limit_exceeded`, which names the rule for `WithTags` and
`WithMetadata`.

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

## Documents

Conditions, rules, rule sets and decision tables are read from and written to
documents: JSON (`spec/json-format.md`), the contract between languages, YAML
with the same documents, the rule syntax (`spec/text.md`) and CSV for
decision tables (`spec/tables.md`). The core reads and writes them, strictly
and within limits.

```go
doc, err := cr.Load("pricing.yaml", cr.WithActions(cr.Actions{"notify_sales": notify}))
switch doc := doc.(type) {                 // a cr.Document
case *cr.RuleSet:                          // bound: doc.Run(order) calls notify
case *cr.DecisionTable:
case *cr.Rule:
case *cr.Expr:                             // a condition document
}
doc, err = cr.Loads(text, cr.WithFormat(cr.FormatText), cr.WithLimits(cr.Limits{MaxDepth: 32}))
yaml, err := cr.Dumps(rules, cr.WithFormat(cr.FormatYAML))
err = cr.Dump(rules, "pricing.rules")      // the format of the extension
vip, err := cr.RuleFromText("RULE vip IF customer.years > 5 THEN discount = 0.15")
recorded, err := rules.ReadDecision(decisionJSON)   // checked against its rule set
```

`Loads` takes a string, as `RuleFromText` and `ReadDecision` do (give bytes
as `string(b)`), and gives a `Document`: an interface that only `*Expr`, `*Rule`, `*RuleSet` and
`*DecisionTable` implement, read with a type switch or `doc.(*cr.RuleSet)`,
the way Go reads a union (there are no typed variants: a type assertion is
one line). `Load` and `Dump` take the format of the file's extension (`.json`,
`.yaml`, `.yml`, `.rules`, `.csv`) unless `WithFormat` gives one, and `Load`
refuses a file beyond the size limit of its format before reading it
(`limit_exceeded`, whose hint names the `Limits` field to raise). A file that
cannot be read gives the `*fs.PathError` of `os`, and an extension that tells
no format a plain error: neither is a `*cr.Error`.

Each function takes options of its own type, and an option that means the
same for several is a value of all their types, so the compiler rejects one
where it does not apply: `WithFormat` (`FormatJSON` by default, `FormatYAML`,
`FormatText`, `FormatCSV`) for `Loads`, `Load`, `Dumps`, `Dump` and
`ReadDecision`; `WithLimits` for those that read, `NewDecisionTable`, the
evaluations (`Evaluate`, `EvaluateMany` and `Run`, of rule sets and tables)
and result trees;
`WithActions` for `Loads`, `Load` and `RuleFromText`. A nil option is a
`rule_definition_error` whose hint lists the function's options.

`Limits` mirrors the C ABI's limits (`MaxSize`, `MaxNesting`, `MaxDepth`,
`MaxTableSize`, `MaxYAMLSize`, `MaxTextSize`, and `MaxValues` for the Go data
of evaluations, above). A field left at 0 keeps the core's default, and a
negative one is a `format_error` that names it.
`MaxSize` is the size limit of every format at once, and `MaxYAMLSize` and
`MaxTextSize`, when set, change those of YAML and of text and CSV:

```go
cr.Limits{MaxSize: 1 << 20}                    // JSON, YAML and text: 1 MiB
cr.Limits{MaxSize: 1 << 20, MaxYAMLSize: 4096} // JSON and text 1 MiB, YAML 4 KiB
limits := cr.DefaultLimits()                   // every field set, to the defaults:
limits.MaxSize = 64 << 20                      // so this raises JSON's only
```

`DefaultLimits` gives the real defaults (MaxSize the JSON one) rather than 0
for "follow MaxSize": a `Limits` made from it means the defaults exactly, and
YAML keeps its smaller default (1 MiB) unless changed.

Rules read from a document keep the core's own rules, so they write back
exactly. A rule's action is a name, never code: `WithActions` binds the names
of a rule set (as `Bind` does, so a missing name is a `registry_error`); a
`*Rule` holds no function in Go, so for a rule it only checks the name, and a
decision table has no actions. A Go rule only ever names its action, which
is always written as its name, so writing never refuses a rule for its
action and Go has no `omit_actions` option (`spec/json-format.md`, *Writing
errors*).

## Decision tables

```go
table, err := cr.NewDecisionTable(
    []any{"RULE", cr.Field("customer", "years"), "order.total", "THEN discount",
        cr.TableOutput{Name: "risk", Values: []any{"high", "medium", "low"}}},
    [][]any{
        {"new", cr.Field("customer", "years").Lt(1), "> 1000", 0, "'high'"},
        {"vip", "> 5", "-", cr.MustDecimal("0.15"), "'low'"},
        {"big", nil, "> 5000", cr.MustDecimal("0.05"), Medium},   // type Risk string
    },
    cr.WithTablePolicy(cr.TablePolicyPriority))
decision, err := table.Evaluate(order)
csv, err := cr.Dumps(table, cr.WithFormat(cr.FormatCSV))
```

A table is a grid (`spec/tables.md`, *Grids*). Each header item is a CSV
header text (`"RULE"`, `"PRIORITY"`, `"DESCRIPTION"`, `"THEN key"`,
`"THEN key IN (...)"` or an input expression; through pointers, and a named
string type too), an input `*Expr`, or a `TableOutput` (or a pointer to one)
with its output values, highest priority first. Each row holds cells in column order, and may be shorter than the
header:

| Cell | Meaning |
|---|---|
| `string` (also through a pointer) | text, as in the CSV: `"> 5"`, `"-"`, `"'VIP'"` (the string), `"VIP"` (the field) |
| `nil`, a nil pointer | empty (a nil `*Expr` is an `expression_error`, as everywhere else) |
| `*Expr` | under an input, its condition, which must test its column; under an output, its value |
| a named string type (`type Risk string`) | a value, its string: under an input or an output a constant (`'medium'`), under `RULE` or `DESCRIPTION` the name or description, under `PRIORITY` a `format_error`; Go's enums are never field names |
| any other value | a constant, as `Const` reads it: under an input `x`, the test `x = value`; under `PRIORITY`, an int |

The core decides what a value means by its column: the package gives each
cell with its kind (text, value or node, `cr_grid_push_row_cells`), as Java
gives its enum constants.

Errors point into the grid, counting from 0: `/header/<c>`, `/rows/<r>/<c>`,
`/rows/<r>`, `/aggregate`. Options: `WithTablePolicy` (`TablePolicyFirst` by
default, ...`Priority`, ...`Unique`, ...`Any`, ...`RuleOrder`,
...`OutputOrder`, ...`Collect`), `WithAggregate`, `WithLogic`, `WithStrict`
and `WithLimits`. A table gives `HitPolicy`, `Aggregate`, `Logic`, `Strict`,
`Inputs`, `Outputs` (output values as Go values; nil for any value), `Len`,
`Rows` (`TableRow`: `Name`, `Priority` as `*int64`, `Description`, and cells
as `*Expr`, nil for an empty one), `RuleSet` (compiled once), `Evaluate`,
`EvaluateMany`, `Summary`, `JSON` and `Close`.

## Result trees

```go
tree, err := cond.EvaluateTree(order, cr.WithLogic("boolean"), cr.WithStrict(false))
tree.Truth                                 // *cr.Truth: nil for a value
tree.Children[1].Reason.Code               // "missing_field"
tree.Children[1].Node()                    // the node evaluated: order.total > 1000
tree.JSON()                                // the core's JSON of this subtree
value, err := cr.Field("order", "total").Mul(0.9).EvaluateValueTree(order)
```

`EvaluateTree` evaluates a condition and `EvaluateValueTree` a value, on Go
data read along the node's paths (`spec/ast.md`, *Evaluating one node*). A
`ResultTree` is a struct of exported fields, the tree's own (changing them
changes nothing else): `Tag` (the node's tag, the `op` of its JSON form),
`Path` (a field's), `Truth` (nil for a value), `Value` (as values are given
back), `Reason` and `Children`; and two methods that read the native tree,
`Node()` and `JSON()`. `EvaluateTree` takes `WithLogic`, `WithStrict` and
`WithLimits` (its `MaxValues`), `EvaluateValueTree` only `WithStrict` and
`WithLimits` (a value has no truth to combine). In an action, `ctx.Tree()` is the result tree of the rule's
condition in the decision.

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
`*Error`, is a rule set, decision or decision table used after `Close`, or a
nil or zero one.

The core writes the messages. The package writes these few itself, as the
other bindings do: a NaN constant and a time finer than a microsecond (which
a value cannot carry; in a table, with the cell's pointer), an unknown
`Closed` or `Format`, a nil or zero `*Expr`, `*Rule`, `Document` or option, a
header item of another type, a file beyond its size limit, a batch with more
rows than memory can hold (`evaluation_error`; the rows are not counted, as
each is an input of its own), and
`rule_not_found` from `WhyNot` and `Explanation.ForRule` (in the core's
words, from the names a decision keeps). The C ABI writes no names of its
functions in messages, so a Go program never sees one.

## Goroutines and memory

`Expr`, `Rule`, `RuleSet`, `Decision`, `DecisionTable` and `ResultTree`
never change once made: any number of goroutines may use one at once. Nil and
zero values give errors, never panics. `RuleSet.Close`, `Decision.Close` and
`DecisionTable.Close` (which closes the table's rule set too) free
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
`Close`, cleanups and shared ownership, nil and zero values, concurrency,
documents (each kind in each format both ways, files, limits, actions on
loading, `RuleFromText`, `ReadDecision`), decision tables (each kind of cell,
errors with their pointers, settings, formats), result trees and
`ActionContext.Tree`, `Limits.MaxValues` (a diamond 30 levels deep refused at
once, what counts at the limit and one over it, lengths beyond memory,
batches, constants and metadata), which files link the library on each
platform and tag, and the examples
[`docs/examples/discounts.go`](../../docs/examples/discounts.go) and
[`docs/examples/risk_table.go`](../../docs/examples/risk_table.go).

### Conformance

`TestConformance` runs every vector of
[`conformance/vectors`](../../conformance/vectors) twice (alone:
`go test -count=1 -run '^TestConformance' .`; about 6 s with a debug library):

- `json:<id>`: documents, inputs and results cross as JSON text. `Loads` and
  `Dumps` read and write JSON, YAML, the rule syntax and CSV,
  `RuleSet.ReadDecision` reads decisions, the package's unexported JSON entry
  points (`jsonentry.go`, the C ABI's `cr_ruleset_evaluate`,
  `cr_value_from_json` and `cr_node_evaluate_tree`) evaluate the inputs, and
  `Decision.JSON`, `Explanation.JSON`, `Summary.JSON`, `ResultTree` documents
  and the texts are compared exactly; decisions are also read with their
  methods.
- `native:<id>`: conditions, rules and rule sets are built in code (`Field`,
  `Const`, the methods of `Expr`, `NewRule` and its options, `NewRuleSet`)
  and checked against the core's reading of the same JSON; inputs are Go data
  in turn: every integer type, a named one, `big.Int` and `*big.Int`,
  `float32` and `float64`, `Decimal`, `Date`, `DateTime`, `time.Time` (in
  `time.UTC` for `Z`, in a zone by name, and in a fixed zone, unnamed, named
  or `Etc/GMT`, for an offset), slices, arrays, maps
  (`map[string]any`, `map[any]any`, typed and named maps) and structs (whose
  fields keep the order of a map that is not sorted), with pointers and nils
  of several types. Results are compared as the Go values given back
  (`spec/data-model.md`, *Go*), checking each Go type (`int64`, `float64`,
  `[]any`, `map[string]any` without order...), through `Decision`,
  `Explanation`, `Summary` and `ResultTree`, and as their JSON forms. `read`
  vectors go through `Loads`, `Dumps`, `RuleFromText`, `NewDecisionTable`
  (grids) and `ReadDecision`.

The comparison follows
[`conformance/README.md`](../../conformance/README.md) (*Comparing*), with
the runner's own JSON reader (order kept, numbers as written), never the
package's equality. Each failure names its route and id, with what came back
and what the vector expects. Skips are counted and listed with their reason:
by JSON, the decision table grids (Go data) and the `max_values` vectors
(native data only); by Go data, the one vector with a limit of 0, which Go
cannot give (0 keeps the default). The report, with the counts and the Go
forms used, is logged and written to `build/reports/conformance.txt` (ignored
by git). `TestConformanceComparator` checks the comparison itself, and
`TestConformanceWrongVectors` that vectors with an altered expected output
fail by both routes.
