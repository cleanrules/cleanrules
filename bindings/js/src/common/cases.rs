//! Test cases and the case runner (`spec/tests.md`) for both front ends: the
//! core reads, checks, writes and runs them; this code builds case files from
//! the reader's encoding, gives their parts back in the encoding of `out`,
//! and drives a run over rules and case files it owns, one result at a time
//! (`lib/api/cases.js`).
//!
//! Three names, the same in every language: a `CaseFileRef` is a bundle's
//! reference to a case file (`bundles`); the `Cases` are the content of a
//! case file document (a [`DocumentCore`] that holds them); a [`CaseFileCore`]
//! is one file given to a run, its path and its cases or the error that keeps
//! it from being read.
//!
//! Internal: the JavaScript half checks the arguments' types, so these
//! functions take what it already checked.

use std::sync::Arc;

use cleanrules::bundle::{Bundle, ENGINE_NAME, ENGINE_VERSION};
use cleanrules::cases::{
    Case, CaseFile, CaseFileBytes, CaseResult, Cases, Expectation, ExpectedOutput, Mismatch,
    Runner, Summary, TestRunLimits, TestRunState,
};
use cleanrules::format::{
    Document, DocumentFormat, write_case_result, write_cases, write_error, write_test_run,
};
use cleanrules::json::{self as json, Json, JsonNumber, to_string};
use cleanrules::rules::RuleSet;
use cleanrules::table::DecisionTable;
use cleanrules::{ErrorCode, Map, Value};

use super::build::data;
use super::core::{DocumentCore, RulesCore};
use super::tables::TableCore;
use super::{Outcome, Problem, Raw, out};

fn internal(what: &str) -> Problem {
    Problem::Internal(format!("the case file encoding is wrong ({what})"))
}

fn number(n: usize) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

fn member<'a>(object: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    object.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn text_member(object: &[(String, Json)], key: &str) -> Outcome<Option<String>> {
    match member(object, key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(internal(key)),
    }
}

fn ids(value: Option<&Json>, key: &str) -> Outcome<Option<Vec<String>>> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| match item {
                Json::String(id) => Ok(id.clone()),
                _ => Err(internal(key)),
            })
            .collect::<Outcome<Vec<String>>>()
            .map(Some),
        Some(_) => Err(internal(key)),
    }
}

/// A value given in the reader's encoding, embedded in the case file's.
fn value(form: &Json) -> Outcome<Value> {
    data(&to_string(form))
}

fn map(form: &Json, key: &str) -> Outcome<Map> {
    match value(form)? {
        Value::Map(map) => Ok(map),
        _ => Err(internal(key)),
    }
}

/// An expectation from its parts, through the core's constructors.
fn expectation(object: &[(String, Json)]) -> Outcome<Expectation> {
    let mut expect = Expectation::new();
    if let Some(selected) = ids(member(object, "selected"), "selected")? {
        expect = expect.with_selected(selected)?;
    }
    if let Some(output) = member(object, "output") {
        let expected = match output {
            Json::Null => ExpectedOutput::NothingSelected,
            form => ExpectedOutput::Includes(map(form, "output")?),
        };
        expect = expect.with_output(expected)?;
    }
    match member(object, "outputs") {
        None | Some(Json::Null) => {}
        Some(Json::Array(forms)) => {
            let maps = forms
                .iter()
                .map(|form| map(form, "outputs"))
                .collect::<Outcome<Vec<Map>>>()?;
            expect = expect.with_outputs(maps)?;
        }
        Some(_) => return Err(internal("outputs")),
    }
    if let Some(unknown) = ids(member(object, "unknown"), "unknown")? {
        expect = expect.with_unknown(unknown)?;
    }
    if let Some(name) = text_member(object, "error")? {
        let code = ErrorCode::ALL
            .into_iter()
            .find(|code| code.as_str() == name)
            .ok_or_else(|| {
                Problem::Range(format!(
                    "unknown error code {name:?}; a case expects 'missing_field', 'type_mismatch' or 'hit_policy_error'"
                ))
            })?;
        expect = expect.with_error(code)?;
    }
    Ok(expect)
}

