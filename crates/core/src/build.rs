//! Building conditions in Rust, idiomatically.
//!
//! Comparisons and tests are methods, and the connectives and arithmetic are
//! operators, as in cleanrules's other languages (`&`, `|`, `!`):
//!
//! ```
//! use cleanrules::build::field;
//!
//! let vip = (field("customer").get("years").gt(5) & field("order").get("total").gt(1000))
//!     .build()?;
//! # Ok::<(), cleanrules::Error>(())
//! ```
//!
//! A [`Build`] carries the first construction error, met from left to right, so
//! a whole expression is written without `?` and checked once by
//! [`Build::build`]. Rust cannot overload `<` and `>` to build nodes, hence
//! `gt`, `le` and the others.

use std::ops::{Add, BitAnd, BitOr, Div, Mul, Neg, Not, Sub};

use jiff::civil::{Date, DateTime};

use crate::ast::{ArithOp, Closed, CompareOp, Expr, Node, StringOp};
use crate::error::{Error, ErrorCode, Result, Segment};
use crate::value::{Decimal, Value};

/// A condition or value being built, or the first error met while building it.
#[derive(Clone, Debug)]
#[must_use = "a Build does nothing until `.build()` checks it"]
pub struct Build(Result<Node, BuildError>);

/// [`Error`] is not `Clone`; a build keeps what it needs to rebuild it.
#[derive(Clone, Debug)]
struct BuildError {
    code: ErrorCode,
    message: String,
    hint: Option<String>,
}

impl From<Error> for BuildError {
    fn from(error: Error) -> Self {
        Self {
            code: error.code(),
            message: error.message().to_owned(),
            hint: error.hint().map(str::to_owned),
        }
    }
}

impl From<BuildError> for Error {
    fn from(error: BuildError) -> Self {
        let built = Error::new(error.code, error.message);
        match error.hint {
            Some(hint) => built.with_hint(hint),
            None => built,
        }
    }
}

/// A field of the input, by its first key. Extend the path with
/// [`Build::get`] and [`Build::index`].
pub fn field(key: impl Into<String>) -> Build {
    Build::from_result(Node::field(vec![Segment::Key(key.into())]))
}

/// A constant.
pub fn constant(value: impl Into<Build>) -> Build {
    value.into()
}

impl Build {
    fn from_result(result: Result<Node>) -> Self {
        Build(result.map_err(BuildError::from))
    }

    /// Check the expression and return its node.
    ///
    /// # Errors
    ///
    /// The first construction error met, from left to right (`expression_error`).
    pub fn build(self) -> Result<Node> {
        self.0.map_err(Error::from)
    }

    fn with_path(self, segment: Segment) -> Build {
        match self.0 {
            Ok(node) => match node.expr() {
                Expr::Field(path) => {
                    let mut path = path.clone();
                    path.push(segment);
                    Build::from_result(Node::field(path))
                }
                _ => Build(Err(BuildError {
                    code: ErrorCode::ExpressionError,
                    message: format!("only a field has keys and indexes, not '{}'", node.tag()),
                    hint: None,
                })),
            },
            Err(error) => Build(Err(error)),
        }
    }

    /// The field one key further: `field("customer").get("age")`.
    pub fn get(self, key: impl Into<String>) -> Build {
        self.with_path(Segment::Key(key.into()))
    }

    /// The field one index further: `field("items").index(0)`.
    pub fn index(self, index: u64) -> Build {
        self.with_path(Segment::Index(index))
    }

    fn combine(self, other: Build, make: impl FnOnce(Node, Node) -> Result<Node>) -> Build {
        match (self.0, other.0) {
            (Ok(a), Ok(b)) => Build::from_result(make(a, b)),
            (Err(e), _) | (_, Err(e)) => Build(Err(e)),
        }
    }

    fn unary(self, make: impl FnOnce(Node) -> Result<Node>) -> Build {
        match self.0 {
            Ok(node) => Build::from_result(make(node)),
            Err(e) => Build(Err(e)),
        }
    }

    fn compare(self, op: CompareOp, other: impl Into<Build>) -> Build {
        self.combine(other.into(), |a, b| Node::compare(op, a, b))
    }

    /// `self == other`.
    pub fn eq(self, other: impl Into<Build>) -> Build {
        self.compare(CompareOp::Eq, other)
    }

    /// `self != other`.
    pub fn ne(self, other: impl Into<Build>) -> Build {
        self.compare(CompareOp::Ne, other)
    }

    /// `self < other`.
    pub fn lt(self, other: impl Into<Build>) -> Build {
        self.compare(CompareOp::Lt, other)
    }

