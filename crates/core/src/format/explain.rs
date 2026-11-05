//! Explanations and summaries in JSON (`spec/json-format.md`, *Explanations*,
//! *Summaries*). Both are output only.

use crate::ast::Node;
use crate::error::Segment;
use crate::eval::{Evaluated, Reason};
use crate::explain::{Cause, Explanation, Gap, GroupKind, Requirement, Summary};
use crate::format::condition::write_node;
use crate::format::result::write_path;
use crate::format::value::write_value;
use crate::json::{Json, to_string};
use crate::logic::Truth;
use crate::rules::Decision;
use crate::value::{Map, NullCause, Value};

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

/// The JSON Pointer of every result of the decision's condition trees, by address.
fn pointers<'d>(decision: &'d Decision<'_>) -> Vec<(*const Evaluated<'d, Truth>, String)> {
    fn walk<'d>(
        result: &'d Evaluated<'_, Truth>,
        pointer: &str,
        out: &mut Vec<(*const Evaluated<'d, Truth>, String)>,
    ) {
        out.push((std::ptr::from_ref(result).cast(), pointer.to_owned()));
        for (i, child) in result.children.iter().enumerate() {
            walk(child, &format!("{pointer}/children/{i}"), out);
        }
    }
    let mut out = Vec::new();
    for (i, result) in decision.results().iter().enumerate() {
        walk(
            &result.when,
            &format!("/decision/results/{i}/when"),
            &mut out,
        );
    }
    out
}

/// Where an explanation is written: a JSON document, or a value (the C ABI's
/// `cr_decision_explain_value`), which keeps the values themselves (opaque
/// ones included) and adds the texts of each rule and cause and whether each
/// cause holds. Both have the structure of `spec/json-format.md`,
/// *Explanations*.
trait Out: Sized {
    /// Whether to add `text` to rules and causes and `holds` to causes.
    const EXTRAS: bool;
    fn text(text: &str) -> Self;
    fn boolean(value: bool) -> Self;
    fn null() -> Self;
    fn int(value: i64) -> Self;
    fn data(value: &Value) -> Self;
    /// A node: in JSON its form, in a value its JSON text.
    fn node(node: &Node) -> Self;
    fn path(path: &[Segment]) -> Self;
    fn list(items: Vec<Self>) -> Self;
    fn object(members: Vec<(&str, Self)>) -> Self;
}

impl Out for Json {
    const EXTRAS: bool = false;
    fn text(text: &str) -> Self {
        string(text)
    }
    fn boolean(value: bool) -> Self {
        Json::Bool(value)
    }
    fn null() -> Self {
        Json::Null
    }
    fn int(value: i64) -> Self {
        write_value(&Value::Int(value))
    }
    fn data(value: &Value) -> Self {
        write_value(value)
    }
    fn node(node: &Node) -> Self {
        write_node(node)
    }
    fn path(path: &[Segment]) -> Self {
        write_path(path)
    }
    fn list(items: Vec<Self>) -> Self {
        Json::Array(items)
    }
    fn object(members: Vec<(&str, Self)>) -> Self {
        Json::Object(
            members
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        )
    }
}

impl Out for Value {
    const EXTRAS: bool = true;
    fn text(text: &str) -> Self {
        Value::String(text.to_owned())
    }
    fn boolean(value: bool) -> Self {
        Value::Bool(value)
    }
    fn null() -> Self {
        Value::Null(NullCause::Null)
    }
    fn int(value: i64) -> Self {
        Value::Int(value)
    }
    fn data(value: &Value) -> Self {
        value.clone()
    }
    fn node(node: &Node) -> Self {
        Value::String(to_string(&write_node(node)))
    }
    fn path(path: &[Segment]) -> Self {
        Value::List(
            path.iter()
                .map(|segment| match segment {
                    Segment::Key(key) => Value::String(key.clone()),
                    // Indexes are below 2^63: they index lists in memory.
                    Segment::Index(index) => Value::Int(i64::try_from(*index).unwrap_or(i64::MAX)),
                })
                .collect(),
        )
    }
    fn list(items: Vec<Self>) -> Self {
        Value::List(items)
    }
    fn object(members: Vec<(&str, Self)>) -> Self {
        let mut map = Map::new();
        for (key, value) in members {
            map.insert(key, value);
        }
        Value::Map(map)
    }
}