/// A case file's content from the JavaScript half's encoding: the shape of a
/// case file's content, with each value (an input, an expected output, the
/// metadata) in the reader's encoding, built with the core's constructors,
/// which refuse what the reader refuses.
pub(crate) fn cases_new(text: &str) -> Outcome<DocumentCore> {
    let mut limits = json::Limits::default();
    limits.max_size = text.len();
    limits.max_nesting = usize::MAX;
    let parsed = json::parse(text, &limits).map_err(|_| internal("not JSON"))?;
    let Json::Object(content) = &parsed else {
        return Err(internal("not an object"));
    };
    let Some(Json::Array(items)) = member(content, "items") else {
        return Err(internal("items"));
    };
    let mut cases = Vec::with_capacity(items.len());
    for item in items {
        let Json::Object(item) = item else {
            return Err(internal("a case"));
        };
        let id = text_member(item, "id")?.ok_or_else(|| internal("id"))?;
        let input = value(member(item, "input").ok_or_else(|| internal("input"))?)?;
        let mut case = Case::new(id, input)?;
        if let Some(description) = text_member(item, "description")? {
            case = case.with_description(description);
        }
        match member(item, "expect") {
            None | Some(Json::Null) => {}
            Some(Json::Object(expect)) => case = case.with_expect(expectation(expect)?),
            Some(_) => return Err(internal("expect")),
        }
        cases.push(case);
    }
    let mut cases = Cases::new(cases)?;
    if let Some(description) = text_member(content, "description")? {
        cases = cases.with_description(description);
    }
    match member(content, "metadata") {
        None | Some(Json::Null) => {}
        Some(form) => cases = cases.with_metadata(map(form, "metadata")?)?,
    }
    Ok(DocumentCore {
        document: cases.into_document(),
    })
}

/// The cases of a case file document; the JavaScript half asks only for
/// those, so any other document is a bug.
fn held(document: &DocumentCore) -> Outcome<&Cases> {
    match &document.document {
        Document::Cases(cases) => Ok(cases),
        _ => Err(Problem::Internal(
            "the document does not hold test cases".to_owned(),
        )),
    }
}

fn expectation_data(expect: &Expectation) -> Json {
    let mut members = Vec::new();
    if let Some(selected) = expect.selected() {
        members.push((
            "selected".to_owned(),
            Json::Array(selected.iter().map(|id| string(id)).collect()),
        ));
    }
    match expect.output() {
        None => {}
        Some(ExpectedOutput::Includes(map)) => members.push(("output".into(), out::map(map))),
        Some(_) => members.push(("output".into(), Json::Null)),
    }
    if let Some(outputs) = expect.outputs() {
        members.push((
            "outputs".into(),
            Json::Array(outputs.iter().map(out::map).collect()),
        ));
    }
    if let Some(unknown) = expect.unknown() {
        members.push((
            "unknown".into(),
            Json::Array(unknown.iter().map(|id| string(id)).collect()),
        ));
    }
    if let Some(code) = expect.error() {
        members.push(("error".into(), string(code.as_str())));
    }
    Json::Object(members)
}

/// What the idiomatic `Cases` reads of a case file document, as JSON:
/// `[description, metadata, items]`, each item `[id, description, input,
/// expect, json]`, values in the encoding of `out`, `expect` an object with
/// the parts given (`output` `null` for "nothing selected"), and `json` the
/// case as the case file document writes it (made stringifiable), for
/// `toJSON()`.
pub(crate) fn cases_data(document: &DocumentCore) -> Outcome<String> {
    let cases = held(document)?;
    let mut written = write_cases(cases);
    out::stringifiable(&mut written);
    let forms: Vec<Json> =
        match &mut written {
            Json::Object(members) => members.iter_mut().find(|(key, _)| key == "items").and_then(
                |(_, items)| match items {
                    Json::Array(list) => Some(std::mem::take(list)),
                    _ => None,
                },
            ),
            _ => None,
        }
        .ok_or_else(|| Problem::Internal("a case file writes its items".to_owned()))?;
    let items = cases
        .iter()
        .zip(forms)
        .map(|(case, form)| {
            Json::Array(vec![
                string(case.id()),
                case.description().map_or(Json::Null, string),
                out::value(case.input()),
                expectation_data(case.expect()),
                form,
            ])
        })
        .collect();
    Ok(to_string(&Json::Array(vec![
        cases.description().map_or(Json::Null, string),
        out::map(cases.metadata()),
        Json::Array(items),
    ])))
}

