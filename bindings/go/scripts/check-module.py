"""Check the assembled Go module, the tree of github.com/cleanrules/cleanrules-go.

Usage: python3 check-module.py <module> [--probes <dir>] [--platforms all|<p1>,<p2>...]
                               [--release <version>]

<module> is the tree that assemble.py writes, as task M.2 will push it to the
repository github.com/cleanrules/cleanrules-go and tag it (nothing here
publishes anything). It is checked against this repository, from which it
was assembled, and against what Go, its module proxy and GitHub require:

- files: exactly go.mod, README.md, LICENSE, NOTICE, VERSION, vendor.go, the
  package's Go files as assemble.py writes them from bindings/go (no tests,
  each one the same text), include/cleanrules.h and include/include.go, and
  lib/<platform>/libcleanrules_ffi.a and lib.go for each platform given (all
  eight by default); nothing else, no symbolic links, names that every file
  system and a module zip take (letters, digits and ._+~-, not starting with
  a dot, distinct when case is ignored);
- sizes: within Go's limits for a module (500 MiB in all, 16 MiB for go.mod
  and LICENSE) and GitHub's (100 MiB a file; a warning past 50 MiB);
- go.mod: the module path github.com/cleanrules/cleanrules-go and the go
  directive of bindings/go/go.mod, and nothing else (no require, replace,
  exclude, retract, toolchain, godebug or tool): the module needs no other;
- VERSION: the workspace's version (Cargo.toml), a semantic version as Go
  module versions are (MAJOR.MINOR.PATCH[-prerelease]), whose tag is v and
  it; from 2.0.0 on, Go needs the module path to end in /v<major>; with
  --release, the version given, and not the workspace's placeholder 0.0.0
  (only a warning without it);
- README.md: bindings/go/scripts/README.module.md, written for Go users: only
  absolute https links (none relative, since it is read outside this
  repository), and those into github.com/cleanrules/cleanrules to files and
  directories of this repository on main (and headings, for a fragment of a
  Markdown file); the go get command and the import of the module path; a
  Platforms section (the errors of unsupported builds name it) with the
  table of bindings/go/README.md, the same text, naming every platform; the
  tags cleanrules_musl and cleanrules_external; an example with package main
  (smoke.sh runs it); no task of the plan;
- LICENSE, NOTICE and the header: the repository's, byte for byte;
- every text file: not the package's former module path
  (github.com/cleanrules/cleanrules/bindings/go), no university or
  affiliation, and no former name of the project;
- the libraries and the rest, as check-libs.py checks them (each library an
  archive of its platform's objects, macOS 11.0, glibc's or musl's own
  symbols, and with --probes, glibc 2.28 at most or musl's libc).

Standard library only (it loads assemble.py and check-libs.py from its
directory); it prints what it checked, every problem to stderr, and exits
with 1 if there is one.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
import zlib
from pathlib import Path
from types import ModuleType

HERE = Path(__file__).resolve().parent
PLACEHOLDER = "0.0.0"
MIB = 1 << 20
# Go's limits for a module (golang.org/x/mod/zip) and GitHub's for a file.
MAX_MODULE = 500 * MIB
MAX_GO_MOD = 16 * MIB
MAX_LICENSE = 16 * MIB
MAX_FILE = 100 * MIB
WARN_FILE = 50 * MIB
SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?")
NAME = re.compile(r"[A-Za-z0-9_+~-][A-Za-z0-9._+~-]*")
FORMER_PATH = "github.com/cleanrules/cleanrules/bindings/go"
FORBIDDEN = re.compile(r"\w*(?:cunef|universi|rulepy)\w*|python[- ]reference", re.IGNORECASE)
DIRECTIVES = ("require", "replace", "exclude", "retract", "toolchain", "godebug", "tool")
REPOSITORY_URL = "https://github.com/cleanrules/cleanrules"
TEXT_FILES = (".go", ".mod", ".md", ".h")


def load(name: str, file: str) -> ModuleType:
    """A script next to this one, as a module."""
    module = sys.modules.get(name)
    if module is not None:
        return module
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {HERE / file}")
    module = importlib.util.module_from_spec(spec)
    # Registered first: dataclasses look their module up.
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


assemble = load("assemble", "assemble.py")
check_libs = load("check_libs", "check-libs.py")
REPOSITORY = assemble.REPOSITORY
MODULE = assemble.MODULE


class Report:
    """What was checked, the warnings and the problems."""

    def __init__(self) -> None:
        self.lines: list[str] = []
        self.warnings: list[str] = []
        self.problems: list[str] = []

    def line(self, text: str) -> None:
        """Something that was checked, to print."""
        self.lines.append(text)

    def warn(self, text: str) -> None:
        """Something to say that does not fail the check."""
        self.warnings.append(text)

    def problem(self, text: str) -> None:
        """Something that fails the check."""
        self.problems.append(text)


def expected_files(platforms: list[str]) -> set[str]:
    """The files the module must hold, relative to it."""
    files = {
        "go.mod",
        "README.md",
        "LICENSE",
        "NOTICE",
        "VERSION",
        "vendor.go",
        "include/cleanrules.h",
        "include/include.go",
    }
    files |= set(assemble.go_files())
    for platform in platforms:
        files |= {f"lib/{platform}/{assemble.LIBRARY}", f"lib/{platform}/lib.go"}
    return files


def check_tree(module: Path, platforms: list[str], report: Report) -> dict[str, bytes]:
    """The exact file set, names and sizes; the files' contents, by relative path."""
    contents: dict[str, bytes] = {}
    folded: dict[str, str] = {}
    for path in sorted(module.rglob("*")):
        relative = path.relative_to(module).as_posix()
        if path.is_symlink():
            report.problem(f"{relative} is a symbolic link")
            continue
        for part in relative.split("/"):
            if not NAME.fullmatch(part):
                report.problem(f"{relative}: {part!r} is not a portable name for a module zip")
        if relative.lower() in folded:
            report.problem(f"{relative} and {folded[relative.lower()]} differ only in case")
        folded[relative.lower()] = relative
        if path.is_dir():
            if not any(path.iterdir()):
                report.problem(f"{relative}/ is an empty directory")
            continue
        contents[relative] = path.read_bytes()
    want = expected_files(platforms)
    for name in sorted(want - contents.keys()):
        report.problem(f"the module lacks {name}")
    for name in sorted(contents.keys() - want):
        report.problem(f"the module holds what it must not: {name}")

    total = sum(len(data) for data in contents.values())
    for name, data in sorted(contents.items()):
        if len(data) > MAX_FILE:
            report.problem(f"{name}: {len(data)} bytes, over GitHub's limit of 100 MiB a file")
        elif len(data) > WARN_FILE:
            report.warn(f"{name}: {len(data)} bytes, over the 50 MiB that GitHub warns about")
    for name, limit in (("go.mod", MAX_GO_MOD), ("LICENSE", MAX_LICENSE)):
        if len(contents.get(name, b"")) > limit:
            report.problem(f"{name} is over Go's limit of {limit // MIB} MiB")
    if total > MAX_MODULE:
        report.problem(f"the module has {total} bytes, over Go's limit of 500 MiB")
    zipped = sum(len(zlib.compress(data, 6)) for data in contents.values())
    report.line(
        f"{len(contents)} files, {total} bytes ({total / MIB:.1f} MiB);"
        f" about {zipped / MIB:.1f} MiB compressed (deflate, as the module zip)"
    )
    return contents


