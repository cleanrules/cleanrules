//! Test cases and the case runner (`spec/tests.md`): the core reads, checks,
//! writes and runs them; Python builds its objects from what this module
//! gives (`python/cleanrules/_cases.py`).
//!
//! Three names, the same in every language: a `CaseFileRef` is a bundle's
//! reference to a case file (`bundles.rs`); a [`PyCases`] is the content of a
//! case file document; a [`PyCaseFile`] is one file given to a run: its path,
//! when it has one, and its cases or the error that keeps it from being read.
//! A [`PyCaseRun`] owns its rules, its case files and the core's cursor
//! ([`TestRunState`]), and gives one result at a time.

use std::sync::Arc;

use cleanrules::bundle::{Bundle, ENGINE_NAME, ENGINE_VERSION};
use cleanrules::cases::{
    Case, CaseFile, CaseFileBytes, CaseResult, Cases, Expectation, ExpectedOutput, Mismatch,
    Runner, Summary, TestRun, TestRunLimits, TestRunState,
};
use cleanrules::format::{Document, DocumentFormat, write_case_result, write_test_run};
use cleanrules::json::{Limits, to_string};
use cleanrules::rules::RuleSet;
use cleanrules::table::DecisionTable;
use cleanrules::{Error, ErrorCode, Map, Value};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple};

use crate::bundles::PyBundle;
use crate::convert;
use crate::documents::document_format;
use crate::errors::{self, IntoPyResult};
use crate::read::Stop;
use crate::rules::PyRuleSet;
use crate::tables::PyDecisionTable;

/// A `format_error` for a Python value that a case file cannot hold.
fn not_data(py: Python<'_>, problem: String, hint: &str) -> PyErr {
    errors::to_py(
        py,
        &Error::new(ErrorCode::FormatError, problem).with_hint(hint),
    )
}

/// `value`, at `at`, as data of a case file; `prefix` names where, in the
/// message of a value it cannot hold.
fn data(value: &Bound<'_, PyAny>, at: &str, prefix: &str, hint: &str) -> PyResult<Value> {
    let py = value.py();
    match convert::case_data(value, at) {
        Ok(Ok(converted)) => Ok(converted),
        Ok(Err(problem)) => Err(not_data(py, format!("{prefix}{problem}"), hint)),
        Err(Stop::Python(error)) => Err(error),
        Err(Stop::Limit(error)) => Err(errors::to_py(py, &error)),
    }
}

/// An expected output, or a case file's metadata: a dict of data.
fn data_map(value: &Bound<'_, PyAny>, at: &str, prefix: &str, hint: &str) -> PyResult<Map> {
    match data(value, at, prefix, hint)? {
        Value::Map(map) => Ok(map),
        _ => Err(PyValueError::new_err(format!("{at} must be a dict"))),
    }
}

const EXPECTED_HINT: &str = "expect the value the rules give, such as a Decimal, or None";

/// A map as a Python dict.
fn map_to_py<'py>(py: Python<'py>, map: &Map) -> PyResult<Bound<'py, PyAny>> {
    convert::to_py(py, &Value::Map(map.clone()))
}

/// The content of a case file document (`spec/tests.md`, *Case files*),
/// immutable.
#[pyclass(frozen, name = "Cases", module = "cleanrules._cleanrules")]
pub(crate) struct PyCases {
    pub(crate) cases: Cases,
}

#[pymethods]
impl PyCases {
    /// The file's description, if it has one.
    #[getter]
    fn description(&self) -> Option<String> {
        self.cases.description().map(str::to_owned)
    }

    /// The file's metadata, as a new dict.
    fn metadata<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        map_to_py(py, self.cases.metadata())
    }

    /// The cases, in order.
    fn items(&self) -> Vec<PyCase> {
        self.cases
            .iter()
            .map(|case| PyCase { case: case.clone() })
            .collect()
    }
}

/// One test case, immutable.
#[pyclass(frozen, name = "Case", module = "cleanrules._cleanrules")]
pub(crate) struct PyCase {
    case: Case,
}

#[pymethods]
impl PyCase {
    /// The case's id.
    #[getter]
    fn id(&self) -> &str {
        self.case.id()
    }

    /// The case's description, if it has one.
    #[getter]
    fn description(&self) -> Option<&str> {
        self.case.description()
    }