/// One case file given to a run.
pub(crate) struct CaseFileCore {
    pub(crate) file: CaseFile,
}

impl CaseFileCore {
    /// The case file whose bytes are `bytes`, read under the default limits,
    /// as YAML or JSON by its path's extension unless `format` says. The
    /// error of reading it is kept.
    pub(crate) fn read(path: Option<&str>, bytes: &[u8], format: Option<&str>) -> Outcome<Self> {
        let limits = json::Limits::default();
        let file = match (path, format) {
            (Some(path), None) => CaseFile::read(path, bytes, &limits),
            (path, format) => {
                let format = match format {
                    None | Some("json") => DocumentFormat::Json,
                    Some("yaml") => DocumentFormat::Yaml,
                    Some(other) => {
                        return Err(Problem::Range(format!(
                            "a case file is JSON or YAML, not {other:?}"
                        )));
                    }
                };
                CaseFile::read_as(path, bytes, format, &limits)
            }
        };
        Ok(Self { file })
    }

    /// The case file of a case file document already read, with a path or
    /// without one.
    pub(crate) fn of(document: &DocumentCore, path: Option<String>) -> Outcome<Self> {
        let mut file = CaseFile::new(held(document)?.clone());
        if let Some(path) = path {
            file = file.with_path(path);
        }
        Ok(Self { file })
    }

    /// `[path, digest, error]` as JSON: `null` for what it does not have,
    /// the error in its JSON form.
    pub(crate) fn data(&self) -> String {
        to_string(&Json::Array(vec![
            self.file.path().map_or(Json::Null, string),
            self.file
                .digest()
                .map_or(Json::Null, |digest| string(&digest.to_string())),
            self.file
                .cases()
                .err()
                .map_or(Json::Null, |error| string(&to_string(&write_error(error)))),
        ]))
    }

    /// The case file document of its cases, or `None` for a file that
    /// cannot be read.
    pub(crate) fn cases(&self) -> Option<DocumentCore> {
        self.file.cases().ok().map(|cases| DocumentCore {
            document: cases.clone().into_document(),
        })
    }
}

/// The rules a run tests, owned by it.
enum Tested {
    RuleSet(Arc<RuleSet>),
    Table(Box<DecisionTable>),
    Bundle(Box<Bundle>),
}

/// The limits of a run, as the JavaScript half checked them; each one left
/// out at its default.
pub(crate) fn run_limits(
    max_results: Option<f64>,
    max_steps: Option<f64>,
    max_explained: Option<f64>,
) -> TestRunLimits {
    // Whole numbers from 0, checked by the JavaScript half; a limit beyond
    // the platform's is no limit, as `usize` saturates.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = |n: f64| n as usize;
    let mut limits = TestRunLimits::default();
    if let Some(max) = max_results {
        limits = limits.with_max_results(whole(max));
    }
    if let Some(max) = max_steps {
        limits = limits.with_max_steps(whole(max));
    }
    if let Some(max) = max_explained {
        limits = limits.with_max_explained(whole(max));
    }
    limits
}

/// A run in progress, over the rules and case files it owns: the core's
/// cursor ([`TestRunState`]) and the results it gave, which its test run
/// holds.
pub(crate) struct RunCore {
    tested: Tested,
    files: Vec<CaseFile>,
    limits: TestRunLimits,
    ruleset_digest: bool,
    /// `None` once its test run is made.
    state: Option<TestRunState>,
    results: Vec<CaseResult>,
    test_run: Option<String>,
    /// The summary of its test run, once it is made.
    summary: Option<String>,
}

impl RunCore {
    fn new(tested: Tested, limits: TestRunLimits, ruleset_digest: bool) -> Self {
        Self {
            tested,
            files: Vec::new(),
            limits,
            ruleset_digest,
            state: Some(TestRunState::new()),
            results: Vec::new(),
            test_run: None,
            summary: None,
        }
    }

