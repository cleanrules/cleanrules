# The classes of this module build one another from their native objects, and
# check at run time the arguments their types already describe.
# pyright: reportPrivateUsage=false, reportUnnecessaryIsInstance=false
"""Test cases and the case runner (`spec/tests.md`).

    cases = cleanrules.load("cases/discounts.json")  # a Cases
    run = rules.test([CaseFile(cases, path="cases/discounts.json")])
    run.all_passed  # every case ran and passed
    print(run)  # the core's text: each failure, explained, and the summary

    for result in rules.iter_test(["cases/discounts.json"], max_results=100):
        print(result.status, result.id)  # one at a time, as the core gives them

A case file holds identified inputs and what evaluating each must give: the
rules selected, the outputs, the rules left unknown or the error. Running
cases evaluates the rules on each input (with `evaluate`, never `run`: a test
calls no action) and gives one result per case, `passed`, `failed` with the
keys that do not match, or `error`. The core reads, checks, writes and runs
every case; this module builds its objects and gives back what it reports.

Three names, the same in every language: a `CaseFileRef` is a bundle's
reference to a case file (its path and digest); a `Cases` is the content of a
case file document; a `CaseFile` is one file given to a run: its path, when it
has one, and its cases or the error that keeps it from being read.

Within version 1 of the format new mismatch keys and reasons of interruption
may appear: they come as plain strings, not as members of the enums below.
Statuses are a closed set.
"""

from __future__ import annotations

import enum
import json
import os
import sys
from collections.abc import Callable, Iterable, Iterator, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal, NoReturn, TypeVar, cast, overload

from cleanrules import _cleanrules as _native
from cleanrules._rules import Aggregate
from cleanrules._tables import TablePolicy

if TYPE_CHECKING:
    from cleanrules._bundles import Bundle, BundleEngine, CaseFileRef
    from cleanrules.errors import CleanRulesError

__all__ = [
    "Case",
    "CaseFile",
    "CaseResult",
    "CaseRun",
    "CaseStatus",
    "Cases",
    "Expectation",
    "ExpectationKey",
    "ExpectedOutput",
    "Interrupted",
    "Interruption",
    "Mismatch",
    "TestRun",
    "TestRunBundle",
    "TestRunFile",
    "TestRunLimits",
    "TestRunSummary",
]


def _immutable(kind: str) -> tuple[Callable[..., NoReturn], Callable[..., NoReturn]]:
    def setattr_(self: object, name: str, value: object) -> NoReturn:
        raise AttributeError(f"{kind} are immutable: cannot set {name!r}")

    def delattr_(self: object, name: str) -> NoReturn:
        raise AttributeError(f"{kind} are immutable: cannot delete {name!r}")

    return setattr_, delattr_


def _copy(value: Any) -> Any:
    """`value` with its dicts, lists and tuples copied: values are immutable, containers not."""
    if isinstance(value, dict):
        return {key: _copy(item) for key, item in value.items()}  # pyright: ignore[reportUnknownVariableType]
    if isinstance(value, list):
        return [_copy(item) for item in value]  # pyright: ignore[reportUnknownVariableType]
    if isinstance(value, tuple):
        return tuple(_copy(item) for item in value)  # pyright: ignore[reportUnknownVariableType]
    return value


def _ids(value: Iterable[str] | None, name: str) -> list[str] | None:
    """Rule ids given as an iterable of strings, not one string."""
    if value is None:
        return None
    if isinstance(value, str | bytes):
        raise TypeError(f"{name} is an iterable of rule ids, such as [{value!r}], not a string")
    ids = list(value)
    for item in ids:
        if not isinstance(item, str):
            raise TypeError(f"{name} holds rule ids as str, not {type(item).__name__}")
    return ids


def _text(value: object, name: str) -> None:
    if value is not None and not isinstance(value, str):
        raise TypeError(f"{name} must be a str, got {type(value).__name__}")


class ExpectedOutput(enum.Enum):
    """An expected output that is not a dict of payload keys."""

    NOTHING_SELECTED = "nothing_selected"
    """No rule is selected, so there is no output (`null` in a case file)."""


