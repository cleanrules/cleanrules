//! Condition nodes in JSON (`spec/json-format.md`, *Conditions*).
//!
//! Reading has two passes, as the error order requires: the shape is checked
//! depth first in pre-order (`format_error`, and the node depth), then the tree is
//! built in post-order with the construction rules of `spec/ast.md`
//! (`expression_error`).

use std::vec::Drain;

use crate::ast::{ArithOp, Closed, CompareOp, Expr, Node, StringOp};
use crate::error::{Error, ErrorCode, Result, Segment};
use crate::format::value::{ValueContext, read_value, write_value};
use crate::json::{Json, JsonNumber, Limits, pointer_token};
use crate::value::Value;
use crate::walk::{Walk, post_order};

/// A node whose shape is valid, not yet built: its own data, and its child
/// nodes in the order of [`Node::children`].
#[derive(Debug)]
pub(crate) struct Raw {
    pointer: String,
    shape: Shape,
    children: Vec<Raw>,
}

/// A path segment before construction: an index may still be negative.
#[derive(Debug)]
enum RawSegment {
    Key(String),
    Index(i64),
}

/// A node's own data; its child nodes are in [`Raw::children`].
#[derive(Debug)]
enum Shape {
    Field(Vec<RawSegment>),
    Const(Value),
    Compare(CompareOp),
    And,
    Or,
    Not,
    In(Vec<Value>),
    Between(Closed),
    IsNull,
    Matches(String),
    StringTest(StringOp, String),
    Arith(ArithOp),
    Abs,
    Neg,
}

fn format_error(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

fn child(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", pointer_token(key))
}

/// The members of a node object, with the keys already read.
struct Object<'a> {
    pointer: String,
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
                    &child(&self.pointer, key),
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
                &child(&self.pointer, key),
                format!("{key:?} must be a string"),
            )),
        }
    }

    /// After the known keys: an unknown key is an error, in text order;
    /// else the object's pointer.
    fn finish(self) -> Result<String> {
        match self
            .members
            .iter()
            .find(|(k, _)| !self.known.contains(&k.as_str()))
        {
            Some((key, _)) => Err(format_error(
                &child(&self.pointer, key),
                format!("unknown key {key:?}"),
            )
            .with_hint("it may come from a newer version of cleanrules")),
            None => Ok(self.pointer),
        }
    }
}

/// A node whose shape is being checked: its object, its depth, its own data
/// read so far, and where its child nodes are.
struct Checking<'j> {
    object: Object<'j>,
    depth: usize,
    shape: Shape,
    children: Children<'j>,
}

/// Where a node's child nodes are: under keys, read in this order, or the items
/// of `operands`, with that array's pointer.
enum Children<'j> {
    Keys(&'static [&'static str]),
    Items(&'j [Json], String),
}

/// Check the shape of a node at `pointer`, `depth` nodes deep: depth first in
/// pre-order, each node's keys in the order of its table, its child nodes
/// where their keys come, then its unknown keys (`spec/json-format.md`,
/// *Errors and limits*). The walk keeps its state on the heap (`crate::walk`),
/// so the call stack does not grow with the depth.
pub(crate) fn shape(json: &Json, pointer: &str, depth: usize, limits: &Limits) -> Result<Raw> {
    post_order(
        open(json, pointer.to_owned(), depth, limits)?,
        |checking, done| {
            let (json, at) = match &checking.children {
                Children::Keys(keys) => {
                    let Some(&key) = keys.get(done) else {
                        return Ok(None);
                    };
                    let at = child(&checking.object.pointer, key);
                    (checking.object.required(key)?, at)
                }
                &Children::Items(items, ref at) => match items.get(done) {
                    Some(item) => (item, format!("{at}/{done}")),
                    None => return Ok(None),
                },
            };
            open(json, at, checking.depth + 1, limits).map(Some)
        },
        close,
    )
}

/// The key of a node's one child node.
const OPERAND: &[&str] = &["operand"];
/// The keys of a node's two sides.
const SIDES: &[&str] = &["left", "right"];

