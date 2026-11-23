//! The static audit (`spec/audit.md`): the core audits a rule set or a
//! decision table; Python reads its report (`python/cleanrules/_audit.py`).
//!
//! The binding converts only: the limits in, and the report out, as its JSON
//! form, its text and each witness as Python data.

use cleanrules::audit::{Audit, AuditLimits, Severity, audit, audit_document, audit_table};
use cleanrules::format::{read_document, write_audit, write_ruleset};
use cleanrules::json::{Limits, to_string};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::convert;
use crate::errors::IntoPyResult;
use crate::rules::PyRuleSet;
use crate::tables::PyDecisionTable;

/// The report of an audit, immutable.
#[pyclass(frozen, name = "Audit", module = "cleanrules._cleanrules")]
pub(crate) struct PyAudit {
    audit: Audit,
}

#[pymethods]
impl PyAudit {
    /// The content of the audit document (`spec/audit.md`, *JSON*), as JSON
    /// text.
    fn to_json(&self) -> String {
        to_string(&write_audit(&self.audit))
    }

    /// The audit as text (`spec/audit.md`, *Text*).
    fn text(&self) -> String {
        self.audit.to_string()
    }

    /// Each diagnostic's witness as Python data, or `None`, in order.
    fn witnesses(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.audit
            .diagnostics
            .iter()
            .map(|d| match &d.witness {
                Some(witness) => Ok(convert::to_py(py, witness)?.unbind()),
                None => Ok(py.None()),
            })
            .collect()
    }

    /// Each diagnostic's text, as the audit's text writes it, in order.
    fn diagnostic_texts(&self) -> Vec<String> {
        self.audit
            .diagnostics
            .iter()
            .map(|d| self.audit.diagnostic_text(d))
            .collect()
    }

    /// The most serious severity among the diagnostics, or `None`.
    #[getter]
    fn max_severity(&self) -> Option<&'static str> {
        self.audit.max_severity().map(Severity::as_str)
    }

    /// Whether some diagnostic is at least as serious as `severity`.
    fn at_least(&self, severity: &str) -> PyResult<bool> {
        let severity = Severity::from_name(severity).ok_or_else(|| {
            PyValueError::new_err(format!(
                "unknown severity {severity:?}; use \"info\", \"warning\" or \"error\""
            ))
        })?;
        Ok(self.audit.at_least(severity))
    }
}

/// The core's limits: each one left out keeps its default.
fn limits(max_terms: Option<usize>, max_steps: Option<usize>) -> AuditLimits {
    let mut limits = AuditLimits::default();
    if let Some(terms) = max_terms {
        limits = limits.with_max_terms(terms);
    }
    if let Some(steps) = max_steps {
        limits = limits.with_max_steps(steps);
    }
    limits
}

/// Audit a rule set (`spec/audit.md`).
#[pyfunction]
#[pyo3(signature = (rules, max_terms=None, max_steps=None, ruleset_digest=false))]
pub(crate) fn audit_ruleset(
    py: Python<'_>,
    rules: &PyRuleSet,
    max_terms: Option<usize>,
    max_steps: Option<usize>,
    ruleset_digest: bool,
) -> PyResult<PyAudit> {
    let limits = limits(max_terms, max_steps);
    let ruleset = rules.shared();
    // The core does not touch Python objects: other threads may run meanwhile.
    let report = py.detach(|| audit(&ruleset, &limits)).or_raise(py)?;
    Ok(PyAudit {
        audit: report.with_ruleset_digest(ruleset_digest),
    })
}

/// Audit a decision table (`spec/audit.md`): its rule set, read cell by
/// cell, and its cells' fields.
#[pyfunction]
#[pyo3(signature = (table, max_terms=None, max_steps=None, ruleset_digest=false))]
pub(crate) fn audit_decision_table(
    py: Python<'_>,
    table: &PyDecisionTable,
    max_terms: Option<usize>,
    max_steps: Option<usize>,
    ruleset_digest: bool,
) -> PyResult<PyAudit> {
    let limits = limits(max_terms, max_steps);
    let table = &table.table;
    let report = py.detach(|| audit_table(table, &limits)).or_raise(py)?;
    Ok(PyAudit {
        audit: report.with_ruleset_digest(ruleset_digest),
    })
}

/// Audit the rules of a document given as JSON text with the core's
/// `audit_document`, for the conformance runner's JSON run: the audit's
/// content and the content of the rule set that evaluates its witnesses (the
/// one the document gives: a table's compiled one, a bundle's, or that of a
/// single rule), both as JSON text.
#[pyfunction]
#[pyo3(signature = (document_json, max_terms=None, max_steps=None, ruleset_digest=false))]
#[pyo3(name = "_audit")]
pub(crate) fn py_audit(
    py: Python<'_>,
    document_json: &str,
    max_terms: Option<usize>,
    max_steps: Option<usize>,
    ruleset_digest: bool,
) -> PyResult<(String, String)> {
    let limits = limits(max_terms, max_steps);
    let document = read_document(document_json, &Limits::default()).or_raise(py)?;
    let report = audit_document(&document, &limits)
        .or_raise(py)?
        .with_ruleset_digest(ruleset_digest);
    let ruleset = document.into_ruleset().or_raise(py)?;
    Ok((
        to_string(&write_audit(&report)),
        to_string(&write_ruleset(&ruleset)),
    ))
}
