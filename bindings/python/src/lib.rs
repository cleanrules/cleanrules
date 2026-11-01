//! Python binding of cleanrules: the native module `cleanrules._cleanrules`.
//!
//! A thin layer over the core (`CLAUDE.md`, invariant 2): it builds rules,
//! converts data and translates errors, and never evaluates on its own. This
//! first version only reports its versions; the API comes in phase C.2.
//!
//! The module supports free-threaded Python (the default of `PyO3`): it keeps no
//! global mutable state.

use pyo3::prelude::*;

/// The native module, imported by the `cleanrules` package.
#[pymodule]
fn _cleanrules(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add("FORMAT_VERSION", cleanrules::FORMAT_VERSION)?;
    Ok(())
}
