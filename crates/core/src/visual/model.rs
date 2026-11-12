//! What a diagram holds: nodes with lines and a class, edges and a title
//! (`spec/visual.md`, *Diagram content*), built from conditions, rules, rule
//! sets and explanations, and counted against `max_diagram_size`.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::ToMark;
use crate::ast::{Expr, Node};
use crate::error::{Error, ErrorCode, Result};
use crate::explain::text::{
    counterfactual, field, header, name, note, policy, reason, status, value,
};
use crate::explain::{Cause, Explanation, GroupKind, Requirement, Status, Summary};
use crate::json::Limits;
use crate::rules::{HitPolicy, Rule};

/// The longest line of a node or cell of a grid, in characters.
const MAX_LINE: usize = 100;

/// What a diagram shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Condition,
    Rule,
    RuleSet,
    Explanation,
    Table,
}

impl Kind {
    /// Its name in an HTML document's title.
    pub(super) fn title(self) -> &'static str {
        match self {
            Kind::Condition => "condition",
            Kind::Rule => "rule",
            Kind::RuleSet => "rule set",
            Kind::Explanation => "explanation",
            Kind::Table => "decision table",
        }
    }

    /// Its name in the HTML class `cleanrules-<kind>`.
    pub(super) fn css(self) -> &'static str {
        match self {
            Kind::Condition => "condition",
            Kind::Rule => "rule",
            Kind::RuleSet => "rule-set",
            Kind::Explanation => "explanation",
            Kind::Table => "decision-table",
        }
    }

    /// Whether it is drawn left to right.
    pub(super) fn across(self) -> bool {
        self == Kind::RuleSet
    }
}

/// How a node is drawn (`spec/visual.md`, *Classes*), in the order of the
/// table there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Class {
    Connective,
    Leaf,
    Rule,
    RuleSelected,
    RuleFired,
    RuleNotFired,
    RuleUnknown,
    TruthTrue,
    TruthFalse,
    TruthUnknown,
    TruthDegree,
    Field,
    Output,
}

/// A node's outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// An ellipse, a stadium in Mermaid.
    Round,
    /// A box.
    Box,
    /// A box with rounded corners.
    Rounded,
    /// A box, a subroutine in Mermaid.
    Output,
}

/// A node's stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stroke {
    Plain,
    Bold,
    Dashed,
}

/// A class's look: fill, stroke colour, shape and stroke.
pub(super) struct Look {
    pub(super) fill: &'static str,
    pub(super) color: &'static str,
    pub(super) shape: Shape,
    pub(super) stroke: Stroke,
}

