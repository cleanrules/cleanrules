//! Comparing two versions of rules on the same cases (`spec/comparison.md`).
//!
//! A [`Comparator`] runs case files against two sides, `a` and `b`, each a
//! [`Runner`] with its own limits, joins their results by path and id, and
//! gives a [`TestComparison`]: the relation of the two sides, each side's
//! run, and every change, added case and removed case, in comparison order,
//! each change expected or not.

use std::collections::{BTreeMap, BTreeSet};

use super::file::CaseFile;
use super::run::{
    Given, Outcome, Runner, Summary, TestRun, TestRunFile, TestRunLimits, TestRunState, repeated,
    same_value,
};
use super::{Case, CaseStatus, Expectation, ExpectedOutput, Statistics};
use crate::bundle::{
    self, Bundle, BundleComparison, BundleId, EngineLimit, EngineLimits, Relation, Revision,
};
use crate::digest::Digest;
use crate::error::{Error, ErrorCode, Result};
use crate::format::write_error;
use crate::json::{Json, to_string};
use crate::logic::LogicName;
use crate::rules::{Aggregate, RulesSource};
use crate::table::TablePolicy;
use crate::value::Map;

/// The kind of a listed item of a comparison (`spec/comparison.md`,
/// *Changes*, *Joining*): one of the changes, or a case that only one side
/// has. Later versions may add kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ChangeKind {
    /// The same rules selected, and other outputs.
    SameSelectionOtherOutput,
    /// Other rules selected, and the same outputs.
    OtherSelectionSameOutput,
    /// Other rules selected, and other outputs.
    OtherSelectionOtherOutput,
    /// The same rules selected and outputs, and other unknown rules.
    OtherUnknown,
    /// A new error, an error that is gone, or another error; or a case file
    /// that one side cannot read.
    Error,
    /// A case only side `b` has.
    Added,
    /// A case only side `a` has.
    Removed,
}

impl ChangeKind {
    /// Every kind, in the order of the specification: the changes, then
    /// `added` and `removed`.
    pub const ALL: &[ChangeKind] = &[
        ChangeKind::SameSelectionOtherOutput,
        ChangeKind::OtherSelectionSameOutput,
        ChangeKind::OtherSelectionOtherOutput,
        ChangeKind::OtherUnknown,
        ChangeKind::Error,
        ChangeKind::Added,
        ChangeKind::Removed,
    ];

    /// Its name in the format, such as `same_selection_other_output`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::SameSelectionOtherOutput => "same_selection_other_output",
            ChangeKind::OtherSelectionSameOutput => "other_selection_same_output",
            ChangeKind::OtherSelectionOtherOutput => "other_selection_other_output",
            ChangeKind::OtherUnknown => "other_unknown",
            ChangeKind::Error => "error",
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
        }
    }

    /// The kind for a name, exactly as the format writes it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == name)
    }

    /// Whether it is a change of a decision, which is expected or not, rather
    /// than a case that one side only has.
    #[must_use]
    pub fn is_change(self) -> bool {
        !matches!(self, ChangeKind::Added | ChangeKind::Removed)
    }
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A part of a decision that can differ between the two sides
/// (`spec/comparison.md`, *Changes*), in the order of the specification,
/// which `Ord` follows. Later versions may add parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ChangedPart {
    /// The selected rules, by effective id, in selection order.
    Selected,
    /// The single output, or the outputs of the selected rules.
    Output,
    /// The rules whose condition is unknown.
    Unknown,
    /// The error raised, or whether one is.
    Error,
}

impl ChangedPart {
    /// Every part, in order.
    pub const ALL: &[ChangedPart] = &[
        ChangedPart::Selected,
        ChangedPart::Output,
        ChangedPart::Unknown,
        ChangedPart::Error,
    ];

    /// Its name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ChangedPart::Selected => "selected",
            ChangedPart::Output => "output",
            ChangedPart::Unknown => "unknown",
            ChangedPart::Error => "error",
        }
    }

    /// The part for a name, exactly as the format writes it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|part| part.as_str() == name)
    }
}

impl std::fmt::Display for ChangedPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a change is unexpected (`spec/comparison.md`, *Expected changes*):
/// a part that side `b`'s expectation does not check, or that `b` did not
/// pass its case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Unstated {
    /// A part that differs and that the expectation does not check.
    Part(ChangedPart),
    /// Side `b`'s result is not `passed`.
    Status,
}

impl Unstated {
    /// Its name in the format: the part's, or `status`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Unstated::Part(part) => part.as_str(),
            Unstated::Status => "status",
        }
    }
}

