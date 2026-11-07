//! Result trees in JSON (`spec/json-format.md`, *Result trees*).

use crate::error::Segment;
use crate::eval::{Evaluated, Outcome, Reason};
use crate::format::explain::{Out, reason, truth};
use crate::json::{Json, JsonNumber};
use crate::logic::Truth;
use crate::value::Value;
use crate::walk::post_order;

/// A field path as JSON: strings and integers.
#[must_use]
pub fn write_path(path: &[Segment]) -> Json {
    Json::Array(
        path.iter()
            .map(|segment| match segment {
                Segment::Key(key) => Json::String(key.clone()),
                Segment::Index(index) => Json::Number(JsonNumber::from_literal(index.to_string())),
            })
            .collect(),
    )
}

/// A reason as JSON, omitting empty `paths` and `types` and an absent `detail`.
#[must_use]
pub fn write_reason(reason: &Reason) -> Json {
    let mut members = vec![("code".to_owned(), Json::String(reason.code.as_str().into()))];
    if !reason.paths.is_empty() {
        members.push((
            "paths".into(),
            Json::Array(reason.paths.iter().map(|p| write_path(p)).collect()),
        ));
    }
    if !reason.types.is_empty() {
        let types = reason
            .types
            .iter()
            .map(|g| Json::String(g.as_str().into()))
            .collect();
        members.push(("types".into(), Json::Array(types)));
    }
    if let Some(detail) = &reason.detail {
        members.push(("detail".into(), Json::String(detail.clone())));
    }
    Json::Object(members)
}

/// A Kleene truth value: `true`, `false`, or `null` for unknown.
#[must_use]
pub fn write_truth(truth: Truth) -> Json {
    match truth {
        Truth::True => Json::Bool(true),
        Truth::False => Json::Bool(false),
        Truth::Unknown => Json::Null,
    }
}

/// A result and its children, in the structure of `spec/json-format.md`,
/// *Result trees*. Written with an explicit stack (`crate::walk`): a result
/// tree is as deep as its condition.
fn result<O: Out>(result: &Evaluated<'_, Truth>) -> O {
    let written: Result<O, std::convert::Infallible> = post_order(
        result,
        |result, done| Ok(result.children.get(done)),
        |result, children| Ok(one(result, children)),
    );
    match written {
        Ok(out) => out,
        Err(never) => match never {},
    }
}

/// A result, given its children written in order.
fn one<O: Out>(result: &Evaluated<'_, Truth>, children: impl ExactSizeIterator<Item = O>) -> O {
    let mut members = vec![("op", O::text(result.node.tag()))];
    if let crate::ast::Expr::Field(path) = result.node.expr() {
        members.push(("path", O::path(path)));
    }
    match &result.outcome {
        Outcome::Truth(found) => members.push(("truth", truth(*found))),
        Outcome::Value(value) => members.push(("value", O::data(value))),
    }
    if let Some(found) = &result.reason {
        members.push(("reason", reason(found)));
    }
    if children.len() > 0 {
        members.push(("children", O::list(children.collect())));
    }
    O::object(members)
}

/// A result tree under Kleene logic, in canonical form.
#[must_use]
pub fn write_result(result: &Evaluated<'_, Truth>) -> Json {
    self::result(result)
}

/// A result tree as a value (the C ABI's `cr_tree_value`), in the structure of
/// [`write_result`]'s JSON (`spec/json-format.md`, *Result trees*): a map per
/// result, a truth as a boolean or a plain null, paths as lists of strings and
/// ints, and the values themselves, opaque ones included. Writing it with
/// [`write_value`](crate::format::write_value) gives [`write_result`]'s JSON.
#[must_use]
pub fn result_value(result: &Evaluated<'_, Truth>) -> Value {
    self::result(result)
}

#[cfg(test)]
mod tests {
    use super::{result_value, write_result};
    use crate::eval::{Options, evaluate_tree, evaluate_value_tree};
    use crate::format::write_value;
    use crate::json::Limits;
    use crate::logic::Kleene;
    use crate::text::parse_node;
    use crate::value::{Map, Value};

    /// The value has the tree's JSON structure: written, it is the JSON.
    #[test]
    fn the_value_writes_as_the_json_tree() {
        let limits = Limits::default();
        let mut input = Map::new();
        input.insert("age", Value::Int(20));
        input.insert(
            "thing",
            Value::Opaque(crate::value::Opaque {
                type_name: "Thing".into(),
                token: 3,
            }),
        );
        input.insert(
            "price",
            Value::Decimal(crate::value::Decimal::parse("1.25").unwrap()),
        );
        let input = Value::Map(input);
        let options = || Options {
            strict: false,
            rule: None,
        };
        let condition = parse_node(
            "age >= 18 AND country IN (\"ES\") AND age BETWEEN 1 AND 30",
            &limits,
        )
        .unwrap();
        let tree = evaluate_tree(&condition, &input, &Kleene, options()).unwrap();
        assert_eq!(write_value(&result_value(&tree)), write_result(&tree));
        let Value::Map(root) = result_value(&tree) else {
            panic!("not a map")
        };
        assert!(matches!(root.get("truth"), Some(Value::Null(_))));
        let value = parse_node("abs(price * 2) + missing", &limits).unwrap();
        let tree = evaluate_value_tree(&value, &input, &Kleene, options()).unwrap();
        assert_eq!(write_value(&result_value(&tree)), write_result(&tree));
        // An opaque value is itself in the value.
        let value = parse_node("thing", &limits).unwrap();
        let tree = evaluate_value_tree(&value, &input, &Kleene, options()).unwrap();
        let Value::Map(root) = result_value(&tree) else {
            panic!("not a map")
        };
        assert!(matches!(root.get("value"), Some(Value::Opaque(o)) if o.token == 3));
    }
}
