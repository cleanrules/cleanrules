//! Reading conditions into descriptions of the inputs that make them `TRUE`
//! and `FALSE` (`spec/audit.md`, *What the audit reads*).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::Infallible;

use super::AuditLimit;
use super::domain::{Hi, Interval, Lo, ValueSet};
use crate::ast::{Closed, CompareOp, Expr, Node};
use crate::error::Segment;
use crate::eval::{Options, evaluate_tree, evaluate_value_tree};
use crate::logic::Logic;
use crate::value::{Group, NullCause, Value};
use crate::walk::post_order;

/// A value node that tests read, and the values it can hold.
#[derive(Debug)]
pub(super) struct Subject {
    /// The field's path, for a field.
    pub(super) path: Option<Vec<Segment>>,
    /// Every value it can hold: anything for a field, a number or null for an
    /// expression.
    pub(super) universe: ValueSet,
}

/// The subjects met so far, numbered in order of first appearance.
#[derive(Debug, Default)]
pub(super) struct Subjects {
    pub(super) list: Vec<Subject>,
    /// Looked up by structural identity, never iterated.
    index: HashMap<Node, usize>,
}

impl Subjects {
    pub(super) fn id(&mut self, node: &Node) -> usize {
        if let Some(&id) = self.index.get(node) {
            return id;
        }
        let (path, universe) = match node.expr() {
            Expr::Field(path) => (Some(path.clone()), ValueSet::all()),
            _ => (None, ValueSet::group(Group::Number).join(&ValueSet::null())),
        };
        let id = self.list.len();
        self.list.push(Subject { path, universe });
        self.index.insert(node.clone(), id);
        id
    }
}

/// A box: a set of values for some subjects, and any value for the others.
#[derive(Clone, Debug, Default)]
pub(super) struct Region(pub(super) BTreeMap<usize, ValueSet>);

impl Region {
    fn on(subject: usize, set: ValueSet) -> Vec<Self> {
        if set.is_empty() {
            Vec::new()
        } else {
            vec![Self(BTreeMap::from([(subject, set)]))]
        }
    }

    /// The inputs in both boxes, or `None` when there are none.
    pub(super) fn meet(&self, other: &Self) -> Option<Self> {
        let mut out = self.0.clone();
        for (subject, set) in &other.0 {
            let both = match out.get(subject) {
                Some(own) => own.meet(set),
                None => set.clone(),
            };
            if both.is_empty() {
                return None;
            }
            out.insert(*subject, both);
        }
        Some(Self(out))
    }

    /// The inputs of this box outside `other`, as disjoint boxes.
    pub(super) fn minus(&self, other: &Self, subjects: &Subjects) -> Vec<Self> {
        let mut out = Vec::new();
        let mut current = self.clone();
        for (subject, set) in &other.0 {
            let own = current
                .0
                .get(subject)
                .cloned()
                .unwrap_or_else(|| subjects.list[*subject].universe.clone());
            let outside = own.minus(set);
            if !outside.is_empty() {
                let mut part = current.clone();
                part.0.insert(*subject, outside);
                out.push(part);
            }
            let inside = own.meet(set);
            if inside.is_empty() {
                // The boxes do not meet: all of this one is outside.
                return vec![self.clone()];
            }
            current.0.insert(*subject, inside);
        }
        out
    }
}

/// The work left (`max_steps`).
#[derive(Debug)]
pub(super) struct Work {
    left: usize,
    /// Whether a step was refused.
    pub(super) exhausted: bool,
}

impl Work {
    pub(super) fn new(steps: usize) -> Self {
        Self {
            left: steps,
            exhausted: false,
        }
    }

    /// Take one step, or say that none is left.
    pub(super) fn step(&mut self) -> bool {
        if self.left == 0 {
            self.exhausted = true;
            false
        } else {
            self.left -= 1;
            true
        }
    }

    /// Take `n` steps at once, or none when fewer are left.
    pub(super) fn take(&mut self, n: usize) -> bool {
        if let Some(left) = self.left.checked_sub(n) {
            self.left = left;
            true
        } else {
            self.exhausted = true;
            false
        }
    }
}

/// The inputs that make a node `TRUE`, and those that make it `FALSE`.
#[derive(Clone, Debug, Default)]
pub(super) struct Desc {
    pub(super) t: Vec<Region>,
    pub(super) f: Vec<Region>,
}

impl Desc {
    fn swapped(self) -> Self {
        Self {
            t: self.f,
            f: self.t,
        }
    }
}