impl std::fmt::Display for Unstated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The single output of a decision (`spec/rules.md`, *Outputs*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum SingleOutput {
    /// The winner's output, or the aggregate, with all its keys.
    Output(Map),
    /// No rule was selected: the output is null.
    NothingSelected,
    /// The decision has no single output (`RULE_ORDER`, and `COLLECT`
    /// without an aggregate).
    NoSingleOutput,
}

/// What a decision gave, without its result trees: what a case's
/// expectation checks (`spec/tests.md`, *Expectations*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CaseDecision {
    /// The effective ids of the selected rules, in selection order.
    pub selected: Vec<String>,
    /// The single output.
    pub output: SingleOutput,
    /// The outputs of the selected rules, in selection order.
    pub outputs: Vec<Map>,
    /// The effective ids of the rules whose condition is unknown, sorted by
    /// code point.
    pub unknown: Vec<String>,
}

impl From<Outcome> for CaseDecision {
    fn from(outcome: Outcome) -> Self {
        Self {
            selected: outcome.selected,
            output: match outcome.output {
                Ok(Some(map)) => SingleOutput::Output(map),
                Ok(None) => SingleOutput::NothingSelected,
                Err(()) => SingleOutput::NoSingleOutput,
            },
            outputs: outcome.outputs,
            unknown: outcome.unknown,
        }
    }
}

/// What one side gave a listed item: the result's status, and the decision
/// or the error (`spec/comparison.md`, *JSON*).
#[derive(Debug)]
#[non_exhaustive]
pub struct CaseOutcome {
    /// The status of the side's result against its case's expectation.
    pub status: CaseStatus,
    /// The decision, when evaluation gave one.
    pub decision: Option<CaseDecision>,
    /// The error evaluation raised (whatever the status), or the error of a
    /// case file that cannot be run.
    pub error: Option<Error>,
    /// For a change explained, the text of the decision's explanation.
    pub explanation_text: Option<String>,
}

impl CaseOutcome {
    fn new(status: CaseStatus, given: Given) -> Self {
        let (decision, error) = match given {
            Ok(outcome) => (Some(outcome.into()), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            status,
            decision,
            error,
            explanation_text: None,
        }
    }
}

/// One listed item of a comparison: a change, an added case or a removed
/// case (`spec/comparison.md`, *JSON*).
#[derive(Debug)]
#[non_exhaustive]
pub struct ComparedItem {
    /// The path of the case's file, when it has one.
    pub path: Option<String>,
    /// The case's id; `None` for a case file that cannot be read.
    pub id: Option<String>,
    /// The kind.
    pub kind: ChangeKind,
    /// For a change, whether it is expected (`spec/comparison.md`, *Expected
    /// changes*); `None` for an added or removed case.
    pub expected: Option<bool>,
    /// For a change, the parts that differ, in order.
    pub differences: Vec<ChangedPart>,
    /// For an unexpected change, why: the parts that differ and that side
    /// `b`'s expectation does not check, or that `b` did not pass.
    pub unstated: Vec<Unstated>,
    /// What side `a` gave, when it gave the item a result.
    pub a: Option<CaseOutcome>,
    /// What side `b` gave, when it gave the item a result.
    pub b: Option<CaseOutcome>,
}

/// Counts of what a side's cases gave (`spec/comparison.md`, *JSON*).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutcomeCounts {
    /// Cases whose evaluation gave a decision.
    pub decisions: usize,
    /// Cases whose decision selected no rule.
    pub nothing_selected: usize,
    /// Cases whose decision left some rule's condition unknown.
    pub with_unknown: usize,
    /// Cases whose evaluation raised an error, expected or not: the cases
    /// that ran less those with a decision.
    pub error: usize,
}

/// One side of a comparison: the header and summary of its test run,
/// without its results, and the counts of its outcomes.
#[derive(Debug)]
#[non_exhaustive]
pub struct ComparedRun {
    /// What was run.
    pub source: RulesSource,
    /// The hit policy, as a test run gives it.
    pub hit_policy: TablePolicy,
    /// The aggregate.
    pub aggregate: Option<Aggregate>,
    /// The logic.
    pub logic: LogicName,
    /// Whether evaluation is strict.
    pub strict: bool,
    /// The number of rules.
    pub rule_count: usize,
    /// The rule set digest of the rules.
    pub ruleset_digest: Digest,
    /// The limits the case files were read with.
    pub engine_limits: EngineLimits,
    /// The bundle's id and revision, for a bundle's cases.
    pub bundle: Option<(BundleId, Revision)>,
    /// The case files given to the run, in order.
    pub files: Vec<TestRunFile>,
    /// The limits it ran with: `max_results` and `max_steps` (a comparison
    /// explains changes, not failures, so `max_explained` is not used).
    pub limits: TestRunLimits,
    /// The counts of its results, and how it stopped when it is partial.
    pub summary: Summary,
    /// What its cases gave.
    pub outcomes: OutcomeCounts,
    /// Its statistics and coverage (`spec/tests.md`, *Statistics*), when its
    /// runner gathers them ([`Runner::with_statistics`]).
    pub statistics: Option<Statistics>,
}

