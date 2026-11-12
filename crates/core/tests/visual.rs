//! Diagrams (`spec/visual.md`): snapshots of every kind in every format, the
//! escaping of hostile text, limits and deep conditions.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use cleanrules::ast::{CompareOp, Node};
use cleanrules::explain::{explain, summary};
use cleanrules::format::{Document, read_document};
use cleanrules::json::Limits;
use cleanrules::rules::RuleSet;
use cleanrules::table::DecisionTable;
use cleanrules::visual::{
    DiagramFormat, draw_condition, draw_document, draw_explanation, draw_rule, draw_ruleset,
    draw_table,
};
use cleanrules::{ErrorCode, Map, Segment, Value};
use common::on_small_stack;

const FORMATS: [DiagramFormat; 4] = [
    DiagramFormat::Dot,
    DiagramFormat::Mermaid,
    DiagramFormat::Html,
    DiagramFormat::HtmlFragment,
];

fn limits() -> Limits {
    Limits::default()
}

fn ruleset(text: &str) -> RuleSet {
    cleanrules::text::parse(text, &limits())
        .unwrap()
        .into_ruleset()
        .unwrap()
}

fn input(pairs: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(*key, value.clone());
    }
    Value::Map(map)
}

/// The spec's explanation example (`spec/explain.md`, *Text*).
const PRIORITY: &str = "HIT POLICY priority
RULE a IF x > 10 AND NOT y <= 3
RULE b IF x BETWEEN 0 AND 5
RULE c IF x > 1 AND NOT x > 1
RULE d PRIORITY 1 IF x > 1
RULE e PRIORITY 5 IF x > 0";

#[test]
fn a_condition_in_every_format() {
    let node = cleanrules::text::parse_node(
        "(age >= 18 OR guardian IS NOT NULL) AND country IN ('ES', 'PT') AND NOT name STARTS WITH 'test'",
        &limits(),
    )
    .unwrap();
    for format in FORMATS {
        insta::assert_snapshot!(
            format!("condition_{}", format.as_str()),
            draw_condition(&node, format, &limits()).unwrap()
        );
    }
}

#[test]
fn a_rule_with_outputs_and_an_action() {
    let rule = cleanrules::text::parse_rule(
        "RULE vip PRIORITY 10 DESCRIPTION \"Loyal customers\\nwith large orders\"
         IF customer.years > 5 AND order.total > 1000
         THEN discount = 0.15, label = 'gold'
         DO notify",
        &limits(),
    )
    .unwrap();
    insta::assert_snapshot!(
        "rule_mermaid",
        draw_rule(&rule, DiagramFormat::Mermaid, &limits()).unwrap()
    );
    insta::assert_snapshot!(
        "rule_dot",
        draw_rule(&rule, DiagramFormat::Dot, &limits()).unwrap()
    );
    insta::assert_snapshot!(
        "rule_html_fragment",
        draw_rule(&rule, DiagramFormat::HtmlFragment, &limits()).unwrap()
    );
}

#[test]
fn a_rule_set_ranks_its_rules_by_the_policy() {
    let rules = ruleset(
        "HIT POLICY priority
         RULE loyal PRIORITY 5 IF customer.years > 2 THEN discount = 0.05 DO notify
         RULE vip PRIORITY 10 DESCRIPTION \"Best customers\" IF customer.years > 5 AND since IS NULL THEN discount = 0.15, label = customer.name
         RULE rest IF TRUE THEN discount = 0",
    );
    // The last rule's action is code of the program.
    let overview = summary(&rules).with_actions([false, false, true]);
    insta::assert_snapshot!(
        "ruleset_mermaid",
        draw_ruleset(&overview, DiagramFormat::Mermaid, &limits()).unwrap()
    );
    insta::assert_snapshot!(
        "ruleset_dot",
        draw_ruleset(&overview, DiagramFormat::Dot, &limits()).unwrap()
    );
    insta::assert_snapshot!(
        "ruleset_html_fragment",
        draw_ruleset(&overview, DiagramFormat::HtmlFragment, &limits()).unwrap()
    );
    // Under any other policy, rule order.
    let first = ruleset(
        "HIT POLICY collect AGGREGATE sum\nRULE b PRIORITY 9 IF x > 1 THEN n = 1\nRULE a IF x > 2 THEN n = 2",
    );
    let drawn = draw_ruleset(&summary(&first), DiagramFormat::Mermaid, &limits()).unwrap();
    assert!(
        drawn.starts_with("---\ntitle: \"HIT POLICY collect AGGREGATE sum\"\n---\nflowchart LR\n")
    );
    assert!(
        drawn.contains("n1(\"1. b<br>x #gt; 1\")\n  n2(\"2. a<br>x #gt; 2\")"),
        "{drawn}"
    );
}

