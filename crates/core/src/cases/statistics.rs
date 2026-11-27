//! The statistics and coverage of a run (`spec/tests.md`, *Statistics*).
//!
//! A run asked for them ([`super::Runner::with_statistics`]) counts, from
//! each case's decision and before the next case is evaluated, how many times
//! each rule was selected, fired, did not fire or was unknown, and how many
//! times each of its conditions was `TRUE`, `FALSE` or `UNKNOWN`: the tests
//! of a rule set's conditions, or those of a decision table's input cells.
//! Only the counts are kept between cases ([`Counts`], laid out by a
//! [`Layout`] made at the run's first result); names and pointers are added
//! when the [`Statistics`] are made.
//!
//! The counts take each rule's status and the truth values of its tests in
//! pre-order, not a result tree ([`Counts::decision`]), so an evaluator that
//! keeps no tree (task T.6) can feed them.

use crate::ast::{Expr, Node};
use crate::eval::{Evaluated, Options, evaluate_tree};
use crate::explain::{Status, statuses};
use crate::logic::{Kleene, Truth};
use crate::rules::{Decision, RuleSet, RulesSource};
use crate::table::DecisionTable;
use crate::value::{NullCause, Value};

/// A measure of coverage (`spec/tests.md`, *Coverage*). Later versions may
/// add measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CoverageMeasure {
    /// Rules selected at least once.
    RuleSelected,
    /// Rules that fired at least once.
    RuleFired,
    /// Rules whose condition was `FALSE` at least once.
    RuleNotFired,
    /// Rules whose condition was `UNKNOWN` at least once.
    RuleUnknown,
    /// Conditions `TRUE` at least once.
    ConditionTrue,
    /// Conditions `FALSE` at least once.
    ConditionFalse,
    /// Conditions `UNKNOWN` at least once.
    ConditionUnknown,
    /// Output values of a decision table given at least once.
    OutputValue,
}

impl CoverageMeasure {
    /// Every measure, in the order of the coverage. A slice, since later
    /// versions may add measures.
    pub const ALL: &'static [CoverageMeasure] = &[
        CoverageMeasure::RuleSelected,
        CoverageMeasure::RuleFired,
        CoverageMeasure::RuleNotFired,
        CoverageMeasure::RuleUnknown,
        CoverageMeasure::ConditionTrue,
        CoverageMeasure::ConditionFalse,
        CoverageMeasure::ConditionUnknown,
        CoverageMeasure::OutputValue,
    ];

    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CoverageMeasure::RuleSelected => "rule_selected",
            CoverageMeasure::RuleFired => "rule_fired",
            CoverageMeasure::RuleNotFired => "rule_not_fired",
            CoverageMeasure::RuleUnknown => "rule_unknown",
            CoverageMeasure::ConditionTrue => "condition_true",
            CoverageMeasure::ConditionFalse => "condition_false",
            CoverageMeasure::ConditionUnknown => "condition_unknown",
            CoverageMeasure::OutputValue => "output_value",
        }
    }

    /// The measure of a name in the format.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|m| m.as_str() == name)
    }

    /// How the text names it (`spec/tests.md`, *Statistics*, *Text*).
    pub(crate) fn words(self) -> &'static str {
        match self {
            CoverageMeasure::RuleSelected => "rules selected",
            CoverageMeasure::RuleFired => "rules fired",
            CoverageMeasure::RuleNotFired => "rules not fired",
            CoverageMeasure::RuleUnknown => "rules unknown",
            CoverageMeasure::ConditionTrue => "conditions true",
            CoverageMeasure::ConditionFalse => "conditions false",
            CoverageMeasure::ConditionUnknown => "conditions unknown",
            CoverageMeasure::OutputValue => "output values",
        }
    }
}
/// Whether an item whose truth value is `constant` (`None` when it reads a
/// field) can have the truth value `truth` (`spec/tests.md`, *Coverage*).
fn reaches(constant: Option<Truth>, truth: Truth) -> bool {
    constant.is_none_or(|c| c == truth)
}