    /// A run against a rule set.
    pub(crate) fn of_ruleset(rules: &RulesCore, limits: TestRunLimits, digest: bool) -> Self {
        Self::new(Tested::RuleSet(Arc::clone(&rules.ruleset)), limits, digest)
    }

    /// A run against a decision table.
    pub(crate) fn of_table(table: &TableCore, limits: TestRunLimits, digest: bool) -> Self {
        Self::new(Tested::Table(Box::new(table.table.clone())), limits, digest)
    }

    /// A run of a bundle's case files against its rules.
    pub(crate) fn of_bundle(
        bundle: &DocumentCore,
        limits: TestRunLimits,
        digest: bool,
    ) -> Outcome<Self> {
        let bundle = bundle.held()?.clone();
        Ok(Self::new(Tested::Bundle(Box::new(bundle)), limits, digest))
    }

    fn runner(&self) -> Runner<'_> {
        match &self.tested {
            Tested::RuleSet(ruleset) => Runner::new(ruleset),
            Tested::Table(table) => Runner::for_table(table),
            Tested::Bundle(bundle) => Runner::for_bundle(bundle),
        }
        .with_limits(self.limits)
    }

    /// Give the run one more case file, before its first result.
    pub(crate) fn add(&mut self, file: &CaseFileCore) -> Outcome<()> {
        if self
            .state
            .as_ref()
            .is_none_or(|state| state.summary().total() > 0)
        {
            return Err(Problem::Internal(
                "case files are given before the first result".to_owned(),
            ));
        }
        self.files.push(file.file.clone());
        Ok(())
    }

    /// Give the run the case files of the bundle it tests, in its order,
    /// each loaded by `load(path, bound)` (its bytes) up to the bound the core
    /// picks (`CaseFile::bundle_files_bounded`), which also checks each
    /// file's digest; the first error of `load` is returned.
    pub(crate) fn add_bundle_files<E>(
        &mut self,
        mut load: impl FnMut(&str, usize) -> Result<Vec<u8>, E>,
    ) -> Result<Outcome<()>, E> {
        let Tested::Bundle(bundle) = &self.tested else {
            return Ok(Err(Problem::Internal(
                "only a bundle's run takes its case files".to_owned(),
            )));
        };
        let files = CaseFile::bundle_files_bounded(bundle, |case_ref, bound| {
            load(case_ref.path(), bound).map(CaseFileBytes::Whole)
        })?;
        self.files.extend(files);
        Ok(Ok(()))
    }

    /// Run the rest of the cases without giving the results one at a time;
    /// `cancel` is asked as `next` asks it.
    pub(crate) fn finish(&mut self, mut cancel: impl FnMut() -> bool) {
        while self.next_result(&mut cancel).is_some() {}
    }

    fn next_result(&mut self, cancel: impl FnOnce() -> bool) -> Option<&CaseResult> {
        let mut state = self.state.take()?;
        let result = self.runner().next_result(&self.files, &mut state, cancel);
        self.state = Some(state);
        self.results.push(result?);
        self.results.last()
    }

    /// The next result, as `result_data` writes it, or `None` once the run
    /// is finished; `cancel` is asked before the item, when the core asks.
    /// The JavaScript half never touches the run while `cancel` is asked
    /// (`lib/api/cases.js`, `CaseRun`), so no second borrow of it is made.
    pub(crate) fn next(&mut self, cancel: impl FnOnce() -> bool) -> Option<String> {
        self.next_result(cancel).map(result_data)
    }

    /// Stop the run: it gives no more results, and is partial (cancelled)
    /// when a result remained.
    pub(crate) fn stop(&mut self) {
        if let Some(mut state) = self.state.take() {
            self.runner().stop(&self.files, &mut state);
            self.state = Some(state);
        }
    }

    /// The summary of the results given so far, as `summary_data` writes it.
    pub(crate) fn summary(&self) -> String {
        match (&self.state, &self.summary) {
            (Some(state), _) => to_string(&summary_data(state.summary())),
            (None, Some(summary)) => summary.clone(),
            (None, None) => to_string(&summary_data(&Summary::default())),
        }
    }

    /// The results not given yet.
    pub(crate) fn remaining(&self) -> usize {
        self.state
            .as_ref()
            .map_or(0, |state| state.remaining(&self.files))
    }

    /// Whether the run gives no more results.
    pub(crate) fn is_finished(&self) -> bool {
        self.state.as_ref().is_none_or(TestRunState::is_finished)
    }

    /// The test run, made once, as JSON: `[document, text, results,
    /// summary, allPassed, isCurrent]`, the test run document made
    /// stringifiable, its text, each result as `result_data` writes it, the
    /// summary, whether every case ran and passed, and whether this engine
    /// ran it: the core's answers. A run that is not
    /// finished is stopped first, and is then partial (cancelled).
    pub(crate) fn test_run(&mut self) -> String {
        if let Some(data) = &self.test_run {
            return data.clone();
        }
        let state = self.state.take().unwrap_or_default();
        let results = std::mem::take(&mut self.results);
        let run = self
            .runner()
            .test_run(&self.files, state, results)
            .with_ruleset_digest(self.ruleset_digest);
        let mut document = Json::Object(vec![
            (
                "version".to_owned(),
                Json::Number(JsonNumber::from_literal(
                    cleanrules::FORMAT_VERSION.to_string(),
                )),
            ),
            ("test_run".to_owned(), write_test_run(&run)),
        ]);
        out::stringifiable(&mut document);
        let results = run
            .results
            .iter()
            .map(|result| Json::String(result_data(result)))
            .collect();
        let summary = summary_data(&run.summary);
        self.summary = Some(to_string(&summary));
        let data = to_string(&Json::Array(vec![
            document,
            Json::String(run.to_string()),
            Json::Array(results),
            summary,
            Json::Bool(run.all_passed()),
            Json::Bool(run.engine() == (ENGINE_NAME, ENGINE_VERSION)),
        ]));
        self.test_run = Some(data.clone());
        data
    }
}

