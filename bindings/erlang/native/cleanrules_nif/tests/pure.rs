//! The unit tests of the modules that need no VM: big integers read from the
//! external term format, the tree of paths, and panics caught as failures.
//!
//! The library itself has no test harness (`Cargo.toml`): it calls `enif_*`
//! functions that only the BEAM provides, so a test executable that links it
//! fails on Linux. These modules use neither `rustler` nor those functions, so
//! this target compiles them on their own, with their `#[cfg(test)]` tests.

// Items that only the NIF uses are dead here.
#![allow(dead_code)]

#[path = "../src/bignum.rs"]
mod bignum;
#[path = "../src/panics.rs"]
mod panics;
#[path = "../src/paths.rs"]
mod paths;
