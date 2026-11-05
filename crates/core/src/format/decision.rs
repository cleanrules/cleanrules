//! Reading a decision with the rule set that produced it (`spec/json-format.md`,
//! *Reading a decision*).
//!
//! The JSON is rebuilt into a [`Decision`] whose result trees point to the rule
//! set's nodes, then checked in the order of the spec; the first failed check is a
//! `decision_mismatch`. A decision that is accepted is one the core could have
//! produced for some input.

use std::borrow::Cow;

use crate::ast::{Closed, CompareOp, Expr, Node};
use crate::error::{Error, ErrorCode, Result, Segment};
use crate::eval::{Evaluated, Outcome, Reason, ReasonCode, recompute};
use crate::format::result::{write_reason, write_truth};
use crate::format::value::{ValueContext, read_value, write_value};
use crate::json::{Json, pointer_token, same};
use crate::logic::{Kleene, Logic, Truth};
use crate::rules::{Aggregated, Decision, HitPolicy, RuleResult, RuleSet};
use crate::value::{Group, Value};

fn format_error(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

fn mismatch(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::DecisionMismatch, message).at_pointer(pointer)
}

fn at(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", pointer_token(key))
}

fn get<'a>(members: &'a [(String, Json)], key: &str) -> Option<&'a Json> {
    members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn unknown_keys(members: &[(String, Json)], known: &[&str], pointer: &str) -> Result<()> {
    match members.iter().find(|(k, _)| !known.contains(&k.as_str())) {
        Some((key, _)) => Err(format_error(
            &at(pointer, key),
            format!("unknown key {key:?}"),
        )),
        None => Ok(()),
    }
}

const TAGS: [&str; 24] = [
    "field",
    "const",
    "eq",
    "ne",
    "lt",
    "le",
    "gt",
    "ge",
    "and",
    "or",
    "not",
    "in",
    "between",
    "is_null",
    "matches",
    "starts_with",
    "ends_with",
    "contains",
    "add",
    "sub",
    "mul",
    "div",
    "abs",
    "neg",
];

/// A recorded result tree whose shape is valid, with its values already read, so
/// any form that *Values* accepts has become the value it stands for.
struct Tree {
    pointer: String,
    op: String,
    path: Option<Vec<Segment>>,
    outcome: Outcome<Truth>,
    reason: Option<Reason>,
    children: Vec<Tree>,
}

/// Whether two values read from a decision, or computed, are the same value:
/// compared in their canonical form (`spec/json-format.md`, *Values*), so the
/// forms a reader accepts (`{"$float": 1}`, `{"$int": "5"}`, `{"$map": ...}`) match
/// the canonical ones, while an int and a float, or a decimal's scale, differ.
fn same_value(a: &Value, b: &Value) -> bool {
    same(&write_value(a), &write_value(b))
}

/// Whether two recorded trees are equal: the same ops, paths, children, truth
/// values or values (by [`same_value`]) and reasons (without `detail`, which is
/// never compared), whatever the order of their keys or the form of their values.
fn same_tree(a: &Tree, b: &Tree) -> bool {
    let outcomes = match (&a.outcome, &b.outcome) {
        (Outcome::Truth(x), Outcome::Truth(y)) => x == y,
        (Outcome::Value(x), Outcome::Value(y)) => same_value(x, y),
        _ => false,
    };
    a.op == b.op
        && a.path == b.path
        && outcomes
        && same(
            &reason_json(a.reason.as_ref()),
            &reason_json(b.reason.as_ref()),
        )
        && a.children.len() == b.children.len()
        && a.children
            .iter()
            .zip(&b.children)
            .all(|(x, y)| same_tree(x, y))
}

