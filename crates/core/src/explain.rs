//! Explanations and summaries (`spec/explain.md`).
//!
//! An explanation is built only from a decision's result trees and its rule set:
//! nothing is evaluated again. Explanations support Kleene truth values. Both
//! explanations and summaries display as text, the same in every language:
//!
//! ```
//! use cleanrules::explain::{explain, summary};
//! use cleanrules::format::Document;
//! use cleanrules::json::Limits;
//! use cleanrules::{Map, Value};
//!
//! let text = "HIT POLICY first\nRULE adult IF age >= 18 THEN ok = TRUE";
//! let Document::RuleSet(rules) = cleanrules::text::parse(text, &Limits::default())? else {
//!     unreachable!()
//! };
//! let mut person = Map::new();
//! person.insert("age", Value::Int(16));
//! let decision = rules.evaluate(&Value::Map(person))?;
//! assert_eq!(
//!     explain(&decision).to_string(),
//!     "HIT POLICY first: selected no rule\n\
//!      [ ] adult (not fired): age >= 18\n\
//!      \x20   - age >= 18 is false (age = 16); short by 2"
//! );
//! assert!(summary(&rules).to_string().starts_with("Rule set: 1 rule, hit policy first"));
//! # Ok::<(), cleanrules::Error>(())
//! ```

use crate::ast::{CompareOp, Expr, Node};
use crate::error::Segment;
use crate::eval::{Evaluated, Outcome, Reason};
use crate::logic::Truth;
use crate::rules::{Decision, HitPolicy, Rule, RuleResult, RuleSet};
use crate::value::{Number, Value};

/// How a rule ended up (`spec/explain.md`, *Rule status*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The hit policy selected it.
    Selected,
    /// It fired, but the hit policy did not select it.
    Fired,
    /// Its condition is false.
    NotFired,
    /// Its condition is unknown.
    Unknown,
}

impl Status {
    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Selected => "selected",
            Status::Fired => "fired",
            Status::NotFired => "not_fired",
            Status::Unknown => "unknown",
        }
    }
}

/// Whether each item of a group matters, or one is enough.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    /// Each item matters.
    All,
    /// One item is enough.
    Any,
}

/// What a field would have to satisfy, and how far it is from it.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Counterfactual {
    /// The field's path.
    pub path: Vec<Segment>,
    /// The comparison it needs, already flipped and negated.
    pub op: CompareOp,
    /// The constant it is compared with.
    pub target: Value,
    /// How far the field's value is from satisfying it, when a gap exists.
    pub gap: Option<Gap>,
}

/// A counterfactual's distance.
#[derive(Clone, Debug)]
pub enum Gap {
    /// Between numbers.
    Number(Value),
    /// Between dates, in days.
    Days(i64),
    /// Between datetimes, in microseconds.
    Microseconds(i64),
}

mod text;

/// One elementary condition behind a rule's outcome.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Cause<'d, 'a> {
    /// The condition's result, in the decision.
    pub result: &'d Evaluated<'a, Truth>,
    /// Whether the rule needs it false.
    pub negated: bool,
    /// The values it read, each path once.
    pub values: Vec<(Vec<Segment>, Value)>,
    /// A counterfactual, when it does not hold and one applies.
    pub counterfactual: Option<Counterfactual>,
}

impl Cause<'_, '_> {
    /// Its truth value.
    #[must_use]
    pub fn truth(&self) -> Truth {
        self.result.truth().unwrap_or(Truth::Unknown)
    }

    /// Whether it has the truth value the rule needs.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.truth()
            == if self.negated {
                Truth::False
            } else {
                Truth::True
            }
    }

    /// Its reason, when it is unknown.
    #[must_use]
    pub fn reason(&self) -> Option<&Reason> {
        if self.truth() == Truth::Unknown {
            self.result.reason.as_ref()
        } else {
            None
        }
    }
}

/// A cause, or a group of at least two requirements.
#[derive(Clone, Debug)]
pub enum Requirement<'d, 'a> {
    /// One cause.
    Cause(Cause<'d, 'a>),
    /// A group.
    Group(GroupKind, Vec<Requirement<'d, 'a>>),
}

