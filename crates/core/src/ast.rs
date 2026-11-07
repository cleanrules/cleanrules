//! The condition AST (`spec/ast.md`): immutable nodes that can only be built
//! through constructors applying the construction rules, so no tree in memory
//! breaks them.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use crate::error::{Error, ErrorCode, Result, Segment};
use crate::value::{Group, NullCause, Value};
use crate::walk::post_order;

/// Where a node may appear (`spec/ast.md`, *Node kinds*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Evaluates to a truth value.
    Condition,
    /// Evaluates to a value.
    Value,
    /// Depends on the data: fields and boolean constants.
    Any,
}

/// `eq`, `ne`, `lt`, `le`, `gt`, `ge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareOp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// `add`, `sub`, `mul`, `div`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArithOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`, always in float64.
    Div,
}

/// `starts_with`, `ends_with`, `contains`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StringOp {
    /// The text is a prefix.
    StartsWith,
    /// The text is a suffix.
    EndsWith,
    /// The text occurs anywhere.
    Contains,
}

/// Which ends of a `between` are included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Closed {
    /// `low <= x <= high`
    #[default]
    Both,
    /// `low <= x < high`
    Left,
    /// `low < x <= high`
    Right,
    /// `low < x < high`
    Neither,
}

impl CompareOp {
    /// The tag.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            CompareOp::Eq => "eq",
            CompareOp::Ne => "ne",
            CompareOp::Lt => "lt",
            CompareOp::Le => "le",
            CompareOp::Gt => "gt",
            CompareOp::Ge => "ge",
        }
    }

    /// The operator for a tag.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "eq" => CompareOp::Eq,
            "ne" => CompareOp::Ne,
            "lt" => CompareOp::Lt,
            "le" => CompareOp::Le,
            "gt" => CompareOp::Gt,
            "ge" => CompareOp::Ge,
            _ => return None,
        })
    }
}

impl ArithOp {
    /// The tag.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            ArithOp::Add => "add",
            ArithOp::Sub => "sub",
            ArithOp::Mul => "mul",
            ArithOp::Div => "div",
        }
    }

    /// The operator for a tag.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "add" => ArithOp::Add,
            "sub" => ArithOp::Sub,
            "mul" => ArithOp::Mul,
            "div" => ArithOp::Div,
            _ => return None,
        })
    }
}

impl StringOp {
    /// The tag.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            StringOp::StartsWith => "starts_with",
            StringOp::EndsWith => "ends_with",
            StringOp::Contains => "contains",
        }
    }

    /// The operator for a tag.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Some(match tag {
            "starts_with" => StringOp::StartsWith,
            "ends_with" => StringOp::EndsWith,
            "contains" => StringOp::Contains,
            _ => return None,
        })
    }
}

impl Closed {
    /// The name in the format: `both`, `left`, `right` or `none`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Closed::Both => "both",
            Closed::Left => "left",
            Closed::Right => "right",
            Closed::Neither => "none",
        }
    }

    /// The value for a name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "both" => Closed::Both,
            "left" => Closed::Left,
            "right" => Closed::Right,
            "none" => Closed::Neither,
            _ => return None,
        })
    }
}

/// A compiled regular expression, identified by its source text.
#[derive(Clone, Debug)]
pub struct Pattern {
    source: String,
    regex: regex::Regex,
}

impl Pattern {
    /// The pattern as written.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The compiled expression.
    #[must_use]
    pub fn regex(&self) -> &regex::Regex {
        &self.regex
    }
}

/// The shape of a node, for reading. Build nodes with [`Node`]'s constructors.
#[derive(Clone, Debug)]
pub enum Expr {
    /// A field path.
    Field(Vec<Segment>),
    /// A scalar constant; null only under `is_null`.
    Const(Value),
    /// A comparison.
    Compare {
        /// The operator.
        op: CompareOp,
        /// The left side.
        left: Box<Node>,
        /// The right side.
        right: Box<Node>,
    },
    /// Conjunction of two or more conditions, flattened.
    And(Vec<Node>),
    /// Disjunction of two or more conditions, flattened.
    Or(Vec<Node>),
    /// Negation.
    Not(Box<Node>),
    /// Membership in a set of values of one group, sorted and deduplicated.
    In {
        /// The value tested.
        operand: Box<Node>,
        /// The values, in normalized order.
        values: Vec<Value>,
    },
    /// A range test.
    Between {
        /// The value tested.
        operand: Box<Node>,
        /// The lower bound.
        low: Box<Node>,
        /// The upper bound.
        high: Box<Node>,
        /// Which ends are included.
        closed: Closed,
    },
    /// Whether a value is null.
    IsNull(Box<Node>),
    /// A regular expression search.
    Matches {
        /// The string tested.
        operand: Box<Node>,
        /// The pattern.
        pattern: Pattern,
    },
    /// A literal string test.
    StringTest {
        /// The test.
        op: StringOp,
        /// The string tested.
        operand: Box<Node>,
        /// The literal text.
        text: String,
    },
    /// Arithmetic on two numbers.
    Arith {
        /// The operator.
        op: ArithOp,
        /// The left operand.
        left: Box<Node>,
        /// The right operand.
        right: Box<Node>,
    },
    /// Absolute value.
    Abs(Box<Node>),
    /// Negation of a number.
    Neg(Box<Node>),
}

