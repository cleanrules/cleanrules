"""The static audit of rule sets and decision tables (`spec/audit.md`).

    report = rules.audit()
    for d in report.diagnostics:
        print(d.severity, d.code, d.rules, d.message)
        if d.witness is not None:
            rules.evaluate(d.witness)  # shows the finding
    report.max_severity  # Severity.ERROR, or None without findings
    print(report)  # the text, the same in every language

An audit reads every condition without any input and reports what it can
show: tests whose types never match, conditions that are never true, rules
that are never selected, rules that fire together and inputs for which no
rule fires. It is sound but incomplete: every diagnostic is true, and what it
could not decide is listed in `undecided` and `not_modelled`. The core
audits; this module only reads its report.

Within version 1 of the format new diagnostic codes, checks, reasons and
limits may appear: they come as plain strings, not as members of the enums
below, so that code written today keeps working. Severities and statuses are
closed sets.
"""

from __future__ import annotations

import enum
import json
import sys
from dataclasses import dataclass, field
from typing import Any, NoReturn, TypeVar

from cleanrules import _cleanrules as _native
from cleanrules._rules import Aggregate
from cleanrules._tables import TablePolicy

__all__ = [
    "Audit",
    "AuditCheck",
    "AuditLimit",
    "AuditLimits",
    "AuditedRule",
    "CheckResult",
    "CheckStatus",
    "Diagnostic",
    "DiagnosticCode",
    "NotModelled",
    "Severity",
    "Undecided",
    "UndecidedReason",
]


class Severity(enum.StrEnum):
    """How serious a finding is, from the least serious, as the core orders them; a closed set.

    Members compare as strings (`"error" < "info"`), not by seriousness: ask
    a report with `Audit.at_least(Severity.WARNING)`, and read the most
    serious from `Audit.max_severity`.
    """

    INFO = "info"
    """A fact the hit policy makes acceptable, which may be intended."""
    WARNING = "warning"
    """Part of the rules has no effect, or probably does not do what was meant."""
    ERROR = "error"
    """Some input makes the evaluation fail, as the witness shows."""


class DiagnosticCode(enum.StrEnum):
    """What a diagnostic found (`spec/audit.md`, *Checks*). New codes may appear."""

    INCOMPATIBLE_TYPES = "incompatible_types"
    """A test that is unknown for every input: its types never compare."""
    MIXED_TYPES = "mixed_types"
    """A field used with values of more than one type group."""
    UNLISTED_FIELD = "unlisted_field"
    """A table cell that reads a field of one key that no input reads, like `ES` for `'ES'`."""
    UNSATISFIABLE = "unsatisfiable"
    """A rule whose condition is never true."""
    SHADOWED = "shadowed"
    """A rule that fires only where a rule ahead of it fires too, so it is never selected."""
    OVERLAP = "overlap"
    """Two rules that fire for one input, the witness."""
    GAP = "gap"
    """A supported input, the witness, for which no rule fires."""


class AuditCheck(enum.StrEnum):
    """A check of the audit, in the order it runs. New checks may appear."""

    TYPES = "types"
    """`incompatible_types` and `mixed_types`."""
    FIELDS = "fields"
    """`unlisted_field`, for decision tables."""
    UNSATISFIABLE = "unsatisfiable"
    """`unsatisfiable`."""
    SHADOWED = "shadowed"
    """`shadowed`, under `first`, `any` and `priority`."""
    OVERLAP = "overlap"
    """`overlap`, under `first`, `priority`, `unique` and `any`."""
    GAP = "gap"
    """`gap`."""


class CheckStatus(enum.StrEnum):
    """Whether a check decided everything it looked at; a closed set."""

    COMPLETE = "complete"
    """Every item was decided."""
    PARTIAL = "partial"
    """Some items were not; they are in `Audit.undecided`."""
    NOT_APPLICABLE = "not_applicable"
    """The check does not apply to what was audited, or to its hit policy."""


class UndecidedReason(enum.StrEnum):
    """Why an item was left undecided. New reasons may appear."""

    NOT_MODELLED = "not_modelled"
    """A rule involved has a test the audit does not model."""
    NO_WITNESS = "no_witness"
    """No witness could be built, or none showed it."""
    LIMIT = "limit"
    """A limit was reached first."""


class AuditLimit(enum.StrEnum):
    """A limit of the audit (`spec/audit.md`, *Limits*). New limits may appear."""

    MAX_TERMS = "max_terms"
    """The boxes of the true or false description of one node (256)."""
    MAX_STEPS = "max_steps"
    """The work of the whole audit, in steps (1,000,000)."""


_E = TypeVar("_E", bound=enum.StrEnum)


def _known(kind: type[_E], name: str) -> _E | str:
    """The member named `name`, or `name` itself when this version does not know it."""
    try:
        return kind(name)
    except ValueError:
        return name


def _optional(kind: type[_E], name: str | None) -> _E | str | None:
    return None if name is None else _known(kind, name)


# The largest limit the core holds: a limit beyond it is no limit at all, as
# in JavaScript.
_LIMIT_MAX = sys.maxsize * 2 + 1


