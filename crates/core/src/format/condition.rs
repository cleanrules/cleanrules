//! Condition nodes in JSON (`spec/json-format.md`, *Conditions*).
//!
//! Reading has two passes, as the error order requires: the shape is checked
//! depth first in pre-order (`format_error`, and the node depth), then the tree is
//! built in post-order with the construction rules of `spec/ast.md`
//! (`expression_error`).

use crate::ast::{ArithOp, Closed, CompareOp, Expr, Node, StringOp};
use crate::error::{Error, ErrorCode, Result, Segment};
use crate::format::value::{ValueContext, read_value, write_value};
use crate::json::{Json, JsonNumber, Limits, pointer_token};
use crate::value::Value;

/// A node whose shape is valid, not yet built.
#[derive(Debug)]
pub(crate) struct Raw {
    pointer: String,
    shape: Shape,
}

/// A path segment before construction: an index may still be negative.
#[derive(Debug)]
enum RawSegment {
    Key(String),
    Index(i64),
}

#[derive(Debug)]
enum Shape {
    Field(Vec<RawSegment>),
    Const(Value),
    Compare(CompareOp, Box<Raw>, Box<Raw>),
    And(Vec<Raw>),
    Or(Vec<Raw>),
    Not(Box<Raw>),
    In(Box<Raw>, Vec<Value>),
    Between(Box<Raw>, Box<Raw>, Box<Raw>, Closed),
    IsNull(Box<Raw>),
    Matches(Box<Raw>, String),
    StringTest(StringOp, Box<Raw>, String),
    Arith(ArithOp, Box<Raw>, Box<Raw>),
    Abs(Box<Raw>),
    Neg(Box<Raw>),
}

fn format_error(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

fn child(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", pointer_token(key))
}

/// The members of a node object, with the keys already read.
struct Object<'a> {
    pointer: &'a str,
    members: &'a [(String, Json)],
    known: Vec<&'static str>,
}

