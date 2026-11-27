//! Running test cases (`spec/tests.md`, *Running cases*, *Test runs*).
//!
//! A [`Runner`] evaluates rules on the cases of [`CaseFile`]s and gives one
//! [`CaseResult`] per case, in run order. [`Runner::iter`] gives them one at a
//! time ([`CaseRun`], an iterator) and stops at a limit or when the caller
//! cancels, marking the run partial; [`Runner::run`] collects them into a
//! [`TestRun`], the content of a test run document. A caller that owns its
//! rules and files (a binding) drives the same run with an owned cursor,
//! [`TestRunState`], through [`Runner::next_result`].

use super::file::CaseFile;
use super::statistics::{Counts, Layout, Statistics, outcomes};
use super::{Case, Expectation, ExpectedOutput};
use crate::bundle::{Bundle, BundleId, BundleRules, ENGINE_NAME, ENGINE_VERSION, EngineLimits};
use crate::digest::Digest;
use crate::error::{Error, ErrorCode, Result};
use crate::explain::{Status, explain, statuses};
use crate::format::write_value;
use crate::json::to_string;
use crate::logic::LogicName;
use crate::rules::{Aggregate, Aggregated, Decision, RuleSet, RulesSource};
use crate::table::{DecisionTable, TablePolicy};
use crate::value::{Map, Value};

/// The failed cases a run explains unless the caller says otherwise.
pub const DEFAULT_MAX_EXPLAINED: usize = 5;

/// The limits of a test run (`spec/tests.md`, *Batches and interruption*,
/// *Explaining failures*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TestRunLimits {
    /// The most results given, unreadable files included; unbounded when
    /// `None`.
    pub max_results: Option<usize>,
    /// The most steps, one per rule for each case; unbounded when `None`.
    pub max_steps: Option<usize>,
    /// The most failed cases explained, the first ones in run order.
    pub max_explained: usize,
}

impl Default for TestRunLimits {
    fn default() -> Self {
        Self {
            max_results: None,
            max_steps: None,
            max_explained: DEFAULT_MAX_EXPLAINED,
        }
    }
}

impl TestRunLimits {
    /// These limits, giving at most `max_results` results.
    #[must_use]
    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = Some(max_results);
        self
    }

    /// These limits, taking at most `max_steps` steps.
    #[must_use]
    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = Some(max_steps);
        self
    }

    /// These limits, explaining at most `max_explained` failed cases.
    #[must_use]
    pub fn with_max_explained(mut self, max_explained: usize) -> Self {
        self.max_explained = max_explained;
        self
    }
}

/// Why a run stopped before its last result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Interruption {
    /// `max_results` results had been given.
    MaxResults,
    /// The next case would have taken the steps past `max_steps`.
    MaxSteps,
    /// The caller cancelled the run, or stopped taking its results.
    Cancelled,
}

impl Interruption {
    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Interruption::MaxResults => "max_results",
            Interruption::MaxSteps => "max_steps",
            Interruption::Cancelled => "cancelled",
        }
    }
}

/// How a partial run stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Interrupted {
    /// Why it stopped.
    pub reason: Interruption,
    /// The results it did not give, among the case files given to it.
    pub remaining: usize,
}

/// The counts of a run's results, and whether it was interrupted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Summary {
    /// The results `passed`.
    pub passed: usize,
    /// The results `failed`.
    pub failed: usize,
    /// The results `error`, case files that cannot be read included.
    pub error: usize,
    /// How the run stopped, when it is partial.
    pub interrupted: Option<Interrupted>,
}

impl Summary {
    /// The number of results.
    #[must_use]
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.error
    }

    /// Whether the run was interrupted before its last result.
    #[must_use]
    pub fn is_partial(&self) -> bool {
        self.interrupted.is_some()
    }

    fn count(&mut self, status: CaseStatus) {
        match status {
            CaseStatus::Passed => self.passed += 1,
            CaseStatus::Failed => self.failed += 1,
            CaseStatus::Error => self.error += 1,
        }
    }
}

/// The status of a result (`spec/tests.md`, *Matching*): a closed set, like
/// an audit's severities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaseStatus {
    /// Every key of the expectation matches.
    Passed,
    /// Some key of the expectation does not match.
    Failed,
    /// Evaluation raised an error the case does not expect, or the case
    /// file cannot be run.
    Error,
}

