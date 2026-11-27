//! Python binding of cleanrules: the native module `cleanrules._cleanrules`.
//!
//! A thin layer over the core (`CLAUDE.md`, invariant 2): it builds rules,
//! converts data and translates errors, and never evaluates on its own. The
//! idiomatic API (`F`, operators, pitfall guards) is Python, in
//! `python/cleanrules`.
//!
//! The module supports free-threaded Python (the default of `PyO3`): it keeps no
//! global mutable state.

mod agents;
mod audit;
mod bundles;
mod cases;
mod convert;
mod documents;
mod errors;
mod json_api;
mod node;
mod read;
mod rules;
mod tables;
mod trees;

use pyo3::prelude::*;

/// The native module, imported by the `cleanrules` package.
#[pymodule]
#[pyo3(name = "_cleanrules")]
fn native_module(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add("FORMAT_VERSION", cleanrules::FORMAT_VERSION)?;
    module.add_class::<node::PyNode>()?;
    module.add_function(wrap_pyfunction!(node::field, module)?)?;
    module.add_function(wrap_pyfunction!(node::constant, module)?)?;
    module.add_function(wrap_pyfunction!(node::compare, module)?)?;
    module.add_function(wrap_pyfunction!(node::and_, module)?)?;
    module.add_function(wrap_pyfunction!(node::or_, module)?)?;
    module.add_function(wrap_pyfunction!(node::not_, module)?)?;
    module.add_function(wrap_pyfunction!(node::is_in, module)?)?;
    module.add_function(wrap_pyfunction!(node::between, module)?)?;
    module.add_function(wrap_pyfunction!(node::is_null, module)?)?;
    module.add_function(wrap_pyfunction!(node::matches, module)?)?;
    module.add_function(wrap_pyfunction!(node::string_test, module)?)?;
    module.add_function(wrap_pyfunction!(node::arith, module)?)?;
    module.add_function(wrap_pyfunction!(node::abs_, module)?)?;
    module.add_function(wrap_pyfunction!(node::round_, module)?)?;
    module.add_function(wrap_pyfunction!(node::neg, module)?)?;
    module.add_function(wrap_pyfunction!(node::node_from_json, module)?)?;
    module.add_class::<rules::PyRule>()?;
    module.add_class::<rules::PyRuleSet>()?;
    module.add_class::<rules::PyDecision>()?;
    module.add_class::<tables::PyDecisionTable>()?;
    module.add_class::<bundles::PyBundle>()?;
    module.add_class::<audit::PyAudit>()?;
    module.add_function(wrap_pyfunction!(audit::audit_ruleset, module)?)?;
    module.add_function(wrap_pyfunction!(audit::audit_decision_table, module)?)?;
    module.add_class::<cases::PyCases>()?;
    module.add_class::<cases::PyCase>()?;
    module.add_class::<cases::PyExpectation>()?;
    module.add_class::<cases::PyCaseFile>()?;
    module.add_class::<cases::PyCaseRun>()?;
    module.add_class::<cases::PyTestRun>()?;
    module.add_function(wrap_pyfunction!(cases::expectation_new, module)?)?;
    module.add_function(wrap_pyfunction!(cases::case_new, module)?)?;
    module.add_function(wrap_pyfunction!(cases::cases_new, module)?)?;
    module.add_function(wrap_pyfunction!(cases::case_file_of, module)?)?;
    module.add_function(wrap_pyfunction!(cases::case_file_read, module)?)?;
    module.add_function(wrap_pyfunction!(cases::bundle_case_files, module)?)?;
    module.add_function(wrap_pyfunction!(cases::run_ruleset, module)?)?;
    module.add_function(wrap_pyfunction!(cases::run_table, module)?)?;
    module.add_function(wrap_pyfunction!(cases::run_bundle, module)?)?;
    module.add_function(wrap_pyfunction!(documents::dump_cases, module)?)?;
    module.add_function(wrap_pyfunction!(bundles::make_bundle, module)?)?;
    module.add_function(wrap_pyfunction!(bundles::case_digest_matches, module)?)?;
    module.add_function(wrap_pyfunction!(bundles::check_case_file, module)?)?;
    module.add_function(wrap_pyfunction!(bundles::default_bundle_limits, module)?)?;
    module.add_function(wrap_pyfunction!(documents::dump_bundle, module)?)?;
    module.add_function(wrap_pyfunction!(rules::rule, module)?)?;
    module.add_function(wrap_pyfunction!(rules::ruleset, module)?)?;
    module.add_function(wrap_pyfunction!(documents::load_document, module)?)?;
    module.add_function(wrap_pyfunction!(documents::load_rule, module)?)?;
    module.add_function(wrap_pyfunction!(documents::dump_table, module)?)?;
    module.add_function(wrap_pyfunction!(tables::table_from_grid, module)?)?;
    module.add_function(wrap_pyfunction!(documents::dump_condition, module)?)?;
    module.add_function(wrap_pyfunction!(documents::dump_rule, module)?)?;
    module.add_function(wrap_pyfunction!(documents::dump_ruleset, module)?)?;
    module.add_function(wrap_pyfunction!(documents::canonical_bytes, module)?)?;
    module.add_function(wrap_pyfunction!(documents::digest, module)?)?;
    module.add_function(wrap_pyfunction!(trees::evaluate_condition, module)?)?;
    module.add_function(wrap_pyfunction!(trees::evaluate_value, module)?)?;
    module.add_class::<agents::PyCatalog>()?;
    module.add_function(wrap_pyfunction!(agents::scalar_json, module)?)?;
    module.add(
        "TOOLS",
        pyo3::types::PyTuple::new(module.py(), agents::tool_names())?,
    )?;
    module.add("DEFAULT_BUDGET", cleanrules::agents::DEFAULT_BUDGET)?;
    module.add("MIN_BUDGET", cleanrules::agents::MIN_BUDGET)?;
    module.add("MAX_BUDGET", cleanrules::agents::MAX_BUDGET)?;
    module.add_function(wrap_pyfunction!(json_api::py_read_document, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_read_decision, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_evaluate, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_evaluate_many, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_explain, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_summary, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_evaluate_node, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_make_bundle, module)?)?;
    module.add_function(wrap_pyfunction!(json_api::py_compare_bundles, module)?)?;
    module.add_function(wrap_pyfunction!(audit::py_audit, module)?)?;
    module.add_function(wrap_pyfunction!(cases::py_test_run, module)?)?;
    Ok(())
}
