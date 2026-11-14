#!/usr/bin/env bash
# Builds the NIF library for one Rust target and packs it as the release asset
# that scripts/nif.escript downloads in a package (README.md, "Prebuilt
# libraries"):
#
#   <out>/libcleanrules_nif-v<vsn>-nif-2.17-<target>.<so|dll>.tar.gz
#
# holding one file, cleanrules_nif.so (cleanrules_nif.dll on Windows), the name
# erlang:load_nif/2 completes. <vsn> is the application's, which must be the
# workspace's (Cargo.toml).
#
# Usage: package-nif.sh <rust target> <out dir>
set -euo pipefail

target=$1
out=$2
app=$(cd "$(dirname "$0")/.." && pwd)
root=$(cd "$app/../.." && pwd)

vsn=$(sed -n 's/.*{vsn, *"\([^"]*\)"}.*/\1/p' "$app/src/cleanrules.app.src")
workspace=$(sed -n '/^\[workspace.package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' "$root/Cargo.toml")
if [ -z "$vsn" ] || [ "$vsn" != "$workspace" ]; then
    echo "the application's version ($vsn) is not the workspace's ($workspace)" >&2
    exit 1
fi

case $target in
    *-windows-msvc) built=cleanrules_nif.dll name=cleanrules_nif.dll ext=dll ;;
    *-apple-darwin) built=libcleanrules_nif.dylib name=cleanrules_nif.so ext=so ;;
    *-linux-gnu | *-linux-musl) built=libcleanrules_nif.so name=cleanrules_nif.so ext=so ;;
    *) echo "no prebuilt NIF for $target" >&2; exit 1 ;;
esac
case $target in
    # A shared library on musl links the C library dynamically.
    *-linux-musl) export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static" ;;
    # The oldest macOS that Erlang/OTP's own builds support.
    *-apple-darwin) export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0} ;;
esac

(cd "$root" && cargo build --locked --release -p cleanrules-erlang --lib --target "$target")

target_dir=${CARGO_TARGET_DIR:-$root/target}
stage=$(mktemp -d)
cp "$target_dir/$target/release/$built" "$stage/$name"
mkdir -p "$out"
asset="libcleanrules_nif-v$vsn-nif-2.17-$target.$ext.tar.gz"
# No AppleDouble files from macOS's tar.
COPYFILE_DISABLE=1 tar -czf "$out/$asset" -C "$stage" "$name"
rm -rf "$stage"
echo "$out/$asset"
