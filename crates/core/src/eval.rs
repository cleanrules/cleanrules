//! The evaluator (`spec/ast.md`, *Evaluation semantics* and *Results and
//! reasons*).
//!
//! Every node is evaluated (no short circuit), so results never depend on operand
//! order, and the whole result tree is kept for explanations. Truth values are
//! combined only through the [`Logic`].

use std::borrow::Cow;
use std::cmp::Ordering;

use crate::ast::{ArithOp, Closed, CompareOp, Expr, Kind, Node, StringOp};
use crate::error::{Error, ErrorCode, Result, Segment};
use crate::logic::Logic;
use crate::value::{Group, NullCause, Number, Value};

/// Why a truth value is unknown or a value is null.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReasonCode {
    /// The path cannot be resolved.
    MissingField,
    /// A null value (or NaN) was met.
    Null,
    /// Values of incompatible groups, or of the `other` group, met; or a
    /// non-boolean used as a condition.
    TypeMismatch,
    /// Booleans were ordered.
    Unorderable,
    /// A division by zero.
    DivisionByZero,
    /// An integer outside the signed 64-bit range.
    IntOverflow,
    /// A decimal in the data that does not fit.
    DecimalOverflow,
    /// An arithmetic result is NaN.
    NanResult,
    /// Reading the data raised an error.
    ReadError,
}

impl ReasonCode {
    /// The code as written in the spec.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ReasonCode::MissingField => "missing_field",
            ReasonCode::Null => "null",
            ReasonCode::TypeMismatch => "type_mismatch",
            ReasonCode::Unorderable => "unorderable",
            ReasonCode::DivisionByZero => "division_by_zero",
            ReasonCode::IntOverflow => "int_overflow",
            ReasonCode::DecimalOverflow => "decimal_overflow",
            ReasonCode::NanResult => "nan_result",
            ReasonCode::ReadError => "read_error",
        }
    }
}

/// A reason: its code, the field paths and type groups involved, and a
/// human-readable detail that is never compared.
#[derive(Clone, Debug, PartialEq)]
pub struct Reason {
    /// What happened.
    pub code: ReasonCode,
    /// The field paths involved; an empty path is the input itself.
    pub paths: Vec<Vec<Segment>>,
    /// The type groups involved.
    pub types: Vec<Group>,
    /// Informative text.
    pub detail: Option<String>,
}

impl Reason {
    fn new(code: ReasonCode, paths: Vec<Vec<Segment>>) -> Self {
        Self {
            code,
            paths,
            types: Vec::new(),
            detail: None,
        }
    }
}

/// What a node evaluated to.
#[derive(Clone, Debug)]
pub enum Outcome<T> {
    /// A truth value, for a node in a condition position.
    Truth(T),
    /// A value, for every other node.
    Value(Value),
}

/// The result of evaluating one node, with the results of its children
/// (`spec/json-format.md`, *Result trees*).
#[derive(Clone, Debug)]
pub struct Evaluated<'a, T> {
    /// The node evaluated; owned for the synthetic halves of a `between`.
    pub node: Cow<'a, Node>,
    /// The truth value or value.
    pub outcome: Outcome<T>,
    /// Why it is unknown or null, and only then.
    pub reason: Option<Reason>,
    /// The children's results.
    pub children: Vec<Evaluated<'a, T>>,
}

impl<T: Copy> Evaluated<'_, T> {
    /// The truth value, for a condition result.
    #[must_use]
    pub fn truth(&self) -> Option<T> {
        match self.outcome {
            Outcome::Truth(t) => Some(t),
            Outcome::Value(_) => None,
        }
    }

    /// The value, for a value result.
    #[must_use]
    pub fn value(&self) -> Option<&Value> {
        match &self.outcome {
            Outcome::Value(v) => Some(v),
            Outcome::Truth(_) => None,
        }
    }
}

/// Evaluation options.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options<'r> {
    /// Raise on missing fields, type mismatches and read errors instead of
    /// returning unknown.
    pub strict: bool,
    /// The rule being evaluated, named in errors.
    pub rule: Option<&'r str>,
}

/// Name `node` as the failing node of an evaluation error that names none yet.
fn failed_at(error: Error, node: &Node) -> Error {
    if error.code().is_a(ErrorCode::EvaluationError) && error.context().node.is_none() {
        error.with_node(node)
    } else {
        error
    }
}

/// Evaluate a condition (or a field or constant used as one) on `input`.
///
/// # Errors
///
/// `expression_error` for a value node or the null constant; in strict mode,
/// `missing_field`, `type_mismatch` or `evaluation_error`, the first in
/// depth-first post-order.
pub fn evaluate_tree<'a, L: Logic>(
    node: &'a Node,
    input: &Value,
    logic: &L,
    options: Options<'_>,
) -> Result<Evaluated<'a, L::Truth>> {
    if node.kind() == Kind::Value || node.is_null_constant() {
        return Err(Error::new(
            ErrorCode::ExpressionError,
            format!("'{}' is not a condition", node.tag()),
        )
        .with_hint("compare it, or evaluate it with evaluate_value_tree"));
    }
    Evaluator {
        input,
        logic,
        options,
    }
    .condition(Cow::Borrowed(node))
}

