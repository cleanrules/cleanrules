//go:build !cleanrules_external

package cleanrules

// How the package finds the C ABI (task H.1a): the static library that cargo
// writes in the repository's target directory, linked by its full path. Not
// with -L and -l, because cargo writes the shared library next to it and the
// linkers of Linux and macOS would prefer that one, which then has to be found
// again when the program runs. The system libraries are the ones Rust's
// standard library needs, as
//
//	cargo rustc -p cleanrules-ffi --lib --crate-type staticlib -- --print native-static-libs
//
// prints them on each platform. Build the library first:
//
//	cargo build -p cleanrules-ffi                       # Linux and macOS
//	cargo rustc -p cleanrules-ffi --lib --crate-type staticlib --target x86_64-pc-windows-gnu
//
// On Windows, cgo compiles and links with MinGW-w64's gcc, so the library is
// built for Rust's GNU target, whose objects and runtime are MinGW's; the
// library of the MSVC target expects Microsoft's C runtime and linker.
//
// To link another build of the library (release, another directory, a
// prebuilt one), build with -tags cleanrules_external and give the library
// and the system libraries in CGO_LDFLAGS (README.md).

/*
#cgo linux LDFLAGS: ${SRCDIR}/../../target/debug/libcleanrules_ffi.a -lgcc_s -lutil -lrt -lpthread -lm -ldl
#cgo darwin LDFLAGS: ${SRCDIR}/../../target/debug/libcleanrules_ffi.a -lm
#cgo windows LDFLAGS: ${SRCDIR}/../../target/x86_64-pc-windows-gnu/debug/libcleanrules_ffi.a -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp
*/
import "C"
