# cleanrules for Erlang

The Erlang binding of cleanrules: the OTP application `cleanrules`, a NIF
written with [rustler](https://github.com/rusterlium/rustler) on the Rust core
(`crates/core`). Elixir and Gleam call it as it is, with their own terms: there
is no Elixir layer. The NIF reads Erlang terms into the engine's values, hands
them to the core and gives back what the core decides; the core reads, checks,
evaluates and explains everything (no rule logic in Erlang or in the NIF).

Status: task N.2 of the roadmap. This directory holds the NIF (crate
`cleanrules-erlang` in [`native/cleanrules_nif`](native/cleanrules_nif)) and
its internal module, `cleanrules_nif`, with their tests. The idiomatic module
`cleanrules` (`explain/1`, `summary/1`, actions, Dialyzer specifications) is
task N.3, and the conformance runner and the Hex package are task N.4: until
then nothing here is a public API, and nothing is published.

Erlang/OTP 26 or later (NIF version 2.17). Building from source needs Rust
1.95 or later and rebar3.

## Building and testing

```bash
cd bindings/erlang
rebar3 compile    # builds the NIF with cargo first (release profile)
rebar3 ct         # the Common Test suites
rebar3 xref
rebar3 dialyzer
```

`rebar3` runs [`scripts/nif.escript`](scripts/nif.escript) before it compiles
(`pre_hooks` in [`rebar.config`](rebar.config)). In a checkout of the
repository it builds the crate with cargo and copies the library to
`priv/cleanrules_nif.so` (`priv/cleanrules_nif.dll` on Windows), which
`cleanrules_nif` loads when it is loaded (`-on_load`). In a package, it
downloads the prebuilt library instead (*Prebuilt libraries* below).

| Variable | Effect |
|---|---|
| `CLEANRULES_NIF` | `build`, `download` or `skip` (use what `priv/` holds). Default: `build` when `native/cleanrules_nif` exists, else `download` |
| `CLEANRULES_NIF_PROFILE` | the cargo profile to build with: `release` (default) or `dev`, faster to build and used for coverage |
| `CLEANRULES_NIF_BASE_URL` | where the prebuilt archives are, `https://` or `file://`. Default: the GitHub release `v<vsn>` |
| `CLEANRULES_NIF_TARGET` | the Rust target triple to download. Default: this VM's |
| `CLEANRULES_NIF_CACHE` | the download cache. Default: the user cache (`filename:basedir(user_cache, "cleanrules")`) |

Coverage of the NIF's Rust code is measured through the Erlang suites, with
an instrumented build:

```bash
eval "$(cargo llvm-cov show-env --export-prefix)"
(cd bindings/erlang && CLEANRULES_NIF_PROFILE=dev rebar3 ct)
cargo test -p cleanrules-erlang    # unit tests of what needs no VM (tests/pure.rs)
cargo llvm-cov report -p cleanrules-erlang
```

## The NIF

`cleanrules_nif` is internal: the module `cleanrules` of N.3 builds on it.
Each function is a thin call into the core.

| Function | Scheduler | Returns |
|---|---|---|
| `version()`, `format_version()`, `default_limits()` | normal | the library's version (the application's), the JSON format's version (1), the default limits as a map |
| `ruleset_read(Text, Format, Limits)` | dirty CPU | `{ok, RuleSet}`: a rule set, rule or decision table document in `json`, `yaml`, `text` or `csv`, with limits (`#{}` for the defaults; atom keys `max_size`, `max_nesting`, `max_depth`, `max_table_size`, `max_yaml_size`, `max_text_size`, `max_values`) |
| `ruleset_write(RuleSet, Format)` | dirty CPU | `{ok, Text}` |
| `ruleset_info(RuleSet)` | dirty CPU | `#{hit_policy, aggregate, logic, strict, rules, field_paths}`: the policies as atoms, the rule names and the field paths read |
| `ruleset_summary(RuleSet, Format)` | dirty CPU | `{ok, Summary}`: as a `term` (the summary document's content, maps with binary keys), a `json` or `yaml` document, or its `text` |
| `evaluate(RuleSet, Data, MaxValues)` | normal, or dirty CPU | `{ok, Decision}` (*Schedulers*) |
| `evaluate_many(RuleSet, Inputs, MaxValues)` | dirty CPU | `{ok, [Decision]}`, or the error of the first input that fails, with its `row` |
| `explain(RuleSet, Data, MaxValues, Format)` | dirty CPU | `{ok, Explanation}`: as a `term`, a `json` or `yaml` document, or its `text` |
| `decision_write(RuleSet, Data, MaxValues, Format)` | dirty CPU | `{ok, Text}`: the input's decision as a `json` or `yaml` document (opaque values as `{"$opaque": Type}`) |
| `decision_read(RuleSet, Text, Format, Limits)` | dirty CPU | `{ok, Decision}`: a recorded decision read with its rule set, opaque values as `null` |
| `error_parent(Code)` | normal | the parent code in the hierarchy, from the core; `undefined` for a root and for `internal_error` |
| `value_to_json(Term, data \| constant, MaxValues)` | dirty CPU | `{ok, Json}`: a term read as data, or as a constant (*Values*) |
| `value_from_json(Json)` | dirty CPU | `{ok, Term}`: a value in the JSON format, given back as a term |
| `test_panic()` | normal | `{error, #{code => internal_error, ...}}`, for the tests |

A rule set is a resource: an opaque reference, immutable, which any number of
processes may hold and use at once (send it in a message, keep it in ETS or
`persistent_term`), freed when the last term that refers to it is collected.

A decision is a map:

```erlang
#{results := [#{rule := binary(), fired := boolean(),
                truth := true | false | unknown,
                output => #{binary() => term()}}],   % when the payload was evaluated
  selected := [binary()],
  output := {ok, #{binary() => term()} | null} | {error, Error},
  aggregated => #{binary() => term()} | null}       % when the rule set has an aggregate
```

`output` is the single output: the winner's for single-hit policies (`null`
when nothing was selected), the aggregate, or the `hit_policy_error` of a
multi-hit decision. An explanation as a `term` has the structure of the
explanation document (`spec/json-format.md`, *Explanations*), as maps with
binary keys and the values themselves (an opaque value is the term it was read
from), plus the `text` of each rule and cause and whether each cause `holds`;
N.3 turns it into its idiomatic form (atom keys, gaps as `{days, N}` and
`{microseconds, N}` among other things).