/// A node's wide description, and its narrow one when it differs
/// (`spec/audit.md`, *Combining tests*).
#[derive(Clone, Debug)]
pub(super) struct Read {
    pub(super) wide: Desc,
    pub(super) narrow: Option<Desc>,
}

impl Read {
    pub(super) fn narrow(&self) -> &Desc {
        self.narrow.as_ref().unwrap_or(&self.wide)
    }

    fn exact(wide: Desc) -> Self {
        Self { wide, narrow: None }
    }

    /// A test that may be anything for any input.
    fn unknown() -> Self {
        Self {
            wide: Desc {
                t: vec![Region::default()],
                f: vec![Region::default()],
            },
            narrow: Some(Desc::default()),
        }
    }

    /// Every input makes it `TRUE`.
    pub(super) fn always() -> Self {
        Self::exact(Desc {
            t: vec![Region::default()],
            f: Vec::new(),
        })
    }
}

/// A test the audit does not model, or a node read as one.
#[derive(Clone, Debug)]
pub(super) struct Unmodelled {
    pub(super) rule: usize,
    /// Its position in depth-first pre-order within the rule.
    pub(super) seq: usize,
    pub(super) op: &'static str,
    pub(super) pointer: String,
    pub(super) limit: Option<AuditLimit>,
}

/// A modelled test that is `UNKNOWN` for every input.
#[derive(Clone, Debug)]
pub(super) struct Incompatible {
    pub(super) rule: usize,
    pub(super) seq: usize,
    pub(super) pointer: String,
    pub(super) text: String,
    /// Whether it is so because a constant is null, such as `1 / 0`.
    pub(super) null_constant: bool,
}

/// The groups a field is used with, and by which rules.
#[derive(Clone, Debug, Default)]
pub(super) struct Expected {
    pub(super) groups: BTreeSet<Group>,
    pub(super) rules: BTreeSet<usize>,
}

/// A condition to read: a rule's `when`, or a decision table's cell.
#[derive(Clone, Copy, Debug)]
pub(super) struct Part<'n> {
    pub(super) node: &'n Node,
    /// Where it is; a cell's tests all point to the cell.
    pub(super) pointer: &'n str,
    pub(super) cell: bool,
}

/// What reading every rule found.
#[derive(Debug)]
pub(super) struct Reader {
    pub(super) subjects: Subjects,
    pub(super) work: Work,
    max_terms: usize,
    /// The fields the conditions read, in order of first appearance.
    pub(super) fields: Vec<usize>,
    pub(super) unmodelled: Vec<Unmodelled>,
    pub(super) incompatible: Vec<Incompatible>,
    pub(super) expected: BTreeMap<usize, Expected>,
}

#[derive(Clone, Copy)]
enum Connective {
    And,
    Or,
}

/// One node being walked.
struct Frame<'n> {
    node: &'n Node,
    pointer: String,
    seq: usize,
}

/// The value of a node that reads no field.
fn fold<L: Logic>(node: &Node, logic: &L) -> Value {
    if let Expr::Const(value) = node.expr() {
        return value.clone();
    }
    evaluate_value_tree(
        node,
        &Value::Null(NullCause::Null),
        logic,
        Options::default(),
    )
    .ok()
    .and_then(|tree| tree.value().cloned())
    .unwrap_or(Value::Null(NullCause::Null))
}

fn reads_field(node: &Node) -> bool {
    !node.field_paths().is_empty()
}

/// The values for which `subject op constant` is `TRUE`, and `FALSE`, before
/// the subject's universe is applied (`spec/audit.md`, *Values of a subject*).
fn compare_sets(op: CompareOp, constant: &Value) -> (ValueSet, ValueSet) {
    let none = || (ValueSet::none(), ValueSet::none());
    let Some(group) = constant.group().filter(|g| *g != Group::Other) else {
        return none();
    };
    let point = ValueSet::point(constant);
    let all = ValueSet::group(group);
    let range = |lo, hi| ValueSet::range(group, Interval { lo, hi });
    let c = || constant.clone();
    match op {
        CompareOp::Eq => (point.clone(), all.minus(&point)),
        CompareOp::Ne => (all.minus(&point), point),
        _ if group == Group::Boolean => none(),
        CompareOp::Lt => (range(Lo::Min, Hi::Excl(c())), range(Lo::Incl(c()), Hi::Max)),
        CompareOp::Le => (range(Lo::Min, Hi::Incl(c())), range(Lo::Excl(c()), Hi::Max)),
        CompareOp::Gt => (range(Lo::Excl(c()), Hi::Max), range(Lo::Min, Hi::Incl(c()))),
        CompareOp::Ge => (range(Lo::Incl(c()), Hi::Max), range(Lo::Min, Hi::Excl(c()))),
    }
}