/// Why a rule that fired was not selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// The winner has a higher priority.
    LowerPriority,
    /// The winner has the same priority and comes first.
    SamePriority,
    /// Under `any`, the outputs agree and the first is reported.
    AnyAgrees,
    /// The winner comes first.
    EarlierRule,
}

impl NoteKind {
    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            NoteKind::LowerPriority => "lower_priority",
            NoteKind::SamePriority => "same_priority",
            NoteKind::AnyAgrees => "any_agrees",
            NoteKind::EarlierRule => "earlier_rule",
        }
    }
}

/// The note of a rule that fired but was not selected.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Note {
    /// Why.
    pub kind: NoteKind,
    /// The first selected rule.
    pub winner: String,
    /// The rule's priority and the winner's, for the priority kinds.
    pub priorities: Option<(i64, i64)>,
}

/// One rule's explanation.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct RuleExplanation<'d, 'a> {
    /// The rule. Formats and bindings write its name (the JSON `rule` member is
    /// `rule.name()`).
    pub rule: &'a Rule,
    /// How it ended up.
    pub status: Status,
    /// For a rule that fired but was not selected.
    pub note: Option<Note>,
    /// For a rule that did not fire: whether changing the listed conditions can
    /// make it fire.
    pub achievable: Option<bool>,
    /// What decides its outcome.
    pub requirement: Requirement<'d, 'a>,
}

/// The explanation of a decision, one entry per rule in rule order.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Explanation<'d, 'a> {
    /// The decision explained.
    pub decision: &'d Decision<'a>,
    /// Every rule's explanation.
    pub rules: Vec<RuleExplanation<'d, 'a>>,
}

/// Explain a decision (`spec/explain.md`).
#[must_use]
pub fn explain<'d, 'a>(decision: &'d Decision<'a>) -> Explanation<'d, 'a> {
    let selected: Vec<&str> = decision.selected().map(|r| r.rule.name()).collect();
    let winner = decision.selected().next();
    let policy = decision.ruleset().hit_policy();
    let rules = decision
        .results()
        .iter()
        .map(|result| {
            let truth = result.when.truth().unwrap_or(Truth::Unknown);
            let status = if selected.contains(&result.rule.name()) {
                Status::Selected
            } else if result.fired {
                Status::Fired
            } else if truth == Truth::Unknown {
                Status::Unknown
            } else {
                Status::NotFired
            };
            let mut requirement = simplify(requirement(&result.when, false));
            let mut achievable = None;
            if matches!(status, Status::NotFired | Status::Unknown) {
                let (kept, ok) = keep_options_that_fire(&result.when, requirement);
                requirement = kept;
                achievable = Some(ok);
            }
            let note = if status == Status::Fired {
                winner.map(|w| note(policy, result, w))
            } else {
                None
            };
            RuleExplanation {
                rule: result.rule,
                status,
                note,
                achievable,
                requirement,
            }
        })
        .collect();
    Explanation { decision, rules }
}

fn note(policy: HitPolicy, result: &RuleResult<'_>, winner: &RuleResult<'_>) -> Note {
    let (mine, theirs) = (result.rule.get_priority(), winner.rule.get_priority());
    let (kind, priorities) = match policy {
        HitPolicy::Priority if theirs > mine => (NoteKind::LowerPriority, Some((mine, theirs))),
        HitPolicy::Priority => (NoteKind::SamePriority, Some((mine, theirs))),
        HitPolicy::Any => (NoteKind::AnyAgrees, None),
        _ => (NoteKind::EarlierRule, None),
    };
    Note {
        kind,
        winner: winner.rule.name().to_owned(),
        priorities,
    }
}

// --- Requirements ----------------------------------------------------------------------

fn kleene_not(t: Truth) -> Truth {
    match t {
        Truth::True => Truth::False,
        Truth::False => Truth::True,
        Truth::Unknown => Truth::Unknown,
    }
}

fn truth_of(result: &Evaluated<'_, Truth>) -> Truth {
    result.truth().unwrap_or(Truth::Unknown)
}