A decision is not a resource: Erlang terms are immutable, so the idiomatic
API can keep `{RuleSet, Data, MaxValues}` and evaluate again to explain or
write it (`explain/4`, `decision_write/4`), which costs a second evaluation;
evaluation is deterministic, so it is the same decision.

## Values

Terms are read, and values come back, as
[`spec/data-model.md`](../../spec/data-model.md) (*Conversions by language*,
*Erlang*) says. In short: binaries that are UTF-8 are text; `null`,
`undefined` and `nil` are null, `true` and `false` booleans, other atoms text;
integers beyond 64 bits are null with reason `int_overflow`; `calendar` tuples
are dates and naive datetimes; decimals, infinities and aware datetimes are
tuples tagged `cleanrules_decimal`, `cleanrules_float` and
`cleanrules_datetime`, and Elixir's `%Decimal{}`, `%Date{}`,
`%NaiveDateTime{}` and `%DateTime{}` are read too; maps are read by binary
key, else atom key, and their other keys are left out; Elixir structs are maps
without their `__` fields; anything else (tuples, pids, references, funs,
improper lists, binaries that are not UTF-8) is opaque, and comes back as the
same term. Null comes back as the atom `null`, and maps with binary keys.

Reading data only follows the paths the rule set reads, and it never creates
an atom: a segment is looked up among the atoms that exist. Everything read
counts against `MaxValues` (`spec/data-model.md`, *Reading native data*,
*Limits*; 1,000,000 in `default_limits()`): beyond it the input is refused
with `limit_exceeded`, `name` `max_values` and, in a batch, its `row`.

`value_to_json/3` converts a whole term: as data (as where a path ends:
opaque values and nulls with a reason allowed), or as a constant, where an
opaque value or an integer or decimal out of range is an `expression_error`
naming where it is (`spec/data-model.md`, *Constants*). Lists and maps deeper
than 256 levels are opaque at that place.

A `%Decimal{}` whose coefficient has more than 2,466 digits (1,024 bytes) is
read as null with reason `decimal_overflow` without looking for trailing zeros
that would let it fit; no decimal library makes such a coefficient.

## Schedulers

A NIF that runs for long on a normal scheduler holds up every process
scheduled there, so a call should stay near a millisecond:

- `evaluate/3` first calls `evaluate_normal/3`, which evaluates on the normal
  scheduler while reading the input stays within a budget of work (10,000
  units: one per value read, per map entry looked at and per 16 bytes of text,
  charged before the work is done: a list is walked only as far as the budget
  goes, a key is charged before it is copied, and a `%Decimal{}` with parts
  beyond 64 bits goes to a dirty scheduler) and the rule set is small (its
  JSON form up to 64 KiB); otherwise it returns `dirty` at once, and
  `evaluate/3` calls `evaluate_dirty/3`, the same on a dirty CPU scheduler.
  Either way, the call tells the scheduler how much of its time slice it used
  (`enif_consume_timeslice`).
- The budget is an estimate: a rule set whose call on the normal scheduler
  takes longer than a millisecond (many scans of long texts, say) is marked,
  and its later calls go to a dirty scheduler at once.
- Batches, explanations, documents and conversions always run on dirty CPU
  schedulers.

Measured in a release build on an Apple M-series CPU with OTP 29, 301 calls
each, a fresh rule set for each call (2026-11-15, load average about 4):