impl ComparedRun {
    fn new(
        run: TestRun,
        ruleset_digest: Digest,
        limits: TestRunLimits,
        outcomes: OutcomeCounts,
    ) -> Self {
        Self {
            source: run.source,
            hit_policy: run.hit_policy,
            aggregate: run.aggregate,
            logic: run.logic,
            strict: run.strict,
            rule_count: run.rule_count,
            ruleset_digest,
            engine_limits: run.engine_limits,
            bundle: run.bundle,
            files: run.files,
            limits,
            summary: run.summary,
            outcomes,
            statistics: run.statistics,
        }
    }

    /// The name and version of the engine that ran it: this one.
    #[must_use]
    pub fn engine(&self) -> (&'static str, &'static str) {
        (bundle::ENGINE_NAME, bundle::ENGINE_VERSION)
    }

    /// Whether its run was interrupted.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.summary.is_partial()
    }
}

/// The counts of a comparison's items, listed or not
/// (`spec/comparison.md`, *JSON*).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TestComparisonSummary {
    /// Items unchanged.
    pub unchanged: usize,
    /// Changes expected.
    pub expected: usize,
    /// Changes unexpected.
    pub unexpected: usize,
    /// Cases only side `b` has.
    pub added: usize,
    /// Cases only side `a` has.
    pub removed: usize,
    /// Items not compared.
    pub not_compared: usize,
    /// Whether either side was interrupted.
    pub partial: bool,
    /// The number of changes of each kind: every kind of change of
    /// [`ChangeKind::ALL`], in its order.
    pub by_change: Vec<(ChangeKind, usize)>,
}

impl TestComparisonSummary {
    /// Every item of the two sides, joined.
    #[must_use]
    pub fn total(&self) -> usize {
        self.unchanged + self.changed() + self.added + self.removed + self.not_compared
    }

    /// The changes, expected or not.
    #[must_use]
    pub fn changed(&self) -> usize {
        self.expected + self.unexpected
    }
}

/// The comparison of two versions of rules on the same cases: the content of
/// a test comparison document (`spec/comparison.md`, *JSON*).
#[derive(Debug)]
#[non_exhaustive]
pub struct TestComparison {
    /// The relation of the two sides: never [`Relation::Conflict`], which a
    /// comparison refuses.
    pub relation: Relation,
    /// The comparison of the two bundles, when both sides are bundles.
    pub bundles: Option<BundleComparison>,
    /// Side `a`.
    pub a: ComparedRun,
    /// Side `b`.
    pub b: ComparedRun,
    /// The most changes explained.
    pub max_explained: usize,
    /// The counts of the items.
    pub summary: TestComparisonSummary,
    /// The changes, added cases and removed cases, in comparison order.
    pub items: Vec<ComparedItem>,
}

impl TestComparison {
    /// Whether some change is unexpected: what a command line fails on.
    #[must_use]
    pub fn has_unexpected_changes(&self) -> bool {
        self.summary.unexpected > 0
    }

    /// Whether either side was interrupted, so some items are not compared.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.summary.partial
    }

    /// Whether every item was compared: no side was interrupted, and no item
    /// is left not compared (`spec/comparison.md`, *Joining*). A command line
    /// treats an incomplete comparison as a failure.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.summary.partial && self.summary.not_compared == 0
    }

    /// The engine limits that differ between the two sides, with side `a`'s
    /// and side `b`'s values, in the order of their table.
    #[must_use]
    pub fn engine_limit_differences(&self) -> Vec<(EngineLimit, u64, u64)> {
        EngineLimit::ALL
            .into_iter()
            .map(|limit| {
                (
                    limit,
                    self.a.engine_limits.get(limit),
                    self.b.engine_limits.get(limit),
                )
            })
            .filter(|(_, a, b)| a != b)
            .collect()
    }
}

// Results cross threads: a binding compares on a worker thread and hands
// the comparison back, and a comparator may be shared between threads.
const _: () = {
    const fn sendable<T: Send + 'static>() {}
    const fn shared<T: Send + Sync>() {}
    sendable::<TestComparison>();
    shared::<Comparator<'static>>();
};