/// Evaluate a value node (or a field) on `input`.
///
/// # Errors
///
/// `expression_error` for a condition; in strict mode, as [`evaluate_tree`].
pub fn evaluate_value_tree<'a, L: Logic>(
    node: &'a Node,
    input: &Value,
    logic: &L,
    options: Options<'_>,
) -> Result<Evaluated<'a, L::Truth>> {
    if node.kind() == Kind::Condition {
        return Err(Error::new(
            ErrorCode::ExpressionError,
            format!("a {} is not a value", node.tag()),
        ));
    }
    Evaluator {
        input,
        logic,
        options,
    }
    .value(Cow::Borrowed(node))
}

/// Two results, such as a comparison's left and right.
type Pair<'a, T> = (Evaluated<'a, T>, Evaluated<'a, T>);

struct Evaluator<'i, 'o, L> {
    input: &'i Value,
    logic: &'o L,
    options: Options<'o>,
}

/// The paths of the fields among a node's direct children, in order.
fn child_paths(node: &Node) -> Vec<Vec<Segment>> {
    node.children()
        .into_iter()
        .filter_map(|child| match child.expr() {
            Expr::Field(path) => Some(path.clone()),
            _ => None,
        })
        .collect()
}

fn group_of(value: &Value) -> Group {
    value.group().unwrap_or(Group::Other)
}

impl<L: Logic> Evaluator<'_, '_, L> {
    fn error(&self, code: ErrorCode, message: String, path: Option<Vec<Segment>>) -> Error {
        let mut error = Error::new(code, message);
        let context = error.context_mut();
        context.rule = self.options.rule.map(str::to_owned);
        context.path = path;
        error
    }

