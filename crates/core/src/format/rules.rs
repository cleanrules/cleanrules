//! Rules, rule sets and decisions in JSON (`spec/json-format.md`, *Rules*, *Rule
//! sets*, *Decisions*).

use crate::ast::Kind;
use crate::error::{Error, ErrorCode, Result};
use crate::format::condition::{Raw, build, shape, write_node};
use crate::format::result::write_result;
use crate::format::value::{ValueContext, read_value, write_value};
use crate::json::{Json, Limits, pointer_token};
use crate::logic::LogicName;
use crate::rules::{Aggregate, Aggregated, Config, Decision, HitPolicy, Rule, RuleSet};
use crate::value::{Map, Value};

pub(crate) fn format_error(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

pub(crate) fn at(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", pointer_token(key))
}

pub(crate) fn get<'a>(members: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// After the known keys: an unknown key is an error, in text order.
pub(crate) fn no_unknown_keys(
    members: &[(String, Json)],
    known: &[&str],
    pointer: &str,
) -> Result<()> {
    match members.iter().find(|(k, _)| !known.contains(&k.as_str())) {
        Some((key, _)) => Err(
            format_error(&at(pointer, key), format!("unknown key {key:?}"))
                .with_hint("it may come from a newer version of cleanrules"),
        ),
        None => Ok(()),
    }
}

/// A rule whose shape is valid, not yet built.
pub(crate) struct RawRule {
    pointer: String,
    name: String,
    when: Raw,
    then: Vec<(String, Raw)>,
    priority: i64,
    tags: Vec<String>,
    description: Option<String>,
    metadata: Map,
    action: Option<String>,
}

const RULE_KEYS: [&str; 8] = [
    "name",
    "when",
    "then",
    "priority",
    "tags",
    "description",
    "metadata",
    "do",
];

/// Check a rule's shape: keys in table order, then unknown keys.
// One block per key of the rule table reads best as a single function.
#[allow(clippy::too_many_lines)]
pub(crate) fn shape_rule(json: &Json, pointer: &str, limits: &Limits) -> Result<RawRule> {
    let Json::Object(members) = json else {
        return Err(format_error(pointer, "a rule must be an object"));
    };
    let name = match get(members, "name") {
        Some(Json::String(name)) => name.clone(),
        Some(_) => {
            return Err(format_error(
                &at(pointer, "name"),
                "\"name\" must be a string",
            ));
        }
        None => {
            return Err(format_error(
                &at(pointer, "name"),
                "the key \"name\" is missing",
            ));
        }
    };
    let Some(when) = get(members, "when") else {
        return Err(format_error(
            &at(pointer, "when"),
            "the key \"when\" is missing",
        ));
    };
    let when = shape(when, &at(pointer, "when"), 1, limits)?;
    let mut then = Vec::new();
    match get(members, "then") {
        None => {}
        Some(Json::Object(entries)) => {
            let base = at(pointer, "then");
            for (key, value) in entries {
                then.push((key.clone(), shape(value, &at(&base, key), 1, limits)?));
            }
        }
        Some(_) => {
            return Err(format_error(
                &at(pointer, "then"),
                "\"then\" must be an object of nodes",
            ));
        }
    }
    let priority = match get(members, "priority") {
        None => 0,
        Some(json) => match read_value(json, &at(pointer, "priority"), ValueContext::Constant)? {
            Value::Int(i) => i,
            _ => {
                return Err(format_error(
                    &at(pointer, "priority"),
                    "\"priority\" must be an int",
                ));
            }
        },
    };
    let tags = match get(members, "tags") {
        None => Vec::new(),
        Some(Json::Array(items)) => {
            let mut tags = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let Json::String(tag) = item else {
                    return Err(format_error(
                        &format!("{}/{i}", at(pointer, "tags")),
                        "a tag must be a string",
                    ));
                };
                tags.push(tag.clone());
            }
            tags
        }
        Some(_) => {
            return Err(format_error(
                &at(pointer, "tags"),
                "\"tags\" must be an array of strings",
            ));
        }
    };
    let description = match get(members, "description") {
        None => None,
        Some(Json::String(text)) => Some(text.clone()),
        Some(_) => {
            return Err(format_error(
                &at(pointer, "description"),
                "\"description\" must be a string",
            ));
        }
    };
    let metadata = match get(members, "metadata") {
        None => Map::new(),
        Some(Json::Object(entries)) => {
            let base = at(pointer, "metadata");
            let mut map = Map::new();
            // Metadata keys are taken literally; their values are data values.
            for (key, value) in entries {
                map.insert(
                    key.clone(),
                    read_value(value, &at(&base, key), ValueContext::Data)?,
                );
            }
            map
        }
        Some(_) => {
            return Err(format_error(
                &at(pointer, "metadata"),
                "\"metadata\" must be an object",
            ));
        }
    };
    let action = match get(members, "do") {
        None => None,
        Some(Json::String(name)) => Some(name.clone()),
        Some(_) => {
            return Err(format_error(
                &at(pointer, "do"),
                "\"do\" must be a string: the name of an action",
            ));
        }
    };
    no_unknown_keys(members, &RULE_KEYS, pointer)?;
    Ok(RawRule {
        pointer: pointer.to_owned(),
        name,
        when,
        then,
        priority,
        tags,
        description,
        metadata,
        action,
    })
}

