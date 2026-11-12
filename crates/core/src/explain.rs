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

use std::convert::Infallible;

use crate::ast::{CompareOp, Expr, Node};
use crate::error::Segment;
use crate::eval::{Evaluated, Outcome, Reason};
use crate::logic::Truth;
use crate::rules::{Decision, HitPolicy, Rule, RuleResult, RuleSet};
use crate::value::{Number, Value};
use crate::walk::{Walk, post_order};

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

pub(crate) use text::{header, places};

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
    let mut walks = Walks::default();
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
            let built = requirement(&mut walks.requirements, &result.when, false);
            let mut requirement = simplify(built);
            let mut achievable = None;
            if matches!(status, Status::NotFired | Status::Unknown) {
                let (kept, ok) = keep_options_that_fire(&mut walks, &result.when, requirement);
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

/// The stacks of an explanation's walks, kept from one rule to the next.
#[derive(Default)]
struct Walks<'d, 'a> {
    requirements: Walk<Opened<'d, 'a>, Requirement<'d, 'a>>,
    replays: Vec<Replaying<'d, 'a>>,
}

/// A connective being replayed: its result, its next child, and the truth of
/// the children before it, combined.
type Replaying<'d, 'a> = (&'d Evaluated<'a, Truth>, usize, Truth);

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
/// requirement*). Built with an explicit stack (`crate::walk`), as are the other
/// walks over result trees and requirements below: both are as deep as the
/// condition.
fn requirement<'d, 'a>(
    walk: &mut Walk<Opened<'d, 'a>, Requirement<'d, 'a>>,
    result: &'d Evaluated<'a, Truth>,
    negated: bool,
) -> Requirement<'d, 'a> {
    let built: Result<Requirement<'d, 'a>, Infallible> = walk.run(
        open_requirement(result, negated),
        |opened, done| {
            Ok(opened
                .chosen
                .get(done)
                .map(|child| open_requirement(child, opened.negated)))
        },
        |opened, items| {
            Ok(match opened.kind {
                Some(kind) => Requirement::Group(kind, items.collect()),
                None => Requirement::Cause(cause(opened.result, opened.negated)),
            })
        },
    );
    match built {
        Ok(requirement) => requirement,
        Err(never) => match never {},
    }
}

/// A requirement being built: a cause (no kind), or a group of the
/// requirements of the chosen children.
struct Opened<'d, 'a> {
    result: &'d Evaluated<'a, Truth>,
    negated: bool,
    kind: Option<GroupKind>,
    chosen: Vec<&'d Evaluated<'a, Truth>>,
}

/// What a result's requirement is made of: a negation is its operand's,
/// negated; a connective or `between` a group of some children; anything else
/// a cause.
fn open_requirement<'d, 'a>(
    mut result: &'d Evaluated<'a, Truth>,
    mut negated: bool,
) -> Opened<'d, 'a> {
    while let (Expr::Not(_), Some(operand)) = (result.node.expr(), result.children.first()) {
        result = operand;
        negated = !negated;
    }
    let is_and = matches!(result.node.expr(), Expr::And(_) | Expr::Between { .. });
    if !matches!(
        result.node.expr(),
        Expr::And(_) | Expr::Or(_) | Expr::Between { .. }
    ) {
        return Opened {
            result,
            negated,
            kind: None,
            chosen: Vec::new(),
        };
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
    Opened {
        result,
        negated,
        kind: Some(kind),
        chosen,
    }
}

/// Two requirements are the same: causes on structurally identical nodes with
/// the same `negated`, or groups of the same kind with the same set of items.
fn same(a: &Requirement<'_, '_>, b: &Requirement<'_, '_>) -> bool {
    match (a, b) {
        (Requirement::Cause(x), Requirement::Cause(y)) => same_cause(x, y),
        (Requirement::Group(ka, ia), Requirement::Group(kb, ib)) if ka == kb => {
            let causes = |items: &[Requirement<'_, '_>]| {
                items.iter().all(|i| matches!(i, Requirement::Cause(_)))
            };
            if causes(ia) && causes(ib) {
                // Groups of causes, by the definition.
                ia.iter().all(|x| ib.iter().any(|y| same(x, y)))
                    && ib.iter().all(|y| ia.iter().any(|x| same(x, y)))
            } else {
                // Sameness is an equivalence, so two groups are the same when
                // their items fall in the same classes: classify both, bottom
                // up, without recursion.
                let mut classes = Classes::default();
                classes.of(a) == classes.of(b)
            }
        }
        _ => false,
    }
}

fn same_cause(x: &Cause<'_, '_>, y: &Cause<'_, '_>) -> bool {
    x.negated == y.negated && *x.result.node == *y.result.node
}

/// The classes of the same requirements met so far: a cause, or a group's kind
/// and the classes of its items, sorted and without repeats.
#[derive(Default)]
struct Classes<'r, 'd, 'a> {
    classes: Vec<Class<'r, 'd, 'a>>,
}

enum Class<'r, 'd, 'a> {
    Cause(&'r Cause<'d, 'a>),
    Group(GroupKind, Vec<usize>),
}

impl<'r, 'd, 'a> Classes<'r, 'd, 'a> {
    /// The class of a requirement, adding it when it is new.
    fn of(&mut self, requirement: &'r Requirement<'d, 'a>) -> usize {
        let found: Result<usize, Infallible> = post_order(
            requirement,
            |requirement, done| {
                Ok(match requirement {
                    Requirement::Group(_, items) => items.get(done),
                    Requirement::Cause(_) => None,
                })
            },
            |requirement, items| {
                let class = match requirement {
                    Requirement::Cause(cause) => Class::Cause(cause),
                    Requirement::Group(kind, _) => {
                        let mut items: Vec<usize> = items.collect();
                        items.sort_unstable();
                        items.dedup();
                        Class::Group(*kind, items)
                    }
                };
                let known = self.classes.iter().position(|c| match (c, &class) {
                    (Class::Cause(x), Class::Cause(y)) => same_cause(x, y),
                    (Class::Group(kx, x), Class::Group(ky, y)) => kx == ky && x == y,
                    _ => false,
                });
                Ok(known.unwrap_or_else(|| {
                    self.classes.push(class);
                    self.classes.len() - 1
                }))
            },
        );
        match found {
            Ok(class) => class,
            Err(never) => match never {},
        }
    }
}

/// Flatten, merge, absorb and unwrap, bottom up (`spec/explain.md`,
/// *Simplifying*).
fn simplify<'d, 'a>(requirement: Requirement<'d, 'a>) -> Requirement<'d, 'a> {
    let Requirement::Group(kind, items) = requirement else {
        return requirement;
    };
    // The group being simplified (its kind, its items still to simplify, and
    // those simplified), and the groups around it: an explicit stack, as a
    // requirement is as deep as its condition. A cause is simplified as it is.
    let mut current = (kind, items.into_iter(), Vec::new());
    let mut around = Vec::new();
    loop {
        match current.1.next() {
            Some(Requirement::Group(kind, items)) => {
                around.push(std::mem::replace(
                    &mut current,
                    (kind, items.into_iter(), Vec::new()),
                ));
            }
            Some(cause) => current.2.push(cause),
            None => {
                let (kind, _, simplified) = current;
                let merged = merge(kind, simplified);
                match around.pop() {
                    Some(mut outer) => {
                        outer.2.push(merged);
                        current = outer;
                    }
                    None => return merged,
                }
            }
        }
    }
}

/// A group of `kind` from its items, already simplified: flattened, merged,
/// absorbed and unwrapped.
fn merge<'d, 'a>(kind: GroupKind, items: Vec<Requirement<'d, 'a>>) -> Requirement<'d, 'a> {
    let mut flat = Vec::new();
    for item in items {
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
    /// More than [`MAX_OPTIONS`] at some step.
    struct TooMany;
    post_order(
        requirement,
        |requirement, done| {
            Ok(match requirement {
                Requirement::Group(_, items) => items.get(done),
                Requirement::Cause(_) => None,
            })
        },
        |requirement: &'r Requirement<'d, 'a>, parts| {
            let Requirement::Group(kind, _) = requirement else {
                let Requirement::Cause(cause) = requirement else {
                    return Err(TooMany);
                };
                return Ok(vec![vec![cause]]);
            };
            let combined = if *kind == GroupKind::Any {
                parts.flatten().collect::<Vec<_>>()
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
                        return Err(TooMany);
                    }
                }
                combined
            };
            if combined.len() <= MAX_OPTIONS {
                Ok(combined)
            } else {
                Err(TooMany)
            }
        },
    )
    .ok()
}