/// Compares two versions of rules on test cases (`spec/comparison.md`).
///
/// ```
/// use cleanrules::cases::{CaseFile, ChangeKind, Comparator, Runner};
/// use cleanrules::json::Limits;
///
/// let parse = |text: &str| {
///     cleanrules::text::parse(text, &Limits::default())?.into_ruleset()
/// };
/// let old = parse("HIT POLICY first\nRULE vip IF years > 5 THEN discount = 10")?;
/// let new = parse("HIT POLICY first\nRULE vip IF years > 5 THEN discount = 15")?;
/// let cases = br#"{"version": 1, "cases": {"items": [
///   {"id": "seven", "input": {"years": 7}, "expect": {"output": {"discount": 15}}},
///   {"id": "one", "input": {"years": 1}}
/// ]}}"#;
/// let files = [CaseFile::read("cases.json", cases, &Limits::default())];
///
/// let comparison = Comparator::new(Runner::new(&old), Runner::new(&new))?
///     .compare(&files, &files);
/// // The new version's case states the new discount: the change is expected.
/// assert_eq!(comparison.items[0].kind, ChangeKind::SameSelectionOtherOutput);
/// assert_eq!(comparison.items[0].expected, Some(true));
/// assert!(!comparison.has_unexpected_changes() && comparison.is_complete());
/// # Ok::<(), cleanrules::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Comparator<'r> {
    a: Runner<'r>,
    b: Runner<'r>,
    bundles: Option<BundleComparison>,
    max_explained: usize,
}

impl<'r> Comparator<'r> {
    /// A comparator of side `a` with side `b`, each running within its
    /// runner's limits (`max_results`, `max_steps`); a runner's
    /// `max_explained` is not used, since a comparison explains changes
    /// rather than failures ([`Comparator::with_max_explained`]).
    ///
    /// # Errors
    ///
    /// `decision_mismatch` when both runners run bundles with the same id and
    /// revision and other rules: a conflict (`spec/comparison.md`,
    /// *Bundles*).
    pub fn new(a: Runner<'r>, b: Runner<'r>) -> Result<Self> {
        if let (Some((id, revision)), Some(other)) = (a.bundle(), b.bundle())
            && (id, revision) == (&other.0, &other.1)
            && a.ruleset().digest() != b.ruleset().digest()
        {
            return Err(conflict(id, revision));
        }
        Ok(Self {
            a,
            b,
            bundles: None,
            max_explained: super::DEFAULT_MAX_EXPLAINED,
        })
    }

    /// A comparator of two bundles' rules, each under the limits it records
    /// and the default run limits, with the comparison of the two bundles
    /// (`spec/bundles.md`, *Comparing bundles*); their case files come from
    /// [`CaseFile::bundle_files`]. To set run limits, give
    /// [`Comparator::new`] the runners of [`Runner::for_bundle`] with their
    /// limits, then [`Comparator::with_bundles`].
    ///
    /// # Errors
    ///
    /// `decision_mismatch` for a conflict: the same id and revision with
    /// other rules.
    pub fn for_bundles(a: &'r Bundle, b: &'r Bundle) -> Result<Self> {
        Self::new(Runner::for_bundle(a), Runner::for_bundle(b))?.with_bundles(a, b)
    }

    /// This comparator, holding the comparison of the two bundles its
    /// runners run.
    ///
    /// # Errors
    ///
    /// `decision_mismatch` for a conflict, or when the bundles are not those
    /// of the runners (their id, revision and rule set digest).
    pub fn with_bundles(mut self, a: &Bundle, b: &Bundle) -> Result<Self> {
        let comparison = a.compare(b);
        if comparison.relation() == Relation::Conflict {
            return Err(conflict(a.id(), a.revision()));
        }
        for (runner, bundle, side) in [(&self.a, a, "a"), (&self.b, b, "b")] {
            let runs_it = runner
                .bundle()
                .is_some_and(|(id, revision)| id == bundle.id() && revision == bundle.revision())
                && runner.ruleset().digest() == bundle.ruleset_digest();
            if !runs_it {
                return Err(Error::new(
                    ErrorCode::DecisionMismatch,
                    format!(
                        "side {side} does not run the bundle {} revision {}",
                        bundle.id().as_str(),
                        bundle.revision().as_str()
                    ),
                )
                .with_hint("give each side the runner of its bundle: Runner::for_bundle"));
            }
        }
        self.bundles = Some(comparison);
        Ok(self)
    }

    /// This comparator, explaining at most `max_explained` changes:
    /// unexpected changes first, then expected ones, each in comparison
    /// order.
    #[must_use]
    pub fn with_max_explained(mut self, max_explained: usize) -> Self {
        self.max_explained = max_explained;
        self
    }

    /// The most changes explained.
    #[must_use]
    pub fn max_explained(&self) -> usize {
        self.max_explained
    }

    /// Compare side `a` running `files_a` with side `b` running `files_b`
    /// (often the same files), each within its runner's limits.
    #[must_use]
    pub fn compare(&self, files_a: &[CaseFile], files_b: &[CaseFile]) -> TestComparison {
        self.compare_until(files_a, files_b, || false)
    }

