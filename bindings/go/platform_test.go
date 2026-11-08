package cleanrules

import (
	gobuild "go/build"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"slices"
	"strings"
	"testing"
)

// platformLibs are the platforms of the prebuilt libraries, their link
// files and the system libraries each links: what rustc prints with
// --print native-static-libs for the platform's Rust target (Rust 1.97),
// without the C library. CI checks them against rustc again
// (scripts/build-lib.sh).
var platformLibs = []struct {
	goos, goarch string
	musl         bool
	dir, file    string
	libs         string
}{
	{"darwin", "amd64", false, "darwin_amd64", "link_darwin_amd64.go", "-lm"},
	{"darwin", "arm64", false, "darwin_arm64", "link_darwin_arm64.go", "-lm"},
	{"linux", "amd64", false, "linux_amd64", "link_linux_amd64.go", "-lgcc_s -lutil -lrt -lpthread -lm -ldl"},
	{"linux", "arm64", false, "linux_arm64", "link_linux_arm64.go", "-lgcc_s -lutil -lrt -lpthread -lm -ldl"},
	{"linux", "amd64", true, "linux_amd64_musl", "link_musl_linux_amd64.go", "-lgcc_s"},
	{"linux", "arm64", true, "linux_arm64_musl", "link_musl_linux_arm64.go", "-lgcc_s"},
	{"windows", "amd64", false, "windows_amd64", "link_windows_amd64.go", "-lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp"},
	{"windows", "arm64", false, "windows_arm64", "link_windows_arm64.go", "-lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp -lunwind"},
}

// selected are the package's Go files that select how the library is linked
// and its header found, for a platform and tags.
func selected(t *testing.T, goos, goarch string, tags ...string) []string {
	t.Helper()
	ctx := gobuild.Default
	ctx.GOOS, ctx.GOARCH, ctx.CgoEnabled, ctx.BuildTags = goos, goarch, true, tags
	pkg, err := ctx.ImportDir(".", 0)
	if err != nil {
		t.Fatalf("%s/%s %v: %v", goos, goarch, tags, err)
	}
	var files []string
	for _, file := range slices.Concat(pkg.GoFiles, pkg.CgoFiles) {
		if strings.HasPrefix(file, "link") || strings.HasPrefix(file, "include") {
			files = append(files, file)
		}
	}
	slices.Sort(files)
	return files
}

// TestPlatformSelection checks which files link the library on each
// platform, with and without the tags.
func TestPlatformSelection(t *testing.T) {
	for _, p := range platformLibs {
		tags := []string{"cleanrules_prebuilt"}
		if p.musl {
			tags = append(tags, "cleanrules_musl")
		}
		want := []string{"include_prebuilt.go", p.file}
		if got := selected(t, p.goos, p.goarch, tags...); !slices.Equal(got, want) {
			t.Errorf("%s with %v: %v, want %v", p.dir, tags, got, want)
		}
		// In the repository, by default: target/ of cargo.
		if got := selected(t, p.goos, p.goarch); !slices.Equal(got, []string{"include.go", "link.go"}) {
			t.Errorf("%s by default: %v", p.dir, got)
		}
		// cleanrules_external: the library comes from CGO_LDFLAGS.
		if got := selected(t, p.goos, p.goarch, append(tags, "cleanrules_external")...); !slices.Equal(got, []string{"include_prebuilt.go"}) {
			t.Errorf("%s external: %v", p.dir, got)
		}
		if got := selected(t, p.goos, p.goarch, "cleanrules_external"); !slices.Equal(got, []string{"include.go"}) {
			t.Errorf("%s external in the repository: %v", p.dir, got)
		}
	}
	// macOS and Windows have no musl: the tag changes nothing there.
	if got := selected(t, "darwin", "arm64", "cleanrules_prebuilt", "cleanrules_musl"); !slices.Equal(got, []string{"include_prebuilt.go", "link_darwin_arm64.go"}) {
		t.Errorf("darwin with musl: %v", got)
	}
	// Elsewhere, the build stops at link_unsupported.go, which names the
	// problem.
	// Android and iOS build the files of Linux and macOS too, but the
	// libraries are not theirs.
	for _, p := range [][2]string{
		{"linux", "386"}, {"linux", "riscv64"}, {"freebsd", "amd64"}, {"windows", "386"}, {"darwin", "amd64"},
		{"android", "arm64"}, {"android", "amd64"}, {"ios", "arm64"}, {"ios", "amd64"},
	} {
		got := selected(t, p[0], p[1], "cleanrules_prebuilt")
		if musl := selected(t, p[0], p[1], "cleanrules_prebuilt", "cleanrules_musl"); p[0] == "android" && !slices.Equal(musl, got) {
			t.Errorf("%s/%s with musl: %v", p[0], p[1], musl)
		}
		supported := p[0] == "darwin"
		if slices.Contains(got, "link_unsupported.go") == supported || len(got) != 2 {
			t.Errorf("%s/%s: %v", p[0], p[1], got)
		}
	}
	source, err := os.ReadFile("link_unsupported.go")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(source), "var _ = cleanrules_has_no_prebuilt_library_for_this_GOOS_and_GOARCH__see_README_md_Platforms") {
		t.Error("link_unsupported.go does not stop the build")
	}
}

