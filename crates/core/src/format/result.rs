//! Result trees in JSON (`spec/json-format.md`, *Result trees*).

use crate::error::Segment;
use crate::eval::{AnyTree, Evaluated, Outcome, Reason};
use crate::format::explain::{Out, reason, truth};
use crate::json::{Json, JsonNumber};
use crate::logic::{Interval, Truth};
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

/// A graded truth value: a degree `[d, d]` as the number `d`, and an
/// undetermined interval as `[lo, hi]` (`spec/json-format.md`, *Result trees*).
#[must_use]
pub fn write_graded_truth(truth: Interval) -> Json {
    graded(truth)
}

fn graded<O: Out>(truth: Interval) -> O {
    let degree = |d: f64| O::data(&Value::Float(d));
    if truth.is_degree() {
        degree(truth.lo())
    } else {
        O::list(vec![degree(truth.lo()), degree(truth.hi())])
    }
}

/// A result and its children, in the structure of `spec/json-format.md`,
/// *Result trees*, with `truth` writing its truth values. Written with an
/// explicit stack (`crate::walk`): a result tree is as deep as its condition.
fn result<O: Out, T: Copy>(result: &Evaluated<'_, T>, truth: fn(T) -> O) -> O {
    let written: Result<O, std::convert::Infallible> = post_order(
        result,
        |result, done| Ok(result.children.get(done)),
        |result, children| Ok(one(result, truth, children)),
    );
    match written {
        Ok(out) => out,
        Err(never) => match never {},
    }
}

/// A result, given its children written in order.
fn one<O: Out, T: Copy>(
    result: &Evaluated<'_, T>,
    truth: fn(T) -> O,
    children: impl ExactSizeIterator<Item = O>,
) -> O {
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
    self::result(result, truth)
}

/// A result tree under a graded logic, in canonical form: each `truth` as
/// [`write_graded_truth`] writes it.
#[must_use]
pub fn write_graded_result(result: &Evaluated<'_, Interval>) -> Json {
    self::result(result, graded)
}

/// A result tree under a logic chosen at run time, in canonical form:
/// [`write_result`] or [`write_graded_result`].
#[must_use]
pub fn write_any_result(tree: &AnyTree<'_>) -> Json {
    match tree {
        AnyTree::Crisp(tree) => write_result(tree),
        AnyTree::Graded(tree) => write_graded_result(tree),
    }
}

/// [`write_any_result`]'s tree as a value: [`result_value`] or
/// [`graded_result_value`].
#[must_use]
pub fn any_result_value(tree: &AnyTree<'_>) -> Value {
    match tree {
        AnyTree::Crisp(tree) => result_value(tree),
        AnyTree::Graded(tree) => graded_result_value(tree),
    }
}

/// A result tree as a value (the C ABI's `cr_tree_value`), in the structure of
/// [`write_result`]'s JSON (`spec/json-format.md`, *Result trees*): a map per
/// result, a truth as a boolean or a plain null, paths as lists of strings and
/// ints, and the values themselves, opaque ones included. Writing it with
/// [`write_value`](crate::format::write_value) gives [`write_result`]'s JSON.
#[must_use]
pub fn result_value(result: &Evaluated<'_, Truth>) -> Value {
    self::result(result, truth)
}

/// [`result_value`] under a graded logic: a truth as a float, or a list of two
/// floats. Writing it with [`write_value`](crate::format::write_value) gives
/// [`write_graded_result`]'s JSON.
#[must_use]
pub fn graded_result_value(result: &Evaluated<'_, Interval>) -> Value {
    self::result(result, graded)
}

#[cfg(test)]
mod tests {
    use super::{
        any_result_value, graded_result_value, result_value, write_any_result, write_graded_result,
        write_graded_truth, write_result,
    };
    use crate::eval::{AnyTree, Options, evaluate_tree, evaluate_value_tree};
    use crate::format::write_value;
    use crate::json::{Limits, to_string};
    use crate::logic::{AnyLogic, Complement, Fuzzy, Interval, Kleene, Norm};
    use crate::text::parse_node;
    use crate::value::{Map, Value};

    /// The value has the tree's JSON structure: written, it is the JSON.
    /// Graded truth values are numbers and intervals, and the value form
    /// writes as the JSON form.
    #[test]
    fn graded_trees() {
        let limits = Limits::default();
        let mut input = Map::new();
        input.insert("a", Value::Float(0.5));
        input.insert("b", Value::Int(1));
        let input = Value::Map(input);
        let logic = Fuzzy::new(Norm::Prod, Complement::Standard);
        let condition = parse_node("a AND b AND NOT missing", &limits).unwrap();
        let tree = evaluate_tree(&condition, &input, &logic, Options::default()).unwrap();
        assert_eq!(
            write_value(&graded_result_value(&tree)),
            write_graded_result(&tree)
        );
        let json = to_string(&write_graded_result(&tree));
        assert!(
            json.starts_with(r#"{"op":"and","truth":[0.0,0.5],"reason":{"code":"missing_field""#),
            "{json}"
        );
        assert!(json.contains(r#""truth":1.0"#), "{json}");
        assert_eq!(
            to_string(&write_graded_truth(Interval::UNKNOWN)),
            "[0.0,1.0]"
        );
        assert_eq!(
            to_string(&write_graded_truth(Interval::degree(0.25).unwrap())),
            "0.25"
        );
    }

    /// A tree under a logic chosen at run time writes as the tree of that logic.
    #[test]
    fn trees_under_any_logic() {
        let limits = Limits::default();
        let mut input = Map::new();
        input.insert("a", Value::Float(0.5));
        let input = Value::Map(input);
        let condition = parse_node("a OR missing", &limits).unwrap();
        for name in ["boolean", "prod"] {
            let logic = AnyLogic::from_name(name).unwrap();
            let tree = logic
                .evaluate_tree(&condition, &input, Options::default())
                .unwrap();
            let json = write_any_result(&tree);
            assert_eq!(write_value(&any_result_value(&tree)), json);
            match (&tree, logic.as_fuzzy()) {
                (AnyTree::Crisp(crisp), None) => assert_eq!(json, write_result(crisp)),
                (AnyTree::Graded(graded), Some(_)) => {
                    assert_eq!(json, write_graded_result(graded));
                }
                _ => panic!("{name} gave the other kind of tree"),
            }
        }
    }

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