class Expectation:
    """What evaluating a case's input must give (`spec/tests.md`, *Expectations*); immutable.

        Expectation(selected=["vip"], output={"discount": Decimal("0.15")})
        Expectation(output=ExpectedOutput.NOTHING_SELECTED)
        Expectation(error="missing_field")

    A part left out is not checked; an expectation without any part checks
    only that evaluation does not fail. The core checks every part, as it
    checks a case file.

    Args:
        selected: The effective ids of the rules expected to be selected, in
            selection order (an id, or the name of a rule without one).
        output: The expected single output: a dict whose keys the output has,
            with equal values (its other keys are not checked), or
            `ExpectedOutput.NOTHING_SELECTED`.
        outputs: The expected outputs of the selected rules, in selection
            order, each a dict as `output`.
        unknown: The effective ids of the rules expected to be unknown, in any
            order; kept sorted by code point.
        error: The code of the error evaluation must raise:
            `"missing_field"`, `"type_mismatch"` or `"hit_policy_error"`.
            It is given alone.

    Attributes:
        selected: As given, a tuple, or `None`.
        output: A new dict on each access (changing it changes nothing),
            `ExpectedOutput.NOTHING_SELECTED` or `None`.
        outputs: A tuple of new dicts on each access, or `None`.
        unknown: A tuple sorted by code point, or `None`.
        error: The code, or `None`.

    Raises:
        FormatError: For an empty or repeated rule id, a value a case file
            cannot hold (not data, or a number out of range), or an error
            given with another part, or that evaluation cannot raise.
        TypeError: For an argument of the wrong type.
        ValueError: For an error code that does not exist.
    """

    __slots__ = ("_native", "_output", "_outputs", "error", "selected", "unknown")

    _native: _native.Expectation
    _output: dict[str, Any] | ExpectedOutput | None
    _outputs: tuple[dict[str, Any], ...] | None
    selected: tuple[str, ...] | None
    unknown: tuple[str, ...] | None
    error: str | None

    def __init__(
        self,
        *,
        selected: Iterable[str] | None = None,
        output: Mapping[str, Any] | ExpectedOutput | None = None,
        outputs: Iterable[Mapping[str, Any]] | None = None,
        unknown: Iterable[str] | None = None,
        error: str | None = None,
    ) -> None:
        if output is None or output is ExpectedOutput.NOTHING_SELECTED:
            given_output = None
        elif isinstance(output, Mapping):
            given_output = dict(output)
        else:
            raise TypeError(
                "output is a dict of payload keys or ExpectedOutput.NOTHING_SELECTED,"
                f" not {type(output).__name__}"
            )
        listed: list[dict[str, Any]] | None = None
        if outputs is not None:
            if isinstance(outputs, Mapping):
                raise TypeError("outputs is a list of dicts, one per selected rule, not a dict")
            listed = []
            for item in outputs:
                if not isinstance(item, Mapping):
                    raise TypeError(f"outputs holds dicts, not {type(item).__name__}")
                listed.append(dict(item))
        _text(error, "error")
        native = _native.expectation_new(
            _ids(selected, "selected"),
            given_output,
            output is not None,
            listed,
            _ids(unknown, "unknown"),
            error,
        )
        self._set(native)

    def _set(self, native: _native.Expectation) -> None:
        set_ = object.__setattr__
        set_(self, "_native", native)
        selected = native.selected
        set_(self, "selected", None if selected is None else tuple(selected))
        given, value = native.output()
        output: dict[str, Any] | ExpectedOutput | None = None
        if given:
            output = ExpectedOutput.NOTHING_SELECTED if value is None else value
        set_(self, "_output", output)
        outputs = native.outputs()
        set_(self, "_outputs", None if outputs is None else tuple(outputs))
        unknown = native.unknown
        set_(self, "unknown", None if unknown is None else tuple(unknown))
        set_(self, "error", native.error)

    @classmethod
    def _from_native(cls, native: _native.Expectation) -> Expectation:
        expectation = cls.__new__(cls)
        expectation._set(native)
        return expectation

    @property
    def output(self) -> dict[str, Any] | ExpectedOutput | None:
        """The expected single output: a new dict, `NOTHING_SELECTED`, or `None` (not checked)."""
        return _copy(self._output)

    @property
    def outputs(self) -> tuple[dict[str, Any], ...] | None:
        """The expected outputs, new dicts, or `None` when not checked."""
        return _copy(self._outputs)

    @property
    def is_empty(self) -> bool:
        """Whether nothing is checked but that evaluation does not fail."""
        return (
            self.selected is None
            and self._output is None
            and self._outputs is None
            and self.unknown is None
            and self.error is None
        )

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Expectation):
            return NotImplemented
        return self._native.canonical() == other._native.canonical()

    def __hash__(self) -> int:
        return hash(self._native.canonical())

    __setattr__, __delattr__ = _immutable("expectations")

    def __repr__(self) -> str:
        parts = [
            f"{name}={getattr(self, name)!r}"
            for name in ("selected", "output", "outputs", "unknown", "error")
            if getattr(self, name) is not None
        ]
        return f"Expectation({', '.join(parts)})"


class Case:
    """One test case (`spec/tests.md`, *Cases*): an id, an input and an expectation; immutable.

        Case("vip-gets-15", {"customer": {"years": 7}}, expect=Expectation(selected=["vip"]))

    Args:
        id: Names the case, unique in its file: any non-empty string, compared
            exactly.
        input: The data the rules evaluate: `None`, bool, int, float,
            `Decimal`, str, dates, lists and dicts with str keys. An int or a
            `Decimal` out of range is null, as in a case file read from JSON.
        expect: What evaluating it must give; by default only that evaluation
            does not fail.
        description: What the case shows.

    Attributes:
        id: The id.
        input: The input as the core holds it, as new Python data on each
            access.
        expect: The expectation.
        description: The description, or `None`.

    Raises:
        FormatError: For an empty id, or an input that is not data.
        TypeError: For an argument of the wrong type.
    """

    __slots__ = ("_input", "_native", "description", "expect", "id")

    _native: _native.Case
    _input: Any
    id: str
    expect: Expectation
    description: str | None

    def __init__(
        self,
        id: str,
        input: Any,
        *,
        expect: Expectation | None = None,
        description: str | None = None,
    ) -> None:
        if not isinstance(id, str):
            raise TypeError(f"a case id must be a str, got {type(id).__name__}")
        if expect is not None and not isinstance(expect, Expectation):
            raise TypeError(f"expect must be an Expectation, got {type(expect).__name__}")
        _text(description, "description")
        expected = None if expect is None else expect._native
        self._set(_native.case_new(id, input, description, expected))

    def _set(self, native: _native.Case) -> None:
        set_ = object.__setattr__
        set_(self, "_native", native)
        set_(self, "id", native.id)
        set_(self, "_input", native.input())
        set_(self, "expect", Expectation._from_native(native.expect()))
        set_(self, "description", native.description)

    @classmethod
    def _from_native(cls, native: _native.Case) -> Case:
        case = cls.__new__(cls)
        case._set(native)
        return case

    @property
    def input(self) -> Any:
        """The input as the core holds it, as new Python data."""
        return _copy(self._input)

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Case):
            return NotImplemented
        return self._native.canonical() == other._native.canonical()

    def __hash__(self) -> int:
        return hash(self._native.canonical())

    __setattr__, __delattr__ = _immutable("cases")

    def __repr__(self) -> str:
        return f"Case({self.id!r}, {self.input!r}, expect={self.expect!r})"