def _check_options(  # pyright: ignore[reportUnusedFunction]  # used by RuleSet and DecisionTable
    max_terms: int | None, max_steps: int | None, ruleset_digest: bool
) -> tuple[int | None, int | None]:
    """Check an audit's options before they reach the core; the limits, clamped."""
    limits: list[int | None] = []
    for name, limit in (("max_terms", max_terms), ("max_steps", max_steps)):
        if limit is not None:
            if type(limit) is not int:
                raise TypeError(f"{name} must be an int, got {type(limit).__name__}")
            if limit < 0:
                raise ValueError(f"{name} must be at least 0, got {limit}")
            limit = min(limit, _LIMIT_MAX)
        limits.append(limit)
    if type(ruleset_digest) is not bool:
        raise TypeError(f"ruleset_digest must be a bool, got {type(ruleset_digest).__name__}")
    return limits[0], limits[1]


@dataclass(frozen=True, slots=True)
class AuditLimits:
    """The limits an audit ran with.

    A limit that a later version adds appears in `Audit.to_dict()` until this
    class gains it.
    """

    max_terms: int
    max_steps: int


@dataclass(frozen=True, slots=True)
class AuditedRule:
    """A rule of what was audited: diagnostics name rules by `name`.

    Attributes:
        name: The rule's name.
        id: Its own id, if it has one (`Rule.id`).
        row: Its row's position in a decision table, from 0; `None` for a rule set.
    """

    name: str
    id: str | None = None
    row: int | None = None


@dataclass(frozen=True, slots=True)
class CheckResult:
    """A check and its status."""

    check: AuditCheck | str
    status: CheckStatus


@dataclass(frozen=True, slots=True)
class NotModelled:
    """A test the audit does not model, or a node read as one because of a limit.

    Attributes:
        rule: The rule, by name.
        op: The node's tag, such as `"matches"`.
        pointer: Where it is, as a JSON Pointer into the audited document.
        limit: The limit that made it one, if any.
    """

    rule: str
    op: str
    pointer: str
    limit: AuditLimit | str | None = None


@dataclass(frozen=True, slots=True)
class Undecided:
    """An item a check left undecided, or the items it did not reach.

    Attributes:
        check: The check.
        rules: The rules involved, by name; none for the gap check and for the
            items `max_steps` did not reach.
        reason: Why.
        limit: The limit, when `reason` is `limit`.
        remaining: How many items `max_steps` left unreached, for that entry.
    """

    check: AuditCheck | str
    rules: tuple[str, ...]
    reason: UndecidedReason | str
    limit: AuditLimit | str | None = None
    remaining: int | None = None


@dataclass(frozen=True, slots=True)
class Diagnostic:
    """One finding of an audit (`spec/audit.md`, *Checks*).

    Attributes:
        code: What was found: a `DiagnosticCode`, or a code this version does
            not know, as a string.
        severity: How serious it is.
        rules: The rules involved, by name: keys into `Audit.rules`.
        message: What was found and how to fix it, in words.
        pointer: The test or cell, as a JSON Pointer into the audited
            document, for `incompatible_types` and `unlisted_field`.
        path: The field, for `mixed_types` and `unlisted_field`.
        groups: The type groups a field is used with, for `mixed_types`.
        witness: The input that shows it, for `overlap` and `gap`: Python data
            that the audited rules evaluate to show the finding, and that can
            be kept as a test case's input. It is a fresh dict, which you may
            change; it takes no part in the diagnostic's hash.
    """

    code: DiagnosticCode | str
    severity: Severity
    rules: tuple[str, ...]
    message: str
    pointer: str | None = None
    path: tuple[str | int, ...] | None = None
    groups: tuple[str, ...] = ()
    witness: Any = field(default=None, hash=False)
    _text: str = field(default="", repr=False, compare=False)

    def __str__(self) -> str:
        """The diagnostic as the audit's text writes it (the core's), with its indented lines."""
        return self._text or repr(self)