/// Build the requirement of a result (`spec/explain.md`, *Building the
/// requirement*).
fn requirement<'d, 'a>(result: &'d Evaluated<'a, Truth>, negated: bool) -> Requirement<'d, 'a> {
    let is_and = matches!(result.node.expr(), Expr::And(_) | Expr::Between { .. });
    match result.node.expr() {
        Expr::Not(_) => return requirement(&result.children[0], !negated),
        Expr::And(_) | Expr::Or(_) | Expr::Between { .. } => {}
        _ => return Requirement::Cause(cause(result, negated)),
    }
    let effective = |r: &Evaluated<'_, Truth>| {
        if negated {
            kleene_not(truth_of(r))
        } else {
            truth_of(r)
        }
    };
    let conjunctive = is_and != negated;
    let value = effective(result);
    let children = result.children.iter();
    let (kind, chosen): (GroupKind, Vec<&Evaluated<'a, Truth>>) = match (conjunctive, value) {
        (true, Truth::False) => (
            GroupKind::All,
            children.filter(|c| effective(c) != Truth::True).collect(),
        ),
        (true, Truth::Unknown) => (
            GroupKind::All,
            children
                .filter(|c| effective(c) == Truth::Unknown)
                .collect(),
        ),
        (true, Truth::True) => (GroupKind::All, children.collect()),
        (false, Truth::True) => (
            GroupKind::Any,
            children.filter(|c| effective(c) == Truth::True).collect(),
        ),
        (false, Truth::Unknown) => (
            GroupKind::Any,
            children
                .filter(|c| effective(c) == Truth::Unknown)
                .collect(),
        ),
        (false, Truth::False) => (GroupKind::Any, children.collect()),
    };
    Requirement::Group(
        kind,
        chosen
            .into_iter()
            .map(|c| requirement(c, negated))
            .collect(),
    )
}

/// Two requirements are the same: causes on structurally identical nodes with
/// the same `negated`, or groups of the same kind with the same set of items.
fn same(a: &Requirement<'_, '_>, b: &Requirement<'_, '_>) -> bool {
    match (a, b) {
        (Requirement::Cause(x), Requirement::Cause(y)) => {
            x.negated == y.negated && *x.result.node == *y.result.node
        }
        (Requirement::Group(ka, ia), Requirement::Group(kb, ib)) => {
            ka == kb
                && ia.iter().all(|x| ib.iter().any(|y| same(x, y)))
                && ib.iter().all(|y| ia.iter().any(|x| same(x, y)))
        }
        _ => false,
    }
}

/// Flatten, merge, absorb and unwrap, bottom up (`spec/explain.md`,
/// *Simplifying*).
fn simplify<'d, 'a>(requirement: Requirement<'d, 'a>) -> Requirement<'d, 'a> {
    let Requirement::Group(kind, items) = requirement else {
        return requirement;
    };
    let mut flat = Vec::new();
    for item in items.into_iter().map(simplify) {
        match item {
            Requirement::Group(k, inner) if k == kind => flat.extend(inner),
            other => flat.push(other),
        }
    }
    let mut unique: Vec<Requirement<'d, 'a>> = Vec::new();
    for item in flat {
        if !unique.iter().any(|u| same(u, &item)) {
            unique.push(item);
        }
    }
    let direct: Vec<Requirement<'d, 'a>> = unique
        .iter()
        .filter(|i| matches!(i, Requirement::Cause(_)))
        .cloned()
        .collect();
    let kept: Vec<Requirement<'d, 'a>> = unique
        .into_iter()
        .filter(|item| match item {
            Requirement::Group(_, inner) => !inner
                .iter()
                .any(|c| matches!(c, Requirement::Cause(_)) && direct.iter().any(|d| same(d, c))),
            Requirement::Cause(_) => true,
        })
        .collect();
    if kept.len() == 1 {
        kept.into_iter()
            .next()
            .unwrap_or(Requirement::Group(kind, Vec::new()))
    } else {
        Requirement::Group(kind, kept)
    }
}

const MAX_OPTIONS: usize = 64;

