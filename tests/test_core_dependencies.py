"""Invariant 4: `import rulepy` loads nothing outside the standard library.

Optional backends (numpy, polars, pandas, fuzzytool, scikit-learn, z3, PyYAML,
rulepy_core, ...) must be imported lazily inside the modules that need them.
"""

import json
import subprocess
import sys

_PROBE = """
import json, sys
before = set(sys.modules)
import {module}
print(json.dumps(sorted(set(sys.modules) - before)))
"""


def _non_stdlib_imports(module: str, *, allowed: str) -> list[str]:
    """Import `module` in a fresh interpreter and return what it loaded beyond the stdlib."""
    out = subprocess.run(
        [sys.executable, "-I", "-c", _PROBE.format(module=module)],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    loaded: list[str] = json.loads(out)
    return sorted(
        name
        for name in loaded
        if (top := name.partition(".")[0]) != allowed and not _is_stdlib(top)
    )


def _is_stdlib(top: str) -> bool:
    # sysconfig's per-platform data module (e.g. _sysconfigdata__darwin_darwin) is
    # generated at build time and missing from sys.stdlib_module_names.
    return top in sys.stdlib_module_names or top.startswith("_sysconfigdata_")


def test_importing_rulepy_loads_only_the_standard_library() -> None:
    assert _non_stdlib_imports("rulepy", allowed="rulepy") == []


def test_probe_detects_third_party_imports() -> None:
    # hypothesis depends on sortedcontainers; the probe must report it, otherwise the
    # test above could pass vacuously.
    leaked = _non_stdlib_imports("hypothesis", allowed="hypothesis")
    assert "sortedcontainers" in {name.partition(".")[0] for name in leaked}