    /// The input, as new Python data.
    fn input<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        convert::to_py(py, self.case.input())
    }

    /// The expectation.
    fn expect(&self) -> PyExpectation {
        PyExpectation {
            expect: self.case.expect().clone(),
        }
    }

    /// The canonical bytes of a case file that holds this case alone: what
    /// tells two cases apart.
    fn canonical<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let cases = Cases::new([self.case.clone()]).or_raise(py)?;
        Ok(PyBytes::new(py, &cases.into_document().canonical_bytes()))
    }
}

/// What evaluating a case's input must give, immutable.
#[pyclass(frozen, name = "Expectation", module = "cleanrules._cleanrules")]
pub(crate) struct PyExpectation {
    expect: Expectation,
}

#[pymethods]
impl PyExpectation {
    /// The rules expected to be selected, or `None`.
    #[getter]
    fn selected(&self) -> Option<Vec<String>> {
        self.expect.selected().map(<[String]>::to_vec)
    }

    /// The expected single output: `(given, value)`, `value` `None` for
    /// "nothing selected" and a dict otherwise.
    fn output<'py>(&self, py: Python<'py>) -> PyResult<(bool, Bound<'py, PyAny>)> {
        Ok(match self.expect.output() {
            None => (false, py.None().into_bound(py)),
            Some(ExpectedOutput::Includes(map)) => (true, map_to_py(py, map)?),
            Some(_) => (true, py.None().into_bound(py)),
        })
    }

    /// The expected outputs, or `None`.
    fn outputs<'py>(&self, py: Python<'py>) -> PyResult<Option<Vec<Bound<'py, PyAny>>>> {
        self.expect
            .outputs()
            .map(|outputs| outputs.iter().map(|map| map_to_py(py, map)).collect())
            .transpose()
    }

    /// The rules expected to be unknown, sorted by code point, or `None`.
    #[getter]
    fn unknown(&self) -> Option<Vec<String>> {
        self.expect
            .unknown()
            .map(|ids| ids.iter().cloned().collect())
    }

    /// The code of the error expected, or `None`.
    #[getter]
    fn error(&self) -> Option<&'static str> {
        self.expect.error().map(ErrorCode::as_str)
    }

    /// The canonical bytes of a case file that holds one case with this
    /// expectation: what tells two expectations apart.
    fn canonical<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let case = Case::new("_", Value::Map(Map::new()))
            .or_raise(py)?
            .with_expect(self.expect.clone());
        let cases = Cases::new([case]).or_raise(py)?;
        Ok(PyBytes::new(py, &cases.into_document().canonical_bytes()))
    }
}

/// An expectation, through the core's constructors, which refuse what the
/// reader refuses.
#[pyfunction]
#[pyo3(signature = (selected, output, output_given, outputs, unknown, error))]
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
pub(crate) fn expectation_new(
    py: Python<'_>,
    selected: Option<Vec<String>>,
    output: Option<Bound<'_, PyAny>>,
    output_given: bool,
    outputs: Option<Vec<Bound<'_, PyAny>>>,
    unknown: Option<Vec<String>>,
    error: Option<&str>,
) -> PyResult<PyExpectation> {
    let mut expect = Expectation::new();
    if let Some(selected) = selected {
        expect = expect.with_selected(selected).or_raise(py)?;
    }
    if output_given {
        let expected = match output {
            None => ExpectedOutput::NothingSelected,
            Some(map) => {
                ExpectedOutput::Includes(data_map(&map, "output", "the expected ", EXPECTED_HINT)?)
            }
        };
        expect = expect.with_output(expected).or_raise(py)?;
    }
    if let Some(outputs) = outputs {
        let maps = outputs
            .iter()
            .enumerate()
            .map(|(i, map)| {
                data_map(
                    map,
                    &format!("outputs[{i}]"),
                    "the expected ",
                    EXPECTED_HINT,
                )
            })
            .collect::<PyResult<Vec<Map>>>()?;
        expect = expect.with_outputs(maps).or_raise(py)?;
    }
    if let Some(unknown) = unknown {
        expect = expect.with_unknown(unknown).or_raise(py)?;
    }
    if let Some(name) = error {
        let code = ErrorCode::ALL
            .into_iter()
            .find(|code| code.as_str() == name)
            .ok_or_else(|| {
                PyValueError::new_err(format!(
                    "unknown error code {name:?}; a case expects \"missing_field\", \"type_mismatch\" or \"hit_policy_error\""
                ))
            })?;
        expect = expect.with_error(code).or_raise(py)?;
    }
    Ok(PyExpectation { expect })
}

