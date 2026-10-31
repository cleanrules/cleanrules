"""Check the JSON Schemas of the format against their examples.

Every `*.schema.json` in `v1/` must be a valid draft 2020-12 schema. Each example
under `v1/examples/<name>/valid/` must validate against `<name>.schema.json`, and
each one under `invalid/` must not. Examples only cover what a schema can check;
construction rules such as and/or flattening are tested by conformance vectors.

Run from the repository root, without adding a dependency to the project:

    uv run --no-project --with jsonschema python spec/schemas/check.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).parent / "v1"


def load(path: Path) -> Any:
    """Parse a JSON file, rejecting duplicate keys as the format requires."""

    def no_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        keys = [key for key, _ in pairs]
        duplicates = {key for key in keys if keys.count(key) > 1}
        if duplicates:
            raise ValueError(f"{path}: duplicate keys {sorted(duplicates)}")
        return dict(pairs)

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_duplicates)


def main() -> int:
    """Check every schema and example; return the process exit code."""
    schemas = {
        path.name.removesuffix(".schema.json"): load(path)
        for path in sorted(ROOT.glob("*.schema.json"))
    }
    for schema in schemas.values():
        Draft202012Validator.check_schema(schema)
    registry = Registry().with_resources(
        (schema["$id"], Resource.from_contents(schema)) for schema in schemas.values()
    )

    failures: list[str] = []
    checked = 0
    for name, schema in schemas.items():
        validator = Draft202012Validator(schema, registry=registry)
        for expect_valid in (True, False):
            folder = ROOT / "examples" / name / ("valid" if expect_valid else "invalid")
            for path in sorted(folder.glob("*.json")):
                checked += 1
                errors = list(validator.iter_errors(load(path)))
                if expect_valid and errors:
                    failures.append(f"{path}: should be valid, but {errors[0].message}")
                elif not expect_valid and not errors:
                    failures.append(f"{path}: should be invalid, but it validates")

    for failure in failures:
        print(failure, file=sys.stderr)
    print(f"{len(schemas)} schemas, {checked} examples, {len(failures)} failures")
    return 1 if failures or not checked else 0


if __name__ == "__main__":
    sys.exit(main())