// TestLinkFiles checks what each link file links, and that every
// constraint has the form scripts/assemble.py rewrites.
func TestLinkFiles(t *testing.T) {
	ldflags := regexp.MustCompile(`(?m)^#cgo LDFLAGS: \$\{SRCDIR\}/lib/([a-z0-9_]+)/libcleanrules_ffi\.a (.*)$`)
	for _, p := range platformLibs {
		source, err := os.ReadFile(p.file)
		if err != nil {
			t.Fatal(err)
		}
		found := ldflags.FindAllStringSubmatch(string(source), -1)
		if len(found) != 1 || found[0][1] != p.dir || found[0][2] != p.libs {
			t.Errorf("%s: %q", p.file, found)
		}
		if !strings.HasPrefix(string(source), "//go:build cleanrules_prebuilt && ") {
			t.Errorf("%s: the constraint must start with cleanrules_prebuilt &&", p.file)
		}
	}
	// The repository's link.go links the same system libraries.
	link, err := os.ReadFile("link.go")
	if err != nil {
		t.Fatal(err)
	}
	for _, line := range []string{
		"#cgo linux LDFLAGS: ${SRCDIR}/../../target/debug/libcleanrules_ffi.a " + platformLibs[2].libs,
		"#cgo darwin LDFLAGS: ${SRCDIR}/../../target/debug/libcleanrules_ffi.a " + platformLibs[0].libs,
		"#cgo windows,amd64 LDFLAGS: ${SRCDIR}/../../target/x86_64-pc-windows-gnu/debug/libcleanrules_ffi.a " + platformLibs[6].libs,
		"#cgo windows,arm64 LDFLAGS: ${SRCDIR}/../../target/aarch64-pc-windows-gnullvm/debug/libcleanrules_ffi.a " + platformLibs[7].libs,
	} {
		if !strings.Contains(string(link), line+"\n") {
			t.Errorf("link.go lacks %q", line)
		}
	}
	for file, prefix := range map[string]string{
		"link.go": "//go:build !cleanrules_prebuilt", "include.go": "//go:build !cleanrules_prebuilt",
		"include_prebuilt.go": "//go:build cleanrules_prebuilt\n", "link_unsupported.go": "//go:build cleanrules_prebuilt && ",
	} {
		source, err := os.ReadFile(file)
		if err != nil {
			t.Fatal(err)
		}
		if !strings.HasPrefix(string(source), prefix) {
			t.Errorf("%s does not start with %q", file, prefix)
		}
	}
}

// TestCgoRequired checks that, without cgo, the first error of a build names
// the problem (cgo_required.go).
func TestCgoRequired(t *testing.T) {
	if testing.Short() {
		t.Skip("runs go vet")
	}
	if got := selected(t, "linux", "amd64"); !slices.Equal(got, []string{"include.go", "link.go"}) {
		t.Errorf("with cgo: %v", got)
	}
	ctx := gobuild.Default
	ctx.CgoEnabled = false
	pkg, err := ctx.ImportDir(".", 0)
	if err != nil {
		t.Fatal(err)
	}
	if !slices.Contains(pkg.GoFiles, "cgo_required.go") {
		t.Errorf("without cgo: %v", pkg.GoFiles)
	}
	vet := exec.Command(filepath.Join(runtime.GOROOT(), "bin", "go"), "vet", ".")
	vet.Env = append(os.Environ(), "CGO_ENABLED=0")
	out, err := vet.CombinedOutput()
	if err == nil {
		t.Fatalf("go vet without cgo passed:\n%s", out)
	}
	lines := strings.Split(string(out), "\n")
	if len(lines) < 2 || !strings.Contains(lines[1], "undefined: cleanrules_needs_cgo__set_CGO_ENABLED_1_and_install_a_C_compiler__see_README_md_Platforms") {
		t.Errorf("the first error does not name the problem:\n%s", out)
	}
}