impl<'a> Object<'a> {
    fn required(&mut self, key: &'static str) -> Result<&'a Json> {
        self.known.push(key);
        self.members
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .ok_or_else(|| {
                format_error(
                    &child(self.pointer, key),
                    format!("the key {key:?} is missing"),
                )
            })
    }

    fn optional(&mut self, key: &'static str) -> Option<&'a Json> {
        self.known.push(key);
        self.members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn string(&mut self, key: &'static str) -> Result<&'a str> {
        match self.required(key)? {
            Json::String(s) => Ok(s),
            _ => Err(format_error(
                &child(self.pointer, key),
                format!("{key:?} must be a string"),
            )),
        }
    }

    /// After the known keys: an unknown key is an error, in text order.
    fn finish(self) -> Result<()> {
        match self
            .members
            .iter()
            .find(|(k, _)| !self.known.contains(&k.as_str()))
        {
            Some((key, _)) => Err(format_error(
                &child(self.pointer, key),
                format!("unknown key {key:?}"),
            )
            .with_hint("it may come from a newer version of cleanrules")),
            None => Ok(()),
        }
    }
}

/// Check the shape of a node at `pointer`, `depth` nodes deep.
// One arm per row of the node table reads best as a single dispatch.
#[allow(clippy::too_many_lines)]
pub(crate) fn shape(json: &Json, pointer: &str, depth: usize, limits: &Limits) -> Result<Raw> {
    if depth > limits.max_depth {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            format!(
                "the condition is nested more than {} nodes deep",
                limits.max_depth
            ),
        )
        .at_pointer(pointer));
    }
    let Json::Object(members) = json else {
        return Err(format_error(
            pointer,
            "a node must be an object with an \"op\"",
        ));
    };
    let mut object = Object {
        pointer,
        members,
        known: Vec::new(),
    };
    let op = object.string("op")?;
    let node = |object: &mut Object<'_>, key: &'static str| -> Result<Box<Raw>> {
        let json = object.required(key)?;
        Ok(Box::new(shape(
            json,
            &child(pointer, key),
            depth + 1,
            limits,
        )?))
    };
    let shape = match op {
        "field" => Shape::Field(read_path(
            object.required("path")?,
            &child(pointer, "path"),
        )?),
        "const" => Shape::Const(read_value(
            object.required("value")?,
            &child(pointer, "value"),
            ValueContext::Constant,
        )?),
        "and" | "or" => {
            let at = child(pointer, "operands");
            let Json::Array(items) = object.required("operands")? else {
                return Err(format_error(&at, "\"operands\" must be an array of nodes"));
            };
            let mut operands = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                operands.push(shape(item, &format!("{at}/{i}"), depth + 1, limits)?);
            }
            if op == "and" {
                Shape::And(operands)
            } else {
                Shape::Or(operands)
            }
        }
        "not" => Shape::Not(node(&mut object, "operand")?),
        "in" => {
            let operand = node(&mut object, "operand")?;
            let at = child(pointer, "values");
            let Json::Array(items) = object.required("values")? else {
                return Err(format_error(&at, "\"values\" must be an array of values"));
            };
            let mut values = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                values.push(read_value(
                    item,
                    &format!("{at}/{i}"),
                    ValueContext::Constant,
                )?);
            }
            Shape::In(operand, values)
        }
        "between" => {
            let operand = node(&mut object, "operand")?;
            let low = node(&mut object, "low")?;
            let high = node(&mut object, "high")?;
            let closed = match object.optional("closed") {
                None => Closed::Both,
                Some(Json::String(name)) => Closed::from_name(name).ok_or_else(|| {
                    format_error(
                        &child(pointer, "closed"),
                        r#""closed" is "both", "left", "right" or "none""#,
                    )
                })?,
                Some(_) => {
                    return Err(format_error(
                        &child(pointer, "closed"),
                        "\"closed\" must be a string",
                    ));
                }
            };
            Shape::Between(operand, low, high, closed)
        }
        "is_null" => Shape::IsNull(node(&mut object, "operand")?),
        "matches" => {
            let operand = node(&mut object, "operand")?;
            Shape::Matches(operand, object.string("pattern")?.to_owned())
        }
        "abs" => Shape::Abs(node(&mut object, "operand")?),
        "neg" => Shape::Neg(node(&mut object, "operand")?),
        tag => {
            if let Some(op) = CompareOp::from_tag(tag) {
                let left = node(&mut object, "left")?;
                Shape::Compare(op, left, node(&mut object, "right")?)
            } else if let Some(op) = ArithOp::from_tag(tag) {
                let left = node(&mut object, "left")?;
                Shape::Arith(op, left, node(&mut object, "right")?)
            } else if let Some(op) = StringOp::from_tag(tag) {
                let operand = node(&mut object, "operand")?;
                Shape::StringTest(op, operand, object.string("text")?.to_owned())
            } else {
                return Err(
                    format_error(&child(pointer, "op"), format!("unknown op {tag:?}"))
                        .with_hint("it may come from a newer version of cleanrules"),
                );
            }
        }
    };
    object.finish()?;
    Ok(Raw {
        pointer: pointer.to_owned(),
        shape,
    })
}

/// A path: strings, and integer literals in the int range (negative ones are a
/// construction error, reported later).
fn read_path(json: &Json, pointer: &str) -> Result<Vec<RawSegment>> {
    let Json::Array(items) = json else {
        return Err(format_error(pointer, "\"path\" must be an array"));
    };
    let mut path = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let at = format!("{pointer}/{i}");
        match item {
            Json::String(key) => path.push(RawSegment::Key(key.clone())),
            Json::Number(n) if n.is_integer_literal() => {
                let index = n
                    .as_i128()
                    .and_then(|i| i64::try_from(i).ok())
                    .ok_or_else(|| {
                        format_error(&at, "a path index must be within the int range")
                    })?;
                path.push(RawSegment::Index(index));
            }
            _ => {
                return Err(format_error(
                    &at,
                    "a path segment is a string or an integer literal",
                ));
            }
        }
    }
    Ok(path)
}