/// Build a rule: its nodes first, then its own rules, each error located.
pub(crate) fn build_rule(raw: RawRule) -> Result<Rule> {
    let pointer = raw.pointer;
    let when = build(raw.when)?;
    let mut then = Vec::with_capacity(raw.then.len());
    for (key, value) in raw.then {
        then.push((key, build(value)?));
    }
    if when.is_null_constant() {
        return Err(Error::new(
            ErrorCode::ExpressionError,
            "a null constant cannot be a rule's condition",
        )
        .at_pointer(at(&pointer, "when")));
    }
    if raw.name.is_empty() {
        return Err(Error::new(
            ErrorCode::RuleDefinitionError,
            "a rule name cannot be empty",
        )
        .at_pointer(at(&pointer, "name")));
    }
    if when.kind() == Kind::Value {
        return Err(Error::new(
            ErrorCode::RuleDefinitionError,
            format!(
                "the condition of {:?} is a value, not a condition",
                raw.name
            ),
        )
        .at_pointer(at(&pointer, "when")));
    }
    if let Some((key, _)) = then.iter().find(|(_, node)| node.kind() == Kind::Condition) {
        return Err(Error::new(
            ErrorCode::RuleDefinitionError,
            format!("the payload value {key:?} is a condition"),
        )
        .at_pointer(at(&at(&pointer, "then"), key)));
    }
    let mut rule = Rule::with_payload(raw.name, when, then)?
        .priority(raw.priority)
        .tags(raw.tags)
        .metadata(raw.metadata);
    if let Some(description) = raw.description {
        rule = rule.description(description);
    }
    match raw.action {
        Some(action) => rule
            .action_name(action)
            .map_err(|e| e.at_pointer(at(&pointer, "do"))),
        None => Ok(rule),
    }
}

/// A rule set whose shape is valid, not yet built.
pub(crate) struct RawRuleSet {
    pointer: String,
    hit_policy: HitPolicy,
    aggregate: Option<Aggregate>,
    logic: String,
    strict: bool,
    rules: Vec<RawRule>,
}

const RULESET_KEYS: [&str; 5] = ["hit_policy", "aggregate", "logic", "strict", "rules"];

/// Check a rule set's shape.
pub(crate) fn shape_ruleset(json: &Json, pointer: &str, limits: &Limits) -> Result<RawRuleSet> {
    let Json::Object(members) = json else {
        return Err(format_error(pointer, "a rule set must be an object"));
    };
    let name = |key: &str, json: &Json| -> Result<String> {
        match json {
            Json::String(s) => Ok(s.clone()),
            _ => Err(format_error(
                &at(pointer, key),
                format!("{key:?} must be a string"),
            )),
        }
    };
    let hit_policy = match get(members, "hit_policy") {
        None => HitPolicy::First,
        Some(json) => {
            let text = name("hit_policy", json)?;
            HitPolicy::from_name(&text).ok_or_else(|| {
                format_error(
                    &at(pointer, "hit_policy"),
                    format!("unknown hit policy {text:?}"),
                )
            })?
        }
    };
    let aggregate = match get(members, "aggregate") {
        None => None,
        Some(json) => {
            let text = name("aggregate", json)?;
            Some(Aggregate::from_name(&text).ok_or_else(|| {
                format_error(
                    &at(pointer, "aggregate"),
                    format!("unknown aggregate {text:?}"),
                )
            })?)
        }
    };
    let logic = match get(members, "logic") {
        None => "boolean".to_owned(),
        Some(Json::String(text)) => text.clone(),
        Some(_) => {
            return Err(format_error(
                &at(pointer, "logic"),
                "\"logic\" is a name in this version of the format",
            ));
        }
    };
    let strict = match get(members, "strict") {
        None => false,
        Some(Json::Bool(b)) => *b,
        Some(_) => {
            return Err(format_error(
                &at(pointer, "strict"),
                "\"strict\" must be a boolean",
            ));
        }
    };
    let rules_at = at(pointer, "rules");
    let rules = match get(members, "rules") {
        Some(Json::Array(items)) => {
            let mut rules = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                rules.push(shape_rule(item, &format!("{rules_at}/{i}"), limits)?);
            }
            rules
        }
        Some(_) => {
            return Err(format_error(
                &rules_at,
                "\"rules\" must be an array of rules",
            ));
        }
        None => return Err(format_error(&rules_at, "the key \"rules\" is missing")),
    };
    no_unknown_keys(members, &RULESET_KEYS, pointer)?;
    Ok(RawRuleSet {
        pointer: pointer.to_owned(),
        hit_policy,
        aggregate,
        logic,
        strict,
        rules,
    })
}