class Audit:
    """The report of an audit (`spec/audit.md`), made by `RuleSet.audit` or `DecisionTable.audit`.

    Attributes:
        source: The kind of rules audited: `"ruleset"` or `"decision_table"`
            (a later version may add sources).
        document: `"bundle"` or `"rule"` when the core audited the rules as
            that document holds them (`audit_document`, as the command line
            and `Document.audit` of JavaScript's native layer do), with
            pointers into it; `None` for a rule set or a table audited
            directly, as `RuleSet.audit` and `DecisionTable.audit` do.
        hit_policy: The hit policy (a rule set's, as the table policy of the
            same name).
        aggregate: The `COLLECT` aggregate, if any.
        logic: The logic's name.
        strict: Whether evaluation is strict; witnesses are evaluated so.
        ruleset_digest: The rule set digest of what was audited (a table's
            compiled rule set), when asked for; otherwise `None`.
        limits: The limits it ran with.
        rules: The rules, in rule order.
        checks: Each check's status, in check order.
        not_modelled: The tests not modelled, in rule order and depth-first
            pre-order.
        undecided: The items left undecided, in check order.
        diagnostics: The findings, by code and then in their check's order.
    """

    __slots__ = (
        "_data",
        "_native",
        "aggregate",
        "checks",
        "diagnostics",
        "document",
        "hit_policy",
        "limits",
        "logic",
        "not_modelled",
        "rules",
        "ruleset_digest",
        "source",
        "strict",
        "undecided",
    )

    _native: _native.Audit
    _data: dict[str, Any]
    source: str
    document: str | None
    hit_policy: TablePolicy
    aggregate: Aggregate | None
    logic: str
    strict: bool
    ruleset_digest: str | None
    limits: AuditLimits
    rules: tuple[AuditedRule, ...]
    checks: tuple[CheckResult, ...]
    not_modelled: tuple[NotModelled, ...]
    undecided: tuple[Undecided, ...]
    diagnostics: tuple[Diagnostic, ...]

    def __init__(self, native: _native.Audit) -> None:
        """Internal: `RuleSet.audit()` and `DecisionTable.audit()` make audits."""
        data: dict[str, Any] = json.loads(native.to_json())
        witnesses = native.witnesses()
        texts = native.diagnostic_texts()
        set_ = object.__setattr__
        set_(self, "_native", native)
        set_(self, "_data", data)
        set_(self, "source", data["source"])
        set_(self, "document", data.get("document"))
        set_(self, "hit_policy", TablePolicy(data["hit_policy"]))
        set_(self, "aggregate", Aggregate(data["aggregate"]) if "aggregate" in data else None)
        set_(self, "logic", data["logic"])
        set_(self, "strict", bool(data.get("strict", False)))
        set_(self, "ruleset_digest", data.get("ruleset_digest"))
        set_(self, "limits", AuditLimits(data["limits"]["max_terms"], data["limits"]["max_steps"]))
        rules = tuple(
            AuditedRule(rule["name"], rule.get("id"), rule.get("row")) for rule in data["rules"]
        )
        set_(self, "rules", rules)
        checks = tuple(
            CheckResult(_known(AuditCheck, c["check"]), CheckStatus(c["status"]))
            for c in data["checks"]
        )
        set_(self, "checks", checks)
        not_modelled = tuple(
            NotModelled(n["rule"], n["op"], n["pointer"], _optional(AuditLimit, n.get("limit")))
            for n in data["not_modelled"]
        )
        set_(self, "not_modelled", not_modelled)
        undecided = tuple(
            Undecided(
                _known(AuditCheck, u["check"]),
                tuple(u["rules"]),
                _known(UndecidedReason, u["reason"]),
                _optional(AuditLimit, u.get("limit")),
                u.get("remaining"),
            )
            for u in data["undecided"]
        )
        set_(self, "undecided", undecided)
        diagnostics = tuple(
            Diagnostic(
                _known(DiagnosticCode, d["code"]),
                Severity(d["severity"]),
                tuple(d["rules"]),
                d["message"],
                d.get("pointer"),
                None if "path" not in d else tuple(d["path"]),
                tuple(d.get("groups", ())),
                witness,
                text,
            )
            for d, witness, text in zip(data["diagnostics"], witnesses, texts, strict=True)
        )
        set_(self, "diagnostics", diagnostics)

    @property
    def max_severity(self) -> Severity | None:
        """The most serious severity among the diagnostics, or `None` without findings.

        A command line maps it to its exit code: an `ERROR` means some input
        makes the evaluation fail.
        """
        severity = self._native.max_severity
        return None if severity is None else Severity(severity)

    def at_least(self, severity: Severity | str) -> bool:
        """Whether some diagnostic is at least as serious as `severity`, as the core orders them.

            if report.at_least(Severity.WARNING):
                ...  # a warning or an error: fail the build

        Raises:
            ValueError: For a name that is not a severity.
        """
        return self._native.at_least(Severity(severity).value)

    def status(self, check: AuditCheck | str) -> CheckStatus:
        """The status of a check; `NOT_APPLICABLE` for a check the report does not list."""
        for result in self.checks:
            if result.check == check:
                return result.status
        return CheckStatus.NOT_APPLICABLE

    def to_dict(self) -> dict[str, Any]:
        """The audit document of the JSON format (`spec/audit.md`, *JSON*).

        Witnesses are in the JSON encoding of values (a decimal is
        `{"$decimal": "0.15"}`), so the document is JSON as it is.
        """
        return {"version": _native.FORMAT_VERSION, "audit": json.loads(json.dumps(self._data))}

    def __setattr__(self, name: str, value: object) -> NoReturn:
        raise AttributeError(f"audits are immutable: cannot set {name!r}")

    def __delattr__(self, name: str) -> NoReturn:
        raise AttributeError(f"audits are immutable: cannot delete {name!r}")

    def __repr__(self) -> str:
        what = "rule set" if self.source == "ruleset" else self.source.replace("_", " ")
        count = len(self.diagnostics)
        worst = self.max_severity
        tail = "" if worst is None else f", max severity {worst.value}"
        return f"<Audit of a {what}: {count} diagnostic{'' if count == 1 else 's'}{tail}>"

    def __str__(self) -> str:
        """The audit as text, the same in every language (`spec/audit.md`, *Text*)."""
        return self._native.text()
