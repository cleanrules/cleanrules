//go:build !cleanrules_prebuilt && !cleanrules_external

package cleanrules

// How the package finds the C ABI in this repository, the default for its
// contributors (task H.1a): the static library that cargo writes in the
// repository's target directory, linked by its full path. Not with -L and
// -l, because cargo writes the shared library next to it and the linkers of
// Linux and macOS would prefer that one, which then has to be found again
// when the program runs. The system libraries are the ones Rust's standard
// library needs, as
//
//	cargo rustc -p cleanrules-ffi --lib --crate-type staticlib -- --print native-static-libs
//
// prints them on each platform. Build the library first:
//
//	cargo build -p cleanrules-ffi                       # Linux and macOS
//	cargo rustc -p cleanrules-ffi --lib --crate-type staticlib --target x86_64-pc-windows-gnu
//	cargo rustc -p cleanrules-ffi --lib --crate-type staticlib --target aarch64-pc-windows-gnullvm
//
// On Windows, cgo compiles and links with MinGW-w64: GCC on x64, whose
// objects and runtime are those of Rust's GNU target, and LLVM's
// (llvm-mingw) on arm64, those of the GNU LLVM target; the library of the
// MSVC targets expects Microsoft's C runtime and linker.
//
// The module published for Go users (task H.2) has no target directory: it
// holds a prebuilt library per platform, linked by the files
// link_<os>_<arch>.go, which this repository builds with the tag
// cleanrules_prebuilt (README.md, Platforms). To link another build of the
// library (release, another directory), build with -tags
// cleanrules_external and give the library and the system libraries in
// CGO_LDFLAGS.

/*
#cgo linux LDFLAGS: ${SRCDIR}/../../target/debug/libcleanrules_ffi.a -lgcc_s -lutil -lrt -lpthread -lm -ldl
#cgo darwin LDFLAGS: ${SRCDIR}/../../target/debug/libcleanrules_ffi.a -lm
#cgo windows,amd64 LDFLAGS: ${SRCDIR}/../../target/x86_64-pc-windows-gnu/debug/libcleanrules_ffi.a -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp
#cgo windows,arm64 LDFLAGS: ${SRCDIR}/../../target/aarch64-pc-windows-gnullvm/debug/libcleanrules_ffi.a -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp -lunwind
*/
import "C"
