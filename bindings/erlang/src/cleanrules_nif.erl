%% @private
%% The NIF of cleanrules (crate cleanrules-erlang, native/cleanrules_nif):
%% internal, for the idiomatic module `cleanrules' (task N.3), which builds on
%% it. Every function is a thin call into the Rust core: none evaluates,
%% explains or decides anything in Erlang.
%%
%% Conventions:
%%
%% - A rule set is a resource (an opaque reference): immutable, shared by
%%   every process that holds it, freed when the last one drops it.
%% - Fallible functions return `{ok, Result}' or `{error, Error}', where
%%   `Error' is the map of the core's error (`spec/errors.md', JSON form) with
%%   atom keys, `code' an atom; a panic in the library comes back the same way,
%%   with the code `internal_error'. An argument of the wrong kind (a format
%%   that is not one of the atoms, a negative `MaxValues') raises `badarg'.
%% - Terms are read as `spec/data-model.md' (Erlang) says, and values come
%%   back as it says too.
%% - One input is evaluated on the normal scheduler when it is small enough,
%%   and on a dirty CPU scheduler otherwise; batches, explanations, documents
%%   and conversions run on dirty CPU schedulers.
-module(cleanrules_nif).

-export([
    version/0,
    format_version/0,
    default_limits/0,
    ruleset_read/3,
    ruleset_write/2,
    ruleset_info/1,
    ruleset_summary/2,
    error_parent/1,
    evaluate/3,
    evaluate_normal/3,
    evaluate_dirty/3,
    evaluate_many/3,
    explain/4,
    decision_write/4,
    decision_read/4,
    value_to_json/3,
    value_from_json/1,
    test_panic/0
]).

-export_type([ruleset/0, error/0, decision/0, rule_result/0, format/0, limits/0]).

-on_load(load/0).

-nifs([
    version/0,
    format_version/0,
    default_limits/0,
    ruleset_read/3,
    ruleset_write/2,
    ruleset_info/1,
    ruleset_summary/2,
    error_parent/1,
    evaluate_normal/3,
    evaluate_dirty/3,
    evaluate_many/3,
    explain/4,
    decision_write/4,
    decision_read/4,
    value_to_json/3,
    value_from_json/1,
    test_panic/0
]).

-type ruleset() :: reference().
%% A rule set: a resource of the NIF.

-type error() :: #{code := atom(), message := binary(), atom() => term()}.
%% The core's error: `code', `message', and `hint' and the context fields
%% (`pointer', `line', `column', `rule', `rules', `path', `types', `row',
%% `name', `available', `executed', `node', `operation', `reason', `cause')
%% that apply. `node' is the node's JSON form as a term (maps with binary
%% keys).

-type rule_result() :: #{
    rule := binary(),
    fired := boolean(),
    truth := true | false | unknown,
    output => #{binary() => term()}
}.

-type decision() :: #{
    results := [rule_result()],
    selected := [binary()],
    output := {ok, #{binary() => term()} | null} | {error, error()},
    aggregated => #{binary() => term()} | null
}.
%% A decision: each rule's result in rule order (`output' when its payload
%% was evaluated), the selected rules' names, the single output (or the error
%% of asking a multi-hit decision for it), and the aggregate when the rule
%% set has one.

-type format() :: json | yaml | text | csv.

-type limits() :: #{
    max_size => non_neg_integer(),
    max_nesting => non_neg_integer(),
    max_depth => non_neg_integer(),
    max_table_size => non_neg_integer(),
    max_yaml_size => non_neg_integer(),
    max_text_size => non_neg_integer(),
    max_values => non_neg_integer(),
    max_path_segments => non_neg_integer()
}.

-define(NOT_LOADED, erlang:nif_error({not_loaded, ?MODULE})).

%% Load the library from the application's priv directory (or, when the
%% application is not known to the code server, from ../priv next to this
%% module's beam).
-spec load() -> ok | {error, term()}.
load() ->
    erlang:load_nif(filename:join(priv_dir(), "cleanrules_nif"), 0).

-spec priv_dir() -> file:filename().
priv_dir() ->
    case code:priv_dir(cleanrules) of
        {error, bad_name} ->
            Ebin = filename:dirname(code:which(?MODULE)),
            filename:join(filename:dirname(Ebin), "priv");
        Dir ->
            Dir
    end.

%% The library's version, as a binary: the application's.
-spec version() -> binary().
version() -> ?NOT_LOADED.

%% The version of the JSON format it reads and writes.
-spec format_version() -> pos_integer().
format_version() -> ?NOT_LOADED.

%% The default limits.
-spec default_limits() -> limits().
default_limits() -> ?NOT_LOADED.

