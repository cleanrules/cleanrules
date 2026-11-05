//! Conditions, rules and rule sets built in code (`lib/api/`), through the
//! core's constructors, which apply every construction rule (`spec/ast.md`,
//! `spec/rules.md`): the same checks and errors in both builds, as in Python.
//!
//! Constants and metadata arrive in the reader's encoding (`wire`): the
//! JavaScript half reads them with the rules of native data and rejects what
//! is not data (NaN, opaque values) before; the core rejects the rest.

use cleanrules::ast::{ArithOp, Closed, CompareOp, Expr, Kind, Node, StringOp};
use cleanrules::json::{Json, to_string};
use cleanrules::logic::LogicName;
use cleanrules::rules::{Aggregate, Config, HitPolicy, Rule, RuleSet};
use cleanrules::{Error, ErrorCode, NullCause, Segment, Value};

use super::wire::decode;
use super::{Outcome, Problem, Raw, out};

fn expression(message: impl Into<String>, hint: &str) -> Problem {
    Problem::Core(Error::new(ErrorCode::ExpressionError, message).with_hint(hint))
}

/// A misuse of these internal functions by the JavaScript half: a bug.
fn internal(what: &str, name: &str) -> Problem {
    Problem::Internal(format!("unknown {what} {name:?}"))
}

/// A value in the reader's encoding (`lib/read.js`, `encodeData`).
fn data(wire: &str) -> Outcome<Value> {
    decode(wire)
        .map(|read| read.value)
        .map_err(|problem| Problem::Internal(format!("the data encoding is wrong ({problem})")))
}

/// A field, by its path of keys (strings) and indices (whole numbers from 0).
pub(crate) fn field(path: Vec<Raw>) -> Outcome<Node> {
    let mut segments = Vec::with_capacity(path.len());
    for segment in path {
        segments.push(match segment {
            Raw::String(key) => Segment::Key(key),
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // Checked to be a whole number from 0 to 2^53.
            Raw::Number(index)
                if index.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&index) =>
            {
                Segment::Index(index as u64)
            }
            other => {
                let shown = match other {
                    Raw::Number(number) => format!("the number {number}"),
                    other => other.describe().to_owned(),
                };
                return Err(expression(
                    format!("a field path segment cannot be {shown}"),
                    "use non-empty names (strings) and indices (whole numbers from 0)",
                ));
            }
        });
    }
    Ok(Node::field(segments)?)
}

/// What is wrong with a value read as a constant, or `None`.
fn not_constant(value: &Value) -> Option<(&'static str, &'static str)> {
    match value {
        Value::Null(NullCause::DecimalOverflow) => Some((
            "the decimal constant does not fit in 38 digits",
            "decimals hold up to 38 significant digits and are never rounded",
        )),
        Value::Null(NullCause::IntOverflow) => Some((
            "the integer constant does not fit in 64 bits",
            "use decimal(...) for larger exact numbers",
        )),
        Value::Opaque(_) => Some((
            "the constant has no value of the data model",
            "constants are null, booleans, numbers, bigints, strings, decimal(), date(), datetime(), Date and Temporal values",
        )),
        _ => None,
    }
}

/// A constant, given in the reader's encoding.
pub(crate) fn constant(wire: &str) -> Outcome<Node> {
    let value = data(wire)?;
    if let Some((message, hint)) = not_constant(&value) {
        return Err(expression(message, hint));
    }
    Ok(Node::constant(value)?)
}

/// `left op right`, with `op` one of `eq`, `ne`, `lt`, `le`, `gt`, `ge`.
pub(crate) fn compare(op: &str, left: &Node, right: &Node) -> Outcome<Node> {
    let op = CompareOp::from_tag(op).ok_or_else(|| internal("comparison", op))?;
    Ok(Node::compare(op, left.clone(), right.clone())?)
}

/// `and` or `or` of `operands`.
pub(crate) fn connective(tag: &str, operands: Vec<Node>) -> Outcome<Node> {
    Ok(match tag {
        "and" => Node::and(operands)?,
        "or" => Node::or(operands)?,
        other => return Err(internal("connective", other)),
    })
}