    fn leaf(
        node: Cow<'_, Node>,
        outcome: Outcome<L::Truth>,
        reason: Option<Reason>,
    ) -> Evaluated<'_, L::Truth> {
        Evaluated {
            node,
            outcome,
            reason,
            children: Vec::new(),
        }
    }

    /// A type mismatch: unknown with its reason, or an error in strict mode.
    fn mismatch(&self, reason: Reason) -> Result<(L::Truth, Option<Reason>)> {
        if self.options.strict {
            let mut error = self.error(
                ErrorCode::TypeMismatch,
                format!(
                    "type mismatch: {}",
                    reason
                        .types
                        .iter()
                        .map(|g| g.as_str())
                        .collect::<Vec<_>>()
                        .join(" vs ")
                ),
                reason.paths.first().cloned(),
            );
            error.context_mut().types.clone_from(&reason.types);
            return Err(error);
        }
        Ok((self.logic.unknown(), Some(reason)))
    }

    // --- Conditions --------------------------------------------------------------------

    /// Evaluate a condition; an evaluation error names the innermost node that
    /// failed (`spec/errors.md`, `node`).
    fn condition<'a>(&self, node: Cow<'a, Node>) -> Result<Evaluated<'a, L::Truth>> {
        match node {
            Cow::Borrowed(node) => self
                .condition_of(Cow::Borrowed(node))
                .map_err(|error| failed_at(error, node)),
            // A synthetic node (the halves of `between`): the error names the
            // user's node above it.
            Cow::Owned(node) => self.condition_of(Cow::Owned(node)),
        }
    }

    fn condition_of<'a>(&self, node: Cow<'a, Node>) -> Result<Evaluated<'a, L::Truth>> {
        let n: &Node = &node;
        match n.expr() {
            Expr::And(_) | Expr::Or(_) => {
                let is_and = matches!(n.expr(), Expr::And(_));
                let children = (0..n.children().len())
                    .map(|i| self.condition(child(&node, i)))
                    .collect::<Result<Vec<_>>>()?;
                let truths: Vec<L::Truth> = children.iter().filter_map(Evaluated::truth).collect();
                let truth = if is_and {
                    self.logic.all(&truths)
                } else {
                    self.logic.any(&truths)
                };
                Ok(self.combined(node, truth, children))
            }
            Expr::Not(_) => {
                let operand = self.condition(child(&node, 0))?;
                let truth = self
                    .logic
                    .not(operand.truth().unwrap_or_else(|| self.logic.unknown()));
                Ok(self.combined(node, truth, vec![operand]))
            }
            Expr::Field(_) | Expr::Const(_) => self.value_as_condition(node),
            Expr::Compare { op, .. } => {
                let op = *op;
                let (left, right) = self.two_values(&node)?;
                let (truth, reason) = self.compare_values(&node, op, &left, &right)?;
                Ok(Evaluated {
                    node,
                    outcome: Outcome::Truth(truth),
                    reason,
                    children: vec![left, right],
                })
            }
            Expr::In { values, .. } => {
                let operand = self.sub_value(&node, 0)?;
                let (truth, reason) = self.member(&node, &operand, values)?;
                Ok(Evaluated {
                    node,
                    outcome: Outcome::Truth(truth),
                    reason,
                    children: vec![operand],
                })
            }
            Expr::Between { .. } => self.between(node),
            Expr::IsNull(_) => {
                let operand = self.sub_value(&node, 0)?;
                let is_null = operand.value().is_none_or(Value::is_null);
                let truth = self.logic.truth(is_null);
                Ok(Evaluated {
                    node,
                    outcome: Outcome::Truth(truth),
                    reason: None,
                    children: vec![operand],
                })
            }
            Expr::Matches { .. } | Expr::StringTest { .. } => {
                let operand = self.sub_value(&node, 0)?;
                let (truth, reason) = self.string_test(&node, &operand)?;
                Ok(Evaluated {
                    node,
                    outcome: Outcome::Truth(truth),
                    reason,
                    children: vec![operand],
                })
            }
            Expr::Arith { .. } | Expr::Abs(_) | Expr::Neg(_) => Err(Error::new(
                ErrorCode::ExpressionError,
                format!("'{}' is not a condition", n.tag()),
            )),
        }
    }

    /// The `index`-th child of `node`, evaluated as a value.
    fn sub_value<'a>(&self, node: &Cow<'a, Node>, index: usize) -> Result<Evaluated<'a, L::Truth>> {
        self.value(child(node, index))
    }

    fn two_values<'a>(&self, node: &Cow<'a, Node>) -> Result<Pair<'a, L::Truth>> {
        let left = self.sub_value(node, 0)?;
        let right = self.sub_value(node, 1)?;
        Ok((left, right))
    }

    /// A connective's result: unknown takes the first unknown child's reason.
    fn combined<'a>(
        &self,
        node: Cow<'a, Node>,
        truth: L::Truth,
        children: Vec<Evaluated<'a, L::Truth>>,
    ) -> Evaluated<'a, L::Truth> {
        let reason = if self.logic.is_unknown(truth) {
            children
                .iter()
                .find(|c| c.truth().is_some_and(|t| self.logic.is_unknown(t)))
                .and_then(|c| c.reason.clone())
        } else {
            None
        };
        Evaluated {
            node,
            outcome: Outcome::Truth(truth),
            reason,
            children,
        }
    }

    fn value_as_condition<'a>(&self, node: Cow<'a, Node>) -> Result<Evaluated<'a, L::Truth>> {
        let result = self.value(node.clone())?;
        let value = result
            .value()
            .cloned()
            .unwrap_or(Value::Null(NullCause::Null));
        let (truth, reason) = match self.logic.value_truth(&value) {
            Ok(Some(truth)) => (truth, None),
            Ok(None) => (self.logic.unknown(), result.reason.clone()),
            Err(group) => {
                let paths = match node.expr() {
                    Expr::Field(path) => vec![path.clone()],
                    _ => Vec::new(),
                };
                let mut reason = Reason::new(ReasonCode::TypeMismatch, paths);
                reason.types = vec![group, Group::Boolean];
                reason.detail = Some(format!("{group} used as a condition"));
                self.mismatch(reason)?
            }
        };
        Ok(Evaluated {
            node,
            outcome: Outcome::Truth(truth),
            reason,
            children: vec![result],
        })
    }

    fn compare_values(
        &self,
        node: &Node,
        op: CompareOp,
        left: &Evaluated<'_, L::Truth>,
        right: &Evaluated<'_, L::Truth>,
    ) -> Result<(L::Truth, Option<Reason>)> {
        let null = Value::Null(NullCause::Null);
        let (a, b) = (
            left.value().unwrap_or(&null),
            right.value().unwrap_or(&null),
        );
        if a.is_null() {
            return Ok((self.logic.unknown(), left.reason.clone()));
        }
        if b.is_null() {
            return Ok((self.logic.unknown(), right.reason.clone()));
        }
        let (ga, gb) = (group_of(a), group_of(b));
        let ordering = !matches!(op, CompareOp::Eq | CompareOp::Ne);
        if ga == Group::Boolean && gb == Group::Boolean && ordering {
            let mut reason = Reason::new(ReasonCode::Unorderable, child_paths(node));
            reason.types = vec![Group::Boolean, Group::Boolean];
            return self.mismatch(reason);
        }
        let order = if ga == gb && ga != Group::Other {
            a.compare(b)
        } else {
            None
        };
        let Some(order) = order else {
            let mut reason = Reason::new(ReasonCode::TypeMismatch, child_paths(node));
            reason.types = vec![ga, gb];
            reason.detail = Some(format!("{ga} vs {gb}"));
            return self.mismatch(reason);
        };
        let holds = match op {
            CompareOp::Eq => order == Ordering::Equal,
            CompareOp::Ne => order != Ordering::Equal,
            CompareOp::Lt => order == Ordering::Less,
            CompareOp::Le => order != Ordering::Greater,
            CompareOp::Gt => order == Ordering::Greater,
            CompareOp::Ge => order != Ordering::Less,
        };
        Ok((self.logic.truth(holds), None))
    }

    fn member(
        &self,
        node: &Node,
        operand: &Evaluated<'_, L::Truth>,
        values: &[Value],
    ) -> Result<(L::Truth, Option<Reason>)> {
        let Some(value) = operand.value().filter(|v| !v.is_null()) else {
            return Ok((self.logic.unknown(), operand.reason.clone()));
        };
        let expected = values.first().map_or(Group::Other, group_of);
        let actual = group_of(value);
        if actual != expected || expected == Group::Other {
            let mut reason = Reason::new(ReasonCode::TypeMismatch, child_paths(node));
            reason.types = vec![actual, expected];
            reason.detail = Some(format!("{actual} vs {expected}"));
            return self.mismatch(reason);
        }
        let found = values
            .iter()
            .any(|v| value.compare(v) == Some(Ordering::Equal));
        Ok((self.logic.truth(found), None))
    }

    fn between<'a>(&self, node: Cow<'a, Node>) -> Result<Evaluated<'a, L::Truth>> {
        let Expr::Between {
            operand,
            low,
            high,
            closed,
        } = node.expr()
        else {
            return Err(Error::new(ErrorCode::ExpressionError, "not a between"));
        };
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
        // The halves are synthetic comparisons, kept so explanations can say which
        // bound failed; the operand is evaluated once and shared.
        let operand_result = self.sub_value(&node, 0)?;
        let low_result = self.sub_value(&node, 1)?;
        let high_result = self.sub_value(&node, 2)?;
        let low_half_node = Node::compare(low_op, (**operand).clone(), (**low).clone())?;
        let high_half_node = Node::compare(high_op, (**operand).clone(), (**high).clone())?;
        let (low_truth, low_reason) =
            self.compare_values(&low_half_node, low_op, &operand_result, &low_result)?;
        let (high_truth, high_reason) =
            self.compare_values(&high_half_node, high_op, &operand_result, &high_result)?;
        let low_half = Evaluated {
            node: Cow::Owned(low_half_node),
            outcome: Outcome::Truth(low_truth),
            reason: low_reason,
            children: vec![operand_result.clone(), low_result],
        };
        let high_half = Evaluated {
            node: Cow::Owned(high_half_node),
            outcome: Outcome::Truth(high_truth),
            reason: high_reason,
            children: vec![operand_result, high_result],
        };
        let truth = self.logic.and(low_truth, high_truth);
        Ok(self.combined(node, truth, vec![low_half, high_half]))
    }

    fn string_test(
        &self,
        node: &Node,
        operand: &Evaluated<'_, L::Truth>,
    ) -> Result<(L::Truth, Option<Reason>)> {
        let Some(value) = operand.value().filter(|v| !v.is_null()) else {
            return Ok((self.logic.unknown(), operand.reason.clone()));
        };
        let Value::String(text) = value else {
            let group = group_of(value);
            let mut reason = Reason::new(ReasonCode::TypeMismatch, child_paths(node));
            reason.types = vec![group, Group::String];
            reason.detail = Some(format!("{group} vs string"));
            return self.mismatch(reason);
        };
        let found = match node.expr() {
            Expr::Matches { pattern, .. } => pattern.regex().is_match(text),
            Expr::StringTest {
                op: StringOp::StartsWith,
                text: t,
                ..
            } => text.starts_with(t.as_str()),
            Expr::StringTest {
                op: StringOp::EndsWith,
                text: t,
                ..
            } => text.ends_with(t.as_str()),
            Expr::StringTest {
                op: StringOp::Contains,
                text: t,
                ..
            } => text.contains(t.as_str()),
            _ => false,
        };
        Ok((self.logic.truth(found), None))
    }

    // --- Values ----------------------------------------------------------------------------

    /// Evaluate a value; an evaluation error names the innermost node that
    /// failed.
    fn value<'a>(&self, node: Cow<'a, Node>) -> Result<Evaluated<'a, L::Truth>> {
        match node {
            Cow::Borrowed(node) => self
                .value_of(Cow::Borrowed(node))
                .map_err(|error| failed_at(error, node)),
            // A synthetic node (the halves of `between`): the error names the
            // user's node above it.
            Cow::Owned(node) => self.value_of(Cow::Owned(node)),
        }
    }

    fn value_of<'a>(&self, node: Cow<'a, Node>) -> Result<Evaluated<'a, L::Truth>> {
        match node.expr() {
            Expr::Field(path) => {
                let (value, reason) = self.read(path)?;
                Ok(Self::leaf(node, Outcome::Value(value), reason))
            }
            Expr::Const(value) => {
                let reason = value
                    .is_null()
                    .then(|| Reason::new(ReasonCode::Null, Vec::new()));
                let value = value.clone();
                Ok(Self::leaf(node, Outcome::Value(value), reason))
            }
            Expr::Arith { op, .. } => {
                let op = *op;
                let (left, right) = self.two_values(&node)?;
                let (value, reason) = self.arith(&node, op, &left, &right)?;
                Ok(Evaluated {
                    node,
                    outcome: Outcome::Value(value),
                    reason,
                    children: vec![left, right],
                })
            }
            Expr::Abs(_) | Expr::Neg(_) => {
                let operand = self.sub_value(&node, 0)?;
                let (value, reason) = self.unary(&node, &operand)?;
                Ok(Evaluated {
                    node,
                    outcome: Outcome::Value(value),
                    reason,
                    children: vec![operand],
                })
            }
            _ => Err(Error::new(
                ErrorCode::ExpressionError,
                format!("a {} is not a value", node.tag()),
            )),
        }
    }

    /// Follow a path through the input (`spec/ast.md`, *Reading data*).
    /// A read error met while reading `path`: null with reason `read_error` for
    /// the path being read, or in strict mode an `evaluation_error`.
    fn read_error(&self, path: &[Segment], detail: &str) -> Result<(Value, Option<Reason>)> {
        if self.options.strict {
            return Err(self.error(
                ErrorCode::EvaluationError,
                format!("cannot read {}: {detail}", crate::text::path(path)),
                Some(path.to_vec()),
            ));
        }
        let mut reason = Reason::new(ReasonCode::ReadError, vec![path.to_vec()]);
        reason.detail = Some(detail.to_owned());
        Ok((
            Value::Null(NullCause::ReadError(detail.to_owned())),
            Some(reason),
        ))
    }

    fn read(&self, path: &[Segment]) -> Result<(Value, Option<Reason>)> {
        let mut current = self.input;
        for (i, segment) in path.iter().enumerate() {
            if let Value::Null(NullCause::ReadError(detail)) = current {
                // A failure to read never passes for an ordinary null (`spec/ast.md`).
                return self.read_error(path, detail);
            }
            if current.is_null() {
                // A null in the middle: the path exists up to it, even in strict mode.
                return Ok((
                    Value::Null(NullCause::Null),
                    Some(Reason::new(ReasonCode::Null, vec![path[..i].to_vec()])),
                ));
            }
            let next = match (current, segment) {
                (Value::Map(map), Segment::Key(key)) => map.get(key),
                (Value::List(list), Segment::Index(index)) => {
                    usize::try_from(*index).ok().and_then(|i| list.get(i))
                }
                // A record's entries also have positions; a plain map has none.
                (Value::Map(map), Segment::Index(index)) => {
                    usize::try_from(*index).ok().and_then(|i| map.get_index(i))
                }
                _ => None,
            };
            let Some(value) = next else {
                if self.options.strict {
                    return Err(self.error(
                        ErrorCode::MissingField,
                        format!("missing field {}", crate::text::path(path)),
                        Some(path.to_vec()),
                    ));
                }
                return Ok((
                    Value::Null(NullCause::Null),
                    Some(Reason::new(
                        ReasonCode::MissingField,
                        vec![path[..=i].to_vec()],
                    )),
                ));
            };
            current = value;
        }
        let full = || vec![path.to_vec()];
        Ok(match current {
            Value::Null(NullCause::Null) => {
                (current.clone(), Some(Reason::new(ReasonCode::Null, full())))
            }
            Value::Null(NullCause::IntOverflow) => (
                current.clone(),
                Some(Reason::new(ReasonCode::IntOverflow, full())),
            ),
            Value::Null(NullCause::DecimalOverflow) => (
                current.clone(),
                Some(Reason::new(ReasonCode::DecimalOverflow, full())),
            ),
            Value::Null(NullCause::ReadError(detail)) => return self.read_error(path, detail),
            other => (other.clone(), None),
        })
    }

    // a, b, x, y and r read like the arithmetic they do.
    #[allow(clippy::many_single_char_names)]
    fn arith(
        &self,
        node: &Node,
        op: ArithOp,
        left: &Evaluated<'_, L::Truth>,
        right: &Evaluated<'_, L::Truth>,
    ) -> Result<(Value, Option<Reason>)> {
        let null = Value::Null(NullCause::Null);
        let (a, b) = (
            left.value().unwrap_or(&null),
            right.value().unwrap_or(&null),
        );
        if a.is_null() {
            return Ok((null.clone(), left.reason.clone()));
        }
        if b.is_null() {
            return Ok((null.clone(), right.reason.clone()));
        }
        let (Some(x), Some(y)) = (a.as_number(), b.as_number()) else {
            let mut reason = Reason::new(ReasonCode::TypeMismatch, child_paths(node));
            reason.types = vec![group_of(a), group_of(b)];
            reason.detail = Some("arithmetic needs numbers".into());
            let (_, reason) = self.mismatch(reason)?;
            return Ok((null, reason));
        };
        let paths = || child_paths(node);
        if op == ArithOp::Div {
            let (x, y) = (x.to_f64(), y.to_f64());
            if y == 0.0 {
                return Ok((null, Some(Reason::new(ReasonCode::DivisionByZero, paths()))));
            }
            return Ok(float_result(x / y, paths));
        }
        if let (Number::Int(x), Number::Int(y)) = (x, y) {
            let result = match op {
                ArithOp::Add => x.checked_add(y),
                ArithOp::Sub => x.checked_sub(y),
                ArithOp::Mul => x.checked_mul(y),
                ArithOp::Div => None,
            };
            return Ok(match result {
                Some(r) => (Value::Int(r), None),
                None => (null, Some(Reason::new(ReasonCode::IntOverflow, paths()))),
            });
        }
        let (x, y) = (x.to_f64(), y.to_f64());
        let result = match op {
            ArithOp::Add => x + y,
            ArithOp::Sub => x - y,
            ArithOp::Mul => x * y,
            ArithOp::Div => x / y,
        };
        Ok(float_result(result, paths))
    }

    fn unary(
        &self,
        node: &Node,
        operand: &Evaluated<'_, L::Truth>,
    ) -> Result<(Value, Option<Reason>)> {
        let null = Value::Null(NullCause::Null);
        let value = operand.value().unwrap_or(&null);
        if value.is_null() {
            return Ok((null.clone(), operand.reason.clone()));
        }
        let Some(number) = value.as_number() else {
            let mut reason = Reason::new(ReasonCode::TypeMismatch, child_paths(node));
            reason.types = vec![group_of(value), Group::Number];
            let (_, reason) = self.mismatch(reason)?;
            return Ok((null, reason));
        };
        let is_abs = matches!(node.expr(), Expr::Abs(_));
        Ok(match number {
            Number::Int(i) => match if is_abs {
                i.checked_abs()
            } else {
                i.checked_neg()
            } {
                Some(r) => (Value::Int(r), None),
                None => (
                    null,
                    Some(Reason::new(ReasonCode::IntOverflow, child_paths(node))),
                ),
            },
            other => {
                let f = other.to_f64();
                (Value::Float(if is_abs { f.abs() } else { -f }), None)
            }
        })
    }
}