/// A summary as `[passed, failed, error, interrupted, total, partial]`,
/// `interrupted` `[reason, remaining]` or `null`: the core's counts.
fn summary_data(summary: &Summary) -> Json {
    Json::Array(vec![
        number(summary.passed),
        number(summary.failed),
        number(summary.error),
        summary.interrupted.map_or(Json::Null, |interrupted| {
            Json::Array(vec![
                string(interrupted.reason.as_str()),
                number(interrupted.remaining),
            ])
        }),
        number(summary.total()),
        Json::Bool(summary.is_partial()),
    ])
}

/// One mismatch as `[key, expected, actual, given, known]`: values in the
/// encoding of `out` (a list of ids, a map form or `null`, a list of map
/// forms, or an error code and an error's JSON form as text); `given` is
/// false when nothing was given; `known` is false for a key this binding does
/// not know, whose parts the caller reads from the result's JSON form.
fn mismatch_data(mismatch: &Mismatch) -> Json {
    let strings = |ids: &[String]| Json::Array(ids.iter().map(|id| string(id)).collect());
    let expected_output = |expected: &ExpectedOutput| match expected {
        ExpectedOutput::Includes(map) => out::map(map),
        _ => Json::Null,
    };
    let maps = |maps: &[Map]| Json::Array(maps.iter().map(out::map).collect());
    let (expected, actual, given, known) = match mismatch {
        Mismatch::Selected { expected, actual } | Mismatch::Unknown { expected, actual } => {
            (strings(expected), strings(actual), true, true)
        }
        Mismatch::Output { expected, actual } => (
            expected_output(expected),
            actual.as_ref().map_or(Json::Null, out::map),
            true,
            true,
        ),
        Mismatch::NoSingleOutput { expected, .. } => {
            (expected_output(expected), Json::Null, false, true)
        }
        Mismatch::Outputs { expected, actual } => (maps(expected), maps(actual), true, true),
        Mismatch::Error { expected, actual } => (
            string(expected.as_str()),
            actual
                .as_ref()
                .map_or(Json::Null, |error| string(&to_string(&write_error(error)))),
            true,
            true,
        ),
        _ => (Json::Null, Json::Null, false, false),
    };
    Json::Array(vec![
        string(mismatch.key().as_str()),
        expected,
        actual,
        Json::Bool(given),
        Json::Bool(known),
    ])
}