/// A case, through the core's constructor.
#[pyfunction]
#[pyo3(signature = (id, input, description=None, expect=None))]
pub(crate) fn case_new(
    py: Python<'_>,
    id: String,
    input: &Bound<'_, PyAny>,
    description: Option<String>,
    expect: Option<&PyExpectation>,
) -> PyResult<PyCase> {
    let input = data(
        input,
        "input",
        &format!("in the case {id:?}, "),
        "an input holds data: None, bool, numbers, str, dates, lists and dicts",
    )?;
    let mut case = Case::new(id, input).or_raise(py)?;
    if let Some(description) = description {
        case = case.with_description(description);
    }
    if let Some(expect) = expect {
        case = case.with_expect(expect.expect.clone());
    }
    Ok(PyCase { case })
}

/// A case file's content, through the core's constructors.
#[pyfunction]
#[pyo3(signature = (items, description=None, metadata=None))]
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
pub(crate) fn cases_new(
    py: Python<'_>,
    items: Vec<PyRef<'_, PyCase>>,
    description: Option<String>,
    metadata: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyCases> {
    let mut cases = Cases::new(items.iter().map(|case| case.case.clone())).or_raise(py)?;
    if let Some(description) = description {
        cases = cases.with_description(description);
    }
    if let Some(metadata) = metadata {
        let map = data_map(
            metadata.as_any(),
            "metadata",
            "the case file's ",
            "metadata holds data: None, bool, numbers, str, dates, lists and dicts",
        )?;
        cases = cases.with_metadata(map).or_raise(py)?;
    }
    Ok(PyCases { cases })
}

/// One case file given to a run, immutable: its path, when it has one, the
/// digest of the bytes it was read from, and its cases or the error of
/// reading it.
#[pyclass(frozen, name = "CaseFile", module = "cleanrules._cleanrules")]
pub(crate) struct PyCaseFile {
    file: CaseFile,
}

#[pymethods]
impl PyCaseFile {
    /// The path, if it has one.
    #[getter]
    fn path(&self) -> Option<&str> {
        self.file.path()
    }

    /// The digest of the bytes it was read from, if it was.
    #[getter]
    fn digest(&self) -> Option<String> {
        self.file.digest().map(|digest| digest.to_string())
    }

    /// The cases, or `None` for a file that cannot be read.
    fn cases(&self) -> Option<PyCases> {
        self.file.cases().ok().map(|cases| PyCases {
            cases: cases.clone(),
        })
    }

    /// The error of reading the file, as a Python exception, or `None`.
    fn error(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.file
            .cases()
            .err()
            .map(|error| errors::to_py(py, error).into_value(py).into_any())
    }
}

/// A case file already read, with a path or without one.
#[pyfunction]
#[pyo3(signature = (cases, path=None))]
pub(crate) fn case_file_of(cases: &PyCases, path: Option<String>) -> PyCaseFile {
    let mut file = CaseFile::new(cases.cases.clone());
    if let Some(path) = path {
        file = file.with_path(path);
    }
    PyCaseFile { file }
}

/// The case file whose bytes are `content`, read under the default limits:
/// as YAML or JSON by its path's extension unless `format` says (`spec/tests.md`,
/// *Running cases*). The error of reading it is kept, not raised.
#[pyfunction]
#[pyo3(signature = (path, content, format=None))]
pub(crate) fn case_file_read(
    path: Option<&str>,
    content: &[u8],
    format: Option<&str>,
) -> PyResult<PyCaseFile> {
    let limits = Limits::default();
    let file = match (path, format) {
        (Some(path), None) => CaseFile::read(path, content, &limits),
        (path, format) => {
            let format = match format {
                Some(name) => document_format(name)?,
                None => DocumentFormat::Json,
            };
            CaseFile::read_as(path, content, format, &limits)
        }
    };
    Ok(PyCaseFile { file })
}

/// The case files of a bundle, in its order, each loaded by `load` up to the
/// bound the core gives (`CaseFile::bundle_files_bounded`): `load(path,
/// bound)` returns the file's bytes, or its size (an int) when it has more
/// bytes than `bound` and was not loaded. The core checks each file against
/// the digest the bundle records and reads it under the limits it records;
/// what `load` raises propagates.
#[pyfunction]
pub(crate) fn bundle_case_files(
    bundle: &PyBundle,
    load: &Bound<'_, PyAny>,
) -> PyResult<Vec<PyCaseFile>> {
    let files = CaseFile::bundle_files_bounded(&bundle.bundle, |case_ref, bound| {
        let loaded = load.call1((case_ref.path(), bound))?;
        if let Ok(bytes) = loaded.cast::<PyBytes>() {
            return Ok(CaseFileBytes::Whole(bytes.as_bytes().to_vec()));
        }
        match loaded.extract::<u64>() {
            Ok(size) => Ok(CaseFileBytes::TooLarge { size: Some(size) }),
            Err(_) => Err(pyo3::exceptions::PyTypeError::new_err(format!(
                "the content of the case file {} must be bytes, got {}",
                case_ref.path(),
                convert::type_name(&loaded)
            ))),
        }
    })?;
    Ok(files.into_iter().map(|file| PyCaseFile { file }).collect())
}

/// The core's reading of a document that holds a case file, for `loads`.
pub(crate) fn cases_object(py: Python<'_>, cases: Cases) -> PyResult<Bound<'_, PyAny>> {
    Ok(PyCases { cases }.into_pyobject(py)?.into_any())
}

/// The rules a run tests, owned by it.
enum Tested {
    RuleSet(Arc<RuleSet>),
    Table(Box<DecisionTable>),
    Bundle(Box<Bundle>),
}

impl Tested {
    fn runner(&self, limits: TestRunLimits) -> Runner<'_> {
        match self {
            Tested::RuleSet(ruleset) => Runner::new(ruleset),
            Tested::Table(table) => Runner::for_table(table),
            Tested::Bundle(bundle) => Runner::for_bundle(bundle),
        }
        .with_limits(limits)
    }
}

/// The limits of a run, each one left out at its default.
fn run_limits(
    max_results: Option<usize>,
    max_steps: Option<usize>,
    max_explained: Option<usize>,
) -> TestRunLimits {
    let mut limits = TestRunLimits::default();
    if let Some(max) = max_results {
        limits = limits.with_max_results(max);
    }
    if let Some(max) = max_steps {
        limits = limits.with_max_steps(max);
    }
    if let Some(max) = max_explained {
        limits = limits.with_max_explained(max);
    }
    limits
}

/// A run in progress, over the rules and case files it owns: the core's
/// cursor ([`TestRunState`]) and the results it gave, which its test run
/// holds.
#[pyclass(name = "CaseRun", module = "cleanrules._cleanrules")]
pub(crate) struct PyCaseRun {
    tested: Tested,
    files: Vec<CaseFile>,
    limits: TestRunLimits,
    ruleset_digest: bool,
    /// `None` once its test run is made.
    state: Option<TestRunState>,
    results: Vec<CaseResult>,
    test_run: Option<Py<PyTestRun>>,
}

/// Ask `cancel` (a Python callable), with the GIL, whether to cancel: a call
/// that raises cancels, and its exception is kept in `raised`.
fn ask(cancel: Option<&Py<PyAny>>, raised: &mut Option<PyErr>) -> bool {
    let Some(cancel) = cancel else {
        return false;
    };
    Python::attach(
        |py| match cancel.call0(py).and_then(|answer| answer.is_truthy(py)) {
            Ok(answer) => answer,
            Err(error) => {
                *raised = Some(error);
                true
            }
        },
    )
}

/// A Python view of one mismatch: `(key, expected, actual, has_actual,
/// reason, known)`; `known` is false for a key this binding does not know,
/// whose parts the caller reads from the result's JSON form.
fn mismatch_to_py<'py>(py: Python<'py>, mismatch: &Mismatch) -> PyResult<Bound<'py, PyTuple>> {
    let none = || py.None().into_bound(py);
    let strings =
        |ids: &[String]| -> PyResult<Bound<'py, PyAny>> { Ok(PyList::new(py, ids)?.into_any()) };
    let expected_output = |expected: &ExpectedOutput| -> PyResult<Bound<'py, PyAny>> {
        match expected {
            ExpectedOutput::Includes(map) => map_to_py(py, map),
            _ => Ok(none()),
        }
    };
    let maps = |maps: &[Map]| -> PyResult<Bound<'py, PyAny>> {
        let items = maps
            .iter()
            .map(|map| map_to_py(py, map))
            .collect::<PyResult<Vec<_>>>()?;
        Ok(PyList::new(py, items)?.into_any())
    };
    let key = mismatch.key().as_str();
    let reason = mismatch.reason();
    let (expected, actual, has_actual, known) = match mismatch {
        Mismatch::Selected { expected, actual } | Mismatch::Unknown { expected, actual } => {
            (strings(expected)?, strings(actual)?, true, true)
        }
        Mismatch::Output { expected, actual } => (
            expected_output(expected)?,
            match actual {
                Some(map) => map_to_py(py, map)?,
                None => none(),
            },
            true,
            true,
        ),
        Mismatch::NoSingleOutput { expected, .. } => {
            (expected_output(expected)?, none(), false, true)
        }
        Mismatch::Outputs { expected, actual } => (maps(expected)?, maps(actual)?, true, true),
        Mismatch::Error { expected, actual } => (
            expected.as_str().into_pyobject(py)?.into_any(),
            match actual {
                Some(error) => errors::to_py(py, error)
                    .into_value(py)
                    .into_bound(py)
                    .into_any(),
                None => none(),
            },
            true,
            true,
        ),
        _ => (none(), none(), false, false),
    };
    PyTuple::new(
        py,
        [
            key.into_pyobject(py)?.into_any(),
            expected,
            actual,
            pyo3::types::PyBool::new(py, has_actual)
                .to_owned()
                .into_any(),
            reason.into_pyobject(py)?.into_any(),
            pyo3::types::PyBool::new(py, known).to_owned().into_any(),
        ],
    )
}