class Cases(Sequence[Case]):
    """The content of a case file document (`spec/tests.md`, *Case files*); immutable.

        cases = Cases([Case("vip", {"customer": {"years": 7}})], description="discounts")
        text = cleanrules.dumps(cases)  # {"version":1,"cases":{"description":...
        cases = cleanrules.load("cases/discounts.json")

    A sequence of its cases, in the order they run, with their ids unique;
    `cases["vip"]` finds one by id. Read one with `load` and `loads` (JSON or
    YAML), and write it with `dump` and `dumps`; `canonical_bytes` and
    `digest` take it too. Equal when their canonical bytes are.

    Args:
        items: The cases, in order.
        description: What the file holds.
        metadata: Data about the file, as a rule's metadata.

    Raises:
        FormatError: For two cases with the same id, or metadata that is not
            data.
        TypeError: For an item that is not a `Case`.
    """

    __slots__ = ("_by_id", "_metadata", "_native", "description", "items")

    _native: _native.Cases
    _by_id: dict[str, Case]
    _metadata: dict[str, Any]
    items: tuple[Case, ...]
    description: str | None

    def __init__(
        self,
        items: Iterable[Case] = (),
        *,
        description: str | None = None,
        metadata: Mapping[str, Any] | None = None,
    ) -> None:
        listed = list(items)
        for item in listed:
            if not isinstance(item, Case):
                raise TypeError(f"Cases holds Case objects, not {type(item).__name__}")
        _text(description, "description")
        if metadata is not None and not isinstance(metadata, Mapping):
            raise TypeError(f"metadata must be a dict, got {type(metadata).__name__}")
        natives = [item._native for item in listed]
        given = None if metadata is None else dict(metadata)
        self._set(_native.cases_new(natives, description, given), tuple(listed))

    def _set(self, native: _native.Cases, items: tuple[Case, ...] | None = None) -> None:
        set_ = object.__setattr__
        set_(self, "_native", native)
        if items is None:
            items = tuple(Case._from_native(item) for item in native.items())
        set_(self, "items", items)
        set_(self, "_by_id", {case.id: case for case in items})
        set_(self, "description", native.description)
        set_(self, "_metadata", native.metadata())

    @classmethod
    def _from_native(cls, native: _native.Cases) -> Cases:
        cases = cls.__new__(cls)
        cases._set(native)
        return cases

    def __len__(self) -> int:
        return len(self.items)

    def __iter__(self) -> Iterator[Case]:
        return iter(self.items)

    @overload
    def __getitem__(self, key: int) -> Case: ...
    @overload
    def __getitem__(self, key: slice) -> Sequence[Case]: ...
    @overload
    def __getitem__(self, key: str) -> Case: ...
    def __getitem__(self, key: int | slice | str) -> Case | Sequence[Case]:
        if isinstance(key, str):
            try:
                return self._by_id[key]
            except KeyError:
                raise KeyError(f"no case has the id {key!r}") from None
        return self.items[key]

    @property
    def metadata(self) -> dict[str, Any]:
        """Data about the file, a new dict on each access; empty when it has none."""
        return _copy(self._metadata)

    def get(self, id: str, default: Case | None = None) -> Case | None:
        """The case with the id `id`, compared exactly, or `default`."""
        return self._by_id.get(id, default)

    def __contains__(self, value: object) -> bool:
        """Whether the file has a case with this id, or this case."""
        if isinstance(value, str):
            return value in self._by_id
        return value in self.items

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Cases):
            return NotImplemented
        return _native.canonical_bytes(self._native) == _native.canonical_bytes(other._native)

    def __hash__(self) -> int:
        return hash(_native.canonical_bytes(self._native))

    __setattr__, __delattr__ = _immutable("case files")

    def __repr__(self) -> str:
        count = len(self.items)
        about = "" if self.description is None else f" {self.description!r}"
        return f"<Cases{about}: {count} case{'' if count == 1 else 's'}>"