#[test]
fn an_explanation_marks_truth_values_and_counterfactuals() {
    let rules = ruleset(PRIORITY);
    let decision = rules
        .evaluate(&input(&[("x", Value::Int(8)), ("y", Value::Int(2))]))
        .unwrap();
    let explanation = explain(&decision);
    for format in FORMATS {
        insta::assert_snapshot!(
            format!("explanation_{}", format.as_str()),
            draw_explanation(&explanation, format, &limits()).unwrap()
        );
    }
}

#[test]
fn unknown_causes_give_their_reason() {
    let rules = ruleset("RULE adult IF age >= 18 OR guardian = TRUE\nRULE named IF name = 'Ana'");
    let decision = rules.evaluate(&input(&[("name", Value::Int(3))])).unwrap();
    insta::assert_snapshot!(
        "explanation_unknown",
        draw_explanation(&explain(&decision), DiagramFormat::Mermaid, &limits()).unwrap()
    );
}

fn risk_table() -> DecisionTable {
    DecisionTable::from_csv(
        "HIT POLICY,priority
RULE,PRIORITY,customer.years,order.total,THEN discount,DESCRIPTION
vip,10,> 5,> 1000,0.15,\"Best customers,
with large orders\"
,,-,> 5000,0.1,
small,,\"IN (1, 2)\",-,0,
",
        &limits(),
    )
    .unwrap()
}

#[test]
fn a_decision_table_is_a_grid() {
    let table = risk_table();
    insta::assert_snapshot!(
        "table_html",
        draw_table(&table, None, DiagramFormat::Html, &limits()).unwrap()
    );
    let data = input(&[
        ("customer", input(&[("years", Value::Int(7))])),
        ("order", input(&[("total", Value::Int(6000))])),
    ]);
    let decision = table.ruleset().evaluate(&data).unwrap();
    insta::assert_snapshot!(
        "table_decision_html_fragment",
        draw_table(
            &table,
            Some(&decision),
            DiagramFormat::HtmlFragment,
            &limits()
        )
        .unwrap()
    );
}

