//! The core's conformance runner (`conformance/README.md`).
//!
//! Runs every vector of the kinds this version of the core implements and
//! reports each failure with its id. Kinds still to come (rule sets, decisions,
//! explanations, summaries) are counted as pending.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::fs;

use cleanrules::Error;
use cleanrules::eval::{Options, evaluate_tree, evaluate_value_tree};
use cleanrules::format::{
    ValueContext, read_node, read_ruleset, read_value, write_decision, write_path, write_result,
};
use cleanrules::json::{Json, Limits, parse, to_string};
use cleanrules::logic::Kleene;
use common::{get, same, text, vector_files};

/// Remove `detail` from reasons, which is never compared.
fn strip_detail(json: &Json) -> Json {
    match json {
        Json::Object(members) => Json::Object(
            members
                .iter()
                .filter(|(k, _)| k != "detail")
                .map(|(k, v)| (k.clone(), strip_detail(v)))
                .collect(),
        ),
        Json::Array(items) => Json::Array(items.iter().map(strip_detail).collect()),
        other => other.clone(),
    }
}

/// The compared fields of an error (`spec/errors.md`, *Comparing errors*).
fn error_json(error: &Error) -> Json {
    let context = error.context();
    let mut members = vec![(
        "code".to_owned(),
        Json::String(error.code().as_str().into()),
    )];
    if let Some(rule) = &context.rule {
        members.push(("rule".into(), Json::String(rule.clone())));
    }
    if let Some(path) = &context.path {
        members.push(("path".into(), write_path(path)));
    }
    if !context.types.is_empty() {
        let types = context
            .types
            .iter()
            .map(|g| Json::String(g.as_str().into()))
            .collect();
        members.push(("types".into(), Json::Array(types)));
    }
    if !context.rules.is_empty() {
        let rules = context
            .rules
            .iter()
            .map(|r| Json::String(r.clone()))
            .collect();
        members.push(("rules".into(), Json::Array(rules)));
    }
    if let Some(row) = context.row {
        members.push((
            "row".into(),
            Json::Number(cleanrules::json::JsonNumber::from_literal(row.to_string())),
        ));
    }
    Json::Object(members)
}

/// Compare error objects without regard to member order.
fn same_error(actual: &Json, expected: &Json) -> bool {
    let (Json::Object(a), Json::Object(b)) = (actual, expected) else {
        return false;
    };
    a.len() == b.len()
        && b.iter()
            .all(|(k, v)| a.iter().any(|(l, w)| l == k && same(w, v)))
}

fn compare(actual: Result<Json, Error>, expect: &Json, key: &str) -> Result<(), String> {
    match (actual, get(expect, "error")) {
        (Ok(json), None) => {
            let actual = strip_detail(&json);
            let wanted = strip_detail(get(expect, key).unwrap());
            if same(&actual, &wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got {}\nwant {}",
                    to_string(&actual),
                    to_string(&wanted)
                ))
            }
        }
        (Ok(json), Some(error)) => Err(format!(
            "got {}, expected the error {}",
            to_string(&json),
            to_string(error)
        )),
        (Err(error), None) => Err(format!(
            "unexpected error {}: {error}",
            to_string(&error_json(&error))
        )),
        (Err(error), Some(wanted)) => {
            let actual = error_json(&error);
            if same_error(&actual, wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got the error {}, expected {}",
                    to_string(&actual),
                    to_string(wanted)
                ))
            }
        }
    }
}

fn run_evaluate(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let limits = Limits::default();
    let ruleset = read_ruleset(get(given, "ruleset").unwrap(), "/ruleset", &limits)
        .map_err(|e| format!("reading the rule set: {e} at {:?}", e.context().pointer))?;
    let expect = get(vector, "expect").unwrap();
    if let Some(Json::Array(items)) = get(given, "inputs") {
        let inputs = items
            .iter()
            .map(|i| read_value(i, "/inputs", ValueContext::Data))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("reading the inputs: {e}"))?;
        let actual = ruleset
            .evaluate_many(&inputs)
            .map(|decisions| Json::Array(decisions.iter().map(write_decision).collect()));
        return compare(actual, expect, "decisions");
    }
    let input = read_value(get(given, "input").unwrap(), "/input", ValueContext::Data)
        .map_err(|e| format!("reading the input: {e}"))?;
    compare(
        ruleset.evaluate(&input).map(|d| write_decision(&d)),
        expect,
        "decision",
    )
}

fn run(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let kind = text(get(given, "kind")).unwrap();
    if kind == "evaluate" {
        return run_evaluate(vector);
    }
    let limits = Limits::default();
    let node_key = if kind == "condition" {
        "condition"
    } else {
        "value"
    };
    let node = read_node(get(given, node_key).unwrap(), "", &limits)
        .map_err(|e| format!("reading the node: {e}"))?;
    let input = read_value(get(given, "input").unwrap(), "/input", ValueContext::Data)
        .map_err(|e| format!("reading the input: {e}"))?;
    let options = Options {
        strict: matches!(get(given, "strict"), Some(Json::Bool(true))),
        rule: None,
    };
    let result = if kind == "condition" {
        evaluate_tree(&node, &input, &Kleene, options)
    } else {
        evaluate_value_tree(&node, &input, &Kleene, options)
    };
    let expect = get(vector, "expect").unwrap();
    match (result, get(expect, "error")) {
        (Ok(tree), None) => {
            let actual = strip_detail(&write_result(&tree));
            let wanted = strip_detail(get(expect, "result").unwrap());
            if same(&actual, &wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got {}\nwant {}",
                    to_string(&actual),
                    to_string(&wanted)
                ))
            }
        }
        (Ok(tree), Some(error)) => Err(format!(
            "got {}, expected the error {}",
            to_string(&write_result(&tree)),
            to_string(error)
        )),
        (Err(error), None) => Err(format!(
            "unexpected error {}: {error}",
            to_string(&error_json(&error))
        )),
        (Err(error), Some(wanted)) => {
            let actual = error_json(&error);
            if same_error(&actual, wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got the error {}, expected {}",
                    to_string(&actual),
                    to_string(wanted)
                ))
            }
        }
    }
}

#[test]
fn conformance_vectors() {
    let mut passed = 0;
    let mut pending: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for path in vector_files() {
        let file = parse(&fs::read_to_string(&path).unwrap(), &Limits::default()).unwrap();
        let Some(Json::Array(vectors)) = get(&file, "vectors") else {
            continue;
        };
        for vector in vectors {
            let kind = text(get(get(vector, "given").unwrap(), "kind")).unwrap();
            if !matches!(kind, "condition" | "value" | "evaluate") {
                *pending.entry(kind.to_owned()).or_default() += 1;
                continue;
            }
            match run(vector) {
                Ok(()) => passed += 1,
                Err(problem) => {
                    failures.push(format!("{}: {problem}", text(get(vector, "id")).unwrap()));
                }
            }
        }
    }
    println!(
        "{passed} passed, {} failed; pending kinds: {pending:?}",
        failures.len()
    );
    let shown: Vec<&String> = failures.iter().take(15).collect();
    assert!(
        failures.is_empty(),
        "{} failed, first ones:\n{}",
        failures.len(),
        shown
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