class CaseFile:
    """One case file given to a run (`spec/tests.md`, *Running cases*); immutable.

        CaseFile(cases, path="cases/discounts.json")  # already read
        CaseFile.read(data, path="cases/discounts.json")  # from its bytes
        CaseFile.load("cases/discounts.json")  # from disk

    Its path, when it has one, names its cases in the results (a case is its
    path and its id), and the digest of the bytes it was read from, when it
    was, is recorded by the test run. A file whose bytes cannot be read as a
    case file keeps the error, which running it gives as its one result.

    Args:
        cases: The cases, already read or built.
        path: The file's path, with `/` between segments.

    Attributes:
        path: The path, or `None`.
        digest: The SHA-256 digest of the bytes it was read from, or `None`.
        cases: The cases, or `None` for a file that cannot be read.
        error: The error of reading it, or `None`.
    """

    __slots__ = ("_native", "cases", "digest", "error", "path")

    _native: _native.CaseFile
    path: str | None
    digest: str | None
    cases: Cases | None
    error: CleanRulesError | None

    def __init__(self, cases: Cases, *, path: str | None = None) -> None:
        if not isinstance(cases, Cases):
            raise TypeError(f"a CaseFile holds Cases, not {type(cases).__name__}")
        _text(path, "path")
        self._set(_native.case_file_of(cases._native, path), cases)

    def _set(self, native: _native.CaseFile, cases: Cases | None = None) -> None:
        set_ = object.__setattr__
        set_(self, "_native", native)
        set_(self, "path", native.path)
        set_(self, "digest", native.digest)
        if cases is None:
            read = native.cases()
            cases = None if read is None else Cases._from_native(read)
        set_(self, "cases", cases)
        set_(self, "error", native.error())

    @classmethod
    def _from_native(cls, native: _native.CaseFile) -> CaseFile:
        file = cls.__new__(cls)
        file._set(native)
        return file

    @classmethod
    def read(
        cls,
        content: bytes | str,
        *,
        path: str | None = None,
        format: Literal["json", "yaml"] | None = None,
    ) -> CaseFile:
        """The case file whose bytes are `content`, read under the default limits.

        It is read as YAML when `path` ends in `.yaml` or `.yml` (in any
        case) and as JSON otherwise, unless `format` says. An error reading it
        is kept in `error`, not raised: running the file gives it as its one
        result, and the run goes on.

        Args:
            content: Its bytes; a `str` is encoded as UTF-8.
            path: The file's path, with `/` between segments, or `None`.
            format: `"json"` or `"yaml"`, when the path does not say.
        """
        _text(path, "path")
        if format not in (None, "json", "yaml"):
            raise ValueError(f"a case file is JSON or YAML, not {format!r}")
        if isinstance(content, str):
            data = content.encode("utf-8")
        elif isinstance(content, bytes | bytearray | memoryview):
            data = bytes(content)
        else:
            raise TypeError(f"content must be bytes or str, got {type(content).__name__}")
        return cls._from_native(_native.case_file_read(path, data, format))

    @classmethod
    def load(cls, path: str | os.PathLike[str]) -> CaseFile:
        """The case file at `path`, read from disk as `read` reads it.

        Its path in the results is `path` with `/` between segments.

        Raises:
            OSError: If the file cannot be opened: the caller's error, not a
                result.
        """
        file = Path(path)
        return cls.read(file.read_bytes(), path=file.as_posix())

    def case(self, id: str) -> Case | None:
        """The case with the id `id`, when the file was read and has it."""
        return None if self.cases is None else self.cases.get(id)

    __setattr__, __delattr__ = _immutable("case files")

    def __repr__(self) -> str:
        where = "without a path" if self.path is None else self.path
        if self.cases is None:
            return f"<CaseFile {where}: unreadable>"
        count = len(self.cases)
        return f"<CaseFile {where}: {count} case{'' if count == 1 else 's'}>"


# --- Results ---------------------------------------------------------------------------


class CaseStatus(enum.StrEnum):
    """The status of a result (`spec/tests.md`, *Matching*); a closed set."""

    PASSED = "passed"
    """Every key of the expectation matches."""
    FAILED = "failed"
    """Some key of the expectation does not match."""
    ERROR = "error"
    """Evaluation raised an error the case does not expect, or the case file cannot be run."""


class ExpectationKey(enum.StrEnum):
    """A key of an expectation, in the order they are checked. New keys may appear."""

    SELECTED = "selected"
    OUTPUT = "output"
    OUTPUTS = "outputs"
    UNKNOWN = "unknown"
    ERROR = "error"


class Interruption(enum.StrEnum):
    """Why a run stopped before its last result. New reasons may appear."""

    MAX_RESULTS = "max_results"
    """`max_results` results had been given."""
    MAX_STEPS = "max_steps"
    """The next case would have taken the steps past `max_steps`."""
    CANCELLED = "cancelled"
    """The caller cancelled the run, or stopped taking its results."""


_E = TypeVar("_E", bound=enum.StrEnum)


def _known(kind: type[_E], name: str) -> _E | str:
    """The member named `name`, or `name` itself when this version does not know it."""
    try:
        return kind(name)
    except ValueError:
        return name