impl CaseStatus {
    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CaseStatus::Passed => "passed",
            CaseStatus::Failed => "failed",
            CaseStatus::Error => "error",
        }
    }
}

/// A key of an expectation (`spec/tests.md`, *Expectations*), in the order
/// they are checked. Later versions may add keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ExpectationKey {
    /// `selected`.
    Selected,
    /// `output`.
    Output,
    /// `outputs`.
    Outputs,
    /// `unknown`.
    Unknown,
    /// `error`.
    Error,
}

impl ExpectationKey {
    /// The key in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ExpectationKey::Selected => "selected",
            ExpectationKey::Output => "output",
            ExpectationKey::Outputs => "outputs",
            ExpectationKey::Unknown => "unknown",
            ExpectationKey::Error => "error",
        }
    }
}

/// A key of an expectation that does not match, with what was expected and
/// what was given (`spec/tests.md`, *Test runs*).
#[derive(Debug)]
#[non_exhaustive]
pub enum Mismatch {
    /// The selected rules, by effective id, in selection order.
    Selected {
        /// The rules expected.
        expected: Vec<String>,
        /// The rules selected.
        actual: Vec<String>,
    },
    /// The single output.
    Output {
        /// The output expected.
        expected: ExpectedOutput,
        /// The decision's output with all its keys; `None` when no rule was
        /// selected.
        actual: Option<Map>,
    },
    /// A single output expected of a decision that has none.
    NoSingleOutput {
        /// The output expected.
        expected: ExpectedOutput,
        /// The hit policy of the rules tested.
        hit_policy: TablePolicy,
    },
    /// The outputs of the selected rules.
    Outputs {
        /// The outputs expected.
        expected: Vec<Map>,
        /// The outputs of the selected rules, in selection order.
        actual: Vec<Map>,
    },
    /// The rules whose condition is unknown, by effective id.
    Unknown {
        /// The rules expected, sorted by code point.
        expected: Vec<String>,
        /// The rules unknown, sorted by code point.
        actual: Vec<String>,
    },
    /// The error evaluation must raise.
    Error {
        /// The code expected.
        expected: ErrorCode,
        /// The error raised, or `None` when evaluation succeeded.
        actual: Option<Error>,
    },
}

impl Mismatch {
    /// The key that does not match.
    #[must_use]
    pub fn key(&self) -> ExpectationKey {
        match self {
            Mismatch::Selected { .. } => ExpectationKey::Selected,
            Mismatch::Output { .. } | Mismatch::NoSingleOutput { .. } => ExpectationKey::Output,
            Mismatch::Outputs { .. } => ExpectationKey::Outputs,
            Mismatch::Unknown { .. } => ExpectationKey::Unknown,
            Mismatch::Error { .. } => ExpectationKey::Error,
        }
    }

    /// Why nothing was given to compare, for an output expected of a
    /// decision that has no single output.
    #[must_use]
    pub fn reason(&self) -> Option<String> {
        match self {
            Mismatch::NoSingleOutput { hit_policy, .. } => Some(format!(
                "the decision has no single output ({})",
                hit_policy.as_str()
            )),
            _ => None,
        }
    }
}

/// The result of one case, or of a case file that cannot be run.
#[derive(Debug)]
#[non_exhaustive]
pub struct CaseResult {
    /// The path of the case's file, when it has one.
    pub path: Option<String>,
    /// The case's id; `None` for a case file that cannot be run.
    pub id: Option<String>,
    /// Passed, failed or error.
    pub status: CaseStatus,
    /// For a failed case, the keys that do not match, in the order of the
    /// expectation's keys.
    pub mismatches: Vec<Mismatch>,
    /// For an error, the error.
    pub error: Option<Error>,
    /// For a failed case explained, the text of its decision's explanation
    /// (`spec/explain.md`, *Text*).
    pub explanation_text: Option<String>,
}

impl CaseResult {
    fn new(path: Option<&str>, id: Option<&str>, status: CaseStatus) -> Self {
        Self {
            path: path.map(str::to_owned),
            id: id.map(str::to_owned),
            status,
            mismatches: Vec::new(),
            error: None,
            explanation_text: None,
        }
    }
}

/// What a decision gives that an expectation checks: each rule's status,
/// without result trees (`spec/tests.md`, *Explaining failures*).
pub(crate) struct Outcome {
    pub(crate) selected: Vec<String>,
    pub(crate) outputs: Vec<Map>,
    /// The single output, or `Err` when the decision has none.
    pub(crate) output: std::result::Result<Option<Map>, ()>,
    pub(crate) unknown: Vec<String>,
}