/// A Python view of one result: `(path, id, status, mismatches, error,
/// explanation_text, json)`, `json` its JSON form as a test run writes it.
fn result_to_py<'py>(py: Python<'py>, result: &CaseResult) -> PyResult<Bound<'py, PyTuple>> {
    let mismatches = result
        .mismatches
        .iter()
        .map(|mismatch| mismatch_to_py(py, mismatch))
        .collect::<PyResult<Vec<_>>>()?;
    let error = match &result.error {
        Some(error) => errors::to_py(py, error)
            .into_value(py)
            .into_bound(py)
            .into_any(),
        None => py.None().into_bound(py),
    };
    PyTuple::new(
        py,
        [
            result.path.as_deref().into_pyobject(py)?.into_any(),
            result.id.as_deref().into_pyobject(py)?.into_any(),
            result.status.as_str().into_pyobject(py)?.into_any(),
            PyList::new(py, mismatches)?.into_any(),
            error,
            result
                .explanation_text
                .as_deref()
                .into_pyobject(py)?
                .into_any(),
            to_string(&write_case_result(result))
                .into_pyobject(py)?
                .into_any(),
        ],
    )
}

/// The summary as `(passed, failed, error, interrupted, total, partial)`,
/// `interrupted` `(reason, remaining)` or `None`: the core's counts.
fn summary_to_py<'py>(py: Python<'py>, summary: &Summary) -> PyResult<Bound<'py, PyTuple>> {
    let interrupted = summary
        .interrupted
        .map(|interrupted| (interrupted.reason.as_str(), interrupted.remaining));
    (
        summary.passed,
        summary.failed,
        summary.error,
        interrupted,
        summary.total(),
        summary.is_partial(),
    )
        .into_pyobject(py)
}