/// Recombine the tree with some leaves forced (`spec/explain.md`, *Keeping only
/// the changes that work*).
fn replay<'d, 'a>(
    frames: &mut Vec<Replaying<'d, 'a>>,
    result: &'d Evaluated<'a, Truth>,
    forced: &[(&Node, Truth)],
) -> Truth {
    // The connectives entered, innermost last: a result tree is as deep as its
    // condition. Anything else is a leaf, forced or as it was.
    frames.clear();
    let mut current = result;
    loop {
        let mut truth = match (current.node.expr(), current.children.first()) {
            (Expr::Not(_) | Expr::And(_) | Expr::Or(_) | Expr::Between { .. }, Some(first)) => {
                // `min` and `max` start from their identities (`False < Unknown < True`).
                let identity = if matches!(current.node.expr(), Expr::Or(_)) {
                    Truth::False
                } else {
                    Truth::True
                };
                frames.push((current, 1, identity));
                current = first;
                continue;
            }
            (Expr::Not(_), None) => kleene_not(Truth::Unknown),
            (Expr::And(_) | Expr::Between { .. }, None) => Truth::True,
            (Expr::Or(_), None) => Truth::False,
            _ => forced
                .iter()
                .find(|(node, _)| **node == *current.node)
                .map_or_else(|| truth_of(current), |(_, truth)| *truth),
        };
        // Combine it into the connectives above, as far as they are complete.
        loop {
            let Some((connective, next, combined)) = frames.last_mut() else {
                return truth;
            };
            let connective: &'d Evaluated<'a, Truth> = connective;
            let value = match connective.node.expr() {
                Expr::Not(_) => kleene_not(truth),
                Expr::Or(_) => (*combined).max(truth),
                _ => (*combined).min(truth),
            };
            if let Some(child) = connective.children.get(*next) {
                *next += 1;
                *combined = value;
                current = child;
                break;
            }
            truth = value;
            frames.pop();
        }
    }
}

fn fires_with<'d, 'a>(
    frames: &mut Vec<Replaying<'d, 'a>>,
    tree: &'d Evaluated<'a, Truth>,
    option: &[&Cause<'_, '_>],
) -> bool {
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
    replay(frames, tree, &forced) == Truth::True
}

fn keep_options_that_fire<'d, 'a>(
    walks: &mut Walks<'d, 'a>,
    tree: &'d Evaluated<'a, Truth>,
    requirement: Requirement<'d, 'a>,
) -> (Requirement<'d, 'a>, bool) {
    let Some(all) = options(&requirement) else {
        return (requirement, true);
    };
    let valid: Vec<Vec<Cause<'d, 'a>>> = all
        .iter()
        .filter(|option| fires_with(&mut walks.replays, tree, option))
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