class Mismatch:
    """A key of an expectation that does not match (`spec/tests.md`, *Test runs*); immutable.

        Mismatch(ExpectationKey.SELECTED, ["loyal"], ["vip"])

    Attributes:
        key: The key: an `ExpectationKey`, or a key a later version adds, as a
            string, whose `expected` and `actual` are in their JSON form.
        expected: What the case expects (new data on each access): a list of
            rule ids (`selected`, `unknown`, sorted), a dict or
            `ExpectedOutput.NOTHING_SELECTED` (`output`), a list of dicts
            (`outputs`), or an error code (`error`).
        actual: What was given, in the same form (new data on each access);
            for `error`, the exception raised, or `None` when none was. `None`
            for `output` only when nothing was given, a decision without a
            single output, and then `reason` says why.
        reason: Why nothing was given to compare, or `None`.
    """

    __slots__ = ("_actual", "_expected", "key", "reason")

    key: ExpectationKey | str
    reason: str | None
    _expected: Any
    _actual: Any

    def __init__(
        self, key: ExpectationKey | str, expected: Any, actual: Any, reason: str | None = None
    ) -> None:
        set_ = object.__setattr__
        set_(self, "key", key)
        set_(self, "_expected", expected)
        set_(self, "_actual", actual)
        set_(self, "reason", reason)

    @property
    def expected(self) -> Any:
        """What the case expects, as new data."""
        return _copy(self._expected)

    @property
    def actual(self) -> Any:
        """What was given, as new data; see the class."""
        return _copy(self._actual)

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, Mismatch):
            return NotImplemented
        return (self.key, self._expected, self._actual, self.reason) == (
            other.key,
            other._expected,
            other._actual,
            other.reason,
        )

    __hash__ = None  # type: ignore[assignment]  # it holds dicts and lists

    __setattr__, __delattr__ = _immutable("mismatches")

    def __repr__(self) -> str:
        return (
            f"Mismatch(key={self.key!r}, expected={self._expected!r},"
            f" actual={self._actual!r}, reason={self.reason!r})"
        )


_ResultData = tuple[
    str | None,
    str | None,
    str,
    list[tuple[str, Any, Any, bool, str | None, bool]],
    Any,
    str | None,
    str,
]


class CaseResult:
    """The result of one case, or of a case file that cannot be run; immutable.

    Attributes:
        path: The path of the case's file, or `None`.
        id: The case's id; `None` for a case file that cannot be run.
        status: Passed, failed or error.
        mismatches: For a failed case, the keys that do not match, in the
            order of the expectation's keys.
        error: For an error, the exception (a `CleanRulesError`); otherwise
            `None`.
        explanation_text: For a failed case explained, the text of its
            decision's explanation; otherwise `None`.
    """

    __slots__ = ("_json", "error", "explanation_text", "id", "mismatches", "path", "status")

    _json: str
    path: str | None
    id: str | None
    status: CaseStatus
    mismatches: tuple[Mismatch, ...]
    error: CleanRulesError | None
    explanation_text: str | None

    def __init__(self, data: _ResultData) -> None:
        """Internal: runs make results."""
        path, id, status, mismatches, error, explanation_text, text = data
        written: dict[str, Any] = json.loads(text)
        set_ = object.__setattr__
        set_(self, "_json", text)
        set_(self, "path", path)
        set_(self, "id", id)
        set_(self, "status", CaseStatus(status))
        listed: list[Mismatch] = []
        for i, (key, expected, actual, given, reason, known) in enumerate(mismatches):
            if not known:
                # A key a later version adds: its parts in their JSON form.
                form = written["mismatches"][i]
                expected, actual = form.get("expected"), form.get("actual")
            elif key == ExpectationKey.OUTPUT:
                # `null` is "nothing selected", as in an expectation; `None`
                # only for what was not given.
                if expected is None:
                    expected = ExpectedOutput.NOTHING_SELECTED
                if given and actual is None:
                    actual = ExpectedOutput.NOTHING_SELECTED
            listed.append(Mismatch(_known(ExpectationKey, key), expected, actual, reason))
        set_(self, "mismatches", tuple(listed))
        set_(self, "error", error)
        set_(self, "explanation_text", explanation_text)

    def to_dict(self) -> dict[str, Any]:
        """The result's JSON form, as a test run document writes it (`spec/tests.md`)."""
        return json.loads(self._json)

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, CaseResult):
            return NotImplemented
        return self._json == other._json

    def __hash__(self) -> int:
        return hash(self._json)

    __setattr__, __delattr__ = _immutable("results")

    def __repr__(self) -> str:
        where = " ".join(repr(part) for part in (self.path, self.id) if part is not None)
        return f"<CaseResult {where or '(a case file without a path)'}: {self.status.value}>"


@dataclass(frozen=True, slots=True)
class Interrupted:
    """How a partial run stopped.

    Attributes:
        reason: Why it stopped.
        remaining: The results it did not give, among the case files given to it.
    """

    reason: Interruption | str
    remaining: int


@dataclass(frozen=True, slots=True)
class TestRunSummary:
    """The counts of a run's results, and whether it was interrupted.

    Attributes:
        total: The number of results.
        passed: The results `passed`.
        failed: The results `failed`.
        error: The results `error`, case files that cannot be read included.
        partial: Whether the run was interrupted before its last result.
        interrupted: How it stopped, when it is partial.
    """

    __test__ = False  # not a test class, for pytest

    total: int
    passed: int
    failed: int
    error: int
    partial: bool
    interrupted: Interrupted | None = None

    @classmethod
    def _of(cls, data: tuple[int, int, int, tuple[str, int] | None, int, bool]) -> TestRunSummary:
        """The summary the core gives: its counts, total and partial included."""
        passed, failed, error, interrupted, total, partial = data
        stopped = None
        if interrupted is not None:
            stopped = Interrupted(_known(Interruption, interrupted[0]), interrupted[1])
        return cls(total, passed, failed, error, partial, stopped)