/// `not`, `is_null`, `abs` or `neg` of `operand`.
pub(crate) fn unary(tag: &str, operand: &Node) -> Outcome<Node> {
    let operand = operand.clone();
    Ok(match tag {
        "not" => Node::not(operand)?,
        "is_null" => Node::is_null(operand)?,
        "abs" => Node::abs(operand)?,
        "neg" => Node::neg(operand)?,
        other => return Err(internal("node", other)),
    })
}

/// Membership in constants, given as a list in the reader's encoding.
pub(crate) fn is_in(operand: &Node, wire: &str) -> Outcome<Node> {
    let Value::List(values) = data(wire)? else {
        return Err(Problem::Internal("is_in takes a list".to_owned()));
    };
    for value in &values {
        if let Some((message, hint)) = not_constant(value) {
            return Err(expression(format!("isIn: {message}"), hint));
        }
    }
    Ok(Node::is_in(operand.clone(), values)?)
}

/// A range test; `closed` is `both`, `left`, `right` or `none`.
pub(crate) fn between(operand: &Node, low: &Node, high: &Node, closed: &str) -> Outcome<Node> {
    let closed = Closed::from_name(closed).ok_or_else(|| internal("closed", closed))?;
    Ok(Node::between(
        operand.clone(),
        low.clone(),
        high.clone(),
        closed,
    )?)
}

/// A regular expression search.
pub(crate) fn matches(operand: &Node, pattern: &str) -> Outcome<Node> {
    Ok(Node::matches(operand.clone(), pattern)?)
}

/// `starts_with`, `ends_with` or `contains`.
pub(crate) fn string_test(op: &str, operand: &Node, text: &str) -> Outcome<Node> {
    let op = StringOp::from_tag(op).ok_or_else(|| internal("string test", op))?;
    Ok(Node::string_test(op, operand.clone(), text)?)
}

/// `add`, `sub`, `mul` or `div`.
pub(crate) fn arith(op: &str, left: &Node, right: &Node) -> Outcome<Node> {
    let op = ArithOp::from_tag(op).ok_or_else(|| internal("operator", op))?;
    Ok(Node::arith(op, left.clone(), right.clone())?)
}

/// A node's tag, kind and own data, as JSON: `[tag, kind, detail]`, where
/// `detail` is a field's path, a constant's value (in `out`'s encoding) or
/// `null`.
pub(crate) fn details(node: &Node) -> String {
    let kind = match node.kind() {
        Kind::Condition => "condition",
        Kind::Value => "value",
        Kind::Any => "any",
    };
    let detail = match node.expr() {
        Expr::Field(path) => cleanrules::format::write_path(path),
        Expr::Const(value) => out::value(value),
        _ => Json::Null,
    };
    to_string(&Json::Array(vec![
        Json::String(node.tag().to_owned()),
        Json::String(kind.to_owned()),
        detail,
    ]))
}

/// What a rule is built from, as the JavaScript half checked it.
pub(crate) struct RuleArgs {
    pub(crate) name: String,
    pub(crate) when: Node,
    pub(crate) then: Vec<(String, Node)>,
    /// The priority's digits (a JavaScript integer or bigint).
    pub(crate) priority: String,
    pub(crate) tags: Vec<String>,
    pub(crate) description: Option<String>,
    /// The metadata in the reader's encoding, or `None` for none.
    pub(crate) metadata: Option<String>,
    pub(crate) action: Option<String>,
}

fn definition(rule: &str, message: impl Into<String>, hint: &str) -> Problem {
    let mut error = Error::new(ErrorCode::RuleDefinitionError, message).with_hint(hint);
    error.context_mut().rule = Some(rule.to_owned());
    Problem::Core(error)
}

/// The path, in metadata, of the first value that is not data.
fn metadata_problem(value: &Value, at: &str) -> Option<String> {
    match value {
        Value::Null(cause) if *cause != NullCause::Null => {
            Some(format!("{at} is out of range or unreadable"))
        }
        Value::Opaque(opaque) => Some(format!("{at} is not a data value: a {}", opaque.type_name)),
        Value::List(items) => items
            .iter()
            .enumerate()
            .find_map(|(i, item)| metadata_problem(item, &format!("{at}[{i}]"))),
        Value::Map(map) => map
            .iter()
            .find_map(|(key, item)| metadata_problem(item, &format!("{at}[{key:?}]"))),
        _ => None,
    }
}