/// The counts of one condition: a test of a rule set's condition, or the
/// test of a decision table's input cell (`spec/tests.md`, *Conditions*).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConditionStatistics {
    /// Where it is: a JSON Pointer into the document of the rules tested; a
    /// decision table's cell, for its test.
    pub pointer: String,
    /// The `op` of the test's node.
    pub op: &'static str,
    /// The decisions in which it was `TRUE`.
    pub true_count: usize,
    /// The decisions in which it was `FALSE`.
    pub false_count: usize,
    /// The decisions in which it was `UNKNOWN`.
    pub unknown_count: usize,
    /// The input column of a decision table's cell, which the text names.
    column: Option<usize>,
    /// Its truth value, when it reads no field.
    constant: Option<Truth>,
}

impl ConditionStatistics {
    /// The decisions in which it had the truth value `truth`.
    #[must_use]
    pub fn count(&self, truth: Truth) -> usize {
        match truth {
            Truth::True => self.true_count,
            Truth::False => self.false_count,
            Truth::Unknown => self.unknown_count,
        }
    }

    /// The input column of a decision table's cell; `None` in a rule set.
    #[must_use]
    pub fn column(&self) -> Option<usize> {
        self.column
    }

    /// Its truth value when it reads no field, the same for every input
    /// (`spec/tests.md`, *Coverage*); `None` otherwise.
    #[must_use]
    pub fn constant(&self) -> Option<Truth> {
        self.constant
    }
}

/// The counts of one rule (`spec/tests.md`, *Rules*).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RuleStatistics {
    /// Its name, as decisions and audits name rules.
    pub name: String,
    /// Its id, when it has one.
    pub id: Option<String>,
    /// For a decision table, the row's position, from 0.
    pub row: Option<usize>,
    /// Where it is: a JSON Pointer to the rule, or to a decision table's row.
    pub pointer: String,
    /// The decisions that selected it.
    pub selected: usize,
    /// The decisions in which it fired, selected or not. This is not the
    /// rule status [`Status::Fired`] of an explanation, a rule that fired and
    /// was not selected, which these counts give as `fired - selected`.
    pub fired: usize,
    /// The decisions in which its condition was `FALSE`.
    pub not_fired: usize,
    /// The decisions in which its condition was `UNKNOWN`.
    pub unknown: usize,
    /// Its conditions, in order.
    pub conditions: Vec<ConditionStatistics>,
    /// The truth value of its condition, when it reads no field.
    constant: Option<Truth>,
}

impl RuleStatistics {
    /// The truth value of its condition when it reads no field, the same for
    /// every input (`spec/tests.md`, *Coverage*); `None` otherwise.
    #[must_use]
    pub fn constant(&self) -> Option<Truth> {
        self.constant
    }
}

/// One output value of a decision table and how many times it was given.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OutputValueStatistics {
    /// The value.
    pub value: Value,
    /// The selections of rows whose cell holds it.
    pub selected: usize,
    /// Where it is listed in the table.
    pointer: String,
}

impl OutputValueStatistics {
    /// Where it is listed in the table: a JSON Pointer to the value in its
    /// output's `values`.
    #[must_use]
    pub fn pointer(&self) -> &str {
        &self.pointer
    }
}

/// An output column of a decision table with output values.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct OutputStatistics {
    /// The output's name.
    pub output: String,
    /// Its values, in its order.
    pub values: Vec<OutputValueStatistics>,
}

/// An item of a measure that the cases never reached, with its place.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Missed {
    /// A rule.
    Rule {
        /// Its name.
        rule: String,
        /// Its pointer: the rule, or a decision table's row.
        pointer: String,
    },
    /// A condition of a rule.
    Condition {
        /// The rule's name.
        rule: String,
        /// The condition's pointer.
        pointer: String,
    },
    /// An output value of a decision table.
    OutputValue {
        /// The output's name.
        output: String,
        /// The value.
        value: Value,
        /// Its pointer, in its output's `values`.
        pointer: String,
    },
}

/// One measure of coverage (`spec/tests.md`, *Coverage*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Coverage {
    /// The measure.
    pub measure: CoverageMeasure,
    /// The items reached at least once.
    pub covered: usize,
    /// The items that can be reached.
    pub total: usize,
    /// The items that read no field and so can never be reached, left out
    /// of `total` and `missed`.
    pub unreachable: usize,
    /// The items never reached, in order.
    pub missed: Vec<Missed>,
}

