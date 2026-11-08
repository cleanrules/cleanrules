//go:build cleanrules_prebuilt && !cleanrules_external

package cleanrules

// The prebuilt library for Windows on arm64, built for llvm-mingw
// (README.md, Platforms), with the system libraries that Rust's standard
// library needs there, as rustc prints them (--print native-static-libs) for
// aarch64-pc-windows-gnullvm, without the C library, which the compiler
// links anyway.

/*
#cgo LDFLAGS: ${SRCDIR}/lib/windows_arm64/libcleanrules_ffi.a -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp -lunwind
*/
import "C"