def check_go_mod(text: str, version: str | None, report: Report) -> None:
    """go.mod: the module path, the go directive, and nothing else."""
    path, go = assemble.go_mod()
    for number, raw in enumerate(text.split("\n"), 1):
        words = raw.split("//", 1)[0].split()
        if words and words[0] in DIRECTIVES:
            report.problem(
                f"go.mod:{number}: a {words[0]} directive: the module needs no other module"
            )
        elif words and words[0] not in ("module", "go"):
            report.problem(f"go.mod:{number}: {raw.strip()!r} is not the module or go directive")
    found = re.search(r"^module\s+(\S+)\s*$", text, re.MULTILINE)
    if not found or found[1] != MODULE:
        report.problem(f"go.mod names the module {found[1] if found else None}, not {MODULE}")
    directive = re.search(r"^go\s+(\S+)\s*$", text, re.MULTILINE)
    if not directive or directive[1] != go:
        report.problem(
            f"go.mod has the go directive {directive[1] if directive else None},"
            f" not bindings/go's {go}"
        )
    if path != MODULE:
        report.problem(f"bindings/go/go.mod names the module {path}, not {MODULE}")
    if text != f"module {MODULE}\n\ngo {go}\n":
        report.problem("go.mod is not as assemble.py writes it")
    if version and SEMVER.fullmatch(version):
        major = int(version.split(".")[0])
        suffix = re.search(r"/v(\d+)$", found[1] if found else "")
        if major >= 2 and (not suffix or int(suffix[1]) != major):
            report.problem(f"version {version} needs the module path {MODULE}/v{major} (Go's rule)")
        if major < 2 and suffix:
            report.problem(f"version {version} cannot have a /v<major> module path")
    report.line(f"go.mod: module {found[1] if found else None}, go {go}, nothing else")