/// An item missed, borrowed from the statistics.
pub(crate) enum MissedRef<'s> {
    Rule(&'s RuleStatistics),
    Condition(&'s RuleStatistics, &'s ConditionStatistics),
    OutputValue(&'s OutputStatistics, &'s OutputValueStatistics),
}

impl MissedRef<'_> {
    fn to_owned(&self) -> Missed {
        match self {
            MissedRef::Rule(rule) => Missed::Rule {
                rule: rule.name.clone(),
                pointer: rule.pointer.clone(),
            },
            MissedRef::Condition(rule, condition) => Missed::Condition {
                rule: rule.name.clone(),
                pointer: condition.pointer.clone(),
            },
            MissedRef::OutputValue(output, value) => Missed::OutputValue {
                output: output.output.clone(),
                value: value.value.clone(),
                pointer: value.pointer.clone(),
            },
        }
    }
}

/// A measure with its items missed borrowed.
pub(crate) struct MeasureRef<'s> {
    pub(crate) measure: CoverageMeasure,
    pub(crate) covered: usize,
    pub(crate) total: usize,
    pub(crate) unreachable: usize,
    pub(crate) missed: Vec<MissedRef<'s>>,
}

impl<'s> MeasureRef<'s> {
    /// A measure of `items`, each with whether it can be reached and whether
    /// it was.
    fn of(
        measure: CoverageMeasure,
        items: impl IntoIterator<Item = (MissedRef<'s>, bool, bool)>,
    ) -> Self {
        let (mut total, mut unreachable, mut missed) = (0, 0, Vec::new());
        for (item, reachable, reached) in items {
            if !reachable {
                unreachable += 1;
                continue;
            }
            total += 1;
            if !reached {
                missed.push(item);
            }
        }
        Self {
            measure,
            covered: total - missed.len(),
            total,
            unreachable,
            missed,
        }
    }
}

/// A condition's place: its pointer, its op, its test's node and, in a
/// table, its column.
type Place<'n> = (String, &'static str, &'n Node, Option<usize>);

/// One of the counts of a rule.
type RuleCount = fn(&RuleStatistics) -> usize;

/// What a run's cases made the rules do (`spec/tests.md`, *Statistics*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Statistics {
    /// What was tested.
    pub source: RulesSource,
    /// The cases run, with or without a decision.
    pub cases: usize,
    /// The cases whose evaluation gave a decision.
    pub decisions: usize,
    /// Each rule, in rule order.
    pub rules: Vec<RuleStatistics>,
    /// For a decision table, each output column with output values, in
    /// column order; `None` for a rule set.
    pub outputs: Option<Vec<OutputStatistics>>,
    /// A decision table's input headers, which the text names cells by.
    inputs: Vec<String>,
}

impl Statistics {
    /// The coverage, one entry per measure in order; `output_value` only for
    /// a decision table with output values.
    #[must_use]
    pub fn coverage(&self) -> Vec<Coverage> {
        self.measures()
            .into_iter()
            .map(|m| Coverage {
                measure: m.measure,
                covered: m.covered,
                total: m.total,
                unreachable: m.unreachable,
                missed: m.missed.iter().map(MissedRef::to_owned).collect(),
            })
            .collect()
    }

    /// The coverage with its items missed borrowed.
    pub(crate) fn measures(&self) -> Vec<MeasureRef<'_>> {
        let mut out = Vec::new();
        let rule_measures: [(CoverageMeasure, RuleCount, Truth); 4] = [
            (CoverageMeasure::RuleSelected, |r| r.selected, Truth::True),
            (CoverageMeasure::RuleFired, |r| r.fired, Truth::True),
            (CoverageMeasure::RuleNotFired, |r| r.not_fired, Truth::False),
            (CoverageMeasure::RuleUnknown, |r| r.unknown, Truth::Unknown),
        ];
        for (measure, count, truth) in rule_measures {
            out.push(MeasureRef::of(
                measure,
                self.rules
                    .iter()
                    .map(|r| (MissedRef::Rule(r), reaches(r.constant, truth), count(r) > 0)),
            ));
        }
        for (measure, truth) in [
            (CoverageMeasure::ConditionTrue, Truth::True),
            (CoverageMeasure::ConditionFalse, Truth::False),
            (CoverageMeasure::ConditionUnknown, Truth::Unknown),
        ] {
            out.push(MeasureRef::of(
                measure,
                self.rules.iter().flat_map(|r| {
                    r.conditions.iter().map(move |c| {
                        (
                            MissedRef::Condition(r, c),
                            reaches(c.constant, truth),
                            c.count(truth) > 0,
                        )
                    })
                }),
            ));
        }
        if let Some(outputs) = self.outputs.as_ref().filter(|o| !o.is_empty()) {
            out.push(MeasureRef::of(
                CoverageMeasure::OutputValue,
                outputs.iter().flat_map(|o| {
                    o.values
                        .iter()
                        .map(move |v| (MissedRef::OutputValue(o, v), true, v.selected > 0))
                }),
            ));
        }
        out
    }

    /// The header of a decision table's input column, in the text syntax.
    pub(crate) fn input_header(&self, column: usize) -> Option<&str> {
        self.inputs.get(column).map(String::as_str)
    }
}

