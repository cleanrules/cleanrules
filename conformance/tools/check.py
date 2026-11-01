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


EXEMPT_MARKS = ("*Informative", "*Not checked by vectors")


def uncovered(cited: set[str]) -> list[str]:
    """Normative spec sections that no vector cites (`conformance/README.md`, *Coverage*).

    A section is covered when a vector cites it or one of its subsections. It is
    exempt when it, or a section containing it, is a *Differences from the Python v1*
    section or starts with a line marked *Informative* or *Not checked by vectors*.
    Document titles (level 1) are not sections.
    """
    missing: list[str] = []
    for path in sorted((ROOT / "spec").glob("*.md")):
        if path.name == "README.md":
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        headings: list[tuple[int, str, bool]] = []  # level, anchor, exempt
        for i, line in enumerate(lines):
            if heading := re.match(r"^(#{1,6}) (.+)$", line):
                following = next((x for x in lines[i + 1 :] if x.strip()), "")
                exempt = heading[2] == "Differences from the Python v1" or following.startswith(
                    EXEMPT_MARKS
                )
                anchor = re.sub(r"[^\w\- ]", "", heading[2].lower()).replace(" ", "-")
                headings.append((len(heading[1]), f"{path.name}#{anchor}", exempt))
        for i, (level, ref, exempt) in enumerate(headings):
            if level == 1:
                continue
            # Ancestors: the nearest earlier heading of each lower level.
            inherited = exempt
            current = level
            for prior_level, _, prior_exempt in reversed(headings[:i]):
                if prior_level < current:
                    inherited = inherited or (prior_exempt and prior_level > 1)
                    current = prior_level
            descendants = []
            for later_level, later_ref, _ in headings[i + 1 :]:
                if later_level <= level:
                    break
                descendants.append(later_ref)
            if not inherited and ref not in cited and not cited.intersection(descendants):
                missing.append(ref)
    return missing


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
    yaml = given.get("yaml")
    if yaml is not None:
        # YAML is not parsed here: only locations are checked against the text.
        source, document = yaml, None
    elif source is not None:
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
    cited = {
        ref
        for path in (ROOT / "conformance/vectors").rglob("*.json")
        for vector in json.loads(path.read_text(encoding="utf-8"))["vectors"]
        for ref in vector["spec"]
    }
    problems.extend(f"spec section cited by no vector: {ref}" for ref in uncovered(cited))
    print("\n".join(problems), file=sys.stderr)
    print(f"{count} vectors, {len(problems)} problems")
    return 1 if problems or not count else 0


if __name__ == "__main__":
    sys.exit(main())