/// Every way to satisfy a requirement, or `None` beyond 64 at any step.
fn options<'r, 'd, 'a>(
    requirement: &'r Requirement<'d, 'a>,
) -> Option<Vec<Vec<&'r Cause<'d, 'a>>>> {
    match requirement {
        Requirement::Cause(cause) => Some(vec![vec![cause]]),
        Requirement::Group(kind, items) => {
            let parts: Vec<Vec<Vec<&Cause<'d, 'a>>>> =
                items.iter().map(options).collect::<Option<_>>()?;
            let combined = if *kind == GroupKind::Any {
                parts.into_iter().flatten().collect::<Vec<_>>()
            } else {
                let mut combined: Vec<Vec<&Cause<'d, 'a>>> = vec![Vec::new()];
                for part in parts {
                    combined = combined
                        .iter()
                        .flat_map(|c| {
                            part.iter()
                                .map(move |o| c.iter().chain(o).copied().collect())
                        })
                        .collect();
                    if combined.len() > MAX_OPTIONS {
                        return None;
                    }
                }
                combined
            };
            (combined.len() <= MAX_OPTIONS).then_some(combined)
        }
    }
}

/// Recombine the tree with some leaves forced (`spec/explain.md`, *Keeping only
/// the changes that work*).
fn replay(result: &Evaluated<'_, Truth>, forced: &[(&Node, Truth)]) -> Truth {
    match result.node.expr() {
        Expr::Not(_) => kleene_not(replay(&result.children[0], forced)),
        Expr::And(_) | Expr::Between { .. } => result
            .children
            .iter()
            .map(|c| replay(c, forced))
            .min()
            .unwrap_or(Truth::True),
        Expr::Or(_) => result
            .children
            .iter()
            .map(|c| replay(c, forced))
            .max()
            .unwrap_or(Truth::False),
        _ => forced
            .iter()
            .find(|(node, _)| **node == *result.node)
            .map_or_else(|| truth_of(result), |(_, truth)| *truth),
    }
}

fn fires_with(tree: &Evaluated<'_, Truth>, option: &[&Cause<'_, '_>]) -> bool {
    let mut forced: Vec<(&Node, Truth)> = Vec::new();
    for cause in option {
        let needed = if cause.negated {
            Truth::False
        } else {
            Truth::True
        };
        match forced.iter().find(|(node, _)| **node == *cause.result.node) {
            Some((_, truth)) if *truth != needed => return false,
            Some(_) => {}
            None => forced.push((&cause.result.node, needed)),
        }
    }
    replay(tree, &forced) == Truth::True
}

fn keep_options_that_fire<'d, 'a>(
    tree: &Evaluated<'a, Truth>,
    requirement: Requirement<'d, 'a>,
) -> (Requirement<'d, 'a>, bool) {
    let Some(all) = options(&requirement) else {
        return (requirement, true);
    };
    let valid: Vec<Vec<Cause<'d, 'a>>> = all
        .iter()
        .filter(|option| fires_with(tree, option))
        .map(|option| option.iter().map(|&c| c.clone()).collect())
        .collect();
    if valid.len() == all.len() {
        return (requirement, true);
    }
    if valid.is_empty() {
        return (requirement, false);
    }
    let mut parts: Vec<Requirement<'d, 'a>> = valid
        .into_iter()
        .map(|option| {
            if let [only] = option.as_slice() {
                Requirement::Cause(only.clone())
            } else {
                Requirement::Group(
                    GroupKind::All,
                    option.into_iter().map(Requirement::Cause).collect(),
                )
            }
        })
        .collect();
    let rebuilt = if parts.len() == 1 {
        parts.remove(0)
    } else {
        Requirement::Group(GroupKind::Any, parts)
    };
    (simplify(rebuilt), true)
}

// --- Causes and counterfactuals ------------------------------------------------------------

fn cause<'d, 'a>(result: &'d Evaluated<'a, Truth>, negated: bool) -> Cause<'d, 'a> {
    let mut cause = Cause {
        result,
        negated,
        values: values_read(result),
        counterfactual: None,
    };
    if !cause.holds() {
        cause.counterfactual = counterfactual(result, negated);
    }
    cause
}

