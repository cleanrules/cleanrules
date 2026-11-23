//! The static audit of rule sets and decision tables (`spec/audit.md`).
//!
//! An audit reads every condition into descriptions of the inputs that make
//! it `TRUE` and `FALSE`, as unions of boxes over its subjects, and reports
//! what it can show: tests whose types never match, conditions that are never
//! `TRUE`, rules never selected, rules that fire together, and inputs for which
//! no rule fires. Every finding that something can happen comes with a witness
//! that the rule set itself has evaluated, with its logic and strictness;
//! where the audit cannot decide, it says so.
//!
//! ```
//! use cleanrules::audit::{AuditLimits, DiagnosticCode, Severity, audit};
//! use cleanrules::text::parse_rule;
//! use cleanrules::json::Limits;
//! use cleanrules::rules::{Config, HitPolicy, RuleSet};
//!
//! let limits = Limits::default();
//! let adult = parse_rule("RULE adult IF age >= 18 THEN band = 'adult'", &limits)?;
//! let senior = parse_rule("RULE senior IF age >= 65 THEN band = 'senior'", &limits)?;
//! let config = Config { hit_policy: HitPolicy::Unique, ..Config::default() };
//! let ruleset = RuleSet::new(vec![adult, senior], config)?;
//! let report = audit(&ruleset, &AuditLimits::default())?;
//! let overlap = &report.diagnostics[0];
//! assert_eq!(overlap.code, DiagnosticCode::Overlap);
//! assert_eq!(overlap.severity, Severity::Error);
//! assert_eq!(overlap.rules, ["adult", "senior"]);
//! assert!(overlap.witness.is_some());
//! assert_eq!(report.max_severity(), Some(Severity::Error));
//! # Ok::<(), cleanrules::Error>(())
//! ```

mod check;
mod domain;
mod read;
mod text;

/// What was audited: a rule set, or a decision table. Shared with test runs.
pub use crate::rules::RulesSource;

use std::collections::BTreeMap;

use check::Auditor;
use read::Part;

use crate::ast::Node;
use crate::digest::Digest;
use crate::error::{Error, ErrorCode, Result, Segment};
use crate::logic::{Kleene, LogicName};
use crate::rules::{Aggregate, HitPolicy, RuleSet};
use crate::table::{DecisionTable, TablePolicy, label};
use crate::value::{Group, Value};

/// The bounds of an audit (`spec/audit.md`, *Limits*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AuditLimits {
    /// The boxes of the `TRUE` or the `FALSE` description of one node.
    pub max_terms: usize,
    /// The work of the whole audit, in steps.
    pub max_steps: usize,
}

impl Default for AuditLimits {
    fn default() -> Self {
        Self {
            max_terms: 256,
            max_steps: 1_000_000,
        }
    }
}

impl AuditLimits {
    /// These limits with `max_terms` boxes per description.
    #[must_use]
    pub fn with_max_terms(mut self, max_terms: usize) -> Self {
        self.max_terms = max_terms;
        self
    }

    /// These limits with `max_steps` steps.
    #[must_use]
    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = max_steps;
        self
    }
}

/// A limit of [`AuditLimits`], when one was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuditLimit {
    /// `max_terms`.
    MaxTerms,
    /// `max_steps`.
    MaxSteps,
}

impl AuditLimit {
    /// The limit's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AuditLimit::MaxTerms => "max_terms",
            AuditLimit::MaxSteps => "max_steps",
        }
    }
}

/// How serious a finding is. The set is closed: a command line maps it to
/// its exit codes.
///
/// Ordered from the most serious: `Error < Warning < Info`, so the most
/// serious of several is their minimum ([`Audit::max_severity`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Some input makes the evaluation fail, as the witness shows.
    Error,
    /// Part of the rules has no effect, or probably does not do what was meant.
    Warning,
    /// A fact the policy makes acceptable.
    Info,
}

impl Severity {
    /// The severity's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }

    /// The severity for a name (lowercase, exact).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "error" => Severity::Error,
            "warning" => Severity::Warning,
            "info" => Severity::Info,
            _ => return None,
        })
    }
}

/// The code of a diagnostic, in the order of `spec/audit.md` (*Checks*).
/// New codes may be added within format version 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// A modelled test that is `UNKNOWN` for every input.
    IncompatibleTypes,
    /// A field used with values of more than one group.
    MixedTypes,
    /// A table cell that reads a field of one key that no input reads.
    UnlistedField,
    /// A rule whose condition is never `TRUE`.
    Unsatisfiable,
    /// A rule that is never selected.
    Shadowed,
    /// Two rules that fire for one input.
    Overlap,
    /// A supported input for which no rule fires.
    Gap,
}

