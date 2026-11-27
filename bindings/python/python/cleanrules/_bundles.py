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
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, NoReturn, TypedDict, cast

from cleanrules import _cleanrules as _native
from cleanrules._documents import (
    _check_actions,  # pyright: ignore[reportPrivateUsage]
    _ruleset,  # pyright: ignore[reportPrivateUsage]
    dumps,
    loads,
)
from cleanrules._rules import RuleSet
from cleanrules._tables import DecisionTable

if TYPE_CHECKING:
    from cleanrules._cases import BundleCases, CaseRun, TestRun

__all__ = [
    "Bundle",
    "BundleComparison",
    "BundleContract",
    "BundleContractOutput",
    "BundleDifference",
    "BundleEngine",
    "BundleLimits",
    "BundleRelation",
    "CaseFileRef",
]


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


class BundleDifference(enum.StrEnum):
    """A part that differs between two bundles (`spec/bundles.md`, *Comparing bundles*).

    The members are in the order of the specification, the order in which
    `BundleComparison.differences` lists them. Their values are the names of
    the JSON form, the same in every language.
    """

    ID = "id"
    REVISION = "revision"
    RULES = "rules"
    """The rule set digests differ."""
    SOURCE = "source"
    """The rule set digests are equal, but the documents that give them differ."""
    ENGINE_NAME = "engine.name"
    ENGINE_VERSION = "engine.version"
    ENGINE_LIMITS_MAX_SIZE = "engine.limits.max_size"
    ENGINE_LIMITS_MAX_NESTING = "engine.limits.max_nesting"
    ENGINE_LIMITS_MAX_DEPTH = "engine.limits.max_depth"
    ENGINE_LIMITS_MAX_PATH_SEGMENTS = "engine.limits.max_path_segments"
    ENGINE_LIMITS_MAX_TABLE_SIZE = "engine.limits.max_table_size"
    ENGINE_LIMITS_MAX_VALUES = "engine.limits.max_values"
    CONTRACT_INPUT = "contract.input"
    CONTRACT_OUTPUT = "contract.output"
    CASES = "cases"
    """The case files differ, or their order."""


class BundleContractOutput(TypedDict):
    """The output part of a bundle's contract."""

    keys: list[str]
    """The payload keys any rule may output, sorted by Unicode code point."""


class BundleContract(TypedDict):
    """The contract of a bundle's rules in its JSON form (`spec/bundles.md`, *Contract*)."""

    input: dict[str, Any]
    """The JSON Schema of the input the rules read (`RuleSet.input_schema`)."""
    output: BundleContractOutput
    """The output keys."""


@dataclass(frozen=True, slots=True)
class BundleLimits:
    """The limits a bundle records: those that can change a result (`spec/bundles.md`, *Engine*).

    Attributes:
        max_size: The largest JSON document, in bytes.
        max_nesting: The deepest nesting of arrays and objects.
        max_depth: The deepest condition or payload value.
        max_path_segments: The longest field path.
        max_table_size: The most cells and nodes of a decision table.
        max_values: The most values read from one input's native data.
    """

    max_size: int
    max_nesting: int
    max_depth: int
    max_path_segments: int
    max_table_size: int
    max_values: int

    def to_dict(self) -> dict[str, int]:
        """The limits' JSON form, the keys in the order of the specification."""
        return {
            "max_size": self.max_size,
            "max_nesting": self.max_nesting,
            "max_depth": self.max_depth,
            "max_path_segments": self.max_path_segments,
            "max_table_size": self.max_table_size,
            "max_values": self.max_values,
        }


@dataclass(frozen=True, slots=True)
class BundleEngine:
    """The engine and limits that made a bundle (`spec/bundles.md`, *Engine*).

    Attributes:
        name: The engine's name, `"cleanrules"` for this one.
        version: Its exact version, a Semantic Versioning version.
        limits: The limits it ran with.
        is_current: Whether it is this engine: the same name and version as
            `cleanrules.__version__`, whatever the limits.
    """

    name: str
    version: str
    limits: BundleLimits
    is_current: bool