    /// `self <= other`.
    pub fn le(self, other: impl Into<Build>) -> Build {
        self.compare(CompareOp::Le, other)
    }

    /// `self > other`.
    pub fn gt(self, other: impl Into<Build>) -> Build {
        self.compare(CompareOp::Gt, other)
    }

    /// `self >= other`.
    pub fn ge(self, other: impl Into<Build>) -> Build {
        self.compare(CompareOp::Ge, other)
    }

    /// `self in values`.
    pub fn is_in<V: Into<Value>>(self, values: impl IntoIterator<Item = V>) -> Build {
        let values: Vec<Value> = values.into_iter().map(Into::into).collect();
        self.unary(|node| Node::is_in(node, values))
    }

    /// `low <= self <= high`.
    pub fn between(self, low: impl Into<Build>, high: impl Into<Build>) -> Build {
        self.between_closed(low, high, Closed::Both)
    }

    /// `between` with the included ends chosen by `closed`.
    pub fn between_closed(
        self,
        low: impl Into<Build>,
        high: impl Into<Build>,
        closed: Closed,
    ) -> Build {
        match (self.0, low.into().0, high.into().0) {
            (Ok(x), Ok(l), Ok(h)) => Build::from_result(Node::between(x, l, h, closed)),
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Build(Err(e)),
        }
    }

    /// Whether the value is null or missing.
    pub fn is_null(self) -> Build {
        self.unary(Node::is_null)
    }

    /// Whether the regular expression matches anywhere in the string.
    pub fn matches(self, pattern: &str) -> Build {
        self.unary(|node| Node::matches(node, pattern))
    }

    /// Whether the string starts with `text`.
    pub fn starts_with(self, text: impl Into<String>) -> Build {
        self.unary(|node| Node::string_test(StringOp::StartsWith, node, text))
    }

    /// Whether the string ends with `text`.
    pub fn ends_with(self, text: impl Into<String>) -> Build {
        self.unary(|node| Node::string_test(StringOp::EndsWith, node, text))
    }

    /// Whether `text` occurs in the string.
    pub fn contains(self, text: impl Into<String>) -> Build {
        self.unary(|node| Node::string_test(StringOp::Contains, node, text))
    }

    /// The absolute value.
    pub fn abs(self) -> Build {
        self.unary(Node::abs)
    }
}

impl From<Node> for Build {
    fn from(node: Node) -> Self {
        Build(Ok(node))
    }
}

impl From<Value> for Build {
    fn from(value: Value) -> Self {
        Build::from_result(Node::constant(value))
    }
}

impl From<f64> for Build {
    fn from(value: f64) -> Self {
        if value.is_nan() {
            return Build(Err(BuildError {
                code: ErrorCode::ExpressionError,
                message: "NaN cannot be a constant".into(),
                hint: Some("NaN in the data is null: test it with is_null".into()),
            }));
        }
        Value::Float(value).into()
    }
}

macro_rules! constants {
    ($($t:ty => $variant:expr),* $(,)?) => {$(
        impl From<$t> for Build {
            fn from(value: $t) -> Self {
                Value::from(value).into()
            }
        }
        impl From<$t> for Value {
            fn from(value: $t) -> Self {
                #[allow(clippy::redundant_closure_call)]
                ($variant)(value)
            }
        }
    )*};
}

constants! {
    bool => Value::Bool,
    i64 => Value::Int,
    i32 => |v: i32| Value::Int(i64::from(v)),
    u32 => |v: u32| Value::Int(i64::from(v)),
    &str => |v: &str| Value::String(v.to_owned()),
    String => Value::String,
    Decimal => Value::Decimal,
    Date => Value::Date,
    DateTime => Value::NaiveDateTime,
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::float(value)
    }
}

impl BitAnd for Build {
    type Output = Build;
    fn bitand(self, other: Build) -> Build {
        self.combine(other, |a, b| Node::and(vec![a, b]))
    }
}

impl BitOr for Build {
    type Output = Build;
    fn bitor(self, other: Build) -> Build {
        self.combine(other, |a, b| Node::or(vec![a, b]))
    }
}

impl Not for Build {
    type Output = Build;
    fn not(self) -> Build {
        self.unary(Node::not)
    }
}

impl Neg for Build {
    type Output = Build;
    fn neg(self) -> Build {
        self.unary(Node::neg)
    }
}