impl PyCaseRun {
    fn new(tested: Tested, files: Vec<CaseFile>, limits: TestRunLimits, digest: bool) -> Self {
        Self {
            tested,
            files,
            limits,
            ruleset_digest: digest,
            state: Some(TestRunState::new()),
            results: Vec::new(),
            test_run: None,
        }
    }

    /// The next result in the core's form, the GIL released while the case is
    /// evaluated; `cancel` is asked, with the GIL, when the core asks.
    fn step(&mut self, py: Python<'_>, cancel: Option<&Py<PyAny>>) -> PyResult<Option<usize>> {
        let Self {
            tested,
            files,
            limits,
            state,
            results,
            ..
        } = self;
        let Some(state) = state.as_mut() else {
            return Ok(None);
        };
        let runner = tested.runner(*limits);
        let mut raised = None;
        let result = py.detach(|| runner.next_result(files, state, || ask(cancel, &mut raised)));
        if let Some(error) = raised {
            return Err(error);
        }
        Ok(result.map(|result| {
            results.push(result);
            results.len() - 1
        }))
    }
}

#[pymethods]
impl PyCaseRun {
    /// The next result as `result_to_py` gives it, or `None` once the run is
    /// finished; `cancel`, a callable, is asked before each item.
    #[pyo3(signature = (cancel=None))]
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts arguments by value.
    fn next<'py>(
        &mut self,
        py: Python<'py>,
        cancel: Option<Py<PyAny>>,
    ) -> PyResult<Option<Bound<'py, PyTuple>>> {
        match self.step(py, cancel.as_ref())? {
            Some(index) => Ok(Some(result_to_py(py, &self.results[index])?)),
            None => Ok(None),
        }
    }

    /// Run the rest of the cases, without giving the results one at a time.
    #[pyo3(signature = (cancel=None))]
    #[allow(clippy::needless_pass_by_value)] // PyO3 extracts arguments by value.
    fn finish(&mut self, py: Python<'_>, cancel: Option<Py<PyAny>>) -> PyResult<()> {
        while self.step(py, cancel.as_ref())?.is_some() {
            // Ctrl-C between cases: the run stops (cancelled), so its test
            // run stays coherent, then the signal's exception propagates.
            if let Err(error) = py.check_signals() {
                self.stop();
                return Err(error);
            }
        }
        Ok(())
    }

    /// Stop the run: it gives no more results, and it is partial (cancelled)
    /// when a result remained.
    fn stop(&mut self) {
        if let Some(state) = self.state.as_mut() {
            self.tested.runner(self.limits).stop(&self.files, state);
        }
    }

    /// The summary of the results given so far.
    fn summary<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        match (&self.state, &self.test_run) {
            (Some(state), _) => summary_to_py(py, state.summary()),
            (None, Some(run)) => summary_to_py(py, &run.get().run.summary),
            (None, None) => Err(PyRuntimeError::new_err("the run has no state")),
        }
    }

    /// The results not given yet.
    #[getter]
    fn remaining(&self) -> usize {
        self.state
            .as_ref()
            .map_or(0, |state| state.remaining(&self.files))
    }

    /// Whether the run gives no more results.
    #[getter]
    fn is_finished(&self) -> bool {
        self.state.as_ref().is_none_or(TestRunState::is_finished)
    }

    /// The test run, made once: a run that is not finished is stopped first,
    /// and is then partial (cancelled).
    fn test_run(&mut self, py: Python<'_>) -> PyResult<Py<PyTestRun>> {
        if let Some(run) = &self.test_run {
            return Ok(run.clone_ref(py));
        }
        let state = self.state.take().unwrap_or_default();
        let results = std::mem::take(&mut self.results);
        let run = self
            .tested
            .runner(self.limits)
            .test_run(&self.files, state, results)
            .with_ruleset_digest(self.ruleset_digest);
        let run = Py::new(py, PyTestRun { run })?;
        self.test_run = Some(run.clone_ref(py));
        Ok(run)
    }
}