@dataclass(frozen=True, slots=True)
class TestRunLimits:
    """The limits a run ran with.

    Attributes:
        max_explained: The most failed cases explained.
        max_results: The most results given, or `None` for no limit.
        max_steps: The most steps, one per rule for each case, or `None`.
    """

    __test__ = False

    max_explained: int
    max_results: int | None = None
    max_steps: int | None = None


@dataclass(frozen=True, slots=True)
class TestRunFile:
    """A case file given to a run, as the test run lists it.

    Attributes:
        path: Its path, or `None`.
        digest: The digest of its bytes, when it was read from bytes.
    """

    __test__ = False

    path: str | None
    digest: str | None


@dataclass(frozen=True, slots=True)
class TestRunBundle:
    """The bundle whose cases a run ran: its id and revision."""

    __test__ = False

    id: str
    revision: str


class TestRun:
    """The results of a run and their summary (`spec/tests.md`, *Test runs*); immutable.

    Made by `RuleSet.test`, `DecisionTable.test`, `Bundle.test` and
    `CaseRun.test_run`.

    Attributes:
        source: What was tested: `"ruleset"` or `"decision_table"` (a
            bundle's rules are one or the other; a later version may add
            sources).
        hit_policy: The hit policy (a rule set's, as the table policy of the
            same name).
        aggregate: The `COLLECT` aggregate, if any.
        logic: The logic's name.
        strict: Whether evaluation is strict.
        ruleset_digest: The rule set digest of the rules tested, when asked
            for; otherwise `None`.
        engine: The engine that ran the cases and the limits its case files
            were read with (a bundle's own, for a bundle's cases).
        bundle: The bundle's id and revision, for a bundle's cases.
        files: The case files given to the run, in order.
        limits: The limits it ran with.
        summary: The counts, and how a partial run stopped.
        results: The results, in run order.
    """

    __test__ = False

    __slots__ = (
        "_data",
        "_native",
        "aggregate",
        "bundle",
        "engine",
        "files",
        "hit_policy",
        "limits",
        "logic",
        "results",
        "ruleset_digest",
        "source",
        "strict",
        "summary",
    )

    _native: _native.TestRun
    _data: dict[str, Any]
    source: str
    hit_policy: TablePolicy
    aggregate: Aggregate | None
    logic: str
    strict: bool
    ruleset_digest: str | None
    engine: BundleEngine
    bundle: TestRunBundle | None
    files: tuple[TestRunFile, ...]
    limits: TestRunLimits
    summary: TestRunSummary
    results: tuple[CaseResult, ...]

    def __init__(self, native: _native.TestRun) -> None:
        """Internal: runs make test runs."""
        from cleanrules._bundles import BundleEngine, BundleLimits

        data: dict[str, Any] = json.loads(native.to_json())
        set_ = object.__setattr__
        set_(self, "_native", native)
        set_(self, "_data", data)
        set_(self, "source", data["source"])
        set_(self, "hit_policy", TablePolicy(data["hit_policy"]))
        set_(self, "aggregate", Aggregate(data["aggregate"]) if "aggregate" in data else None)
        set_(self, "logic", data["logic"])
        set_(self, "strict", bool(data.get("strict", False)))
        set_(self, "ruleset_digest", data.get("ruleset_digest"))
        engine = data["engine"]
        known = set(BundleLimits.__dataclass_fields__)
        limits = BundleLimits(**{k: v for k, v in engine["limits"].items() if k in known})
        current = native.is_current
        set_(self, "engine", BundleEngine(engine["name"], engine["version"], limits, current))
        bundle = data.get("bundle")
        set_(
            self,
            "bundle",
            None if bundle is None else TestRunBundle(bundle["id"], bundle["revision"]),
        )
        files = tuple(TestRunFile(f.get("path"), f.get("digest")) for f in data["files"])
        set_(self, "files", files)
        given = data["limits"]
        set_(
            self,
            "limits",
            TestRunLimits(given["max_explained"], given.get("max_results"), given.get("max_steps")),
        )
        summary = data["summary"]
        interrupted = summary.get("interrupted")
        stopped = None
        if interrupted is not None:
            stopped = Interrupted(
                _known(Interruption, interrupted["reason"]), interrupted["remaining"]
            )
        set_(
            self,
            "summary",
            TestRunSummary(
                summary["total"],
                summary["passed"],
                summary["failed"],
                summary["error"],
                summary["partial"],
                stopped,
            ),
        )
        set_(self, "results", tuple(CaseResult(result) for result in native.results()))

    @property
    def all_passed(self) -> bool:
        """Whether every case ran and passed: no failure, no error, and not partial.

        True for a run of no cases.
        """
        return self._native.all_passed

    def to_dict(self) -> dict[str, Any]:
        """The test run document of the JSON format (`spec/tests.md`, *Test runs*).

        Values are in the JSON encoding of values (a decimal is
        `{"$decimal": "0.15"}`), so the document is JSON as it is.
        """
        return {"version": _native.FORMAT_VERSION, "test_run": json.loads(json.dumps(self._data))}

    __setattr__, __delattr__ = _immutable("test runs")

    def __repr__(self) -> str:
        summary = self.summary
        what = "rule set" if self.source == "ruleset" else self.source.replace("_", " ")
        partial = ", partial" if summary.partial else ""
        return (
            f"<TestRun of a {what}: {summary.passed} passed, {summary.failed} failed,"
            f" {summary.error} error{'' if summary.error == 1 else 's'}{partial}>"
        )

    def __str__(self) -> str:
        """The test run as text, the same in every language (`spec/tests.md`, *Text*)."""
        return self._native.text()


