"""Check the Go module that assemble.py writes, with its prebuilt libraries.

Usage: python3 check-libs.py <module> [--probes <dir>] [--platforms all|<p1>,<p2>...]

The module must hold, under lib/, exactly the static library of each platform
given (all eight by default) and its empty package (lib.go), and nothing
else. Each library must be an archive with a symbol index that defines the C
ABI (cr_abi_version), whose objects are all of its platform's format and
processor (ELF, Mach-O or COFF, x64 or arm64); the macOS ones may need macOS
11.0 at most (Go's own minimum); a glibc one must ask for some of glibc's
own names (the large-file names such as stat64, which musl 1.2.4 dropped,
or gnu_get_libc_version) and a musl one for none of them. The rest of the module:
go.mod, README.md, LICENSE, NOTICE, vendor.go, include/cleanrules.h and
include/include.go, and the package's Go files, none of them a test, none
needing the tag cleanrules_prebuilt or reaching outside the module
(${SRCDIR}/..).

A static library only meets the C library when it is linked, so the glibc
and musl ones are also checked through probes: --probes holds, for each Linux
platform, <platform>/probe, a C program that the build linked from the
library in the oldest environment supported (manylinux_2_28 for glibc, Alpine
for musl) and ran there, and <platform>/libcleanrules_ffi.a.sha256, the
SHA-256 of the library it was linked from, which must be the module's. A
glibc probe may need glibc 2.28 at most (the symbol versions of its
SHT_GNU_verneed section, as `objdump -T` shows them, but those libgcc_s
names) and must need libc.so.6; a musl one must need musl's libc and no
glibc symbol version.

Standard library only; it prints one line per library, and every problem to
stderr with exit code 1.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

PLATFORMS = [
    "darwin_amd64",
    "darwin_arm64",
    "linux_amd64",
    "linux_arm64",
    "linux_amd64_musl",
    "linux_arm64_musl",
    "windows_amd64",
    "windows_arm64",
]
LIBRARY = "libcleanrules_ffi.a"
MAX_GLIBC = (2, 28)
MAX_MACOS = (11, 0)
SHT_SYMTAB = 2
SHT_DYNAMIC = 6
SHT_GNU_VERNEED = 0x6FFFFFFE
DT_NEEDED = 1
LC_VERSION_MIN_MACOSX = 0x24
LC_BUILD_VERSION = 0x32
# glibc's own symbols, which only a library built for glibc asks for.
GLIBC_ONLY = {"gnu_get_libc_version"}
# The large-file names of glibc, which musl 1.2.4 no longer has.
LFS64 = {"stat64", "fstat64", "lstat64", "fstatat64", "open64", "openat64", "lseek64", "mmap64"}
ARCHES = {"amd64": "x64", "arm64": "arm64"}


@dataclass(frozen=True)
class Expected:
    """What the library of a platform must be."""

    format: str
    arch: str
    libc: str | None


def expected(platform: str) -> Expected:
    """The object format, processor and C library of a platform's library."""
    system, arch, *abi = platform.split("_")
    if system == "darwin":
        return Expected("Mach-O", ARCHES[arch], None)
    if system == "windows":
        return Expected("COFF", ARCHES[arch], None)
    return Expected("ELF", ARCHES[arch], abi[0] if abi else "gnu")


# ---- Archives.


def members(data: bytes) -> list[tuple[str, bytes]]:
    """The members of an ar archive (GNU or BSD names), symbol tables included."""
    if data[:8] != b"!<arch>\n":
        raise ValueError("not an ar archive")
    found = []
    names = b""
    at = 8
    while at + 60 <= len(data):
        header = data[at : at + 60]
        if header[58:60] != b"`\n":
            raise ValueError(f"a damaged member header at byte {at}")
        name = header[:16].decode("latin-1").rstrip()
        size = int(header[48:58].decode("ascii"))
        body = data[at + 60 : at + 60 + size]
        if name.startswith("#1/"):
            length = int(name[3:])
            name = body[:length].rstrip(b"\0").decode("latin-1")
            body = body[length:]
        elif name == "//":
            names = body
        elif name.startswith("/") and name[1:].isdigit():
            offset = int(name[1:])
            name = names[offset : names.index(b"\n", offset)].decode("latin-1").rstrip("/")
        else:
            name = name.rstrip("/") if name not in ("/", "//", "/SYM64/") else name
        found.append((name, body))
        at += 60 + size + (size & 1)
    return found


