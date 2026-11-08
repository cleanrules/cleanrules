//go:build cleanrules_prebuilt && cleanrules_musl && !android && !cleanrules_external

package cleanrules

// The prebuilt library for Linux on x64 with musl (Alpine), with the tag
// cleanrules_musl (README.md, Platforms), with the system libraries that
// Rust's standard library needs there, as rustc prints them (--print native-
// static-libs) for x86_64-unknown-linux-musl, without the C library, which
// the compiler links anyway.

/*
#cgo LDFLAGS: ${SRCDIR}/lib/linux_amd64_musl/libcleanrules_ffi.a -lgcc_s
*/
import "C"