fn read_path(json: &Json, pointer: &str) -> Result<Vec<Segment>> {
    let Json::Array(items) = json else {
        return Err(format_error(pointer, "a path is an array"));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| match item {
            Json::String(key) => Ok(Segment::Key(key.clone())),
            Json::Number(n) if n.is_integer_literal() => n
                .as_i128()
                .and_then(|i| u64::try_from(i).ok())
                .map(Segment::Index)
                .ok_or_else(|| {
                    format_error(
                        &format!("{pointer}/{i}"),
                        "a path index is a non-negative int",
                    )
                }),
            _ => Err(format_error(
                &format!("{pointer}/{i}"),
                "a path segment is a string or an int",
            )),
        })
        .collect()
}

fn read_reason(json: &Json, pointer: &str) -> Result<Reason> {
    let Json::Object(members) = json else {
        return Err(format_error(pointer, "a reason is an object"));
    };
    let code = match get(members, "code") {
        Some(Json::String(code)) => [
            ReasonCode::MissingField,
            ReasonCode::Null,
            ReasonCode::TypeMismatch,
            ReasonCode::Unorderable,
            ReasonCode::DivisionByZero,
            ReasonCode::IntOverflow,
            ReasonCode::DecimalOverflow,
            ReasonCode::NanResult,
            ReasonCode::ReadError,
        ]
        .into_iter()
        .find(|c| c.as_str() == code)
        .ok_or_else(|| {
            format_error(
                &at(pointer, "code"),
                format!("unknown reason code {code:?}"),
            )
        })?,
        _ => return Err(format_error(&at(pointer, "code"), "a reason needs a code")),
    };
    let paths = match get(members, "paths") {
        None => Vec::new(),
        Some(Json::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, p)| read_path(p, &format!("{}/{i}", at(pointer, "paths"))))
            .collect::<Result<_>>()?,
        Some(_) => {
            return Err(format_error(
                &at(pointer, "paths"),
                "\"paths\" is an array of paths",
            ));
        }
    };
    let groups = [
        Group::Number,
        Group::String,
        Group::Boolean,
        Group::Date,
        Group::NaiveDatetime,
        Group::AwareDatetime,
        Group::Other,
    ];
    let types = match get(members, "types") {
        None => Vec::new(),
        Some(Json::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, t)| {
                match t {
                    Json::String(name) => groups.into_iter().find(|g| g.as_str() == name),
                    _ => None,
                }
                .ok_or_else(|| {
                    format_error(
                        &format!("{}/{i}", at(pointer, "types")),
                        "an unknown type group",
                    )
                })
            })
            .collect::<Result<_>>()?,
        Some(_) => {
            return Err(format_error(
                &at(pointer, "types"),
                "\"types\" is an array of type groups",
            ));
        }
    };
    let detail = match get(members, "detail") {
        None => None,
        Some(Json::String(text)) => Some(text.clone()),
        Some(_) => {
            return Err(format_error(
                &at(pointer, "detail"),
                "\"detail\" is a string",
            ));
        }
    };
    unknown_keys(members, &["code", "paths", "types", "detail"], pointer)?;
    Ok(Reason {
        code,
        paths,
        types,
        detail,
    })
}