/// How the rules tested are laid out for counting: made once, at a run's
/// first result, so that counting a decision only adds to counts.
#[derive(Clone, Debug)]
pub(crate) struct Layout {
    /// The number of conditions of each rule, in rule order.
    conditions: Vec<usize>,
    /// For a decision table, each rule's output values.
    table: Option<TableLayout>,
}

#[derive(Clone, Debug)]
struct TableLayout {
    /// For each rule, the position of each output cell in its column's
    /// values, for the outputs that have values.
    positions: Vec<Vec<Option<usize>>>,
    /// The number of values of each output that has values.
    values: Vec<usize>,
}

/// The node a test strips its `not`s from: a cell's `NOT IN` is the test
/// `in`, as in a rule set's condition.
fn without_not(mut node: &Node) -> &Node {
    while let Expr::Not(operand) = node.expr() {
        node = operand;
    }
    node
}

/// The number of tests of a condition (`spec/audit.md`, *Tests*), with an
/// explicit stack, as a condition nests as deep as the reader allows.
fn test_count(when: &Node) -> usize {
    let mut count = 0;
    let mut stack = vec![when];
    while let Some(node) = stack.pop() {
        match node.expr() {
            Expr::And(operands) | Expr::Or(operands) => stack.extend(operands.iter()),
            Expr::Not(operand) => stack.push(operand),
            _ => count += 1,
        }
    }
    count
}

/// The tests of a condition in depth-first pre-order, each with its pointer
/// below the condition's.
fn tests<'n>(when: &'n Node, pointer: &str) -> Vec<(String, &'static str, &'n Node)> {
    let mut out = Vec::new();
    let mut stack = vec![(when, pointer.to_owned())];
    while let Some((node, at)) = stack.pop() {
        match node.expr() {
            Expr::And(operands) | Expr::Or(operands) => {
                for (k, operand) in operands.iter().enumerate().rev() {
                    stack.push((operand, format!("{at}/operands/{k}")));
                }
            }
            Expr::Not(operand) => stack.push((operand, format!("{at}/operand"))),
            _ => out.push((at, node.tag(), node)),
        }
    }
    out
}

/// The truth values of the tests of a condition's result tree, in
/// pre-order: the tree has a child per operand of a connective
/// (`spec/json-format.md`, *Result trees*).
fn test_truths(when: &Evaluated<'_, Truth>) -> Vec<Truth> {
    let mut out = Vec::new();
    let mut stack = vec![when];
    while let Some(result) = stack.pop() {
        match result.node.expr() {
            Expr::And(_) | Expr::Or(_) | Expr::Not(_) => stack.extend(result.children.iter().rev()),
            _ => out.push(result.truth().unwrap_or(Truth::Unknown)),
        }
    }
    out
}

/// The truth value of a node that reads no field, the same for every input
/// (`spec/audit.md`, *What the audit reads*: constant); `None` for one that
/// reads a field.
fn constant(node: &Node) -> Option<Truth> {
    if !node.field_paths().is_empty() {
        return None;
    }
    let tree = evaluate_tree(
        node,
        &Value::Null(NullCause::Null),
        &Kleene,
        Options::default(),
    );
    Some(tree.ok()?.truth().unwrap_or(Truth::Unknown))
}

