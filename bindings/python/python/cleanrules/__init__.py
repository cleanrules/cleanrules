"""cleanrules: an explainable rules engine with one Rust core.

This is the Python binding of the core. It shares the specification, the JSON
format and the conformance vectors with every other language.
"""

from cleanrules._cleanrules import FORMAT_VERSION, __version__

__all__ = ["FORMAT_VERSION", "__version__"]
