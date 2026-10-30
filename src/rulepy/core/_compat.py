"""Detection of third-party scalar types without importing their packages (invariant 4)."""

from __future__ import annotations

import math
from typing import Any


def is_numpy_bool(value: object) -> bool:
    """Return whether `value` is a `numpy.bool_` (named `numpy.bool` in numpy 2)."""
    cls = type(value)
    return cls.__module__ == "numpy" and cls.__name__ in ("bool", "bool_")


_PLAIN = (str, bytes, int, bool)
_PANDAS_MISSING = ("NAType", "NaTType")


def is_null(value: object) -> bool:
    """Return whether a data value counts as missing (see `docs/spec/ast.md`).

    Missing means `None`, any NaN (Python, numpy, `Decimal`), `pandas.NA` and
    `pandas.NaT`. Values from data are passed through this check once, when a field is
    read, so the rest of the engine only ever sees `None` for missing.
    """
    if value is None:
        return True
    if isinstance(value, float):
        return math.isnan(value)
    if isinstance(value, _PLAIN):
        return False
    cls = type(value)
    if cls.__module__.startswith("pandas") and cls.__name__ in _PANDAS_MISSING:
        return True
    # NaN-like values (numpy floats, Decimal('NaN'), numpy NaT) are unequal to
    # themselves; only a real boolean answer counts, never an array or a node.
    try:
        unequal: Any = value != value
    except Exception:
        return False
    return (type(unequal) is bool or is_numpy_bool(unequal)) and bool(unequal)