#[test]
fn a_decision_table_is_drawn_in_html_only() {
    let table = risk_table();
    for format in [DiagramFormat::Dot, DiagramFormat::Mermaid] {
        let error = draw_table(&table, None, format, &limits()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(error.message().contains(format.as_str()), "{error}");
        let document = Document::DecisionTable(table.clone());
        assert_eq!(
            draw_document(&document, None, format, &limits())
                .unwrap_err()
                .code(),
            ErrorCode::Unsupported
        );
    }
}

#[test]
fn a_decision_of_another_rule_set_is_a_mismatch() {
    let table = risk_table();
    for (other, named) in [
        ("RULE vip IF TRUE", r#"no rule "row_2", the rule of row 2"#),
        (
            "RULE vip IF TRUE\nRULE row_2 IF TRUE\nRULE big IF TRUE",
            r#"no rule "small", the rule of row 3"#,
        ),
        (
            "RULE vip IF TRUE\nRULE row_2 IF TRUE\nRULE small IF TRUE\nRULE big IF TRUE",
            "it has 4 rules, and the table 3 rows",
        ),
    ] {
        let other = ruleset(other);
        let decision = other.evaluate(&input(&[])).unwrap();
        let error =
            draw_table(&table, Some(&decision), DiagramFormat::Html, &limits()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::DecisionMismatch, "{error}");
        assert!(error.message().ends_with(named), "{error}");
    }
}

#[test]
fn a_decision_goes_with_a_table_only() {
    let rules = ruleset("RULE a IF x > 1");
    let decision = rules.evaluate(&input(&[])).unwrap();
    let error = draw_document(
        &Document::RuleSet(rules.clone()),
        Some(&decision),
        DiagramFormat::Mermaid,
        &limits(),
    )
    .unwrap_err();
    assert_eq!(error.code(), ErrorCode::Unsupported);
    let table = Document::DecisionTable(risk_table());
    let Document::DecisionTable(t) = &table else {
        unreachable!()
    };
    let own = t.ruleset().evaluate(&input(&[])).unwrap();
    assert!(
        draw_document(&table, Some(&own), DiagramFormat::Html, &limits())
            .unwrap()
            .contains("<th>STATUS</th>")
    );
}

#[test]
fn documents_are_drawn_by_their_content() {
    let text = r#"{"version": 1, "condition": {"op": "gt", "left": {"op": "field", "path": ["x"]}, "right": {"op": "const", "value": 1}}}"#;
    let document = read_document(text, &limits()).unwrap();
    assert_eq!(
        draw_document(&document, None, DiagramFormat::Mermaid, &limits()).unwrap(),
        "flowchart TD\n  n0[\"x #gt; 1\"]\n  classDef leaf fill:#f8f9fa,stroke:#5f6368\n  class n0 leaf\n"
    );
    let rules = ruleset("RULE a IF x > 1");
    let rule = Document::Rule(rules.rules()[0].clone());
    assert!(
        draw_document(&rule, None, DiagramFormat::Dot, &limits())
            .unwrap()
            .contains("n0 [label=\"RULE a\"")
    );
    let set = Document::RuleSet(rules);
    assert!(
        draw_document(&set, None, DiagramFormat::Html, &limits())
            .unwrap()
            .contains("<title>cleanrules: rule set</title>")
    );
    let table = Document::DecisionTable(risk_table());
    assert!(
        draw_document(&table, None, DiagramFormat::Html, &limits())
            .unwrap()
            .contains("<caption>HIT POLICY priority</caption>")
    );
}

/// A rule set whose names, keys, strings, data and description hold every
/// character that could end a label, start markup or break a line.
fn hostile() -> (RuleSet, Value) {
    let text = r#"{"version": 1, "ruleset": {"hit_policy": "first", "rules": [
      {"name": "a\"b<i>&amp;#quot;`x`", "description": "<script>alert('x')</script>\nline \"2\" & é😀\u0000",
       "when": {"op": "and", "operands": [
         {"op": "eq", "left": {"op": "field", "path": ["we\"ird<key>", "k\ny"]},
                      "right": {"op": "const", "value": "</script><b>x</b>\n--> n9;%% #35; `md`"}},
         {"op": "not", "operand": {"op": "starts_with", "operand": {"op": "field", "path": ["é😀"]}, "text": "\\N\\l{|}"}}]},
       "then": {"label\"<>": {"op": "const", "value": "&lt;"}}}]}}"#;
    let Document::RuleSet(rules) = read_document(text, &limits()).unwrap() else {
        panic!("not a rule set")
    };
    let mut inner = Map::new();
    inner.insert(
        "k\ny",
        Value::String("<img src=x onerror=alert(1)>\"\n".into()),
    );
    let mut data = Map::new();
    data.insert("we\"ird<key>", Value::Map(inner));
    data.insert("é😀", Value::String("\\N".into()));
    (rules, Value::Map(data))
}