/// The values read by fields under a result, in depth-first pre-order, each path
/// once, skipping those with a reason.
fn values_read(result: &Evaluated<'_, Truth>) -> Vec<(Vec<Segment>, Value)> {
    let mut found: Vec<(Vec<Segment>, Value)> = Vec::new();
    let mut stack = vec![result];
    while let Some(current) = stack.pop() {
        if let (Expr::Field(path), Outcome::Value(value), None) =
            (current.node.expr(), &current.outcome, &current.reason)
            && !found.iter().any(|(p, _)| p == path)
        {
            found.push((path.clone(), value.clone()));
        }
        stack.extend(current.children.iter().rev());
    }
    found
}

fn flip(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Lt => CompareOp::Gt,
        CompareOp::Le => CompareOp::Ge,
        CompareOp::Gt => CompareOp::Lt,
        CompareOp::Ge => CompareOp::Le,
        other => other,
    }
}

fn negate(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Lt => CompareOp::Ge,
        CompareOp::Le => CompareOp::Gt,
        CompareOp::Gt => CompareOp::Le,
        CompareOp::Ge => CompareOp::Lt,
        CompareOp::Eq => CompareOp::Ne,
        CompareOp::Ne => CompareOp::Eq,
    }
}

fn counterfactual(result: &Evaluated<'_, Truth>, negated: bool) -> Option<Counterfactual> {
    let Expr::Compare { op, left, right } = result.node.expr() else {
        return None;
    };
    if truth_of(result) == Truth::Unknown {
        return None;
    }
    let (path, current, mut op, target) = match (left.expr(), right.expr()) {
        (Expr::Field(path), Expr::Const(target)) => {
            (path, result.children[0].value()?, *op, target)
        }
        (Expr::Const(target), Expr::Field(path)) => {
            (path, result.children[1].value()?, flip(*op), target)
        }
        _ => return None,
    };
    if negated {
        op = negate(op);
    }
    if matches!(op, CompareOp::Eq | CompareOp::Ne) {
        return negated.then(|| Counterfactual {
            path: path.clone(),
            op,
            target: target.clone(),
            gap: None,
        });
    }
    let gap = if matches!(op, CompareOp::Gt | CompareOp::Ge) {
        gap(target, current)
    } else {
        gap(current, target)
    };
    if gap.is_none() && !negated {
        return None;
    }
    Some(Counterfactual {
        path: path.clone(),
        op,
        target: target.clone(),
        gap,
    })
}

/// `a − b` with the gap arithmetic of `spec/explain.md` (*Counterfactuals*).
fn gap(a: &Value, b: &Value) -> Option<Gap> {
    let finite = |f: f64| f.is_finite().then_some(f);
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.checked_sub(*y).map(|d| Gap::Number(Value::Int(d))),
        (Value::Decimal(x), Value::Decimal(y)) => {
            x.checked_sub(*y).map(|d| Gap::Number(Value::Decimal(d)))
        }
        (Value::Decimal(x), Value::Int(y)) => x
            .checked_sub((*y).into())
            .map(|d| Gap::Number(Value::Decimal(d))),
        (Value::Int(x), Value::Decimal(y)) => crate::value::Decimal::from(*x)
            .checked_sub(*y)
            .map(|d| Gap::Number(Value::Decimal(d))),
        (Value::Float(_) | Value::Int(_), Value::Float(_) | Value::Int(_)) => {
            let (x, y) = (
                Number::to_f64(a.as_number()?),
                Number::to_f64(b.as_number()?),
            );
            finite(x)
                .and(finite(y))
                .and_then(|_| finite(x - y))
                .map(|d| Gap::Number(Value::Float(d)))
        }
        (Value::Date(x), Value::Date(y)) => y
            .until(*x)
            .ok()
            .map(|span| Gap::Days(i64::from(span.get_days()))),
        (Value::NaiveDateTime(x), Value::NaiveDateTime(y)) => {
            // Wall-clock difference: UTC has no clock changes.
            let micros = |d: &jiff::civil::DateTime| {
                d.to_zoned(jiff::tz::TimeZone::UTC)
                    .ok()
                    .map(|z| z.timestamp().as_microsecond())
            };
            Some(Gap::Microseconds(micros(x)?.checked_sub(micros(y)?)?))
        }
        (Value::AwareDateTime(x), Value::AwareDateTime(y)) => {
            let micros = x
                .timestamp()
                .as_microsecond()
                .checked_sub(y.timestamp().as_microsecond())?;
            Some(Gap::Microseconds(micros))
        }
        // A decimal and a float, and every other pair: no gap.
        _ => None,
    }
}