/// A node of a condition tree, valid by construction.
#[derive(Debug)]
pub struct Node {
    expr: Expr,
}

/// Copied with an explicit stack (`crate::walk`): a tree as deep as the
/// reader's limits allow needs no more call stack than a leaf.
impl Clone for Node {
    fn clone(&self) -> Self {
        let copied: std::result::Result<Node, std::convert::Infallible> = post_order(
            self,
            |node, done| Ok(node.child(done)),
            |node, children| Ok(node.with_children(children)),
        );
        match copied {
            Ok(node) => node,
            Err(never) => match never {},
        }
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorCode::ExpressionError, message)
}

impl Node {
    /// The node's shape.
    #[must_use]
    pub fn expr(&self) -> &Expr {
        &self.expr
    }

    /// The node's tag (`spec/ast.md`, *Catalogue*).
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match &self.expr {
            Expr::Field(_) => "field",
            Expr::Const(_) => "const",
            Expr::Compare { op, .. } => op.tag(),
            Expr::And(_) => "and",
            Expr::Or(_) => "or",
            Expr::Not(_) => "not",
            Expr::In { .. } => "in",
            Expr::Between { .. } => "between",
            Expr::IsNull(_) => "is_null",
            Expr::Matches { .. } => "matches",
            Expr::StringTest { op, .. } => op.tag(),
            Expr::Arith { op, .. } => op.tag(),
            Expr::Abs(_) => "abs",
            Expr::Neg(_) => "neg",
        }
    }

    /// Where the node may appear.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match &self.expr {
            Expr::Field(_) | Expr::Const(Value::Bool(_)) => Kind::Any,
            Expr::Const(_) | Expr::Arith { .. } | Expr::Abs(_) | Expr::Neg(_) => Kind::Value,
            _ => Kind::Condition,
        }
    }

    /// Whether the node is the null constant.
    #[must_use]
    pub fn is_null_constant(&self) -> bool {
        matches!(self.expr, Expr::Const(Value::Null(_)))
    }

    /// A field. Each segment is a non-empty key or an index.
    ///
    /// # Errors
    ///
    /// `expression_error` for an empty path or an empty key.
    pub fn field(path: Vec<Segment>) -> Result<Self> {
        if path.is_empty() {
            return Err(invalid("a field path cannot be empty"));
        }
        if path
            .iter()
            .any(|s| matches!(s, Segment::Key(k) if k.is_empty()))
        {
            return Err(invalid("a field path segment cannot be an empty string"));
        }
        Ok(Self {
            expr: Expr::Field(path),
        })
    }

    /// A constant: null, a boolean, a number, a string, a date or a datetime.
    ///
    /// # Errors
    ///
    /// `expression_error` for a list, a map, an opaque value, or a null that
    /// carries an overflow or read error.
    pub fn constant(value: Value) -> Result<Self> {
        match &value {
            Value::List(_) | Value::Map(_) | Value::Opaque(_) => {
                Err(invalid("a constant must be a scalar"))
            }
            Value::Null(cause) if *cause != NullCause::Null => Err(invalid(
                "a constant cannot be an overflowing or unreadable value",
            )),
            _ => Ok(Self {
                expr: Expr::Const(value),
            }),
        }
    }

    /// `left op right`.
    ///
    /// # Errors
    ///
    /// `expression_error` when a side is a condition or the null constant.
    pub fn compare(op: CompareOp, left: Node, right: Node) -> Result<Self> {
        value_operand(&left, op.tag())?;
        value_operand(&right, op.tag())?;
        Ok(Self {
            expr: Expr::Compare {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// The conjunction of `operands`, at least two, flattened.
    ///
    /// # Errors
    ///
    /// `expression_error` for fewer than two operands, or an operand that is a
    /// value or the null constant.
    pub fn and(operands: Vec<Node>) -> Result<Self> {
        Ok(Self {
            expr: Expr::And(connective(operands, "and", |n| match n.expr {
                Expr::And(inner) => Ok(inner),
                other => Err(Node { expr: other }),
            })?),
        })
    }

    /// The disjunction of `operands`, at least two, flattened.
    ///
    /// # Errors
    ///
    /// As [`Node::and`].
    pub fn or(operands: Vec<Node>) -> Result<Self> {
        Ok(Self {
            expr: Expr::Or(connective(operands, "or", |n| match n.expr {
                Expr::Or(inner) => Ok(inner),
                other => Err(Node { expr: other }),
            })?),
        })
    }

    /// The negation of a condition.
    ///
    /// # Errors
    ///
    /// `expression_error` when `operand` is a value or the null constant.
    // Named after the tag; a fallible constructor cannot be `std::ops::Not`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(operand: Node) -> Result<Self> {
        condition_operand(&operand, "not")?;
        Ok(Self {
            expr: Expr::Not(Box::new(operand)),
        })
    }

    /// `operand in values`: the values are sorted and deduplicated.
    ///
    /// # Errors
    ///
    /// `expression_error` when the operand is a condition or the null constant,
    /// or `values` is empty, holds a null or mixes type groups.
    pub fn is_in(operand: Node, values: Vec<Value>) -> Result<Self> {
        value_operand(&operand, "in")?;
        if values.is_empty() {
            return Err(invalid("'in' needs at least one value"));
        }
        let mut groups = Vec::new();
        for value in &values {
            let constant = Node::constant(value.clone())?;
            if constant.is_null_constant() {
                return Err(invalid(
                    "'in' values cannot contain null; combine with is_null",
                ));
            }
            if let Some(group) = value.group()
                && !groups.contains(&group)
            {
                groups.push(group);
            }
        }
        if groups.len() > 1 {
            let names: Vec<&str> = groups.iter().map(|g| g.as_str()).collect();
            return Err(invalid(format!(
                "'in' values mix type groups: {}",
                names.join(", ")
            )));
        }
        Ok(Self {
            expr: Expr::In {
                operand: Box::new(operand),
                values: normalize_in(values),
            },
        })
    }

    /// `low <= operand <= high`, with `closed` choosing the included ends.
    ///
    /// # Errors
    ///
    /// `expression_error` when a node is a condition or the null constant, or both
    /// bounds are constants that are not of one orderable group.
    pub fn between(operand: Node, low: Node, high: Node, closed: Closed) -> Result<Self> {
        for node in [&operand, &low, &high] {
            value_operand(node, "between")?;
        }
        if let (Expr::Const(a), Expr::Const(b)) = (&low.expr, &high.expr) {
            let (ga, gb) = (a.group(), b.group());
            if ga != gb || matches!(ga, Some(Group::Boolean | Group::Other) | None) {
                return Err(invalid(
                    "'between' constant bounds must be of one orderable group",
                ));
            }
        }
        Ok(Self {
            expr: Expr::Between {
                operand: Box::new(operand),
                low: Box::new(low),
                high: Box::new(high),
                closed,
            },
        })
    }

    /// Whether `operand` is null; the only place for the null constant.
    ///
    /// # Errors
    ///
    /// `expression_error` when `operand` is a condition.
    pub fn is_null(operand: Node) -> Result<Self> {
        if !operand.is_null_constant() {
            value_operand(&operand, "is_null")?;
        }
        Ok(Self {
            expr: Expr::IsNull(Box::new(operand)),
        })
    }

    /// A regular expression search, in the syntax of the `regex` crate.
    ///
    /// # Errors
    ///
    /// `expression_error` when the operand is a condition or the null constant,
    /// or the pattern does not compile within the default size limit.
    pub fn matches(operand: Node, pattern: &str) -> Result<Self> {
        value_operand(&operand, "matches")?;
        let regex = regex::Regex::new(pattern).map_err(|e| {
            invalid(format!("the pattern {pattern:?} is not valid: {e}")).with_hint(
                "patterns use the syntax of Rust's regex crate: no lookaround, no backreferences",
            )
        })?;
        Ok(Self {
            expr: Expr::Matches {
                operand: Box::new(operand),
                pattern: Pattern {
                    source: pattern.to_owned(),
                    regex,
                },
            },
        })
    }

    /// A literal string test.
    ///
    /// # Errors
    ///
    /// `expression_error` when the operand is a condition or the null constant.
    pub fn string_test(op: StringOp, operand: Node, text: impl Into<String>) -> Result<Self> {
        value_operand(&operand, op.tag())?;
        Ok(Self {
            expr: Expr::StringTest {
                op,
                operand: Box::new(operand),
                text: text.into(),
            },
        })
    }

    /// Arithmetic on two numbers.
    ///
    /// # Errors
    ///
    /// `expression_error` when an operand is a condition, the null constant or a
    /// constant that is not a number.
    pub fn arith(op: ArithOp, left: Node, right: Node) -> Result<Self> {
        numeric_operand(&left, op.tag())?;
        numeric_operand(&right, op.tag())?;
        Ok(Self {
            expr: Expr::Arith {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// The absolute value of a number.
    ///
    /// # Errors
    ///
    /// As [`Node::arith`].
    pub fn abs(operand: Node) -> Result<Self> {
        numeric_operand(&operand, "abs")?;
        Ok(Self {
            expr: Expr::Abs(Box::new(operand)),
        })
    }

    /// The negation of a number.
    ///
    /// # Errors
    ///
    /// As [`Node::arith`].
    // Named after the tag; a fallible constructor cannot be `std::ops::Neg`.
    #[allow(clippy::should_implement_trait)]
    pub fn neg(operand: Node) -> Result<Self> {
        numeric_operand(&operand, "neg")?;
        Ok(Self {
            expr: Expr::Neg(Box::new(operand)),
        })
    }

    /// The `index`-th direct child, in the order of [`Node::children`], without
    /// collecting them: the step of the core's iterative walks.
    pub(crate) fn child(&self, index: usize) -> Option<&Node> {
        let child: &Node = match (&self.expr, index) {
            (Expr::And(operands) | Expr::Or(operands), _) => return operands.get(index),
            (
                Expr::Compare { left, .. }
                | Expr::Arith { left, .. }
                | Expr::Not(left)
                | Expr::IsNull(left)
                | Expr::Abs(left)
                | Expr::Neg(left)
                | Expr::In { operand: left, .. }
                | Expr::Matches { operand: left, .. }
                | Expr::StringTest { operand: left, .. }
                | Expr::Between { operand: left, .. },
                0,
            ) => left,
            (
                Expr::Compare { right, .. }
                | Expr::Arith { right, .. }
                | Expr::Between { low: right, .. },
                1,
            )
            | (Expr::Between { high: right, .. }, 2) => right,
            _ => return None,
        };
        Some(child)
    }

    /// A copy of this node with `children`, copies of its own, in order, in
    /// their place (the step of [`Clone`]). A child missing from `children` is
    /// copied from this node's.
    fn with_children(&self, mut children: impl Iterator<Item = Node>) -> Node {
        let mut take = |own: &Node| Box::new(children.next().unwrap_or_else(|| own.clone()));
        let expr = match &self.expr {
            Expr::Field(path) => Expr::Field(path.clone()),
            Expr::Const(value) => Expr::Const(value.clone()),
            Expr::Compare { op, left, right } => Expr::Compare {
                op: *op,
                left: take(left),
                right: take(right),
            },
            Expr::And(operands) => Expr::And(operands.iter().map(|o| *take(o)).collect()),
            Expr::Or(operands) => Expr::Or(operands.iter().map(|o| *take(o)).collect()),
            Expr::Not(operand) => Expr::Not(take(operand)),
            Expr::In { operand, values } => Expr::In {
                operand: take(operand),
                values: values.clone(),
            },
            Expr::Between {
                operand,
                low,
                high,
                closed,
            } => Expr::Between {
                operand: take(operand),
                low: take(low),
                high: take(high),
                closed: *closed,
            },
            Expr::IsNull(operand) => Expr::IsNull(take(operand)),
            Expr::Matches { operand, pattern } => Expr::Matches {
                operand: take(operand),
                pattern: pattern.clone(),
            },
            Expr::StringTest { op, operand, text } => Expr::StringTest {
                op: *op,
                operand: take(operand),
                text: text.clone(),
            },
            Expr::Arith { op, left, right } => Expr::Arith {
                op: *op,
                left: take(left),
                right: take(right),
            },
            Expr::Abs(operand) => Expr::Abs(take(operand)),
            Expr::Neg(operand) => Expr::Neg(take(operand)),
        };
        Node { expr }
    }

    /// Whether two nodes are the same apart from their children: tag and own
    /// data (the step of structural identity).
    fn same_own(&self, other: &Node) -> bool {
        match (&self.expr, &other.expr) {
            (Expr::Field(a), Expr::Field(b)) => a == b,
            (Expr::Const(a), Expr::Const(b)) => same_constant(a, b),
            (Expr::In { values: va, .. }, Expr::In { values: vb, .. }) => {
                va.len() == vb.len() && va.iter().zip(vb).all(|(x, y)| same_constant(x, y))
            }
            (Expr::Between { closed: a, .. }, Expr::Between { closed: b, .. }) => a == b,
            (Expr::Matches { pattern: a, .. }, Expr::Matches { pattern: b, .. }) => {
                a.source == b.source
            }
            (Expr::StringTest { text: a, .. }, Expr::StringTest { text: b, .. }) => {
                self.tag() == other.tag() && a == b
            }
            _ => self.tag() == other.tag(),
        }
    }

    /// The direct child nodes, in field order.
    #[must_use]
    pub fn children(&self) -> Vec<&Node> {
        match &self.expr {
            Expr::Field(_) | Expr::Const(_) => vec![],
            Expr::Compare { left, right, .. } | Expr::Arith { left, right, .. } => {
                vec![left, right]
            }
            Expr::And(operands) | Expr::Or(operands) => operands.iter().collect(),
            Expr::Not(operand)
            | Expr::IsNull(operand)
            | Expr::Abs(operand)
            | Expr::Neg(operand)
            | Expr::In { operand, .. }
            | Expr::Matches { operand, .. }
            | Expr::StringTest { operand, .. } => vec![operand],
            Expr::Between {
                operand, low, high, ..
            } => vec![operand, low, high],
        }
    }

    /// The paths of every field the node reads, in depth-first pre-order, each
    /// once (`spec/explain.md`, *Summaries*).
    #[must_use]
    pub fn field_paths(&self) -> Vec<&[Segment]> {
        let mut paths: Vec<&[Segment]> = Vec::new();
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            if let Expr::Field(path) = &node.expr
                && !paths.contains(&path.as_slice())
            {
                paths.push(path);
            }
            stack.extend(node.children().into_iter().rev());
        }
        paths
    }
}

fn value_operand(node: &Node, of: &str) -> Result<()> {
    if node.is_null_constant() {
        return Err(
            invalid(format!("a null constant cannot be an operand of '{of}'"))
                .with_hint("test for missing values with is_null")
                .with_node(node),
        );
    }
    if node.kind() == Kind::Condition {
        return Err(invalid(format!(
            "a condition ('{}') cannot be an operand of '{of}'",
            node.tag()
        ))
        .with_node(node));
    }
    Ok(())
}

fn numeric_operand(node: &Node, of: &str) -> Result<()> {
    value_operand(node, of)?;
    if let Expr::Const(value) = &node.expr
        && value.as_number().is_none()
    {
        return Err(
            invalid(format!("'{of}' needs numbers; this constant is not one")).with_node(node),
        );
    }
    Ok(())
}

fn condition_operand(node: &Node, of: &str) -> Result<()> {
    if node.is_null_constant() {
        return Err(
            invalid(format!("a null constant cannot be an operand of '{of}'"))
                .with_hint("test for missing values with is_null")
                .with_node(node),
        );
    }
    if node.kind() == Kind::Value {
        return Err(invalid(format!(
            "a value ('{}') cannot be an operand of '{of}'",
            node.tag()
        ))
        .with_node(node));
    }
    Ok(())
}

/// Check the operands of `and` / `or` as written, then flatten same-kind nesting.
fn connective(
    operands: Vec<Node>,
    tag: &str,
    unwrap: impl Fn(Node) -> std::result::Result<Vec<Node>, Node>,
) -> Result<Vec<Node>> {
    if operands.len() < 2 {
        return Err(invalid(format!("'{tag}' needs at least two operands")));
    }
    for operand in &operands {
        condition_operand(operand, tag)?;
    }
    let mut flat = Vec::with_capacity(operands.len());
    for operand in operands {
        match unwrap(operand) {
            Ok(inner) => flat.extend(inner),
            Err(node) => flat.push(node),
        }
    }
    Ok(flat)
}

/// Sort `in` values ascending, numbers of equal value as decimal, float, int, and
/// keep the first of values that are the same constant (`spec/json-format.md`,
/// *Writing*).
fn normalize_in(mut values: Vec<Value>) -> Vec<Value> {
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Decimal(_) => 0,
            Value::Float(_) => 1,
            _ => 2,
        }
    }
    values.sort_by(|a, b| {
        a.compare(b)
            .unwrap_or(Ordering::Equal)
            .then(rank(a).cmp(&rank(b)))
    });
    let mut unique: Vec<Value> = Vec::with_capacity(values.len());
    for value in values {
        if !unique
            .last()
            .is_some_and(|last| same_constant(last, &value))
        {
            unique.push(value);
        }
    }
    unique
}

/// Whether two constants are the same node's constant: same type and value
/// (`spec/ast.md`, *Structural identity*).
// Exact float equality is the identity: 0.0 and -0.0 are the same node.
#[allow(clippy::float_cmp)]
fn same_constant(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null(_), Value::Null(_)) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        // 0.0 and -0.0 are the same node.
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Decimal(x), Value::Decimal(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Date(x), Value::Date(y)) => x == y,
        (Value::NaiveDateTime(x), Value::NaiveDateTime(y)) => x == y,
        (Value::AwareDateTime(x), Value::AwareDateTime(y)) => x == y,
        _ => false,
    }
}

fn hash_constant<H: Hasher>(value: &Value, state: &mut H) {
    std::mem::discriminant(value).hash(state);
    match value {
        Value::Bool(b) => b.hash(state),
        Value::Int(i) => i.hash(state),
        Value::Float(f) => (if *f == 0.0 { 0.0_f64 } else { *f }).to_bits().hash(state),
        Value::Decimal(d) => d.hash(state),
        Value::String(s) => s.hash(state),
        Value::Date(d) => d.hash(state),
        Value::NaiveDateTime(d) => d.hash(state),
        Value::AwareDateTime(a) => a.timestamp().hash(state),
        _ => {}
    }
}

/// Structural identity: same shape, tags and constants (`spec/ast.md`).
/// Compared with an explicit stack (`crate::walk`).
impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        // Pairs whose children are still to compare: leaves are compared in
        // place, so comparing a node of leaves allocates nothing.
        let mut pending: Vec<(&Node, &Node)> = Vec::new();
        let (mut left, mut right) = (self, other);
        loop {
            if !left.same_own(right) {
                return false;
            }
            let mut index = 0;
            loop {
                match (left.child(index), right.child(index)) {
                    (Some(l), Some(r)) if l.child(0).is_none() && r.child(0).is_none() => {
                        if !l.same_own(r) {
                            return false;
                        }
                    }
                    (Some(l), Some(r)) => pending.push((l, r)),
                    (None, None) => break,
                    _ => return false,
                }
                index += 1;
            }
            match pending.pop() {
                Some(pair) => (left, right) = pair,
                None => return true,
            }
        }
    }
}