#[test]
fn hostile_text_is_escaped_in_every_format() {
    let (rules, data) = hostile();
    let decision = rules.evaluate(&data).unwrap();
    let explanation = explain(&decision);
    for format in FORMATS {
        let drawn = [
            draw_ruleset(&summary(&rules), format, &limits()).unwrap(),
            draw_rule(&rules.rules()[0], format, &limits()).unwrap(),
            draw_explanation(&explanation, format, &limits()).unwrap(),
        ];
        for text in &drawn {
            match format {
                DiagramFormat::Dot => {
                    for line in text.lines().filter(|l| l.contains("[label=")) {
                        // Every quote left after removing the escapes
                        // delimits one of the five attributes.
                        let bare = line.replace("\\\\", "").replace("\\\"", "");
                        assert_eq!(bare.matches('"').count(), 10, "{line}");
                    }
                }
                DiagramFormat::Mermaid => {
                    for line in text
                        .lines()
                        .filter(|l| l.starts_with("  n") && l.contains('"'))
                    {
                        assert_eq!(line.matches('"').count(), 2, "{line}");
                        assert!(
                            !line.contains('<')
                                || line.matches('<').count() == line.matches("<br>").count(),
                            "{line}"
                        );
                        assert!(
                            !line.contains('`') && !line.contains('&') && !line.contains('\\'),
                            "{line}"
                        );
                    }
                }
                _ => {
                    assert!(
                        !text.contains("<script")
                            && !text.contains("<img")
                            && !text.contains("<b>"),
                        "{text}"
                    );
                    assert!(!text.contains("<i>"), "{text}");
                }
            }
        }
        insta::assert_snapshot!(format!("hostile_{}", format.as_str()), drawn.join("\n"));
    }
}

#[test]
fn long_lines_are_cut() {
    let values: Vec<Value> = (0..60).map(Value::Int).collect();
    let node = Node::is_in(Node::field(vec![Segment::Key("x".into())]).unwrap(), values).unwrap();
    let drawn = draw_condition(&node, DiagramFormat::Mermaid, &limits()).unwrap();
    let label = drawn.lines().nth(1).unwrap();
    assert_eq!(label.chars().count(), "  n0[\"\"]".len() + 100, "{label}");
    assert!(label.ends_with("...\"]"), "{label}");
}

#[test]
fn diagrams_are_bounded_by_max_diagram_size() {
    let rules = ruleset(PRIORITY);
    let decision = rules
        .evaluate(&input(&[("x", Value::Int(8)), ("y", Value::Int(2))]))
        .unwrap();
    let explanation = explain(&decision);
    // 12 nodes and 7 edges.
    let mut limits = limits();
    limits.max_diagram_size = 19;
    for format in FORMATS {
        assert!(draw_explanation(&explanation, format, &limits).is_ok());
    }
    limits.max_diagram_size = 18;
    for format in FORMATS {
        let error = draw_explanation(&explanation, format, &limits).unwrap_err();
        assert_eq!(error.code(), ErrorCode::LimitExceeded);
        assert_eq!(error.context().name.as_deref(), Some("max_diagram_size"));
        assert!(error.message().contains("more than 18 elements"), "{error}");
    }
    // A grid counts its header and cells: 4 rows of 6 columns.
    let table = risk_table();
    limits.max_diagram_size = 24;
    assert!(draw_table(&table, None, DiagramFormat::Html, &limits).is_ok());
    limits.max_diagram_size = 23;
    let error = draw_table(&table, None, DiagramFormat::Html, &limits).unwrap_err();
    assert_eq!(error.code(), ErrorCode::LimitExceeded);
    limits.max_diagram_size = 0;
    let node = Node::field(vec![Segment::Key("x".into())]).unwrap();
    assert!(draw_condition(&node, DiagramFormat::Dot, &limits).is_err());
}