%% A rule set from a document (a rule set, a rule or a decision table) in a
%% format, with limits (`#{}' for the defaults).
-spec ruleset_read(binary(), format(), limits()) -> {ok, ruleset()} | {error, error()}.
ruleset_read(_Text, _Format, _Limits) -> ?NOT_LOADED.

%% A rule set as a document.
-spec ruleset_write(ruleset(), format()) -> {ok, binary()} | {error, error()}.
ruleset_write(_RuleSet, _Format) -> ?NOT_LOADED.

%% What a rule set is: its policies, rule names and the field paths it reads.
-spec ruleset_info(ruleset()) ->
    #{
        hit_policy := first | unique | priority | any | collect | rule_order,
        aggregate := sum | min | max | count | null,
        logic := atom(),
        strict := boolean(),
        rules := [binary()],
        field_paths := [[binary() | non_neg_integer()]]
    }.
ruleset_info(_RuleSet) -> ?NOT_LOADED.

%% A rule set's summary as a term (`term': the summary document's content,
%% maps with binary keys), as a document (`json', `yaml') or as its `text'.
-spec ruleset_summary(ruleset(), term | json | yaml | text) -> {ok, term()} | {error, error()}.
ruleset_summary(_RuleSet, _Format) -> ?NOT_LOADED.

%% The parent of an error code in the hierarchy (spec/errors.md), from the
%% core: `undefined' for a code directly under the base, and for
%% `internal_error', which is outside it.
-spec error_parent(atom()) -> atom().
error_parent(_Code) -> ?NOT_LOADED.

%% Evaluate one input, reading at most MaxValues values from it: on the
%% normal scheduler when it is small enough, on a dirty CPU scheduler
%% otherwise.
-spec evaluate(ruleset(), term(), non_neg_integer()) -> {ok, decision()} | {error, error()}.
evaluate(RuleSet, Data, MaxValues) ->
    case evaluate_normal(RuleSet, Data, MaxValues) of
        dirty -> evaluate_dirty(RuleSet, Data, MaxValues);
        Result -> Result
    end.

%% Evaluate one input on the normal scheduler, or return `dirty' when the
%% rule set or the input is too large to stay near a millisecond there, or
%% when one of the rule set's calls there took longer than a millisecond.
-spec evaluate_normal(ruleset(), term(), non_neg_integer()) ->
    {ok, decision()} | {error, error()} | dirty.
evaluate_normal(_RuleSet, _Data, _MaxValues) -> ?NOT_LOADED.

%% Evaluate one input on a dirty CPU scheduler.
-spec evaluate_dirty(ruleset(), term(), non_neg_integer()) -> {ok, decision()} | {error, error()}.
evaluate_dirty(_RuleSet, _Data, _MaxValues) -> ?NOT_LOADED.

%% Evaluate a list of inputs, each read with at most MaxValues values, on a
%% dirty CPU scheduler: the error is the first input's in order that fails,
%% with its `row'.
-spec evaluate_many(ruleset(), [term()], non_neg_integer()) ->
    {ok, [decision()]} | {error, error()}.
evaluate_many(_RuleSet, _Inputs, _MaxValues) -> ?NOT_LOADED.

%% Evaluate one input and explain its decision, on a dirty CPU scheduler: as
%% a term (`term': the structure of the explanation document, maps with binary
%% keys, with the values themselves), as a document (`json', `yaml') or as
%% its `text'.
-spec explain(ruleset(), term(), non_neg_integer(), term | json | yaml | text) ->
    {ok, term()} | {error, error()}.
explain(_RuleSet, _Data, _MaxValues, _Format) -> ?NOT_LOADED.

%% Evaluate one input and write its decision as a `json' or `yaml'
%% document, on a dirty CPU scheduler (opaque values as {"$opaque": Type}).
-spec decision_write(ruleset(), term(), non_neg_integer(), json | yaml) ->
    {ok, binary()} | {error, error()}.
decision_write(_RuleSet, _Data, _MaxValues, _Format) -> ?NOT_LOADED.

%% Read a recorded decision document with the rule set it was made with, on
%% a dirty CPU scheduler; its opaque values come back as `null'.
-spec decision_read(ruleset(), binary(), json | yaml, limits()) ->
    {ok, decision()} | {error, error()}.
decision_read(_RuleSet, _Text, _Format, _Limits) -> ?NOT_LOADED.

%% A term converted as `data' (as where a path ends) or as a `constant'
%% (opaque values and integers or decimals out of range are an
%% `expression_error'), in the JSON format.
-spec value_to_json(term(), data | constant, non_neg_integer()) ->
    {ok, binary()} | {error, error()}.
value_to_json(_Term, _Mode, _MaxValues) -> ?NOT_LOADED.

%% A value in the JSON format, read as data, as a term.
-spec value_from_json(binary()) -> {ok, term()} | {error, error()}.
value_from_json(_Json) -> ?NOT_LOADED.

%% Panic inside the library on purpose: `{error, #{code => internal_error}}'.
-spec test_panic() -> {error, error()}.
test_panic() -> ?NOT_LOADED.
