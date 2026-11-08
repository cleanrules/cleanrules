"""Assemble the Go module with the prebuilt static libraries of every platform.

Usage: python3 assemble.py <libs> <out>

<libs> holds <platform>/libcleanrules_ffi.a for each platform that a file
link_*.go of the package names (lib/<platform>/libcleanrules_ffi.a in its
#cgo LDFLAGS): those files are the one list of platforms. <out>, which must
not exist, receives the module as Go users get it (task H.2 publishes it):

- go.mod, README.md, and the repository's LICENSE and NOTICE;
- the package's Go files but its tests, as they build with the tag
  cleanrules_prebuilt: a file that needs the tag loses it (the link files and
  include_prebuilt.go), a file that needs its absence is left out (link.go,
  which links the repository's target directory, and include.go); the tag
  may only be the start of a file's first //go:build line;
- include/cleanrules.h, the C ABI's header;
- lib/<platform>/libcleanrules_ffi.a, the libraries;
- an empty package in include/ and in each lib/<platform>/, and vendor.go,
  which imports them all: go mod vendor copies only the directories of
  packages, so without them a vendored build would lack the libraries.

Standard library only; it prints what it writes, and every problem to stderr
with exit code 1.
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
from pathlib import Path

PACKAGE = Path(__file__).resolve().parent.parent
REPOSITORY = PACKAGE.parent.parent
HEADER = REPOSITORY / "crates" / "ffi" / "include" / "cleanrules.h"
LIBRARY = "libcleanrules_ffi.a"
TAG = "cleanrules_prebuilt"
LINKED = re.compile(r"\$\{SRCDIR\}/lib/([a-z0-9_]+)/" + re.escape(LIBRARY))


class AssemblyError(Exception):
    """A problem that stops the assembly."""


def module_path() -> str:
    """The module path of go.mod."""
    match = re.search(r"^module\s+(\S+)", (PACKAGE / "go.mod").read_text(), re.MULTILINE)
    if not match:
        raise AssemblyError("go.mod names no module")
    return match[1]


def platforms() -> list[str]:
    """The platforms of the link files, in the order of their names."""
    found = []
    for file in sorted(PACKAGE.glob("link_*.go")):
        found += LINKED.findall(file.read_text())
    if len(found) != len(set(found)):
        raise AssemblyError(f"two link files name the same platform: {found}")
    return found


def transform(source: str, name: str) -> str | None:
    """A Go file as the module holds it, or None to leave it out."""
    lines = source.split("\n")
    constraint = lines[0] if lines[0].startswith("//go:build ") else ""
    others = [line for line in lines[1:] if line.startswith("//go:build") and TAG in line]
    if others:
        raise AssemblyError(f"{name} names {TAG} in a //go:build line that is not its first")
    if TAG not in constraint:
        return source
    expression = constraint.removeprefix("//go:build ")
    if expression == TAG:
        # The whole constraint: the file builds everywhere, so the line and
        # the blank line after it go.
        rest = lines[1:]
        if rest and rest[0] == "":
            rest = rest[1:]
        return "\n".join(rest)
    if expression.startswith(f"{TAG} && "):
        lines[0] = "//go:build " + expression.removeprefix(f"{TAG} && ")
        result = "\n".join(lines)
    elif expression.startswith(f"!{TAG}"):
        return None
    else:
        raise AssemblyError(
            f"{name}: the constraint {constraint!r} must start with {TAG} or !{TAG}"
        )
    if TAG in result.split("\n", 1)[0]:
        raise AssemblyError(f"{name} names {TAG} more than once")
    return result


def empty_package(directory: str, doc: str) -> str:
    """An empty package for directory, with its documentation."""
    package = directory.rsplit("/", 1)[-1]
    return f"// Package {package} {doc}\npackage {package}\n"


def assemble(libs: Path, out: Path) -> list[str]:
    """Write the module into out; the files written, relative to it."""
    if out.exists():
        raise AssemblyError(f"{out} exists: give a new directory")
    module = module_path()
    targets = platforms()
    missing = [p for p in targets if not (libs / p / LIBRARY).is_file()]
    if missing:
        raise AssemblyError(f"{libs} lacks the library of {missing}")
    extra = sorted(p.name for p in libs.iterdir() if p.is_dir() and p.name not in targets)
    if extra:
        raise AssemblyError(f"{libs} holds platforms that no link file names: {extra}")
    written: list[str] = []

    def write(relative: str, text: str) -> None:
        path = out / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8", newline="\n")
        written.append(relative)

    def copy(source: Path, relative: str) -> None:
        path = out / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, path)
        written.append(relative)

    out.mkdir(parents=True)
    copy(PACKAGE / "go.mod", "go.mod")
    copy(PACKAGE / "README.md", "README.md")
    copy(REPOSITORY / "LICENSE", "LICENSE")
    copy(REPOSITORY / "NOTICE", "NOTICE")
    for file in sorted(PACKAGE.glob("*.go")):
        if file.name.endswith("_test.go"):
            continue
        text = transform(file.read_text(encoding="utf-8"), file.name)
        if text is not None:
            write(file.name, text)
    copy(HEADER, "include/cleanrules.h")
    write(
        "include/include.go",
        empty_package("include", "holds the header of the C ABI of cleanrules, cleanrules.h."),
    )
    imports = [f"{module}/include"]
    for platform in targets:
        copy(libs / platform / LIBRARY, f"lib/{platform}/{LIBRARY}")
        write(
            f"lib/{platform}/lib.go",
            empty_package(
                platform, f"holds the prebuilt static library of cleanrules for {platform}."
            ),
        )
        imports.append(f"{module}/lib/{platform}")
    lines = "".join(f'\t_ "{path}"\n' for path in imports)
    write(
        "vendor.go",
        "package cleanrules\n\n"
        "// The directories of the header and of the prebuilt libraries hold no\n"
        "// code, and go mod vendor copies only those of packages: so each holds an\n"
        "// empty package, imported here (written by bindings/go/scripts/assemble.py).\n"
        f"import (\n{lines})\n",
    )
    return written


def main() -> int:
    """Assemble the module given on the command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("libs", type=Path)
    parser.add_argument("out", type=Path)
    arguments = parser.parse_args()
    existed = arguments.out.exists()
    try:
        written = assemble(arguments.libs, arguments.out)
    except AssemblyError as problem:
        if not existed:
            shutil.rmtree(arguments.out, ignore_errors=True)  # no half-written module
        print(problem, file=sys.stderr)
        return 1
    for relative in written:
        print(relative)
    print(f"{arguments.out}: the module with {len(platforms())} prebuilt libraries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
