//! Bundles (`spec/bundles.md`, task R.5): the core makes, reads, writes and
//! compares them; Python wraps the result.

use cleanrules::bundle::{Bundle, BundleId, BundleRules, CaseFile, Revision};
use cleanrules::digest::Digest;
use cleanrules::json::{Limits, to_string};
use pyo3::exceptions::{PyKeyError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyTuple};

use crate::documents::ruleset_parts;
use crate::errors::IntoPyResult;
use crate::rules::PyRuleSet;
use crate::tables::PyDecisionTable;

/// A bundle of the core, immutable.
#[pyclass(frozen, name = "Bundle", module = "cleanrules._cleanrules")]
pub(crate) struct PyBundle {
    pub(crate) bundle: Bundle,
}

/// The limits a bundle records: those given, the others at their defaults.
pub(crate) fn recorded_limits(
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
    max_path_segments: Option<usize>,
    max_table_size: Option<usize>,
    max_values: Option<usize>,
) -> Limits {
    let mut limits = Limits::default();
    let given = [
        (&mut limits.max_size, max_size),
        (&mut limits.max_nesting, max_nesting),
        (&mut limits.max_depth, max_depth),
        (&mut limits.max_path_segments, max_path_segments),
        (&mut limits.max_table_size, max_table_size),
        (&mut limits.max_values, max_values),
    ];
    for (limit, value) in given {
        if let Some(value) = value {
            *limit = value;
        }
    }
    limits
}