/// The case files of a run, copied out of their Python objects.
#[allow(clippy::needless_pass_by_value)] // PyO3 extracts lists by value.
fn owned(files: Vec<PyRef<'_, PyCaseFile>>) -> Vec<CaseFile> {
    files.iter().map(|file| file.file.clone()).collect()
}

/// A run of `files` against a rule set.
#[pyfunction]
#[pyo3(signature = (rules, files, max_results=None, max_steps=None, max_explained=None, ruleset_digest=false))]
pub(crate) fn run_ruleset(
    rules: &PyRuleSet,
    files: Vec<PyRef<'_, PyCaseFile>>,
    max_results: Option<usize>,
    max_steps: Option<usize>,
    max_explained: Option<usize>,
    ruleset_digest: bool,
) -> PyCaseRun {
    PyCaseRun::new(
        Tested::RuleSet(rules.shared()),
        owned(files),
        run_limits(max_results, max_steps, max_explained),
        ruleset_digest,
    )
}

/// A run of `files` against a decision table.
#[pyfunction]
#[pyo3(signature = (table, files, max_results=None, max_steps=None, max_explained=None, ruleset_digest=false))]
pub(crate) fn run_table(
    table: &PyDecisionTable,
    files: Vec<PyRef<'_, PyCaseFile>>,
    max_results: Option<usize>,
    max_steps: Option<usize>,
    max_explained: Option<usize>,
    ruleset_digest: bool,
) -> PyCaseRun {
    PyCaseRun::new(
        Tested::Table(Box::new(table.table.clone())),
        owned(files),
        run_limits(max_results, max_steps, max_explained),
        ruleset_digest,
    )
}

