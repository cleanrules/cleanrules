"""Bundles (`spec/bundles.md`): reproducible packages of rules.

    bundle = cleanrules.Bundle(
        rules,
        id="acme/discounts",
        revision="2026-11-16.1",
        cases=[("cases/discounts.json", Path("cases/discounts.json").read_bytes())],
    )
    cleanrules.dump(bundle, "discounts.bundle.json")

    old = cleanrules.load("discounts-1.bundle.json")
    print(old.compare(bundle).relation)  # e.g. "other_rules_same_engine"

A bundle holds the rules themselves (a rule set or a decision table), the
identifier and revision its author gives them, their rule set digest, the
engine and limits that made it, the contract of their input and output, and
the digests of its files of test cases. The same bundle on the same engine
gives the same decisions, explanations and errors in every language.
"""

from __future__ import annotations

import enum
import json
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from types import MappingProxyType
from typing import Any, NoReturn, cast

from cleanrules import _cleanrules as _native
from cleanrules._documents import (
    _check_actions,  # pyright: ignore[reportPrivateUsage]
    _ruleset,  # pyright: ignore[reportPrivateUsage]
    dumps,
    loads,
)
from cleanrules._rules import RuleSet
from cleanrules._tables import DecisionTable

__all__ = ["Bundle", "BundleComparison", "BundleEngine", "BundleRelation", "CaseFile"]


class BundleRelation(enum.StrEnum):
    """How two bundles relate: their rules and their engine (`spec/bundles.md`)."""

    CONFLICT = "conflict"
    """The same identifier and revision with other rules: a revision named two rule sets."""
    SAME_RULES_SAME_ENGINE = "same_rules_same_engine"
    """The same rules on the same engine and limits."""
    SAME_RULES_OTHER_ENGINE = "same_rules_other_engine"
    """The same rules on another engine, version or limits: changes come from the engine."""
    OTHER_RULES_SAME_ENGINE = "other_rules_same_engine"
    """Other rules on the same engine and limits: changes come from the rules."""
    OTHER_RULES_OTHER_ENGINE = "other_rules_other_engine"
    """Other rules on another engine, version or limits."""


@dataclass(frozen=True, slots=True)
class BundleEngine:
    """The engine and limits that made a bundle (`spec/bundles.md`, *Engine*).

    Attributes:
        name: The engine's name, `"cleanrules"` for this one.
        version: Its exact version, a Semantic Versioning version.
        limits: The limits it ran with, read-only, in the order of the
            specification: `max_size`, `max_nesting`, `max_depth`,
            `max_path_segments`, `max_table_size` and `max_values`.
        is_current: Whether it is this engine: the same name and version as
            `cleanrules.__version__`, whatever the limits.
    """

    name: str
    version: str
    limits: Mapping[str, int]
    is_current: bool

    def __hash__(self) -> int:
        return hash((self.name, self.version, tuple(self.limits.items()), self.is_current))


@dataclass(frozen=True, slots=True)
class CaseFile:
    """A file of test cases a bundle refers to (`spec/bundles.md`, *Cases*).

    Attributes:
        path: The file's path, relative to the bundle's directory, with `/`
            between segments.
        digest: The SHA-256 digest of the file's bytes, `sha256:` and 64
            lowercase hexadecimal digits.
    """

    path: str
    digest: str

    def matches(self, data: bytes | str) -> bool:
        """Whether `data` are the bytes the bundle was made with.

        Args:
            data: The file's content; a `str` is encoded as UTF-8.

        Raises:
            FormatError: If `path` or `digest` does not have its form.
        """
        return _native.case_matches(self.path, self.digest, _bytes(data, "data"))


@dataclass(frozen=True, slots=True)
class BundleComparison:
    """The comparison of two bundles (`spec/bundles.md`, *Comparing bundles*).

    Attributes:
        relation: How the rules and the engines relate.
        identical: Whether the two bundles are the same: the same digest.
        differences: What differs, in the order of the specification: `id`,
            `revision`, `rules`, `source`, `engine.name`, `engine.version`,
            `engine.limits.<name>`, `contract.input`, `contract.output` and
            `cases`. Empty exactly when the bundles are identical.
    """

    relation: BundleRelation
    identical: bool
    differences: tuple[str, ...]

    def to_dict(self) -> dict[str, Any]:
        """The comparison's JSON form: `relation`, `identical` and `differences`."""
        return {
            "relation": self.relation.value,
            "identical": self.identical,
            "differences": list(self.differences),
        }