impl DiagnosticCode {
    /// Every code, in order.
    pub const ALL: &'static [DiagnosticCode] = &[
        DiagnosticCode::IncompatibleTypes,
        DiagnosticCode::MixedTypes,
        DiagnosticCode::UnlistedField,
        DiagnosticCode::Unsatisfiable,
        DiagnosticCode::Shadowed,
        DiagnosticCode::Overlap,
        DiagnosticCode::Gap,
    ];

    /// The code's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::IncompatibleTypes => "incompatible_types",
            DiagnosticCode::MixedTypes => "mixed_types",
            DiagnosticCode::UnlistedField => "unlisted_field",
            DiagnosticCode::Unsatisfiable => "unsatisfiable",
            DiagnosticCode::Shadowed => "shadowed",
            DiagnosticCode::Overlap => "overlap",
            DiagnosticCode::Gap => "gap",
        }
    }

    /// The code for a name (lowercase, exact).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == name)
    }
}

/// One finding (`spec/audit.md`, *Checks*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Diagnostic {
    /// What was found.
    pub code: DiagnosticCode,
    /// How serious it is.
    pub severity: Severity,
    /// The rules involved, by name: keys into [`Audit::rules`].
    pub rules: Vec<String>,
    /// What was found and how to fix it, in words.
    pub message: String,
    /// The test or cell, as a JSON Pointer into the audited document.
    pub pointer: Option<String>,
    /// The field, for `mixed_types` and `unlisted_field`.
    pub path: Option<Vec<Segment>>,
    /// The groups a field is used with, for `mixed_types`.
    pub groups: Vec<Group>,
    /// The input that shows it, for `overlap` and `gap`.
    pub witness: Option<Value>,
}

impl Diagnostic {
    fn new(code: DiagnosticCode, severity: Severity, rules: Vec<String>, message: String) -> Self {
        Self {
            code,
            severity,
            rules,
            message,
            pointer: None,
            path: None,
            groups: Vec::new(),
            witness: None,
        }
    }
}

/// Equal when every member is, witnesses compared as their JSON forms
/// (`spec/json-format.md`, *Comparing documents*).
impl PartialEq for Diagnostic {
    fn eq(&self, other: &Self) -> bool {
        let witness = match (&self.witness, &other.witness) {
            (None, None) => true,
            (Some(a), Some(b)) => crate::json::same(
                &crate::format::write_value(a),
                &crate::format::write_value(b),
            ),
            _ => false,
        };
        witness
            && self.code == other.code
            && self.severity == other.severity
            && self.rules == other.rules
            && self.message == other.message
            && self.pointer == other.pointer
            && self.path == other.path
            && self.groups == other.groups
    }
}

/// A check of the audit, in the order it runs. New checks may be added within
/// format version 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Check {
    /// `incompatible_types` and `mixed_types`.
    Types,
    /// `unlisted_field`, for decision tables.
    Fields,
    /// `unsatisfiable`.
    Unsatisfiable,
    /// `shadowed`.
    Shadowed,
    /// `overlap`.
    Overlap,
    /// `gap`.
    Gap,
}

impl Check {
    /// Every check, in order.
    pub const ALL: &'static [Check] = &[
        Check::Types,
        Check::Fields,
        Check::Unsatisfiable,
        Check::Shadowed,
        Check::Overlap,
        Check::Gap,
    ];

    /// The check's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Check::Types => "types",
            Check::Fields => "fields",
            Check::Unsatisfiable => "unsatisfiable",
            Check::Shadowed => "shadowed",
            Check::Overlap => "overlap",
            Check::Gap => "gap",
        }
    }

    /// The check for a name (lowercase, exact).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == name)
    }
}

/// Whether a check decided everything it looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckStatus {
    /// Every item was decided.
    Complete,
    /// Some items were not; they are in [`Audit::undecided`].
    Partial,
    /// The check does not apply to what was audited.
    NotApplicable,
}

impl CheckStatus {
    /// The status's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CheckStatus::Complete => "complete",
            CheckStatus::Partial => "partial",
            CheckStatus::NotApplicable => "not_applicable",
        }
    }
}

/// A check and its status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckResult {
    /// The check.
    pub check: Check,
    /// Whether it decided everything.
    pub status: CheckStatus,
}

/// A test the audit does not model, or a node read as one because of a limit.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NotModelled {
    /// The rule, by name.
    pub rule: String,
    /// The node's tag.
    pub op: &'static str,
    /// Where it is, as a JSON Pointer into the audited document.
    pub pointer: String,
    /// The limit that made it one, if any.
    pub limit: Option<AuditLimit>,
}

/// Why an item was left undecided. New reasons may be added within format
/// version 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UndecidedReason {
    /// A rule involved has a test the audit does not model.
    NotModelled,
    /// No witness could be built, or none showed it.
    NoWitness,
    /// A limit was reached first.
    Limit(AuditLimit),
}

