//go:build cleanrules_prebuilt && !cleanrules_external

package cleanrules

// The prebuilt library for Windows on x64, built for MinGW-w64's GCC
// (README.md, Platforms), with the system libraries that Rust's standard
// library needs there, as rustc prints them (--print native-static-libs) for
// x86_64-pc-windows-gnu, without the C library, which the compiler links
// anyway.

/*
#cgo LDFLAGS: ${SRCDIR}/lib/windows_amd64/libcleanrules_ffi.a -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp
*/
import "C"
