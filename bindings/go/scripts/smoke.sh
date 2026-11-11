#!/usr/bin/env bash
# Use the assembled Go module (assemble.py) as Go users would, with no cargo,
# no target directory and no network (go-libs.yml), from the repository's
# root:
#
#   bash bindings/go/scripts/smoke.sh <module> <work> [tags]
#
# 1. A clean module in <work>, outside the repository, whose go.mod requires
#    github.com/cleanrules/cleanrules-go v0.0.0 and replaces it with <module>,
#    with GOPROXY=off, GOSUMDB=off, GOFLAGS=-mod=mod, GOTOOLCHAIN=local and
#    GOWORK=off, so that nothing can be fetched: the module must resolve from
#    the tree alone and need no other module (go list -m all, no go.sum). Then
#    go vet, and bindings/go/scripts/smoke/main.go built, run and run again
#    with go run, given the module's VERSION, which the linked library must
#    report; the example of the module's README.md (its block of Go with
#    package main) is vetted and run too. Again from a vendor directory (go
#    mod vendor), which must hold the header and every library.
# 2. The package's own tests against the prebuilt library: the module's lib/
#    and include/ are copied into bindings/go, which then builds with the tag
#    cleanrules_prebuilt (and [tags], such as cleanrules_musl); go vet and
#    go test -count=1 (the results are cached by flags, not by library).
set -euo pipefail

module=$(cd "$1" && pwd)
if command -v cygpath > /dev/null; then
  module=$(cygpath -m "$module") # Git Bash on Windows: a path Go reads
fi
work=$2
extra=${3:-}
tags=cleanrules_prebuilt${extra:+,$extra}
repository=$(pwd)
path=$(sed -n 's/^module //p' "$module/go.mod")
version=$(cat "$module/VERSION")
test -n "$path" && test -n "$version"

# Nothing from the network, and no other Go than this one.
export GOPROXY=off GOSUMDB=off GOFLAGS=-mod=mod GOTOOLCHAIN=local GOWORK=off

# <work> by its absolute path, checked before anything is removed or made.
mkdir -p "$(dirname "$work")"
work=$(cd "$(dirname "$work")" && pwd)/$(basename "$work")
case "$work/" in
  "$repository"/*)
    echo "smoke.sh: $work is inside the repository: give a directory outside it" >&2
    exit 1
    ;;
esac
rm -rf "$work"
mkdir "$work"
cp bindings/go/scripts/smoke/main.go "$work/"
mkdir "$work/readme"
# The README's example: the first block of Go that holds package main.
awk '/^```go$/ { on = 1; main = 0; text = ""; next }
  /^```$/ { if (on && main) { printf "%s", text; exit } on = 0; next }
  on { text = text $0 "\n"; if ($0 == "package main") main = 1 }' "$module/README.md" > "$work/readme/main.go"
test -s "$work/readme/main.go" || { echo "smoke.sh: README.md has no example with package main" >&2; exit 1; }
cd "$work"
cat > go.mod <<EOF
module smoke

go 1.24

require $path v0.0.0

replace $path => $module
EOF
go list -m all
test "$(go list -m -f '{{.Replace.Path}}' "$path")" = "$module"
test "$(go list -m all | wc -l)" -eq 2 # smoke and the module: it needs no other
go vet ${extra:+-tags "$extra"} ./...
exe=$(go env GOEXE)
go build ${extra:+-tags "$extra"} -o "smoke$exe" .
"./smoke$exe" "$version"
go run ${extra:+-tags "$extra"} . "$version"
go run ${extra:+-tags "$extra"} ./readme
test ! -s go.sum # nothing to check a sum of

go mod vendor
test -f "vendor/$path/include/cleanrules.h"
for lib in "$module"/lib/*/; do
  test -f "vendor/$path/lib/$(basename "$lib")/libcleanrules_ffi.a"
done
case "$(go list -mod=vendor -f '{{.Dir}}' "$path")" in
  *vendor*) ;;
  *) echo "smoke.sh: the vendored build does not read vendor/" >&2; exit 1 ;;
esac
go build -mod=vendor ${extra:+-tags "$extra"} -o "smoke-vendored$exe" .
"./smoke-vendored$exe" "$version"
go run -mod=vendor ${extra:+-tags "$extra"} ./readme

cd "$repository/bindings/go"
rm -rf lib include
cp -R "$module/lib" "$module/include" .
go vet -tags "$tags" ./...
go test -count=1 -tags "$tags" ./...