_READ_IN_CANCEL = "a run cannot be read from its own cancel callback; return True to stop it"


class CaseRun(Iterator[CaseResult]):
    """A run in progress: an iterator of its results, one at a time, in run order.

        run = rules.iter_test(files, cancel=stop.is_set)
        for result in run:
            if result.status != CaseStatus.PASSED:
                print(result.path, result.id, result.status)
        report = run.test_run()  # partial when the loop stopped early

    Each result is given before the next case is evaluated. The run ends after
    its last result, or when a limit or the caller stops it: `cancel` (a
    callable the run asks between results, never while a case is evaluated),
    `cancel()`, or asking for `test_run()`, which stops a run that is not
    finished. A run that stopped early is partial: its results are the first
    ones of the full run. The GIL is released while each case is evaluated.

    Leaving a `for` loop with `break` does not stop the run: Python tells an
    iterator nothing, so the run can go on (another `for`, or `next(run)`),
    and it is partial only once `test_run()` or `cancel()` stops it. (In
    JavaScript, leaving a `for...of` loop stops the run at once.)

    The `cancel` callable must not read the run: while it is asked, `summary`,
    `remaining`, `is_finished`, `test_run()` and `next()` raise `TypeError`,
    and `cancel()` asks the run to stop, as returning `True` does.

    `test_run()` holds every result given, and is made once.
    """

    __slots__ = ("_busy", "_cancel", "_native", "_report", "_stop")

    _native: _native.CaseRun
    _cancel: Callable[[], object] | None
    _report: TestRun | None
    _busy: bool
    _stop: bool

    def __init__(self, native: _native.CaseRun, cancel: Callable[[], object] | None) -> None:
        """Internal: the `iter_test` methods make runs."""
        set_ = object.__setattr__
        set_(self, "_native", native)
        set_(self, "_cancel", cancel)
        set_(self, "_report", None)
        set_(self, "_busy", False)
        set_(self, "_stop", False)

    def _ask(self) -> bool:
        """The answer to the core's question: stop, asked by `cancel()` or by `cancel`."""
        if self._stop:
            return True
        answer = self._cancel is not None and bool(self._cancel())
        # `cancel()` called from the callable asks to stop too.
        return answer or self._stop

    def _idle(self) -> None:
        if self._busy:
            raise TypeError(_READ_IN_CANCEL)

    def _advance(self, finish: bool) -> _ResultData | None:
        """One step of the native run (or all of them), the run marked busy meanwhile."""
        self._idle()
        object.__setattr__(self, "_busy", True)
        try:
            ask = self._ask if self._cancel is not None or self._stop else None
            if finish:
                self._native.finish(ask)
                return None
            return self._native.next(ask)
        finally:
            object.__setattr__(self, "_busy", False)

    def __iter__(self) -> CaseRun:
        return self

    def __next__(self) -> CaseResult:
        data = self._advance(False)
        if data is None:
            raise StopIteration
        return CaseResult(data)

    def cancel(self) -> None:
        """Stop the run now: it gives no more results, and is partial when a result remained.

        Called from the run's own `cancel` callable, it stops the run as
        returning `True` would.
        """
        if self._busy:
            object.__setattr__(self, "_stop", True)
            return
        self._native.stop()

    @property
    def summary(self) -> TestRunSummary:
        """The summary of the results given so far."""
        self._idle()
        return TestRunSummary._of(self._native.summary())

    @property
    def remaining(self) -> int:
        """The results not given yet; 0 once the run is finished."""
        self._idle()
        return self._native.remaining

    @property
    def is_finished(self) -> bool:
        """Whether the run gives no more results: complete, or interrupted."""
        self._idle()
        return self._native.is_finished

    def test_run(self) -> TestRun:
        """The test run: its results and summary.

        A run that is not finished is stopped first, and is then partial
        (`cancelled`): a run is never reported complete before its last
        result. The same object each time.
        """
        self._idle()
        if self._report is None:
            object.__setattr__(self, "_report", TestRun(self._native.test_run()))
        assert self._report is not None
        return self._report

    __setattr__, __delattr__ = _immutable("runs")

    def __repr__(self) -> str:
        if self._busy:
            return "<CaseRun: running>"
        summary = self.summary
        state = "finished" if self.is_finished else f"{self.remaining} remaining"
        return f"<CaseRun: {summary.total} results, {state}>"


# --- Running ---------------------------------------------------------------------------

