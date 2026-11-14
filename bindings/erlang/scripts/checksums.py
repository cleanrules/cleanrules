"""Write nif-checksums.config for the prebuilt NIF libraries.

Usage: python3 checksums.py <dir with the archives> [--expect-all]

The file lists each archive's SHA-256 as Erlang terms, one per line, sorted
by name: {"<archive>", "sha256:<hex>"}. scripts/nif.escript reads it in a
package and refuses an archive whose checksum differs. With --expect-all,
every platform of README.md ("Prebuilt libraries") must be there, and
nothing else.
"""

import hashlib
import re
import sys
from pathlib import Path

TARGETS = [
    "aarch64-apple-darwin",
    "aarch64-pc-windows-msvc",
    "aarch64-unknown-linux-gnu",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
]

ASSET = re.compile(
    r"^libcleanrules_nif-v(?P<vsn>.+?)-nif-2\.17-(?P<target>.+)\.(?:so|dll)\.tar\.gz$"
)


def main() -> int:
    """Check the archives and write their checksums; 1 on a problem."""
    directory = Path(sys.argv[1])
    expect_all = "--expect-all" in sys.argv[2:]
    assets = sorted(p for p in directory.iterdir() if p.name.endswith(".tar.gz"))
    found = {}
    for asset in assets:
        match = ASSET.match(asset.name)
        if not match:
            print(f"not a NIF archive: {asset.name}", file=sys.stderr)
            return 1
        found[match["target"]] = match["vsn"]
    if len(set(found.values())) > 1:
        print(f"archives of several versions: {sorted(set(found.values()))}", file=sys.stderr)
        return 1
    if expect_all and sorted(found) != TARGETS:
        print(f"expected the targets {TARGETS}, found {sorted(found)}", file=sys.stderr)
        return 1
    lines = []
    for asset in assets:
        digest = hashlib.sha256(asset.read_bytes()).hexdigest()
        lines.append(f'{{"{asset.name}", "sha256:{digest}"}}.\n')
    (directory / "nif-checksums.config").write_text("".join(lines))
    print("".join(lines), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
