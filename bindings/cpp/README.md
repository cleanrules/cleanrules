# cleanrules for C++

`cleanrules.hpp` is a header-only C++17 layer over the C ABI
([`crates/ffi`](../../crates/ffi/README.md)): the same engine and the same
concepts (`ruleset`, `decision`, `explain`, `summary`), with handles that free
themselves, values you build from C++ data, and errors as exceptions or as
values.

## Example

```cpp
#include <cleanrules.hpp>
#include <iostream>

namespace cr = cleanrules;

int main() {
    auto rules = cr::ruleset::read(R"(
        HIT POLICY priority
        RULE vip PRIORITY 10
        IF customer.years > 5 AND order.total > 1000
        THEN discount = 0.15
        RULE loyal PRIORITY 5
        IF customer.years > 2
        THEN discount = 0.05)", cr::format::text);

    auto decision = rules.evaluate(cr::value::map({
        {"customer", cr::value::map({{"years", 7}})},
        {"order", cr::value::map({{"total", 1500}})},
    }));
    std::cout << decision.output()["discount"].as_decimal() << "\n";  // 0.15
    std::cout << decision.explain();
}
```

[`docs/examples/discounts.cpp`](../../docs/examples/discounts.cpp) does more:
JSON input, which rules fired, an action, and an error both ways.

## Building

The CMake project builds the C library with cargo, so it needs a Rust
toolchain (the version `rust-version` names in the workspace's `Cargo.toml`, or
later) and CMake 3.21:

```sh
cmake -S bindings/cpp -B build/cpp            # Debug; -DCMAKE_BUILD_TYPE=Release
ctest --test-dir build/cpp                    # builds, then runs the tests
cmake --install build/cpp --prefix /opt/cleanrules
```

Then, in your project:

```cmake
find_package(cleanrules CONFIG REQUIRED)   # with CMAKE_PREFIX_PATH=/opt/cleanrules
target_link_libraries(app PRIVATE cleanrules::cleanrules)   # C++; cleanrules::c for C
```

Or add the directory to your build (`add_subdirectory(path/to/bindings/cpp)`),
which gives the same two targets.

Options:

| Option | Default | |
| --- | --- | --- |
| `CLEANRULES_SHARED` | `OFF` | link the shared library instead of the static one |
| `CLEANRULES_BUILD_TESTS` | on at the top level | the tests and the example |
| `CLEANRULES_INSTALL` | on at the top level | the install rules and the CMake package |
| `CLEANRULES_RUST_TARGET` | the host's, or derived | the Rust target triple, such as `aarch64-linux-android` |
| `CLEANRULES_MSVC_STATIC_CRT` | from `CMAKE_MSVC_RUNTIME_LIBRARY` | build for MSVC's static C runtime (`/MT`) |
| `CLEANRULES_SANITIZERS` | empty | sanitizers for the tests, such as `address,undefined` |
| `CLEANRULES_CARGO_TARGET_DIR` | `<build>/cargo` | where cargo builds; the workspace's `target` reuses its builds |
| `CLEANRULES_NATIVE_LIBS` | asked of rustc | the system libraries of the static library |
| `CARGO` | found on `PATH` | the cargo to use (also from the environment) |