/// Enter a node: its depth, that it is an object, its `op`, and the keys that
/// come before its child nodes.
fn open<'j>(
    json: &'j Json,
    pointer: String,
    depth: usize,
    limits: &Limits,
) -> Result<Checking<'j>> {
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
            &pointer,
            "a node must be an object with an \"op\"",
        ));
    };
    let mut object = Object {
        pointer,
        members,
        known: Vec::new(),
    };
    let op = object.string("op")?;
    let (shape, children) = match op {
        "field" => {
            let path = object.required("path")?;
            let at = child(&object.pointer, "path");
            (Shape::Field(read_path(path, &at)?), Children::Keys(&[]))
        }
        "const" => {
            let value = object.required("value")?;
            let at = child(&object.pointer, "value");
            (
                Shape::Const(read_value(value, &at, ValueContext::Constant)?),
                Children::Keys(&[]),
            )
        }
        "and" | "or" => {
            let at = child(&object.pointer, "operands");
            let Json::Array(items) = object.required("operands")? else {
                return Err(format_error(&at, "\"operands\" must be an array of nodes"));
            };
            let shape = if op == "and" { Shape::And } else { Shape::Or };
            (shape, Children::Items(items, at))
        }
        "not" => (Shape::Not, Children::Keys(OPERAND)),
        "in" => (Shape::In(Vec::new()), Children::Keys(OPERAND)),
        "between" => (
            Shape::Between(Closed::Both),
            Children::Keys(&["operand", "low", "high"]),
        ),
        "is_null" => (Shape::IsNull, Children::Keys(OPERAND)),
        "matches" => (Shape::Matches(String::new()), Children::Keys(OPERAND)),
        "abs" => (Shape::Abs, Children::Keys(OPERAND)),
        "neg" => (Shape::Neg, Children::Keys(OPERAND)),
        tag => {
            if let Some(op) = CompareOp::from_tag(tag) {
                (Shape::Compare(op), Children::Keys(SIDES))
            } else if let Some(op) = ArithOp::from_tag(tag) {
                (Shape::Arith(op), Children::Keys(SIDES))
            } else if let Some(op) = StringOp::from_tag(tag) {
                (
                    Shape::StringTest(op, String::new()),
                    Children::Keys(OPERAND),
                )
            } else {
                return Err(format_error(
                    &child(&object.pointer, "op"),
                    format!("unknown op {tag:?}"),
                )
                .with_hint("it may come from a newer version of cleanrules"));
            }
        }
    };
    Ok(Checking {
        object,
        depth,
        shape,
        children,
    })
}

