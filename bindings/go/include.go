//go:build !cleanrules_prebuilt

package cleanrules

// The C ABI's header, in this repository: crates/ffi/include. The module
// published for Go users holds a copy in include/ (include_prebuilt.go).

/*
#cgo CFLAGS: -I${SRCDIR}/../../crates/ffi/include
*/
import "C"