macro_rules! arithmetic {
    ($($trait:ident $method:ident $op:expr),*) => {$(
        impl<T: Into<Build>> $trait<T> for Build {
            type Output = Build;
            fn $method(self, other: T) -> Build {
                self.combine(other.into(), |a, b| Node::arith($op, a, b))
            }
        }
        impl $trait<Build> for i64 {
            type Output = Build;
            fn $method(self, other: Build) -> Build {
                Build::from(self).combine(other, |a, b| Node::arith($op, a, b))
            }
        }
        impl $trait<Build> for f64 {
            type Output = Build;
            fn $method(self, other: Build) -> Build {
                Build::from(self).combine(other, |a, b| Node::arith($op, a, b))
            }
        }
    )*};
}

arithmetic!(Add add ArithOp::Add, Sub sub ArithOp::Sub, Mul mul ArithOp::Mul, Div div ArithOp::Div);

#[cfg(test)]
mod tests {
    use super::{constant, field};
    use crate::ast::{Closed, CompareOp, Expr, Node};
    use crate::error::{ErrorCode, Segment};
    use crate::format::read_document;
    use crate::json::Limits;
    use crate::value::{Decimal, Value};

    fn read(text: &str) -> Node {
        read_document(text, &Limits::default())
            .unwrap()
            .into_condition()
            .unwrap()
    }

    #[test]
    fn builds_the_same_tree_as_the_json() {
        let built = (field("customer").get("years").gt(5) & field("order").get("total").gt(1000))
            .build()
            .unwrap();
        let json = read(
            r#"{"version":1,"condition":{"op":"and","operands":[
                {"op":"gt","left":{"op":"field","path":["customer","years"]},"right":{"op":"const","value":5}},
                {"op":"gt","left":{"op":"field","path":["order","total"]},"right":{"op":"const","value":1000}}]}}"#,
        );
        assert_eq!(built, json);
    }

    #[test]
    fn every_node() {
        let d = Decimal::parse("0.10").unwrap();
        let tree = (field("c").is_in(["ES", "FR"])
            | field("age").between_closed(18, 65, Closed::Left)
            | field("email").matches("(?i)@example\\.com$")
            | field("code").starts_with("ES-")
            | !field("vip").eq(true)
            | ((field("items").index(0).get("price") + 1.5) / d).lt(100)
            | (-field("delta")).abs().ge(f64::INFINITY)
            | field("x").is_null()
            | (2 * field("y")).ne(4))
        .build()
        .unwrap();
        let Expr::Or(operands) = tree.expr() else {
            panic!()
        };
        assert_eq!(operands.len(), 9);
        let Expr::Compare {
            op: CompareOp::Lt,
            left,
            ..
        } = operands[5].expr()
        else {
            panic!()
        };
        assert_eq!(left.tag(), "div");
        assert_eq!(
            tree.field_paths()[5],
            [
                Segment::Key("items".into()),
                Segment::Index(0),
                Segment::Key("price".into())
            ]
        );
    }

    #[test]
    fn the_first_error_wins() {
        let error = (field("a").gt(constant(f64::NAN)) & field("b").is_in(Vec::<Value>::new()))
            .build()
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::ExpressionError);
        assert!(error.message().contains("NaN"));
        let error = (field("a").gt(1) & field("a").gt(1).get("x"))
            .build()
            .unwrap_err();
        assert!(error.message().contains("only a field"));
        assert!((field("a") + "x").gt(1).build().is_err());
        assert!((field("a").gt(1) & (field("b") + 1)).build().is_err());
    }

    #[test]
    fn an_error_passes_through_every_builder() {
        let bad = || constant(f64::NAN);
        for build in [
            bad().get("x"),
            !bad(),
            bad().between(1, 2),
            field("a").between(bad(), 2),
            field("a").between(1, bad()),
            bad().ends_with("x"),
            bad().contains("x"),
            bad().abs(),
            1.5 - bad(),
        ] {
            let error = build.build().unwrap_err();
            assert_eq!(error.code(), ErrorCode::ExpressionError);
            assert!(error.hint().unwrap().contains("is_null"));
        }
    }

    #[test]
    fn the_remaining_builders() {
        let tree = (field("a").le(1)
            | field("s").ends_with(".pdf")
            | field("s").contains("@")
            | (field("x").abs() * 2.5).gt(1)
            | field("n").eq(7_u32))
        .build()
        .unwrap();
        let tags: Vec<&str> = tree.children().iter().map(|c| c.tag()).collect();
        assert_eq!(tags, ["le", "ends_with", "contains", "gt", "eq"]);
        assert!(matches!(Value::from(0.5), Value::Float(x) if x.to_bits() == 0.5_f64.to_bits()));
        assert!(matches!(Value::from(7_u32), Value::Int(7)));
    }
}