/// Leave a node, its child nodes checked: the keys that come after them, then
/// its unknown keys.
fn close(checking: Checking<'_>, children: Drain<'_, Raw>) -> Result<Raw> {
    let Checking {
        mut object,
        mut shape,
        ..
    } = checking;
    match &mut shape {
        Shape::In(values) => {
            let at = child(&object.pointer, "values");
            let Json::Array(items) = object.required("values")? else {
                return Err(format_error(&at, "\"values\" must be an array of values"));
            };
            values.reserve_exact(items.len());
            for (i, item) in items.iter().enumerate() {
                values.push(read_value(
                    item,
                    &format!("{at}/{i}"),
                    ValueContext::Constant,
                )?);
            }
        }
        Shape::Between(closed) => {
            *closed = match object.optional("closed") {
                None => Closed::Both,
                Some(Json::String(name)) => Closed::from_name(name).ok_or_else(|| {
                    format_error(
                        &child(&object.pointer, "closed"),
                        r#""closed" is "both", "left", "right" or "none""#,
                    )
                })?,
                Some(_) => {
                    return Err(format_error(
                        &child(&object.pointer, "closed"),
                        "\"closed\" must be a string",
                    ));
                }
            };
        }
        Shape::Matches(pattern) => object.string("pattern")?.clone_into(pattern),
        Shape::StringTest(_, text) => object.string("text")?.clone_into(text),
        _ => {}
    }
    Ok(Raw {
        pointer: object.finish()?,
        shape,
        children: children.collect(),
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
/// errors: depth-first post-order, with an explicit stack (`crate::walk`).
pub(crate) fn build(raw: Raw) -> Result<Node> {
    let building = |raw: Raw| (raw.pointer, raw.shape, raw.children.into_iter());
    post_order(
        building(raw),
        |(_, _, children), _| Ok(children.next().map(building)),
        |(pointer, shape, _), nodes| construct(&pointer, shape, nodes),
    )
}

/// A node from its own data and its child nodes, built.
fn construct(pointer: &str, shape: Shape, mut nodes: Drain<'_, Node>) -> Result<Node> {
    let at = |error: Error| error.at_pointer(pointer);
    // `shape` read as many child nodes as its node has: one is never missing.
    let mut next = || {
        nodes
            .next()
            .ok_or_else(|| Error::new(ErrorCode::ExpressionError, "a node lacks an operand"))
    };
    match shape {
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
        Shape::Compare(op) => {
            let (left, right) = (next()?, next()?);
            Node::compare(op, left, right).map_err(at)
        }
        Shape::And => Node::and(nodes.collect()).map_err(at),
        Shape::Or => Node::or(nodes.collect()).map_err(at),
        Shape::Not => Node::not(next()?).map_err(at),
        Shape::In(values) => Node::is_in(next()?, values).map_err(at),
        Shape::Between(closed) => {
            let (operand, low, high) = (next()?, next()?, next()?);
            Node::between(operand, low, high, closed).map_err(at)
        }
        Shape::IsNull => Node::is_null(next()?).map_err(at),
        Shape::Matches(pattern) => Node::matches(next()?, &pattern).map_err(at),
        Shape::StringTest(op, text) => Node::string_test(op, next()?, text).map_err(at),
        Shape::Arith(op) => {
            let (left, right) = (next()?, next()?);
            Node::arith(op, left, right).map_err(at)
        }
        Shape::Abs => Node::abs(next()?).map_err(at),
        Shape::Neg => Node::neg(next()?).map_err(at),
    }
}

/// Write a node in canonical form: keys in table order, `op` first, `closed`
/// always written. Written with an explicit stack (`crate::walk`), so the
/// call stack does not grow with the node's depth.
#[must_use]
pub fn write_node(node: &Node) -> Json {
    write_node_in(&mut Walk::default(), node)
}

/// [`write_node`], with the walk's stacks of `walk`: a rule set writes every
/// node of its rules with the same ones.
pub(crate) fn write_node_in<'n>(walk: &mut Walk<&'n Node, Json>, node: &'n Node) -> Json {
    let written: std::result::Result<Json, std::convert::Infallible> = walk.run(
        node,
        |node, done| Ok(node.child(done)),
        |node, children| Ok(write_one(node, children)),
    );
    match written {
        Ok(json) => json,
        Err(never) => match never {},
    }
}

/// A node, given its children written in order.
fn write_one(node: &Node, mut children: impl Iterator<Item = Json>) -> Json {
    let mut members: Vec<(String, Json)> = vec![("op".into(), Json::String(node.tag().into()))];
    let mut put = |key: &str, json: Json| members.push((key.to_owned(), json));
    // The walk gives every child: a missing one cannot happen.
    let mut child = || children.next().unwrap_or(Json::Null);
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
        Expr::Compare { .. } | Expr::Arith { .. } => {
            put("left", child());
            put("right", child());
        }
        Expr::And(operands) | Expr::Or(operands) => {
            put(
                "operands",
                Json::Array((0..operands.len()).map(|_| child()).collect()),
            );
        }
        Expr::Not(_) | Expr::IsNull(_) | Expr::Abs(_) | Expr::Neg(_) => {
            put("operand", child());
        }
        Expr::In { values, .. } => {
            put("operand", child());
            put(
                "values",
                Json::Array(values.iter().map(write_value).collect()),
            );
        }
        Expr::Between { closed, .. } => {
            put("operand", child());
            put("low", child());
            put("high", child());
            put("closed", Json::String(closed.as_str().into()));
        }
        Expr::Matches { pattern, .. } => {
            put("operand", child());
            put("pattern", Json::String(pattern.source().into()));
        }
        Expr::StringTest { text, .. } => {
            put("operand", child());
            put("text", Json::String(text.clone()));
        }
    }
    Json::Object(members)
}