fn flipped(op: CompareOp) -> CompareOp {
    match op {
        CompareOp::Lt => CompareOp::Gt,
        CompareOp::Le => CompareOp::Ge,
        CompareOp::Gt => CompareOp::Lt,
        CompareOp::Ge => CompareOp::Le,
        same => same,
    }
}

/// A modelled test, before its universe: its subject, its sets, and the
/// groups of its constants.
struct Modelled<'n> {
    subject: &'n Node,
    t: ValueSet,
    f: ValueSet,
    groups: Vec<Group>,
    /// Whether a constant it compares with is null (`x > 1 / 0`).
    null_constant: bool,
}

/// What a test is to the audit.
enum Test<'n> {
    Constant,
    Modelled(Box<Modelled<'n>>),
    /// Not modelled; a string test's or `matches`'s field operand expects a
    /// string.
    NotModelled(Option<&'n Node>),
}

fn classify<'n, L: Logic>(node: &'n Node, logic: &L) -> Test<'n> {
    if !reads_field(node) {
        return Test::Constant;
    }
    let group_of = |v: &Value| v.group().into_iter().collect::<Vec<_>>();
    match node.expr() {
        Expr::Field(_) => Test::Modelled(Box::new(Modelled {
            subject: node,
            t: ValueSet::point(&Value::Bool(true)),
            f: ValueSet::point(&Value::Bool(false)),
            groups: vec![Group::Boolean],
            null_constant: false,
        })),
        Expr::Compare { op, left, right } => {
            let (subject, op, constant) = match (reads_field(left), reads_field(right)) {
                (true, false) => (left.as_ref(), *op, fold(right, logic)),
                (false, true) => (right.as_ref(), flipped(*op), fold(left, logic)),
                _ => return Test::NotModelled(None),
            };
            let (t, f) = compare_sets(op, &constant);
            Test::Modelled(Box::new(Modelled {
                subject,
                t,
                f,
                groups: group_of(&constant),
                null_constant: constant.is_null(),
            }))
        }
        Expr::In { operand, values } => {
            let t = values
                .iter()
                .fold(ValueSet::none(), |set, v| set.join(&ValueSet::point(v)));
            let groups: Vec<Group> = values.first().map(group_of).unwrap_or_default();
            let f = groups
                .first()
                .map_or_else(ValueSet::none, |g| ValueSet::group(*g).minus(&t));
            Test::Modelled(Box::new(Modelled {
                subject: operand,
                t,
                f,
                groups,
                null_constant: false,
            }))
        }
        Expr::Between {
            operand,
            low,
            high,
            closed,
        } => {
            if reads_field(low) || reads_field(high) {
                return Test::NotModelled(None);
            }
            let (low, high) = (fold(low, logic), fold(high, logic));
            let (from, to) = match closed {
                Closed::Both => (CompareOp::Ge, CompareOp::Le),
                Closed::Left => (CompareOp::Ge, CompareOp::Lt),
                Closed::Right => (CompareOp::Gt, CompareOp::Le),
                Closed::Neither => (CompareOp::Gt, CompareOp::Lt),
            };
            let (t1, f1) = compare_sets(from, &low);
            let (t2, f2) = compare_sets(to, &high);
            let mut groups = group_of(&low);
            groups.extend(group_of(&high));
            Test::Modelled(Box::new(Modelled {
                subject: operand,
                t: t1.meet(&t2),
                f: f1.join(&f2),
                groups,
                null_constant: low.is_null() || high.is_null(),
            }))
        }
        Expr::IsNull(operand) => Test::Modelled(Box::new(Modelled {
            subject: operand,
            t: ValueSet::null(),
            f: ValueSet::not_null(),
            groups: Vec::new(),
            null_constant: false,
        })),
        Expr::Matches { operand, .. } | Expr::StringTest { operand, .. } => {
            Test::NotModelled(matches!(operand.expr(), Expr::Field(_)).then_some(operand.as_ref()))
        }
        // Connectives are walked; value nodes are never in a condition
        // position. Either way, nothing here is modelled.
        _ => Test::NotModelled(None),
    }
}

/// Every intersection of one box of each list, in order, without the empty
/// ones (`spec/audit.md`, *Combining tests*).
fn product(
    lists: &[&[Region]],
    max_terms: usize,
    work: &mut Work,
) -> Result<Vec<Region>, AuditLimit> {
    let mut acc = vec![Region::default()];
    for list in lists {
        let mut next = Vec::new();
        for a in &acc {
            for b in *list {
                if !work.step() {
                    return Err(AuditLimit::MaxSteps);
                }
                if let Some(both) = a.meet(b) {
                    next.push(both);
                    if next.len() > max_terms {
                        return Err(AuditLimit::MaxTerms);
                    }
                }
            }
        }
        acc = next;
        if acc.is_empty() {
            break;
        }
    }
    Ok(acc)
}