    /// Compare as [`Comparator::compare`] does, until `cancel` says so: it is
    /// asked before each item of either side, side `a`'s first, and once it
    /// cancels, both sides stop.
    #[must_use]
    pub fn compare_until(
        &self,
        files_a: &[CaseFile],
        files_b: &[CaseFile],
        mut cancel: impl FnMut() -> bool,
    ) -> TestComparison {
        let mut cancelled = false;
        let mut ask = || {
            cancelled = cancelled || cancel();
            cancelled
        };
        let (run_a, side_a) = run_side(&self.a, files_a, &mut ask);
        let (run_b, side_b) = run_side(&self.b, files_b, &mut ask);
        let relation = relation(&run_a, &run_b);
        let partial = run_a.is_partial() || run_b.is_partial();
        let (summary, mut items) = join(&side_a, &side_b, partial);
        self.explain(&side_a, &side_b, &mut items);
        TestComparison {
            relation,
            bundles: self.bundles.clone(),
            a: run_a,
            b: run_b,
            max_explained: self.max_explained,
            summary,
            items: items.into_iter().map(|item| item.item).collect(),
        }
    }

    /// Explain `max_explained` changes, unexpected ones first: each side
    /// that gave a decision, evaluated again (`spec/comparison.md`,
    /// *Explaining changes*).
    fn explain(&self, side_a: &Side<'_>, side_b: &Side<'_>, items: &mut [Listed]) {
        let mut order: Vec<&mut Listed> = items
            .iter_mut()
            .filter(|listed| listed.item.kind.is_change())
            .collect();
        // Stable: comparison order within each group.
        order.sort_by_key(|listed| listed.item.expected == Some(true));
        for listed in order.into_iter().take(self.max_explained) {
            for (runner, side, slot, outcome) in [
                (&self.a, side_a, listed.slot_a, listed.item.a.as_mut()),
                (&self.b, side_b, listed.slot_b, listed.item.b.as_mut()),
            ] {
                if let (Some(case), Some(outcome)) = (slot.and_then(|i| side.case(i)), outcome)
                    && outcome.decision.is_some()
                {
                    outcome.explanation_text = runner.explain(case).ok();
                }
            }
        }
    }
}

/// The error of two bundles that name one revision with other rules.
fn conflict(id: &BundleId, revision: &Revision) -> Error {
    Error::new(
        ErrorCode::DecisionMismatch,
        format!(
            "the bundles compared are both {} revision {}, with other rules: a revision names one set of rules",
            id.as_str(),
            revision.as_str()
        ),
    )
    .with_hint("give the other rules a new revision, then compare the two revisions")
}

/// The relation of two sides' runs (`spec/comparison.md`, *Relation*): both
/// run on this engine, so only their limits can differ.
fn relation(a: &ComparedRun, b: &ComparedRun) -> Relation {
    let same_rules = a.ruleset_digest == b.ruleset_digest;
    let same_engine = a.engine() == b.engine() && a.engine_limits == b.engine_limits;
    match (same_rules, same_engine) {
        (true, true) => Relation::SameRulesSameEngine,
        (true, false) => Relation::SameRulesOtherEngine,
        (false, true) => Relation::OtherRulesSameEngine,
        (false, false) => Relation::OtherRulesOtherEngine,
    }
}

/// One item of a side, in run order: a case of a file, or a file that
/// cannot be run, by position, so that the items do not borrow the files.
struct SideItem {
    file: usize,
    /// The case's position in its file; `None` for a file that cannot be run.
    case: Option<usize>,
    /// The result's status and what evaluation gave, when the run reached it.
    result: Option<(CaseStatus, Given)>,
}

/// The items of a side, over its case files, and the paths whose first file
/// it cannot read.
struct Side<'c> {
    files: &'c [CaseFile],
    items: Vec<SideItem>,
    unreadable: BTreeSet<Option<&'c str>>,
    /// Whether the side's rules select at most one rule.
    single_hit: bool,
}

impl<'c> Side<'c> {
    fn path(&self, index: usize) -> Option<&'c str> {
        self.files[self.items[index].file].path()
    }

    fn case(&self, index: usize) -> Option<&'c Case> {
        let item = &self.items[index];
        let cases = self.files[item.file].cases().ok()?;
        cases.items().get(item.case?)
    }

    fn id(&self, index: usize) -> Option<&'c str> {
        self.case(index).map(Case::id)
    }
}

