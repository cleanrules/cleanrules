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
  `cr_rule`, `cr_tree`, `cr_batch`, `cr_value`, `cr_grid`, `cr_actions`, `cr_limits`
  and `cr_error` are opaque. Whoever receives one owns it and frees it with its
  `cr_*_free` function, which accepts `NULL`. Documents, rule sets, decisions,
  nodes, rules, result trees, batches and errors never change once made, so one may be used from several
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
  the argument (never the function, so bindings show messages as they are),
  or `CR_INTERNAL_ERROR` for a bug. Its last argument,
  `cr_error **error`, may be `NULL`; otherwise `*error` must be `NULL` on entry,
  receives a `cr_error` on failure and is left alone on success. The error
  holds the message, the hint and the error's JSON form (`cr_error_json`, with
  a [schema](../../spec/schemas/v1/error.schema.json)) with its context.
  `cr_error_is(status, CR_EVALUATION_ERROR)` follows the hierarchy of errors.
  Out-parameters are `NULL` after a failure.
- **Limits.** `CR_LIMIT_MAX_SIZE` sets the size limit of every format at once
  and reads JSON's; `CR_LIMIT_MAX_YAML_SIZE` and `CR_LIMIT_MAX_TEXT_SIZE` (the
  rule syntax and CSV) set one each. `CR_LIMIT_MAX_VALUES` (1,000,000 by
  default) is for bindings that read native data on this ABI (Java, Go): they
  count the values they read for one input and, beyond the limit, stop and
  refuse the input with the core's error, `cr_error_new_max_values(limit, row,
  rule, len, ...)` (`CR_LIMIT_EXCEEDED`, `name` `max_values`; `spec/data-model.md`,
  *Reading native data*, *Limits*). The ABI does not count the values a
  program builds itself. `CR_LIMIT_MAX_DIAGRAM_SIZE` (20,000 by default)
  bounds the elements of a diagram.
- **Diagrams.** `cr_node_draw`, `cr_rule_draw`, `cr_ruleset_draw`,
  `cr_decision_draw` (its explanation) and `cr_document_draw` (any document;
  a decision table's HTML grid, with an optional decision) write a diagram in
  `CR_DIAGRAM_DOT`, `CR_DIAGRAM_MERMAID`, `CR_DIAGRAM_HTML` or
  `CR_DIAGRAM_HTML_FRAGMENT` (`spec/visual.md`): a new text, freed with
  `cr_string_free`.
- **Stack.** At the default limits every call needs at most 128 KiB of stack
  in a release build of the library (512 KiB in a debug build), less than a
  JVM thread has (1 MiB on Linux and Windows). Freeing values or
  conditions nested far deeper than the default `CR_LIMIT_MAX_NESTING` or
  `CR_LIMIT_MAX_DEPTH` takes more, about 64 bytes a level of data in a release
  build: give such threads a larger stack
  ([`spec/json-format.md`](../../spec/json-format.md#errors-and-limits)).
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
  `cr_value_clone` copies a value without losing either.
- **Building in code.** Conditions, rules and rule sets are built through the
  core's constructors, which apply every construction rule (`spec/ast.md`,
  `spec/rules.md`) with the core's errors, as Python's and JavaScript's
  builders do. Nodes: `cr_node_new_field` (a list of strings and ints, never split
  at dots), `cr_node_new_constant` (a scalar `cr_value`; an overflowing or
  unreadable null, an opaque value, a list or a map is a
  `CR_EXPRESSION_ERROR`), `cr_node_new_compare` and `cr_node_new_arith` (by tag:
  `eq`..., `add`...), `cr_node_new_and` and `cr_node_new_or` (an array of nodes),
  `cr_node_new_not`, `cr_node_new_is_in` (a list of constants), `cr_node_new_between`
  (`closed` by name, `NULL` for `both`), `cr_node_new_is_null`, `cr_node_new_matches`,
  `cr_node_new_string_test`, `cr_node_new_abs` and `cr_node_new_neg`. Every argument is
  copied: the caller keeps and frees its own nodes and values, and each call
  gives a new node. An unknown tag or name is `CR_INVALID_ARGUMENT`. A binding
  rejects a NaN or a datetime finer than a microsecond before it builds the
  value, which cannot hold them. What a node says about itself:
  `cr_node_tag`, `cr_node_kind` (static strings), `cr_node_children_len` and
  `cr_node_child` (a copy), `cr_node_detail` (a field's path, a constant's
  value, an `in`'s values, `between`'s `closed`, a pattern or a text),
  `cr_node_same` (structural identity) and `cr_node_hash` (consistent with
  it: 64-bit FNV-1a over a fixed-width little-endian encoding, the same on
  every platform for one version of the library, never to be stored),
  besides `cr_node_write`. Rules: `cr_rule_new` (a name and a condition),
  then each `cr_rule_with_payload`, `cr_rule_with_priority`,
  `cr_rule_with_tags`, `cr_rule_with_description`, `cr_rule_with_metadata`
  (a map of data) or `cr_rule_with_action` (a name) gives a new rule;
  `cr_rule_tags` and `cr_rule_metadata` give what the core keeps.
  `cr_ruleset_new` makes a rule set of an array of rules (copied) with a hit
  policy, an aggregate and a logic by name (`NULL` for the defaults) and
  strictness, read by the core as JSON's are.
  `cr_rule_from_text` reads one rule written in the rule syntax (fuzzytool's
  `rule_from_text`: without a `RULE` line it is `rule_1`).
- **What a rule holds.** For rules read from documents and rule sets, so a
  binding can make its own objects of them while it keeps the core's rule
  (which writes back exactly): `cr_ruleset_rule` (a copy of the rule at a
  position), `cr_rule_name`, `cr_rule_description` and `cr_rule_action`
  (borrowed texts, `NULL` when the rule has none), `cr_rule_priority`,
  `cr_rule_when` (a copy of the condition), `cr_rule_payload_len`,
  `cr_rule_payload_key` (borrowed) and `cr_rule_payload_value` (a copy of the
  value node), in the payload's order.
- **Documents of a condition or a rule.** `cr_document_new_condition` (a
  value node is a `CR_EXPRESSION_ERROR`) and `cr_document_new_rule` make a
  document to write with `cr_document_write`; `cr_document_condition` and
  `cr_document_rule` give a copy of what a document of that kind holds (another
  kind is `CR_INVALID_ARGUMENT`).
- **Describing a rule set.** `cr_ruleset_len`, `cr_ruleset_rule_name`,
  `cr_ruleset_hit_policy`, `cr_ruleset_logic`, `cr_ruleset_aggregate` and
  `cr_ruleset_strict` describe one; its summary has the rest, and
  `cr_ruleset_summary_actions` marks the rules whose action is code of the
  program (one `bool` per rule, or none). `cr_ruleset_rule_position` looks a rule up by
  name: a name it lacks is a `CR_RULE_NOT_FOUND` with the names there are.
- **Decisions.** Which rules fired (`cr_decision_fired`, `cr_decision_truth`),
  which were selected (`cr_decision_selected`) and the outputs
  (`cr_decision_output_value`, `cr_decision_outputs_value`) are kept when the
  decision is made, so reading them is free. Writing and explaining a decision
  evaluate its input again, or check its recorded decision again.
  `cr_decision_clone` keeps a decision borrowed from a batch, an action call
  or an error after what lends it is freed. `cr_decision_explain_value` gives
  the explanation as a `cr_value`, for bindings that build objects of it: the
  structure of the explanation document (`spec/json-format.md`,
  *Explanations*) with the values themselves (opaque tokens included), plus
  the `text` of each rule and cause and whether each cause `holds`.
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
  `cr_ruleset_evaluate*` never run actions. A binding that runs actions
  itself (Java's and JavaScript's idiomatic `run`) makes the `action_error`
  of one that failed with `cr_error_new_action`, so that its message, hint and
  JSON form are the core's.
- **Grids.** `cr_grid_new` starts a decision table from its settings and a
  header of strings, `cr_grid_push_row` adds rows of cells (a string is text,
  as in CSV; any other value a constant; null an empty cell), and
  `cr_grid_finish` gives the table's document (`spec/tables.md`, *Grids*).
  After a row fails, the grid can only be freed. A binding with nodes and
  constants of its own starts with `cr_grid_new_open` instead, adds the
  columns one at a time (`cr_grid_add_column_text`, `cr_grid_add_column_input`
  with a node, `cr_grid_add_column_output` with a key and a list of output
  values), which are read and checked at the first row or at
  `cr_grid_finish`, and may push rows whose cells are nodes
  (`cr_grid_push_row_nodes`, one node pointer or `NULL` per cell), or rows
  whose cells say what they are (`cr_grid_push_row_cells`: `CR_CELL_TEXT`,
  `CR_CELL_VALUE` or `CR_CELL_NODE` per cell), so that a string can be a
  value, such as an enum constant: a constant under an input or an output, a
  name or description under `RULE` or `DESCRIPTION`. Errors
  point at `/header/<c>` or `/rows/<r>/<c>`. What a table holds:
  `cr_document_table_value` (its settings, the number of inputs and rows, and
  the outputs with their values), `cr_document_table_row` (a row's name,
  priority and description), `cr_document_table_input` and
  `cr_document_table_cell` (copies of the nodes; `NULL` for an empty cell).
- **Single nodes.** `cr_node_read` reads one condition or value (the node of
  format version 1, not a document), in JSON, YAML or the rule syntax;
  `cr_node_evaluate_tree` (with a logic and strictness) and
  `cr_node_evaluate_value_tree` (with strictness) evaluate it on an input and
  give its result tree as JSON, as `evaluate_tree` and `evaluate_value_tree`
  do in every language (`spec/ast.md`, *Evaluating one node*);
  `cr_node_field_paths` lists the paths it reads, as `cr_ruleset_field_paths`
  does for a rule set. For bindings that give the tree as objects,
  `cr_tree_new_condition` and `cr_tree_new_value` give it as a `cr_tree`, and
  `cr_decision_rule_tree` gives the tree of a rule's condition in a decision:
  `cr_tree_value` is the tree as a `cr_value` in the structure of the JSON
  result tree (opaque values keep their token; `cr_value_to_json` of it is the
  JSON), and `cr_tree_len` and `cr_tree_node` give the node of each result, in
  pre-order. Python calls a node `Expr`; C, Go and the JVM call it a
  node.
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
