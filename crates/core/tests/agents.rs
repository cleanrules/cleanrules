//! The tools for agents (`spec/agents.md`), beyond what the conformance
//! vectors reach.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;

use cleanrules::agents::{
    Catalog, MAX_BUDGET, MAX_LINE, MIN_BUDGET, Tool, agent_error, compact_explanation, input_schema,
};
use cleanrules::explain::{explain, summary};
use cleanrules::json::{Json, JsonNumber, Limits, parse, to_string};
use cleanrules::rules::RuleSet;
use cleanrules::{Error, ErrorCode, Map, Value};
use common::get;
use proptest::prelude::*;

fn rules(text: &str) -> RuleSet {
    cleanrules::text::parse(text, &Limits::default())
        .unwrap()
        .into_ruleset()
        .unwrap()
}

fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    catalog.insert("adults", rules("RULE adult IF age >= 18 THEN ok = TRUE"));
    catalog
}

fn json(text: &str) -> Json {
    parse(text, &Limits::default()).unwrap()
}

fn error_of(result: &cleanrules::agents::ToolResult) -> &Json {
    assert!(result.is_error, "{}", to_string(&result.content));
    get(&result.content, "error").unwrap()
}

#[test]
fn tools_have_stable_names_sorted() {
    let names: Vec<&str> = Tool::ALL.iter().map(|t| t.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    for tool in Tool::ALL {
        assert_eq!(Tool::from_name(tool.as_str()), Some(*tool));
    }
    assert_eq!(Tool::from_name("Evaluate"), None);
}

#[test]
fn the_catalog_holds_rule_sets_by_name() {
    let mut catalog = catalog();
    assert!(catalog.get("adults").is_some());
    assert!(catalog.get("Adults").is_none());
    let replaced = catalog.insert("adults", Arc::new(rules("RULE x IF a THEN b = 1")));
    assert!(replaced.is_some());
    catalog.insert("Zeta", Arc::new(rules("RULE z IF a THEN b = 1")));
    // Sorted by code point: upper case first.
    assert_eq!(catalog.names().collect::<Vec<_>>(), ["Zeta", "adults"]);
}

#[test]
fn a_member_given_twice_is_an_error_at_it() {
    // The core's parser rejects repeated keys, but arguments built in code,
    // or by another parser, may hold them.
    let arguments = Json::Object(vec![
        ("ruleset".into(), Json::String("adults".into())),
        ("input".into(), Json::Object(Vec::new())),
        ("ruleset".into(), Json::String("adults".into())),
    ]);
    let result = catalog().call("evaluate", &arguments);
    let error = error_of(&result);
    assert_eq!(
        get(error, "code"),
        Some(&Json::String("format_error".into()))
    );
    assert_eq!(
        get(error, "pointer"),
        Some(&Json::String("/ruleset".into()))
    );
}

#[test]
fn a_key_needing_escapes_is_located_by_its_token() {
    let result = catalog().call_text("summary", r#"{"ruleset": "adults", "a/b~c": 1}"#);
    assert_eq!(
        get(error_of(&result), "pointer"),
        Some(&Json::String("/a~1b~0c".into()))
    );
}

#[test]
fn every_agent_error_has_a_hint() {
    for code in ErrorCode::ALL {
        let error = agent_error(&Error::new(code, "something failed"));
        match get(&error, "hint") {
            Some(Json::String(hint)) if !hint.is_empty() => {}
            other => panic!("{code}: no hint, {other:?}"),
        }
        // The hint follows the message, as in the JSON form.
        let Json::Object(members) = &error else {
            panic!()
        };
        let keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["code", "message", "hint"]);
    }
    // The core's own hint is kept.
    let error = agent_error(&Error::new(ErrorCode::FormatError, "bad").with_hint("mine"));
    assert_eq!(get(&error, "hint"), Some(&Json::String("mine".into())));
}

#[test]
fn an_evaluation_error_points_through_lists() {
    let mut catalog = Catalog::new();
    catalog.insert("s", Arc::new(rules("STRICT\nRULE r IF items[3].qty > 1")));
    // The list is too short: the pointer goes where the item would be.
    let result = catalog.call_text("evaluate", r#"{"ruleset": "s", "input": {"items": [1]}}"#);
    let error = error_of(&result);
    assert_eq!(
        get(error, "code"),
        Some(&Json::String("missing_field".into()))
    );
    assert_eq!(
        get(error, "pointer"),
        Some(&Json::String("/input/items/3/qty".into()))
    );
    // Through a scalar where a map was expected.
    let result = catalog.call_text("evaluate", r#"{"ruleset": "s", "input": 5}"#);
    assert_eq!(
        get(error_of(&result), "pointer"),
        Some(&Json::String("/input/items/3/qty".into()))
    );
}

/// The typed forms of `spec/schemas/v1/value.schema.json`, which input schemas
/// copy (`spec/agents.md`, *Building the schema*).
#[test]
fn typed_forms_are_those_of_the_value_schema() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/schemas/v1/value.schema.json"
    );
    let spec = json(&std::fs::read_to_string(path).unwrap());
    let defs = get(&spec, "$defs").unwrap();
    let ruleset = rules(
        "RULE r IF a = 1.5 AND b = DATE \"2026-01-01\" AND c = DATETIME \"2026-01-01T00:00:00\" \
         AND d = DATETIME \"2026-01-01T00:00:00Z\"",
    );
    let schema = input_schema(&summary(&ruleset));
    let properties = get(&schema, "properties").unwrap();
    for (field, def) in [
        ("a", "decimal"),
        ("b", "date"),
        ("c", "naiveDatetime"),
        ("d", "awareDatetime"),
    ] {
        let Some(Json::Array(alternatives)) = get(get(properties, field).unwrap(), "anyOf") else {
            panic!("{field}")
        };
        assert!(
            cleanrules::json::same(&alternatives[0], get(defs, def).unwrap()),
            "{field}: {} is not {def}",
            to_string(&alternatives[0])
        );
    }
}

#[test]
fn an_index_at_the_limit_has_prefix_items_and_one_past_it_none() {
    for (index, items) in [(255, true), (256, false)] {
        let ruleset = rules(&format!("RULE r IF x[{index}] > 1"));
        let schema = input_schema(&summary(&ruleset));
        let x = get(get(&schema, "properties").unwrap(), "x").unwrap();
        let Some(Json::Array(alternatives)) = get(x, "anyOf") else {
            panic!()
        };
        assert_eq!(
            get(&alternatives[0], "prefixItems").is_some(),
            items,
            "{index}"
        );
    }
}

fn levels() -> RuleSet {
    rules(
        "HIT POLICY first\n\
         RULE never IF x > 100 AND NOT x > 100 THEN level = \"never\"\n\
         RULE high IF x > 50 AND region = \"north\" THEN level = \"high\"\n\
         RULE missing IF score >= 700 THEN level = \"score\"\n\
         RULE mid IF x > 5 THEN level = \"mid\"\n\
         RULE low IF x > 0 THEN level = \"low\"",
    )
}

fn input(x: i64) -> Value {
    let mut map = Map::new();
    map.insert("x", Value::Int(x));
    map.insert("region", Value::String("south".into()));
    Value::Map(map)
}

#[test]
fn budgets_out_of_range_are_the_tools_error() {
    let ruleset = levels();
    let decision = ruleset.evaluate(&input(8)).unwrap();
    let explanation = explain(&decision);
    for budget in [0, MIN_BUDGET - 1, MAX_BUDGET + 1, usize::MAX] {
        let error = compact_explanation(&explanation, budget).unwrap_err();
        assert_eq!(error.code(), ErrorCode::FormatError);
        assert_eq!(error.context().pointer.as_deref(), Some("/budget"));
    }
    for budget in [MIN_BUDGET, MAX_BUDGET] {
        assert!(compact_explanation(&explanation, budget).is_ok());
    }
    let compact = compact_explanation(&explanation, MAX_BUDGET).unwrap();
    assert_eq!(compact.to_string(), compact.text);
}

#[test]
fn integral_budgets_are_budgets() {
    let mut catalog = Catalog::new();
    catalog.insert("levels", levels());
    for (budget, ok) in [
        ("256.0", true),
        ("2.56e2", true),
        ("256.5", false),
        ("1e300", false),
        ("-0.0", false),
    ] {
        let arguments = format!(r#"{{"ruleset": "levels", "input": {{}}, "budget": {budget}}}"#);
        assert_eq!(
            !catalog.call_text("explain", &arguments).is_error,
            ok,
            "{budget}"
        );
    }
}

#[test]
fn summaries_show_actions_given_as_code() {
    let mut catalog = Catalog::new();
    let ruleset = Arc::new(rules(
        "RULE a IF x > 1
RULE b IF x > 2 DO notify",
    ));
    catalog.insert_with_actions("coded", Arc::clone(&ruleset), [true]);
    catalog.insert("plain", ruleset);
    let actions = |name: &str| {
        let result = catalog.call_text("summary", &format!(r#"{{"ruleset": "{name}"}}"#));
        let Some(Json::Array(rules)) = get(get(&result.content, "summary").unwrap(), "rules")
        else {
            panic!()
        };
        rules
            .iter()
            .map(|r| get(r, "action").is_some())
            .collect::<Vec<_>>()
    };
    assert_eq!(actions("coded"), [true, true]);
    assert_eq!(actions("plain"), [false, true]);
}

#[test]
fn catalogs_are_collections() {
    let mut catalog: Catalog = [("b", rules("RULE r IF x")), ("a", rules("RULE r IF y"))]
        .into_iter()
        .collect();
    assert_eq!(catalog.len(), 2);
    catalog.extend([("c".to_owned(), Arc::new(rules("RULE r IF z")))]);
    assert_eq!(catalog.names().collect::<Vec<_>>(), ["a", "b", "c"]);
    assert!(catalog.remove("b").is_some());
    assert!(catalog.remove("b").is_none());
    assert!(!catalog.is_empty());
    assert!(Catalog::new().is_empty());
}

#[test]
fn a_catalog_offers_the_tools_it_is_given() {
    let catalog = catalog().with_tools([Tool::Summary, Tool::Evaluate, Tool::Summary]);
    assert_eq!(catalog.tools(), [Tool::Evaluate, Tool::Summary]);
    assert_eq!(Catalog::new().tools(), Tool::ALL);
    let result = catalog.call_text("explain", r#"{"ruleset": "adults", "input": {}}"#);
    let error = error_of(&result);
    assert_eq!(get(error, "name"), Some(&Json::String("explain".into())));
    let available = Json::Array(vec![
        Json::String("evaluate".into()),
        Json::String("summary".into()),
    ]);
    assert_eq!(get(error, "available"), Some(&available));
    assert!(
        !catalog
            .call_text("summary", r#"{"ruleset": "adults"}"#)
            .is_error
    );
}

proptest! {
    /// The text never exceeds the budget, its lines never exceed
    /// `MAX_LINE`, and it is complete exactly when it is the whole text.
    #[test]
    fn compact_explanations_fit_their_budget(x in -200_i64..200, budget in MIN_BUDGET..1200) {
        let ruleset = levels();
        let decision = ruleset.evaluate(&input(x)).unwrap();
        let explanation = explain(&decision);
        let compact = compact_explanation(&explanation, budget).unwrap();
        prop_assert!(compact.text.chars().count() <= budget);
        prop_assert!(compact.text.lines().all(|l| l.chars().count() <= MAX_LINE));
        prop_assert_eq!(compact.complete, compact.text == explanation.to_string());
        // The selected rule is always shown whole at these sizes.
        let first = explanation.to_string().lines().next().unwrap().to_owned();
        prop_assert!(compact.text.starts_with(&first));
    }
}

#[test]
fn the_budget_argument_reaches_the_explanation() {
    let mut catalog = Catalog::new();
    catalog.insert("levels", Arc::new(levels()));
    let arguments = |budget: u64| {
        Json::Object(vec![
            ("ruleset".into(), Json::String("levels".into())),
            ("input".into(), json(r#"{"x": 8, "region": "south"}"#)),
            (
                "budget".into(),
                Json::Number(JsonNumber::from_literal(budget.to_string())),
            ),
        ])
    };
    let size = |budget| {
        let result = catalog.call("explain", &arguments(budget));
        let Some(Json::String(text)) = get(get(&result.content, "explanation").unwrap(), "text")
        else {
            panic!()
        };
        text.chars().count()
    };
    assert!(size(256) <= 256);
    assert!(size(300) > size(256));
}