/// Build the node, children first, adding the node's pointer to construction
/// errors.
pub(crate) fn build(raw: Raw) -> Result<Node> {
    let pointer = raw.pointer;
    let at = |error: Error| error.at_pointer(pointer.clone());
    let b = |raw: Box<Raw>| build(*raw);
    match raw.shape {
        Shape::Field(path) => {
            let mut segments = Vec::with_capacity(path.len());
            for segment in path {
                segments.push(match segment {
                    RawSegment::Key(key) => Segment::Key(key),
                    RawSegment::Index(index) => {
                        Segment::Index(u64::try_from(index).map_err(|_| {
                            at(Error::new(
                                ErrorCode::ExpressionError,
                                "a path index cannot be negative",
                            ))
                        })?)
                    }
                });
            }
            Node::field(segments).map_err(at)
        }
        Shape::Const(value) => Node::constant(value).map_err(at),
        Shape::Compare(op, left, right) => {
            let (left, right) = (b(left)?, b(right)?);
            Node::compare(op, left, right).map_err(at)
        }
        Shape::And(operands) => {
            let operands = build_all(operands)?;
            Node::and(operands).map_err(at)
        }
        Shape::Or(operands) => {
            let operands = build_all(operands)?;
            Node::or(operands).map_err(at)
        }
        Shape::Not(operand) => Node::not(b(operand)?).map_err(at),
        Shape::In(operand, values) => Node::is_in(b(operand)?, values).map_err(at),
        Shape::Between(operand, low, high, closed) => {
            let (operand, low, high) = (b(operand)?, b(low)?, b(high)?);
            Node::between(operand, low, high, closed).map_err(at)
        }
        Shape::IsNull(operand) => Node::is_null(b(operand)?).map_err(at),
        Shape::Matches(operand, pattern) => Node::matches(b(operand)?, &pattern).map_err(at),
        Shape::StringTest(op, operand, text) => {
            Node::string_test(op, b(operand)?, text).map_err(at)
        }
        Shape::Arith(op, left, right) => {
            let (left, right) = (b(left)?, b(right)?);
            Node::arith(op, left, right).map_err(at)
        }
        Shape::Abs(operand) => Node::abs(b(operand)?).map_err(at),
        Shape::Neg(operand) => Node::neg(b(operand)?).map_err(at),
    }
}

fn build_all(raws: Vec<Raw>) -> Result<Vec<Node>> {
    raws.into_iter().map(build).collect()
}

/// Write a node in canonical form: keys in table order, `op` first, `closed`
/// always written.
#[must_use]
pub fn write_node(node: &Node) -> Json {
    let mut members: Vec<(String, Json)> = vec![("op".into(), Json::String(node.tag().into()))];
    let mut put = |key: &str, json: Json| members.push((key.to_owned(), json));
    match node.expr() {
        Expr::Field(path) => put(
            "path",
            Json::Array(
                path.iter()
                    .map(|s| match s {
                        Segment::Key(k) => Json::String(k.clone()),
                        Segment::Index(i) => Json::Number(JsonNumber::from_literal(i.to_string())),
                    })
                    .collect(),
            ),
        ),
        Expr::Const(value) => put("value", write_value(value)),
        Expr::Compare { left, right, .. } | Expr::Arith { left, right, .. } => {
            put("left", write_node(left));
            put("right", write_node(right));
        }
        Expr::And(operands) | Expr::Or(operands) => {
            put(
                "operands",
                Json::Array(operands.iter().map(write_node).collect()),
            );
        }
        Expr::Not(operand) | Expr::IsNull(operand) | Expr::Abs(operand) | Expr::Neg(operand) => {
            put("operand", write_node(operand));
        }
        Expr::In { operand, values } => {
            put("operand", write_node(operand));
            put(
                "values",
                Json::Array(values.iter().map(write_value).collect()),
            );
        }
        Expr::Between {
            operand,
            low,
            high,
            closed,
        } => {
            put("operand", write_node(operand));
            put("low", write_node(low));
            put("high", write_node(high));
            put("closed", Json::String(closed.as_str().into()));
        }
        Expr::Matches { operand, pattern } => {
            put("operand", write_node(operand));
            put("pattern", Json::String(pattern.source().into()));
        }
        Expr::StringTest { operand, text, .. } => {
            put("operand", write_node(operand));
            put("text", Json::String(text.clone()));
        }
    }
    Json::Object(members)
}