fn read_tree(json: &Json, pointer: &str, depth: usize, max_depth: usize) -> Result<Tree> {
    if depth > max_depth {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            "the result tree is nested too deep",
        )
        .at_pointer(pointer));
    }
    let Json::Object(members) = json else {
        return Err(format_error(pointer, "a result is an object"));
    };
    let op = match get(members, "op") {
        Some(Json::String(op)) if TAGS.contains(&op.as_str()) => op.clone(),
        Some(Json::String(op)) => {
            return Err(format_error(
                &at(pointer, "op"),
                format!("unknown op {op:?}"),
            ));
        }
        _ => return Err(format_error(&at(pointer, "op"), "a result needs an \"op\"")),
    };
    let path = match (get(members, "path"), op == "field") {
        (Some(path), true) => Some(read_path(path, &at(pointer, "path"))?),
        (None, true) => {
            return Err(format_error(
                &at(pointer, "path"),
                "a field result needs its path",
            ));
        }
        (Some(_), false) => {
            return Err(format_error(
                &at(pointer, "path"),
                "only a field result has a path",
            ));
        }
        (None, false) => None,
    };
    let outcome = match (get(members, "truth"), get(members, "value")) {
        (Some(Json::Bool(true)), None) => Outcome::Truth(Truth::True),
        (Some(Json::Bool(false)), None) => Outcome::Truth(Truth::False),
        (Some(Json::Null), None) => Outcome::Truth(Truth::Unknown),
        (Some(_), None) => {
            return Err(format_error(
                &at(pointer, "truth"),
                "a truth value is true, false or null",
            ));
        }
        (None, Some(value)) => Outcome::Value(read_value(
            value,
            &at(pointer, "value"),
            ValueContext::Recorded,
        )?),
        _ => {
            return Err(format_error(
                pointer,
                "a result has either \"truth\" or \"value\"",
            ));
        }
    };
    let reason = get(members, "reason")
        .map(|r| read_reason(r, &at(pointer, "reason")))
        .transpose()?;
    let children = match get(members, "children") {
        None => Vec::new(),
        Some(Json::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, child)| {
                read_tree(
                    child,
                    &format!("{}/{i}", at(pointer, "children")),
                    depth + 1,
                    max_depth,
                )
            })
            .collect::<Result<_>>()?,
        Some(_) => {
            return Err(format_error(
                &at(pointer, "children"),
                "\"children\" is an array",
            ));
        }
    };
    unknown_keys(
        members,
        &["op", "path", "truth", "value", "reason", "children"],
        pointer,
    )?;
    Ok(Tree {
        pointer: pointer.to_owned(),
        op,
        path,
        outcome,
        reason,
        children,
    })
}

/// The children a result of `node` has: each child's node and whether it has a
/// truth value (`spec/json-format.md`, *Result trees*).
fn expected_children<'a>(node: &Cow<'a, Node>, truth: bool) -> Result<Vec<(Cow<'a, Node>, bool)>> {
    let child = |i: usize| -> Cow<'a, Node> {
        match node {
            Cow::Borrowed(n) => Cow::Borrowed(n.children()[i]),
            Cow::Owned(n) => Cow::Owned(n.children()[i].clone()),
        }
    };
    Ok(match node.expr() {
        Expr::Field(_) | Expr::Const(_) if truth => vec![(node.clone(), false)],
        Expr::Field(_) | Expr::Const(_) => Vec::new(),
        Expr::And(ops) | Expr::Or(ops) => (0..ops.len()).map(|i| (child(i), true)).collect(),
        Expr::Not(_) => vec![(child(0), true)],
        Expr::Between {
            operand,
            low,
            high,
            closed,
        } => {
            let low_op = if matches!(closed, Closed::Both | Closed::Left) {
                CompareOp::Ge
            } else {
                CompareOp::Gt
            };
            let high_op = if matches!(closed, Closed::Both | Closed::Right) {
                CompareOp::Le
            } else {
                CompareOp::Lt
            };
            vec![
                (
                    Cow::Owned(Node::compare(low_op, (**operand).clone(), (**low).clone())?),
                    true,
                ),
                (
                    Cow::Owned(Node::compare(
                        high_op,
                        (**operand).clone(),
                        (**high).clone(),
                    )?),
                    true,
                ),
            ]
        }
        _ => (0..node.children().len())
            .map(|i| (child(i), false))
            .collect(),
    })
}

