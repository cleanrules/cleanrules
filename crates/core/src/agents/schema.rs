//! The input schema of a rule set (`spec/agents.md`, *Input schemas*).

use std::collections::BTreeMap;

use crate::ast::{Expr, Node};
use crate::error::Segment;
use crate::explain::{Summary, places};
use crate::json::{Json, JsonNumber};
use crate::logic::LogicName;
use crate::value::Value;

/// The JSON Schema dialect of input schemas.
const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Lists whose largest index read is at least this get no `prefixItems`, so
/// that a rule reading `items[1000000000]` cannot make a huge schema.
pub const MAX_PREFIX_ITEMS: u64 = 256;

/// The type a place that reads a field expects (`spec/agents.md`, *Types of a
/// field*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Type {
    Boolean,
    Number,
    Decimal,
    String,
    Date,
    NaiveDatetime,
    AwareDatetime,
}

/// The typed form of a value in `spec/schemas/v1/value.schema.json`.
fn typed(key: &str, pattern: &str) -> Json {
    object(vec![
        ("type", string("object")),
        (
            "properties",
            object(vec![(
                key,
                object(vec![
                    ("type", string("string")),
                    ("pattern", string(pattern)),
                ]),
            )]),
        ),
        ("required", Json::Array(vec![string(key)])),
        ("additionalProperties", Json::Bool(false)),
    ])
}

const DATE: &str = "(?!0000)[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])";
const TIME: &str = "T([01][0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\\.[0-9]{1,6})?";
const OFFSET: &str = "(Z|[+-]((0[0-9]|1[0-7]):[0-5][0-9](:[0-5][0-9])?|18:00(:00)?)(\\[[A-Za-z0-9_+-]+(/[A-Za-z0-9_+-]+)*\\])?)";

impl Type {
    fn of(value: &Value) -> Option<Self> {
        Some(match value {
            Value::Bool(_) => Type::Boolean,
            Value::Int(_) | Value::Float(_) => Type::Number,
            Value::Decimal(_) => Type::Decimal,
            Value::String(_) => Type::String,
            Value::Date(_) => Type::Date,
            Value::NaiveDateTime(_) => Type::NaiveDatetime,
            Value::AwareDateTime(_) => Type::AwareDatetime,
            // Constants are scalars and never null where a type is taken.
            _ => return None,
        })
    }

    fn schema(self) -> Json {
        match self {
            Type::Boolean => object(vec![("type", string("boolean"))]),
            Type::Number => object(vec![("type", string("number"))]),
            Type::String => object(vec![("type", string("string"))]),
            Type::Decimal => typed(
                "$decimal",
                "^(-?[0-9]+(\\.[0-9]+)?([eE][+-]?[0-9]+)?|-?inf)$",
            ),
            Type::Date => typed("$date", &format!("^{DATE}$")),
            Type::NaiveDatetime => typed("$datetime", &format!("^{DATE}{TIME}$")),
            Type::AwareDatetime => typed("$datetime", &format!("^{DATE}{TIME}{OFFSET}$")),
        }
    }
}

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

