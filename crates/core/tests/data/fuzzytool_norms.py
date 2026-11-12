"""Generate `fuzzytool_norms.json`: fuzzytool's connectives on a grid of degrees.

The core ports fuzzytool's t-norms, s-norms and complements (`spec/ast.md`,
*Logics*), and `tests/fuzzy_reference.rs` checks it against these values.
Regenerate with the fuzzytool version named in the file:

    uv run --no-project --with fuzzytool==0.7.0 python \
        crates/core/tests/data/fuzzytool_norms.py > crates/core/tests/data/fuzzytool_norms.json

Each connective is called once on NumPy arrays holding the whole grid, as
fuzzytool's engines call it. NumPy computes the powers 1, -1, 2 and 0.5 of an
array without `pow`, and the core does the same.
"""

from __future__ import annotations

import json
from importlib.metadata import version

import fuzzytool.norms as n
import numpy as np

GRID = [0.0, 0.1, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 0.99, 1.0]

NAMED = ["min", "prod", "lukasiewicz", "einstein", "hamacher", "drastic", "nilpotent"]
DUAL = {
    "min": "max",
    "prod": "probor",
    "lukasiewicz": "lukasiewicz",
    "einstein": "einstein",
    "hamacher": "hamacher",
    "drastic": "drastic",
    "nilpotent": "nilpotent",
}
FAMILIES = {
    "yager": (n.yager_tnorm, n.yager_snorm, [0.5, 1.0, 2.0, 3.5]),
    "dombi": (n.dombi_tnorm, n.dombi_snorm, [0.5, 1.0, 2.0, 5.0]),
    "frank": (n.frank_tnorm, n.frank_snorm, [0.5, 2.0, 10.0]),
}
COMPLEMENTS = {
    "sugeno": (n.sugeno_complement, [-0.5, 0.0, 2.0]),
    "yager": (n.yager_complement, [0.5, 1.0, 2.0, 3.0]),
}


def pairs(norm) -> list[list[float]]:
    """`[a, b, norm(a, b)]` for every pair of the grid."""
    a = np.array([a for a in GRID for _ in GRID])
    b = np.array([b for _ in GRID for b in GRID])
    return [[x, y, z] for x, y, z in zip(a.tolist(), b.tolist(), norm(a, b).tolist(), strict=True)]


def singles(complement) -> list[list[float]]:
    """`[a, complement(a)]` for every degree of the grid."""
    a = np.array(GRID)
    return [[x, y] for x, y in zip(a.tolist(), complement(a).tolist(), strict=True)]


def main() -> None:
    """Print the reference document."""
    cases = []
    for name in NAMED:
        cases.append({"kind": "tnorm", "name": name, "values": pairs(n.get_tnorm(name))})
        cases.append({"kind": "snorm", "name": name, "values": pairs(n.get_snorm(DUAL[name]))})
    for name, (tnorm, snorm, parameters) in FAMILIES.items():
        for p in parameters:
            cases.append({"kind": "tnorm", "name": name, "parameter": p, "values": pairs(tnorm(p))})
            cases.append({"kind": "snorm", "name": name, "parameter": p, "values": pairs(snorm(p))})
    cases.append({"kind": "complement", "name": "standard", "values": singles(n.complement)})
    for name, (complement, parameters) in COMPLEMENTS.items():
        for p in parameters:
            cases.append(
                {
                    "kind": "complement",
                    "name": name,
                    "parameter": p,
                    "values": singles(complement(p)),
                }
            )
    # One case per line, so a regenerated file diffs by connective.
    print("{")
    source = "fuzzytool " + version("fuzzytool")
    print(f'  "source": {json.dumps(source)},')
    print(f'  "grid": {json.dumps(GRID)},')
    print('  "cases": [')
    print(",\n".join(f"    {json.dumps(case)}" for case in cases))
    print("  ]")
    print("}")


if __name__ == "__main__":
    main()
