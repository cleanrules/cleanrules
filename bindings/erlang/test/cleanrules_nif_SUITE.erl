%% Tests of the NIF (task N.2): terms read and given back as
%% spec/data-model.md (Erlang) says, max_values, rule sets as shared
%% resources, the schedulers, panics and errors. The idiomatic API (N.3) and
%% the conformance runner (N.4) have their own suites.
-module(cleanrules_nif_SUITE).

-include_lib("common_test/include/ct.hrl").
-include_lib("stdlib/include/assert.hrl").

-export([all/0, groups/0, init_per_suite/1, end_per_suite/1]).
-export([
    version_and_limits/1,
    scalars_read_and_given_back/1,
    integers_and_floats/1,
    atoms/1,
    binaries/1,
    decimals/1,
    elixir_decimals/1,
    calendar_dates/1,
    aware_datetimes/1,
    elixir_dates/1,
    maps_and_structs/1,
    lists_and_opaque_terms/1,
    paths_read_keys_and_indices/1,
    reading_never_creates_atoms/1,
    struct_paths/1,
    deep_terms/1,
    opaque_equality/1,
    max_values_counts/1,
    max_values_in_batches/1,
    constants/1,
    values_from_json/1,
    resource_shared_by_processes/1,
    resource_outlives_its_creator/1,
    ruleset_documents/1,
    normal_scheduler_budget/1,
    dirty_calls/1,
    normal_call_time/1,
    panics/1,
    core_errors/1,
    bad_arguments/1,
    decisions/1,
    explanations/1,
    summaries/1,
    decision_documents/1,
    error_parents/1,
    error_nodes/1,
    adaptive_dirty/1,
    budget_before_work/1,
    safe_depth_limits/1,
    long_path_regression/1
]).

all() ->
    [{group, conversion}, {group, limits}, {group, resources}, {group, schedulers}, {group, errors}, {group, results}].

groups() ->
    [
        {conversion, [parallel], [
            scalars_read_and_given_back,
            integers_and_floats,
            atoms,
            binaries,
            decimals,
            elixir_decimals,
            calendar_dates,
            aware_datetimes,
            elixir_dates,
            maps_and_structs,
            lists_and_opaque_terms,
            paths_read_keys_and_indices,
            reading_never_creates_atoms,
            struct_paths,
            deep_terms,
            opaque_equality
        ]},
        {limits, [parallel], [max_values_counts, max_values_in_batches, constants, values_from_json]},
        {resources, [], [version_and_limits, resource_shared_by_processes, resource_outlives_its_creator, ruleset_documents]},
        {schedulers, [], [normal_scheduler_budget, dirty_calls, normal_call_time, adaptive_dirty, budget_before_work]},
        {errors, [], [panics, core_errors, bad_arguments, error_parents, error_nodes, safe_depth_limits]},
        {results, [parallel], [decisions, explanations, summaries, decision_documents, long_path_regression]}
    ].

init_per_suite(Config) ->
    {module, cleanrules_nif} = code:ensure_loaded(cleanrules_nif),
    Config.

end_per_suite(_Config) ->
    ok.

%% --- Helpers -----------------------------------------------------------------

-define(MAX, 1000000).

