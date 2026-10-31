"""Snapshot assertions for human-readable output (explanations, reports).

Snapshots live in tests/snapshots/<name>.txt. Set RULEPY_UPDATE_SNAPSHOTS=1 to
rewrite them; a missing snapshot is written and the test fails, so new snapshots
are always reviewed before they are committed.
"""

import os
from pathlib import Path

import pytest

SNAPSHOTS = Path(__file__).parent / "snapshots"


def assert_snapshot(name: str, text: str) -> None:
    path = SNAPSHOTS / f"{name}.txt"
    expected = text if text.endswith("\n") else text + "\n"
    if os.environ.get("RULEPY_UPDATE_SNAPSHOTS") == "1":
        path.write_text(expected, encoding="utf-8")
        return
    if not path.exists():
        path.write_text(expected, encoding="utf-8")
        pytest.fail(f"snapshot {path.name} was created; review it and run the tests again")
    assert path.read_text(encoding="utf-8") == expected
