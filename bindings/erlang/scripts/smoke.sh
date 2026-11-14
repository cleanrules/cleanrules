#!/usr/bin/env bash
# Uses the prebuilt NIF libraries as a package does: copies the application
# without native/ (so scripts/nif.escript downloads instead of building, and no
# cargo is needed), with the nif-checksums.config of the archives, points
# CLEANRULES_NIF_BASE_URL at them, and runs the Common Test suites against the
# library it downloaded and checked. Then checks that an archive whose
# checksum differs is refused.
#
# Usage: smoke.sh <dir with the archives and nif-checksums.config> <work dir>
set -euo pipefail

assets=$(cd "$1" && pwd)
work=$2
app=$(cd "$(dirname "$0")/.." && pwd)
# Erlang on Windows wants C:/... paths, not Git Bash's /c/...
url_path() { if command -v cygpath > /dev/null; then cygpath -m "$1"; else echo "$1"; fi; }

rm -rf "$work"
mkdir -p "$work/app"
cp -R "$app/src" "$app/test" "$app/scripts" "$app/rebar.config" "$app/LICENSE" "$app/NOTICE" "$work/app/"
cp "$assets/nif-checksums.config" "$work/app/"

export CLEANRULES_NIF_BASE_URL="file://$(url_path "$assets")"
export CLEANRULES_NIF_CACHE="$work/cache"
erl -noshell -eval 'io:format("~ts, NIF ~ts~n", [erlang:system_info(system_version), erlang:system_info(nif_version)]), halt().'
(cd "$work/app" && rebar3 ct)

# A changed archive is refused, and nothing is put in priv/.
mkdir -p "$work/tampered"
cp "$assets"/*.tar.gz "$work/tampered/"
for archive in "$work"/tampered/*.tar.gz; do printf 'x' >> "$archive"; done
rm -rf "$work/app/priv" "$work/cache"
if CLEANRULES_NIF_BASE_URL="file://$(url_path "$work/tampered")" escript "$work/app/scripts/nif.escript" 2> "$work/refused.txt"; then
    echo "a changed archive was accepted" >&2
    exit 1
fi
grep -q "refusing it" "$work/refused.txt" || { cat "$work/refused.txt" >&2; exit 1; }
test ! -e "$work/app/priv/cleanrules_nif.so" && test ! -e "$work/app/priv/cleanrules_nif.dll"
echo "smoke: ok"