/// What evaluating a case gave, reduced: a decision's parts, or the error
/// (of the evaluation, or of a file that cannot be run).
pub(crate) type Given = std::result::Result<Outcome, Error>;

impl Outcome {
    fn of(decision: &Decision<'_>) -> Self {
        let selected = decision
            .selected()
            .map(|r| r.rule.effective_id().to_owned())
            .collect();
        let outputs: Vec<Map> = decision
            .selected()
            .map(|r| r.output().into_iter().collect())
            .collect();
        let output = match decision.aggregated() {
            Some(Aggregated::Values(map)) => Ok(Some(map.clone())),
            Some(Aggregated::NothingSelected) => Ok(None),
            None if decision.ruleset().hit_policy().single_hit() => Ok(outputs.first().cloned()),
            None => Err(()),
        };
        let mut unknown: Vec<String> = decision
            .results()
            .iter()
            .zip(statuses(decision))
            .filter(|(_, status)| *status == Status::Unknown)
            .map(|(r, _)| r.rule.effective_id().to_owned())
            .collect();
        // By code point: Rust orders strings by their UTF-8 bytes.
        unknown.sort();
        Self {
            selected,
            outputs,
            output,
            unknown,
        }
    }
}

/// Whether two values have the same canonical bytes (`spec/tests.md`,
/// *Values*): nulls are written `null`, whatever their reason.
pub(crate) fn same_value(expected: &Value, actual: &Value) -> bool {
    to_string(&write_value(expected)) == to_string(&write_value(actual))
}

/// Whether `actual` has every key of `expected`, with an equal value.
fn includes(expected: &Map, actual: &Map) -> bool {
    expected
        .iter()
        .all(|(key, value)| actual.get(key).is_some_and(|a| same_value(value, a)))
}

/// Whether the file at `index` repeats the path of an earlier one, or is a
/// second file without a path (`spec/tests.md`, *Running cases*).
pub(crate) fn repeated(files: &[CaseFile], index: usize) -> bool {
    files[..index]
        .iter()
        .any(|earlier| earlier.path() == files[index].path())
}

/// The results the file at `index` gives in a run of `files`.
fn file_results(files: &[CaseFile], index: usize) -> usize {
    if repeated(files, index) {
        1
    } else {
        files[index].results()
    }
}

/// The error of a file that repeats an earlier file's path.
fn repeated_path(path: Option<&str>) -> Error {
    let message = match path {
        Some(path) => format!("the case file {path} is given twice in the run"),
        None => "two case files without a path are given in the run".to_owned(),
    };
    Error::new(ErrorCode::FormatError, message).with_hint(
        "give each case file once, each with its own path: a case is its path and its id",
    )
}

/// The next item of a run.
enum Item<'c> {
    Case(&'c Case),
    Unreadable(Error),
}

/// Where a run is, owned by the caller: the cursor that
/// [`Runner::next_result`] moves over the same case files, with the counts
/// of the summary and, when the runner gathers them, of the statistics. A
/// binding keeps it next to its rules and files.
#[derive(Clone, Debug, Default)]
pub struct TestRunState {
    /// The file of the next item.
    file: usize,
    /// The next case of that file; for a file that gives one error, 0 until
    /// its result is given.
    case: usize,
    steps: usize,
    explained: usize,
    summary: Summary,
    finished: bool,
    /// The layout and counts of the statistics, made at the first result of
    /// a runner that gathers them.
    statistics: Option<(Layout, Counts)>,
}

impl TestRunState {
    /// The state of a run that has not started.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The summary of the results given so far.
    #[must_use]
    pub fn summary(&self) -> &Summary {
        &self.summary
    }

    /// Whether the run gives no more results: complete, or interrupted.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The results not given yet, among `files`, the files of the run.
    #[must_use]
    pub fn remaining(&self, files: &[CaseFile]) -> usize {
        if self.finished || self.file >= files.len() {
            return 0;
        }
        let here = file_results(files, self.file).saturating_sub(self.case);
        here + (self.file + 1..files.len())
            .map(|i| file_results(files, i))
            .sum::<usize>()
    }

