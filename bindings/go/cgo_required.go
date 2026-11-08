//go:build !cgo

package cleanrules

// The package calls the core through cgo, so it needs cgo: CGO_ENABLED=1 and
// a C compiler (README.md, Platforms). Without it, every file that calls the
// core is left out and the build would stop at the names they declare, such
// as "undefined: HitPolicy"; this declaration, in the file that sorts first
// among those left, makes the first error name the problem instead. (A type
// alias or a blank name would be checked later.)
type cgoRequired cleanrules_needs_cgo__set_CGO_ENABLED_1_and_install_a_C_compiler__see_README_md_Platforms
