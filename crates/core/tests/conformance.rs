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
use cleanrules::agents::Catalog;
use cleanrules::eval::{Options, evaluate_value_tree};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    Document, read_decision_document, read_document, read_parsed_document, write_explanation,
    write_summary,
};
use cleanrules::format::{
    ValueContext, read_node, read_ruleset, read_value, write_decision, write_path, write_result,
};
use cleanrules::format::{read_logic, write_any_result};
use cleanrules::json::{Json, Limits, parse, to_string};
use cleanrules::logic::{AnyLogic, Kleene};
use cleanrules::visual::{DiagramFormat, draw_document, draw_explanation};
use common::{get, on_small_stack, path, same, text, vector_files};

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
    if let Some(name) = &context.name {
        members.push(("name".into(), Json::String(name.clone())));
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

/// Members of an agent error that are free text or not compared
/// (`conformance/README.md`, *Comparing*).
const AGENT_ERROR_FREE: [&str; 6] = ["message", "hint", "cause", "node", "operation", "reason"];

/// An agent error without its free text, after checking that it has a hint
/// (`spec/agents.md`, *Errors*).
fn agent_error_compared(error: &Json) -> Result<Json, String> {
    match get(error, "hint") {
        Some(Json::String(hint)) if !hint.is_empty() => {}
        _ => return Err(format!("the agent error {} has no hint", to_string(error))),
    }
    let Json::Object(members) = error else {
        return Err(format!(
            "the agent error {} is not an object",
            to_string(error)
        ));
    };
    Ok(Json::Object(
        members
            .iter()
            .filter(|(k, _)| !AGENT_ERROR_FREE.contains(&k.as_str()))
            .cloned()
            .collect(),
    ))
}

/// A `tool` vector (`spec/agents.md`): a call on a catalog of the given rule
/// sets, compared with `output` or with `error`.
fn run_tool(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let expect = get(vector, "expect").unwrap();
    let mut catalog = Catalog::new();
    if let Some(Json::Object(rulesets)) = get(given, "rulesets") {
        for (name, content) in rulesets {
            let pointer = format!("/rulesets/{}", cleanrules::json::pointer_token(name));
            let ruleset = read_ruleset(content, &pointer, &Limits::default())
                .map_err(|e| format!("reading the rule set {name}: {e}"))?;
            catalog.insert(name.clone(), ruleset);
        }
    }
    let tool = text(get(given, "tool")).unwrap();
    let result = match get(given, "arguments_text") {
        Some(Json::String(arguments)) => catalog.call_text(tool, arguments),
        _ => catalog.call(tool, get(given, "arguments").unwrap()),
    };
    match (result.is_error, get(expect, "error")) {
        (true, Some(wanted)) => {
            let actual = agent_error_compared(get(&result.content, "error").unwrap())?;
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
        (false, None) => {
            let mut actual = result.content.clone();
            // A `validate` result's error is compared without its free text.
            if let Json::Object(members) = &mut actual {
                for (key, value) in members.iter_mut() {
                    if key == "error" {
                        *value = agent_error_compared(value)?;
                    }
                }
            }
            let wanted = get(expect, "output").unwrap();
            if same(&actual, wanted) {
                Ok(())
            } else {
                Err(format!(
                    "got {}\nwant {}",
                    to_string(&actual),
                    to_string(wanted)
                ))
            }
        }
        (true, None) => Err(format!(
            "unexpected tool error {}",
            to_string(&result.content)
        )),
        (false, Some(wanted)) => Err(format!(
            "got {}, expected the error {}",
            to_string(&result.content),
            to_string(wanted)
        )),
    }
}

/// A diagram (`spec/visual.md`): of the document, or, with an input, of the
/// explanation of a rule set's decision or of a table's grid with its statuses.
fn run_draw(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let mut limits = Limits::default();
    if let Some(Json::Number(n)) = path(given, &["limits", "max_diagram_size"]) {
        limits.max_diagram_size = usize::try_from(n.as_i128().unwrap()).unwrap();
    }
    let format = DiagramFormat::from_name(text(get(given, "format")).unwrap()).unwrap();
    let document = read_parsed_document(get(given, "document").unwrap(), &Limits::default())
        .map_err(|e| format!("reading the document: {e}"))?;
    let drawn = match (get(given, "input"), &document) {
        (None, document) => draw_document(document, None, format, &limits),
        (Some(input), Document::RuleSet(ruleset)) => {
            let input = read_value(input, "/input", ValueContext::Data)
                .map_err(|e| format!("reading the input: {e}"))?;
            let decision = ruleset.evaluate(&input).map_err(|e| e.to_string())?;
            draw_explanation(&explain(&decision), format, &limits)
        }
        (Some(input), Document::DecisionTable(table)) => {
            let input = read_value(input, "/input", ValueContext::Data)
                .map_err(|e| format!("reading the input: {e}"))?;
            let decision = table
                .ruleset()
                .evaluate(&input)
                .map_err(|e| e.to_string())?;
            draw_document(&document, Some(&decision), format, &limits)
        }
        (Some(_), _) => return Err("an input is given with a rule set or a table only".into()),
    };
    let expect = get(vector, "expect").unwrap();
    match (drawn, get(expect, "error")) {
        (Ok(diagram), None) => {
            let wanted = text(get(expect, "diagram")).unwrap();
            if diagram == wanted {
                Ok(())
            } else {
                Err(format!("got\n{diagram}\nwant\n{wanted}"))
            }
        }
        (Ok(diagram), Some(error)) => Err(format!(
            "got\n{diagram}\nexpected the error {}",
            to_string(error)
        )),
        (Err(error), wanted) => compare(
            Err(error),
            &Json::Object(
                wanted
                    .map(|w| vec![("error".to_owned(), w.clone())])
                    .unwrap_or_default(),
            ),
            "diagram",
        ),
    }
}

/// A `canonical` vector: the document's canonical bytes, its digest and, for
/// the documents that give a rule set, its rule set digest
/// (`spec/json-format.md`, *Canonical bytes*, *Digests*).
fn run_canonical(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let expect = get(vector, "expect").unwrap();
    let source = text(get(given, "text"))
        .map_or_else(|| to_string(get(given, "document").unwrap()), str::to_owned);
    let document = read_document(&source, &Limits::default())
        .map_err(|e| format!("reading the document: {e}"))?;
    let mut problems = Vec::new();
    let bytes = String::from_utf8(document.canonical_bytes()).unwrap();
    if Some(bytes.as_str()) != text(get(expect, "canonical")) {
        problems.push(format!("canonical bytes {bytes}"));
    }
    let digest = document.digest().to_string();
    if Some(digest.as_str()) != text(get(expect, "digest")) {
        problems.push(format!("digest {digest}"));
    }
    let ruleset_digest = document.into_ruleset().ok().map(|r| r.digest().to_string());
    if ruleset_digest.as_deref() != text(get(expect, "ruleset_digest")) {
        problems.push(format!("rule set digest {ruleset_digest:?}"));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

fn run(vector: &Json) -> Result<(), String> {
    let given = get(vector, "given").unwrap();
    let kind = text(get(given, "kind")).unwrap();
    if kind == "canonical" {
        return run_canonical(vector);
    }
    if kind == "draw" {
        return run_draw(vector);
    }
    if kind == "evaluate" {
        return run_evaluate(vector);
    }
    if kind == "explain" || kind == "summary" {
        return run_explain(vector);
    }
    if kind == "tool" {
        return run_tool(vector);
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
                    "condition"
                        | "value"
                        | "evaluate"
                        | "explain"
                        | "summary"
                        | "tool"
                        | "draw"
                        | "canonical"
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