Debug builds use cargo's `dev` profile, the others `release`. When CMake
builds for another platform (cross-compiling, one `CMAKE_OSX_ARCHITECTURES`
that is not the host's, an iOS device or simulator, or MinGW), cargo builds
for the matching Rust target; set `CLEANRULES_RUST_TARGET` when it cannot be
told. Two things must be ready for that target: its Rust standard library
(`rustup target add <triple>`), and, except on Apple platforms and with MSVC,
a linker: CMake hands cargo its C compiler for the target
(`CARGO_TARGET_<TRIPLE>_LINKER`), so a toolchain file whose C compiler links
for the target is enough. One architecture at a time: join macOS ones with
`lipo`. `CMAKE_OSX_DEPLOYMENT_TARGET` reaches cargo too.

With MSVC, the Rust objects use the release C runtime: the DLL one (`/MD`) by
default, or the static one (`/MT`) when `CMAKE_MSVC_RUNTIME_LIBRARY` asks for
it (`MultiThreaded`), in which case cargo builds with `crt-static` in its own
target directory. Build your program with the same runtime. A Debug program
(`/MDd` or `/MTd`) links too, with a harmless `LNK4098` warning: nothing from
the C runtime crosses the C ABI. A shared library on Windows is a DLL that
must sit next to your program or on `PATH`; on macOS its install name is
`@rpath/libcleanrules_ffi.dylib`, and on Linux its SONAME
`libcleanrules_ffi.so`, so it works wherever it is installed.

## Tests

`ctest` builds the project first, then runs the API tests, the header without
exceptions, the example, the installed package, and `conformance`: every
vector of [`conformance/`](../../conformance/README.md), through this header.
Each vector runs as `json:<id>`, with JSON documents and inputs and the
overloads that throw (`read` vectors also with `error_info`), and, except the
`read` vectors, as `native:<id>`, with input built by the value constructors,
the `error_info` overloads, and decisions read back with the decision
readers. The runner reports every vector that fails, with what it got and
what the vector expects:

```sh
ctest --test-dir build/cpp -R conformance --output-on-failure
build/cpp/cleanrules_conformance path/to/vectors   # another directory of vectors
```

Under the sanitizers (`-DCLEANRULES_SANITIZERS=address,undefined`, in a Debug
build), any report of AddressSanitizer or UndefinedBehaviorSanitizer fails the
test, and so does a leak on Linux (macOS has no LeakSanitizer):

```sh
cmake -S bindings/cpp -B build/cpp-asan -DCMAKE_BUILD_TYPE=Debug \
  -DCLEANRULES_SANITIZERS=address,undefined
ctest --test-dir build/cpp-asan --output-on-failure
```

The tests need a checkout of the repository: packages (Conan, vcpkg) build
without them.

## Conan

A Conan 2 recipe lives in [`conan/`](conan/conanfile.py). It builds the same
CMake project, so the build machine needs a Rust toolchain
([rustup](https://rustup.rs)) as well as Conan:

```sh
conan create bindings/cpp/conan --build=missing                    # static
conan create bindings/cpp/conan --build=missing -o "&:shared=True" # shared
```

`conan create` also builds and runs [`conan/test_package`](conan/test_package).
In a consumer, with the `CMakeDeps` and `CMakeToolchain` generators:

```text
[requires]
cleanrules/<version>
```

```cmake
find_package(cleanrules CONFIG REQUIRED)
target_link_libraries(app PRIVATE cleanrules::cleanrules)   # or cleanrules::c
```

The package targets are the same as CMake's: `cleanrules::cleanrules` asks for
C++17 whatever the profile's `compiler.cppstd`, and the package does not depend
on it. Debug builds use cargo's `dev` profile, and `compiler.runtime=static`
builds for MSVC's static C runtime. When Conan cross-builds, the recipe passes
the matching Rust target, which needs what any other cross build needs (above).
iOS and Android are not packaged yet: build them with CMake.

## vcpkg

An overlay port lives in [`vcpkg/ports/cleanrules`](vcpkg/ports/cleanrules).
It builds from this checkout, so it also needs cargo. Static or shared follows
the triplet:

```sh
vcpkg install cleanrules --overlay-ports=bindings/cpp/vcpkg/ports
vcpkg install cleanrules:arm64-osx-dynamic --overlay-ports=bindings/cpp/vcpkg/ports
```

In manifest mode, list `cleanrules` in your `vcpkg.json` and pass the overlay
to CMake, as [`vcpkg/test`](vcpkg/test) does:

```sh
cmake -S bindings/cpp/vcpkg/test -B build/vcpkg-test \
  -DCMAKE_TOOLCHAIN_FILE="$VCPKG_ROOT/scripts/buildsystems/vcpkg.cmake" \
  -DVCPKG_OVERLAY_PORTS="$PWD/bindings/cpp/vcpkg/ports"
```

The triplet also chooses the Rust target (x64, x86 and arm64 Windows; x64 and
arm64 Linux and macOS) and, on Windows, the C runtime: a `static` CRT linkage
builds with `crt-static`. Two things to know:

- vcpkg's binary cache sees only the port's own files, not this checkout nor
  rustc's version. After changing either, drop the installed copy, with
  `vcpkg remove cleanrules` (classic mode) or by deleting
  `<build>/vcpkg_installed` (manifest mode), and install again with
  `--binarysource=clear` (in manifest mode,
  `-DVCPKG_INSTALL_OPTIONS=--binarysource=clear`).
- On Windows, vcpkg builds with a reduced environment. CMake looks for cargo
  in `CARGO_HOME`, `~/.cargo/bin` and `%USERPROFILE%\.cargo\bin`. If rustup
  lives elsewhere, tell vcpkg to keep its variables: in a POSIX shell,
  `export VCPKG_KEEP_ENV_VARS="CARGO_HOME;RUSTUP_HOME"`; in `cmd`,
  `set VCPKG_KEEP_ENV_VARS=CARGO_HOME;RUSTUP_HOME`. Add `CARGO` to the list to
  use a cargo of your choice.

The port's package config names the release library, also for Debug builds:
the C ABI is the same in both. The ports in the Conan and vcpkg registries
(ConanCenter, a `vcpkg_from_github` port on a release tag) come with the first
release.

## Errors

Each call that can fail has two overloads, as in `std::filesystem`:

```cpp
try {
    auto decision = rules.evaluate(input);           // throws on failure
} catch (const cr::missing_field_error &e) {         // or evaluation_error, or error
    std::cerr << e.what() << "\n" << e.json() << "\n";
}

cr::error_info error;
auto decision = rules.evaluate(input, error);        // never throws a cleanrules::error
if (error) {
    std::cerr << cr::name(error.code()) << ": " << error.message() << "\n";
}
```

The exceptions follow the hierarchy of [`spec/errors.md`](../../spec/errors.md):
`cleanrules::error` (a `std::runtime_error`) above `format_error`
(also for `unsupported_version`, `limit_exceeded` and `decision_mismatch`),
`expression_error`, `rule_definition_error`, `rule_syntax_error`,
`registry_error`, `rule_not_found_error`, `evaluation_error` (above
`missing_field_error`, `type_mismatch_error` and `hit_policy_error`),
`action_error`, `unsupported_error`, and, for a misuse of the API such as a
moved-from handle, `invalid_argument_error`; `internal_error` is a bug in
cleanrules. Each gives its `code()`, `hint()`, `json()` (spec/errors.md,
*JSON form*) and `info()`.

An `error_info` is false when nothing failed; a call clears it on success. A
failed call returns an empty handle (false), an empty text or a zero.

Built without exceptions (`-fno-exceptions`, `/EHs-c-`, or with
`CLEANRULES_NO_EXCEPTIONS` defined), only the `error_info` overloads exist,
`value::map({...}, error)` and `value::record({...}, error)` among them. The
value constructors then turn what they cannot build (a string that is not
UTF-8, an impossible date) into a null with the reason `read_error` and the
message, as native data that cannot be read; `value::make(..., error)` reports
it instead. With exceptions, the constructors throw a `format_error` (a string)
or an `invalid_argument_error` (a date, a datetime, an offset), and
`value::map` and `value::record` a `format_error` for a key that is not UTF-8.
Both kinds of translation unit may share a program: the header puts each in
its own inline namespace.

## Values

```cpp
auto input = cr::value::record({
    {"name", "Ada"},                                  // string (UTF-8)
    {"years", 7},                                     // int
    {"score", 0.5},                                   // float
    {"price", cr::value::decimal("19.90")},           // exact decimal
    {"born", cr::date{1815, 12, 10}},
    {"seen", cr::aware_datetime{1762156800, 0, 3600, "Europe/Madrid"}},
    {"handle", cr::opaque{"socket", 42}},              // a token of your own
    {"tags", cr::value::list({"vip", "beta"})},
    {"note", nullptr},                                // a plain null
});
input["years"].as_int();            // 7; a value_ref borrowed from input
input["tags"][0].as_string();       // "vip", a string_view
input["missing"];                   // an empty value_ref: false
```

Values copy deeply (null reasons and opaque tokens included) and move
cheaply. `value::list({...})` holds its items, `value::list_of_size(n)` has
`n` nulls to `set`, `value::map().insert("a", 1).insert("b", 2)` builds in
place, and `value::from_json` and `to_json` convert to and from JSON. Strings
are UTF-8; other pointers and character types (`char32_t`, `unsigned char`,
...) are not values.

A `value_ref` (and a `std::string_view` read from one) is valid until the
value it comes from changes, moves or is destroyed; `clone()` makes it a
`value`. Read from a temporary, they are owned instead, so
`auto label = decision.output()["label"].as_string();` is a `std::string`.

`limits` carries `limit::max_values` (1,000,000 by default) with the other
limits, for the bindings that read native data, count what they convert and
refuse an input beyond it with `limit_exceeded` (`spec/data-model.md`,
*Reading native data*, *Limits*). A value you build here is not counted,
since you build every part of it.

## Decisions

`decision.fired(i)`, `truth(i)`, `selected()`, `rule_name(i)`,
`rule_output(i)`, `output()` and `outputs()` (or `output_json()`), `write()`
(JSON by default, as every `write`) and `explain()` (text by default, as
`summary()`). A decision keeps its rule set alive and a copy of its input.
`ruleset::evaluate_many` returns a `batch`, whose `decision_ref`s live as long
as it, and `clone()` keeps one longer; `read_decision` reads a recorded one
back. A `node` evaluates on its own: `evaluate_tree(input, logic, strict)`
and `evaluate_value_tree(input, strict)`. `decision.explanation()` gives the
explanation as a `value`: the explanation document's structure with the
values themselves, and the `text` of each rule and cause and whether each
cause `holds` (`cr_decision_explain_value`).

## Building in code

The C ABI's constructors, checked by the core (an `expression_error` or a
`rule_definition_error` for what breaks a construction rule):

```cpp
auto years = cr::node::field(cr::value::list({"customer", "years"}));
auto total = cr::node::field(cr::value::list({"order", "total"}));
auto old = cr::node::compare("gt", years, cr::node::constant(5));
auto vip = cr::rule::make("vip", cr::node::and_({old, cr::node::compare("gt", total, cr::node::constant(1000))}))
               .with_payload("discount", cr::node::constant(cr::value::decimal("0.15")))
               .with_priority(10)
               .with_action("notify_sales");
auto rules = cr::ruleset::make({vip}, cr::ruleset_settings{"priority", {}, {}, false});
auto position = rules.rule_position("vip");  // rule_not_found_error otherwise
```

`node::` `field`, `constant`, `compare` and `arith` (by tag), `and_` and
`or_` (named nodes and temporaries), `not_`, `is_null`, `abs`, `neg`, `is_in`, `between`,
`matches` and `string_test`; a node's `tag()`, `kind()`, `children()`,
`detail()`, `same_as()` and `hash()` (the same on every platform for one
version of the library), with `std::hash` and `std::equal_to` for unordered
containers. `rule::make(name, when)` and its
`with_*` steps, each a new rule, with `tags()` and `metadata()`.
`ruleset::make(rules, settings)` takes `ruleset_settings` (the same as a
decision table's `table_settings`), and `ruleset::summary(actions, format)` marks the rules
whose action is code.

Rules read back: `ruleset::rule_at(i)` and `document::as_rule()` give a
`rule` (a copy), and `rule::from_text(text)` reads one rule in the rule
syntax (`rule_1` without a `RULE` line). A rule gives its `name()` (borrowed,
like `ruleset::rule_name`), `description()` and `action_name()` (copied,
`std::nullopt` when it has none), `priority()`, `when()` and `payload()` (each
key with its value node, in order). `document::from_condition(node)` and `document::from_rule(rule)`
make documents to `write`, and `document::as_condition()` reads a condition
document's node.

## Actions

```cpp
cr::actions actions;
actions.add("notify_sales", [](cr::action_call &call) {
    send_mail(call.rule_name(), call.input()["customer"]["email"].as_string());
    // call.fail("why") or a throw stops the run with an action_error
});
auto decision = rules.run(actions, input);
```

The registry keeps the callables. An action that throws stops the run with an
`action_error` thrown over it (`std::rethrow_if_nested` gives the original),
whose `decision()` is a copy of the decision being run; with an `error_info`,
`cause()` holds the exception, and `decision()` a copy that outlives the
error_info's next use. Actions run on the thread that calls `run`, and may
run the rules again. On Windows, each DLL that includes the header keeps the
exception apart: one thrown by an action registered in one DLL, during a run
started in another, fails the run without being nested in it.

## Threads

Rule sets, documents, decisions, nodes and batches never change once made:
use them from as many threads as you like. Values, limits, grids and action
registries change while they are built; finish them first. A finished
registry may run on several threads at once, and its actions then run
concurrently.

## Ownership

Every handle frees its C handle; `c_ptr()` lends it to C code,
`value::adopt` and `decision::adopt` take ownership of a C handle, and the
member `release()` of a value gives its C handle up.
The header includes `cleanrules.h`, so the whole C ABI stays available.

## License

Apache-2.0, with the [NOTICE](../../NOTICE) that every redistribution keeps.