SYMBOL_TABLES = {
    "/",
    "/SYM64/",
    "__.SYMDEF",
    "__.SYMDEF SORTED",
    "__.SYMDEF_64",
    "__.SYMDEF_64 SORTED",
}


def object_kind(data: bytes) -> tuple[str, str] | None:
    """(format, processor) of an object file, or None if it is none of the three."""
    if len(data) >= 20 and data[:4] == b"\x7fELF":
        machine = struct.unpack_from("<H", data, 18)[0]
        return "ELF", {0x3E: "x64", 0xB7: "arm64"}.get(machine, "other")
    if len(data) >= 8 and struct.unpack_from("<I", data, 0)[0] == 0xFEEDFACF:
        cpu = struct.unpack_from("<I", data, 4)[0]
        return "Mach-O", {0x01000007: "x64", 0x0100000C: "arm64"}.get(cpu, "other")
    coff = {0x8664: "x64", 0xAA64: "arm64"}
    if len(data) >= 8 and data[:4] == b"\0\0\xff\xff":
        # A big object, or a short import (the raw-dylib imports of Rust's
        # standard library): the machine follows the signature and version.
        return "COFF", coff.get(struct.unpack_from("<H", data, 6)[0], "other")
    if len(data) >= 20:
        machine = struct.unpack_from("<H", data, 0)[0]
        if machine in coff:
            return "COFF", coff[machine]
    return None


# ---- ELF.


@dataclass(frozen=True)
class Section:
    """An ELF section header: what this script reads of it."""

    type: int
    offset: int
    size: int
    link: int
    info: int
    entsize: int


def sections(data: bytes) -> list[Section]:
    """The section headers of a 64-bit little-endian ELF object."""
    shoff = struct.unpack_from("<Q", data, 0x28)[0]
    shentsize, shnum = struct.unpack_from("<HH", data, 0x3A)
    found = []
    for index in range(shnum):
        _, kind, _, _, offset, size, link, info, _, entsize = struct.unpack_from(
            "<IIQQQQIIQQ", data, shoff + index * shentsize
        )
        found.append(Section(kind, offset, size, link, info, entsize))
    return found


def text(data: bytes, strings: Section, offset: int) -> str:
    """A NUL-terminated string of a string table."""
    start = strings.offset + offset
    return data[start : data.index(b"\0", start)].decode("latin-1")


def undefined_symbols(data: bytes) -> set[str]:
    """The undefined symbols of an ELF relocatable object."""
    table = sections(data)
    found = set()
    for symtab in table:
        if symtab.type != SHT_SYMTAB:
            continue
        strings = table[symtab.link]
        for at in range(symtab.offset, symtab.offset + symtab.size, 24):
            name, _, _, shndx = struct.unpack_from("<IBBH", data, at)
            if shndx == 0 and name:
                found.add(text(data, strings, name))
    return found


def needed_versions(data: bytes) -> list[tuple[str, str]]:
    """(file, version) of each symbol version the object needs, from SHT_GNU_verneed."""
    table = sections(data)
    needed = []
    for verneed in table:
        if verneed.type != SHT_GNU_VERNEED:
            continue
        strings = table[verneed.link]
        entry = verneed.offset
        for _ in range(verneed.info):
            count, file_offset, aux_offset, next_entry = struct.unpack_from("<xxHIII", data, entry)
            file = text(data, strings, file_offset)
            aux = entry + aux_offset
            for _ in range(count):
                name, next_aux = struct.unpack_from("<8xII", data, aux)
                needed.append((file, text(data, strings, name)))
                aux += next_aux
            entry += next_entry
    return needed


def needed_libraries(data: bytes) -> list[str]:
    """The DT_NEEDED entries of the object's dynamic section."""
    table = sections(data)
    found = []
    for dynamic in table:
        if dynamic.type != SHT_DYNAMIC:
            continue
        strings = table[dynamic.link]
        for at in range(dynamic.offset, dynamic.offset + dynamic.size, 16):
            tag, value = struct.unpack_from("<qQ", data, at)
            if tag == DT_NEEDED:
                found.append(text(data, strings, value))
    return found