| Call on the normal scheduler | Minimum | Median | 90th percentile |
|---|---|---|---|
| a payload that copies a list of 1,000 integers | 0.09 ms | 0.10 ms | 0.10 ms |
| the same with 4,900 integers, the most the budget takes (read, evaluated and given back twice) | 0.36 ms | 0.37 ms | 0.40 ms |
| a rule set of 61 KiB (250 rules) with a small input | 0.10 ms | 0.11 ms | 0.13 ms |

On the same machine busy with other builds (load average about 130), the
4,900-integer call had a median of 1.9 ms and a 90th percentile of 29 ms:
the budget leaves room for slower machines, and the adaptive guard above
sends a rule set that still takes too long to dirty schedulers. The suite's
`normal_call_time` logs these times on each run. A rule set over 64 KiB goes
to a dirty scheduler at once.

Terms, lists, maps and field paths are read and built with explicit stacks,
and the values the NIF holds are dropped the same way
(`cleanrules::Value::drop_iteratively`): a dirty scheduler's stack is 320 KiB
by default, and neither a deep term nor a long field path may overflow it
(paths of 100,000 segments are tested).

The core's documents still nest on the stack in places (freeing a node tree
or nested data, for one), so the limits `max_depth` and `max_nesting` a caller
gives are refused with `badarg` above **2,048**: in a release build on
aarch64 macOS, reading, evaluating, explaining, writing and freeing a
document on a 320 KiB dirty scheduler failed beyond about 5,500 levels of
either, so 2,048 leaves room for other platforms. A debug build of the NIF
(`CLEANRULES_NIF_PROFILE=dev`) failed beyond about 1,700 levels: keep the
defaults (256 and 1,024) there.

## Errors

A fallible function returns `{error, Error}`, the core's error as a map with
the atom keys of its JSON form (`spec/errors.md`): `code` (an atom such as
`format_error` or `type_mismatch`), `message` and, when they apply, `hint`,
`pointer`, `line`, `column`, `rule`, `rules`, `path`, `types`, `row`, `name`,
`available`, `executed`, `node` (its JSON form as a term), `operation`,
`reason` and `cause`. `error_parent(Code)` gives a code's parent in the
hierarchy, from the core. A panic inside the library is caught and comes back the same way, with
the code `internal_error`: a bug, to report. An argument of the wrong kind (a
format that is not one of the atoms, a negative `MaxValues`, something that is
not a rule set) raises `badarg`, as Erlang's own functions do.

## Prebuilt libraries

A package must not need Rust, so the library of each platform is built by CI
and downloaded when the package is compiled, the way `rustler_precompiled`
does for Elixir, in Erlang and rebar3:

| Platform | Rust target |
|---|---|
| macOS x64, arm64 (11.0 or later) | `x86_64-apple-darwin`, `aarch64-apple-darwin` |
| Linux x64, arm64, glibc 2.28 or later | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` |
| Linux x64, arm64, musl (Alpine) | `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` |
| Windows x64, arm64 (Erlang/OTP's MSVC builds) | `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` |

- Each library is an archive `libcleanrules_nif-v<vsn>-nif-2.17-<target>.<so|dll>.tar.gz`
  holding `cleanrules_nif.so` (`.dll`), built by
  [`scripts/package-nif.sh`](scripts/package-nif.sh). It is meant for the
  GitHub release `v<vsn>`.
- `nif-checksums.config`, written by
  [`scripts/checksums.py`](scripts/checksums.py), lists the SHA-256 of every
  archive as Erlang terms, and goes in the package.
- In a package, `scripts/nif.escript` works out the VM's target (from
  `erlang:system_info(system_architecture)`, musl included), checks that the VM
  loads NIF 2.17, takes the archive from its cache or downloads it (`httpc`,
  verifying the server's certificate against the system's CA certificates;
  `HTTPS_PROXY` as `host:port` and `NO_PROXY` are honoured, a proxy with
  credentials is not: use a `file://` mirror), refuses it unless its SHA-256 is the one in
  `nif-checksums.config`, and unpacks only the library into `priv/`. A
  platform without a prebuilt library stops with a message.

The workflow [`erlang-nifs.yml`](../../.github/workflows/erlang-nifs.yml)
builds the eight archives, writes `nif-checksums.config`, and uses them as a
package does on macOS, Windows x64 and Linux (Debian and Alpine images, x64 and
arm64, OTP 26 and 29): the application without its Rust sources downloads its
platform's archive from the artifact, checks it, loads it and runs the suites,
and a changed archive is refused ([`scripts/smoke.sh`](scripts/smoke.sh)).
Windows on arm64 is built but not run: there is no Erlang/OTP set up for its
runners yet. Nothing is published: attaching the archives to the release and
the Hex package are task N.4.

## Licence

Apache-2.0, like the rest of cleanrules: see [LICENSE](LICENSE) and
[NOTICE](NOTICE).