// --- Summaries ---------------------------------------------------------------------------------

/// Where one field is read.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct FieldUsage {
    /// The field's path.
    pub path: Vec<Segment>,
    /// The rules whose condition reads it, in rule order.
    pub conditions: Vec<String>,
    /// The `(rule, output key)` pairs whose payload value reads it.
    pub payloads: Vec<(String, String)>,
}

/// A static overview of a rule set (`spec/explain.md`, *Summaries*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Summary<'r> {
    /// The rule set.
    pub ruleset: &'r RuleSet,
    /// Every field read, in order of first appearance.
    pub fields: Vec<FieldUsage>,
    /// Every payload key, in order of first appearance.
    pub outputs: Vec<String>,
    /// Each tag with the rules carrying it, tags sorted.
    pub tags: Vec<(String, Vec<String>)>,
    /// Whether each rule has an action, in rule order, one per rule. Rules
    /// that name their action start true; actions given as code of the host
    /// language, which the core's rules never hold, are added by a binding
    /// with [`Summary::with_actions`].
    pub actions: Vec<bool>,
}

impl Summary<'_> {
    /// Say which rules have an action given as code, in rule order. Rules that
    /// name their action keep it; missing entries are false and extra ones are
    /// dropped, so there is always one per rule.
    #[must_use]
    pub fn with_actions(mut self, actions: impl IntoIterator<Item = bool>) -> Self {
        let named = self
            .ruleset
            .rules()
            .iter()
            .map(|r| r.get_action_name().is_some());
        self.actions = named
            .zip(actions.into_iter().chain(std::iter::repeat(false)))
            .map(|(named, code)| named || code)
            .collect();
        self
    }
}

/// Summarize a rule set without evaluating it.
#[must_use]
pub fn summary(ruleset: &RuleSet) -> Summary<'_> {
    let mut fields: Vec<FieldUsage> = Vec::new();
    let mut outputs: Vec<String> = Vec::new();
    let mut tags: Vec<(String, Vec<String>)> = Vec::new();
    let mut usage = |path: &[Segment]| -> usize {
        if let Some(at) = fields.iter().position(|f| f.path == path) {
            at
        } else {
            fields.push(FieldUsage {
                path: path.to_vec(),
                conditions: Vec::new(),
                payloads: Vec::new(),
            });
            fields.len() - 1
        }
    };
    let mut reads: Vec<(usize, Option<String>, String)> = Vec::new();
    for rule in ruleset.rules() {
        for path in rule.when().field_paths() {
            reads.push((usage(path), None, rule.name().to_owned()));
        }
        for (key, value) in rule.then() {
            if !outputs.contains(key) {
                outputs.push(key.clone());
            }
            for path in value.field_paths() {
                reads.push((usage(path), Some(key.clone()), rule.name().to_owned()));
            }
        }
        for tag in rule.get_tags() {
            match tags.iter_mut().find(|(t, _)| t == tag) {
                Some((_, names)) => names.push(rule.name().to_owned()),
                None => tags.push((tag.clone(), vec![rule.name().to_owned()])),
            }
        }
    }
    for (at, key, rule) in reads {
        match key {
            None => fields[at].conditions.push(rule),
            Some(key) => fields[at].payloads.push((rule, key)),
        }
    }
    tags.sort_by(|a, b| a.0.cmp(&b.0));
    Summary {
        ruleset,
        fields,
        outputs,
        tags,
        actions: ruleset
            .rules()
            .iter()
            .map(|r| r.get_action_name().is_some())
            .collect(),
    }
}