def check_version(text: str | None, release: str | None, report: Report) -> str | None:
    """VERSION: the workspace's, a semantic version, and the release's when given."""
    if text is None:
        return None
    if not text.endswith("\n") or "\n" in text[:-1]:
        report.problem("VERSION is not one line")
    version = text.strip()
    if not SEMVER.fullmatch(version):
        report.problem(
            f"VERSION {version!r} is not MAJOR.MINOR.PATCH[-prerelease], as a Go module's"
        )
    workspace = assemble.workspace_version()
    if version != workspace:
        report.problem(f"VERSION is {version}, not the workspace's (Cargo.toml) {workspace}")
    if release is not None and version != release:
        report.problem(f"the version is {version}, not the release's {release}")
    if version == PLACEHOLDER:
        message = (
            f"version {version} is the workspace's placeholder: set the release's in Cargo.toml"
        )
        if release is not None:
            report.problem(message)
        else:
            report.warn(message)
    report.line(f"VERSION: {version}, tag v{version} (M.2 tags it)")
    return version


def slug(heading: str) -> str:
    """The anchor GitHub gives a Markdown heading."""
    text = re.sub(r"[`*]", "", heading.strip().lower())
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def anchors(markdown: str) -> set[str]:
    """The anchors of a Markdown file's headings (outside code blocks)."""
    return {slug(m[1]) for m in re.finditer(r"^#{1,6}\s+(.+?)\s*#*$", prose(markdown), re.M)}


def prose(markdown: str) -> str:
    """Markdown without its fenced code blocks."""
    return re.sub(r"^```.*?^```[ \t]*$", "", markdown, flags=re.MULTILINE | re.DOTALL)


def links(markdown: str) -> list[str]:
    """The targets of the links and images of Markdown, but in code."""
    text = re.sub(r"`[^`\n]*`", "", prose(markdown))
    found = re.findall(r"\]\(\s*<?([^)\s>]*)>?(?:\s+\"[^\"]*\")?\s*\)", text)
    found += re.findall(r"^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s|$)", text, re.MULTILINE)
    found += re.findall(r"<([a-z][a-z0-9+.-]*:[^>\s]*)>", text)
    found += re.findall(r"\b(?:href|src)\s*=\s*[\"']([^\"']*)[\"']", text)
    return found