def glibc_versions(data: bytes) -> list[tuple[int, int]]:
    """The glibc versions the object needs, (major, minor), but those libgcc_s names."""
    versions = []
    # libgcc_s names two of its own symbols GLIBC_2.0 (`__register_frame_info`,
    # on Alpine too), so only what the other libraries ask counts.
    for file, version in needed_versions(data):
        match = re.fullmatch(r"GLIBC_(\d+)\.(\d+)(?:\.\d+)?", version)
        if match and not file.startswith("libgcc_s.so"):
            versions.append((int(match[1]), int(match[2])))
    return versions


# ---- Mach-O.


def macos_minimum(data: bytes) -> tuple[int, int] | None:
    """The macOS version a Mach-O object needs, from its load commands."""
    ncmds = struct.unpack_from("<I", data, 16)[0]
    at = 32
    for _ in range(ncmds):
        command, size = struct.unpack_from("<II", data, at)
        if command == LC_BUILD_VERSION:
            minos = struct.unpack_from("<I", data, at + 12)[0]
            return minos >> 16, (minos >> 8) & 0xFF
        if command == LC_VERSION_MIN_MACOSX:
            version = struct.unpack_from("<I", data, at + 8)[0]
            return version >> 16, (version >> 8) & 0xFF
        at += size
    return None


# ---- Checks.


def check_library(platform: str, data: bytes) -> tuple[list[str], str]:
    """The problems of a platform's library, and what to print about it."""
    want = expected(platform)
    where = f"lib/{platform}/{LIBRARY}"
    try:
        found = members(data)
    except ValueError as problem:
        return [f"{where}: {problem}"], "unreadable"
    problems = []
    tables = [body for name, body in found if name in SYMBOL_TABLES]
    if not tables:
        problems.append(f"{where} has no symbol index (ranlib)")
    elif not any(b"cr_abi_version\0" in body for body in tables):
        problems.append(f"{where} does not define cr_abi_version")
    objects = [(name, body) for name, body in found if name not in SYMBOL_TABLES and name != "//"]
    if not objects:
        problems.append(f"{where} holds no objects")
    wrong = []
    undefined: set[str] = set()
    newest_macos = None
    for name, body in objects:
        kind = object_kind(body)
        if kind != (want.format, want.arch):
            wrong.append(f"{name} ({kind[0] + ' ' + kind[1] if kind else 'not an object'})")
            continue
        if want.format == "ELF":
            undefined |= undefined_symbols(body)
        if want.format == "Mach-O":
            minimum = macos_minimum(body)
            if minimum and (newest_macos is None or minimum > newest_macos):
                newest_macos = minimum
    if wrong:
        problems.append(
            f"{where}: {len(wrong)} members are not {want.format} for {want.arch}: {wrong[:5]}"
        )
    detail = f"{len(objects)} {want.format} {want.arch} objects"
    if newest_macos is not None:
        detail += f", macOS {newest_macos[0]}.{newest_macos[1]}"
        if newest_macos > MAX_MACOS:
            problems.append(
                f"{where} needs macOS {newest_macos[0]}.{newest_macos[1]}, over"
                f" {MAX_MACOS[0]}.{MAX_MACOS[1]}"
            )
    if want.libc == "gnu":
        if not undefined & (GLIBC_ONLY | LFS64):
            problems.append(
                f"{where} asks for none of glibc's own symbols {sorted(GLIBC_ONLY | LFS64)}"
            )
        detail += ", glibc"
    if want.libc == "musl":
        asked = sorted(undefined & (GLIBC_ONLY | LFS64))
        if asked:
            problems.append(f"{where} asks for glibc's symbols {asked}: not built for musl")
        detail += ", musl"
    return problems, detail