/// Make a bundle of a rule set or a decision table with this engine
/// (`spec/bundles.md`, *Making a bundle*); `cases` are `(path, bytes)` pairs.
#[pyfunction]
#[pyo3(signature = (rules, id, revision, cases, max_size=None, max_nesting=None, max_depth=None, max_path_segments=None, max_table_size=None, max_values=None))]
#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)] // The limits are keyword arguments in Python; PyO3 extracts lists by value.
pub(crate) fn make_bundle(
    py: Python<'_>,
    rules: &Bound<'_, PyAny>,
    id: &str,
    revision: &str,
    cases: Vec<(String, Bound<'_, PyBytes>)>,
    max_size: Option<usize>,
    max_nesting: Option<usize>,
    max_depth: Option<usize>,
    max_path_segments: Option<usize>,
    max_table_size: Option<usize>,
    max_values: Option<usize>,
) -> PyResult<PyBundle> {
    let rules: BundleRules = if let Ok(ruleset) = rules.cast::<PyRuleSet>() {
        ruleset.get().ruleset().clone().into()
    } else if let Ok(table) = rules.cast::<PyDecisionTable>() {
        table.get().table.clone().into()
    } else {
        return Err(PyTypeError::new_err("a rule set or a decision table"));
    };
    let cases = cases
        .iter()
        .map(|(path, bytes)| CaseFile::new(path, bytes.as_bytes()))
        .collect::<cleanrules::Result<Vec<_>>>()
        .or_raise(py)?;
    let limits = recorded_limits(
        max_size,
        max_nesting,
        max_depth,
        max_path_segments,
        max_table_size,
        max_values,
    );
    let bundle = make(rules, id, revision, cases, &limits).or_raise(py)?;
    Ok(PyBundle { bundle })
}

/// A bundle as the core makes it (`spec/bundles.md`, *Making a bundle*).
pub(crate) fn make(
    rules: BundleRules,
    id: &str,
    revision: &str,
    cases: Vec<CaseFile>,
    limits: &Limits,
) -> cleanrules::Result<Bundle> {
    Bundle::new(rules, BundleId::new(id)?, Revision::new(revision)?)
        .with_cases(cases)?
        .with_limits(limits)
}

/// The default limits a bundle records, `(name, value)` in the order of the
/// specification.
#[pyfunction]
pub(crate) fn default_bundle_limits() -> Vec<(&'static str, u64)> {
    cleanrules::bundle::EngineLimits::default()
        .iter()
        .map(|(limit, n)| (limit.as_str(), n))
        .collect()
}

/// Check a case file's path and digest (`spec/bundles.md`, *Cases*).
#[pyfunction]
pub(crate) fn check_case_file(py: Python<'_>, path: &str, digest: &str) -> PyResult<()> {
    let digest: Digest = digest.parse().or_raise(py)?;
    CaseFile::with_digest(path, digest).or_raise(py)?;
    Ok(())
}

/// Whether `content` is the case file whose digest is `digest`
/// (`spec/bundles.md`, *Cases*).
#[pyfunction]
pub(crate) fn case_matches(
    py: Python<'_>,
    path: &str,
    digest: &str,
    content: &[u8],
) -> PyResult<bool> {
    let digest: Digest = digest.parse().or_raise(py)?;
    Ok(CaseFile::with_digest(path, digest)
        .or_raise(py)?
        .matches(content))
}

#[pymethods]
impl PyBundle {
    /// The identifier.
    #[getter]
    fn id(&self) -> &str {
        self.bundle.id().as_str()
    }

    /// The revision.
    #[getter]
    fn revision(&self) -> &str {
        self.bundle.revision().as_str()
    }

    /// The kind of rules it holds: `"ruleset"` or `"decision_table"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.bundle.rules() {
            BundleRules::DecisionTable(_) => "decision_table",
            _ => "ruleset",
        }
    }

    /// The number of case files.
    #[getter]
    fn case_count(&self) -> usize {
        self.bundle.cases().len()
    }

    /// The rules as held: `("ruleset", parts)` or `("decision_table", table)`.
    fn rules<'py>(&self, py: Python<'py>) -> PyResult<(&'static str, Bound<'py, PyAny>)> {
        Ok(match self.bundle.rules() {
            BundleRules::DecisionTable(table) => (
                "decision_table",
                PyDecisionTable {
                    table: table.clone(),
                }
                .into_pyobject(py)?
                .into_any(),
            ),
            // `BundleRules` is `#[non_exhaustive]`: anything else gives its rule set.
            _ => (
                "ruleset",
                ruleset_parts(py, self.bundle.ruleset())?.into_any(),
            ),
        })
    }

    /// The rule set digest of its rules.
    #[getter]
    fn ruleset_digest(&self) -> String {
        self.bundle.ruleset_digest().to_string()
    }

    /// `(name, version, [(limit, value), ...], is_current)`, the limits in
    /// the order of the specification.
    fn engine<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let engine = self.bundle.engine();
        let pairs: Vec<(&'static str, u64)> = engine
            .limits()
            .iter()
            .map(|(limit, n)| (limit.as_str(), n))
            .collect();
        (engine.name(), engine.version(), pairs, engine.is_current()).into_pyobject(py)
    }

    /// The contract in its JSON form, as text.
    fn contract_json(&self) -> String {
        to_string(&self.bundle.contract().to_json())
    }

    /// The case files as `(path, digest)`, in order.
    fn cases(&self) -> Vec<(String, String)> {
        self.bundle
            .cases()
            .iter()
            .map(|case| (case.path().to_owned(), case.digest().to_string()))
            .collect()
    }

    /// Whether `content` is the case file at `path`; `KeyError` for a path
    /// the bundle does not have, with the paths it has.
    fn case_matches(&self, path: &str, content: &[u8]) -> PyResult<bool> {
        self.bundle.case_matches(path, content).ok_or_else(|| {
            let cases = self.bundle.cases();
            let known = if cases.is_empty() {
                ": it has none".to_owned()
            } else {
                let paths: Vec<String> = cases.iter().map(|c| format!("{:?}", c.path())).collect();
                format!("; its case files are {}", paths.join(", "))
            };
            PyKeyError::new_err(format!("the bundle refers to no case file {path:?}{known}"))
        })
    }

    /// Compare with another bundle: `(relation, identical, differences)`.
    fn compare(&self, other: &Bound<'_, PyBundle>) -> (&'static str, bool, Vec<&'static str>) {
        let comparison = self.bundle.compare(&other.get().bundle);
        (
            comparison.relation().as_str(),
            comparison.is_identical(),
            comparison
                .differences()
                .iter()
                .map(|difference| difference.as_str())
                .collect(),
        )
    }
}