class Bundle:
    """A reproducible package of rules (`spec/bundles.md`).

        bundle = Bundle(rules, id="acme/discounts", revision="3")

    Making a bundle records the rules, their rule set digest, this engine's
    name and version, the limits given (the defaults for those not given),
    the contract derived from the rules, and the digest of each case file's
    bytes. The same rules, identifier, revision, cases and limits always give
    the same bundle, byte for byte: nothing comes from a clock or a random
    source.

    Read one with `cleanrules.load` or `cleanrules.loads` (JSON or YAML), and
    write it with `dump` and `dumps`; `canonical_bytes` and `digest` take it
    too. A bundle read from a file may have been made by another engine or
    version, and is reported as such in `engine`. Bundles are immutable,
    equal when their digests are, and pickle as their JSON document.

    Args:
        rules: The rules: a rule set, or a decision table. A single rule is
            given as `RuleSet([rule])`.
        id: The stable identifier of the rules, such as `"acme/discounts"`:
            lowercase segments separated by `/`, up to 128 characters.
        revision: One immutable revision of them, such as `"3"`,
            `"2026-11-16.1"` or `"v1.2.0"`, up to 128 characters.
        cases: The files of test cases, as `(path, content)` pairs or a
            mapping of paths to contents, in the order they run; a `str`
            content is encoded as UTF-8. The bundle keeps each path and the
            digest of its bytes, not the content.
        omit_actions: Bundle a rule set whose rules hold actions given as code
            without those actions, instead of refusing, as `dumps` does.
        max_size: The largest JSON document, in bytes, to record (16 MiB).
        max_nesting: The deepest nesting of arrays and objects (1024).
        max_depth: The deepest condition or payload value (256).
        max_path_segments: The longest field path (256).
        max_table_size: The most cells and nodes of a decision table
            (2,097,152).
        max_values: The most values read from one input's native data
            (1,000,000).

    Raises:
        FormatError: For an identifier, revision or case path that does not
            have its form, or a case path given twice.
        UnsupportedError: For a rule with an action given as code, unless
            `omit_actions` is true, with a `pointer` to it; or a limit above
            2**53 - 1, which a bundle cannot record.
        TypeError: For rules that are not a `RuleSet` or a `DecisionTable`,
            or an identifier, revision, path or content of another type.
        ValueError: For a limit below 1.
    """

    __slots__ = ("_digest", "_native", "_rules")

    _native: _native.Bundle
    _rules: RuleSet | DecisionTable | None
    _digest: str | None

    def __init__(
        self,
        rules: RuleSet | DecisionTable,
        *,
        id: str,
        revision: str,
        cases: Iterable[tuple[str, bytes | str]] | Mapping[str, bytes | str] = (),
        omit_actions: bool = False,
        max_size: int | None = None,
        max_nesting: int | None = None,
        max_depth: int | None = None,
        max_path_segments: int | None = None,
        max_table_size: int | None = None,
        max_values: int | None = None,
    ) -> None:
        if isinstance(rules, RuleSet):
            _check_actions(list(rules), "/bundle/ruleset/rules/{}", omit_actions)
            native_rules: _native.RuleSet | _native.DecisionTable = rules._native  # pyright: ignore[reportPrivateUsage]
        elif isinstance(rules, DecisionTable):  # pyright: ignore[reportUnnecessaryIsInstance]
            native_rules = rules._native  # pyright: ignore[reportPrivateUsage]
        else:
            raise TypeError(
                "a bundle holds a RuleSet or a DecisionTable, not "
                f"{type(rules).__name__}; give a single rule as RuleSet([rule])"
            )
        for name, text in (("id", id), ("revision", revision)):
            if not isinstance(text, str):  # pyright: ignore[reportUnnecessaryIsInstance]
                raise TypeError(f"{name} must be a str, got {type(text).__name__}")
        limits = {
            "max_size": max_size,
            "max_nesting": max_nesting,
            "max_depth": max_depth,
            "max_path_segments": max_path_segments,
            "max_table_size": max_table_size,
            "max_values": max_values,
        }
        for name, limit in limits.items():
            # 0 goes to the core, which refuses to record it (UnsupportedError);
            # a negative limit is no limit at all.
            if limit is not None and limit < 0:
                raise ValueError(f"{name} must be at least 0, got {limit}")
        native = _native.make_bundle(native_rules, id, revision, _cases(cases), **limits)
        self._set(native)

    @classmethod
    def _from_native(cls, native: _native.Bundle) -> Bundle:
        bundle = object.__new__(cls)
        bundle._set(native)
        return bundle

    def _set(self, native: _native.Bundle) -> None:
        set_ = object.__setattr__
        set_(self, "_native", native)
        # The rules are built when first asked for: a bundle may be read only
        # to be compared or written again.
        set_(self, "_rules", None)
        set_(self, "_digest", None)

    @property
    def id(self) -> str:
        """The stable identifier of the rules."""
        return self._native.id

    @property
    def revision(self) -> str:
        """The revision of the rules."""
        return self._native.revision

    @property
    def rules(self) -> RuleSet | DecisionTable:
        """The rules as the bundle holds them: a `RuleSet` or a `DecisionTable`.

        A rule's action is its name, unbound, as `loads` gives it: a bundle
        holds no code. Bind the rule set with `RuleSet.bind` to `run` it.
        """
        if self._rules is None:
            kind, content = self._native.rules()
            rules: RuleSet | DecisionTable = (
                DecisionTable._from_native(content)  # pyright: ignore[reportPrivateUsage]
                if kind == "decision_table"
                else _ruleset(content)
            )
            object.__setattr__(self, "_rules", rules)
            return rules
        return self._rules

    @property
    def ruleset(self) -> RuleSet:
        """The rule set the bundle gives: the one it holds, or the table's compiled one."""
        rules = self.rules
        return rules.ruleset if isinstance(rules, DecisionTable) else rules

    @property
    def ruleset_digest(self) -> str:
        """The rule set digest of its rules, as `digest(bundle.ruleset)` gives it."""
        return self._native.ruleset_digest

    @property
    def engine(self) -> BundleEngine:
        """The engine and the limits that made the bundle."""
        name, version, limits, is_current = self._native.engine()
        return BundleEngine(name, version, MappingProxyType(dict(limits)), is_current)

    @property
    def contract(self) -> dict[str, Any]:
        """The contract of the rules, in its JSON form (a new copy each time).

        `{"input": <JSON Schema of the input>, "output": {"keys": [...]}}`:
        the input schema the rules read (`RuleSet.input_schema`), and the
        payload keys any rule may output, sorted.
        """
        return json.loads(self._native.contract_json())

    @property
    def cases(self) -> tuple[CaseFile, ...]:
        """The case files, in the order they run."""
        return tuple(CaseFile(path, digest) for path, digest in self._native.cases())

    def case_matches(self, path: str, data: bytes | str) -> bool:
        """Whether `data` is the case file at `path` the bundle was made with.

            bundle.case_matches("cases/vip.json", Path("cases/vip.json").read_bytes())

        Args:
            path: The case file's path, as the bundle holds it.
            data: The file's content; a `str` is encoded as UTF-8.

        Raises:
            KeyError: If the bundle has no case file at `path`.
        """
        return self._native.case_matches(path, _bytes(data, "data"))

    def compare(self, other: Bundle) -> BundleComparison:
        """Compare this bundle with another (`spec/bundles.md`, *Comparing bundles*).

            comparison = old.compare(new)
            if comparison.relation == BundleRelation.SAME_RULES_OTHER_ENGINE:
                ...  # any change in the results comes from the engine

        Comparing uses nothing but the two bundles, and is symmetric but for
        which bundle comes first.

        Raises:
            TypeError: If `other` is not a `Bundle`.
        """
        if not isinstance(other, Bundle):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise TypeError(f"a bundle compares with a Bundle, not {type(other).__name__}")
        relation, identical, differences = self._native.compare(other._native)
        return BundleComparison(BundleRelation(relation), identical, tuple(differences))

    def _bundle_digest(self) -> str:
        if self._digest is None:
            digest = _native.digest(self._native)
            object.__setattr__(self, "_digest", digest)
            return digest
        return self._digest

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Bundle):
            return NotImplemented
        return self._bundle_digest() == other._bundle_digest()

    def __hash__(self) -> int:
        return hash(self._bundle_digest())

    def __setattr__(self, name: str, value: object) -> NoReturn:
        raise AttributeError(f"bundles are immutable: cannot set {name!r}")

    def __delattr__(self, name: str) -> NoReturn:
        raise AttributeError(f"bundles are immutable: cannot delete {name!r}")

    def __reduce__(self) -> tuple[Any, tuple[Any, ...]]:
        return (loads, (dumps(self),))

    def __repr__(self) -> str:
        engine = self.engine
        kind = "decision_table" if self._native.rules()[0] == "decision_table" else "ruleset"
        count = len(self._native.cases())
        return (
            f"<Bundle {self.id!r} revision {self.revision!r}: {kind},"
            f" {count} case file{'' if count == 1 else 's'}, {engine.name} {engine.version}>"
        )


def _bytes(data: bytes | str, what: str) -> bytes:
    if isinstance(data, str):
        return data.encode("utf-8")
    if isinstance(data, bytes | bytearray | memoryview):  # pyright: ignore[reportUnnecessaryIsInstance]
        return bytes(data)
    raise TypeError(f"{what} must be bytes or str, got {type(data).__name__}")


def _cases(
    cases: Iterable[tuple[str, bytes | str]] | Mapping[str, bytes | str],
) -> list[tuple[str, bytes]]:
    """The case files as `(path, bytes)` pairs, in order."""
    if isinstance(cases, str | bytes):
        raise TypeError("cases are (path, content) pairs or a mapping, not a str")
    items: Iterable[Any] = (
        cast(Mapping[str, Any], cases).items() if isinstance(cases, Mapping) else cases
    )
    pairs: list[tuple[str, bytes]] = []
    for position, item in enumerate(items):
        try:
            path, data = item
        except (TypeError, ValueError):
            raise TypeError(
                f"case {position} must be a (path, content) pair, got {item!r}"
            ) from None
        if not isinstance(path, str):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise TypeError(f"the path of case {position} must be a str, got {type(path).__name__}")
        pairs.append((path, _bytes(data, f"the content of case {path!r}")))
    return pairs
