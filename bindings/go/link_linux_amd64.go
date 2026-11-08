//go:build cleanrules_prebuilt && !cleanrules_musl && !android && !cleanrules_external

package cleanrules

// The prebuilt library for Linux on x64 with glibc 2.28 or later (README.md,
// Platforms), with the system libraries that Rust's standard library needs
// there, as rustc prints them (--print native-static-libs) for
// x86_64-unknown-linux-gnu, without the C library, which the compiler links
// anyway.

/*
#cgo LDFLAGS: ${SRCDIR}/lib/linux_amd64/libcleanrules_ffi.a -lgcc_s -lutil -lrt -lpthread -lm -ldl
*/
import "C"
