//! The JSON reader against the conformance vectors.
//!
//! Every vector file must be readable by the reader itself. The `read` vectors
//! whose expected error is located by line and column (syntax errors, and limits
//! checked while parsing) are run here; the others need the document readers that
//! later tasks add.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::fs;

use cleanrules::json::{Json, Limits, parse};
use common::{get, int, limits_of, text, vector_files, vectors_dir};

#[test]
fn every_vector_file_is_readable() {
    let files = vector_files();
    assert!(
        files.len() > 20,
        "vectors not found in {}",
        vectors_dir().display()
    );
    for path in files {
        let content = fs::read_to_string(&path).unwrap();
        if let Err(error) = parse(&content, &Limits::default()) {
            panic!("{}: {error} at {:?}", path.display(), error.context());
        }
    }
}

#[test]
fn lexical_read_vectors() {
    let mut run = 0;
    let mut failures = Vec::new();
    for path in vector_files() {
        let document = parse(&fs::read_to_string(&path).unwrap(), &Limits::default()).unwrap();
        let Some(Json::Array(vectors)) = get(&document, "vectors") else {
            continue;
        };
        for vector in vectors {
            let id = text(get(vector, "id")).unwrap_or("?");
            let Some(given) = get(vector, "given") else {
                continue;
            };
            let Some(source) = text(get(given, "text")) else {
                continue;
            };
            let Some(expected) = get(vector, "expect").and_then(|e| get(e, "error")) else {
                continue;
            };
            let code = text(get(expected, "code")).unwrap_or_default();
            let lexical = int(get(expected, "line")).is_some()
                || (code == "limit_exceeded" && get(expected, "pointer").is_none());
            if !lexical {
                continue;
            }
            run += 1;
            let wanted = (
                code.to_owned(),
                int(get(expected, "line")),
                int(get(expected, "column")),
            );
            let actual = match parse(source, &limits_of(given)) {
                Ok(_) => ("<no error>".to_owned(), None, None),
                Err(error) => (
                    error.code().as_str().to_owned(),
                    error.context().line,
                    error.context().column,
                ),
            };
            if actual != wanted {
                failures.push(format!("{id}: expected {wanted:?}, got {actual:?}"));
            }
        }
    }
    assert!(run >= 10, "only {run} lexical vectors found");
    println!("{run} lexical read vectors");
    assert!(
        failures.is_empty(),
        "{} of {run} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
