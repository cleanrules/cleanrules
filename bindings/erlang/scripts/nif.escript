#!/usr/bin/env escript
%%! -noshell
%% Puts the NIF library of cleanrules in priv/ (rebar3 runs this before it
%% compiles, rebar.config's pre_hooks):
%%
%% - build: in a checkout of the repository (native/cleanrules_nif exists),
%%   with cargo, and copied to priv/cleanrules_nif.so (.dll on Windows);
%% - download: in a package, the prebuilt library of this platform, from the
%%   release of the application's version, checked against its SHA-256 in
%%   nif-checksums.config before it is used, and kept in a cache;
%% - skip: use the library already in priv/.
%%
%% Environment:
%%   CLEANRULES_NIF           build | download | skip (default: build when
%%                            native/cleanrules_nif exists, else download)
%%   CLEANRULES_NIF_PROFILE   cargo profile to build with (default: release)
%%   CLEANRULES_NIF_BASE_URL  where the prebuilt libraries are, https:// or
%%                            file:// (default: the GitHub release v<vsn>)
%%   CLEANRULES_NIF_TARGET    the Rust target triple to download (default:
%%                            this platform's)
%%   CLEANRULES_NIF_CACHE     cache directory (default: the user cache)
%%   HTTPS_PROXY, NO_PROXY    a proxy for https:// as host:port, and the hosts
%%                            to reach without it (comma-separated); a proxy
%%                            that needs credentials is not supported: use a
%%                            file:// mirror instead
-mode(compile).

-define(NIF_VERSION, "2.17").
-define(RELEASES, "https://github.com/cleanrules/cleanrules/releases/download/v").

main(_) ->
    Root = filename:dirname(filename:dirname(filename:absname(escript:script_name()))),
    Priv = filename:join(Root, "priv"),
    ok = filelib:ensure_dir(filename:join(Priv, "x")),
    try
        case mode(Root) of
            "build" -> build(Root, Priv);
            "download" -> download(Root, Priv);
            "skip" -> ok;
            Other -> fail("CLEANRULES_NIF is build, download or skip, not ~ts", [Other])
        end
    catch
        throw:{cleanrules_nif, Message} ->
            io:format(standard_error, "cleanrules: ~ts~n", [Message]),
            halt(1);
        Class:Reason ->
            io:format(standard_error, "cleanrules: cannot put the NIF in priv/: ~p:~p~n", [Class, Reason]),
            halt(1)
    end.

fail(Format, Args) ->
    throw({cleanrules_nif, io_lib:format(Format, Args)}).

mode(Root) ->
    case os:getenv("CLEANRULES_NIF") of
        false ->
            Crate = filename:join([Root, "native", "cleanrules_nif", "Cargo.toml"]),
            case filelib:is_regular(Crate) of
                true -> "build";
                false -> "download"
            end;
        Mode ->
            Mode
    end.

%% The library's file name in priv/, which erlang:load_nif/2 completes.
library() ->
    case os:type() of
        {win32, _} -> "cleanrules_nif.dll";
        _ -> "cleanrules_nif.so"
    end.

%% --- Build ------------------------------------------------------------------

build(Root, Priv) ->
    Crate = filename:join([Root, "native", "cleanrules_nif"]),
    Cargo =
        case os:find_executable("cargo") of
            false -> fail("cargo is needed to build the NIF from source (https://rustup.rs)", []);
            Path -> Path
        end,
    Profile =
        case os:getenv("CLEANRULES_NIF_PROFILE") of
            false -> "release";
            "" -> "release";
            Name -> Name
        end,
    Args = ["build", "-p", "cleanrules-erlang", "--lib", "--profile", Profile],
    ok = run(Cargo, Args, Crate),
    Dir =
        case Profile of
            "dev" -> "debug";
            "test" -> "debug";
            "bench" -> "release";
            _ -> Profile
        end,
    Built = filename:join([target_dir(Cargo, Crate), Dir, built_name()]),
    copy_if_changed(Built, filename:join(Priv, library())).

%% The name cargo gives the cdylib on this platform.
built_name() ->
    case os:type() of
        {win32, _} -> "cleanrules_nif.dll";
        {unix, darwin} -> "libcleanrules_nif.dylib";
        _ -> "libcleanrules_nif.so"
    end.

run(Executable, Args, Dir) ->
    Port = open_port(
        {spawn_executable, Executable},
        [{args, Args}, {cd, Dir}, exit_status, stderr_to_stdout, binary, stream]
    ),
    relay(Port, Executable).

relay(Port, Executable) ->
    receive
        {Port, {data, Data}} ->
            io:put_chars(standard_error, Data),
            relay(Port, Executable);
        {Port, {exit_status, 0}} ->
            ok;
        {Port, {exit_status, Status}} ->
            fail("~ts exited with status ~b", [Executable, Status])
    end.

%% cargo's target directory: CARGO_TARGET_DIR, or the workspace's.
target_dir(Cargo, Crate) ->
    case os:getenv("CARGO_TARGET_DIR") of
        Dir when Dir =/= false, Dir =/= "" ->
            filename:absname(Dir);
        _ ->
            Metadata = capture(Cargo, ["metadata", "--format-version", "1", "--no-deps"], Crate),
            case re:run(Metadata, <<"\"target_directory\":\"((?:[^\"\\\\]|\\\\.)*)\"">>, [{capture, [1], binary}]) of
                {match, [Escaped]} -> unicode:characters_to_list(json_unescape(Escaped));
                nomatch -> fail("cargo metadata does not name the target directory", [])
            end
    end.

%% The escapes cargo writes in a path: \\ and \".
json_unescape(<<"\\", C, Rest/binary>>) -> <<C, (json_unescape(Rest))/binary>>;
json_unescape(<<C, Rest/binary>>) -> <<C, (json_unescape(Rest))/binary>>;
json_unescape(<<>>) -> <<>>.

capture(Executable, Args, Dir) ->
    Port = open_port({spawn_executable, Executable}, [{args, Args}, {cd, Dir}, exit_status, binary, stream]),
    collect(Port, Executable, []).

collect(Port, Executable, Acc) ->
    receive
        {Port, {data, Data}} -> collect(Port, Executable, [Acc, Data]);
        {Port, {exit_status, 0}} -> iolist_to_binary(Acc);
        {Port, {exit_status, Status}} -> fail("~ts exited with status ~b", [Executable, Status])
    end.

copy_if_changed(From, To) ->
    New =
        case file:read_file(From) of
            {ok, Bytes} -> Bytes;
            {error, Reason} -> fail("cannot read ~ts: ~p", [From, Reason])
        end,
    case file:read_file(To) of
        {ok, New} -> ok;
        _ -> write(To, New)
    end.

write(Path, Bytes) ->
    Temporary = Path ++ ".tmp",
    case file:write_file(Temporary, Bytes) of
        ok -> ok;
        {error, Reason} -> fail("cannot write ~ts: ~p", [Temporary, Reason])
    end,
    case file:rename(Temporary, Path) of
        ok -> ok;
        {error, Why} -> fail("cannot write ~ts: ~p", [Path, Why])
    end.

%% --- Download ---------------------------------------------------------------

download(Root, Priv) ->
    Version = app_version(Root),
    check_nif_version(),
    Target =
        case os:getenv("CLEANRULES_NIF_TARGET") of
            false -> target();
            "" -> target();
            Given -> Given
        end,
    Name = artifact(Version, Target),
    Expected = expected_checksum(Root, Name, Target),
    Bytes = cached(Name, Expected, fun() -> fetch(base_url(Version) ++ "/" ++ Name) end),
    Member = library(),
    case erl_tar:extract({binary, Bytes}, [compressed, memory, {files, [Member]}]) of
        {ok, [{Member, Library}]} -> copy_bytes_if_changed(Library, filename:join(Priv, Member));
        {ok, _} -> fail("~ts does not hold ~ts", [Name, Member]);
        {error, Reason} -> fail("cannot unpack ~ts: ~p", [Name, Reason])
    end.

copy_bytes_if_changed(Bytes, To) ->
    case file:read_file(To) of
        {ok, Bytes} -> ok;
        _ -> write(To, Bytes)
    end.

%% The name of a prebuilt library's archive, as erlang-nifs.yml makes it.
artifact(Version, Target) ->
    Ext =
        case lists:suffix("windows-msvc", Target) of
            true -> "dll";
            false -> "so"
        end,
    lists:flatten(io_lib:format("libcleanrules_nif-v~ts-nif-~ts-~ts.~ts.tar.gz", [Version, ?NIF_VERSION, Target, Ext])).

app_version(Root) ->
    File = filename:join([Root, "src", "cleanrules.app.src"]),
    case file:consult(File) of
        {ok, [{application, cleanrules, Properties}]} ->
            proplists:get_value(vsn, Properties);
        _ ->
            fail("cannot read the version in ~ts", [File])
    end.

check_nif_version() ->
    [Major, Minor] = [list_to_integer(P) || P <- string:split(erlang:system_info(nif_version), ".")],
    [WantMajor, WantMinor] = [list_to_integer(P) || P <- string:split(?NIF_VERSION, ".")],
    case {Major, Minor} >= {WantMajor, WantMinor} of
        true -> ok;
        false -> fail("the prebuilt NIF needs NIF version ~ts (Erlang/OTP 26) or later", [?NIF_VERSION])
    end.

%% The Rust target triple of this VM. Only 64-bit VMs have prebuilt NIFs.
target() ->
    Architecture = erlang:system_info(system_architecture),
    case erlang:system_info(wordsize) of
        8 -> ok;
        _ -> fail("no prebuilt NIF for a 32-bit Erlang VM (~ts); build it with CLEANRULES_NIF=build", [Architecture])
    end,
    Cpu =
        case os:type() of
            {win32, _} ->
                %% PROCESSOR_ARCHITECTURE is this process's architecture, which
                %% the library must match: AMD64 for x64 Erlang emulated on
                %% arm64 Windows (PROCESSOR_ARCHITEW6432, the machine's, is not
                %% what counts).
                case os:getenv("PROCESSOR_ARCHITECTURE", "") of
                    "ARM64" -> "aarch64";
                    "AMD64" -> "x86_64";
                    Other -> fail("no prebuilt NIF for Windows on ~ts", [Other])
                end;
            _ ->
                case hd(string:split(Architecture, "-")) of
                    "x86_64" -> "x86_64";
                    "amd64" -> "x86_64";
                    "aarch64" -> "aarch64";
                    "arm64" -> "aarch64";
                    Other -> fail("no prebuilt NIF for ~ts (~ts)", [Other, Architecture])
                end
        end,
    case os:type() of
        {win32, _} ->
            Cpu ++ "-pc-windows-msvc";
        {unix, darwin} ->
            Cpu ++ "-apple-darwin";
        {unix, linux} ->
            case string:find(Architecture, "musl") of
                nomatch -> Cpu ++ "-unknown-linux-gnu";
                _ -> Cpu ++ "-unknown-linux-musl"
            end;
        {_, OS} ->
            fail("no prebuilt NIF for ~p (~ts); build it with CLEANRULES_NIF=build", [OS, Architecture])
    end.

%% The SHA-256 of an archive, from nif-checksums.config: terms
%% {"<archive>", "sha256:<hex>"}.
expected_checksum(Root, Name, Target) ->
    File = filename:join(Root, "nif-checksums.config"),
    Checksums =
        case file:consult(File) of
            {ok, Terms} -> Terms;
            {error, Reason} -> fail("cannot read ~ts (~p): it lists the prebuilt NIFs", [File, Reason])
        end,
    case lists:keyfind(Name, 1, Checksums) of
        {Name, "sha256:" ++ Hex} -> string:lowercase(Hex);
        false -> fail("no prebuilt NIF for ~ts (~ts is not in ~ts)", [Target, Name, File])
    end.

sha256(Bytes) ->
    string:lowercase(lists:flatten([io_lib:format("~2.16.0b", [B]) || <<B>> <= crypto:hash(sha256, Bytes)])).

base_url(Version) ->
    case os:getenv("CLEANRULES_NIF_BASE_URL") of
        false -> ?RELEASES ++ Version;
        "" -> ?RELEASES ++ Version;
        Url -> string:trim(Url, trailing, "/")
    end.

cache_dir() ->
    case os:getenv("CLEANRULES_NIF_CACHE") of
        Dir when Dir =/= false, Dir =/= "" -> Dir;
        _ -> filename:basedir(user_cache, "cleanrules")
    end.

%% The archive from the cache when its checksum is the expected one, else
%% fetched, checked and cached. An archive whose checksum differs is never
%% used.
cached(Name, Expected, Fetch) ->
    Path = filename:join(cache_dir(), Name),
    case file:read_file(Path) of
        {ok, Bytes} ->
            case sha256(Bytes) of
                Expected -> Bytes;
                _ -> fetch_checked(Name, Path, Expected, Fetch)
            end;
        {error, _} ->
            fetch_checked(Name, Path, Expected, Fetch)
    end.

fetch_checked(Name, Path, Expected, Fetch) ->
    Bytes = Fetch(),
    case sha256(Bytes) of
        Expected ->
            %% A cache that cannot be written is not an error.
            _ = filelib:ensure_dir(Path),
            _ = file:write_file(Path, Bytes),
            Bytes;
        Actual ->
            fail("~ts has SHA-256 ~ts, not ~ts as nif-checksums.config says: refusing it", [Name, Actual, Expected])
    end.

fetch("file://" ++ Path) ->
    case file:read_file(Path) of
        {ok, Bytes} -> Bytes;
        {error, Reason} -> fail("cannot read ~ts: ~p", [Path, Reason])
    end;
fetch("https://" ++ _ = Url) ->
    {ok, _} = application:ensure_all_started([inets, ssl]),
    case os:getenv("HTTPS_PROXY", os:getenv("https_proxy", "")) of
        "" -> ok;
        Proxy -> set_proxy(Proxy, os:getenv("NO_PROXY", os:getenv("no_proxy", "")))
    end,
    CaCerts =
        try
            public_key:cacerts_get()
        catch
            _:_ ->
                fail(
                    "no CA certificates found to check ~ts: install the system's CA "
                    "certificates, or download the archive yourself and set "
                    "CLEANRULES_NIF_BASE_URL=file://<its directory>",
                    [Url]
                )
        end,
    Ssl = [
        {verify, verify_peer},
        {cacerts, CaCerts},
        {depth, 4},
        {customize_hostname_check, [{match_fun, public_key:pkix_verify_hostname_match_fun(https)}]}
    ],
    io:format(standard_error, "cleanrules: downloading ~ts~n", [Url]),
    Request = {Url, [{"user-agent", "cleanrules-nif"}]},
    %% GitHub answers a release asset with a redirect to its storage host;
    %% httpc follows it with the same TLS options, and the archive is checked
    %% against its SHA-256 whatever host served it.
    case httpc:request(get, Request, [{ssl, Ssl}, {autoredirect, true}, {timeout, 300000}], [{body_format, binary}]) of
        {ok, {{_, 200, _}, _, Body}} -> Body;
        {ok, {{_, Status, _}, _, _}} -> fail("~ts answered ~b", [Url, Status]);
        {error, Reason} -> fail("cannot download ~ts: ~p", [Url, Reason])
    end;
fetch(Url) ->
    fail("CLEANRULES_NIF_BASE_URL must start with https:// or file://, not ~ts", [Url]).

set_proxy(Proxy, NoProxy) ->
    Exceptions = [string:trim(Host) || Host <- string:split(NoProxy, ",", all), string:trim(Host) =/= ""],
    case uri_string:parse(Proxy) of
        #{userinfo := _} ->
            fail("HTTPS_PROXY with credentials is not supported: use CLEANRULES_NIF_BASE_URL=file://<a mirror>", []);
        #{host := Host, port := Port} ->
            ok = httpc:set_options([{https_proxy, {{unicode:characters_to_list(Host), Port}, Exceptions}}]);
        _ ->
            fail("cannot read HTTPS_PROXY ~ts (give it as http://host:port)", [Proxy])
    end.