    /// The next item, skipping case files that are done.
    fn peek<'c>(&mut self, files: &'c [CaseFile]) -> Option<Item<'c>> {
        while let Some(file) = files.get(self.file) {
            if repeated(files, self.file) {
                if self.case == 0 {
                    return Some(Item::Unreadable(repeated_path(file.path())));
                }
            } else {
                match file.cases() {
                    Err(error) if self.case == 0 => {
                        return Some(Item::Unreadable(error.duplicate()));
                    }
                    Ok(cases) if self.case < cases.len() => {
                        return Some(Item::Case(&cases.items()[self.case]));
                    }
                    _ => {}
                }
            }
            self.file += 1;
            self.case = 0;
        }
        None
    }

    fn stop(&mut self, files: &[CaseFile], reason: Interruption) {
        if self.finished {
            return;
        }
        let remaining = self.remaining(files);
        if remaining > 0 {
            self.summary.interrupted = Some(Interrupted { reason, remaining });
        }
        self.finished = true;
    }
}

/// Runs test cases against rules (`spec/tests.md`, *Running cases*).
#[derive(Clone, Debug)]
pub struct Runner<'r> {
    ruleset: &'r RuleSet,
    /// The decision table tested, when it is one.
    table: Option<&'r DecisionTable>,
    source: RulesSource,
    hit_policy: TablePolicy,
    limits: TestRunLimits,
    engine_limits: EngineLimits,
    bundle: Option<(BundleId, crate::bundle::Revision)>,
    /// Whether the runner gathers statistics.
    statistics: bool,
}

impl<'r> Runner<'r> {
    /// A runner of cases against a rule set, with the default limits.
    #[must_use]
    pub fn new(ruleset: &'r RuleSet) -> Self {
        Self {
            ruleset,
            table: None,
            source: RulesSource::RuleSet,
            hit_policy: TablePolicy::of_ruleset(ruleset.hit_policy()),
            limits: TestRunLimits::default(),
            engine_limits: EngineLimits::default(),
            bundle: None,
            statistics: false,
        }
    }

    /// A runner of cases against a decision table, through the rule set it
    /// compiles to.
    #[must_use]
    pub fn for_table(table: &'r DecisionTable) -> Self {
        Self {
            table: Some(table),
            source: RulesSource::DecisionTable,
            hit_policy: table.config().hit_policy,
            ..Self::new(table.ruleset())
        }
    }

    /// A runner of cases against a bundle's rules, under the limits the
    /// bundle records (`spec/tests.md`, *Case files of a bundle*); its case
    /// files come from [`CaseFile::bundle_files`].
    #[must_use]
    pub fn for_bundle(bundle: &'r Bundle) -> Self {
        Self {
            engine_limits: *bundle.engine().limits(),
            bundle: Some((bundle.id().clone(), bundle.revision().clone())),
            ..Self::for_bundle_rules(bundle)
        }
    }

    /// A runner of cases against a bundle's rules alone, as for a rule set or
    /// a decision table: for case files that are not the bundle's own, read
    /// with the limits the caller declares ([`Runner::with_engine_limits`],
    /// the defaults otherwise), so the test run has no `bundle`.
    #[must_use]
    pub fn for_bundle_rules(bundle: &'r Bundle) -> Self {
        match bundle.rules() {
            BundleRules::DecisionTable(table) => Self::for_table(table),
            BundleRules::RuleSet(ruleset) => Self::new(ruleset),
        }
    }

    /// This runner with other limits.
    #[must_use]
    pub fn with_limits(mut self, limits: TestRunLimits) -> Self {
        self.limits = limits;
        self
    }

    /// This runner, recording that its case files were read with `limits`
    /// (the test run's `engine`; a bundle's runner records the bundle's).
    #[must_use]
    pub fn with_engine_limits(mut self, limits: EngineLimits) -> Self {
        self.engine_limits = limits;
        self
    }

    /// The limits.
    #[must_use]
    pub fn limits(&self) -> TestRunLimits {
        self.limits
    }