@dataclass(frozen=True, slots=True)
class CaseFileRef:
    """A bundle's reference to a case file (`spec/bundles.md`, *Cases*): its path and digest.

    The case file itself is a document of test cases (`spec/tests.md`).

    Attributes:
        path: The file's path, relative to the bundle's directory, with `/`
            between segments.
        digest: The SHA-256 digest of the file's bytes, `sha256:` and 64
            lowercase hexadecimal digits.
    """

    path: str
    digest: str

    def __post_init__(self) -> None:
        for name, text in (("path", self.path), ("digest", self.digest)):
            if not isinstance(text, str):  # pyright: ignore[reportUnnecessaryIsInstance]
                raise TypeError(f"{name} must be a str, got {type(text).__name__}")
        _native.check_case_file(self.path, self.digest)

    def digest_matches(self, content: bytes | str) -> bool:
        """Whether `content` has the digest the bundle recorded: the same bytes.

        Args:
            content: The file's content; a `str` is encoded as UTF-8.
        """
        return _native.case_digest_matches(self.path, self.digest, _bytes(content, "content"))


@dataclass(frozen=True, slots=True)
class BundleComparison:
    """The comparison of two bundles (`spec/bundles.md`, *Comparing bundles*).

    Attributes:
        relation: How the rules and the engines relate.
        identical: Whether the two bundles are the same: the same digest.
        differences: What differs, in the order of the specification: `id`,
            `revision`, `rules`, `source`, `engine.name`, `engine.version`,
            `engine.limits.<key>`, `contract.input`, `contract.output` and
            `cases`. Empty exactly when the bundles are identical.
    """

    relation: BundleRelation
    identical: bool
    differences: tuple[BundleDifference, ...]

    def to_dict(self) -> dict[str, Any]:
        """The comparison's JSON form: `relation`, `identical` and `differences`."""
        return {
            "relation": self.relation.value,
            "identical": self.identical,
            "differences": [difference.value for difference in self.differences],
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
            `omit_actions` is true, with a `pointer` to it; or a limit of 0 or
            above 2**53 - 1, which a bundle cannot record.
        TypeError: For rules that are not a `RuleSet` or a `DecisionTable`,
            or an identifier, revision, path or content of another type.
        ValueError: For a negative limit.
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
        return BundleEngine(name, version, BundleLimits(**dict(limits)), is_current)

    @property
    def contract(self) -> BundleContract:
        """The contract of the rules, in its JSON form (a new copy each time).

        `{"input": <JSON Schema of the input>, "output": {"keys": [...]}}`:
        the input schema the rules read (`RuleSet.input_schema`), and the
        payload keys any rule may output, sorted.
        """
        return cast(BundleContract, json.loads(self._native.contract_json()))

    @property
    def cases(self) -> tuple[CaseFileRef, ...]:
        """The case files, in the order they run."""
        return tuple(CaseFileRef(path, digest) for path, digest in self._native.cases())

    def case_digest_matches(self, path: str, content: bytes | str) -> bool:
        """Whether `content` has the digest the bundle recorded for the case file at `path`.

            bundle.case_digest_matches("cases/vip.json", Path("cases/vip.json").read_bytes())

        Args:
            path: The case file's path, as the bundle holds it.
            content: The file's content; a `str` is encoded as UTF-8.

        Raises:
            KeyError: If the bundle has no case file at `path`; the message
                lists the paths it has.
        """
        return self._native.case_digest_matches(path, _bytes(content, "content"))

    def test(
        self,
        read: BundleCases,
        *,
        max_results: int | None = None,
        max_steps: int | None = None,
        max_explained: int | None = None,
        cancel: Callable[[], object] | None = None,
        ruleset_digest: bool = False,
    ) -> TestRun:
        """Run the bundle's case files against its rules (`spec/tests.md`).

            run = bundle.test("path/to/bundle/directory")
            run = bundle.test(lambda ref: storage.get(ref.path))

        The files run in the bundle's order (*Case files of a bundle*), each
        named by its path. Before a file is read, its bytes are checked
        against the digest the bundle records: a file that differs is one
        `decision_mismatch` result, at `/bundle/cases/<i>/digest`, and none of
        its cases runs. Files are read under the limits the bundle records,
        which the test run's `engine` records. The other arguments are those
        of `RuleSet.test`.

        Args:
            read: Where the bytes of each case file come from: a callable
                given each `CaseFileRef` (returning bytes, or a str as UTF-8),
                or the bundle's directory, which the paths are relative to.
            max_results: As `RuleSet.test`.
            max_steps: As `RuleSet.test`.
            max_explained: As `RuleSet.test`.
            cancel: As `RuleSet.test`.
            ruleset_digest: As `RuleSet.test`.

        Raises:
            OSError: If a file cannot be read from the directory; an exception
                that `read` raises propagates too: a file the caller cannot
                open is the caller's error, not a result.
        """
        from cleanrules._cases import _bundle_files, _test  # pyright: ignore[reportPrivateUsage]

        return _test(
            "bundle",
            self._native,
            _bundle_files(self, read),
            max_results=max_results,
            max_steps=max_steps,
            max_explained=max_explained,
            cancel=cancel,
            ruleset_digest=ruleset_digest,
        )

    def iter_test(
        self,
        read: BundleCases,
        *,
        max_results: int | None = None,
        max_steps: int | None = None,
        max_explained: int | None = None,
        cancel: Callable[[], object] | None = None,
        ruleset_digest: bool = False,
    ) -> CaseRun:
        """Run the bundle's case files one at a time, as `RuleSet.iter_test` does."""
        from cleanrules._cases import _bundle_files, _iter_test  # pyright: ignore[reportPrivateUsage]

        return _iter_test(
            "bundle",
            self._native,
            _bundle_files(self, read),
            max_results=max_results,
            max_steps=max_steps,
            max_explained=max_explained,
            cancel=cancel,
            ruleset_digest=ruleset_digest,
        )

    def compare(self, other: Bundle) -> BundleComparison:
        """Compare this bundle with another (`spec/bundles.md`, *Comparing bundles*).

            comparison = old.compare(new)
            if comparison.relation == BundleRelation.SAME_RULES_OTHER_ENGINE:
                ...  # any change in the results comes from the engine

        Comparing uses nothing but the two bundles, and is symmetric:
        `b.compare(a)` gives the same result.

        Raises:
            TypeError: If `other` is not a `Bundle`.
        """
        if not isinstance(other, Bundle):  # pyright: ignore[reportUnnecessaryIsInstance]
            raise TypeError(f"a bundle compares with a Bundle, not {type(other).__name__}")
        relation, identical, differences = self._native.compare(other._native)
        return BundleComparison(
            BundleRelation(relation),
            identical,
            tuple(BundleDifference(difference) for difference in differences),
        )

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
        # Read back under the limits the bundle records (its rules may need
        # more than the defaults), never below the defaults.
        return (_unpickle, (dumps(self), self.engine.limits.to_dict()))

    def __repr__(self) -> str:
        engine = self.engine
        kind = self._native.kind
        count = self._native.case_count
        return (
            f"<Bundle {self.id!r} revision {self.revision!r}: {kind},"
            f" {count} case file{'' if count == 1 else 's'}, {engine.name} {engine.version}>"
        )


def _unpickle(text: str, recorded: dict[str, int]) -> Bundle:
    """Read a pickled bundle under its recorded limits, never below the defaults."""
    limits = {
        name: max(recorded[name], default) for name, default in _native.default_bundle_limits()
    }
    bundle = loads(
        text,
        max_size=max(limits["max_size"], len(text.encode("utf-8"))),
        max_nesting=limits["max_nesting"],
        max_depth=limits["max_depth"],
        max_table_size=limits["max_table_size"],
        max_path_segments=limits["max_path_segments"],
    )
    return cast(Bundle, bundle)


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
