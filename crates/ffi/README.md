# cleanrules-ffi

The C ABI of [cleanrules](https://cleanrules.github.io): one header,
[`include/cleanrules.h`](include/cleanrules.h), over the Rust core. C and C++
use it directly; the Go and JVM bindings are built on it.

```sh
cargo build --release -p cleanrules-ffi
# target/release/libcleanrules_ffi.{a,so,dylib}, and include/cleanrules.h
```

## Example

```c
cr_error *error = NULL;
cr_ruleset *rules = NULL;
if (cr_ruleset_read(text, len, CR_FORMAT_TEXT, NULL, &rules, &error) != CR_OK) {
    fprintf(stderr, "%s\n", cr_error_message(error));
    cr_error_free(error);
    return 1;
}
const char *input = "{\"customer\": {\"years\": 7}, \"order\": {\"total\": 1500}}";
cr_decision *decision = NULL;
char *output = NULL;
if (cr_ruleset_evaluate(rules, input, strlen(input), NULL, &decision, &error) == CR_OK &&
    cr_decision_output(decision, &output, NULL, &error) == CR_OK) {
    printf("%s\n", output);  /* {"discount":{"$decimal":"0.15"}} */
} else {
    fprintf(stderr, "%s\n", cr_error_message(error));
    cr_error_free(error);
}
cr_string_free(output);      /* each accepts NULL */
cr_decision_free(decision);
cr_ruleset_free(rules);
```

The whole program, with explanations and errors, is
[`docs/examples/discounts.c`](../../docs/examples/discounts.c).

## Conventions

- **Names.** Functions and types start with `cr_`, constants with `CR_`.
- **Handles.** `cr_document`, `cr_ruleset`, `cr_decision`, `cr_node`,
  `cr_batch`, `cr_value`, `cr_grid`, `cr_actions`, `cr_limits` and `cr_error`
  are opaque. Whoever receives one owns it and frees it with its `cr_*_free`
  function, which accepts `NULL`. Documents, rule sets, decisions, nodes,
  batches and errors never change once made, so one may be used from several
  threads at once, but not while it is freed; limits, values, grids and
  registries change while they are built, so finish them before sharing them.
  A finished registry may be used by several threads at once, and its actions
  then run concurrently with the same data. A decision keeps its rule set
  alive: either may be freed first.
- **Texts in.** A pointer and a length in bytes, UTF-8, with no final NUL
  needed; `NULL` with a length of 0 is the empty text. The library never keeps
  a pointer after the call. Bytes that are not UTF-8 are a `CR_FORMAT_ERROR` at
  the first invalid byte, in every format.
- **Texts out.** Through a `char **out` parameter, NUL-terminated, with their
  length through `size_t *len` when it is not `NULL`, and freed with
  `cr_string_free`. The strings of `cr_error_message`, `cr_error_hint` and
  `cr_error_json` belong to the error, and those of `cr_version` and
  `cr_status_name` are static: never free them.
- **Errors.** A fallible function returns a `cr_status`: `CR_OK`, the number of
  an error code (`CR_FORMAT_ERROR`, `CR_MISSING_FIELD`, ...; the table of
  [`spec/errors.md`](../../spec/errors.md#numeric-codes)), `CR_INVALID_ARGUMENT`
  for a broken call (a `NULL` handle, an unknown format), whose message names
  the function, or `CR_INTERNAL_ERROR` for a bug. Its last argument,
  `cr_error **error`, may be `NULL`; otherwise `*error` must be `NULL` on entry,
  receives a `cr_error` on failure and is left alone on success. The error
  holds the message, the hint and the error's JSON form (`cr_error_json`, with
  a [schema](../../spec/schemas/v1/error.schema.json)) with its context.
  `cr_error_is(status, CR_EVALUATION_ERROR)` follows the hierarchy of errors.
  Out-parameters are `NULL` after a failure.
- **Limits.** `CR_LIMIT_MAX_SIZE` sets the size limit of every format at once
  and reads JSON's; `CR_LIMIT_MAX_YAML_SIZE` and `CR_LIMIT_MAX_TEXT_SIZE` (the
  rule syntax and CSV) set one each.
- **Rule sets.** `cr_ruleset_read` accepts a rule set, a decision table, which
  it compiles, or one rule, which becomes a rule set of one with the default
  settings (`spec/json-format.md`, *Documents*).
- **Values.** `cr_value` holds data natively, without JSON: nulls with a reason
  (a `read_error` keeps its message), booleans, ints, floats, decimals (from
  their text), strings, dates, naive and aware datetimes, lists, maps, records
  and opaque values (a type name and a token your program chooses; equal
  tokens mean the same object, and an output gives the token back). Build
  children first and move them into their list or map (`cr_value_list_set`,
  `cr_value_map_insert`), which then owns them, even when the call fails.
  Unset positions of a list stay null, as the unread positions of native data
  do. A date outside years 1 to 9999 or an offset beyond 18 hours is
  `CR_INVALID_ARGUMENT`: build an opaque value instead if you need one.
  Values nest at most 1024 lists and maps deep, as JSON does by default.
  `cr_ruleset_field_paths` lists the paths a rule set reads, so a binding
  converts only those (`spec/data-model.md`, *Reading native data*). Readers
  (`cr_value_kind`, `cr_value_get_*`, `cr_value_len`, `cr_value_list_get`,
  `cr_value_map_entry`, `cr_value_map_get`) borrow from the value, until it
  changes. `cr_value_to_json` and `cr_value_from_json` convert to and from
  JSON, though not losslessly: null reasons are written as plain nulls, and
  opaque values cannot be read back.
- **Describing a rule set.** `cr_ruleset_len`, `cr_ruleset_rule_name`,
  `cr_ruleset_hit_policy`, `cr_ruleset_aggregate` and `cr_ruleset_strict`
  describe one; its summary has the rest.
- **Decisions.** Which rules fired (`cr_decision_fired`, `cr_decision_truth`),
  which were selected (`cr_decision_selected`) and the outputs
  (`cr_decision_output_value`, `cr_decision_outputs_value`) are kept when the
  decision is made, so reading them is free. Writing and explaining a decision
  evaluate its input again, or check its recorded decision again.
- **Actions.** A rule may name its action (`DO notify` in text, `"do"` in
  JSON). Register C functions under those names (`cr_actions_new`,
  `cr_actions_add`, with a `void *` passed back to them) and call
  `cr_ruleset_run`: it checks that every named action is registered before it
  evaluates (`cr_ruleset_check_actions` makes the same check, Python's
  `bind`), then calls the actions of the selected rules in rule order. An
  action receives its data and a `cr_action_call`, which gives the decision,
  the rule's position and the input as the decision keeps it, valid during the
  call. It returns 0; any other number stops the run with `CR_ACTION_ERROR`,
  after `cr_action_call_fail` says why, which becomes the error's `cause`. The
  error lists the rules whose actions had run, and `cr_error_decision` gives
  the decision: the same handle the actions saw. An action may read the
  decision and use the rule set, even run it again, but must not change or
  free the registry, the decision or the input, and must not unwind: a C++
  action catches its exceptions in a `noexcept` function, a JVM upcall catches
  everything, and Go passes its data as a `cgo.Handle`.
  `cr_ruleset_evaluate*` never run actions.
- **Grids.** `cr_grid_new` starts a decision table from its settings and a
  header of strings, `cr_grid_push_row` adds rows of cells (a string is text,
  as in CSV; any other value a constant; null an empty cell), and
  `cr_grid_finish` gives the table's document (`spec/tables.md`, *Grids*).
  After a row fails, the grid can only be freed.
- **Single nodes.** `cr_node_read` reads one condition or value (the node of
  format version 1, not a document), in JSON, YAML or the rule syntax;
  `cr_node_evaluate_tree` (with a logic and strictness) and
  `cr_node_evaluate_value_tree` (with strictness) evaluate it on an input and
  give its result tree as JSON, as `evaluate_tree` and `evaluate_value_tree`
  do in every language (`spec/ast.md`, *Evaluating one node*). Python calls a
  node `Expr`; C, Go and the JVM call it a node.
- **Batches.** `cr_ruleset_evaluate_many` evaluates a list of inputs; the first
  that fails gives the error, with its `row`. `cr_batch_get` borrows each
  decision.
- **Data.** Documents, inputs, outputs and explanations are JSON
  ([`spec/json-format.md`](../../spec/json-format.md)); documents may also be
  YAML, the rule syntax or, for decision tables, CSV, and explanations and
  summaries text (`CR_FORMAT_*`).
- **Versions.** `CR_ABI_VERSION` is the ABI the header describes and
  `cr_abi_version()` the one the library implements; a program checks they are
  equal. New functions and constants keep the version; any other change raises
  it. `cr_version()` is the library's version and `cr_format_version()` the JSON
  format's.

## The header

The header is generated by [cbindgen](https://github.com/mozilla/cbindgen) and
committed; CI fails when it is out of date:

```sh
cd crates/ffi && cbindgen --config cbindgen.toml --output include/cleanrules.h
```

## Safety

Every `unsafe` block says why it is sound, every function says what its caller
promises, and CI runs the ABI's tests under Miri and the example under the
address and undefined behaviour sanitizers. Panics never cross the boundary:
they become `CR_INTERNAL_ERROR`.

## Linking

The static library needs the system libraries that Rust's standard library
uses, which `cargo rustc -p cleanrules-ffi --lib --crate-type staticlib --
--print native-static-libs` lists: `-lm -lpthread -ldl` on Linux, `-lm` on
macOS, and import libraries such as `kernel32.lib` and `ntdll.lib` on Windows,
where the program uses the same C runtime (`cl /MD`) and passes them after
`/link`. CI builds and runs the example on Linux, macOS and Windows (MSVC).

## License

Apache License 2.0, created by Jose L. Salmeron: redistributions keep the
[NOTICE](https://github.com/cleanrules/cleanrules/blob/main/NOTICE) with its
attribution of authorship.