fn concat(lists: &[&[Region]], max_terms: usize) -> Result<Vec<Region>, AuditLimit> {
    let all: Vec<Region> = lists.iter().flat_map(|l| l.iter().cloned()).collect();
    if all.len() > max_terms {
        Err(AuditLimit::MaxTerms)
    } else {
        Ok(all)
    }
}

impl Reader {
    pub(super) fn new(max_terms: usize, max_steps: usize) -> Self {
        Self {
            subjects: Subjects::default(),
            work: Work::new(max_steps),
            max_terms,
            fields: Vec::new(),
            unmodelled: Vec::new(),
            incompatible: Vec::new(),
            expected: BTreeMap::new(),
        }
    }

    fn expect(&mut self, subject: usize, group: Group, rule: usize) {
        let entry = self.expected.entry(subject).or_default();
        entry.groups.insert(group);
        entry.rules.insert(rule);
    }

    /// Note every field a condition reads, in depth-first pre-order, and the
    /// fields that are operands of arithmetic, which expect numbers.
    pub(super) fn scan(&mut self, rule: usize, node: &Node) {
        let mut stack = vec![node];
        while let Some(node) = stack.pop() {
            match node.expr() {
                Expr::Field(_) => {
                    let id = self.subjects.id(node);
                    if !self.fields.contains(&id) {
                        self.fields.push(id);
                    }
                }
                Expr::Arith { .. } | Expr::Abs(_) | Expr::Neg(_) | Expr::Round { .. } => {
                    for child in node.children() {
                        if matches!(child.expr(), Expr::Field(_)) {
                            let id = self.subjects.id(child);
                            self.expect(id, Group::Number, rule);
                        }
                    }
                }
                _ => {}
            }
            stack.extend(node.children().into_iter().rev());
        }
    }

