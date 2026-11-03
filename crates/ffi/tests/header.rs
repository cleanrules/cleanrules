//! The header declares every function the library exports: cbindgen skips
//! some silently, such as functions a macro defines.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::Path;

/// The names of the functions after `#[unsafe(no_mangle)]` in a source.
fn exported(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut marked = false;
    for line in source.lines().map(str::trim) {
        if line == "#[unsafe(no_mangle)]" {
            marked = true;
        } else if marked && let Some(at) = line.find("fn ") {
            let rest = &line[at + 3..];
            let end = rest.find(['(', '<']).unwrap_or(rest.len());
            names.push(rest[..end].to_owned());
            marked = false;
        }
    }
    names
}

#[test]
#[cfg_attr(miri, ignore = "reads the sources and the header")]
fn every_exported_function_is_declared() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let header = fs::read_to_string(root.join("include/cleanrules.h")).unwrap();
    let mut names = Vec::new();
    for entry in fs::read_dir(root.join("src")).unwrap() {
        let path = entry.unwrap().path();
        names.extend(exported(&fs::read_to_string(&path).unwrap()));
    }
    assert!(
        names.len() > 100,
        "found only {} exported functions",
        names.len()
    );
    let missing: Vec<_> = names
        .iter()
        .filter(|name| {
            !header.contains(&format!(" {name}(")) && !header.contains(&format!("*{name}("))
        })
        .collect();
    assert!(missing.is_empty(), "not in cleanrules.h: {missing:?}");
}