def check_link(target: str, report: Report) -> None:
    """A link of README.md: absolute, https, and into this repository where it exists."""
    if not target.startswith("https://"):
        kind = "not https" if re.match(r"[a-z][a-z0-9+.-]*:", target) else "relative"
        report.problem(f"README.md links to {target!r}: {kind}, and it is read outside the repo")
        return
    if not target.startswith(REPOSITORY_URL + "/") and target != REPOSITORY_URL:
        if re.match(r"https://github\.com/cleanrules/", target):
            report.problem(f"README.md links to {target}: not a page this check can verify")
        return
    rest = target.removeprefix(REPOSITORY_URL)
    if rest in ("", "/", "/issues"):
        return
    found = re.fullmatch(r"/(blob|tree)/main/([^#?]+?)/?(?:#(.*))?", rest)
    if not found:
        report.problem(f"README.md links to {target}: not a file or directory on main")
        return
    kind, relative, fragment = found.groups()
    path = REPOSITORY / relative
    if kind == "blob" and not path.is_file():
        report.problem(f"README.md links to {target}: the repository has no file {relative}")
    elif kind == "tree" and not path.is_dir():
        report.problem(f"README.md links to {target}: the repository has no directory {relative}")
    elif fragment and path.suffix == ".md" and fragment not in anchors(path.read_text("utf-8")):
        report.problem(f"README.md links to {target}: {relative} has no heading #{fragment}")


def platforms_table(markdown: str) -> list[str]:
    """The lines of the first table under the heading Platforms."""
    section = re.search(r"^#{2,3} Platforms\n(.*?)(?=^#{1,3} |\Z)", markdown, re.M | re.S)
    if not section:
        return []
    lines = section[1].split("\n")
    rows = [line for line in lines if line.startswith("|")]
    if not rows:
        return []
    start = lines.index(rows[0])
    table = []
    for line in lines[start:]:
        if not line.startswith("|"):
            break
        table.append(line)
    return table


def check_readme(text: str, platforms: list[str], report: Report) -> None:
    """README.md: for Go users, with absolute links, and the same platforms as the guide."""
    source = assemble.README.read_text(encoding="utf-8")
    if text != source:
        report.problem(f"README.md is not {assemble.README.relative_to(REPOSITORY)}")
    targets = links(text)
    for target in targets:
        check_link(target, report)
    if f"go get {MODULE}\n" not in text:
        report.problem(f"README.md does not say go get {MODULE}")
    if not re.search(rf'^import (?:\w+ )?"{re.escape(MODULE)}"$', text, re.MULTILINE):
        report.problem(f"README.md does not import {MODULE}")
    if not re.search(r"^## Platforms$", text, re.MULTILINE):
        report.problem("README.md has no section ## Platforms, which the build errors name")
    table = platforms_table(text)
    guide = platforms_table((assemble.PACKAGE / "README.md").read_text(encoding="utf-8"))
    if not table:
        report.problem("README.md has no table of platforms")
    elif table != guide:
        report.problem("README.md's table of platforms is not that of bindings/go/README.md")
    for platform in platforms:
        if f"`{platform}`" not in "\n".join(table):
            report.problem(f"README.md's table of platforms does not name {platform}")
    for tag in ("cleanrules_musl", "cleanrules_external"):
        if tag not in text:
            report.problem(f"README.md does not explain the tag {tag}")
    blocks = re.findall(r"^```go\n(.*?)^```$", text, re.MULTILINE | re.DOTALL)
    if not any(re.search(r"^package main$", block, re.MULTILINE) for block in blocks):
        report.problem("README.md has no example with package main (smoke.sh runs it)")
    task = re.search(r"\b[A-O]\.\d+[a-z]?\b", prose(text))
    if task:
        report.problem(f"README.md names a task of the plan ({task[0]}), which Go users do not see")
    report.line(f"README.md: {len(targets)} links, all absolute; {len(table) - 2} platform rows")


def check_texts(contents: dict[str, bytes], report: Report) -> None:
    """Every text file: not the former path, no affiliation, no former name."""
    for name, data in sorted(contents.items()):
        if not (name.endswith(TEXT_FILES) or ("/" not in name and name.isupper())):
            continue
        text = data.decode("utf-8", errors="replace")
        if FORMER_PATH in text:
            report.problem(f"{name} names the former module path {FORMER_PATH}")
        found = FORBIDDEN.search(text)
        if found:
            report.problem(f"{name} names {found[0]!r}: no university, affiliation or former name")
        for comment in re.findall(r'^package \w+\s*//\s*import\s+"([^"]+)"', text, re.MULTILINE):
            if comment != MODULE and not comment.startswith(MODULE + "/"):
                report.problem(f"{name}: an import comment names {comment}, not {MODULE}")