/// Check 3: the tree matches its node, node by node; build the result.
fn match_tree<'a>(tree: &Tree, node: Cow<'a, Node>, truth: bool) -> Result<Evaluated<'a, Truth>> {
    if tree.op != node.tag() {
        return Err(mismatch(
            &at(&tree.pointer, "op"),
            format!("expected '{}', found '{}'", node.tag(), tree.op),
        ));
    }
    let has_truth = matches!(tree.outcome, Outcome::Truth(_));
    if has_truth != truth {
        return Err(mismatch(
            &tree.pointer,
            "this result has a truth value where a value belongs, or the reverse",
        ));
    }
    match node.expr() {
        Expr::Field(path) if tree.path.as_ref() != Some(path) => {
            return Err(mismatch(&at(&tree.pointer, "path"), "not the rule's field"));
        }
        Expr::Const(constant) if !truth => {
            // Without a truth value, the outcome is a value (checked above).
            if let Outcome::Value(value) = &tree.outcome
                && Node::constant(value.clone()).ok() != Node::constant(constant.clone()).ok()
            {
                return Err(mismatch(
                    &at(&tree.pointer, "value"),
                    "not the rule's constant",
                ));
            }
        }
        _ => {}
    }
    let expected = expected_children(&node, truth)?;
    if expected.len() != tree.children.len() {
        return Err(mismatch(
            &at(&tree.pointer, "children"),
            format!("expected {} children", expected.len()),
        ));
    }
    let mut children = Vec::with_capacity(expected.len());
    for (child_tree, (child_node, child_truth)) in tree.children.iter().zip(expected) {
        children.push(match_tree(child_tree, child_node, child_truth)?);
    }
    if matches!(node.expr(), Expr::Between { .. })
        // Both halves were matched above, so each has its operand first.
        && let (Some(first), Some(second)) = (
            tree.children[0].children.first(),
            tree.children[1].children.first(),
        )
        && !same_tree(first, second)
    {
        return Err(mismatch(
            &second.pointer,
            "both halves of a between hold the same operand",
        ));
    }
    Ok(Evaluated {
        node,
        outcome: tree.outcome.clone(),
        reason: tree.reason.clone(),
        children,
    })
}

fn reason_json(reason: Option<&Reason>) -> Json {
    // `detail` is never compared.
    reason.map_or(Json::Null, |r| {
        write_reason(&Reason {
            detail: None,
            ..r.clone()
        })
    })
}

/// Check 5: every result is what evaluation gives from its children's.
fn check_recomputed(result: &Evaluated<'_, Truth>, tree: &Tree) -> Result<()> {
    for (child, child_tree) in result.children.iter().zip(&tree.children) {
        check_recomputed(child, child_tree)?;
    }
    let (outcome, reason) = recompute(&Kleene, result)?;
    match (&outcome, &result.outcome) {
        (Outcome::Truth(a), Outcome::Truth(b)) if a != b => {
            return Err(mismatch(
                &at(&tree.pointer, "truth"),
                format!(
                    "evaluation gives {}",
                    crate::json::to_string(&write_truth(*a))
                ),
            ));
        }
        (Outcome::Value(a), Outcome::Value(b)) if !same_value(a, b) => {
            return Err(mismatch(
                &at(&tree.pointer, "value"),
                "evaluation gives another value",
            ));
        }
        _ => {}
    }
    if !same(
        &reason_json(reason.as_ref()),
        &reason_json(result.reason.as_ref()),
    ) {
        return Err(mismatch(
            &at(&tree.pointer, "reason"),
            "evaluation gives another reason",
        ));
    }
    Ok(())
}