    /// Read a rule's condition, given as the conjunction of `parts` (one for
    /// a rule set; the cells of a row, under `row`, for a decision table).
    pub(super) fn read_rule<L: Logic>(
        &mut self,
        logic: &L,
        rule: usize,
        parts: &[Part<'_>],
        row: Option<&str>,
    ) -> Read {
        let mut seq = 0;
        if parts.len() > 1 {
            // The row's conjunction comes first in pre-order.
            seq = 1;
        }
        let mut reads = Vec::with_capacity(parts.len());
        for part in parts {
            reads.push(self.read(logic, rule, *part, &mut seq));
        }
        match reads.len() {
            0 => Read::always(),
            1 => reads.pop().unwrap_or_else(Read::always),
            _ => {
                let pointer = row.unwrap_or_default().to_owned();
                self.combine(Connective::And, &reads, rule, 0, "and", pointer)
            }
        }
    }

    /// Read one condition; `seq` numbers its nodes in pre-order.
    fn read<L: Logic>(&mut self, logic: &L, rule: usize, part: Part<'_>, seq: &mut usize) -> Read {
        let mut counter = *seq;
        let root = Frame {
            node: part.node,
            pointer: part.pointer.to_owned(),
            seq: counter,
        };
        let cell = part.cell;
        let read: Result<Read, Infallible> = post_order(
            root,
            |frame, done| {
                let child = match frame.node.expr() {
                    Expr::And(operands) | Expr::Or(operands) => operands
                        .get(done)
                        .map(|child| (child, format!("/operands/{done}"))),
                    Expr::Not(operand) if done == 0 => Some((operand.as_ref(), "/operand".into())),
                    _ => None,
                };
                Ok(child.map(|(node, step)| {
                    counter += 1;
                    Frame {
                        node,
                        pointer: if cell {
                            frame.pointer.clone()
                        } else {
                            format!("{}{step}", frame.pointer)
                        },
                        seq: counter,
                    }
                }))
            },
            |frame, children| Ok(self.finish(logic, rule, &frame, children.collect())),
        );
        *seq = counter + 1;
        match read {
            Ok(read) => read,
            Err(never) => match never {},
        }
    }

    fn finish<L: Logic>(
        &mut self,
        logic: &L,
        rule: usize,
        frame: &Frame<'_>,
        children: Vec<Read>,
    ) -> Read {
        let tag = frame.node.tag();
        match frame.node.expr() {
            Expr::And(_) => self.combine(
                Connective::And,
                &children,
                rule,
                frame.seq,
                tag,
                frame.pointer.clone(),
            ),
            Expr::Or(_) => self.combine(
                Connective::Or,
                &children,
                rule,
                frame.seq,
                tag,
                frame.pointer.clone(),
            ),
            Expr::Not(_) => match children.into_iter().next() {
                Some(read) => Read {
                    wide: read.wide.swapped(),
                    narrow: read.narrow.map(Desc::swapped),
                },
                None => Read::unknown(),
            },
            _ => self.test(logic, rule, frame),
        }
    }

    fn unmodelled(&mut self, rule: usize, frame: &Frame<'_>, limit: Option<AuditLimit>) -> Read {
        self.unmodelled.push(Unmodelled {
            rule,
            seq: frame.seq,
            op: frame.node.tag(),
            pointer: frame.pointer.clone(),
            limit,
        });
        Read::unknown()
    }

    fn test<L: Logic>(&mut self, logic: &L, rule: usize, frame: &Frame<'_>) -> Read {
        let node = frame.node;
        match classify(node, logic) {
            Test::Constant => {
                let truth = evaluate_tree(
                    node,
                    &Value::Null(NullCause::Null),
                    logic,
                    Options::default(),
                )
                .ok()
                .and_then(|tree| tree.truth());
                let all = || vec![Region::default()];
                // TRUE fires; FALSE is the truth whose negation fires; the
                // rest is unknown (`spec/audit.md`, *Tests*).
                match truth {
                    Some(t) if logic.fires(t) => Read::exact(Desc {
                        t: all(),
                        f: Vec::new(),
                    }),
                    Some(t) if logic.fires(logic.not(t)) => Read::exact(Desc {
                        t: Vec::new(),
                        f: all(),
                    }),
                    _ => {
                        self.incompatible(rule, frame, false);
                        Read::exact(Desc::default())
                    }
                }
            }
            Test::NotModelled(string_operand) => {
                if let Some(field) = string_operand {
                    let id = self.subjects.id(field);
                    self.expect(id, Group::String, rule);
                }
                self.unmodelled(rule, frame, None)
            }
            Test::Modelled(test) => {
                let id = self.subjects.id(test.subject);
                let universe = &self.subjects.list[id].universe;
                let t = test.t.meet(universe);
                let f = test.f.meet(universe);
                if t.is_empty() && f.is_empty() {
                    self.incompatible(rule, frame, test.null_constant);
                } else if matches!(test.subject.expr(), Expr::Field(_)) {
                    for group in test.groups {
                        self.expect(id, group, rule);
                    }
                }
                Read::exact(Desc {
                    t: Region::on(id, t),
                    f: Region::on(id, f),
                })
            }
        }
    }

    fn incompatible(&mut self, rule: usize, frame: &Frame<'_>, null_constant: bool) {
        self.incompatible.push(Incompatible {
            rule,
            seq: frame.seq,
            pointer: frame.pointer.clone(),
            text: frame.node.to_string(),
            null_constant,
        });
    }

    fn join(&mut self, connective: Connective, descs: &[&Desc]) -> Result<Desc, AuditLimit> {
        let ts: Vec<&[Region]> = descs.iter().map(|d| d.t.as_slice()).collect();
        let fs: Vec<&[Region]> = descs.iter().map(|d| d.f.as_slice()).collect();
        Ok(match connective {
            Connective::And => Desc {
                t: product(&ts, self.max_terms, &mut self.work)?,
                f: concat(&fs, self.max_terms)?,
            },
            Connective::Or => Desc {
                t: concat(&ts, self.max_terms)?,
                f: product(&fs, self.max_terms, &mut self.work)?,
            },
        })
    }

    fn combine(
        &mut self,
        connective: Connective,
        reads: &[Read],
        rule: usize,
        seq: usize,
        op: &'static str,
        pointer: String,
    ) -> Read {
        let wide: Vec<&Desc> = reads.iter().map(|r| &r.wide).collect();
        let combined = self.join(connective, &wide).and_then(|wide| {
            if reads.iter().all(|r| r.narrow.is_none()) {
                return Ok(Read::exact(wide));
            }
            let narrow: Vec<&Desc> = reads.iter().map(Read::narrow).collect();
            let narrow = self.join(connective, &narrow)?;
            Ok(Read {
                wide,
                narrow: Some(narrow),
            })
        });
        combined.unwrap_or_else(|limit| {
            self.unmodelled.push(Unmodelled {
                rule,
                seq,
                op,
                pointer,
                limit: Some(limit),
            });
            Read::unknown()
        })
    }
}
