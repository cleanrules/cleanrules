//go:build cleanrules_prebuilt && !ios && !cleanrules_external

package cleanrules

// The prebuilt library for macOS on arm64 (README.md, Platforms), with the
// system libraries that Rust's standard library needs there, as rustc prints
// them (--print native-static-libs) for aarch64-apple-darwin, without the C
// library, which the compiler links anyway.

/*
#cgo LDFLAGS: ${SRCDIR}/lib/darwin_arm64/libcleanrules_ffi.a -lm
*/
import "C"