ruleset(Text) ->
    {ok, RuleSet} = cleanrules_nif:ruleset_read(Text, text, #{}),
    RuleSet.

%% A rule set that always fires and gives back field x as v.
echo() ->
    ruleset(<<"RULE echo IF x IS NULL OR NOT x IS NULL THEN v = x">>).

%% What the echo rule set gives back for Data as x.
echo(Data) ->
    echo(echo(), Data).

echo(RuleSet, Data) ->
    {ok, #{output := {ok, #{<<"v">> := Value}}}} = cleanrules_nif:evaluate(RuleSet, #{x => Data}, ?MAX),
    Value.

%% Data read as a value and written in the JSON format.
json(Term) ->
    {ok, Json} = cleanrules_nif:value_to_json(Term, data, ?MAX),
    Json.

%% The truth of `x > 0' on Data, and the reason in the JSON explanation.
reason_of(Data) ->
    RuleSet = ruleset(<<"RULE r IF x > 0">>),
    {ok, Json} = cleanrules_nif:explain(RuleSet, #{x => Data}, ?MAX, json),
    Json.

%% --- Conversion --------------------------------------------------------------

scalars_read_and_given_back(_Config) ->
    %% Each value reads back as itself (spec/data-model.md, Erlang: "Each
    %% value reads back as the same value").
    Same = [
        true,
        false,
        null,
        0,
        -7,
        1.5,
        -0.0,
        {cleanrules_float, <<"inf">>},
        {cleanrules_float, <<"-inf">>},
        {cleanrules_decimal, <<"0.10">>},
        <<"gold">>,
        <<"héllo"/utf8>>,
        {2026, 11, 12},
        {{2026, 11, 12}, {9, 30, 0}},
        {{2026, 11, 12}, {9, 30, 0, 5}},
        {cleanrules_datetime, {{2026, 11, 12}, {9, 30, 0}}, 3600, <<"Europe/Madrid">>},
        {cleanrules_datetime, {{2026, 11, 12}, {9, 30, 0}}, -18000, undefined},
        [1, <<"a">>, [2, null]],
        #{<<"a">> => 1, <<"b">> => [#{<<"c">> => true}]}
    ],
    [?assertEqual(Term, echo(Term), Term) || Term <- Same],
    %% -0.0 is kept.
    ?assert(echo(-0.0) =:= -0.0),
    ok.

integers_and_floats(_Config) ->
    Max = (1 bsl 63) - 1,
    Min = -(1 bsl 63),
    ?assertEqual(Max, echo(Max)),
    ?assertEqual(Min, echo(Min)),
    %% Integers beyond 64 bits are null with reason int_overflow.
    ?assertEqual(null, echo(Max + 1)),
    ?assertEqual(null, echo(Min - 1)),
    ?assertEqual(null, echo(1 bsl 200)),
    ?assertMatch({match, _}, re:run(reason_of(1 bsl 70), <<"int_overflow">>)),
    ?assertEqual(<<"1e300">>, json(1.0e300)),
    ?assertEqual(<<"[1,1.5]">>, json([1, 1.5])),
    ok.

atoms(_Config) ->
    ?assertEqual(true, echo(true)),
    ?assertEqual(false, echo(false)),
    %% null, undefined and Elixir's nil are null; null comes back as null.
    [?assertEqual(null, echo(A)) || A <- [null, undefined, nil]],
    ?assertMatch({match, _}, re:run(reason_of(undefined), <<"\"reason\":\\{\"code\":\"null\"">>)),
    %% Other atoms are text, their name; Gleam's None is the text "none".
    ?assertEqual(<<"gold">>, echo(gold)),
    ?assertEqual(<<"none">>, echo(none)),
    ?assertEqual(<<"Elixir.Foo">>, echo('Elixir.Foo')),
    ?assertEqual(<<"ñandú"/utf8>>, echo(binary_to_atom(<<"ñandú"/utf8>>))),
    ok.

binaries(_Config) ->
    ?assertEqual(<<>>, echo(<<>>)),
    ?assertEqual(<<"€"/utf8>>, echo(<<"€"/utf8>>)),
    %% Binaries that are not UTF-8, and bitstrings, are opaque: the same
    %% term comes back.
    ?assertEqual(<<255, 0>>, echo(<<255, 0>>)),
    ?assertEqual(<<1:3>>, echo(<<1:3>>)),
    ?assertEqual(<<"{\"$opaque\":\"binary\"}">>, json(<<255>>)),
    ?assertEqual(<<"{\"$opaque\":\"bitstring\"}">>, json(<<1:3>>)),
    %% Charlists are lists of integers, never text.
    ?assertEqual("abc", echo("abc")),
    ?assertEqual(<<"[97,98,99]">>, json("abc")),
    ok.

decimals(_Config) ->
    D = fun(Text) -> echo({cleanrules_decimal, Text}) end,
    ?assertEqual({cleanrules_decimal, <<"1.50">>}, D(<<"1.50">>)),
    %% Exponent forms are read; the text comes back canonical.
    ?assertEqual({cleanrules_decimal, <<"0.0015">>}, D(<<"1.5e-3">>)),
    ?assertEqual({cleanrules_decimal, <<"1000">>}, D(<<"1E+3">>)),
    ?assertEqual({cleanrules_decimal, <<"inf">>}, D(<<"inf">>)),
    ?assertEqual({cleanrules_decimal, <<"-inf">>}, D(<<"-inf">>)),
    %% Beyond 38 digits: null with reason decimal_overflow.
    ?assertEqual(null, D(<<"1e40">>)),
    ?assertMatch({match, _}, re:run(reason_of({cleanrules_decimal, <<"1e40">>}), <<"decimal_overflow">>)),
    %% Malformed tagged tuples are opaque.
    [?assertEqual(T, echo(T)) || T <- [{cleanrules_decimal, <<"abc">>}, {cleanrules_decimal, 1}, {cleanrules_decimal, <<255>>}]],
    [?assertEqual(T, echo(T)) || T <- [{cleanrules_float, <<"nan">>}, {cleanrules_float, inf}, {cleanrules_float, <<"inf">>, x}]],
    ok.

elixir_decimals(_Config) ->
    D = fun(Sign, Coef, Exp) -> #{'__struct__' => 'Elixir.Decimal', sign => Sign, coef => Coef, exp => Exp} end,
    ?assertEqual({cleanrules_decimal, <<"-123.45">>}, echo(D(-1, 12345, -2))),
    ?assertEqual({cleanrules_decimal, <<"1000">>}, echo(D(1, 1, 3))),
    ?assertEqual({cleanrules_decimal, <<"0">>}, echo(D(-1, 0, 0))),
    ?assertEqual({cleanrules_decimal, <<"inf">>}, echo(D(1, inf, 0))),
    ?assertEqual({cleanrules_decimal, <<"-inf">>}, echo(D(-1, inf, 0))),
    [?assertEqual(null, echo(D(1, N, 0))) || N <- ['NaN', qNaN, sNaN]],
    %% A coefficient beyond 64 bits; trailing zeros dropped only as far as
    %% needed (spec/data-model.md, Decimals).
    ?assertEqual(
        {cleanrules_decimal, <<"1000000000000000000000000000000.0000000">>},
        echo(D(1, binary_to_integer(<<"1", (binary:copy(<<"0">>, 40))/binary>>), -10))
    ),
    ?assertEqual({cleanrules_decimal, <<"18446744073709551616">>}, echo(D(1, 1 bsl 64, 0))),
    %% Out of range: null with decimal_overflow; zero fits at any exponent.
    ?assertEqual(null, echo(D(1, 1, 39))),
    ?assertEqual(null, echo(D(1, 1 bsl 200, 0))),
    ?assertEqual(null, echo(D(1, 1 bsl 9000, 0))),
    ?assertEqual(null, echo(D(1, 1, 1 bsl 70))),
    ?assertEqual(null, echo(D(1, 1, -(1 bsl 70)))),
    ?assertEqual({cleanrules_decimal, <<"0">>}, echo(D(1, 0, 1 bsl 70))),
    ?assertEqual({cleanrules_decimal, <<"0.00000000000000000000000000000000000000">>}, echo(D(1, 0, -(1 bsl 70)))),
    ?assertEqual({cleanrules_decimal, <<"0.00000000000000000000000000000000000000">>}, echo(D(1, 0, -(1 bsl 62)))),
    %% Shapes that are not a decimal's are opaque.
    Bad = [D(0, 1, 0), D(1, -1, 0), D(1, -(1 bsl 70), 0), D(1, 1.5, 0), D(1, 1, 0.5), D(1, foo, 0), maps:remove(exp, D(1, 1, 0))],
    [?assertEqual(T, echo(T)) || T <- Bad],
    ok.

calendar_dates(_Config) ->
    ?assertEqual({2026, 11, 12}, echo({2026, 11, 12})),
    ?assertEqual({1, 1, 1}, echo({1, 1, 1})),
    ?assertEqual({9999, 12, 31}, echo({9999, 12, 31})),
    %% A tuple of three integers that is a valid date is a date, whatever it
    %% meant: a time of day or a timestamp (spec/data-model.md, Erlang).
    ?assertEqual(<<"{\"$date\":\"0009-05-07\"}">>, json({9, 5, 7})),
    ?assertEqual(<<"{\"$date\":\"0001-02-03\"}">>, json({1, 2, 3})),
    %% Invalid or out of range: opaque.
    Opaque = [{2026, 2, 30}, {0, 1, 1}, {10000, 1, 1}, {2026, 13, 1}, {1 bsl 70, 1, 1}, {2026, 1, 1.0}, {1700000, 123456, 789}],
    [?assertEqual(T, echo(T)) || T <- Opaque],
    [?assertEqual(<<"{\"$opaque\":\"tuple\"}">>, json(T)) || T <- Opaque],
    ?assertEqual({{2026, 11, 12}, {23, 59, 59}}, echo({{2026, 11, 12}, {23, 59, 59}})),
    %% Microseconds 0 come back without them.
    ?assertEqual({{2026, 11, 12}, {9, 30, 0}}, echo({{2026, 11, 12}, {9, 30, 0, 0}})),
    ?assertEqual(<<"{\"$datetime\":\"2026-11-12T09:30:00.000005\"}">>, json({{2026, 11, 12}, {9, 30, 0, 5}})),
    BadTimes = [
        {{2026, 1, 1}, {24, 0, 0}},
        {{2026, 1, 1}, {0, 60, 0}},
        {{2026, 1, 1}, {23, 59, 60}},
        {{2026, 1, 1}, {0, 0, 0, 1000000}},
        {{2026, 1, 1}, {0, 0}},
        {{2026, 2, 30}, {0, 0, 0}},
        {date, {0, 0, 0}}
    ],
    [?assertEqual(T, echo(T)) || T <- BadTimes],
    ok.

aware_datetimes(_Config) ->
    A = fun(Local, Offset, Zone) -> {cleanrules_datetime, Local, Offset, Zone} end,
    L = {{2026, 11, 12}, {9, 30, 0}},
    ?assertEqual(<<"{\"$datetime\":\"2026-11-12T09:30:00+01:00[Europe/Madrid]\"}">>, json(A(L, 3600, <<"Europe/Madrid">>))),
    ?assertEqual(<<"{\"$datetime\":\"2026-11-12T09:30:00Z\"}">>, json(A(L, 0, undefined))),
    %% A zone that disagrees with the offset, or is unknown, is dropped.
    ?assertEqual(A(L, 7200, undefined), echo(A(L, 7200, <<"Europe/Madrid">>))),
    ?assertEqual(A(L, 3600, undefined), echo(A(L, 3600, <<"Mars/Olympus">>))),
    %% With microseconds, and the local time comes back in the zone.
    ?assertEqual(A({{2026, 7, 1}, {12, 0, 0, 7}}, 7200, <<"Europe/Madrid">>), echo(A({{2026, 7, 1}, {12, 0, 0, 7}}, 7200, <<"Europe/Madrid">>))),
    %% Offsets beyond 18 hours, instants outside years 1 to 9999, and other
    %% shapes are opaque.
    Opaque = [
        A(L, 18 * 3600 + 1, undefined),
        A({{1, 1, 1}, {0, 0, 0}}, 3600, undefined),
        A({{9999, 12, 31}, {23, 0, 0}}, -3600, undefined),
        A({2026, 11, 12}, 0, undefined),
        A(L, 1.5, undefined),
        A(L, 0, utc),
        A(L, 0, <<255>>),
        A(L, 1 bsl 70, undefined)
    ],
    [?assertEqual(T, echo(T)) || T <- Opaque],
    ok.

elixir_dates(_Config) ->
    Iso = 'Elixir.Calendar.ISO',
    Date = #{'__struct__' => 'Elixir.Date', calendar => Iso, year => 2026, month => 11, day => 12},
    ?assertEqual({2026, 11, 12}, echo(Date)),
    Naive = maps:merge(Date#{'__struct__' := 'Elixir.NaiveDateTime'}, #{hour => 9, minute => 30, second => 0, microsecond => {5, 6}}),
    ?assertEqual({{2026, 11, 12}, {9, 30, 0, 5}}, echo(Naive)),
    ?assertEqual({{2026, 11, 12}, {9, 30, 0}}, echo(Naive#{microsecond := {0, 0}})),
    DateTime = maps:merge(Naive#{'__struct__' := 'Elixir.DateTime', microsecond := {0, 0}}, #{
        time_zone => <<"Etc/UTC">>, zone_abbr => <<"UTC">>, utc_offset => 0, std_offset => 0
    }),
    %% Etc/UTC with offset 0 is Z.
    ?assertEqual(<<"{\"$datetime\":\"2026-11-12T09:30:00Z\"}">>, json(DateTime)),
    ?assertEqual({cleanrules_datetime, {{2026, 11, 12}, {9, 30, 0}}, 0, undefined}, echo(DateTime)),
    %% The offset is utc_offset plus std_offset (summer time in Madrid).
    Madrid = DateTime#{month := 7, time_zone := <<"Europe/Madrid">>, zone_abbr := <<"CEST">>, utc_offset := 3600, std_offset := 3600},
    ?assertEqual(<<"{\"$datetime\":\"2026-07-12T09:30:00+02:00[Europe/Madrid]\"}">>, json(Madrid)),
    %% Another calendar, or fields of other shapes: opaque.
    Opaque = [
        Date#{calendar := 'Elixir.Calendar.Holocene'},
        Date#{year := 0},
        Date#{month := 13},
        maps:remove(day, Date),
        Naive#{microsecond := 5},
        Naive#{microsecond := {5, 7}},
        Naive#{microsecond := {1000000, 6}},
        Naive#{hour := 24},
        DateTime#{time_zone := utc},
        DateTime#{utc_offset := 64801},
        DateTime#{std_offset := 1.0}
    ],
    [?assertEqual(T, echo(T)) || T <- Opaque],
    ?assertEqual(<<"{\"$opaque\":\"Elixir.Date\"}">>, json(Date#{calendar := 'Elixir.Calendar.Holocene'})),
    ok.

maps_and_structs(_Config) ->
    %% Binary and atom keys, by name; a binary key wins over an atom with the
    %% same text; other keys are left out.
    ?assertEqual(
        #{<<"a">> => 1, <<"b">> => 4},
        echo(#{<<"a">> => 1, a => 2, 3 => 4, b => 4, {k} => 5, "a" => 6, <<255>> => 7})
    ),
    %% Sorted by key, by code point.
    ?assertEqual(<<"{\"B\":1,\"a\":2,\"z\":4,\"é\":3}"/utf8>>, json(#{z => 4, <<"é"/utf8>> => 3, a => 2, 'B' => 1})),
    %% Structs are maps of their fields, without those starting with __.
    User = #{'__struct__' => 'Elixir.User', '__meta__' => meta, name => <<"Ada">>, age => 36},
    ?assertEqual(#{<<"name">> => <<"Ada">>, <<"age">> => 36}, echo(User)),
    %% A __struct__ that is not an atom is not a struct.
    ?assertEqual(#{<<"__struct__">> => 1}, echo(#{'__struct__' => 1})),
    %% These structs, and exceptions, are opaque.
    Opaque = [
        #{'__struct__' => 'Elixir.MapSet', map => #{}},
        #{'__struct__' => 'Elixir.Range', first => 1, last => 2, step => 1},
        #{'__struct__' => 'Elixir.Regex', source => <<"a">>},
        #{'__struct__' => 'Elixir.Time', hour => 1},
        #{'__struct__' => 'Elixir.Duration', year => 1},
        #{'__struct__' => 'Elixir.ArgumentError', '__exception__' => true, message => <<"m">>}
    ],
    [?assertEqual(T, echo(T)) || T <- Opaque],
    ?assertEqual(<<"{\"$opaque\":\"Elixir.MapSet\"}">>, json(hd(Opaque))),
    ok.

lists_and_opaque_terms(_Config) ->
    Ref = make_ref(),
    Fun = fun() -> ok end,
    %% A port of our own: erlang:ports() may be empty (it is on Windows, with
    %% no port drivers open), and the inet backend's sockets are ports.
    {ok, Port} = gen_udp:open(0, [{inet_backend, inet}]),
    true = is_port(Port),
    [?assert(T =:= echo(T)) || T <- [self(), Ref, Fun, Port, {a, b}, {}, [1 | 2], [a | b]]],
    ?assertEqual(<<"{\"$opaque\":\"pid\"}">>, json(self())),
    ?assertEqual(<<"{\"$opaque\":\"reference\"}">>, json(Ref)),
    ?assertEqual(<<"{\"$opaque\":\"fun\"}">>, json(Fun)),
    ?assertEqual(<<"{\"$opaque\":\"port\"}">>, json(Port)),
    ?assertEqual(<<"{\"$opaque\":\"improper_list\"}">>, json([1 | 2])),
    %% A list of pairs (proplists, keyword lists) is a list of opaque tuples.
    ?assertEqual(<<"[{\"$opaque\":\"tuple\"}]">>, json([{a, 1}])),
    ?assertEqual([{a, 1}, {b, 2}], echo([{a, 1}, {b, 2}])),
    %% Opaque values inside lists and maps come back as they were.
    ?assertEqual([self(), #{<<"r">> => Ref}], echo([self(), #{r => Ref}])),
    ok = gen_udp:close(Port).

paths_read_keys_and_indices(_Config) ->
    RuleSet = ruleset(<<"RULE r IF NOT a IS NULL OR a IS NULL THEN b = a.b, c = items[1].c, d = items[5]">>),
    Out = fun(Data) ->
        {ok, #{output := {ok, Output}}} = cleanrules_nif:evaluate(RuleSet, Data, ?MAX),
        Output
    end,
    Items = [#{c => 1}, #{<<"c">> => 2}],
    ?assertEqual(#{<<"b">> => 10, <<"c">> => 2, <<"d">> => null}, Out(#{a => #{b => 10}, items => Items})),
    %% The binary key first, then the atom.
    ?assertEqual(10, maps:get(<<"b">>, Out(#{<<"a">> => #{<<"b">> => 10, b => 11}}))),
    ?assertEqual(11, maps:get(<<"b">>, Out(#{<<"a">> => #{b => 11}}))),
    %% Integer segments index proper lists only: never tuples, maps or binaries.
    [?assertEqual(null, maps:get(<<"c">>, Out(#{items => Bad}))) || Bad <- [{x, #{c => 1}}, #{1 => #{c => 1}}, <<"ab">>, [x | y]]],
    %% A path through something else is missing.
    ?assertEqual(null, maps:get(<<"b">>, Out(#{a => 5}))),
    ?assertEqual(null, maps:get(<<"b">>, Out(#{a => [#{b => 1}]}))),
    ?assertEqual(null, maps:get(<<"b">>, Out(#{a => {b, 1}}))),
    %% The whole input may be a list.
    Top = ruleset(<<"RULE r IF NOT [0] IS NULL OR [0] IS NULL THEN v = [1]">>),
    {ok, #{output := {ok, #{<<"v">> := 20}}}} = cleanrules_nif:evaluate(Top, [10, 20], ?MAX),
    ok.

reading_never_creates_atoms(_Config) ->
    Name = <<"cleanrules_suite_field_that_is_never_an_atom">>,
    RuleSet = ruleset(<<"RULE r IF ", Name/binary, " IS NULL OR NOT ", Name/binary, " IS NULL THEN v = ", Name/binary>>),
    ?assertError(badarg, binary_to_existing_atom(Name)),
    {ok, #{output := {ok, #{<<"v">> := null}}}} = cleanrules_nif:evaluate(RuleSet, #{other => 1}, ?MAX),
    {ok, #{output := {ok, #{<<"v">> := 7}}}} = cleanrules_nif:evaluate(RuleSet, #{Name => 7}, ?MAX),
    {ok, _} = cleanrules_nif:evaluate_many(RuleSet, [#{a => 1}], ?MAX),
    {ok, _} = cleanrules_nif:explain(RuleSet, #{a => 1}, ?MAX, term),
    ?assertError(badarg, binary_to_existing_atom(Name)),
    ok.

struct_paths(_Config) ->
    RuleSet = ruleset(<<"RULE r IF d IS NULL OR NOT d IS NULL THEN y = d.year, m = u.__meta__, n = u.name">>),
    Date = #{'__struct__' => 'Elixir.Date', calendar => 'Elixir.Calendar.ISO', year => 2026, month => 1, day => 2},
    User = #{'__struct__' => 'Elixir.User', '__meta__' => secret, name => <<"Ada">>},
    {ok, #{output := {ok, Output}}} = cleanrules_nif:evaluate(RuleSet, #{d => Date, u => User}, ?MAX),
    %% d.year on a %Date{} is missing, as on a date in every language; the
    %% fields starting with __ are never read.
    ?assertEqual(#{<<"y">> => null, <<"m">> => null, <<"n">> => <<"Ada">>}, Output),
    ok.

deep_terms(_Config) ->
    Nest = fun Nest(0, Inner) -> Inner; Nest(N, Inner) -> Nest(N - 1, [Inner]) end,
    %% 256 containers are converted; the next one is opaque at that place.
    ?assertEqual(nomatch, re:run(json(Nest(256, 1)), <<"opaque">>)),
    Deep = json(Nest(257, 1)),
    ?assertMatch({match, _}, re:run(Deep, <<"\\{\"\\$opaque\":\"list\"\\}">>)),
    ?assertEqual(Nest(300, 1), echo(Nest(300, 1))),
    DeepMap = lists:foldl(fun(_, Acc) -> #{k => Acc} end, 1, lists:seq(1, 300)),
    ?assertMatch({match, _}, re:run(json(DeepMap), <<"\\{\"\\$opaque\":\"map\"\\}">>)),
    %% Very deep terms and long paths on dirty schedulers, whose stacks are
    %% small: read with explicit stacks.
    Huge = Nest(100000, 1),
    {ok, _} = cleanrules_nif:evaluate_dirty(echo(), #{x => Huge}, ?MAX),
    {ok, _} = cleanrules_nif:evaluate_many(echo(), [#{x => Huge}], ?MAX),
    Path = iolist_to_binary(["x", lists:duplicate(5000, "[0]")]),
    {ok, Long} = cleanrules_nif:ruleset_read(<<"RULE r IF ", Path/binary, " = 1">>, text, #{max_path_segments => 5001}),
    {ok, #{selected := [<<"r">>]}} = cleanrules_nif:evaluate_dirty(Long, #{x => Nest(5000, 1)}, ?MAX),
    ok.

opaque_equality(_Config) ->
    %% Under ANY, outputs are compared; two opaque values are the same object
    %% when they are exactly equal (=:=).
    RuleSet = ruleset(<<"HIT POLICY any\nRULE a IF x IS NOT NULL THEN v = x\nRULE b IF y IS NOT NULL THEN v = y">>),
    Pid = self(),
    {ok, #{output := {ok, #{<<"v">> := {t, Pid}}}}} = cleanrules_nif:evaluate(RuleSet, #{x => {t, Pid}, y => {t, Pid}}, ?MAX),
    {error, #{code := hit_policy_error, rules := [<<"a">>, <<"b">>]}} =
        cleanrules_nif:evaluate(RuleSet, #{x => {t, 1}, y => {t, 1.0}}, ?MAX),
    ok.

%% --- Limits ------------------------------------------------------------------

max_values_counts(_Config) ->
    RuleSet = echo(),
    Limit = fun(Data, Max) ->
        case cleanrules_nif:evaluate(RuleSet, #{x => Data}, Max) of
            {ok, _} -> ok;
            {error, #{code := limit_exceeded, name := <<"max_values">>} = E} ->
                ?assertNot(maps:is_key(row, E)),
                limit
        end
    end,
    %% A list counts one, and each of its elements one.
    ?assertEqual(ok, Limit([1, 2, 3], 4)),
    ?assertEqual(limit, Limit([1, 2, 3], 3)),
    %% A map counts one and each entry value one; keys and left-out entries
    %% do not count.
    ?assertEqual(ok, Limit(#{a => 1, b => [2], 3 => [4, 5, 6]}, 4)),
    ?assertEqual(limit, Limit(#{a => 1, b => [2]}, 3)),
    %% Opaque values and scalars count one, whatever they hold.
    ?assertEqual(ok, Limit({lists:seq(1, 100)}, 1)),
    ?assertEqual(limit, Limit(1, 0)),
    %% The same term in two places counts twice.
    Shared = lists:seq(1, 10),
    ?assertEqual(ok, Limit([Shared, Shared], 23)),
    ?assertEqual(limit, Limit([Shared, Shared], 22)),
    %% A list along the paths counts its declared length; maps along the
    %% paths do not count.
    Indexed = ruleset(<<"RULE r IF items[0] = 1">>),
    {ok, _} = cleanrules_nif:evaluate(Indexed, #{items => lists:seq(1, 1000)}, 1001),
    {error, #{code := limit_exceeded}} = cleanrules_nif:evaluate(Indexed, #{items => lists:seq(1, 1000)}, 1000),
    {error, #{code := limit_exceeded}} = cleanrules_nif:evaluate_dirty(Indexed, #{items => lists:seq(1, 1000)}, 1000),
    %% The default limit.
    #{max_values := 1000000} = cleanrules_nif:default_limits(),
    {error, #{code := limit_exceeded}} = cleanrules_nif:evaluate(RuleSet, #{x => lists:seq(1, 1000000)}, 1000000),
    {ok, _} = cleanrules_nif:evaluate(RuleSet, #{x => lists:seq(1, 999999)}, 1000000),
    ok.

max_values_in_batches(_Config) ->
    RuleSet = echo(),
    Inputs = [#{x => [1]}, #{x => [1, 2, 3]}, #{x => [1, 2, 3, 4]}],
    {ok, [_, _, _]} = cleanrules_nif:evaluate_many(RuleSet, Inputs, 5),
    %% The first input in order beyond the limit, with its row.
    {error, #{code := limit_exceeded, row := 1, name := <<"max_values">>}} =
        cleanrules_nif:evaluate_many(RuleSet, Inputs, 3),
    %% Read before anything is evaluated: it comes before an evaluation error.
    Strict = ruleset(<<"STRICT\nRULE r IF x > 1">>),
    {error, #{code := type_mismatch, row := 0}} = cleanrules_nif:evaluate_many(Strict, [#{x => <<"a">>}, #{x => [1, 2]}], 10),
    {error, #{code := limit_exceeded, row := 1}} = cleanrules_nif:evaluate_many(Strict, [#{x => <<"a">>}, #{x => [1, 2]}], 2),
    ok.

constants(_Config) ->
    Constant = fun(Term) -> cleanrules_nif:value_to_json(Term, constant, ?MAX) end,
    {ok, <<"{\"a\":[1,2.5,{\"$decimal\":\"0.10\"}],\"b\":null}">>} =
        Constant(#{a => [1, 2.5, {cleanrules_decimal, <<"0.10">>}], b => undefined}),
    {ok, <<"{\"$date\":\"2026-11-12\"}">>} = Constant({2026, 11, 12}),
    %% Opaque values and nulls with a reason are an expression_error, which
    %% names where they are.
    [
        ?assertMatch({error, #{code := expression_error, message := _}}, Constant(T))
     || T <- [self(), 1 bsl 64, {cleanrules_decimal, <<"1e40">>}, [1, {a}], #{a => [<<255>>]}]
    ],
    {error, #{message := Message}} = Constant(#{a => [1, <<255>>]}),
    ?assertMatch({match, _}, re:run(Message, <<"\\[\"a\",1\\]">>)),
    %% Beyond max_values: limit_exceeded.
    {error, #{code := limit_exceeded, name := <<"max_values">>}} = cleanrules_nif:value_to_json([1, 2], constant, 2),
    {ok, _} = cleanrules_nif:value_to_json([1, 2], constant, 3),
    ok.

values_from_json(_Config) ->
    From = fun(Json) ->
        {ok, Term} = cleanrules_nif:value_from_json(Json),
        Term
    end,
    ?assertEqual({cleanrules_decimal, <<"1.5">>}, From(<<"{\"$decimal\":\"1.5\"}">>)),
    ?assertEqual({cleanrules_float, <<"-inf">>}, From(<<"{\"$float\":\"-inf\"}">>)),
    ?assertEqual(#{<<"a">> => [1, null, {2026, 11, 12}]}, From(<<"{\"a\":[1,null,{\"$date\":\"2026-11-12\"}]}">>)),
    ?assertEqual(
        {cleanrules_datetime, {{2026, 11, 12}, {9, 30, 0, 500}}, 3600, <<"Europe/Madrid">>},
        From(<<"{\"$datetime\":\"2026-11-12T09:30:00.0005+01:00[Europe/Madrid]\"}">>)
    ),
    ?assertEqual({{2026, 11, 12}, {9, 30, 0}}, From(<<"{\"$datetime\":\"2026-11-12T09:30:00\"}">>)),
    ?assertEqual({cleanrules_datetime, {{2026, 11, 12}, {9, 30, 0}}, 0, undefined}, From(<<"{\"$datetime\":\"2026-11-12T09:30:00Z\"}">>)),
    ?assertEqual(9007199254740993, From(<<"{\"$int\":\"9007199254740993\"}">>)),
    ?assertEqual(#{<<"$x">> => 1}, From(<<"{\"$map\":{\"$x\":1}}">>)),
    %% Everything reads back as the same value.
    [
        ?assertEqual(Json, json(From(Json)))
     || Json <- [
            <<"[1,\"a\",true,null,{\"$decimal\":\"-0.50\"}]">>,
            <<"{\"$datetime\":\"2026-07-01T12:00:00.000007+02:00[Europe/Madrid]\"}">>,
            <<"{\"$float\":\"inf\"}">>
        ]
    ],
    {error, #{code := format_error}} = cleanrules_nif:value_from_json(<<"[1,">>),
    ok.

%% --- Resources ---------------------------------------------------------------

version_and_limits(_Config) ->
    _ = application:load(cleanrules),
    {ok, Vsn} = application:get_key(cleanrules, vsn),
    ?assertEqual(list_to_binary(Vsn), cleanrules_nif:version()),
    ?assertEqual(1, cleanrules_nif:format_version()),
    #{max_size := _, max_nesting := _, max_depth := 256, max_table_size := _, max_yaml_size := _, max_text_size := _} =
        cleanrules_nif:default_limits(),
    ok.

resource_shared_by_processes(_Config) ->
    RuleSet = ruleset(<<"RULE big IF x > 10 THEN size = \"big\"\nRULE small IF x <= 10 THEN size = \"small\"">>),
    ?assert(is_reference(RuleSet)),
    Parent = self(),
    Pids = [
        spawn_link(fun() ->
            Results = [cleanrules_nif:evaluate(RuleSet, #{x => N + I}, ?MAX) || I <- lists:seq(1, 200)],
            Parent ! {self(), Results}
        end)
     || N <- lists:seq(1, 50)
    ],
    [
        receive
            {Pid, Results} ->
                [{ok, #{output := {ok, #{<<"size">> := _}}}} = R || R <- Results]
        after 60000 -> ct:fail(timeout)
        end
     || Pid <- Pids
    ],
    ok.

resource_outlives_its_creator(_Config) ->
    Parent = self(),
    {Creator, Monitor} = spawn_monitor(fun() ->
        Parent ! {ruleset, ruleset(<<"RULE r IF x = 1 THEN v = \"one\"">>)}
    end),
    RuleSet =
        receive
            {ruleset, R} -> R
        after 10000 -> ct:fail(timeout)
        end,
    receive
        {'DOWN', Monitor, process, Creator, normal} -> ok
    after 10000 -> ct:fail(timeout)
    end,
    erlang:garbage_collect(),
    {ok, #{output := {ok, #{<<"v">> := <<"one">>}}}} = cleanrules_nif:evaluate(RuleSet, #{x => 1}, ?MAX),
    %% A copy of the term in an ETS table is the same resource.
    Table = ets:new(rulesets, [set, public]),
    true = ets:insert(Table, {r, RuleSet}),
    [{r, Copy}] = ets:lookup(Table, r),
    {ok, #{selected := [<<"r">>]}} = cleanrules_nif:evaluate(Copy, #{x => 1}, ?MAX),
    true = ets:delete(Table),
    ok.

ruleset_documents(_Config) ->
    Text = <<"HIT POLICY collect\nAGGREGATE sum\nRULE a PRIORITY 2 IF x > 1 THEN n = 1\nRULE b IF x > 2 THEN n = 2">>,
    RuleSet = ruleset(Text),
    Info = cleanrules_nif:ruleset_info(RuleSet),
    ?assertEqual(
        #{hit_policy => collect, aggregate => sum, logic => boolean, strict => false, rules => [<<"a">>, <<"b">>], field_paths => [[<<"x">>]]},
        Info
    ),
    {ok, Json} = cleanrules_nif:ruleset_write(RuleSet, json),
    {ok, Again} = cleanrules_nif:ruleset_read(Json, json, #{}),
    ?assertEqual(Info, cleanrules_nif:ruleset_info(Again)),
    {ok, Yaml} = cleanrules_nif:ruleset_write(RuleSet, yaml),
    {ok, FromYaml} = cleanrules_nif:ruleset_read(Yaml, yaml, #{}),
    ?assertEqual(Info, cleanrules_nif:ruleset_info(FromYaml)),
    {ok, Printed} = cleanrules_nif:ruleset_write(RuleSet, text),
    ?assertMatch({match, _}, re:run(Printed, <<"AGGREGATE sum">>)),
    {ok, Summary} = cleanrules_nif:ruleset_summary(RuleSet, text),
    ?assert(is_binary(Summary)),
    {ok, SummaryJson} = cleanrules_nif:ruleset_summary(RuleSet, json),
    ?assertMatch({match, _}, re:run(SummaryJson, <<"\"summary\"">>)),
    %% A rule document is a rule set of its one rule.
    {ok, One} = cleanrules_nif:ruleset_read(<<"{\"version\":1,\"rule\":{\"name\":\"r\",\"when\":{\"op\":\"is_null\",\"operand\":{\"op\":\"field\",\"path\":[\"x\"]}}}}">>, json, #{}),
    #{rules := [<<"r">>], hit_policy := first, aggregate := null} = cleanrules_nif:ruleset_info(One),
    %% A decision table.
    Csv = <<"x,out\n> 1,\"\"\"big\"\"\"\n">>,
    case cleanrules_nif:ruleset_read(Csv, csv, #{}) of
        {ok, Table} -> #{rules := [_]} = cleanrules_nif:ruleset_info(Table);
        {error, #{code := Code}} -> ct:fail({csv, Code})
    end,
    ok.

%% --- Schedulers --------------------------------------------------------------

normal_scheduler_budget(_Config) ->
    %% A fresh rule set for each call on the normal scheduler: one that took
    %% longer than a millisecond (a debug build, a loaded machine) would send
    %% the next calls to a dirty scheduler (adaptive_dirty).
    RuleSet = echo(),
    %% Small inputs run on the normal scheduler; large ones are moved.
    {ok, _} = cleanrules_nif:evaluate_normal(echo(), #{x => lists:seq(1, 1000)}, ?MAX),
    dirty = cleanrules_nif:evaluate_normal(RuleSet, #{x => lists:seq(1, 100000)}, ?MAX),
    dirty = cleanrules_nif:evaluate_normal(RuleSet, #{x => binary:copy(<<"a">>, 10000000)}, ?MAX),
    dirty = cleanrules_nif:evaluate_normal(RuleSet, #{x => maps:from_list([{I, I} || I <- lists:seq(1, 100000)])}, ?MAX),
    %% evaluate/3 gives the same decision either way.
    {ok, #{output := {ok, #{<<"v">> := L}}}} = cleanrules_nif:evaluate(RuleSet, #{x => lists:seq(1, 100000)}, ?MAX),
    ?assertEqual(100000, length(L)),
    %% max_values still applies first when it is lower than the budget.
    {error, #{code := limit_exceeded}} = cleanrules_nif:evaluate_normal(echo(), #{x => lists:seq(1, 100)}, 10),
    %% A large rule set goes to a dirty scheduler at once.
    Rules = [io_lib:format("RULE r~b IF x = ~b THEN v = ~b~n", [I, I, I]) || I <- lists:seq(1, 2000)],
    Large = ruleset(iolist_to_binary(["HIT POLICY first\n" | Rules])),
    dirty = cleanrules_nif:evaluate_normal(Large, #{x => 1}, ?MAX),
    {ok, #{output := {ok, #{<<"v">> := 1999}}}} = cleanrules_nif:evaluate(Large, #{x => 1999}, ?MAX),
    ok.

dirty_calls(_Config) ->
    %% Batches and explanations run on dirty CPU schedulers: a long batch in
    %% one process leaves the normal schedulers free for others.
    ?assert(erlang:system_info(dirty_cpu_schedulers) >= 1),
    RuleSet = echo(),
    Inputs = [#{x => lists:seq(1, 20000)} || _ <- lists:seq(1, 100)],
    Parent = self(),
    Worker = spawn_link(fun() -> Parent ! {self(), cleanrules_nif:evaluate_many(RuleSet, Inputs, ?MAX)} end),
    Pings = [element(1, timer:tc(fun() -> erlang:yield() end)) || _ <- lists:seq(1, 100)],
    receive
        {Worker, {ok, Decisions}} -> ?assertEqual(100, length(Decisions))
    after 120000 -> ct:fail(timeout)
    end,
    ct:log("yield times while a batch ran (us): max ~b", [lists:max(Pings)]),
    {ok, Text} = cleanrules_nif:explain(RuleSet, #{x => 1}, ?MAX, text),
    ?assert(is_binary(Text)),
    ok.

normal_call_time(_Config) ->
    %% The largest input the normal scheduler takes stays near a millisecond
    %% (README.md, Schedulers): measured, and logged; checked loosely so that
    %% slow machines pass.
    %% A fresh rule set for each call (normal_scheduler_budget).
    Sizes = [100, 1000, 2500, 4900],
    Time = fun(Size) ->
        RuleSet = echo(),
        Data = #{x => lists:seq(1, Size)},
        element(1, timer:tc(fun() -> {ok, _} = cleanrules_nif:evaluate_normal(RuleSet, Data, ?MAX) end))
    end,
    Times = [{Size, median([Time(Size) || _ <- lists:seq(1, 51)])} || Size <- Sizes],
    ct:log("evaluate_normal median times (us) by list length: ~p", [Times]),
    {_, Largest} = lists:last(Times),
    ?assert(Largest < 20000),
    ok.

median(Values) ->
    lists:nth(length(Values) div 2 + 1, lists:sort(Values)).

%% --- Errors ------------------------------------------------------------------

panics(_Config) ->
    {error, #{code := internal_error, message := Message}} = cleanrules_nif:test_panic(),
    ?assertMatch({match, _}, re:run(Message, <<"please report it">>)),
    %% The library still works after a panic.
    ?assertEqual(1, echo(1)),
    {error, #{code := internal_error}} = cleanrules_nif:test_panic(),
    ok.

core_errors(_Config) ->
    {error, #{code := format_error, line := 1, column := 2, message := _}} = cleanrules_nif:ruleset_read(<<"{">>, json, #{}),
    {error, #{code := unsupported_version, pointer := <<"/version">>}} =
        cleanrules_nif:ruleset_read(<<"{\"version\":9,\"rule\":{}}">>, json, #{}),
    {error, #{code := rule_syntax_error, line := 1, column := _}} = cleanrules_nif:ruleset_read(<<"RULE r IF >">>, text, #{}),
    {error, #{code := limit_exceeded}} = cleanrules_nif:ruleset_read(<<"RULE r IF x > 1">>, text, #{max_size => 5}),
    {error, #{code := format_error}} = cleanrules_nif:ruleset_read(<<"{\"version\":1,\"condition\":{\"op\":\"is_null\",\"operand\":{\"op\":\"field\",\"path\":[\"x\"]}}}">>, json, #{}),
    {error, #{code := format_error}} = cleanrules_nif:ruleset_read(<<255>>, text, #{}),
    %% Strict mode: the error names the rule, the path and the types.
    Strict = ruleset(<<"STRICT\nRULE r IF x > 1">>),
    {error, #{code := type_mismatch, rule := <<"r">>, path := [<<"x">>], types := [<<"string">>, <<"number">>], message := _}} =
        cleanrules_nif:evaluate(Strict, #{x => <<"a">>}, ?MAX),
    {error, #{code := missing_field, rule := <<"r">>, path := [<<"x">>]}} = cleanrules_nif:evaluate(Strict, #{}, ?MAX),
    {error, #{code := type_mismatch}} = cleanrules_nif:explain(Strict, #{x => <<"a">>}, ?MAX, term),
    %% A batch is a proper list.
    [{error, #{code := evaluation_error}} = cleanrules_nif:evaluate_many(Strict, Bad, ?MAX) || Bad <- [#{}, <<"x">>, [#{} | #{}]]],
    {error, #{code := type_mismatch, row := 1}} = cleanrules_nif:evaluate_many(Strict, [#{x => 2}, #{x => <<"a">>}], ?MAX),
    %% A multi-hit decision has no single output.
    Collect = ruleset(<<"HIT POLICY rule_order\nRULE a IF x > 1\nRULE b IF x > 2">>),
    {ok, #{output := {error, #{code := hit_policy_error}}, selected := [<<"a">>, <<"b">>]}} =
        cleanrules_nif:evaluate(Collect, #{x => 3}, ?MAX),
    %% Writing a summary as CSV is unsupported.
    {error, #{code := unsupported}} = cleanrules_nif:ruleset_summary(Collect, csv),
    ok.

bad_arguments(_Config) ->
    RuleSet = echo(),
    ?assertError(badarg, cleanrules_nif:evaluate(not_a_ruleset, #{}, 1)),
    ?assertError(badarg, cleanrules_nif:evaluate(make_ref(), #{}, 1)),
    ?assertError(badarg, cleanrules_nif:evaluate(RuleSet, #{}, -1)),
    ?assertError(badarg, cleanrules_nif:evaluate(RuleSet, #{}, 1.0)),
    ?assertError(badarg, cleanrules_nif:evaluate_many(RuleSet, [], max)),
    ?assertError(badarg, cleanrules_nif:ruleset_read(<<"RULE r IF x">>, xml, #{})),
    ?assertError(badarg, cleanrules_nif:ruleset_read(<<"RULE r IF x">>, "text", #{})),
    ?assertError(badarg, cleanrules_nif:ruleset_read("RULE r IF x", text, #{})),
    ?assertError(badarg, cleanrules_nif:ruleset_read(<<"RULE r IF x">>, text, #{max_size => -1})),
    ?assertError(badarg, cleanrules_nif:ruleset_read(<<"RULE r IF x">>, text, #{max_sizes => 1})),
    ?assertError(badarg, cleanrules_nif:ruleset_read(<<"RULE r IF x">>, text, #{<<"max_size">> => 1})),
    ?assertError(badarg, cleanrules_nif:ruleset_read(<<"RULE r IF x">>, text, [])),
    ?assertError(badarg, cleanrules_nif:explain(RuleSet, #{}, 1, html)),
    ?assertError(badarg, cleanrules_nif:value_to_json(1, other, 1)),
    ?assertError(badarg, cleanrules_nif:value_from_json("[]")),
    ?assertError(badarg, cleanrules_nif:ruleset_write(RuleSet, <<"json">>)),
    %% Every limit is accepted.
    {ok, _} = cleanrules_nif:ruleset_read(<<"RULE r IF x">>, text, cleanrules_nif:default_limits()),
    ok.

%% --- Results -----------------------------------------------------------------

decisions(_Config) ->
    RuleSet = ruleset(<<"HIT POLICY collect\nAGGREGATE sum\nRULE a IF x > 1 THEN n = 1\nRULE b IF x > 2 THEN n = 2\nRULE c IF y > 0 THEN n = 4">>),
    {ok, Decision} = cleanrules_nif:evaluate(RuleSet, #{x => 2}, ?MAX),
    ?assertEqual(
        #{
            results => [
                #{rule => <<"a">>, fired => true, truth => true, output => #{<<"n">> => 1}},
                #{rule => <<"b">>, fired => false, truth => false},
                #{rule => <<"c">>, fired => false, truth => unknown}
            ],
            selected => [<<"a">>],
            output => {ok, #{<<"n">> => 1}},
            aggregated => #{<<"n">> => 1}
        },
        Decision
    ),
    {ok, #{aggregated := null, output := {ok, null}, selected := []}} = cleanrules_nif:evaluate(RuleSet, #{x => 0}, ?MAX),
    {ok, [#{aggregated := #{<<"n">> := 3}}, #{selected := []}]} = cleanrules_nif:evaluate_many(RuleSet, [#{x => 3}, #{}], ?MAX),
    {ok, []} = cleanrules_nif:evaluate_many(RuleSet, [], ?MAX),
    %% Same inputs, same decision.
    ?assertEqual(cleanrules_nif:evaluate_dirty(RuleSet, #{x => 2}, ?MAX), {ok, Decision}),
    ok.

explanations(_Config) ->
    RuleSet = ruleset(<<"RULE big IF x > 10 THEN size = \"big\"">>),
    {ok, Text} = cleanrules_nif:explain(RuleSet, #{x => 8}, ?MAX, text),
    ?assertMatch({match, _}, re:run(Text, <<"big">>)),
    {ok, Json} = cleanrules_nif:explain(RuleSet, #{x => 8}, ?MAX, json),
    ?assertMatch({match, _}, re:run(Json, <<"\"counterfactual\"">>)),
    {ok, Yaml} = cleanrules_nif:explain(RuleSet, #{x => 8}, ?MAX, yaml),
    ?assert(is_binary(Yaml)),
    {ok, #{<<"rules">> := [Rule], <<"selected">> := []}} = cleanrules_nif:explain(RuleSet, #{x => 8}, ?MAX, term),
    #{<<"rule">> := <<"big">>, <<"requirement">> := #{<<"counterfactual">> := #{<<"gap">> := 2, <<"target">> := 10}}} = Rule,
    %% Opaque values in an explanation are the terms read.
    Opaque = ruleset(<<"RULE r IF x IS NULL">>),
    Pid = self(),
    {ok, #{<<"rules">> := [#{<<"requirement">> := #{<<"values">> := [#{<<"value">> := Pid}]}}]}} =
        cleanrules_nif:explain(Opaque, #{x => Pid}, ?MAX, term),
    {error, #{code := limit_exceeded}} = cleanrules_nif:explain(RuleSet, #{x => [1, 2]}, 2, term),
    ok.

summaries(_Config) ->
    RuleSet = ruleset(<<"RULE a PRIORITY 3 TAGS gold IF x > 1 THEN n = y\nRULE b IF z IS NULL">>),
    {ok, Summary} = cleanrules_nif:ruleset_summary(RuleSet, term),
    #{
        <<"hit_policy">> := <<"first">>,
        <<"rules">> := [#{<<"name">> := <<"a">>, <<"priority">> := 3, <<"outputs">> := [<<"n">>]}, #{<<"name">> := <<"b">>}],
        <<"fields">> := [#{<<"path">> := [<<"x">>]}, #{<<"path">> := [<<"y">>]}, #{<<"path">> := [<<"z">>]}],
        <<"tags">> := [#{<<"tag">> := <<"gold">>, <<"rules">> := [<<"a">>]}]
    } = Summary,
    %% Priorities beyond 2^53 are integers too.
    Big = ruleset(<<"RULE a PRIORITY 9007199254740993 IF x > 1">>),
    {ok, #{<<"rules">> := [#{<<"priority">> := 9007199254740993}]}} = cleanrules_nif:ruleset_summary(Big, term),
    ok.

decision_documents(_Config) ->
    RuleSet = ruleset(<<"RULE big IF x > 10 THEN size = \"big\", seen = x\nRULE small IF x <= 10 THEN size = \"small\", seen = x">>),
    {ok, Decision} = cleanrules_nif:evaluate(RuleSet, #{x => 12}, ?MAX),
    {ok, Json} = cleanrules_nif:decision_write(RuleSet, #{x => 12}, ?MAX, json),
    ?assertMatch({match, _}, re:run(Json, <<"\"decision\"">>)),
    %% Read back with its rule set, it is the same decision.
    {ok, Decision} = cleanrules_nif:decision_read(RuleSet, Json, json, #{}),
    {ok, Yaml} = cleanrules_nif:decision_write(RuleSet, #{x => 12}, ?MAX, yaml),
    {ok, Decision} = cleanrules_nif:decision_read(RuleSet, Yaml, yaml, #{}),
    %% Opaque values are written by type and come back as null.
    {ok, OpaqueJson} = cleanrules_nif:decision_write(RuleSet, #{x => self()}, ?MAX, json),
    ?assertMatch({match, _}, re:run(OpaqueJson, <<"\\$opaque">>)),
    {ok, #{results := [#{fired := false}, #{fired := false}]}} = cleanrules_nif:decision_read(RuleSet, OpaqueJson, json, #{}),
    %% A decision of another rule set is refused.
    Other = ruleset(<<"RULE other IF y > 1">>),
    {error, #{code := decision_mismatch}} = cleanrules_nif:decision_read(Other, Json, json, #{}),
    {error, #{code := unsupported}} = cleanrules_nif:decision_write(RuleSet, #{x => 1}, ?MAX, text),
    {error, #{code := limit_exceeded}} = cleanrules_nif:decision_write(RuleSet, #{x => [1, 2]}, 2, json),
    {error, #{code := format_error}} = cleanrules_nif:decision_read(RuleSet, <<"{">>, json, #{}),
    ok.

error_parents(_Config) ->
    ?assertEqual(format_error, cleanrules_nif:error_parent(limit_exceeded)),
    ?assertEqual(format_error, cleanrules_nif:error_parent(unsupported_version)),
    ?assertEqual(evaluation_error, cleanrules_nif:error_parent(missing_field)),
    ?assertEqual(evaluation_error, cleanrules_nif:error_parent(hit_policy_error)),
    [?assertEqual(undefined, cleanrules_nif:error_parent(C)) || C <- [format_error, evaluation_error, unsupported, internal_error]],
    ?assertError(badarg, cleanrules_nif:error_parent(not_a_code)),
    ?assertError(badarg, cleanrules_nif:error_parent(<<"format_error">>)),
    ok.

error_nodes(_Config) ->
    Strict = ruleset(<<"STRICT\nRULE r IF x > 1.5">>),
    {error, #{code := type_mismatch, node := Node}} = cleanrules_nif:evaluate(Strict, #{x => <<"a">>}, ?MAX),
    %% The node's JSON form as a term.
    #{<<"op">> := <<"gt">>, <<"left">> := #{<<"op">> := <<"field">>, <<"path">> := [<<"x">>]}, <<"right">> := #{<<"value">> := _}} = Node,
    ok.

adaptive_dirty(_Config) ->
    %% A small rule set whose calls cost more than the budget expects: many
    %% scans of a long text. Once a call on the normal scheduler takes longer
    %% than a millisecond, the rule set's later calls go to a dirty one.
    Rules = [io_lib:format("RULE r~b IF x CONTAINS \"needle~b\"~n", [I, I]) || I <- lists:seq(1, 400)],
    RuleSet = ruleset(iolist_to_binary(["HIT POLICY collect\n" | Rules])),
    Text = binary:copy(<<"haystack ">>, 15000),
    {ok, #{selected := []}} = cleanrules_nif:evaluate_normal(RuleSet, #{x => Text}, ?MAX),
    dirty = cleanrules_nif:evaluate_normal(RuleSet, #{x => <<"short">>}, ?MAX),
    {ok, #{selected := [<<"r7">>]}} = cleanrules_nif:evaluate(RuleSet, #{x => <<"a needle7">>}, ?MAX),
    %% Another rule set is not affected.
    {ok, _} = cleanrules_nif:evaluate_normal(echo(), #{x => 1}, ?MAX),
    ok.

budget_before_work(_Config) ->
    RuleSet = echo(),
    %% A long list is walked only as far as the budget, not to its end.
    Long = lists:seq(1, 5000000),
    {Time, dirty} = timer:tc(fun() -> cleanrules_nif:evaluate_normal(RuleSet, #{x => Long}, 10000000) end),
    ct:log("a list of 5,000,000 sent to a dirty scheduler in ~b us", [Time]),
    ?assert(Time < 20000),
    %% A %Decimal{} with parts beyond 64 bits is read on a dirty scheduler.
    Big = #{'__struct__' => 'Elixir.Decimal', sign => 1, coef => 1 bsl 70, exp => -10},
    dirty = cleanrules_nif:evaluate_normal(RuleSet, #{x => Big}, ?MAX),
    {ok, #{output := {ok, #{<<"v">> := {cleanrules_decimal, _}}}}} = cleanrules_nif:evaluate(RuleSet, #{x => Big}, ?MAX),
    %% Map keys are charged before they are copied.
    Keys = maps:from_list([{binary:copy(<<"k">>, 1000 + I), I} || I <- lists:seq(1, 300)]),
    dirty = cleanrules_nif:evaluate_normal(RuleSet, #{x => Keys}, ?MAX),
    ok.

safe_depth_limits(_Config) ->
    Text = <<"RULE r IF x > 1">>,
    {ok, _} = cleanrules_nif:ruleset_read(Text, text, #{max_depth => 2048, max_nesting => 2048}),
    ?assertError(badarg, cleanrules_nif:ruleset_read(Text, text, #{max_depth => 2049})),
    ?assertError(badarg, cleanrules_nif:ruleset_read(Text, text, #{max_nesting => 2049})),
    ?assertError(badarg, cleanrules_nif:decision_read(echo(), <<"{}">>, json, #{max_nesting => 100000})),
    ok.

long_path_regression(_Config) ->
    %% A path has at most max_path_segments segments, 256 by default
    %% (spec/json-format.md, Errors and limits).
    Nest = fun Nest(0, Inner) -> Inner; Nest(N, Inner) -> Nest(N - 1, [Inner]) end,
    AtLimit = iolist_to_binary(["x", lists:duplicate(255, "[0]")]),
    {ok, #{selected := [<<"r">>]}} =
        cleanrules_nif:evaluate_dirty(ruleset(<<"RULE r IF ", AtLimit/binary, " = 1">>), #{x => Nest(255, 1)}, ?MAX),
    {error, #{code := limit_exceeded, name := <<"max_path_segments">>, line := 1, column := 777}} =
        cleanrules_nif:ruleset_read(<<"RULE r IF ", AtLimit/binary, "[0] = 1">>, text, #{}),
    {error, #{code := limit_exceeded, name := <<"max_path_segments">>}} =
        cleanrules_nif:ruleset_read(<<"RULE r IF a.b.c">>, text, #{max_path_segments => 2}),
    #{max_path_segments := 256} = cleanrules_nif:default_limits(),
    %% Raised, a path of 100,000 segments through data as deep: its sparse
    %% value tree is dropped with an explicit stack (it crashed the VM before).
    Path = iolist_to_binary(["x", lists:duplicate(100000, "[0]")]),
    {ok, Long} = cleanrules_nif:ruleset_read(<<"RULE r IF ", Path/binary, " = 1">>, text, #{max_path_segments => 100001}),
    Data = #{x => Nest(100000, 1)},
    {ok, #{selected := [<<"r">>]}} = cleanrules_nif:evaluate_dirty(Long, Data, ?MAX),
    {ok, #{selected := [<<"r">>]}} = cleanrules_nif:evaluate(Long, Data, ?MAX),
    {ok, [_, _]} = cleanrules_nif:evaluate_many(Long, [Data, Data], ?MAX),
    {error, #{code := limit_exceeded, row := 1}} = cleanrules_nif:evaluate_many(Long, [Data, #{x => lists:seq(1, 200000)}], 150000),
    erlang:garbage_collect(),
    ok.
