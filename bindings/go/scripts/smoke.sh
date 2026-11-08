#!/usr/bin/env bash
# Use the assembled Go module (assemble.py) as Go users would, with no cargo
# and no target directory (go-libs.yml), from the repository's root:
#
#   bash bindings/go/scripts/smoke.sh <module> <work> [tags]
#
# 1. A clean module in <work> whose go.mod replaces the binding with
#    <module>: go vet, then bindings/go/scripts/smoke/main.go built and run;
#    again from a vendor directory (go mod vendor), which must hold the
#    libraries and the header too.
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

rm -rf "$work"
mkdir -p "$work"
cp bindings/go/scripts/smoke/main.go "$work/"
cd "$work"
cat > go.mod <<EOF
module smoke

go 1.24

require $path v0.0.0

replace $path => $module
EOF
go vet ${extra:+-tags "$extra"} .
exe=$(go env GOEXE)
go build ${extra:+-tags "$extra"} -o "smoke$exe" .
"./smoke$exe"
go mod vendor
test -f "vendor/$path/include/cleanrules.h"
go build -mod=vendor ${extra:+-tags "$extra"} -o "smoke-vendored$exe" .
"./smoke-vendored$exe"

cd "$repository/bindings/go"
rm -rf lib include
cp -R "$module/lib" "$module/include" .
go vet -tags "$tags" ./...
go test -count=1 -tags "$tags" ./...