#[test]
fn deep_conditions_need_no_stack() {
    // As deep as the readers allow, on a binding's small stack.
    let depth = limits().max_depth;
    let mut node = Node::compare(
        CompareOp::Gt,
        Node::field(vec![Segment::Key("x".into())]).unwrap(),
        Node::constant(Value::Int(1)).unwrap(),
    )
    .unwrap();
    for _ in 1..depth {
        node = Node::not(node).unwrap();
    }
    on_small_stack(|| {
        for format in FORMATS {
            let drawn = draw_condition(&node, format, &limits()).unwrap();
            assert!(
                drawn.contains("x &gt; 1") || drawn.contains("x > 1") || drawn.contains("x #gt; 1")
            );
        }
    });
}

#[test]
fn drawing_is_deterministic() {
    let rules = ruleset(PRIORITY);
    let data = input(&[("x", Value::Int(8)), ("y", Value::Int(2))]);
    let first: Vec<String> = FORMATS
        .iter()
        .map(|&f| {
            draw_explanation(&explain(&rules.evaluate(&data).unwrap()), f, &limits()).unwrap()
        })
        .collect();
    for _ in 0..3 {
        let again: Vec<String> = FORMATS
            .iter()
            .map(|&f| {
                draw_explanation(&explain(&rules.evaluate(&data).unwrap()), f, &limits()).unwrap()
            })
            .collect();
        assert_eq!(again, first);
    }
}

/// The diagrams of `docs/examples/diagrams.md`, from `docs/examples/discounts.rules`.
fn docs_diagrams() -> (String, String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    let rules = ruleset(&std::fs::read_to_string(root.join("discounts.rules")).unwrap());
    let data = input(&[
        (
            "customer",
            input(&[
                ("blocked", Value::Bool(false)),
                ("years", Value::Int(4)),
                ("newsletter", Value::Bool(false)),
            ]),
        ),
        ("order", input(&[("total", Value::Int(1500))])),
    ]);
    let decision = rules.evaluate(&data).unwrap();
    (
        draw_ruleset(&summary(&rules), DiagramFormat::Mermaid, &limits()).unwrap(),
        draw_explanation(&explain(&decision), DiagramFormat::Mermaid, &limits()).unwrap(),
    )
}

#[test]
fn the_example_in_the_docs_is_current() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples/diagrams.md");
    let docs = std::fs::read_to_string(path).unwrap();
    let blocks: Vec<&str> = docs
        .split("```mermaid\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    let (ruleset, explanation) = docs_diagrams();
    assert_eq!(blocks, [ruleset.as_str(), explanation.as_str()]);
}

#[test]
fn titles_are_escaped() {
    // The rule `a<b>&"#\`, whose name the title quotes: "a<b>&\"#\\".
    let rules = ruleset(r##"RULE `a<b>&"#\` IF TRUE"##);
    let decision = rules.evaluate(&input(&[])).unwrap();
    let explanation = explain(&decision);
    let mermaid = draw_explanation(&explanation, DiagramFormat::Mermaid, &limits()).unwrap();
    let head = concat!(
        r##"---"##,
        "\n",
        r##"title: "HIT POLICY first: selected \"a<b>&\\\"#\\\\\"""##,
        "\n---\nflowchart TD\n",
        r##"  accTitle: HIT POLICY first: selected #quot;a#lt;b#gt;#amp;#92;#quot;#35;#92;#92;#quot;"##,
        "\n",
    );
    assert!(mermaid.starts_with(head), "{mermaid}");
    let dot = draw_explanation(&explanation, DiagramFormat::Dot, &limits()).unwrap();
    let label = r##"label="HIT POLICY first: selected \"a<b>&\\\"#\\\\\"", labelloc=t"##;
    assert!(dot.contains(label), "{dot}");
    let html = draw_explanation(&explanation, DiagramFormat::Html, &limits()).unwrap();
    let title =
        r#"<p class="title">HIT POLICY first: selected &quot;a&lt;b&gt;&amp;\&quot;#\\&quot;</p>"#;
    assert!(html.contains(title), "{html}");
}