impl Eq for Node {}

/// Each node's tag and own data, in depth-first pre-order, with an explicit
/// stack.
impl Hash for Node {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut pending = Vec::new();
        let mut next = Some(self);
        while let Some(node) = next.take().or_else(|| pending.pop()) {
            node.tag().hash(state);
            match &node.expr {
                Expr::Field(path) => path.hash(state),
                Expr::Const(value) => hash_constant(value, state),
                Expr::In { values, .. } => values.iter().for_each(|v| hash_constant(v, state)),
                Expr::Between { closed, .. } => closed.hash(state),
                Expr::Matches { pattern, .. } => pattern.source.hash(state),
                Expr::StringTest { text, .. } => text.hash(state),
                _ => {}
            }
            // The first child next, the others after it, in order.
            next = node.child(0);
            let start = pending.len();
            let mut index = 1;
            while let Some(child) = node.child(index) {
                pending.push(child);
                index += 1;
            }
            pending[start..].reverse();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ArithOp, Closed, CompareOp, Expr, Kind, Node};
    use crate::error::{ErrorCode, Segment};
    use crate::value::{Decimal, NullCause, Value};

    fn f(name: &str) -> Node {
        Node::field(vec![Segment::Key(name.into())]).unwrap()
    }
    fn c(value: Value) -> Node {
        Node::constant(value).unwrap()
    }
    fn gt(a: &str, n: i64) -> Node {
        Node::compare(CompareOp::Gt, f(a), c(Value::Int(n))).unwrap()
    }
    fn code<T>(result: crate::Result<T>) -> ErrorCode {
        result.err().unwrap().code()
    }