fn truth<O: Out>(truth: Truth) -> O {
    match truth {
        Truth::True => O::boolean(true),
        Truth::False => O::boolean(false),
        Truth::Unknown => O::null(),
    }
}

/// A reason, as `write_reason` writes it.
fn reason<O: Out>(reason: &Reason) -> O {
    let mut members = vec![("code", O::text(reason.code.as_str()))];
    if !reason.paths.is_empty() {
        members.push((
            "paths",
            O::list(reason.paths.iter().map(|p| O::path(p)).collect()),
        ));
    }
    if !reason.types.is_empty() {
        members.push((
            "types",
            O::list(reason.types.iter().map(|g| O::text(g.as_str())).collect()),
        ));
    }
    if let Some(detail) = &reason.detail {
        members.push(("detail", O::text(detail)));
    }
    O::object(members)
}

fn write_cause<O: Out>(
    cause: &Cause<'_, '_>,
    pointers: &[(*const Evaluated<'_, Truth>, String)],
) -> O {
    let address: *const Evaluated<'_, Truth> = std::ptr::from_ref(cause.result).cast();
    let at = pointers
        .iter()
        .find(|(p, _)| std::ptr::eq(*p, address))
        .map_or("", |(_, s)| s.as_str());
    let values = cause
        .values
        .iter()
        .map(|(path, value)| O::object(vec![("path", O::path(path)), ("value", O::data(value))]))
        .collect();
    let mut members = vec![
        ("condition", O::node(&cause.result.node)),
        ("at", O::text(at)),
        ("truth", truth(cause.truth())),
        ("negated", O::boolean(cause.negated)),
        ("values", O::list(values)),
    ];
    if let Some(found) = cause.reason() {
        members.push(("reason", reason(found)));
    }
    if let Some(counterfactual) = &cause.counterfactual {
        let mut fields = vec![
            ("path", O::path(&counterfactual.path)),
            ("op", O::text(counterfactual.op.tag())),
            ("target", O::data(&counterfactual.target)),
        ];
        match &counterfactual.gap {
            Some(Gap::Number(value)) => fields.push(("gap", O::data(value))),
            Some(Gap::Days(days)) => {
                fields.push(("gap", O::object(vec![("days", O::int(*days))])));
            }
            Some(Gap::Microseconds(micros)) => {
                fields.push(("gap", O::object(vec![("microseconds", O::int(*micros))])));
            }
            None => {}
        }
        members.push(("counterfactual", O::object(fields)));
    }
    if O::EXTRAS {
        members.push(("holds", O::boolean(cause.holds())));
        members.push(("text", O::text(&cause.to_string())));
    }
    O::object(members)
}

fn write_requirement<O: Out>(
    requirement: &Requirement<'_, '_>,
    pointers: &[(*const Evaluated<'_, Truth>, String)],
) -> O {
    match requirement {
        Requirement::Cause(cause) => write_cause(cause, pointers),
        Requirement::Group(kind, items) => {
            let key = if *kind == GroupKind::All {
                "all"
            } else {
                "any"
            };
            O::object(vec![(
                key,
                O::list(
                    items
                        .iter()
                        .map(|i| write_requirement(i, pointers))
                        .collect(),
                ),
            )])
        }
    }
}

fn explanation<O: Out>(explanation: &Explanation<'_, '_>) -> O {
    let decision = explanation.decision;
    let ruleset = decision.ruleset();
    let pointers = pointers(decision);
    let rules = explanation
        .rules
        .iter()
        .map(|entry| {
            let mut members = vec![
                ("rule", O::text(entry.rule.name())),
                ("status", O::text(entry.status.as_str())),
            ];
            if let Some(note) = &entry.note {
                let mut fields = vec![
                    ("kind", O::text(note.kind.as_str())),
                    ("winner", O::text(&note.winner)),
                ];
                if let Some((mine, theirs)) = note.priorities {
                    fields.push(("priority", O::int(mine)));
                    fields.push(("winner_priority", O::int(theirs)));
                }
                members.push(("note", O::object(fields)));
            }
            if let Some(achievable) = entry.achievable {
                members.push(("achievable", O::boolean(achievable)));
            }
            members.push((
                "requirement",
                write_requirement(&entry.requirement, &pointers),
            ));
            if O::EXTRAS {
                members.push(("text", O::text(&entry.to_string())));
            }
            O::object(members)
        })
        .collect();
    O::object(vec![
        ("hit_policy", O::text(ruleset.hit_policy().as_str())),
        ("logic", O::text(ruleset.logic().as_str())),
        (
            "selected",
            O::list(
                decision
                    .selected()
                    .map(|r| O::text(r.rule.name()))
                    .collect(),
            ),
        ),
        ("rules", O::list(rules)),
    ])
}

/// Write an explanation's content (the object under `"explanation"`).
#[must_use]
pub fn write_explanation(explanation: &Explanation<'_, '_>) -> Json {
    self::explanation(explanation)
}

/// An explanation's content as a value: the structure of
/// [`write_explanation`], with the values themselves (opaque ones keep their
/// token), a node as its JSON text, and, added, the `text` of each rule and
/// cause (their parts of the explanation's text) and whether each cause
/// `holds`. For bindings that build objects of it (the C ABI's
/// `cr_decision_explain_value`).
#[must_use]
pub fn explanation_value(explanation: &Explanation<'_, '_>) -> Value {
    self::explanation(explanation)
}

/// Write a summary's content (the object under `"summary"`).
#[must_use]
pub fn write_summary(summary: &Summary<'_>) -> Json {
    let ruleset = summary.ruleset;
    let mut members = vec![(
        "hit_policy".to_owned(),
        string(ruleset.hit_policy().as_str()),
    )];
    if let Some(aggregate) = ruleset.aggregate() {
        members.push(("aggregate".into(), string(aggregate.as_str())));
    }
    members.push(("logic".into(), string(ruleset.logic().as_str())));
    if ruleset.strict() {
        members.push(("strict".into(), Json::Bool(true)));
    }
    let rules = ruleset
        .rules()
        .iter()
        .enumerate()
        .map(|(i, rule)| {
            let mut fields = vec![
                ("name".to_owned(), string(rule.name())),
                (
                    "priority".to_owned(),
                    write_value(&Value::Int(rule.get_priority())),
                ),
                (
                    "outputs".to_owned(),
                    Json::Array(rule.then().iter().map(|(k, _)| string(k)).collect()),
                ),
            ];
            if summary.actions.get(i).copied().unwrap_or(false) {
                fields.push(("action".into(), Json::Bool(true)));
            }
            if let Some(action) = rule.get_action_name() {
                fields.push(("do".into(), string(action)));
            }
            Json::Object(fields)
        })
        .collect();
    members.push(("rules".into(), Json::Array(rules)));
    let fields = summary
        .fields
        .iter()
        .map(|usage| {
            Json::Object(vec![
                ("path".into(), write_path(&usage.path)),
                (
                    "conditions".into(),
                    Json::Array(usage.conditions.iter().map(|n| string(n)).collect()),
                ),
                (
                    "payloads".into(),
                    Json::Array(
                        usage
                            .payloads
                            .iter()
                            .map(|(rule, key)| {
                                Json::Object(vec![
                                    ("rule".into(), string(rule)),
                                    ("key".into(), string(key)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ])
        })
        .collect();
    members.push(("fields".into(), Json::Array(fields)));
    members.push((
        "outputs".into(),
        Json::Array(summary.outputs.iter().map(|k| string(k)).collect()),
    ));
    let tags = summary
        .tags
        .iter()
        .map(|(tag, names)| {
            Json::Object(vec![
                ("tag".into(), string(tag)),
                (
                    "rules".into(),
                    Json::Array(names.iter().map(|n| string(n)).collect()),
                ),
            ])
        })
        .collect();
    members.push(("tags".into(), Json::Array(tags)));
    Json::Object(members)
}

#[cfg(test)]
mod tests {
    use super::{explanation_value, write_explanation};
    use crate::explain::explain;
    use crate::format::Document;
    use crate::json::{Json, Limits};
    use crate::value::{Map, Value};

    /// The value has the document's structure, with the values themselves
    /// and the added `text` and `holds`.
    #[test]
    fn the_value_has_the_document_structure_and_the_texts() {
        let text = "HIT POLICY priority\n\
                    RULE big PRIORITY 1 IF x > 10 AND ok\n\
                    RULE any PRIORITY 2 IF TRUE\n\
                    RULE low IF TRUE\n\
                    RULE d IF day < DATE \"2026-01-01\"";
        let Document::RuleSet(rules) = crate::text::parse(text, &Limits::default()).unwrap() else {
            panic!("not a rule set")
        };
        let mut input = Map::new();
        input.insert("x", Value::Int(8));
        input.insert("day", Value::Date(jiff::civil::date(2026, 1, 3)));
        let decision = rules.evaluate(&Value::Map(input)).unwrap();
        let explanation = explain(&decision);
        let json = write_explanation(&explanation);
        let Value::Map(value) = explanation_value(&explanation) else {
            panic!("not a map")
        };
        let Json::Object(members) = &json else {
            panic!("not an object")
        };
        let keys: Vec<&str> = value.iter().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            members.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>()
        );
        let Some(Value::List(entries)) = value.get("rules") else {
            panic!("no rules")
        };
        let Value::Map(big) = &entries[0] else {
            panic!("not a map")
        };
        let rule_text = explanation.rules[0].to_string();
        assert!(matches!(big.get("text"), Some(Value::String(t)) if *t == rule_text));
        let Some(Value::Map(requirement)) = big.get("requirement") else {
            panic!("no requirement")
        };
        let Some(Value::List(items)) = requirement.get("all") else {
            panic!("not a group")
        };
        let Value::Map(first) = &items[0] else {
            panic!("not a cause")
        };
        assert!(matches!(first.get("holds"), Some(Value::Bool(false))));
        assert!(
            matches!(first.get("text"), Some(Value::String(t)) if t == "x > 10 is false (x = 8); short by 2")
        );
        assert!(
            matches!(first.get("condition"), Some(Value::String(t)) if t.starts_with("{\"op\":\"gt\""))
        );
        let Value::Map(low) = &entries[2] else {
            panic!("not a map")
        };
        let Some(Value::Map(note)) = low.get("note") else {
            panic!("no note")
        };
        assert!(matches!(note.get("winner_priority"), Some(Value::Int(2))));
        let Value::Map(dated) = &entries[3] else {
            panic!("not a map")
        };
        let Some(Value::Map(cause)) = dated.get("requirement") else {
            panic!("no cause")
        };
        let Some(Value::Map(counterfactual)) = cause.get("counterfactual") else {
            panic!("no counterfactual")
        };
        let Some(Value::Map(gap)) = counterfactual.get("gap") else {
            panic!("no gap")
        };
        assert!(matches!(gap.get("days"), Some(Value::Int(2))));
        assert!(matches!(cause.get("values"), Some(Value::List(v)) if !v.is_empty()));
        let Value::Map(second) = &items[1] else {
            panic!("not a cause")
        };
        assert!(matches!(second.get("truth"), Some(Value::Null(_))));
        assert!(matches!(second.get("reason"), Some(Value::Map(r)) if r.get("paths").is_some()));
    }
}