# The largest limit the core holds: a limit beyond it is no limit at all.
_LIMIT_MAX = sys.maxsize * 2 + 1

CaseFiles = (
    CaseFile | Cases | str | os.PathLike[str] | Iterable[CaseFile | Cases | str | os.PathLike[str]]
)
"""The case files of a run: `CaseFile`s, `Cases` (a file without a path) or paths to read."""


def _limit(name: str, value: int | None) -> int | None:
    if value is None:
        return None
    if type(value) is not int:
        raise TypeError(f"{name} must be an int, got {type(value).__name__}")
    if value < 0:
        raise ValueError(f"{name} must be at least 0, got {value}")
    return min(value, _LIMIT_MAX)


def _one_file(item: object) -> _native.CaseFile:
    if isinstance(item, CaseFile):
        return item._native
    if isinstance(item, Cases):
        return _native.case_file_of(item._native, None)
    if isinstance(item, str | os.PathLike):
        return CaseFile.load(item)._native  # pyright: ignore[reportUnknownArgumentType]
    raise TypeError(f"a case file is a CaseFile, Cases or a path, not {type(item).__name__}")


def _files(  # pyright: ignore[reportUnusedFunction]  # used by RuleSet and DecisionTable
    files: CaseFiles,
) -> list[_native.CaseFile]:
    """The native case files of a run, in order; paths are read from disk."""
    if isinstance(files, CaseFile | Cases | str | os.PathLike):
        return [_one_file(files)]
    return [_one_file(item) for item in files]


def _run_options(
    max_results: int | None,
    max_steps: int | None,
    max_explained: int | None,
    cancel: Callable[[], object] | None,
    ruleset_digest: bool,
) -> dict[str, Any]:
    """Check a run's options; the limits clamped, by the native names."""
    if cancel is not None and not callable(cancel):
        raise TypeError(f"cancel must be a callable, got {type(cancel).__name__}")
    if type(ruleset_digest) is not bool:
        raise TypeError(f"ruleset_digest must be a bool, got {type(ruleset_digest).__name__}")
    return {
        "max_results": _limit("max_results", max_results),
        "max_steps": _limit("max_steps", max_steps),
        "max_explained": _limit("max_explained", max_explained),
        "ruleset_digest": ruleset_digest,
    }


def _iter_test(  # pyright: ignore[reportUnusedFunction]  # used by RuleSet, DecisionTable and Bundle
    kind: Literal["ruleset", "table", "bundle"],
    native: Any,
    files: list[_native.CaseFile],
    *,
    max_results: int | None,
    max_steps: int | None,
    max_explained: int | None,
    cancel: Callable[[], object] | None,
    ruleset_digest: bool,
) -> CaseRun:
    options = _run_options(max_results, max_steps, max_explained, cancel, ruleset_digest)
    make = {
        "ruleset": _native.run_ruleset,
        "table": _native.run_table,
        "bundle": _native.run_bundle,
    }[kind]
    return CaseRun(make(native, files, **options), cancel)


def _test(  # pyright: ignore[reportUnusedFunction]  # used by RuleSet, DecisionTable and Bundle
    kind: Literal["ruleset", "table", "bundle"],
    native: Any,
    files: list[_native.CaseFile],
    *,
    max_results: int | None,
    max_steps: int | None,
    max_explained: int | None,
    cancel: Callable[[], object] | None,
    ruleset_digest: bool,
) -> TestRun:
    run = _iter_test(
        kind,
        native,
        files,
        max_results=max_results,
        max_steps=max_steps,
        max_explained=max_explained,
        cancel=cancel,
        ruleset_digest=ruleset_digest,
    )
    run._advance(True)
    return run.test_run()


BundleCases = Callable[["CaseFileRef"], bytes | str] | str | os.PathLike[str]
"""Where a bundle's case files are read: a callable given each `CaseFileRef`, or a directory."""


def _bundle_files(  # pyright: ignore[reportUnusedFunction]  # used by Bundle
    bundle: Bundle, read: BundleCases
) -> list[_native.CaseFile]:
    """The case files of `bundle`, loaded from `read` up to the bound the core gives.

    The core picks the bound (the larger of the size limit the bundle records
    for the file's syntax and the default one), checks each file against its
    digest and reads it under the bundle's limits; a file in a directory
    larger than the bound is not read, and is a `limit_exceeded` result.
    """
    refs = {ref.path: ref for ref in bundle.cases}
    if isinstance(read, str | os.PathLike):
        directory = Path(cast("str | os.PathLike[str]", read))

        def load(path: str, bound: int) -> bytes | int:
            file = directory / path
            size = file.stat().st_size
            return size if size > bound else file.read_bytes()

    elif callable(read):
        reader = read

        def load(path: str, bound: int) -> bytes | int:
            content = reader(refs[path])
            if isinstance(content, str):
                return content.encode("utf-8")
            if isinstance(content, bytearray | memoryview):
                return bytes(content)
            if not isinstance(content, bytes):
                raise TypeError(
                    f"the content of the case file {path} must be bytes or str,"
                    f" got {type(content).__name__}"
                )
            return content

    else:
        raise TypeError(
            "a bundle's case files are read by a callable given each CaseFileRef, or from a"
            f" directory, not {type(read).__name__}"
        )
    return _native.bundle_case_files(bundle._native, load)