impl Class {
    /// Every class, in order.
    pub(super) const ALL: [Class; 13] = [
        Class::Connective,
        Class::Leaf,
        Class::Rule,
        Class::RuleSelected,
        Class::RuleFired,
        Class::RuleNotFired,
        Class::RuleUnknown,
        Class::TruthTrue,
        Class::TruthFalse,
        Class::TruthUnknown,
        Class::TruthDegree,
        Class::Field,
        Class::Output,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Class::Connective => "connective",
            Class::Leaf => "leaf",
            Class::Rule => "rule",
            Class::RuleSelected => "rule_selected",
            Class::RuleFired => "rule_fired",
            Class::RuleNotFired => "rule_not_fired",
            Class::RuleUnknown => "rule_unknown",
            Class::TruthTrue => "truth_true",
            Class::TruthFalse => "truth_false",
            Class::TruthUnknown => "truth_unknown",
            Class::TruthDegree => "truth_degree",
            Class::Field => "field",
            Class::Output => "output",
        }
    }

    pub(super) fn look(self) -> Look {
        let look = |fill, color, shape, stroke| Look {
            fill,
            color,
            shape,
            stroke,
        };
        match self {
            Class::Connective | Class::Field => {
                look("#ffffff", "#5f6368", Shape::Round, Stroke::Plain)
            }
            Class::Leaf => look("#f8f9fa", "#5f6368", Shape::Box, Stroke::Plain),
            Class::Rule => look("#ffffff", "#202124", Shape::Rounded, Stroke::Plain),
            Class::RuleSelected => look("#e8f0fe", "#1a73e8", Shape::Rounded, Stroke::Bold),
            Class::RuleFired => look("#ffffff", "#1a73e8", Shape::Rounded, Stroke::Plain),
            Class::RuleNotFired => look("#ffffff", "#5f6368", Shape::Rounded, Stroke::Plain),
            Class::RuleUnknown => look("#f1f3f4", "#80868b", Shape::Rounded, Stroke::Dashed),
            Class::TruthTrue => look("#e6f4ea", "#1e8e3e", Shape::Box, Stroke::Plain),
            Class::TruthFalse => look("#fce8e6", "#d93025", Shape::Box, Stroke::Plain),
            Class::TruthUnknown => look("#f1f3f4", "#80868b", Shape::Box, Stroke::Dashed),
            Class::TruthDegree => look("#fef7e0", "#f9ab00", Shape::Box, Stroke::Plain),
            Class::Output => look("#f3e8fd", "#9334e6", Shape::Output, Stroke::Plain),
        }
    }

    /// The class's CSS declarations, the same in the `<style>` element and in
    /// the `style` attribute of each element that has the class
    /// (`spec/visual.md`, *HTML*).
    pub(super) fn css(self) -> String {
        let look = self.look();
        let (width, line) = match look.stroke {
            Stroke::Plain => ("1px", "solid"),
            Stroke::Bold => ("2px", "solid"),
            Stroke::Dashed => ("1px", "dashed"),
        };
        let radius = match look.shape {
            Shape::Round => ";border-radius:12px",
            Shape::Rounded => ";border-radius:8px",
            Shape::Box | Shape::Output => "",
        };
        format!(
            "background:{};border:{width} {line} {}{radius}",
            look.fill, look.color
        )
    }

    /// The class of a rule's status.
    pub(super) fn of_status(status: Status) -> Self {
        match status {
            Status::Selected => Class::RuleSelected,
            Status::Fired => Class::RuleFired,
            Status::NotFired => Class::RuleNotFired,
            Status::Unknown => Class::RuleUnknown,
        }
    }

    fn of_truth(class: super::TruthClass) -> Self {
        match class {
            super::TruthClass::True => Class::TruthTrue,
            super::TruthClass::False => Class::TruthFalse,
            super::TruthClass::Unknown => Class::TruthUnknown,
            super::TruthClass::Degree => Class::TruthDegree,
        }
    }
}

/// A line of a node, a title or a cell: no control character (which labels
/// never hold: their text is escaped as the explanation's text escapes it), and
/// at most [`MAX_LINE`] characters, cut to 97 and `...`.
pub(super) fn line(text: &str) -> String {
    let clean = text
        .chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c });
    if text.chars().count() <= MAX_LINE {
        clean.collect()
    } else {
        clean.take(MAX_LINE - 3).chain("...".chars()).collect()
    }
}

/// The error of a diagram larger than its limit (`spec/visual.md`, *Limits*),
/// naming the kind of diagram.
pub(super) fn too_large(kind: Kind, limit: usize) -> Error {
    let mut error = Error::new(
        ErrorCode::LimitExceeded,
        format!(
            "the {} diagram has more than {limit} elements (max_diagram_size)",
            kind.title()
        ),
    )
    .with_hint("draw a smaller part, such as one rule or its condition, or raise max_diagram_size");
    error.context_mut().name = Some("max_diagram_size".to_owned());
    error
}

/// A node: its lines, class and description (drawn in HTML only).
pub(super) struct Item {
    pub(super) lines: Vec<String>,
    pub(super) class: Class,
    pub(super) description: Option<String>,
}

/// An edge between two nodes, by their positions.
pub(super) struct Edge {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) dashed: bool,
}

/// A diagram being built, or built.
pub(super) struct Diagram {
    pub(super) kind: Kind,
    pub(super) title: Option<String>,
    pub(super) nodes: Vec<Item>,
    pub(super) edges: Vec<Edge>,
    limit: usize,
}

impl Diagram {
    fn new(kind: Kind, title: Option<&str>, limits: &Limits) -> Self {
        Self {
            kind,
            title: title.map(line),
            nodes: Vec::new(),
            edges: Vec::new(),
            limit: limits.max_diagram_size,
        }
    }

    /// Count one more element.
    fn grow(&self) -> Result<()> {
        if self.nodes.len() + self.edges.len() >= self.limit {
            Err(too_large(self.kind, self.limit))
        } else {
            Ok(())
        }
    }

    fn node(&mut self, lines: &[String], class: Class, description: Option<&str>) -> Result<usize> {
        self.grow()?;
        self.nodes.push(Item {
            lines: lines.iter().map(|l| line(l)).collect(),
            class,
            description: description.map(str::to_owned),
        });
        Ok(self.nodes.len() - 1)
    }