    /// This runner, gathering the statistics of its runs or not
    /// (`spec/tests.md`, *Statistics*): counted from each case's decision
    /// before the next case is evaluated, and given in the test run
    /// ([`TestRun::statistics`]) and by [`Runner::statistics`]. Building a
    /// runner costs nothing more: the run lays its counts out at its first
    /// result, in its [`TestRunState`].
    ///
    /// ```
    /// use cleanrules::cases::{Case, CaseFile, Cases, Runner};
    /// use cleanrules::{Map, Value};
    ///
    /// let rules = cleanrules::text::parse("RULE adult IF age >= 18", &Default::default())?
    ///     .into_ruleset()?;
    /// let mut input = Map::new();
    /// input.insert("age", Value::Int(20));
    /// let files = [CaseFile::new(Cases::new([Case::new("one", Value::Map(input))?])?)];
    /// let run = Runner::new(&rules).with_statistics(true).run(&files);
    /// let statistics = run.statistics.expect("asked for");
    /// assert_eq!((statistics.rules[0].fired, statistics.rules[0].not_fired), (1, 0));
    /// assert!(statistics.to_string().starts_with("Statistics of 1 case: 1 decision"));
    /// # Ok::<(), cleanrules::Error>(())
    /// ```
    #[must_use]
    pub fn with_statistics(mut self, gather: bool) -> Self {
        self.statistics = gather;
        self
    }

    /// Whether the runner gathers statistics.
    #[must_use]
    pub fn gathers_statistics(&self) -> bool {
        self.statistics
    }

    /// The statistics of the results that `state` has given so far, when
    /// the runner gathers them: of a finished run, its statistics. `None`
    /// when the runner does not gather them, or when `state` gave results
    /// without counting them (a runner that did not gather them moved it).
    #[must_use]
    pub fn statistics(&self, state: &TestRunState) -> Option<Statistics> {
        if !self.statistics {
            return None;
        }
        let prefix = if self.bundle.is_some() { "/bundle" } else { "" };
        if let Some((_, counts)) = &state.statistics {
            return Some(counts.statistics(self.ruleset, self.table, prefix));
        }
        debug_assert_eq!(
            state.summary.total(),
            0,
            "statistics asked of a run whose results were not counted"
        );
        (state.summary.total() == 0).then(|| {
            Counts::new(&Layout::new(self.ruleset, self.table)).statistics(
                self.ruleset,
                self.table,
                prefix,
            )
        })
    }

    /// Run `files` one result at a time, in run order (`spec/tests.md`,
    /// *Batches and interruption*).
    #[must_use]
    pub fn iter<'c>(&'c self, files: &'c [CaseFile]) -> CaseRun<'c> {
        CaseRun {
            runner: self,
            files,
            state: TestRunState::new(),
            cancel: None,
        }
    }

    /// Run every case of `files`, within the limits.
    #[must_use]
    pub fn run(&self, files: &[CaseFile]) -> TestRun {
        let mut run = self.iter(files);
        let results: Vec<CaseResult> = run.by_ref().collect();
        run.into_test_run(results)
    }

    /// Run every case of `files`, within the limits, until `cancel` says
    /// so: it is asked before each result.
    #[must_use]
    pub fn run_until(&self, files: &[CaseFile], cancel: impl FnMut() -> bool) -> TestRun {
        let mut run = self.iter(files).cancel_when(cancel);
        let results: Vec<CaseResult> = run.by_ref().collect();
        run.into_test_run(results)
    }

    /// The next result of the run of `files` that `state` is at, moving it
    /// on; `None` once the run is finished, complete or interrupted. `cancel`
    /// is asked before the item, as `CaseRun::cancel_when` asks.
    pub fn next_result(
        &self,
        files: &[CaseFile],
        state: &mut TestRunState,
        cancel: impl FnOnce() -> bool,
    ) -> Option<CaseResult> {
        self.next_given(files, state, cancel, false)
            .map(|(result, _)| result)
    }

    /// The next result, as [`Runner::next_result`] gives it, with what the
    /// case's evaluation gave when `keep` is set (a comparison's record).
    pub(crate) fn next_given(
        &self,
        files: &[CaseFile],
        state: &mut TestRunState,
        cancel: impl FnOnce() -> bool,
        keep: bool,
    ) -> Option<(CaseResult, Option<Given>)> {
        if state.finished {
            return None;
        }
        // Counting starts with the run: a state that gave results without
        // counting them never counts.
        if self.statistics && state.statistics.is_none() && state.summary.total() == 0 {
            let layout = Layout::new(self.ruleset, self.table);
            let counts = Counts::new(&layout);
            state.statistics = Some((layout, counts));
        }
        let Some(item) = state.peek(files) else {
            state.finished = true;
            return None;
        };
        let cost = match item {
            Item::Case(_) => self.ruleset.rules().len(),
            Item::Unreadable(_) => 0,
        };
        if let Some(reason) = self.interruption(state, cost, cancel) {
            state.stop(files, reason);
            return None;
        }
        state.steps += cost;
        state.case += 1;
        let path = files[state.file].path();
        let (result, given) = match item {
            Item::Case(case) => {
                let explain = state.explained < self.limits.max_explained;
                let counts = if self.statistics {
                    state.statistics.as_mut()
                } else {
                    None
                };
                let (result, given) = self.check(path, case, explain, counts, keep);
                if result.explanation_text.is_some() {
                    state.explained += 1;
                }
                (result, given)
            }
            Item::Unreadable(error) => {
                let given = keep.then(|| Err(error.duplicate()));
                let mut result = CaseResult::new(path, None, CaseStatus::Error);
                result.error = Some(error);
                (result, given)
            }
        };
        state.summary.count(result.status);
        Some((result, given))
    }

