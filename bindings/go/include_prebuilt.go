//go:build cleanrules_prebuilt

package cleanrules

// The C ABI's header in the module published for Go users, which holds it in
// include/ next to the prebuilt libraries in lib/ (README.md, Platforms).

/*
#cgo CFLAGS: -I${SRCDIR}/include
*/
import "C"