    fn edge(&mut self, from: usize, to: usize, dashed: bool) -> Result<()> {
        self.grow()?;
        self.edges.push(Edge { from, to, dashed });
        Ok(())
    }

    /// The nodes of a condition, in pre-order, with an edge from `parent` to
    /// its root (`spec/visual.md`, *Conditions*).
    fn condition(&mut self, root: &Node, parent: Option<usize>) -> Result<()> {
        // An explicit stack: a condition is as deep as the reader's limits.
        let mut pending = vec![(root, parent)];
        while let Some((node, parent)) = pending.pop() {
            let (label, operands): (Option<&str>, &[Node]) = match node.expr() {
                Expr::And(operands) => (Some("all"), operands),
                Expr::Or(operands) => (Some("any"), operands),
                Expr::Not(operand) => (Some("not"), std::slice::from_ref(&**operand)),
                _ => (None, &[]),
            };
            let id = match label {
                Some(label) => self.node(&[label.to_owned()], Class::Connective, None)?,
                None => self.node(&[node.to_string()], Class::Leaf, None)?,
            };
            if let Some(parent) = parent {
                self.edge(parent, id, false)?;
            }
            pending.extend(operands.iter().rev().map(|o| (o, Some(id))));
        }
        Ok(())
    }
}

/// A condition's diagram.
pub(super) fn condition(node: &Node, limits: &Limits) -> Result<Diagram> {
    let mut diagram = Diagram::new(Kind::Condition, None, limits);
    diagram.condition(node, None)?;
    Ok(diagram)
}

/// A rule's diagram (`spec/visual.md`, *Rules*).
pub(super) fn rule(rule: &Rule, limits: &Limits) -> Result<Diagram> {
    let mut diagram = Diagram::new(Kind::Rule, None, limits);
    let mut head = format!("RULE {}", name(rule.name()));
    if rule.get_priority() != 0 {
        let _ = write!(head, " PRIORITY {}", rule.get_priority());
    }
    let root = diagram.node(&[head], Class::Rule, rule.get_description())?;
    diagram.condition(rule.when(), Some(root))?;
    for (key, value) in rule.then() {
        let output = diagram.node(
            &[format!("THEN {} = {value}", name(key))],
            Class::Output,
            None,
        )?;
        diagram.edge(root, output, true)?;
    }
    if let Some(action) = rule.get_action_name() {
        let output = diagram.node(&[format!("DO {}", name(action))], Class::Output, None)?;
        diagram.edge(root, output, true)?;
    }
    Ok(diagram)
}

/// A rule set's diagram, from its summary (`spec/visual.md`, *Rule sets*).
pub(super) fn ruleset(summary: &Summary<'_>, limits: &Limits) -> Result<Diagram> {
    let ruleset = summary.ruleset;
    let rules = ruleset.rules();
    let by_priority = ruleset.hit_policy() == HitPolicy::Priority;
    let mut diagram = Diagram::new(Kind::RuleSet, Some(&policy(ruleset)), limits);
    // The rank: by priority, highest first, ties in rule order (a stable
    // sort), or rule order.
    let mut ranked: Vec<usize> = (0..rules.len()).collect();
    if by_priority {
        ranked.sort_by_key(|&i| Reverse(rules[i].get_priority()));
    }
    let fields = summary
        .fields
        .iter()
        .map(|usage| diagram.node(&[field(&usage.path)], Class::Field, None))
        .collect::<Result<Vec<usize>>>()?;
    let mut nodes = vec![0; rules.len()];
    for (rank, &i) in ranked.iter().enumerate() {
        let rule = &rules[i];
        let mut lines = vec![format!("{}. {}", rank + 1, name(rule.name()))];
        if by_priority {
            lines.push(format!("PRIORITY {}", rule.get_priority()));
        }
        lines.push(rule.when().to_string());
        if summary.actions.get(i).copied().unwrap_or(false) {
            lines.push(match rule.get_action_name() {
                Some(action) => format!("DO {}", name(action)),
                None => "+action".to_owned(),
            });
        }
        nodes[i] = diagram.node(&lines, Class::Rule, rule.get_description())?;
    }
    let mut outputs = BTreeMap::new();
    for key in &summary.outputs {
        outputs.insert(
            key.as_str(),
            diagram.node(&[name(key)], Class::Output, None)?,
        );
    }
    // What each rule reads, by field: (field, only by its payload).
    let position: BTreeMap<&str, usize> = rules
        .iter()
        .enumerate()
        .map(|(i, rule)| (rule.name(), i))
        .collect();
    let mut reads: Vec<Vec<(usize, bool)>> = vec![Vec::new(); rules.len()];
    for (f, usage) in summary.fields.iter().enumerate() {
        let conditions = usage.conditions.iter().map(|rule| (rule, false));
        let payloads = usage.payloads.iter().map(|(rule, _)| (rule, true));
        for (rule, payload) in conditions.chain(payloads) {
            if let Some(&i) = position.get(rule.as_str()) {
                reads[i].push((f, payload));
            }
        }
    }
    for &i in &ranked {
        let mut read = std::mem::take(&mut reads[i]);
        // Condition reads first, then payload reads, each in field order;
        // a field read by both has its solid edge only.
        read.sort_unstable_by_key(|&(f, payload)| (payload, f));
        let mut seen = std::collections::BTreeSet::new();
        for (f, payload) in read {
            if seen.insert(f) {
                diagram.edge(fields[f], nodes[i], payload)?;
            }
        }
        for (key, _) in rules[i].then() {
            if let Some(&output) = outputs.get(key.as_str()) {
                diagram.edge(nodes[i], output, false)?;
            }
        }
    }
    Ok(diagram)
}