def check_probe(platform: str, probes: Path, sha: str) -> tuple[list[str], str]:
    """The problems of a Linux platform's probe, and what to print about it."""
    want = expected(platform)
    probe = probes / platform / "probe"
    recorded = probes / platform / f"{LIBRARY}.sha256"
    if not probe.is_file() or not recorded.is_file():
        return [f"{platform}: {probes} lacks {platform}/probe or its {LIBRARY}.sha256"], ""
    problems = []
    if recorded.read_text().split()[:1] != [sha]:
        problems.append(f"{platform}: the probe was linked from another library than the module's")
    data = probe.read_bytes()
    if not data.startswith(b"\x7fELF") or object_kind(data) != ("ELF", want.arch):
        return [*problems, f"{platform}: the probe is not an ELF program for {want.arch}"], ""
    glibc = glibc_versions(data)
    libraries = needed_libraries(data)
    detail = f"probe needs {', '.join(libraries)}"
    if want.libc == "gnu":
        if not glibc:
            problems.append(f"{platform}: the probe asks for no glibc symbol versions")
        elif max(glibc) > MAX_GLIBC:
            highest = ".".join(map(str, max(glibc)))
            problems.append(f"{platform}: the probe needs glibc {highest}, over 2.28")
        else:
            detail += f", glibc {'.'.join(map(str, max(glibc)))}"
        if "libc.so.6" not in libraries:
            problems.append(f"{platform}: the probe does not need libc.so.6 ({libraries})")
    if want.libc == "musl":
        if glibc:
            problems.append(f"{platform}: the probe asks for glibc symbol versions")
        if not any(name.startswith("libc.musl-") or name == "libc.so" for name in libraries):
            problems.append(f"{platform}: the probe does not need musl's libc ({libraries})")
    return problems, detail


def check_module(
    module: Path, platforms: list[str], probes: Path | None
) -> tuple[list[str], list[str]]:
    """What to print about a module's libraries, and its problems."""
    problems: list[str] = []
    lines: list[str] = []
    lib = module / "lib"
    allowed = {f"lib/{p}/{name}" for p in platforms for name in (LIBRARY, "lib.go")}
    present = (
        {path.relative_to(module).as_posix() for path in lib.rglob("*") if path.is_file()}
        if lib.is_dir()
        else set()
    )
    for name in sorted(present - allowed):
        problems.append(f"the module holds what it must not under lib/: {name}")
    for platform in platforms:
        path = lib / platform / LIBRARY
        if not path.is_file() or not (lib / platform / "lib.go").is_file():
            problems.append(f"{platform}: the module lacks lib/{platform}/{LIBRARY} or its lib.go")
            continue
        data = path.read_bytes()
        sha = hashlib.sha256(data).hexdigest()
        found, detail = check_library(platform, data)
        problems += found
        if probes is not None and expected(platform).libc:
            found, more = check_probe(platform, probes, sha)
            problems += found
            detail += f"; {more}" if more else ""
        lines.append(f"lib/{platform}/{LIBRARY}: {len(data)} bytes, sha256 {sha[:16]}..., {detail}")
    for required in [
        "go.mod",
        "README.md",
        "LICENSE",
        "NOTICE",
        "vendor.go",
        "include/cleanrules.h",
        "include/include.go",
    ]:
        if not (module / required).is_file():
            problems.append(f"the module lacks {required}")
    for file in sorted(module.glob("*.go")):
        source = file.read_text(encoding="utf-8")
        if file.name.endswith("_test.go"):
            problems.append(f"the module holds a test, {file.name}")
        if any(
            line.startswith("//go:build") and "cleanrules_prebuilt" in line
            for line in source.split("\n")
        ):
            problems.append(f"{file.name} still needs the tag cleanrules_prebuilt")
        if "${SRCDIR}/../" in source:
            problems.append(f"{file.name} links the repository's target directory")
    vendor = (module / "vendor.go").read_text() if (module / "vendor.go").is_file() else ""
    for platform in platforms:
        if f'/lib/{platform}"' not in vendor:
            problems.append(f"vendor.go does not import lib/{platform}")
    return lines, problems


def parse_platforms(value: str) -> list[str]:
    """The platforms of a --platforms value: all or a comma-separated list."""
    if value == "all":
        return PLATFORMS
    unknown = set(value.split(",")) - set(PLATFORMS)
    if unknown:
        raise ValueError(f"unknown platforms {sorted(unknown)}; they are {PLATFORMS}")
    return [p for p in PLATFORMS if p in value.split(",")]


def main() -> int:
    """Check the module given on the command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("module", type=Path)
    parser.add_argument("--probes", type=Path, help="the probes of the Linux libraries")
    parser.add_argument(
        "--platforms", default="all", help="all (the default), or a comma-separated list"
    )
    arguments = parser.parse_args()
    try:
        platforms = parse_platforms(arguments.platforms)
    except ValueError as error:
        parser.error(str(error))

    lines, problems = check_module(arguments.module, platforms, arguments.probes)
    for line in lines:
        print(line)
    for problem in problems:
        print(problem, file=sys.stderr)
    if not problems:
        print(f"{arguments.module}: {len(platforms)} prebuilt libraries, as expected")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
