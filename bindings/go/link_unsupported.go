//go:build cleanrules_prebuilt && !cleanrules_external && !(darwin && !ios && (amd64 || arm64)) && !(linux && !android && (amd64 || arm64)) && !(windows && (amd64 || arm64))

package cleanrules

// There is no prebuilt library for this platform (Android and iOS included,
// whose GOOS also builds the files of Linux and macOS): the build stops here,
// with an error that names the problem, rather than at the link with symbols
// the linker cannot find. Build the library for the platform from the
// repository (cargo build -p cleanrules-ffi --release) and link it with -tags
// cleanrules_external and CGO_LDFLAGS (README.md, Platforms).
var _ = cleanrules_has_no_prebuilt_library_for_this_GOOS_and_GOARCH__see_README_md_Platforms