    #[test]
    fn kinds() {
        assert_eq!(f("a").kind(), Kind::Any);
        assert_eq!(c(Value::Bool(true)).kind(), Kind::Any);
        assert_eq!(c(Value::Int(1)).kind(), Kind::Value);
        assert_eq!(gt("a", 1).kind(), Kind::Condition);
    }

    #[test]
    fn construction_errors_name_the_offending_operand() {
        let condition = gt("a", 1);
        let error = Node::compare(CompareOp::Lt, condition.clone(), f("b")).unwrap_err();
        assert_eq!(*error.context().node.clone().unwrap(), condition);
        let text = c(Value::String("x".into()));
        let error = Node::abs(text.clone()).unwrap_err();
        assert_eq!(*error.context().node.clone().unwrap(), text);
    }

    #[test]
    fn construction_rules() {
        assert_eq!(code(Node::field(vec![])), ErrorCode::ExpressionError);
        assert_eq!(
            code(Node::field(vec![Segment::Key(String::new())])),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::constant(Value::List(vec![]))),
            ErrorCode::ExpressionError
        );
        assert_eq!(code(Node::and(vec![f("a")])), ErrorCode::ExpressionError);
        assert_eq!(
            code(Node::compare(
                CompareOp::Eq,
                gt("a", 1),
                c(Value::Bool(true))
            )),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::compare(
                CompareOp::Eq,
                f("a"),
                c(Value::Null(NullCause::Null))
            )),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::not(c(Value::Null(NullCause::Null)))),
            ErrorCode::ExpressionError
        );
        assert!(Node::is_null(c(Value::Null(NullCause::Null))).is_ok());
        assert_eq!(
            code(Node::arith(
                ArithOp::Add,
                c(Value::String("a".into())),
                c(Value::Int(1))
            )),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::arith(ArithOp::Add, c(Value::Bool(true)), f("a"))),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::is_in(
                f("a"),
                vec![Value::Int(1), Value::String("1".into())]
            )),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::is_in(f("a"), vec![])),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::between(
                f("a"),
                c(Value::Bool(false)),
                c(Value::Bool(true)),
                Closed::Both
            )),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::matches(f("a"), "a(?=b)")),
            ErrorCode::ExpressionError
        );
        assert_eq!(
            code(Node::matches(f("a"), "(a)\\1")),
            ErrorCode::ExpressionError
        );
        assert!(Node::matches(f("a"), "(?i)x$").is_ok());
        assert_eq!(
            code(Node::and(vec![
                f("a"),
                Node::arith(ArithOp::Add, f("b"), c(Value::Int(1))).unwrap()
            ])),
            ErrorCode::ExpressionError
        );
    }

    #[test]
    fn and_is_flattened() {
        let inner = Node::and(vec![f("a"), f("b")]).unwrap();
        let outer = Node::and(vec![inner, f("c")]).unwrap();
        let Expr::And(operands) = outer.expr() else {
            panic!()
        };
        assert_eq!(operands.len(), 3);
        let or = Node::or(vec![f("a"), f("b")]).unwrap();
        let mixed = Node::and(vec![or, f("c")]).unwrap();
        let Expr::And(operands) = mixed.expr() else {
            panic!()
        };
        assert_eq!(operands.len(), 2);
    }

    #[test]
    fn in_values_are_normalized() {
        let d = |t: &str| Value::Decimal(Decimal::parse(t).unwrap());
        let node = Node::is_in(
            f("a"),
            vec![Value::Int(2), Value::Int(1), Value::Float(1.0), d("1")],
        )
        .unwrap();
        let Expr::In { values, .. } = node.expr() else {
            panic!()
        };
        assert!(matches!(
            values.as_slice(),
            [
                Value::Decimal(_),
                Value::Float(_),
                Value::Int(1),
                Value::Int(2)
            ]
        ));
        let node = Node::is_in(
            f("a"),
            vec![Value::Float(-0.0), Value::Float(0.0), d("1.00"), d("1.0")],
        )
        .unwrap();
        let Expr::In { values, .. } = node.expr() else {
            panic!()
        };
        assert_eq!(values.len(), 2);
        assert!(matches!(values[0], Value::Float(z) if z.is_sign_negative()));
        assert!(matches!(&values[1], Value::Decimal(x) if x.to_string() == "1.00"));
    }

    #[test]
    fn structural_identity() {
        assert_eq!(c(Value::Float(0.0)), c(Value::Float(-0.0)));
        assert_ne!(c(Value::Int(1)), c(Value::Float(1.0)));
        assert_eq!(
            Node::is_in(f("a"), vec![Value::Int(2), Value::Int(1)]).unwrap(),
            Node::is_in(f("a"), vec![Value::Int(1), Value::Int(2)]).unwrap()
        );
        assert_ne!(gt("a", 1), gt("a", 2));
        assert_eq!(gt("a", 1), gt("a", 1));
    }

    #[test]
    fn field_paths_in_pre_order() {
        let node = Node::and(vec![
            gt("b", 1),
            Node::between(f("a"), f("b"), f("c"), Closed::Both).unwrap(),
        ])
        .unwrap();
        let paths: Vec<String> = node
            .field_paths()
            .iter()
            .map(|p| format!("{p:?}"))
            .collect();
        assert_eq!(paths.len(), 3);
        assert!(
            paths[0].contains("\"b\"") && paths[1].contains("\"a\"") && paths[2].contains("\"c\"")
        );
    }

    fn hash(node: &Node) -> u64 {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        BuildHasherDefault::<DefaultHasher>::default().hash_one(node)
    }

    #[test]
    fn equal_nodes_hash_alike() {
        use jiff::civil::date;
        use jiff::tz::Offset;

        use crate::value::{AwareDateTime, Zone};

        let instant = "2026-11-02T09:00:00Z".parse().unwrap();
        let aware = |zone| Value::AwareDateTime(AwareDateTime::new(instant, zone).unwrap());
        let pairs = [
            (c(Value::Float(0.0)), c(Value::Float(-0.0))),
            (c(Value::Bool(true)), c(Value::Bool(true))),
            (
                c(Value::Decimal(Decimal::parse("1.5").unwrap())),
                c(Value::Decimal(Decimal::parse("1.5").unwrap())),
            ),
            (c(Value::String("a".into())), c(Value::String("a".into()))),
            (
                c(Value::Date(date(2026, 11, 2))),
                c(Value::Date(date(2026, 11, 2))),
            ),
            (
                c(Value::NaiveDateTime(date(2026, 11, 2).at(9, 0, 0, 0))),
                c(Value::NaiveDateTime(date(2026, 11, 2).at(9, 0, 0, 0))),
            ),
            (
                c(aware(Zone::Fixed(Offset::UTC))),
                c(aware(Zone::Named("Europe/Madrid".into()))),
            ),
            (
                c(Value::Null(NullCause::Null)),
                c(Value::Null(NullCause::Null)),
            ),
            (
                Node::is_in(f("a"), vec![Value::Int(1), Value::Int(2)]).unwrap(),
                Node::is_in(f("a"), vec![Value::Int(2), Value::Int(1)]).unwrap(),
            ),
            (
                Node::between(f("a"), c(Value::Int(1)), c(Value::Int(2)), Closed::Left).unwrap(),
                Node::between(f("a"), c(Value::Int(1)), c(Value::Int(2)), Closed::Left).unwrap(),
            ),
            (
                Node::matches(f("a"), "^x").unwrap(),
                Node::matches(f("a"), "^x").unwrap(),
            ),
            (
                Node::string_test(super::StringOp::EndsWith, f("a"), "x").unwrap(),
                Node::string_test(super::StringOp::EndsWith, f("a"), "x").unwrap(),
            ),
            (gt("a", 1), gt("a", 1)),
        ];
        for (a, b) in &pairs {
            assert_eq!(a, b);
            assert_eq!(hash(a), hash(b), "{a}");
        }
        assert_ne!(
            Node::between(f("a"), c(Value::Int(1)), c(Value::Int(2)), Closed::Left).unwrap(),
            Node::between(f("a"), c(Value::Int(1)), c(Value::Int(2)), Closed::Right).unwrap()
        );
    }

    #[test]
    fn a_constant_is_not_an_unreadable_value() {
        for cause in [
            NullCause::IntOverflow,
            NullCause::DecimalOverflow,
            NullCause::ReadError("boom".into()),
        ] {
            assert_eq!(
                code(Node::constant(Value::Null(cause))),
                ErrorCode::ExpressionError
            );
        }
    }
}