impl UndecidedReason {
    /// The reason's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UndecidedReason::NotModelled => "not_modelled",
            UndecidedReason::NoWitness => "no_witness",
            UndecidedReason::Limit(_) => "limit",
        }
    }
}

/// An item a check left undecided, or the items it did not reach.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Undecided {
    /// The check.
    pub check: Check,
    /// The rules involved, by name; none for the gap check, and none for the
    /// items `max_steps` did not reach.
    pub rules: Vec<String>,
    /// Why.
    pub reason: UndecidedReason,
    /// For the items `max_steps` did not reach: how many there were.
    pub remaining: Option<usize>,
}

/// A rule of what was audited.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AuditedRule {
    /// Its name.
    pub name: String,
    /// Its own id, when it has one.
    pub id: Option<String>,
    /// Its row's position in the table, from 0, for a decision table.
    pub row: Option<usize>,
}

/// The report of an audit (`spec/audit.md`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Audit {
    /// What was audited.
    pub source: RulesSource,
    /// The hit policy: a rule set's, as the table policy of the same name, or
    /// a decision table's.
    pub hit_policy: TablePolicy,
    /// The aggregate.
    pub aggregate: Option<Aggregate>,
    /// The logic.
    pub logic: LogicName,
    /// Whether evaluation is strict.
    pub strict: bool,
    /// The rule set digest of the rule set audited (or that the table
    /// compiles to), when asked for ([`Audit::with_ruleset_digest`]).
    pub ruleset_digest: Option<Digest>,
    /// The limits it ran with.
    pub limits: AuditLimits,
    /// The rules, in rule order.
    pub rules: Vec<AuditedRule>,
    /// Each check's status, in check order.
    pub checks: Vec<CheckResult>,
    /// The tests not modelled, in rule order and depth-first pre-order.
    pub not_modelled: Vec<NotModelled>,
    /// The items left undecided, in check order.
    pub undecided: Vec<Undecided>,
    /// The findings, by code and then in the order of their check.
    pub diagnostics: Vec<Diagnostic>,
    /// The rule set digest, kept for [`Audit::with_ruleset_digest`].
    digest: Digest,
}

impl Audit {
    /// The status of a check.
    #[must_use]
    pub fn status(&self, check: Check) -> CheckStatus {
        self.checks
            .iter()
            .find(|c| c.check == check)
            .map_or(CheckStatus::NotApplicable, |c| c.status)
    }

    /// The most serious severity among the diagnostics, if there are any.
    #[must_use]
    pub fn max_severity(&self) -> Option<Severity> {
        self.diagnostics.iter().map(|d| d.severity).min()
    }

    /// The same report, carrying the rule set digest or not
    /// (`spec/audit.md`, *JSON*), as decisions do (`spec/json-format.md`,
    /// *Digests*, *In decisions*).
    #[must_use]
    pub fn with_ruleset_digest(mut self, carry: bool) -> Self {
        self.ruleset_digest = carry.then_some(self.digest);
        self
    }
}

/// The table policy of the same name as a rule set's policy.
fn table_policy(policy: HitPolicy) -> TablePolicy {
    TablePolicy::of_ruleset(policy)
}

/// The logic the audit reasons with: Kleene's, the only one a rule set takes
/// until rules fire to degrees (task J.2).
fn crisp(ruleset: &RuleSet) -> Result<Kleene> {
    match ruleset.logic() {
        LogicName::Boolean => Ok(Kleene),
        other => Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "the audit reasons with Kleene logic, not with {}",
                other.as_str()
            ),
        )
        .unsupported(
            "audit",
            "graded logics are audited once rules fire to degrees (task J.2)",
        )),
    }
}

/// Audit a rule set.
///
/// # Errors
///
/// `unsupported` for a rule set whose logic is not `boolean`.
pub fn audit(ruleset: &RuleSet, limits: &AuditLimits) -> Result<Audit> {
    let logic = crisp(ruleset)?;
    let pointers: Vec<String> = (0..ruleset.rules().len())
        .map(|i| format!("/ruleset/rules/{i}/when"))
        .collect();
    let parts: Vec<Vec<Part<'_>>> = ruleset
        .rules()
        .iter()
        .zip(&pointers)
        .map(|(rule, pointer)| {
            vec![Part {
                node: rule.when(),
                pointer,
                cell: false,
            }]
        })
        .collect();
    let rules = ruleset
        .rules()
        .iter()
        .map(|rule| AuditedRule {
            name: rule.name().to_owned(),
            id: rule.get_id().map(str::to_owned),
            row: None,
        })
        .collect();
    let mut auditor = Auditor::new(ruleset, &logic, limits);
    let mut report = auditor.run(&parts, &[], Vec::new(), CheckStatus::NotApplicable);
    report.rules = rules;
    Ok(report)
}