/// A run of a bundle's case files (`bundle_case_files`) against its rules.
#[pyfunction]
#[pyo3(signature = (bundle, files, max_results=None, max_steps=None, max_explained=None, ruleset_digest=false))]
pub(crate) fn run_bundle(
    bundle: &PyBundle,
    files: Vec<PyRef<'_, PyCaseFile>>,
    max_results: Option<usize>,
    max_steps: Option<usize>,
    max_explained: Option<usize>,
    ruleset_digest: bool,
) -> PyCaseRun {
    PyCaseRun::new(
        Tested::Bundle(Box::new(bundle.bundle.clone())),
        owned(files),
        run_limits(max_results, max_steps, max_explained),
        ruleset_digest,
    )
}

/// The results of a run and their summary, immutable.
#[pyclass(frozen, name = "TestRun", module = "cleanrules._cleanrules")]
pub(crate) struct PyTestRun {
    run: TestRun,
}

#[pymethods]
impl PyTestRun {
    /// The content of the test run document (`spec/tests.md`, *Test runs*),
    /// as JSON text.
    fn to_json(&self) -> String {
        to_string(&write_test_run(&self.run))
    }

    /// The test run as text (`spec/tests.md`, *Text*).
    fn text(&self) -> String {
        self.run.to_string()
    }

    /// The results, as `result_to_py` gives them.
    fn results<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyTuple>>> {
        self.run
            .results
            .iter()
            .map(|result| result_to_py(py, result))
            .collect()
    }

    /// Whether every case ran and passed.
    #[getter]
    fn all_passed(&self) -> bool {
        self.run.all_passed()
    }

    /// Whether this engine ran it: the same name and version.
    #[getter]
    fn is_current(&self) -> bool {
        self.run.engine() == (ENGINE_NAME, ENGINE_VERSION)
    }
}

/// Run the cases of a test run vector given as JSON text, for the
/// conformance runner's JSON route: the rules document, and either one case
/// file document (`cases`) or `(path, text)` files (for a bundle, the files
/// it names, by path). The test run's content, as JSON text.
#[pyfunction]
#[pyo3(name = "_test_run")]
#[pyo3(signature = (rules_json, cases_json, files, max_results=None, max_steps=None, max_explained=None, cancel_after=None, ruleset_digest=false))]
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
pub(crate) fn py_test_run(
    py: Python<'_>,
    rules_json: &str,
    cases_json: Option<&str>,
    files: Vec<(String, String)>,
    max_results: Option<usize>,
    max_steps: Option<usize>,
    max_explained: Option<usize>,
    cancel_after: Option<usize>,
    ruleset_digest: bool,
) -> PyResult<String> {
    let limits = Limits::default();
    let document = cleanrules::format::read_document(rules_json, &limits).or_raise(py)?;
    let case_files: Vec<CaseFile> = match (&document, cases_json) {
        (_, Some(text)) => match cleanrules::format::read_document(text, &limits).or_raise(py)? {
            Document::Cases(cases) => vec![CaseFile::new(cases)],
            _ => return Err(PyValueError::new_err("not a case file")),
        },
        (Document::Bundle(bundle), None) => CaseFile::bundle_files(bundle, |r| {
            files
                .iter()
                .find(|(path, _)| path == r.path())
                .map(|(_, text)| text.as_bytes().to_vec())
                .ok_or_else(|| PyValueError::new_err(format!("no file {}", r.path())))
        })?,
        (_, None) => files
            .iter()
            .map(|(path, text)| CaseFile::read(path, text.as_bytes(), &limits))
            .collect(),
    };
    let runner = match &document {
        Document::RuleSet(ruleset) => Runner::new(ruleset),
        Document::DecisionTable(table) => Runner::for_table(table),
        Document::Bundle(bundle) => Runner::for_bundle(bundle),
        _ => return Err(PyValueError::new_err("not rules")),
    }
    .with_limits(run_limits(max_results, max_steps, max_explained));
    let mut asked = 0;
    let run = runner
        .run_until(&case_files, || {
            asked += 1;
            cancel_after.is_some_and(|after| asked > after)
        })
        .with_ruleset_digest(ruleset_digest);
    Ok(to_string(&write_test_run(&run)))
}