    /// Stop the run of `files` that `state` is at: it gives no more results,
    /// and it is partial (cancelled) when a result remained.
    pub fn stop(&self, files: &[CaseFile], state: &mut TestRunState) {
        state.stop(files, Interruption::Cancelled);
    }

    /// The test run of `files` that `state` ended, with `results`: the
    /// results it gave, in order. A run that is not finished is stopped
    /// first, so a run the caller left early is partial (cancelled).
    #[must_use]
    pub fn test_run(
        &self,
        files: &[CaseFile],
        mut state: TestRunState,
        results: Vec<CaseResult>,
    ) -> TestRun {
        state.stop(files, Interruption::Cancelled);
        debug_assert_eq!(
            results.len(),
            state.summary.total(),
            "the results given, in order"
        );
        self.finish(files, state, results)
    }

    /// The test run of `files` that `state` ended, stopping it first, with
    /// `results`, which a comparison leaves empty.
    pub(crate) fn finish(
        &self,
        files: &[CaseFile],
        mut state: TestRunState,
        results: Vec<CaseResult>,
    ) -> TestRun {
        state.stop(files, Interruption::Cancelled);
        let statistics = self.statistics(&state);
        TestRun {
            source: self.source,
            hit_policy: self.hit_policy,
            aggregate: self.ruleset.aggregate(),
            logic: self.ruleset.logic(),
            strict: self.ruleset.strict(),
            rule_count: self.ruleset.rules().len(),
            ruleset_digest: None,
            engine_limits: self.engine_limits,
            bundle: self.bundle.clone(),
            files: files
                .iter()
                .map(|file| TestRunFile {
                    path: file.path().map(str::to_owned),
                    digest: file.digest(),
                })
                .collect(),
            limits: self.limits,
            summary: state.summary,
            results,
            statistics,
            digest: self.ruleset.digest(),
        }
    }

    /// Run one case, without a path; a failure is explained when
    /// `max_explained` is not 0. It is no run's case, so no statistics count
    /// it.
    #[must_use]
    pub fn run_case(&self, case: &Case) -> CaseResult {
        self.check(None, case, self.limits.max_explained > 0, None, false)
            .0
    }

