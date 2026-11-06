"""Check the native libraries inside a cleanrules jar.

Usage: python3 check-jar.py <jar> [--platforms all|none|<p1>,<p2>...]

The jar must hold, under native/, exactly the library of each platform given
(all eight by default: those of the npm packages) and its .sha256, and
nothing else; each library must be a binary of its system and processor, with
the SHA-256 its .sha256 gives; a glibc one may need glibc 2.28 at most (the
symbol versions it needs, from its SHT_GNU_verneed section, as `objdump -T`
shows them) and must need libc.so.6, and a musl one must need musl's libc and
no glibc symbol version. The manifest's Cleanrules-Native-Platforms must list
the same platforms, and the jar must hold the module descriptor and the
licence and NOTICE. Standard library only; it prints one line per library,
and every problem to stderr with exit code 1.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import struct
import sys
import zipfile
from dataclasses import dataclass

PLATFORMS = [
    "darwin-x64",
    "darwin-arm64",
    "linux-x64-gnu",
    "linux-arm64-gnu",
    "linux-x64-musl",
    "linux-arm64-musl",
    "win32-x64-msvc",
    "win32-arm64-msvc",
]
MAX_GLIBC = (2, 28)
SHT_DYNAMIC = 6
SHT_GNU_VERNEED = 0x6FFFFFFE
DT_NEEDED = 1


@dataclass(frozen=True)
class Expected:
    """What the library of a platform must be."""

    file: str
    format: str
    arch: str
    libc: str | None


def expected(platform: str) -> Expected:
    """The file name, binary format, processor and C library of a platform's library."""
    os, arch, *abi = platform.split("-")
    if os == "darwin":
        return Expected("libcleanrules_ffi.dylib", "Mach-O", arch, None)
    if os == "win32":
        return Expected("cleanrules_ffi.dll", "PE", arch, None)
    return Expected("libcleanrules_ffi.so", "ELF", arch, abi[0] if abi else None)


def binary_kind(data: bytes) -> tuple[str, str] | None:
    """(format, processor) of an executable's header, or None if it is none of the three formats."""
    if len(data) >= 20 and data[:4] == b"\x7fELF":
        machine = struct.unpack_from("<H", data, 18)[0]
        return "ELF", {0x3E: "x64", 0xB7: "arm64"}.get(machine, "other")
    if len(data) >= 8 and struct.unpack_from("<I", data, 0)[0] == 0xFEEDFACF:
        cpu = struct.unpack_from("<I", data, 4)[0]
        return "Mach-O", {0x01000007: "x64", 0x0100000C: "arm64"}.get(cpu, "other")
    if len(data) >= 64 and data[:2] == b"MZ":
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        if pe + 6 <= len(data) and data[pe : pe + 4] == b"PE\0\0":
            machine = struct.unpack_from("<H", data, pe + 4)[0]
            return "PE", {0x8664: "x64", 0xAA64: "arm64"}.get(machine, "other")
    return None


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
        at = shoff + index * shentsize
        _, kind, _, _, offset, size, link, info, _, entsize = struct.unpack_from(
            "<IIQQQQIIQQ", data, at
        )
        found.append(Section(kind, offset, size, link, info, entsize))
    return found


def text(data: bytes, strings: Section, offset: int) -> str:
    """A NUL-terminated string of a string table."""
    start = strings.offset + offset
    return data[start : data.index(b"\0", start)].decode("latin-1")


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


