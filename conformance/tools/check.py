"""Check the conformance vectors: schema, unique ids and cited spec sections.

uv run --no-project --with jsonschema python conformance/tools/check.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[2]


def anchors() -> set[str]:
    """Every `file.md#anchor` of the spec, with GitHub's heading anchors."""
    found: set[str] = set()
    for path in (ROOT / "spec").glob("*.md"):
        for line in path.read_text(encoding="utf-8").splitlines():
            if heading := re.match(r"^#{1,6} (.+)$", line):
                text = re.sub(r"[^\w\- ]", "", heading[1].lower()).replace(" ", "-")
                found.add(f"{path.name}#{text}")
    return found


def resolves(document: Any, pointer: str) -> bool:
    """Whether a JSON Pointer reaches a value, or a missing member of an existing object."""
    if pointer == "":
        return True
    node = document
    parts = [p.replace("~1", "/").replace("~0", "~") for p in pointer[1:].split("/")]
    for i, part in enumerate(parts):
        last = i == len(parts) - 1
        if isinstance(node, dict):
            if part not in node:
                return last
            node = node[part]
        elif isinstance(node, list) and part.isdigit() and int(part) < len(node):
            node = node[int(part)]
        else:
            return False
    return True


def check_read(vector: dict[str, Any]) -> list[str]:
    """Sanity checks of a hand-written `read` vector's expected error location."""
    given, error = vector["given"], vector["expect"].get("error")
    if not error:
        return []
    found: list[str] = []
    source = given.get("text")
    document = given.get("document")
    if source is not None:
        try:
            document = json.loads(source)
        except ValueError:
            document = None
    if "pointer" in error and document is not None and not resolves(document, error["pointer"]):
        found.append(f"{vector['id']}: pointer {error['pointer']} does not resolve")
    if "line" in error:
        lines = (source or "").split("\n")
        if not (
            source
            and 1 <= error["line"] <= len(lines)
            # One past the last character marks an unexpected end of the text.
            and 1 <= error["column"] <= len(lines[error["line"] - 1].rstrip("\r")) + 1
        ):
            found.append(f"{vector['id']}: line/column outside the text")
    return found


def main() -> int:
    """Validate every vector file; print a summary and return the exit code."""
    schemas: list[Any] = [
        json.loads(p.read_text()) for p in (ROOT / "spec/schemas/v1").glob("*.schema.json")
    ]
    own = json.loads((ROOT / "conformance/vectors.schema.json").read_text())
    registry = Registry().with_resources(
        (s["$id"], Resource.from_contents(s)) for s in [*schemas, own]
    )
    validator = Draft202012Validator(own, registry=registry)
    known = anchors()
    problems: list[str] = []
    ids: set[str] = set()
    count = 0
    for path in sorted((ROOT / "conformance/vectors").rglob("*.json")):
        document = json.loads(path.read_text(encoding="utf-8"))
        name = path.relative_to(ROOT)
        problems.extend(
            f"{name}: {e.json_path}: {e.message[:200]}" for e in validator.iter_errors(document)
        )
        for vector in document.get("vectors", []):
            count += 1
            vid = vector.get("id", "?")
            if vid in ids:
                problems.append(f"{name}: duplicate id {vid}")
            ids.add(vid)
            if not vid.startswith(f"{document.get('area')}/{path.stem}/"):
                problems.append(f"{name}: {vid} is not in <area>/{path.stem}/")
            if vector.get("given", {}).get("kind") == "read":
                problems.extend(check_read(vector))
            problems.extend(
                f"{name}: {vid}: unknown section {ref}"
                for ref in vector.get("spec", [])
                if ref not in known
            )
    print("\n".join(problems), file=sys.stderr)
    print(f"{count} vectors, {len(problems)} problems")
    return 1 if problems or not count else 0


if __name__ == "__main__":
    sys.exit(main())