/// Run one side: every item of its files, with the result of those it
/// reached.
fn run_side<'c>(
    runner: &Runner<'_>,
    files: &'c [CaseFile],
    cancel: &mut impl FnMut() -> bool,
) -> (ComparedRun, Side<'c>) {
    let mut items = Vec::new();
    let mut unreadable = BTreeSet::new();
    for (index, file) in files.iter().enumerate() {
        match file.cases() {
            Ok(cases) if !repeated(files, index) => {
                items.extend((0..cases.len()).map(|case| SideItem {
                    file: index,
                    case: Some(case),
                    result: None,
                }));
            }
            content => {
                if content.is_err() && !repeated(files, index) {
                    unreadable.insert(file.path());
                }
                items.push(SideItem {
                    file: index,
                    case: None,
                    result: None,
                });
            }
        }
    }
    let limits = runner.limits();
    // A comparison explains changes, not failures.
    let quiet = runner.clone().with_limits(limits.with_max_explained(0));
    let mut state = TestRunState::new();
    let mut outcomes = OutcomeCounts::default();
    let mut slots = items.iter_mut();
    while let Some((result, given)) = quiet.next_given(files, &mut state, &mut *cancel, true) {
        // The run gives its items in the order listed above, and `keep`
        // makes it give what each evaluation gave.
        let (Some(slot), Some(given)) = (slots.next(), given) else {
            break;
        };
        if slot.case.is_some() {
            match &given {
                Ok(outcome) => {
                    outcomes.decisions += 1;
                    outcomes.nothing_selected += usize::from(outcome.selected.is_empty());
                    outcomes.with_unknown += usize::from(!outcome.unknown.is_empty());
                }
                Err(_) => outcomes.error += 1,
            }
        }
        slot.result = Some((result.status, given));
    }
    let run = quiet.finish(files, state, Vec::new());
    let side = Side {
        files,
        items,
        unreadable,
        single_hit: runner.ruleset().hit_policy().single_hit(),
    };
    (
        ComparedRun::new(run, runner.ruleset().digest(), limits, outcomes),
        side,
    )
}

/// A listed item, with the positions of the side items it came from, to
/// explain it.
struct Listed {
    item: ComparedItem,
    slot_a: Option<usize>,
    slot_b: Option<usize>,
}

/// The identity of an item: its path, its id, and for items without an id,
/// its rank among those of its side with the same path.
type Key<'c> = (Option<&'c str>, Option<&'c str>, usize);

fn keys<'c>(side: &Side<'c>) -> Vec<Key<'c>> {
    let mut seen: BTreeMap<(Option<&str>, Option<&str>), usize> = BTreeMap::new();
    (0..side.items.len())
        .map(|index| {
            let (path, id) = (side.path(index), side.id(index));
            let rank = seen.entry((path, id)).or_default();
            let key = (path, id, *rank);
            *rank += 1;
            key
        })
        .collect()
}

/// Join the items of the two sides, in comparison order
/// (`spec/comparison.md`, *Joining*).
fn join(a: &Side<'_>, b: &Side<'_>, partial: bool) -> (TestComparisonSummary, Vec<Listed>) {
    let keys_b = keys(b);
    let index_b: BTreeMap<Key<'_>, usize> = keys_b
        .iter()
        .enumerate()
        .map(|(index, key)| (*key, index))
        .collect();
    let mut taken = vec![false; b.items.len()];
    let mut summary = TestComparisonSummary {
        partial,
        by_change: ChangeKind::ALL
            .iter()
            .filter(|kind| kind.is_change())
            .map(|kind| (*kind, 0))
            .collect(),
        ..TestComparisonSummary::default()
    };
    let mut listed = Vec::new();
    for (index, key) in keys(a).iter().enumerate() {
        let entry = match index_b.get(key) {
            Some(&other) => {
                taken[other] = true;
                pair(a, index, b, other)
            }
            None => alone(a, index, b, ChangeKind::Removed),
        };
        tally(&mut summary, &mut listed, entry);
    }
    for index in (0..b.items.len()).filter(|index| !taken[*index]) {
        let entry = flip(alone(b, index, a, ChangeKind::Added));
        tally(&mut summary, &mut listed, entry);
    }
    (summary, listed)
}

/// What joining an item gave.
enum Joined {
    Unchanged,
    NotCompared,
    Listed(Box<Listed>),
}

fn tally(summary: &mut TestComparisonSummary, listed: &mut Vec<Listed>, entry: Joined) {
    match entry {
        Joined::Unchanged => summary.unchanged += 1,
        Joined::NotCompared => summary.not_compared += 1,
        Joined::Listed(entry) => {
            let item = &entry.item;
            match item.expected {
                Some(true) => summary.expected += 1,
                Some(false) => summary.unexpected += 1,
                None if item.kind == ChangeKind::Added => summary.added += 1,
                None => summary.removed += 1,
            }
            if let Some((_, count)) = summary
                .by_change
                .iter_mut()
                .find(|(kind, _)| *kind == item.kind)
            {
                *count += 1;
            }
            listed.push(*entry);
        }
    }
}

fn outcome(item: &SideItem) -> Option<(CaseStatus, &Given)> {
    item.result.as_ref().map(|(status, given)| (*status, given))
}