/// Read a decision's content (the object under `"decision"`) with the rule set
/// that produced it.
///
/// # Errors
///
/// `format_error` for its shape, then `decision_mismatch` for the first check of
/// `spec/json-format.md` (*Reading a decision*) that fails.
#[allow(clippy::too_many_lines)]
pub(crate) fn read_decision<'a>(
    json: &Json,
    pointer: &str,
    ruleset: &'a RuleSet,
    max_depth: usize,
) -> Result<Decision<'a>> {
    // Shape.
    let Json::Object(members) = json else {
        return Err(format_error(pointer, "a decision is an object"));
    };
    let string = |key: &str| -> Result<Option<String>> {
        match get(members, key) {
            None => Ok(None),
            Some(Json::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format_error(
                &at(pointer, key),
                format!("{key:?} is a string"),
            )),
        }
    };
    let hit_policy =
        string("hit_policy")?.ok_or_else(|| format_error(&at(pointer, "hit_policy"), "missing"))?;
    let policy = HitPolicy::from_name(&hit_policy).ok_or_else(|| {
        format_error(
            &at(pointer, "hit_policy"),
            format!("unknown hit policy {hit_policy:?}"),
        )
    })?;
    let aggregate = string("aggregate")?;
    let logic = string("logic")?.ok_or_else(|| format_error(&at(pointer, "logic"), "missing"))?;
    let results_at = at(pointer, "results");
    let Some(Json::Array(items)) = get(members, "results") else {
        return Err(format_error(&results_at, "\"results\" is an array"));
    };
    let mut recorded = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let item_at = format!("{results_at}/{i}");
        let Json::Object(fields) = item else {
            return Err(format_error(&item_at, "a rule result is an object"));
        };
        let Some(Json::String(name)) = get(fields, "rule") else {
            return Err(format_error(
                &at(&item_at, "rule"),
                "a rule result names its rule",
            ));
        };
        let Some(Json::Bool(fired)) = get(fields, "fired") else {
            return Err(format_error(
                &at(&item_at, "fired"),
                "\"fired\" is a boolean",
            ));
        };
        let when = read_tree(
            get(fields, "when").ok_or_else(|| format_error(&at(&item_at, "when"), "missing"))?,
            &at(&item_at, "when"),
            1,
            max_depth + 1,
        )?;
        let then = match get(fields, "then") {
            None => None,
            Some(Json::Object(entries)) => {
                let base = at(&item_at, "then");
                Some(
                    entries
                        .iter()
                        .map(|(k, v)| {
                            Ok((k.clone(), read_tree(v, &at(&base, k), 1, max_depth + 1)?))
                        })
                        .collect::<Result<Vec<_>>>()?,
                )
            }
            Some(_) => return Err(format_error(&at(&item_at, "then"), "\"then\" is an object")),
        };
        unknown_keys(fields, &["rule", "fired", "when", "then"], &item_at)?;
        recorded.push((item_at, name.clone(), *fired, when, then));
    }
    let Some(Json::Array(selected_json)) = get(members, "selected") else {
        return Err(format_error(
            &at(pointer, "selected"),
            "\"selected\" is an array of rule names",
        ));
    };
    let mut selected_names = Vec::new();
    for (i, item) in selected_json.iter().enumerate() {
        let Json::String(name) = item else {
            return Err(format_error(
                &format!("{}/{i}", at(pointer, "selected")),
                "a rule name",
            ));
        };
        selected_names.push(name.clone());
    }
    let aggregated_at = at(pointer, "aggregated");
    let aggregated_json = get(members, "aggregated");
    if aggregate.is_some() != aggregated_json.is_some() {
        return Err(format_error(
            &aggregated_at,
            "\"aggregated\" comes with \"aggregate\"",
        ));
    }
    // The recorded aggregate, read as values: `None` when nothing was selected.
    let recorded_aggregated: Option<Vec<(String, Value)>> = match aggregated_json {
        None | Some(Json::Null) => None,
        Some(Json::Object(entries)) => Some(
            entries
                .iter()
                .map(|(key, value)| {
                    let value_at = at(&aggregated_at, key);
                    Ok((
                        key.clone(),
                        read_value(value, &value_at, ValueContext::Recorded)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        Some(_) => {
            return Err(format_error(
                &aggregated_at,
                "\"aggregated\" is an object of payload keys, or null when nothing was selected",
            ));
        }
    };
    unknown_keys(
        members,
        &[
            "hit_policy",
            "aggregate",
            "logic",
            "results",
            "selected",
            "aggregated",
        ],
        pointer,
    )?;

    // 1. The rule set's configuration.
    if policy != ruleset.hit_policy() {
        return Err(mismatch(
            &at(pointer, "hit_policy"),
            "not the rule set's hit policy",
        ));
    }
    if aggregate.as_deref() != ruleset.aggregate().map(crate::rules::Aggregate::as_str) {
        return Err(mismatch(
            &at(pointer, "aggregate"),
            "not the rule set's aggregate",
        ));
    }
    if logic != ruleset.logic().as_str() {
        return Err(mismatch(&at(pointer, "logic"), "not the rule set's logic"));
    }
    // 2. Every rule, in rule order.
    let rules = ruleset.rules();
    for i in 0..rules.len().max(recorded.len()) {
        match (rules.get(i), recorded.get(i)) {
            (Some(rule), Some((item_at, name, ..))) if rule.name() != name => {
                return Err(mismatch(
                    &at(item_at, "rule"),
                    format!("expected the rule {:?}", rule.name()),
                ));
            }
            (Some(rule), None) => {
                return Err(mismatch(
                    &results_at,
                    format!("the rule {:?} has no result", rule.name()),
                ));
            }
            (None, Some((item_at, ..))) => {
                return Err(mismatch(item_at, "the rule set has no such rule"));
            }
            _ => {}
        }
    }
    // 3. Each tree matches its rule's nodes.
    let mut results: Vec<RuleResult<'a>> = Vec::with_capacity(rules.len());
    for (rule, (_, _, fired, when, then)) in rules.iter().zip(&recorded) {
        let when_result = match_tree(when, Cow::Borrowed(rule.when()), true)?;
        let mut then_results = None;
        if let Some(entries) = then {
            let mut built = Vec::new();
            for (key, tree) in entries {
                if let Some((_, node)) = rule.then().iter().find(|(k, _)| k == key) {
                    built.push((key.clone(), match_tree(tree, Cow::Borrowed(node), false)?));
                }
            }
            then_results = Some(built);
        }
        results.push(RuleResult {
            rule,
            fired: *fired,
            when: when_result,
            then: then_results,
        });
    }
    // 4. The payloads that were evaluated, judged from `selected` and `fired` as written.
    for (result, (item_at, name, fired, _, then)) in results.iter().zip(&recorded) {
        let evaluated = selected_names.contains(name) || (policy == HitPolicy::Any && *fired);
        if evaluated != then.is_some() {
            return Err(mismatch(
                &at(item_at, "then"),
                if evaluated {
                    "the payload was evaluated"
                } else {
                    "the payload was not evaluated"
                },
            ));
        }
        if let Some(entries) = then {
            let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
            let wanted: Vec<&str> = result.rule.then().iter().map(|(k, _)| k.as_str()).collect();
            if keys != wanted {
                return Err(mismatch(
                    &at(item_at, "then"),
                    "not the payload's keys, in order",
                ));
            }
        }
    }
    // 5. Every result is what evaluation gives from its children's.
    for (result, (_, _, _, when, then)) in results.iter().zip(&recorded) {
        check_recomputed(&result.when, when)?;
        if let (Some(built), Some(trees)) = (&result.then, then) {
            for ((_, evaluated), (_, tree)) in built.iter().zip(trees) {
                check_recomputed(evaluated, tree)?;
            }
        }
    }
    // 6. `fired` agrees with the condition's truth value.
    for (result, (item_at, ..)) in results.iter().zip(&recorded) {
        let truth = result.when.truth().unwrap_or(Truth::Unknown);
        if result.fired != Kleene.fires(truth) {
            return Err(mismatch(
                &at(item_at, "fired"),
                "the condition's truth value says otherwise",
            ));
        }
    }
    // 7. The selection is the hit policy's.
    let fired: Vec<usize> = (0..results.len()).filter(|&i| results[i].fired).collect();
    let selected_at = at(pointer, "selected");
    let selected = ruleset
        .select(&fired)
        .map_err(|_| mismatch(&selected_at, "the hit policy could not decide"))?;
    let names: Vec<&str> = selected.iter().map(|&i| rules[i].name()).collect();
    if names
        != selected_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    {
        return Err(mismatch(&selected_at, "not the hit policy's selection"));
    }
    if policy == HitPolicy::Any && !RuleSet::outputs_agree(&results, &fired) {
        return Err(mismatch(
            &selected_at,
            "under any, the fired rules' outputs differ",
        ));
    }
    // 8. The aggregate.
    let aggregated = match ruleset.aggregate() {
        None => None,
        Some(aggregate) => {
            let computed = ruleset
                .aggregated(aggregate, &results, &selected)
                .map_err(|_| mismatch(&aggregated_at, "the aggregate cannot be computed"))?;
            // Compared as values, key by key in order (`spec/rules.md`, *`COLLECT`
            // aggregates*), so every accepted form of a value matches.
            let agrees = match (&computed, &recorded_aggregated) {
                (Aggregated::NothingSelected, None) => true,
                (Aggregated::Values(map), Some(entries)) => {
                    map.len() == entries.len()
                        && map
                            .iter()
                            .zip(entries)
                            .all(|((k, v), (l, w))| k == l && same_value(v, w))
                }
                _ => false,
            };
            if !agrees {
                return Err(mismatch(
                    &aggregated_at,
                    "not the aggregate of the selected outputs",
                ));
            }
            Some(computed)
        }
    };
    Ok(Decision::from_parts(ruleset, results, selected, aggregated))
}

#[cfg(test)]
mod tests {
    use super::{read_decision, read_tree, same_tree, same_value};
    use crate::ErrorCode;
    use crate::format::read_ruleset;
    use crate::format::value::{ValueContext, read_value};
    use crate::json::{Json, Limits, parse};
    use crate::value::Value;

    fn json(text: &str) -> Json {
        parse(text, &Limits::default()).unwrap()
    }

    fn value(text: &str) -> Value {
        read_value(&json(text), "", ValueContext::Recorded).unwrap()
    }

    #[test]
    fn values_compare_whatever_their_form() {
        for (a, b) in [
            ("608.0", r#"{"$float": 608}"#),
            ("6.08e2", r#"{"$float": 608.0}"#),
            (r#"{"$float": "inf"}"#, r#"{"$float": "inf"}"#),
            ("5", r#"{"$int": "5"}"#),
            ("5", r#"{"$int": "005"}"#),
            ("5", r#"{"$int": 5}"#),
            (
                r#"{"a": [1, 2.5]}"#,
                r#"{"$map": {"a": [{"$int": 1}, {"$float": 2.5}]}}"#,
            ),
            (r#"{"$decimal": "1.50"}"#, r#"{"$decimal": "150e-2"}"#),
        ] {
            assert!(same_value(&value(a), &value(b)), "{a} and {b}");
        }
        for (a, b) in [
            ("608", r#"{"$float": 608}"#),
            (r#"{"$int": 3}"#, "3.0"),
            ("0.0", "-0.0"),
            (r#"{"$decimal": "1.0"}"#, r#"{"$decimal": "1.00"}"#),
            (r#"{"a": 1, "b": 2}"#, r#"{"b": 2, "a": 1}"#),
        ] {
            assert!(!same_value(&value(a), &value(b)), "{a} and {b}");
        }
    }

    #[test]
    fn trees_compare_by_content_not_by_text() {
        let tree = |text: &str| read_tree(&json(text), "", 1, 8).unwrap();
        let canonical = tree(
            r#"{"op": "field", "path": ["w"], "value": null, "reason": {"code": "missing_field", "paths": [["w"]]}}"#,
        );
        let spelled_out = tree(
            r#"{"reason": {"detail": "w is missing", "types": [], "paths": [["w"]], "code": "missing_field"}, "value": null, "path": ["w"], "op": "field"}"#,
        );
        assert!(same_tree(&canonical, &spelled_out));
        let other_reason = tree(
            r#"{"op": "field", "path": ["w"], "value": null, "reason": {"code": "null", "paths": [["w"]]}}"#,
        );
        assert!(!same_tree(&canonical, &other_reason));
        let float = tree(r#"{"op": "field", "path": ["x"], "value": 3.0}"#);
        let alternative = tree(r#"{"op": "field", "path": ["x"], "value": {"$float": 3}}"#);
        let int = tree(r#"{"op": "field", "path": ["x"], "value": 3}"#);
        assert!(same_tree(&float, &alternative));
        assert!(!same_tree(&float, &int));
        assert!(!same_tree(
            &float,
            &tree(r#"{"op": "field", "path": ["y"], "value": 3.0}"#)
        ));
        let truth = tree(
            r#"{"op": "field", "path": ["x"], "truth": true, "children": [{"op": "field", "path": ["x"], "value": true}]}"#,
        );
        assert!(!same_tree(&truth, &float));
        assert!(!same_tree(
            &truth,
            &tree(r#"{"op": "field", "path": ["x"], "truth": true}"#)
        ));
    }

    /// A collect-sum rule set whose sum is the float 608.0 and the int 3, and its
    /// decision with `aggregated` replaced.
    fn read_with_aggregated(aggregated: &str) -> crate::Result<()> {
        let ruleset = read_ruleset(
            &json(
                r#"{"hit_policy": "collect", "aggregate": "sum", "logic": "boolean", "rules": [
                    {"name": "p", "when": {"op": "const", "value": true}, "then": {"y": {"op": "const", "value": 600.0}, "n": {"op": "const", "value": 1}}},
                    {"name": "q", "when": {"op": "const", "value": true}, "then": {"y": {"op": "const", "value": 8.0}, "n": {"op": "const", "value": 2}}}]}"#,
            ),
            "/ruleset",
            &Limits::default(),
        )
        .unwrap();
        let result = |name: &str, y: &str, n: &str| {
            format!(
                r#"{{"rule": "{name}", "fired": true, "when": {{"op": "const", "truth": true, "children": [{{"op": "const", "value": true}}]}}, "then": {{"y": {{"op": "const", "value": {y}}}, "n": {{"op": "const", "value": {n}}}}}}}"#
            )
        };
        let decision = format!(
            r#"{{"hit_policy": "collect", "aggregate": "sum", "logic": "boolean", "results": [{}, {}], "selected": ["p", "q"], "aggregated": {aggregated}}}"#,
            result("p", r#"{"$float": 600}"#, "1"),
            result("q", "8.0", r#"{"$int": "2"}"#),
        );
        read_decision(&json(&decision), "/decision", &ruleset, 8).map(|_| ())
    }

    #[test]
    fn the_aggregate_is_compared_as_values() {
        for accepted in [
            r#"{"y": 608.0, "n": 3}"#,
            r#"{"y": {"$float": 608}, "n": {"$int": "3"}}"#,
            r#"{"y": 6.08e2, "n": {"$int": 3}}"#,
        ] {
            assert!(read_with_aggregated(accepted).is_ok(), "{accepted}");
        }
        for (rejected, code, pointer) in [
            (
                r#"{"y": 608, "n": 3}"#,
                ErrorCode::DecisionMismatch,
                "/decision/aggregated",
            ),
            (
                r#"{"y": 608.0, "n": {"$float": 3}}"#,
                ErrorCode::DecisionMismatch,
                "/decision/aggregated",
            ),
            (
                r#"{"n": 3, "y": 608.0}"#,
                ErrorCode::DecisionMismatch,
                "/decision/aggregated",
            ),
            (
                r#"{"y": 608.0}"#,
                ErrorCode::DecisionMismatch,
                "/decision/aggregated",
            ),
            ("null", ErrorCode::DecisionMismatch, "/decision/aggregated"),
            ("[608.0, 3]", ErrorCode::FormatError, "/decision/aggregated"),
            (
                r#"{"y": {"$float": "608"}, "n": 3}"#,
                ErrorCode::FormatError,
                "/decision/aggregated/y/$float",
            ),
        ] {
            let error = read_with_aggregated(rejected).unwrap_err();
            assert_eq!(error.code(), code, "{rejected}");
            assert_eq!(
                error.context().pointer.as_deref(),
                Some(pointer),
                "{rejected}"
            );
        }
    }
}