/// Recompute a result from its children's results, as evaluation would
/// (`spec/json-format.md`, *Reading a decision*, check 5). Leaves (a field or
/// constant in a value position) are taken as they are.
///
/// # Errors
///
/// `expression_error` when the result's shape does not fit its node; callers
/// check shapes first.
pub(crate) fn recompute<L: Logic>(
    logic: &L,
    result: &Evaluated<'_, L::Truth>,
) -> Result<(Outcome<L::Truth>, Option<Reason>)> {
    let null = Value::Null(NullCause::Null);
    let evaluator = Evaluator {
        input: &null,
        logic,
        options: Options::default(),
    };
    let node: &Node = &result.node;
    let children = &result.children;
    let child = |i: usize| {
        children
            .get(i)
            .ok_or_else(|| Error::new(ErrorCode::ExpressionError, "a result lacks a child"))
    };
    let truth_of = |c: &Evaluated<'_, L::Truth>| c.truth().unwrap_or_else(|| logic.unknown());
    if let Outcome::Value(_) = result.outcome {
        return match node.expr() {
            Expr::Field(_) | Expr::Const(_) => Ok((result.outcome.clone(), result.reason.clone())),
            Expr::Arith { op, .. } => {
                let (value, reason) = evaluator.arith(node, *op, child(0)?, child(1)?)?;
                Ok((Outcome::Value(value), reason))
            }
            _ => {
                let (value, reason) = evaluator.unary(node, child(0)?)?;
                Ok((Outcome::Value(value), reason))
            }
        };
    }
    let connective = |truth: L::Truth| {
        let reason = if logic.is_unknown(truth) {
            children
                .iter()
                .find(|c| c.truth().is_some_and(|t| logic.is_unknown(t)))
                .and_then(|c| c.reason.clone())
        } else {
            None
        };
        (Outcome::Truth(truth), reason)
    };
    Ok(match node.expr() {
        Expr::And(_) => connective(logic.all(&children.iter().map(truth_of).collect::<Vec<_>>())),
        Expr::Or(_) => connective(logic.any(&children.iter().map(truth_of).collect::<Vec<_>>())),
        Expr::Not(_) => connective(logic.not(truth_of(child(0)?))),
        Expr::Between { .. } => connective(logic.and(truth_of(child(0)?), truth_of(child(1)?))),
        // A field or constant in a condition position (a constant there is a
        // boolean or null; a field may hold anything).
        Expr::Field(_) | Expr::Const(_) => {
            let value = child(0)?;
            let read = value
                .value()
                .cloned()
                .unwrap_or(Value::Null(NullCause::Null));
            match logic.value_truth(&read) {
                Ok(Some(truth)) => (Outcome::Truth(truth), None),
                Ok(None) => (Outcome::Truth(logic.unknown()), value.reason.clone()),
                Err(group) => {
                    let paths = match node.expr() {
                        Expr::Field(path) => vec![path.clone()],
                        _ => Vec::new(),
                    };
                    let mut reason = Reason::new(ReasonCode::TypeMismatch, paths);
                    reason.types = vec![group, Group::Boolean];
                    (Outcome::Truth(logic.unknown()), Some(reason))
                }
            }
        }
        Expr::Compare { op, .. } => {
            let (truth, reason) = evaluator.compare_values(node, *op, child(0)?, child(1)?)?;
            (Outcome::Truth(truth), reason)
        }
        Expr::In { values, .. } => {
            let (truth, reason) = evaluator.member(node, child(0)?, values)?;
            (Outcome::Truth(truth), reason)
        }
        Expr::IsNull(_) => {
            let is_null = child(0)?.value().is_none_or(Value::is_null);
            (Outcome::Truth(logic.truth(is_null)), None)
        }
        Expr::Matches { .. } | Expr::StringTest { .. } => {
            let (truth, reason) = evaluator.string_test(node, child(0)?)?;
            (Outcome::Truth(truth), reason)
        }
        Expr::Arith { .. } | Expr::Abs(_) | Expr::Neg(_) => {
            return Err(Error::new(
                ErrorCode::ExpressionError,
                "a value has no truth value",
            ));
        }
    })
}