/// An item that both sides have.
fn pair(a: &Side<'_>, index_a: usize, b: &Side<'_>, index_b: usize) -> Joined {
    let (item_a, item_b) = (&a.items[index_a], &b.items[index_b]);
    let (Some((status_a, given_a)), Some((status_b, given_b))) = (outcome(item_a), outcome(item_b))
    else {
        return Joined::NotCompared;
    };
    let differences = differences(given_a, given_b);
    if differences.is_empty() {
        // A file neither side can read, with the same error: none of its
        // cases is known, so it is not compared.
        return if item_a.case.is_none() {
            Joined::NotCompared
        } else {
            Joined::Unchanged
        };
    }
    let kind = kind(&differences);
    let unstated = match b.case(index_b) {
        // A file that cannot be read has no expectation.
        None => differences
            .iter()
            .map(|part| Unstated::Part(*part))
            .collect(),
        Some(_) if status_b != CaseStatus::Passed => vec![Unstated::Status],
        Some(case) => unstated(case.expect(), &differences, given_a, given_b, b.single_hit),
    };
    Joined::Listed(Box::new(Listed {
        item: ComparedItem {
            path: a.path(index_a).map(str::to_owned),
            id: a.id(index_a).map(str::to_owned),
            kind,
            expected: Some(unstated.is_empty()),
            differences,
            unstated,
            a: Some(CaseOutcome::new(status_a, duplicate(given_a))),
            b: Some(CaseOutcome::new(status_b, duplicate(given_b))),
        },
        slot_a: Some(index_a),
        slot_b: Some(index_b),
    }))
}

/// A copy of what an evaluation gave, for the item that lists it.
fn duplicate(given: &Given) -> Given {
    match given {
        Ok(outcome) => Ok(Outcome {
            selected: outcome.selected.clone(),
            outputs: outcome.outputs.clone(),
            output: outcome.output.clone(),
            unknown: outcome.unknown.clone(),
        }),
        Err(error) => Err(error.duplicate()),
    }
}

/// An item that only one side has, written as side `a`'s: a case is
/// `missing` (removed or added) unless the other side cannot read its file;
/// a file that cannot be read is an error.
fn alone(side: &Side<'_>, index: usize, other: &Side<'_>, missing: ChangeKind) -> Joined {
    let item = &side.items[index];
    let (kind, expected, differences) = if item.case.is_some() {
        if other.unreadable.contains(&side.path(index)) {
            return Joined::NotCompared;
        }
        (missing, None, Vec::new())
    } else if item.result.is_some() {
        (ChangeKind::Error, Some(false), vec![ChangedPart::Error])
    } else {
        return Joined::NotCompared;
    };
    // A file only one side cannot read has no expectation on side `b`.
    let unstated = differences
        .iter()
        .map(|part| Unstated::Part(*part))
        .collect();
    Joined::Listed(Box::new(Listed {
        item: ComparedItem {
            path: side.path(index).map(str::to_owned),
            id: side.id(index).map(str::to_owned),
            kind,
            expected,
            differences,
            unstated,
            a: outcome(item).map(|(status, given)| CaseOutcome::new(status, duplicate(given))),
            b: None,
        },
        slot_a: Some(index),
        slot_b: None,
    }))
}

/// The same entry with its sides swapped: an item that only side `b` has.
fn flip(entry: Joined) -> Joined {
    match entry {
        Joined::Listed(mut listed) => {
            std::mem::swap(&mut listed.item.a, &mut listed.item.b);
            std::mem::swap(&mut listed.slot_a, &mut listed.slot_b);
            Joined::Listed(listed)
        }
        other => other,
    }
}

/// An error's code and compared fields (`spec/errors.md`, *Comparing
/// errors*), as compact JSON.
fn compared_error(error: &Error) -> String {
    const COMPARED: [&str; 11] = [
        "code", "pointer", "line", "column", "rule", "rules", "path", "types", "row", "name",
        "executed",
    ];
    let mut json = write_error(error);
    if let Json::Object(members) = &mut json {
        members.retain(|(key, _)| COMPARED.contains(&key.as_str()));
    }
    to_string(&json)
}

/// The payload keys whose values differ between two outputs: those with
/// other values, and those only one of them has.
fn changed_keys<'m>(a: &'m Map, b: &'m Map) -> BTreeSet<&'m str> {
    let differs = |key: &str, here: &Map, there: &Map| {
        here.get(key)
            .is_none_or(|value| there.get(key).is_none_or(|other| !same_value(value, other)))
    };
    a.iter()
        .map(|(key, _)| key)
        .filter(|key| differs(key, a, b))
        .chain(
            b.iter()
                .map(|(key, _)| key)
                .filter(|key| differs(key, b, a)),
        )
        .collect()
}

fn same_outputs(a: &[Map], b: &[Map]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| changed_keys(a, b).is_empty())
}

fn same_single(a: &Outcome, b: &Outcome) -> bool {
    match (&a.output, &b.output) {
        (Ok(Some(a)), Ok(Some(b))) => changed_keys(a, b).is_empty(),
        (Ok(None), Ok(None)) | (Err(()), Err(())) => true,
        _ => false,
    }
}