def check_sources(contents: dict[str, bytes], platforms: list[str], report: Report) -> None:
    """The Go files, the header, the licence and NOTICE, as assemble.py writes them."""
    for name, text in assemble.go_files().items():
        data = contents.get(name)
        if data is not None and data.decode("utf-8", errors="replace") != text:
            report.problem(f"{name} is not bindings/go/{name} as assemble.py writes it")
    for name, data in contents.items():
        root_go = "/" not in name and name.endswith(".go")
        if root_go and not re.search(rb"^package cleanrules$", data, re.MULTILINE):
            report.problem(f"{name} is not in package cleanrules")
    generated = {
        "vendor.go": assemble.vendor_go(platforms),
        "include/include.go": assemble.empty_package(
            "include", "holds the header of the C ABI of cleanrules, cleanrules.h."
        ),
    }
    for platform in platforms:
        generated[f"lib/{platform}/lib.go"] = assemble.empty_package(
            platform, f"holds the prebuilt static library of cleanrules for {platform}."
        )
    for name, text in generated.items():
        data = contents.get(name)
        if data is not None and data.decode("utf-8", errors="replace") != text:
            report.problem(f"{name} is not as assemble.py writes it")
    for name, source in (
        ("LICENSE", REPOSITORY / "LICENSE"),
        ("NOTICE", REPOSITORY / "NOTICE"),
        ("include/cleanrules.h", assemble.HEADER),
    ):
        data = contents.get(name)
        if data is not None and data != source.read_bytes():
            report.problem(f"{name} is not the repository's {source.relative_to(REPOSITORY)}")


def check(module: Path, platforms: list[str], probes: Path | None, release: str | None) -> Report:
    """Check the module; the report."""
    report = Report()
    contents = check_tree(module, platforms, report)

    def text(name: str) -> str | None:
        data = contents.get(name)
        return data.decode("utf-8", errors="replace") if data is not None else None

    version = check_version(text("VERSION"), release, report)
    if text("go.mod") is not None:
        check_go_mod(text("go.mod") or "", version, report)
    if text("README.md") is not None:
        check_readme(text("README.md") or "", platforms, report)
    check_texts(contents, report)
    check_sources(contents, platforms, report)
    lines, problems = check_libs.check_module(module, platforms, probes)
    for line in lines:
        report.line(line)
    for problem in problems:
        if problem not in report.problems:
            report.problem(problem)
    return report


def main() -> int:
    """Check the module given on the command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("module", type=Path)
    parser.add_argument("--probes", type=Path, help="the probes of the Linux libraries")
    parser.add_argument(
        "--platforms", default="all", help="all (the default), or a comma-separated list"
    )
    parser.add_argument("--release", metavar="VERSION", help="the version being released")
    arguments = parser.parse_args()
    try:
        platforms = check_libs.parse_platforms(arguments.platforms)
    except ValueError as error:
        parser.error(str(error))
    if not arguments.module.is_dir():
        parser.error(f"{arguments.module} is not a directory: run assemble.py")
    try:
        report = check(arguments.module, platforms, arguments.probes, arguments.release)
    except assemble.AssemblyError as problem:
        print(problem, file=sys.stderr)
        return 1
    for line in report.lines:
        print(line)
    for warning in report.warnings:
        print(f"warning: {warning}")
    for problem in report.problems:
        print(problem, file=sys.stderr)
    if not report.problems:
        print(f"{arguments.module}: the module {MODULE} with {len(platforms)} prebuilt libraries")
    return 1 if report.problems else 0


if __name__ == "__main__":
    sys.exit(main())