/// A float result: NaN is null with `nan_result`; infinities are kept.
fn float_result(value: f64, paths: impl Fn() -> Vec<Vec<Segment>>) -> (Value, Option<Reason>) {
    if value.is_nan() {
        (
            Value::Null(NullCause::Null),
            Some(Reason::new(ReasonCode::NanResult, paths())),
        )
    } else {
        (Value::Float(value), None)
    }
}

/// The `index`-th child of a node: borrowed for a borrowed node, cloned for the
/// owned halves of a `between`. Callers pass an index below the child count.
fn child<'a>(node: &Cow<'a, Node>, index: usize) -> Cow<'a, Node> {
    match node {
        Cow::Borrowed(n) => Cow::Borrowed(n.children()[index]),
        Cow::Owned(n) => Cow::Owned(n.children()[index].clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::{Options, ReasonCode, evaluate_tree, evaluate_value_tree};
    use crate::build::field;
    use crate::error::ErrorCode;
    use crate::logic::{Kleene, Truth};
    use crate::value::{Group, Map, NullCause, Opaque, Value};

    fn input(key: &str, value: Value) -> Value {
        let mut map = Map::new();
        map.insert(key, value);
        Value::Map(map)
    }

    #[test]
    fn read_errors_from_native_data() {
        let node = field("a").gt(1).build().unwrap();
        let data = input(
            "a",
            Value::Null(NullCause::ReadError("property raised".into())),
        );
        let result = evaluate_tree(&node, &data, &Kleene, Options::default()).unwrap();
        assert_eq!(result.truth(), Some(Truth::Unknown));
        assert_eq!(result.reason.as_ref().unwrap().code, ReasonCode::ReadError);
        let strict = Options {
            strict: true,
            rule: Some("r"),
        };
        let error = evaluate_tree(&node, &data, &Kleene, strict).unwrap_err();
        assert_eq!(error.code(), ErrorCode::EvaluationError);
        assert_eq!(error.context().rule.as_deref(), Some("r"));
    }

    #[test]
    fn read_errors_in_the_middle_of_a_path_are_not_ordinary_nulls() {
        let node = field("customer").get("age").gt(1).build().unwrap();
        let data = input(
            "customer",
            Value::Null(NullCause::ReadError("getter raised".into())),
        );
        let result = evaluate_tree(&node, &data, &Kleene, Options::default()).unwrap();
        let reason = result.reason.as_ref().unwrap();
        assert_eq!(reason.code, ReasonCode::ReadError);
        assert_eq!(reason.paths.len(), 1);
        assert_eq!(reason.paths[0].len(), 2); // the path being read
        let strict = Options {
            strict: true,
            rule: None,
        };
        let error = evaluate_tree(&node, &data, &Kleene, strict).unwrap_err();
        assert_eq!(error.code(), ErrorCode::EvaluationError);
    }

    #[test]
    fn strict_errors_name_the_innermost_failing_node() {
        let strict = Options {
            strict: true,
            rule: None,
        };
        let failing = |node: &crate::ast::Node, data: &Value| {
            let error = evaluate_tree(node, data, &Kleene, strict).unwrap_err();
            (error.code(), *error.context().node.clone().unwrap())
        };
        // A missing field: the field itself.
        let node = (field("a").gt(1) & field("b").lt(2)).build().unwrap();
        let (code, at) = failing(&node, &input("a", Value::Int(5)));
        assert_eq!(code, ErrorCode::MissingField);
        assert_eq!(at, field("b").build().unwrap());
        // A type mismatch: the comparison.
        let (code, at) = failing(&node, &input("a", Value::String("x".into())));
        assert_eq!(code, ErrorCode::TypeMismatch);
        assert_eq!(at, field("a").gt(1).build().unwrap());
        // Inside `between`, whose halves are synthetic: the user's `between`.
        let node = field("a").between(1, 5).build().unwrap();
        let (code, at) = failing(&node, &input("a", Value::String("x".into())));
        assert_eq!(code, ErrorCode::TypeMismatch);
        assert_eq!(at, node);
    }

    #[test]
    fn records_are_read_by_key_and_by_position() {
        use crate::Segment;
        use crate::ast::Node;

        let mut row = Map::record();
        row.insert("name", Value::String("ana".into()));
        row.insert("age", Value::Int(30));
        let mut plain = Map::new();
        plain.insert("age", Value::Int(30));
        let data = |map: Map| input("row", Value::Map(map));
        let by_position = Node::compare(
            crate::ast::CompareOp::Gt,
            Node::field(vec![Segment::Key("row".into()), Segment::Index(1)]).unwrap(),
            Node::constant(Value::Int(18)).unwrap(),
        )
        .unwrap();
        let truth = |map| {
            evaluate_tree(&by_position, &data(map), &Kleene, Options::default())
                .unwrap()
                .truth()
        };
        assert_eq!(truth(row.clone()), Some(Truth::True));
        // An integer segment never reads a plain map, nor beyond the record.
        assert_eq!(truth(plain), Some(Truth::Unknown));
        let beyond = Node::field(vec![Segment::Key("row".into()), Segment::Index(2)]).unwrap();
        let result =
            super::evaluate_value_tree(&beyond, &data(row), &Kleene, Options::default()).unwrap();
        assert_eq!(result.reason.unwrap().code, ReasonCode::MissingField);
    }

    #[test]
    fn opaque_values_are_other() {
        let opaque = Value::Opaque(Opaque {
            type_name: "bytes".into(),
            token: 1,
        });
        let data = input("a", opaque);
        let compare = field("a").eq(1).build().unwrap();
        let result = evaluate_tree(&compare, &data, &Kleene, Options::default()).unwrap();
        let reason = result.reason.unwrap();
        assert_eq!(
            (reason.code, reason.types),
            (ReasonCode::TypeMismatch, vec![Group::Other, Group::Number])
        );
        let is_null = field("a").is_null().build().unwrap();
        let result = evaluate_tree(&is_null, &data, &Kleene, Options::default()).unwrap();
        assert_eq!(result.truth(), Some(Truth::False));
    }

    #[test]
    fn values_are_not_conditions() {
        let node = (field("a") + 1).build().unwrap();
        let error = evaluate_tree(
            &node,
            &Value::Null(NullCause::Null),
            &Kleene,
            Options::default(),
        )
        .unwrap_err();
        assert_eq!(error.code(), ErrorCode::ExpressionError);
    }

    #[test]
    fn a_result_has_a_truth_value_or_a_value() {
        let data = input("a", Value::Int(2));
        let condition = field("a").gt(1).build().unwrap();
        let result = evaluate_tree(&condition, &data, &Kleene, Options::default()).unwrap();
        assert!(result.value().is_none());
        let value = (field("a") + 1).build().unwrap();
        let result = evaluate_value_tree(&value, &data, &Kleene, Options::default()).unwrap();
        assert!(result.truth().is_none());
        assert!(matches!(result.value(), Some(Value::Int(3))));
        let error =
            evaluate_value_tree(&condition, &data, &Kleene, Options::default()).unwrap_err();
        assert_eq!(error.code(), ErrorCode::ExpressionError);
    }

    #[test]
    fn strict_errors_show_indexes() {
        let node = field("items").index(2).get("price").gt(1).build().unwrap();
        let strict = Options {
            strict: true,
            rule: None,
        };
        let data = input("items", Value::List(vec![]));
        let error = evaluate_tree(&node, &data, &Kleene, strict).unwrap_err();
        assert_eq!(error.code(), ErrorCode::MissingField);
        assert_eq!(error.message(), "missing field items[2].price");
    }
}