/// One result, as JSON: `[json, mismatches, error]`, its JSON form as a test
/// run writes it (made stringifiable), each mismatch as `mismatch_data`
/// writes it, and its error's JSON form as text, or `null`.
pub(crate) fn result_data(result: &CaseResult) -> String {
    let mut form = write_case_result(result);
    out::stringifiable(&mut form);
    to_string(&Json::Array(vec![
        form,
        Json::Array(result.mismatches.iter().map(mismatch_data).collect()),
        result
            .error
            .as_ref()
            .map_or(Json::Null, |error| string(&to_string(&write_error(error)))),
    ]))
}

// --- `Document.test` of the native layer -------------------------------------

/// A case file of `Document.test` as JavaScript gives it: `{ path, content }`
/// (bytes, whose digest the run records) or `{ path, cases }` (a case file
/// document already read, which the front end resolves).
#[derive(Debug)]
pub(crate) enum RawCaseFile {
    /// An object: its members.
    Given {
        path: Raw,
        content: Raw,
        /// `cases`, when it is given: the cases of the document, or why it
        /// gives none.
        cases: Option<Outcome<Cases>>,
    },
    /// Anything else, by what it is.
    Wrong(&'static str),
}

/// The options of `Document.test` as JavaScript gives them, each member read
/// once: `{ format, limits: { maxResults, maxSteps, maxExplained },
/// rulesetDigest }`; the front end reads `cancel`.
#[derive(Debug, Default)]
pub(crate) enum RawTestOptions {
    /// `undefined` or `null`: the defaults.
    #[default]
    Absent,
    /// An object: its members.
    Given {
        format: Raw,
        limits: RawTestLimits,
        ruleset_digest: Raw,
    },
    /// Anything else, by what it is.
    Wrong(&'static str),
}

/// The `limits` of a test run's options, as JavaScript gives them.
#[derive(Debug, Default)]
pub(crate) enum RawTestLimits {
    /// `undefined` or `null`: the defaults.
    #[default]
    Absent,
    /// An object: its members, read once; other members are ignored.
    Given {
        max_results: Raw,
        max_steps: Raw,
        max_explained: Raw,
    },
    /// Anything else, by what it is.
    Wrong(&'static str),
}

/// What a document holds, in words: `"a rule set"`, `"a case file"`...
fn holds(document: &DocumentCore) -> &'static str {
    match document.kind() {
        "condition" => "a condition",
        "rule" => "a rule",
        "ruleset" => "a rule set",
        "decision_table" => "a decision table",
        "bundle" => "a bundle",
        "cases" => "a case file",
        _ => "another document",
    }
}

/// The cases of the case file document given as `files[i].cases` (the front
/// end's reading of it): a `TypeError` for a document of another kind.
pub(crate) fn cases_of(document: &DocumentCore, i: usize) -> Outcome<Cases> {
    match &document.document {
        Document::Cases(cases) => Ok(cases.clone()),
        _ => Err(Problem::Type(format!(
            "files[{i}].cases is a Document that holds a case file, not {}",
            holds(document)
        ))),
    }
}

/// The checked options: format, limits and whether to carry the digest.
fn test_options(raw: &RawTestOptions) -> Outcome<(DocumentFormat, TestRunLimits, bool)> {
    let mut limits = TestRunLimits::default();
    match raw {
        RawTestOptions::Absent => Ok((DocumentFormat::Text, limits, false)),
        RawTestOptions::Wrong(kind) => Err(Problem::Type(format!(
            "options must be an object, such as {{ limits: {{ maxResults: 100 }} }}, not {kind}"
        ))),
        RawTestOptions::Given {
            format: given,
            limits: raw_limits,
            ruleset_digest,
        } => {
            let format = super::format(given, DocumentFormat::Text)?;
            match raw_limits {
                RawTestLimits::Absent => {}
                RawTestLimits::Wrong(kind) => {
                    return Err(Problem::Type(format!(
                        "limits must be an object, such as {{ maxResults: 100 }}, not {kind}"
                    )));
                }
                RawTestLimits::Given {
                    max_results,
                    max_steps,
                    max_explained,
                } => {
                    if let Some(max) = super::whole("limits.maxResults", max_results)? {
                        limits = limits.with_max_results(max);
                    }
                    if let Some(max) = super::whole("limits.maxSteps", max_steps)? {
                        limits = limits.with_max_steps(max);
                    }
                    if let Some(max) = super::whole("limits.maxExplained", max_explained)? {
                        limits = limits.with_max_explained(max);
                    }
                }
            }
            let digest = super::flag(ruleset_digest, "options.rulesetDigest")?;
            Ok((format, limits, digest))
        }
    }
}

/// A file's path: a string, or left out.
fn path_of(raw: &Raw, i: usize) -> Outcome<Option<String>> {
    match raw {
        Raw::Undefined | Raw::Null => Ok(None),
        Raw::String(path) => Ok(Some(path.clone())),
        other => Err(Problem::Type(format!(
            "files[{i}].path is a string, not {}",
            other.describe()
        ))),
    }
}

/// `Document.test` of the native layer: run the case files `files` against
/// the rules the document holds (a rule set, a decision table, or a bundle,
/// whose files are found by path), and write the test run in `format`: its
/// text (the default), or a test run document in JSON or YAML. `cancel` is
/// asked before each item, when the core asks.
pub(crate) fn document_test(
    document: &DocumentCore,
    files: Vec<RawCaseFile>,
    options: &RawTestOptions,
    cancel: impl FnMut() -> bool,
) -> Outcome<String> {
    let (format, limits, digest) = test_options(options)?;
    let json_limits = json::Limits::default();
    let mut given = Vec::with_capacity(files.len());
    for (i, file) in files.into_iter().enumerate() {
        let (path, content, cases) = match file {
            RawCaseFile::Given {
                path,
                content,
                cases,
            } => (path, content, cases),
            RawCaseFile::Wrong(kind) => {
                return Err(Problem::Type(format!(
                    "files[{i}] is an object such as {{ path: 'cases/a.json', content }}, not {kind}"
                )));
            }
        };
        let path = path_of(&path, i)?;
        let file = match (cases, content) {
            (Some(cases), Raw::Undefined) => (path, Err(cases?)),
            (Some(_), _) => {
                return Err(Problem::Type(format!(
                    "files[{i}] gives content or cases, not both"
                )));
            }
            (None, content) => (
                path,
                Ok(super::text(content, &format!("files[{i}].content"))?),
            ),
        };
        given.push(file);
    }
    let runner = match &document.document {
        Document::RuleSet(ruleset) => Runner::new(ruleset),
        Document::DecisionTable(table) => Runner::for_table(table),
        Document::Bundle(bundle) => Runner::for_bundle(bundle),
        _ => {
            return Err(Problem::Type(format!(
                "test runs the cases of a document that holds a rule set, a decision table or a bundle, not {}",
                holds(document)
            )));
        }
    }
    .with_limits(limits);
    let case_files: Vec<CaseFile> = if let Document::Bundle(bundle) = &document.document {
        // The core picks the bound and checks each file's digest.
        CaseFile::bundle_files_bounded(bundle, |reference, _| {
            given
                .iter()
                .find(|(path, _)| path.as_deref() == Some(reference.path()))
                .ok_or_else(|| {
                    Problem::Range(format!(
                        "the bundle's case file {} is not among the files given",
                        reference.path()
                    ))
                })
                .and_then(|(_, content)| match content {
                    Ok(bytes) => Ok(CaseFileBytes::Whole(bytes.clone())),
                    Err(_) => Err(Problem::Type(format!(
                        "the bundle's case file {} is given as content (its bytes, checked against its digest), not as cases",
                        reference.path()
                    ))),
                })
        })?
    } else {
        given
            .into_iter()
            .map(|(path, content)| match content {
                Ok(bytes) => match &path {
                    Some(path) => CaseFile::read(path, &bytes, &json_limits),
                    None => CaseFile::read_as(None, &bytes, DocumentFormat::Json, &json_limits),
                },
                Err(cases) => {
                    let file = CaseFile::new(cases);
                    match path {
                        Some(path) => file.with_path(path),
                        None => file,
                    }
                }
            })
            .collect()
    };
    let run = runner
        .run_until(&case_files, cancel)
        .with_ruleset_digest(digest);
    Ok(cleanrules::format::write_test_run_as(&run, format)?)
}