/// The cells of a decision table's row that are not `-`: each column with
/// its cell's node.
fn cells(table: &DecisionTable, row: usize) -> Vec<(usize, &Node)> {
    table.rows().get(row).map_or_else(Vec::new, |row| {
        row.inputs
            .iter()
            .enumerate()
            .filter_map(|(c, cell)| cell.as_ref().map(|node| (c, node)))
            .collect()
    })
}

impl Layout {
    /// The layout of a rule set's rules, or of a decision table's.
    pub(crate) fn new(ruleset: &RuleSet, table: Option<&DecisionTable>) -> Self {
        match table {
            None => Self {
                conditions: ruleset
                    .rules()
                    .iter()
                    .map(|rule| test_count(rule.when()))
                    .collect(),
                table: None,
            },
            Some(table) => {
                let rows = table.rule_rows();
                let by_row = table.output_value_positions();
                Self {
                    // A cell holds one test, under its `not`s.
                    conditions: rows.iter().map(|&r| cells(table, r).len()).collect(),
                    table: Some(TableLayout {
                        positions: rows
                            .iter()
                            .map(|&r| by_row.get(r).cloned().unwrap_or_default())
                            .collect(),
                        values: table
                            .outputs()
                            .iter()
                            .filter(|o| !o.values.is_empty())
                            .map(|o| o.values.len())
                            .collect(),
                    }),
                }
            }
        }
    }

    /// Whether the rule at `i` has conditions: a table's row with every
    /// cell `-` has none, though its condition is the constant `TRUE`.
    fn has_conditions(&self, i: usize) -> bool {
        self.conditions.get(i).is_some_and(|&n| n > 0)
    }
}

/// Each rule's status and the truth values of its tests in pre-order, from
/// a decision: what [`Counts::decision`] takes.
pub(crate) fn outcomes(decision: &Decision<'_>, layout: &Layout) -> Vec<(Status, Vec<Truth>)> {
    decision
        .results()
        .iter()
        .zip(statuses(decision))
        .enumerate()
        .map(|(i, (result, status))| {
            let truths = if layout.has_conditions(i) {
                test_truths(&result.when)
            } else {
                Vec::new()
            };
            (status, truths)
        })
        .collect()
}

/// The counts of one rule, without names.
#[derive(Clone, Debug, Default)]
struct RuleCounts {
    selected: usize,
    fired: usize,
    not_fired: usize,
    unknown: usize,
    /// Per condition: `TRUE`, `FALSE` and `UNKNOWN`.
    conditions: Vec<[usize; 3]>,
}

/// The counts a run keeps between results when it gathers statistics.
#[derive(Clone, Debug, Default)]
pub(crate) struct Counts {
    cases: usize,
    decisions: usize,
    rules: Vec<RuleCounts>,
    /// Per output with values, per value: the selections that gave it.
    values: Vec<Vec<usize>>,
}

/// The slot of a truth value in a condition's counts.
fn slot(truth: Truth) -> usize {
    match truth {
        Truth::True => 0,
        Truth::False => 1,
        Truth::Unknown => 2,
    }
}

impl Counts {
    pub(crate) fn new(layout: &Layout) -> Self {
        Self {
            cases: 0,
            decisions: 0,
            rules: layout
                .conditions
                .iter()
                .map(|&n| RuleCounts {
                    conditions: vec![[0; 3]; n],
                    ..RuleCounts::default()
                })
                .collect(),
            values: layout
                .table
                .as_ref()
                .map_or_else(Vec::new, |t| t.values.iter().map(|&n| vec![0; n]).collect()),
        }
    }

    /// Count a case whose evaluation raised an error: it has no decision.
    pub(crate) fn error(&mut self) {
        self.cases += 1;
    }

