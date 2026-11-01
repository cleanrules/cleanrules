//! Python binding of cleanrules: the native module `cleanrules._cleanrules`.
//!
//! A thin layer over the core (`CLAUDE.md`, invariant 2): it builds rules,
//! converts data and translates errors, and never evaluates on its own. The
//! idiomatic API (`F`, operators, pitfall guards) is Python, in
//! `python/cleanrules`.
//!
//! The module supports free-threaded Python (the default of `PyO3`): it keeps no
//! global mutable state.

mod convert;
mod errors;
mod node;
mod rules;

use pyo3::prelude::*;

/// The native module, imported by the `cleanrules` package.
#[pymodule]
fn _cleanrules(module: &Bound<'_, PyModule>) -> PyResult<()> {
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
    module.add_function(wrap_pyfunction!(node::neg, module)?)?;
    module.add_function(wrap_pyfunction!(node::node_from_json, module)?)?;
    module.add_class::<rules::PyRule>()?;
    module.add_class::<rules::PyRuleSet>()?;
    module.add_class::<rules::PyDecision>()?;
    module.add_function(wrap_pyfunction!(rules::rule, module)?)?;
    module.add_function(wrap_pyfunction!(rules::ruleset, module)?)?;
    Ok(())
}