/// The lines and class of a cause (`spec/visual.md`, *Explanations*).
fn cause(cause: &Cause<'_, '_>) -> (Vec<String>, Class) {
    let mark = cause.truth().to_mark();
    let mut state = mark.label;
    if !cause.values.is_empty() {
        let shown: Vec<String> = cause
            .values
            .iter()
            .map(|(p, v)| format!("{} = {}", field(p), value(v)))
            .collect();
        let _ = write!(state, " ({})", shown.join(", "));
    }
    if let Some(why) = cause.reason() {
        state.push_str(": ");
        state.push_str(&reason(why));
    } else if !cause.holds() && cause.negated {
        state.push_str(", but the rule needs it false");
    }
    let mut lines = vec![cause.result.node.to_string(), state];
    lines.extend(counterfactual(cause));
    (lines, Class::of_truth(mark.class))
}

/// An explanation's diagram (`spec/visual.md`, *Explanations*).
pub(super) fn explanation(explanation: &Explanation<'_, '_>, limits: &Limits) -> Result<Diagram> {
    let mut diagram = Diagram::new(Kind::Explanation, Some(&header(explanation)), limits);
    for explained in &explanation.rules {
        let rule = explained.rule;
        let (mark, words) = status(explained.status);
        let mut lines = vec![
            format!("{mark} {} ({words})", name(rule.name())),
            rule.when().to_string(),
        ];
        lines.extend(explained.note.as_ref().map(note));
        if explained.achievable == Some(false) {
            lines.push("cannot fire by changing these conditions alone; they conflict".to_owned());
        }
        let root = diagram.node(
            &lines,
            Class::of_status(explained.status),
            rule.get_description(),
        )?;
        // In pre-order, with an explicit stack: a requirement is as deep as
        // its condition.
        let mut pending = vec![(&explained.requirement, root)];
        while let Some((requirement, parent)) = pending.pop() {
            let id = match requirement {
                Requirement::Group(kind, items) => {
                    let label = match kind {
                        GroupKind::All => "all of",
                        GroupKind::Any => "any of",
                    };
                    let id = diagram.node(&[label.to_owned()], Class::Connective, None)?;
                    pending.extend(items.iter().rev().map(|item| (item, id)));
                    id
                }
                Requirement::Cause(c) => {
                    let (lines, class) = cause(c);
                    diagram.node(&lines, class, None)?
                }
            };
            diagram.edge(parent, id, false)?;
        }
    }
    Ok(diagram)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_cut_and_cleaned() {
        assert_eq!(line("x > 1"), "x > 1");
        let hundred = "é".repeat(100);
        assert_eq!(line(&hundred), hundred);
        assert_eq!(line(&"é".repeat(101)), format!("{}...", "é".repeat(97)));
        assert_eq!(line("a\nb\u{7f}"), "a\u{fffd}b\u{fffd}");
    }

    #[test]
    fn every_class_has_a_look() {
        let names: Vec<&str> = Class::ALL.iter().map(|c| c.name()).collect();
        let mut sorted = Class::ALL.to_vec();
        sorted.sort();
        assert_eq!(
            sorted,
            Class::ALL,
            "ALL is in the order of the spec's table"
        );
        assert_eq!(names.len(), 13);
        for class in Class::ALL {
            let look = class.look();
            assert!(look.fill.starts_with('#') && look.color.starts_with('#'));
        }
    }
}