    /// Count a case's decision, given as each rule's status and the truth
    /// values of its tests in pre-order, in rule order.
    pub(crate) fn decision<T: IntoIterator<Item = Truth>>(
        &mut self,
        layout: &Layout,
        rules: impl IntoIterator<Item = (Status, T)>,
    ) {
        self.cases += 1;
        self.decisions += 1;
        for (i, (status, truths)) in rules.into_iter().enumerate() {
            let Some(rule) = self.rules.get_mut(i) else {
                continue;
            };
            match status {
                Status::Selected => {
                    rule.selected += 1;
                    rule.fired += 1;
                }
                Status::Fired => rule.fired += 1,
                Status::NotFired => rule.not_fired += 1,
                Status::Unknown => rule.unknown += 1,
            }
            let mut k = 0;
            for truth in truths {
                if let Some(counts) = rule.conditions.get_mut(k) {
                    counts[slot(truth)] += 1;
                }
                k += 1;
            }
            debug_assert_eq!(k, rule.conditions.len(), "the tests of rule {i}");
            if status == Status::Selected
                && let Some(positions) = layout.table.as_ref().and_then(|t| t.positions.get(i))
            {
                for (o, position) in positions.iter().enumerate() {
                    if let Some(count) =
                        position.and_then(|p| self.values.get_mut(o).and_then(|v| v.get_mut(p)))
                    {
                        *count += 1;
                    }
                }
            }
        }
    }

    /// The statistics these counts give, with the names and pointers of the
    /// rules tested: `table` when they are a decision table's, and every
    /// pointer under `prefix` (`/bundle` for a bundle's).
    pub(crate) fn statistics(
        &self,
        ruleset: &RuleSet,
        table: Option<&DecisionTable>,
        prefix: &str,
    ) -> Statistics {
        let rows = table.map(DecisionTable::rule_rows);
        let empty = RuleCounts::default();
        let rules = ruleset
            .rules()
            .iter()
            .enumerate()
            .map(|(i, rule)| {
                let counts = self.rules.get(i).unwrap_or(&empty);
                let (row, pointer, places) = if let (Some(table), Some(rows)) = (table, &rows) {
                    let r = rows.get(i).copied().unwrap_or(i);
                    let places: Vec<Place<'_>> = cells(table, r)
                        .into_iter()
                        .map(|(c, node)| {
                            let test = without_not(node);
                            (
                                format!("{prefix}/decision_table/rows/{r}/inputs/{c}"),
                                test.tag(),
                                test,
                                Some(c),
                            )
                        })
                        .collect();
                    (Some(r), format!("{prefix}/decision_table/rows/{r}"), places)
                } else {
                    let at = format!("{prefix}/ruleset/rules/{i}");
                    let places = tests(rule.when(), &format!("{at}/when"))
                        .into_iter()
                        .map(|(pointer, op, node)| (pointer, op, node, None))
                        .collect();
                    (None, at, places)
                };
                let conditions = places
                    .into_iter()
                    .enumerate()
                    .map(|(k, (pointer, op, node, column))| {
                        let [t, f, u] = counts.conditions.get(k).copied().unwrap_or([0; 3]);
                        ConditionStatistics {
                            pointer,
                            op,
                            true_count: t,
                            false_count: f,
                            unknown_count: u,
                            column,
                            constant: constant(node),
                        }
                    })
                    .collect();
                RuleStatistics {
                    name: rule.name().to_owned(),
                    id: rule.get_id().map(str::to_owned),
                    row,
                    pointer,
                    selected: counts.selected,
                    fired: counts.fired,
                    not_fired: counts.not_fired,
                    unknown: counts.unknown,
                    conditions,
                    constant: constant(rule.when()),
                }
            })
            .collect();
        Statistics {
            source: if table.is_some() {
                RulesSource::DecisionTable
            } else {
                RulesSource::RuleSet
            },
            cases: self.cases,
            decisions: self.decisions,
            rules,
            outputs: table.map(|table| self.outputs(table, prefix)),
            inputs: table.map_or_else(Vec::new, |t| {
                t.inputs().iter().map(ToString::to_string).collect()
            }),
        }
    }

    /// The output columns of a table that have values, with their counts.
    fn outputs(&self, table: &DecisionTable, prefix: &str) -> Vec<OutputStatistics> {
        table
            .outputs()
            .iter()
            .enumerate()
            .filter(|(_, o)| !o.values.is_empty())
            .zip(0..)
            .map(|((c, output), o)| OutputStatistics {
                output: output.name.clone(),
                values: output
                    .values
                    .iter()
                    .enumerate()
                    .map(|(p, value)| OutputValueStatistics {
                        value: value.clone(),
                        selected: self
                            .values
                            .get(o)
                            .and_then(|v| v.get(p))
                            .copied()
                            .unwrap_or(0),
                        pointer: format!("{prefix}/decision_table/outputs/{c}/values/{p}"),
                    })
                    .collect(),
            })
            .collect()
    }
}
