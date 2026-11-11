# cleanrules for Go

The Go module of [cleanrules](https://cleanrules.github.io), an explainable
rules engine whose single implementation is a Rust core, with the same
semantics in every language it supports. Conditions are data: the core
checks, evaluates, explains and summarizes them, and the same rules give the
same decisions, explanations and summaries in Go as everywhere else. The
module builds conditions, rules and decision tables in code or reads them from
documents (JSON, YAML, the rule syntax, CSV), reads Go data into the engine's
values, and gives decisions, explanations, summaries and result trees back as
Go values, with errors as values.

This repository holds the module as Go users get it: the package's Go files
and a prebuilt static library of the core for each platform, so `go get`
needs no Rust. It is assembled from
[`bindings/go`](https://github.com/cleanrules/cleanrules/tree/main/bindings/go)
of the [main repository](https://github.com/cleanrules/cleanrules), where the
code is written and tested: please send issues and pull requests
[there](https://github.com/cleanrules/cleanrules/issues).

## Installing

```bash
go get github.com/cleanrules/cleanrules-go
```

```go
import cr "github.com/cleanrules/cleanrules-go"
```

Go 1.24 or later, and cgo with a C compiler (see *Platforms*). The module
needs no other module: only Go's standard library and the library of the
core, linked statically, so a program needs nothing at run time either.

## Example

```go
package main

import (
	"fmt"

	cr "github.com/cleanrules/cleanrules-go"
)

func main() {
	vip := cr.Must(cr.NewRule("vip",
		cr.Field("customer", "years").Gt(5).And(cr.Field("order", "total").Gt(1000)),
		cr.Then("discount", cr.MustDecimal("0.15")),
		cr.WithPriority(10)))
	loyal := cr.Must(cr.NewRule("loyal", cr.Field("customer", "years").Gt(2),
		cr.Then("discount", cr.MustDecimal("0.05")),
		cr.WithPriority(5)))
	rules := cr.Must(cr.NewRuleSet([]*cr.Rule{vip, loyal},
		cr.WithHitPolicy(cr.HitPolicyPriority)))

	decision := cr.Must(rules.Evaluate(map[string]any{
		"customer": map[string]any{"years": 7},
		"order":    map[string]any{"total": 900},
	}))
	fmt.Println(cr.Must(decision.Output())["discount"]) // 0.05, a cr.Decimal
	fmt.Println(decision.Fired())                      // [loyal]
	fmt.Println(cr.Must(decision.WhyNot("vip")))       // order.total > 1000 is false: short by 100
}
```

`cr.Must` panics on an error, for rules written in the program; check the
error where rules come from users. The
[guide](https://github.com/cleanrules/cleanrules/blob/main/bindings/go/README.md)
covers every part of the API: conditions and rules in code, Go data and the
values given back, actions, documents, decision tables, result trees, errors,
goroutines and memory. The examples
[`discounts.go`](https://github.com/cleanrules/cleanrules/blob/main/docs/examples/discounts.go)
and
[`risk_table.go`](https://github.com/cleanrules/cleanrules/blob/main/docs/examples/risk_table.go)
do more, and the
[specification](https://github.com/cleanrules/cleanrules/tree/main/spec)
says what every operation means.

## Platforms

The module links the prebuilt library of the platform it builds for, from
`lib/<os>_<arch>[_musl]/libcleanrules_ffi.a`, with the C ABI's header in
`include/`:

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
with `go build -tags cleanrules_musl`; without the tag, the glibc library
fails to link there. On Windows, cgo's C compiler decides: GCC's MinGW-w64 on
x64, and on arm64 llvm-mingw, since GCC has no Windows on arm64 target. On
any other platform, Android and iOS included, the build stops with
`undefined:
cleanrules_has_no_prebuilt_library_for_this_GOOS_and_GOARCH__see_README_md_Platforms`:
build the library there from the main repository and link it as *Linking
another build* says.

The libraries are release builds of the core (fat link-time optimization, one
codegen unit, debug information stripped), 5 to 12 MB each. `go mod vendor`
works: the directories of the header and of the libraries hold empty
packages that the module imports, so that they are vendored too.

### Linking another build

With the tag `cleanrules_external`, the module links no prebuilt library:
`CGO_LDFLAGS` gives the static library, built from the main repository
(`cargo build -p cleanrules-ffi --release`), and the system libraries that
Rust's standard library needs on the platform, as the
[guide](https://github.com/cleanrules/cleanrules/blob/main/bindings/go/README.md#linking-another-build)
lists them. On macOS:

```bash
CGO_LDFLAGS="/path/to/target/release/libcleanrules_ffi.a -lm" go build -tags cleanrules_external
```

### Thread stacks

Calls into the core run on the thread's stack, as every cgo call does, not
on the goroutine's. At the default limits a call needs at most 128 KiB of it
with a release build of the library, as the prebuilt ones are: well within
the default thread stacks of Linux (8 MiB) and macOS (512 KiB for threads
other than the main one). A debug build, linked with `cleanrules_external`,
needs up to 512 KiB, which is at the edge of macOS's.

## Versions

The module's version (its tag, `v` and the contents of `VERSION`) is the
version of cleanrules, which `cr.Version()` gives as the linked library
reports it; `cr.FormatVersion` is the version of the JSON format it reads and
writes, and `cr.ABIVersion` that of the C ABI. Changes are listed in the
[changelog](https://github.com/cleanrules/cleanrules/blob/main/CHANGELOG.md).

## Licence

Apache License 2.0 (`LICENSE`), created by Jose L. Salmeron, who designed
cleanrules' architecture: redistributions keep the `NOTICE` with its
attribution of authorship.
