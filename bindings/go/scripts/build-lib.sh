#!/usr/bin/env bash
# Build the prebuilt static library of one platform for the Go module
# (go-libs.yml), from the repository's root:
#
#   bash bindings/go/scripts/build-lib.sh <rust target> <platform> <out> [--probe]
#
# 1. The library, in release with fat link-time optimization and one codegen
#    unit (cargo rustc ... --crate-type staticlib, which needs no linker of
#    the target): the archive then holds one object with the code the C ABI
#    reaches, not every object of the standard library; musl's without a
#    static C runtime, as the Go program links musl dynamically.
# 2. The system libraries rustc asks for (--print native-static-libs) must
#    all be in the platform's link file (bindings/go/link_*.go), but the C
#    library, which the compiler links anyway.
# 3. Its debug information and any LLVM bitcode that rustc embeds are
#    stripped (llvm-strip of rustup's llvm-tools); but on Windows on arm64,
#    whose archive holds short import members (raw-dylib) that llvm-strip
#    cannot rewrite.
# 4. It goes to <out>/libs/<platform>/libcleanrules_ffi.a.
# 5. With --probe (Linux): bindings/go/scripts/probe.c is linked from it with
#    the link file's system libraries and run here, and goes to
#    <out>/probes/<platform>/probe with the library's SHA-256, for
#    check-libs.py.
set -euo pipefail

target=$1
platform=$2
out=$3
probe=${4:-}

case $platform in
  *_musl) link=bindings/go/link_musl_${platform%_musl}.go ;;
  *) link=bindings/go/link_${platform}.go ;;
esac
test -f "$link" || { echo "no link file $link for $platform" >&2; exit 1; }

case $target in
  *-musl) export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static" ;;
  *-apple-darwin) export MACOSX_DEPLOYMENT_TARGET=11.0 ;; # Go 1.24's minimum
esac

export CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
log=$(cargo rustc --color never --locked --release -p cleanrules-ffi --lib --crate-type staticlib --target "$target" \
  -- --print native-static-libs 2>&1) || { echo "$log"; exit 1; }
echo "$log" | tail -n 3
# CI sets CARGO_TERM_COLOR=always; drop any colour codes that still get through.
wanted=$(echo "$log" | sed 's/\x1b\[[0-9;]*m//g' | grep -o 'native-static-libs: .*' | sed 's/native-static-libs: //')
linked=$(sed -n 's/^#cgo LDFLAGS: [^ ]*libcleanrules_ffi\.a//p' "$link")
echo "rustc asks for: $wanted"
echo "$link links: $linked"
missing=""
for lib in $wanted; do
  case $lib in -lc | -lSystem) continue ;; esac
  case " $linked " in *" $lib "*) ;; *) missing="$missing $lib" ;; esac
done
if [ -n "$missing" ]; then
  echo "rustc asks for$missing, which $link does not link" >&2
  exit 1
fi

library=$out/libs/$platform/libcleanrules_ffi.a
mkdir -p "$(dirname "$library")"
cp "target/$target/release/libcleanrules_ffi.a" "$library"
before=$(wc -c < "$library")
if [ "$platform" != windows_arm64 ]; then
  host=$(rustc -vV | sed -n 's/^host: //p')
  strip=$(rustc --print sysroot)/lib/rustlib/$host/bin/llvm-strip
  [ -x "$strip" ] || strip=$strip.exe
  "$strip" --strip-debug -R .llvmbc -R .llvmcmd -R __LLVM,__bitcode -R __LLVM,__cmdline "$library"
fi
echo "$library: $before bytes, $(wc -c < "$library") stripped"

if [ "$probe" = --probe ]; then
  dir=$out/probes/$platform
  mkdir -p "$dir"
  # shellcheck disable=SC2086 # the system libraries are words
  cc -O2 -I crates/ffi/include bindings/go/scripts/probe.c "$library" $linked -o "$dir/probe"
  "$dir/probe"
  (cd "$(dirname "$library")" && sha256sum libcleanrules_ffi.a) > "$dir/libcleanrules_ffi.a.sha256"
fi