    /// The rules being tested.
    pub(crate) fn ruleset(&self) -> &'r RuleSet {
        self.ruleset
    }

    /// The bundle's id and revision, for a bundle's runner.
    pub(crate) fn bundle(&self) -> Option<&(BundleId, crate::bundle::Revision)> {
        self.bundle.as_ref()
    }

    /// Evaluate a case's input again, as a run does: the decision of a
    /// failure, to explain or draw (`spec/tests.md`, *Explaining failures*).
    /// No statistics count it.
    ///
    /// # Errors
    ///
    /// The error evaluation raises.
    pub fn evaluate(&self, case: &Case) -> Result<Decision<'r>> {
        self.ruleset.evaluate(case.input())
    }

    /// The text of the explanation of the decision a case's input gives,
    /// evaluated again (`spec/tests.md`, *Explaining failures*).
    ///
    /// # Errors
    ///
    /// The error evaluation raises.
    pub fn explain(&self, case: &Case) -> Result<String> {
        Ok(explain(&self.evaluate(case)?).to_string())
    }

    /// Why the run stops before an item that costs `cost` steps, if it does
    /// (`spec/tests.md`, *Batches and interruption*: in this order).
    fn interruption(
        &self,
        state: &TestRunState,
        cost: usize,
        cancel: impl FnOnce() -> bool,
    ) -> Option<Interruption> {
        if self
            .limits
            .max_results
            .is_some_and(|max| state.summary.total() >= max)
        {
            return Some(Interruption::MaxResults);
        }
        if self
            .limits
            .max_steps
            .is_some_and(|max| state.steps.saturating_add(cost) > max)
        {
            return Some(Interruption::MaxSteps);
        }
        cancel().then_some(Interruption::Cancelled)
    }

    /// Run one case (`spec/tests.md`, *Matching*), counting its decision
    /// in `counts` when the run gathers statistics, and keeping what its
    /// evaluation gave when `keep` is set.
    fn check(
        &self,
        path: Option<&str>,
        case: &Case,
        explain_failure: bool,
        counts: Option<&mut (Layout, Counts)>,
        keep: bool,
    ) -> (CaseResult, Option<Given>) {
        let expect = case.expect();
        let mut result = CaseResult::new(path, Some(case.id()), CaseStatus::Passed);
        let evaluated = self.ruleset.evaluate(case.input());
        if let Some(gathered) = counts {
            let (layout, counts) = gathered;
            match &evaluated {
                Ok(decision) => counts.decision(layout, outcomes(decision, layout)),
                Err(_) => counts.error(),
            }
        }
        let mut given = None;
        match evaluated {
            Err(error) => {
                if keep {
                    given = Some(Err(error.duplicate()));
                }
                match expect.error() {
                    None => {
                        result.status = CaseStatus::Error;
                        result.error = Some(error);
                    }
                    Some(code) if error.code() == code => {}
                    Some(code) => {
                        result.status = CaseStatus::Failed;
                        result.mismatches.push(Mismatch::Error {
                            expected: code,
                            actual: Some(error),
                        });
                    }
                }
            }
            Ok(decision) => {
                let outcome = (keep || expect.error().is_none()).then(|| Outcome::of(&decision));
                result.mismatches = match (expect.error(), &outcome) {
                    (Some(code), _) => vec![Mismatch::Error {
                        expected: code,
                        actual: None,
                    }],
                    (None, Some(outcome)) => self.compare(expect, outcome),
                    (None, None) => Vec::new(),
                };
                if !result.mismatches.is_empty() {
                    result.status = CaseStatus::Failed;
                    if explain_failure {
                        result.explanation_text = Some(explain(&decision).to_string());
                    }
                }
                if keep {
                    given = outcome.map(Ok);
                }
            }
        }
        (result, given)
    }

    /// The keys of `expect` that `outcome` does not match, in the order of
    /// the expectation's keys.
    fn compare(&self, expect: &Expectation, outcome: &Outcome) -> Vec<Mismatch> {
        let mut mismatches = Vec::new();
        if let Some(expected) = expect.selected()
            && expected != outcome.selected.as_slice()
        {
            mismatches.push(Mismatch::Selected {
                expected: expected.to_vec(),
                actual: outcome.selected.clone(),
            });
        }
        if let Some(expected) = expect.output() {
            match &outcome.output {
                Err(()) => mismatches.push(Mismatch::NoSingleOutput {
                    expected: expected.clone(),
                    hit_policy: self.hit_policy,
                }),
                Ok(actual) => {
                    let matches = match (expected, actual) {
                        (ExpectedOutput::NothingSelected, None) => true,
                        (ExpectedOutput::Includes(keys), Some(output)) => includes(keys, output),
                        _ => false,
                    };
                    if !matches {
                        mismatches.push(Mismatch::Output {
                            expected: expected.clone(),
                            actual: actual.clone(),
                        });
                    }
                }
            }
        }
        if let Some(expected) = expect.outputs() {
            let matches = expected.len() == outcome.outputs.len()
                && expected
                    .iter()
                    .zip(&outcome.outputs)
                    .all(|(keys, output)| includes(keys, output));
            if !matches {
                mismatches.push(Mismatch::Outputs {
                    expected: expected.to_vec(),
                    actual: outcome.outputs.clone(),
                });
            }
        }
        if let Some(expected) = expect.unknown()
            && !expected.iter().eq(outcome.unknown.iter())
        {
            mismatches.push(Mismatch::Unknown {
                expected: expected.iter().cloned().collect(),
                actual: outcome.unknown.clone(),
            });
        }
        mismatches
    }
}