/// A rule.
pub(crate) fn rule(args: RuleArgs) -> Outcome<Rule> {
    let RuleArgs {
        name,
        when,
        then,
        priority,
        tags,
        description,
        metadata,
        action,
    } = args;
    let priority: i64 = priority.parse().map_err(|_| {
        definition(
            &name,
            format!("priority {priority} does not fit in 64 bits"),
            "a priority is an integer of 64 bits",
        )
    })?;
    let mut rule = Rule::with_payload(name.clone(), when, then)?
        .priority(priority)
        .tags(tags);
    if let Some(description) = description {
        rule = rule.description(description);
    }
    if let Some(action) = action {
        rule = rule.action_name(action)?;
    }
    if let Some(metadata) = metadata {
        let value = data(&metadata)?;
        if let Some(problem) = metadata_problem(&value, "metadata") {
            return Err(definition(
                &name,
                problem,
                "metadata holds data: null, booleans, numbers, strings, dates, arrays and objects",
            ));
        }
        let Value::Map(map) = value else {
            return Err(Problem::Internal("metadata is a map".to_owned()));
        };
        if !map.is_empty() {
            rule = rule.metadata(map);
        }
    }
    Ok(rule)
}

/// A rule's tags as the core holds them (sorted, without repeats), its
/// metadata (in `out`'s encoding) and its action's name: `[tags, metadata,
/// action]`.
pub(crate) fn rule_details(rule: &Rule) -> String {
    let tags = rule
        .get_tags()
        .iter()
        .map(|tag| Json::String(tag.clone()))
        .collect();
    let action = rule
        .get_action_name()
        .map_or(Json::Null, |name| Json::String(name.to_owned()));
    to_string(&Json::Array(vec![
        Json::Array(tags),
        out::map(rule.get_metadata()),
        action,
    ]))
}

fn name_or<'a>(raw: &'a Raw, default: &'a str, what: &str) -> Outcome<Option<&'a str>> {
    match raw {
        Raw::Undefined => Ok(Some(default)),
        Raw::Null => Ok(None),
        Raw::String(name) => Ok(Some(name)),
        other => Err(definition_plain(
            format!("{what} is a string, not {}", other.describe()),
            "",
        )),
    }
}

fn definition_plain(message: impl Into<String>, hint: &str) -> Problem {
    let error = Error::new(ErrorCode::RuleDefinitionError, message);
    Problem::Core(if hint.is_empty() {
        error
    } else {
        error.with_hint(hint)
    })
}

/// A rule set of `rules` with its options as JavaScript gives them: the hit
/// policy (`"first"` when left out), the aggregate, the logic (`"boolean"`)
/// and `strict` (`false`). Names are the format's, in any case.
pub(crate) fn ruleset(
    rules: Vec<Rule>,
    hit_policy: &Raw,
    aggregate: &Raw,
    logic: &Raw,
    strict: &Raw,
) -> Outcome<RuleSet> {
    let policy = name_or(hit_policy, "first", "hitPolicy")?.unwrap_or("first");
    let hit_policy = HitPolicy::from_name(&policy.to_lowercase()).ok_or_else(|| {
        definition_plain(
            format!("unknown hit policy {policy:?}"),
            "use one of: first, priority, unique, any, rule_order, collect",
        )
    })?;
    let aggregate = match name_or(aggregate, "", "aggregate")? {
        None | Some("") => None,
        Some(name) => Some(Aggregate::from_name(&name.to_lowercase()).ok_or_else(|| {
            definition_plain(
                format!("unknown aggregate {name:?}"),
                "use one of: count, max, min, sum",
            )
        })?),
    };
    let logic = name_or(logic, "boolean", "logic")?.unwrap_or("boolean");
    let strict = match strict {
        Raw::Undefined | Raw::Null => false,
        Raw::Bool(strict) => *strict,
        other => {
            return Err(definition_plain(
                format!("strict must be a boolean, not {}", other.describe()),
                "",
            ));
        }
    };
    let config = Config {
        hit_policy,
        aggregate,
        logic: Some(LogicName::from_name(logic)?),
        strict,
    };
    Ok(RuleSet::new(rules, config)?)
}