/// Build a rule set: its rules in order, then its own rules.
pub(crate) fn build_ruleset(raw: RawRuleSet) -> Result<RuleSet> {
    let pointer = raw.pointer;
    let mut rules: Vec<Rule> = Vec::with_capacity(raw.rules.len());
    for raw_rule in raw.rules {
        rules.push(build_rule(raw_rule)?);
    }
    for (i, rule) in rules.iter().enumerate() {
        if rules[..i].iter().any(|r| r.name() == rule.name()) {
            return Err(Error::new(
                ErrorCode::RuleDefinitionError,
                format!("two rules are named {:?}", rule.name()),
            )
            .at_pointer(format!("{pointer}/rules/{i}/name")));
        }
    }
    if raw.aggregate.is_some() && raw.hit_policy != HitPolicy::Collect {
        return Err(Error::new(
            ErrorCode::RuleDefinitionError,
            "an aggregate needs the collect hit policy",
        )
        .at_pointer(at(&pointer, "aggregate")));
    }
    let logic = LogicName::from_name(&raw.logic)
        .map_err(|error| error.at_pointer(at(&pointer, "logic")))?;
    let config = Config {
        hit_policy: raw.hit_policy,
        aggregate: raw.aggregate,
        logic: Some(logic),
        strict: raw.strict,
    };
    RuleSet::new(rules, config)
}

/// Write a rule, omitting optional keys at their default.
#[must_use]
pub fn write_rule(rule: &Rule) -> Json {
    let mut members = vec![
        ("name".to_owned(), Json::String(rule.name().into())),
        ("when".to_owned(), write_node(rule.when())),
    ];
    if !rule.then().is_empty() {
        let then = rule
            .then()
            .iter()
            .map(|(k, v)| (k.clone(), write_node(v)))
            .collect();
        members.push(("then".into(), Json::Object(then)));
    }
    if rule.get_priority() != 0 {
        members.push((
            "priority".into(),
            write_value(&Value::Int(rule.get_priority())),
        ));
    }
    if !rule.get_tags().is_empty() {
        members.push((
            "tags".into(),
            Json::Array(
                rule.get_tags()
                    .iter()
                    .map(|t| Json::String(t.clone()))
                    .collect(),
            ),
        ));
    }
    if let Some(description) = rule.get_description() {
        members.push(("description".into(), Json::String(description.into())));
    }
    if !rule.get_metadata().is_empty() {
        let metadata = rule
            .get_metadata()
            .iter()
            .map(|(k, v)| (k.to_owned(), write_value(v)))
            .collect();
        members.push(("metadata".into(), Json::Object(metadata)));
    }
    if let Some(action) = rule.get_action_name() {
        members.push(("do".into(), Json::String(action.into())));
    }
    Json::Object(members)
}

/// Write a rule set's content.
#[must_use]
pub fn write_ruleset(ruleset: &RuleSet) -> Json {
    let mut members = vec![(
        "hit_policy".to_owned(),
        Json::String(ruleset.hit_policy().as_str().into()),
    )];
    if let Some(aggregate) = ruleset.aggregate() {
        members.push(("aggregate".into(), Json::String(aggregate.as_str().into())));
    }
    members.push((
        "logic".into(),
        Json::String(ruleset.logic().as_str().into()),
    ));
    if ruleset.strict() {
        members.push(("strict".into(), Json::Bool(true)));
    }
    members.push((
        "rules".into(),
        Json::Array(ruleset.rules().iter().map(write_rule).collect()),
    ));
    Json::Object(members)
}

/// Write a decision's content (`spec/json-format.md`, *Decisions*).
#[must_use]
pub fn write_decision(decision: &Decision<'_>) -> Json {
    let ruleset = decision.ruleset();
    let mut members = vec![(
        "hit_policy".to_owned(),
        Json::String(ruleset.hit_policy().as_str().into()),
    )];
    if let Some(aggregate) = ruleset.aggregate() {
        members.push(("aggregate".into(), Json::String(aggregate.as_str().into())));
    }
    members.push((
        "logic".into(),
        Json::String(ruleset.logic().as_str().into()),
    ));
    let results = decision
        .results()
        .iter()
        .map(|result| {
            let mut entry = vec![
                ("rule".to_owned(), Json::String(result.rule.name().into())),
                ("fired".to_owned(), Json::Bool(result.fired)),
                ("when".to_owned(), write_result(&result.when)),
            ];
            if let Some(then) = &result.then {
                let trees = then
                    .iter()
                    .map(|(k, tree)| (k.clone(), write_result(tree)))
                    .collect();
                entry.push(("then".into(), Json::Object(trees)));
            }
            Json::Object(entry)
        })
        .collect();
    members.push(("results".into(), Json::Array(results)));
    let selected = decision
        .selected()
        .map(|r| Json::String(r.rule.name().into()))
        .collect();
    members.push(("selected".into(), Json::Array(selected)));
    match decision.aggregated() {
        // Payload keys are taken literally: never wrapped in `$map`.
        Some(Aggregated::Values(map)) => {
            let values = map
                .iter()
                .map(|(k, v)| (k.to_owned(), write_value(v)))
                .collect();
            members.push(("aggregated".into(), Json::Object(values)));
        }
        Some(Aggregated::NothingSelected) => members.push(("aggregated".into(), Json::Null)),
        None => {}
    }
    Json::Object(members)
}