/// A run in progress, over borrowed rules and case files: an iterator over
/// its results, in run order (`spec/tests.md`, *Batches and
/// interruption*). It keeps only the counts of its summary between results;
/// it ends after the last result, or when a limit or the caller interrupts
/// it, and is then partial. [`Runner::next_result`] drives the same run
/// with a [`TestRunState`] the caller owns.
pub struct CaseRun<'c> {
    runner: &'c Runner<'c>,
    files: &'c [CaseFile],
    state: TestRunState,
    cancel: Option<Box<dyn FnMut() -> bool + 'c>>,
}

impl std::fmt::Debug for CaseRun<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaseRun")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl<'c> CaseRun<'c> {
    /// This run, which asks `cancel` before each result and stops, partial,
    /// when it says so. It is never asked while a case is evaluated.
    #[must_use]
    pub fn cancel_when(mut self, cancel: impl FnMut() -> bool + 'c) -> Self {
        self.cancel = Some(Box::new(cancel));
        self
    }

    /// Stop the run now: it gives no more results, and it is partial
    /// (cancelled) when a result remained.
    pub fn cancel(&mut self) {
        self.runner.stop(self.files, &mut self.state);
    }

    /// The summary of the results given so far.
    #[must_use]
    pub fn summary(&self) -> &Summary {
        self.state.summary()
    }

    /// Whether the run gives no more results: complete, or interrupted.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.state.is_finished()
    }

    /// The results not given yet.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.state.remaining(self.files)
    }

    /// The statistics of the results given so far, when the runner gathers
    /// them ([`Runner::with_statistics`]).
    #[must_use]
    pub fn statistics(&self) -> Option<Statistics> {
        self.runner.statistics(&self.state)
    }

    /// The test run of this run, with `results`: the results it gave, in
    /// order. A run that is not finished, because the caller stopped taking
    /// its results, is stopped first and so is partial (cancelled).
    #[must_use]
    pub fn into_test_run(self, results: Vec<CaseResult>) -> TestRun {
        self.runner.test_run(self.files, self.state, results)
    }
}

impl Iterator for CaseRun<'_> {
    type Item = CaseResult;

    fn next(&mut self) -> Option<CaseResult> {
        let cancel = &mut self.cancel;
        self.runner.next_result(self.files, &mut self.state, || {
            cancel.as_mut().is_some_and(|cancel| cancel())
        })
    }
}

/// One case file of a run, as a test run lists it: its path, and the digest
/// of its bytes when it was read from bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TestRunFile {
    /// The path, when the file has one.
    pub path: Option<String>,
    /// The SHA-256 digest of the bytes it was read from.
    pub digest: Option<Digest>,
}

/// The results of a run and their summary: the content of a test run
/// document (`spec/tests.md`, *Test runs*).
#[derive(Debug)]
#[non_exhaustive]
pub struct TestRun {
    /// What was tested.
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
    /// The number of rules tested.
    pub rule_count: usize,
    /// The rule set digest of the rules tested, when asked for
    /// ([`TestRun::with_ruleset_digest`]).
    pub ruleset_digest: Option<Digest>,
    /// The limits of this engine the case files were read with (the
    /// document's `engine`, with this engine's name and version).
    pub engine_limits: EngineLimits,
    /// The bundle's id and revision, for a bundle's cases.
    pub bundle: Option<(BundleId, crate::bundle::Revision)>,
    /// The case files given to the run, in order.
    pub files: Vec<TestRunFile>,
    /// The limits it ran with.
    pub limits: TestRunLimits,
    /// The counts, and how a partial run stopped.
    pub summary: Summary,
    /// The results, in run order.
    pub results: Vec<CaseResult>,
    /// The statistics, when the runner gathered them
    /// ([`Runner::with_statistics`]).
    pub statistics: Option<Statistics>,
    /// The rule set digest, kept for [`TestRun::with_ruleset_digest`].
    digest: Digest,
}

impl TestRun {
    /// Whether every case ran and passed: no failure, no error, and not
    /// partial. It is true for a run of no cases.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.summary.failed == 0 && self.summary.error == 0 && !self.summary.is_partial()
    }

    /// The same test run, carrying the rule set digest or not
    /// (`spec/tests.md`, *Test runs*), as audits and decisions do.
    #[must_use]
    pub fn with_ruleset_digest(mut self, carry: bool) -> Self {
        self.ruleset_digest = carry.then_some(self.digest);
        self
    }

    /// The name and version of the engine that ran it: this one.
    #[must_use]
    pub fn engine(&self) -> (&'static str, &'static str) {
        (ENGINE_NAME, ENGINE_VERSION)
    }
}
