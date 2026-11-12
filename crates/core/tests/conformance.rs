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
use cleanrules::eval::{Options, evaluate_value_tree};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    ValueContext, read_node, read_ruleset, read_value, write_decision, write_path, write_result,
};
use cleanrules::format::{
    read_decision_document, read_logic, write_any_result, write_explanation, write_summary,
};
use cleanrules::json::{Json, Limits, parse, to_string};
use cleanrules::logic::{AnyLogic, Kleene};
use common::{get, on_small_stack, same, text, vector_files};

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
    if let Some(name) = &context.name {
        members.push(("name".into(), Json::String(name.clone())));
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
    let decision = ruleset.evaluate(&input);
    if let Ok(decision) = &decision {
        // Every decision the core produces reads back, passes the checks of
        // *Reading a decision*, and is written the same way.
        let written = write_decision(decision);
        let document = Json::Object(vec![
            (
                "version".into(),
                Json::Number(cleanrules::json::JsonNumber::from_literal("1")),
            ),
            ("decision".into(), written.clone()),
        ]);
        let read =
            read_decision_document(&to_string(&document), &ruleset, &limits).map_err(|e| {
                format!(
                    "the decision does not read back: {e} at {:?}",
                    e.context().pointer
                )
            })?;
        if !same(&write_decision(&read), &written) {
            return Err("the decision read back is written differently".into());
        }
    }
    compare(decision.map(|d| write_decision(&d)), expect, "decision")
}

fn run_explain(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let ruleset = read_ruleset(
        get(given, "ruleset").unwrap(),
        "/ruleset",
        &Limits::default(),
    )
    .map_err(|e| format!("reading the rule set: {e}"))?;
    let expect = get(vector, "expect").unwrap();
    if text(get(given, "kind")) == Some("summary") {
        return compare(Ok(write_summary(&summary(&ruleset))), expect, "summary");
    }
    let input = read_value(get(given, "input").unwrap(), "/input", ValueContext::Data)
        .map_err(|e| format!("reading the input: {e}"))?;
    let decision = ruleset.evaluate(&input);
    if let Ok(d) = &decision {
        // A decision read back from JSON explains exactly as the evaluated one.
        let document = Json::Object(vec![
            (
                "version".into(),
                Json::Number(cleanrules::json::JsonNumber::from_literal("1")),
            ),
            ("decision".into(), write_decision(d)),
        ]);
        let read = read_decision_document(&to_string(&document), &ruleset, &Limits::default())
            .map_err(|e| format!("the decision does not read back: {e}"))?;
        if !same(
            &write_explanation(&explain(&read)),
            &write_explanation(&explain(d)),
        ) {
            return Err("the decision read back explains differently".into());
        }
    }
    let (decision_json, explanation_json) = match &decision {
        Ok(d) => (Ok(write_decision(d)), Ok(write_explanation(&explain(d)))),
        Err(e) => (
            Err(cleanrules::Error::new(e.code(), e.message())),
            Err(cleanrules::Error::new(e.code(), e.message())),
        ),
    };
    compare(decision_json, expect, "decision")?;
    compare(explanation_json, expect, "explanation")
}

fn run(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let kind = text(get(given, "kind")).unwrap();
    if kind == "evaluate" {
        return run_evaluate(vector);
    }
    if kind == "explain" || kind == "summary" {
        return run_explain(vector);
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
    let expect = get(vector, "expect").unwrap();
    if kind == "value" {
        let result = evaluate_value_tree(&node, &input, &Kleene, options);
        return compare(result.map(|tree| write_result(&tree)), expect, "result");
    }
    // The logic in its JSON form (`spec/json-format.md`, *Logics*), boolean
    // when absent; an error reading it is the vector's result.
    let logic = get(given, "logic").map_or(Ok(AnyLogic::Boolean(Kleene)), |json| {
        read_logic(json, "/logic")
    });
    let result = logic
        .and_then(|logic| logic.evaluate_tree(&node, &input, options))
        .map(|tree| write_any_result(&tree));
    compare(result, expect, "result")
}

#[test]
fn conformance_vectors() {
    // On a small stack, as a binding's thread may be (`spec/json-format.md`,
    // *Errors and limits*).
    on_small_stack(|| {
        let mut passed = 0;
        let mut pending: BTreeMap<String, usize> = BTreeMap::new();
        let mut failures = Vec::new();
        for path in vector_files() {
            let file = parse(&fs::read_to_string(&path).unwrap(), &Limits::default()).unwrap();
            let Some(Json::Array(vectors)) = get(&file, "vectors") else {
                continue;
            };
            for vector in vectors {
                let given = get(vector, "given").unwrap();
                let kind = text(get(given, "kind")).unwrap();
                // `max_values` bounds native data, which the core never reads:
                // only the bindings' native runs check these vectors.
                if get(given, "limits").is_some() && kind == "evaluate" {
                    *pending.entry("native-only limits".to_owned()).or_default() += 1;
                    continue;
                }
                if !matches!(
                    kind,
                    "condition" | "value" | "evaluate" | "explain" | "summary"
                ) {
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
    });
}