/// Audit a decision table: the rule set it compiles into, read cell by cell,
/// and its cells' fields (`spec/audit.md`, *Fields of decision tables*).
///
/// # Errors
///
/// `unsupported` for a table whose logic is not `boolean`.
pub fn audit_table(table: &DecisionTable, limits: &AuditLimits) -> Result<Audit> {
    let ruleset = table.ruleset();
    let logic = crisp(ruleset)?;
    let labels: BTreeMap<String, usize> = table
        .rows()
        .iter()
        .enumerate()
        .map(|(r, row)| (label(r, row), r))
        .collect();
    // Each rule's row: rules are named after their rows.
    let rows: Vec<usize> = ruleset
        .rules()
        .iter()
        .enumerate()
        .map(|(i, rule)| labels.get(rule.name()).copied().unwrap_or(i))
        .collect();
    let row_pointers: Vec<String> = rows
        .iter()
        .map(|r| format!("/decision_table/rows/{r}"))
        .collect();
    let cell_pointers: Vec<Vec<(usize, String)>> = rows
        .iter()
        .map(|&r| {
            table.rows().get(r).map_or_else(Vec::new, |row| {
                row.inputs
                    .iter()
                    .enumerate()
                    .filter(|(_, cell)| cell.is_some())
                    .map(|(c, _)| (c, format!("/decision_table/rows/{r}/inputs/{c}")))
                    .collect()
            })
        })
        .collect();
    let parts: Vec<Vec<Part<'_>>> = rows
        .iter()
        .zip(&cell_pointers)
        .map(|(&r, cells)| {
            cells
                .iter()
                .filter_map(|(c, pointer)| {
                    let node = table.rows().get(r)?.inputs.get(*c)?.as_ref()?;
                    Some(Part {
                        node,
                        pointer,
                        cell: true,
                    })
                })
                .collect()
        })
        .collect();
    let unlisted = unlisted_fields(table, ruleset, &rows);
    let mut auditor = Auditor::new(ruleset, &logic, limits);
    let mut report = auditor.run(&parts, &row_pointers, unlisted, CheckStatus::Complete);
    report.source = RulesSource::DecisionTable;
    report.hit_policy = table.config().hit_policy;
    report.rules = ruleset
        .rules()
        .iter()
        .zip(&rows)
        .map(|(rule, &row)| AuditedRule {
            name: rule.name().to_owned(),
            id: rule.get_id().map(str::to_owned),
            row: Some(row),
        })
        .collect();
    Ok(report)
}

/// The `unlisted_field` diagnostics of a table, in rule order and then cell
/// order: inputs, then outputs.
fn unlisted_fields(table: &DecisionTable, ruleset: &RuleSet, rows: &[usize]) -> Vec<Diagnostic> {
    let listed: Vec<&[Segment]> = table.inputs().iter().flat_map(Node::field_paths).collect();
    let mut found = Vec::new();
    for (rule, &r) in ruleset.rules().iter().zip(rows) {
        let Some(row) = table.rows().get(r) else {
            continue;
        };
        let cells = row
            .inputs
            .iter()
            .enumerate()
            .map(|(c, cell)| {
                (
                    cell,
                    format!("/decision_table/rows/{r}/inputs/{c}"),
                    c,
                    true,
                )
            })
            .chain(row.outputs.iter().enumerate().map(|(c, cell)| {
                (
                    cell,
                    format!("/decision_table/rows/{r}/outputs/{c}"),
                    c,
                    false,
                )
            }));
        for (cell, pointer, column, input) in cells {
            let Some(node) = cell else { continue };
            for path in node.field_paths() {
                if !matches!(path, [Segment::Key(_)]) || listed.contains(&path) {
                    continue;
                }
                let field = crate::text::path(path);
                let under = if input {
                    format!(
                        "the input {}",
                        table
                            .inputs()
                            .get(column)
                            .map_or_else(String::new, ToString::to_string)
                    )
                } else {
                    format!(
                        "the output {}",
                        table.outputs().get(column).map_or("", |o| o.name.as_str())
                    )
                };
                let mut diagnostic = Diagnostic::new(
                    DiagnosticCode::UnlistedField,
                    Severity::Warning,
                    vec![rule.name().to_owned()],
                    format!(
                        "the cell of row {} under {under} reads the field {field}, which no input \
                         reads: if it is text, write it between single quotes ('{field}'), or add an \
                         input for the field",
                        rule.name()
                    ),
                );
                diagnostic.pointer = Some(pointer.clone());
                diagnostic.path = Some(path.to_vec());
                found.push(diagnostic);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests;