def check_library(platform: str, data: bytes) -> tuple[list[str], str]:
    """The problems of a platform's library, and what to print about it."""
    want = expected(platform)
    kind = binary_kind(data)
    if kind != (want.format, want.arch):
        found = f"{kind[0]} for {kind[1]}" if kind else "no known binary"
        problem = f"{platform}: {want.file} is {found}, not {want.format} for {want.arch}"
        return [problem], f"{found} (wrong)"
    problems = []
    detail = f"{want.format} {want.arch}"
    if want.format == "ELF":
        glibc = glibc_versions(data)
        libraries = needed_libraries(data)
        if want.libc == "gnu":
            if not glibc:
                problems.append(f"{platform}: {want.file} asks for no glibc symbol versions")
            elif max(glibc) > MAX_GLIBC:
                highest = ".".join(map(str, max(glibc)))
                most = ".".join(map(str, MAX_GLIBC))
                problems.append(f"{platform}: {want.file} needs glibc {highest}, over {most}")
            if "libc.so.6" not in libraries:
                problems.append(
                    f"{platform}: {want.file} does not need libc.so.6 (it needs {libraries})"
                )
            if glibc:
                detail += f", needs glibc {'.'.join(map(str, max(glibc)))}"
        if want.libc == "musl":
            if glibc:
                problems.append(f"{platform}: {want.file} asks for glibc symbol versions")
            if not any(name.startswith("libc.musl-") or name == "libc.so" for name in libraries):
                problems.append(
                    f"{platform}: {want.file} does not need musl's libc (it needs {libraries})"
                )
            detail += ", musl"
        detail += f", needs {', '.join(libraries)}"
    return problems, detail


def main() -> int:
    """Check the jar given on the command line."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("jar")
    parser.add_argument(
        "--platforms", default="all", help="all (the default), none, or a comma-separated list"
    )
    arguments = parser.parse_args()
    if arguments.platforms == "all":
        platforms = PLATFORMS
    elif arguments.platforms == "none":
        platforms = []
    else:
        platforms = [p for p in PLATFORMS if p in arguments.platforms.split(",")]
        unknown = set(arguments.platforms.split(",")) - set(PLATFORMS)
        if unknown:
            parser.error(f"unknown platforms {sorted(unknown)}; they are {PLATFORMS}")

    problems: list[str] = []
    lines: list[str] = []
    with zipfile.ZipFile(arguments.jar) as jar:
        names = set(jar.namelist())
        allowed = {"native/"} if platforms else set()
        for platform in platforms:
            file = expected(platform).file
            library = f"native/{platform}/{file}"
            allowed |= {f"native/{platform}/", library, f"{library}.sha256"}
            if library not in names or f"{library}.sha256" not in names:
                problems.append(f"{platform}: the jar lacks {library} or its .sha256")
                continue
            data = jar.read(library)
            sha = hashlib.sha256(data).hexdigest()
            checksum = jar.read(f"{library}.sha256").decode("ascii")
            if checksum != f"{sha}  {file}\n":
                problems.append(
                    f"{platform}: {library}.sha256 is {checksum!r}, not '{sha}  {file}\\n'"
                )
            found, detail = check_library(platform, data)
            problems += found
            lines.append(f"{library}: {len(data)} bytes, sha256 {sha[:16]}..., {detail}")
        extra = sorted(name for name in names if name.startswith("native/") and name not in allowed)
        if extra:
            problems.append(f"the jar holds what it must not under native/: {extra}")
        for required in [
            "module-info.class",
            "META-INF/LICENSE",
            "META-INF/NOTICE",
            "META-INF/MANIFEST.MF",
        ]:
            if required not in names:
                problems.append(f"the jar lacks {required}")
        if "META-INF/MANIFEST.MF" in names:
            manifest = (
                jar.read("META-INF/MANIFEST.MF")
                .decode("utf-8")
                .replace("\r\n", "\n")
                .replace("\n ", "")
            )
            match = re.search(r"^Cleanrules-Native-Platforms: (.*)$", manifest, re.MULTILINE)
            listed = match[1].split() if match else None
            if listed != (platforms or ["none"]):
                problems.append(
                    f"the manifest lists the platforms {listed}, not {platforms or ['none']}"
                )

    for line in lines:
        print(line)
    for problem in problems:
        print(problem, file=sys.stderr)
    if not problems:
        print(f"{arguments.jar}: {len(platforms)} native libraries, as expected")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
