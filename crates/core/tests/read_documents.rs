//! Reading documents against the `read` conformance vectors: conditions, rules,
//! rule sets, and decisions read with their rule set.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use cleanrules::Error;
use cleanrules::format::{
    read_decision_document, read_document, read_ruleset, write_decision, write_document,
};
use cleanrules::json::{Json, JsonNumber, Limits, to_string};
use common::{get, int, limits_of, same, text, vectors};

const FILES: [&str; 8] = [
    "format/documents.json",
    "format/values.json",
    "format/conditions.json",
    "format/limits.json",
    "format/rules.json",
    "format/rulesets.json",
    "format/decisions.json",
    "format/yaml.json",
];

/// A decision as the document it is written in.
fn decision_document(decision: &cleanrules::rules::Decision<'_>) -> Json {
    Json::Object(vec![
        (
            "version".into(),
            Json::Number(JsonNumber::from_literal("1")),
        ),
        ("decision".into(), write_decision(decision)),
    ])
}

/// Read the vector's document, as a decision when a rule set is given.
fn read(given: &Json, source: &str) -> Result<Json, Error> {
    #[cfg(feature = "yaml")]
    if get(given, "yaml").is_some() {
        if let Some(ruleset_json) = get(given, "ruleset") {
            let ruleset = read_ruleset(ruleset_json, "/ruleset", &Limits::default()).unwrap();
            return cleanrules::format::read_yaml_decision_document(
                source,
                &ruleset,
                &limits_of(given),
            )
            .map(|decision| decision_document(&decision));
        }
        return cleanrules::format::read_yaml_document(source, &limits_of(given))
            .map(|d| write_document(&d));
    }
    match get(given, "ruleset") {
        Some(ruleset_json) => {
            let ruleset = read_ruleset(ruleset_json, "/ruleset", &Limits::default()).unwrap();
            read_decision_document(source, &ruleset, &limits_of(given))
                .map(|decision| decision_document(&decision))
        }
        None => read_document(source, &limits_of(given)).map(|d| write_document(&d)),
    }
}

fn check(vector: &Json) -> Option<String> {
    let given = get(vector, "given").unwrap();
    if get(given, "yaml").is_some() && !cfg!(feature = "yaml") {
        return None; // YAML vectors need the `yaml` feature.
    }
    let source = match (
        text(get(given, "text")).or(text(get(given, "yaml"))),
        get(given, "document"),
    ) {
        (Some(source), _) => source.to_owned(),
        (None, Some(document)) => to_string(document),
        _ => panic!("nothing given"),
    };
    let expect = get(vector, "expect").unwrap();
    match (read(given, &source), get(expect, "error")) {
        (Ok(written), None) => {
            let wanted = get(expect, "document").unwrap();
            (!same(&written, wanted)).then(|| {
                format!(
                    "wrote {}, expected {}",
                    to_string(&written),
                    to_string(wanted)
                )
            })
        }
        (Ok(written), Some(error)) => Some(format!(
            "read {}, expected {}",
            to_string(&written),
            to_string(error)
        )),
        (Err(error), None) => Some(format!(
            "{} at {:?}: {error}",
            error.code(),
            error.context()
        )),
        (Err(error), Some(wanted)) => {
            let context = error.context();
            let ok = error.code().as_str() == text(get(wanted, "code")).unwrap()
                && (get(wanted, "pointer").is_none()
                    || context.pointer.as_deref() == text(get(wanted, "pointer")))
                && (get(wanted, "line").is_none() || context.line == int(get(wanted, "line")))
                && (get(wanted, "column").is_none()
                    || context.column == int(get(wanted, "column")));
            (!ok).then(|| {
                format!(
                    "got {} pointer={:?} line={:?} column={:?} ({error}), expected {}",
                    error.code(),
                    context.pointer,
                    context.line,
                    context.column,
                    to_string(wanted)
                )
            })
        }
    }
}

#[test]
fn read_vectors() {
    let (mut run, mut failures) = (0, Vec::new());
    for file in FILES {
        let all = vectors(file);
        let Json::Array(list) = &all else { panic!() };
        for vector in list {
            run += 1;
            if let Some(problem) = check(vector) {
                failures.push(format!("{}: {problem}", text(get(vector, "id")).unwrap()));
            }
        }
    }
    println!("{run} read vectors run");
    assert!(run >= 187, "only {run} read vectors");
    assert!(
        failures.is_empty(),
        "{} of {run} failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