fn object(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

fn int(n: u64) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

/// What a node at the other side of a comparison makes a field expect.
fn facing(other: &Node) -> Vec<Type> {
    match other.expr() {
        Expr::Const(value) => Type::of(value).into_iter().collect(),
        Expr::Arith { .. } | Expr::Abs(_) | Expr::Neg(_) => vec![Type::Number],
        _ => Vec::new(),
    }
}

/// The types each child of `node` expects, in the order of `children()`.
fn child_types(node: &Node, condition: &[Type]) -> Vec<Vec<Type>> {
    match node.expr() {
        Expr::Field(_) | Expr::Const(_) => Vec::new(),
        Expr::And(operands) | Expr::Or(operands) => vec![condition.to_vec(); operands.len()],
        Expr::Not(_) => vec![condition.to_vec()],
        Expr::Compare { left, right, .. } => vec![facing(right), facing(left)],
        Expr::Between {
            operand, low, high, ..
        } => {
            let mut both = facing(low);
            both.extend(facing(high));
            vec![both, facing(operand), facing(operand)]
        }
        Expr::In { values, .. } => vec![values.iter().filter_map(Type::of).collect()],
        Expr::Matches { .. } | Expr::StringTest { .. } => vec![vec![Type::String]],
        Expr::Arith { .. } => vec![vec![Type::Number]; 2],
        Expr::Abs(_) | Expr::Neg(_) => vec![vec![Type::Number]],
        Expr::IsNull(_) => vec![Vec::new()],
    }
}

/// The types each field expects, by path: every place that reads it, in the
/// summary's order (rules in order; condition, then payload values; pre-order).
fn field_types(summary: &Summary<'_>) -> BTreeMap<Vec<Segment>, Vec<Type>> {
    let ruleset = summary.ruleset;
    let condition = if ruleset.logic() == LogicName::Boolean {
        vec![Type::Boolean]
    } else {
        // Graded logics also take numbers as degrees (`spec/ast.md`).
        vec![Type::Boolean, Type::Number]
    };
    let mut found: BTreeMap<Vec<Segment>, Vec<Type>> = BTreeMap::new();
    for rule in ruleset.rules() {
        let roots = std::iter::once((rule.when(), condition.clone()))
            .chain(rule.then().iter().map(|(_, value)| (value, Vec::new())));
        for root in roots {
            // Pre-order, with an explicit stack: a node is as deep as its tree.
            let mut pending = vec![root];
            while let Some((node, types)) = pending.pop() {
                if let Expr::Field(path) = node.expr() {
                    let known = found.entry(path.clone()).or_default();
                    for t in types {
                        if !known.contains(&t) {
                            known.push(t);
                        }
                    }
                }
                let children = node.children();
                let expected = child_types(node, &condition);
                pending.extend(children.into_iter().zip(expected).rev());
            }
        }
    }
    found
}

/// A node of the tree that the fields' paths form.
#[derive(Default)]
struct Place {
    /// Children by key, in order of first appearance.
    keys: Vec<(String, usize)>,
    /// Children by index, in order of first appearance.
    indices: Vec<(u64, usize)>,
    /// The field that ends here: its position in the summary.
    field: Option<usize>,
    /// Whether a field read by a condition is here or below.
    condition: bool,
}

/// The tree of the fields' paths, in an arena: the root first, and a child
/// always after its parent.
fn tree(summary: &Summary<'_>) -> Vec<Place> {
    let mut tree = vec![Place::default()];
    let mut lookup: BTreeMap<(usize, Segment), usize> = BTreeMap::new();
    for (at, usage) in summary.fields.iter().enumerate() {
        let condition = !usage.conditions.is_empty();
        let mut here = 0;
        for segment in &usage.path {
            let next = tree.len();
            let child = *lookup.entry((here, segment.clone())).or_insert(next);
            if child == next {
                tree.push(Place::default());
                match segment {
                    Segment::Key(key) => tree[here].keys.push((key.clone(), child)),
                    Segment::Index(index) => tree[here].indices.push((*index, child)),
                }
            }
            tree[child].condition |= condition;
            here = child;
        }
        tree[here].field = Some(at);
    }
    tree
}

/// The map alternative of a place with keys; `schemas` holds its children's.
fn map(place: &Place, tree: &[Place], schemas: &mut [Option<Json>], strict: bool) -> Json {
    let mut properties = Vec::new();
    let mut required = Vec::new();
    for (key, child) in &place.keys {
        properties.push((key.clone(), schemas[*child].take().unwrap_or(Json::Null)));
        if strict && tree[*child].condition {
            required.push(string(key));
        }
    }
    let mut map = vec![
        ("type", string("object")),
        ("properties", Json::Object(properties)),
    ];
    if !required.is_empty() {
        map.push(("required", Json::Array(required)));
    }
    let map = object(map);
    if place.keys.iter().any(|(k, _)| k.starts_with('$')) {
        // A map with a `$` key is written in `$map` (`spec/json-format.md`).
        object(vec![
            ("type", string("object")),
            ("properties", object(vec![("$map", map)])),
            ("required", Json::Array(vec![string("$map")])),
            ("additionalProperties", Json::Bool(false)),
        ])
    } else {
        map
    }
}

/// The list alternative of a place with indices.
fn list(place: &Place, tree: &[Place], schemas: &mut [Option<Json>], strict: bool) -> Json {
    let largest = place.indices.iter().map(|(i, _)| *i).max().unwrap_or(0);
    let mut list = vec![("type", string("array"))];
    if largest < MAX_PREFIX_ITEMS {
        // `largest` is below MAX_PREFIX_ITEMS, so it fits a usize.
        let mut items = vec![Json::Object(Vec::new()); usize::try_from(largest + 1).unwrap_or(0)];
        for (index, child) in &place.indices {
            if let Some(item) = usize::try_from(*index).ok().and_then(|i| items.get_mut(i)) {
                *item = schemas[*child].take().unwrap_or(Json::Null);
            }
        }
        list.push(("prefixItems", Json::Array(items)));
    }
    let needed = place
        .indices
        .iter()
        .filter(|(_, child)| strict && tree[*child].condition)
        .map(|(i, _)| *i)
        .max();
    if let Some(needed) = needed {
        list.push(("minItems", int(needed.saturating_add(1))));
    }
    object(list)
}

/// The input schema of a rule set, from its summary (`spec/agents.md`, *Input
/// schemas*): the fields it reads, and the types its conditions and payloads
/// expect of them.
#[must_use]
pub fn input_schema(summary: &Summary<'_>) -> Json {
    let types = field_types(summary);
    let strict = summary.ruleset.strict();
    let tree = tree(summary);
    // Bottom up: children before their parents, without recursion.
    let mut schemas: Vec<Option<Json>> = vec![None; tree.len()];
    for at in (0..tree.len()).rev() {
        let place = &tree[at];
        let mut alternatives: Vec<Json> = Vec::new();
        let usage = place.field.map(|f| &summary.fields[f]);
        if let Some(usage) = usage
            && let Some(found) = types.get(&usage.path)
        {
            alternatives.extend(found.iter().map(|t| t.schema()));
        }
        if !place.keys.is_empty() {
            alternatives.push(map(place, &tree, &mut schemas, strict));
        }
        if !place.indices.is_empty() {
            alternatives.push(list(place, &tree, &mut schemas, strict));
        }
        let mut members: Vec<(String, Json)> = Vec::new();
        if at == 0 {
            members.push(("$schema".into(), string(DIALECT)));
            if alternatives.len() == 1
                && let Some(Json::Object(only)) = alternatives.first_mut()
            {
                members.append(only);
            } else if !alternatives.is_empty() {
                members.push(("anyOf".into(), Json::Array(alternatives)));
            }
        } else {
            if let Some(usage) = usage {
                members.push(("description".into(), string(&places(usage))));
            }
            if !alternatives.is_empty() {
                alternatives.push(object(vec![("type", string("null"))]));
                members.push(("anyOf".into(), Json::Array(alternatives)));
            }
        }
        schemas[at] = Some(Json::Object(members));
    }
    schemas[0].take().unwrap_or(Json::Null)
}