/// The parts in which two results differ (`spec/comparison.md`, *Changes*).
fn differences(a: &Given, b: &Given) -> Vec<ChangedPart> {
    match (a, b) {
        (Err(a), Err(b)) if compared_error(a) == compared_error(b) => Vec::new(),
        (Err(_), _) | (_, Err(_)) => vec![ChangedPart::Error],
        (Ok(a), Ok(b)) => [
            (a.selected != b.selected, ChangedPart::Selected),
            (
                !same_single(a, b) || !same_outputs(&a.outputs, &b.outputs),
                ChangedPart::Output,
            ),
            (a.unknown != b.unknown, ChangedPart::Unknown),
        ]
        .into_iter()
        .filter_map(|(differs, part)| differs.then_some(part))
        .collect(),
    }
}

/// The kind of a change from the parts that differ.
fn kind(differences: &[ChangedPart]) -> ChangeKind {
    let has = |part| differences.contains(&part);
    match (
        has(ChangedPart::Error),
        has(ChangedPart::Selected),
        has(ChangedPart::Output),
    ) {
        (true, _, _) => ChangeKind::Error,
        (false, true, true) => ChangeKind::OtherSelectionOtherOutput,
        (false, true, false) => ChangeKind::OtherSelectionSameOutput,
        (false, false, true) => ChangeKind::SameSelectionOtherOutput,
        (false, false, false) => ChangeKind::OtherUnknown,
    }
}

/// The parts that differ and that the expectation of side `b`'s case,
/// which `b` passed, does not check (`spec/comparison.md`, *Expected
/// changes*): none when the change is expected.
fn unstated(
    expect: &Expectation,
    differences: &[ChangedPart],
    a: &Given,
    b: &Given,
    single_hit: bool,
) -> Vec<Unstated> {
    let states_decision =
        expect.selected().is_some() || expect.output().is_some() || expect.outputs().is_some();
    differences
        .iter()
        .copied()
        .filter(|part| {
            !match part {
                ChangedPart::Selected => expect.selected().is_some(),
                ChangedPart::Unknown => expect.unknown().is_some(),
                ChangedPart::Error => match b {
                    // `b` passed, so it raised the error it expects.
                    Err(_) => expect.error().is_some(),
                    Ok(_) => states_decision,
                },
                ChangedPart::Output => match (a, b) {
                    (Ok(a), Ok(b)) => output_covered(expect, a, b, single_hit),
                    _ => false,
                },
            }
        })
        .map(Unstated::Part)
        .collect()
}

/// The keys an expected output names.
fn named(expected: &Map) -> BTreeSet<&str> {
    expected.iter().map(|(key, _)| key).collect()
}

/// Whether the expectation names every key of `changed`, through `output`
/// or, under a single-hit policy at the first position, `outputs`.
fn names_all(
    expect: &Expectation,
    position: usize,
    changed: &BTreeSet<&str>,
    single_hit: bool,
) -> bool {
    let by_outputs = expect
        .outputs()
        .and_then(|outputs| outputs.get(position))
        .is_some_and(|expected| changed.is_subset(&named(expected)));
    let by_output = single_hit
        && position == 0
        && matches!(expect.output(), Some(ExpectedOutput::Includes(expected)) if changed.is_subset(&named(expected)));
    by_outputs || by_output
}

/// Whether the expectation checks the change of the outputs between `a` and
/// `b` (`spec/comparison.md`, *Expected changes*): every payload key that
/// differs is named, a switch between an output and none is stated, and a
/// change in the number of outputs is stated.
fn output_covered(expect: &Expectation, a: &Outcome, b: &Outcome, single_hit: bool) -> bool {
    let single = match (&a.output, &b.output) {
        (Ok(Some(x)), Ok(Some(y))) => {
            let changed = changed_keys(x, y);
            // The winner's output, or the aggregate, whose keys only
            // `output` names; under a single-hit policy it is the first of
            // the outputs too.
            let by_output = matches!(
                expect.output(),
                Some(ExpectedOutput::Includes(expected)) if changed.is_subset(&named(expected))
            );
            let by_outputs = single_hit
                && expect
                    .outputs()
                    .and_then(|outputs| outputs.first())
                    .is_some_and(|expected| changed.is_subset(&named(expected)));
            changed.is_empty() || by_output || by_outputs
        }
        (Ok(None), Ok(None)) | (Err(()), Err(())) => true,
        // Some output and none, or a single output and none at all.
        _ => expect.output().is_some() || expect.outputs().is_some(),
    };
    let count = a.outputs.len() == b.outputs.len()
        || expect.outputs().is_some()
        || (single_hit && expect.output().is_some());
    let positions = a
        .outputs
        .iter()
        .zip(&b.outputs)
        .enumerate()
        .all(|(i, (x, y))| {
            let changed = changed_keys(x, y);
            changed.is_empty() || names_all(expect, i, &changed, single_hit)
        });
    single && count && positions
}
