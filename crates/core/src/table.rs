//! Decision tables (`spec/tables.md`): inputs, outputs and rows, read and
//! written as JSON or CSV, compiled into a rule set.
//!
//! ```
//! use cleanrules::json::Limits;
//! use cleanrules::table::DecisionTable;
//! use cleanrules::{Map, Value};
//!
//! let csv = "HIT POLICY,first\nRULE,age,THEN band\nminor,< 18,'minor'\nadult,-,'adult'\n";
//! let table = DecisionTable::from_csv(csv, &Limits::default())?;
//! let mut person = Map::new();
//! person.insert("age", Value::Int(16));
//! let decision = table.ruleset().evaluate(&Value::Map(person))?;
//! let selected: Vec<&str> = decision.selected().map(|r| r.rule.name()).collect();
//! assert_eq!(selected, ["minor"]);
//! assert_eq!(table.to_csv(), csv);
//! # Ok::<(), cleanrules::Error>(())
//! ```

mod csv;
mod grid;

use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::ast::{Kind, Node};
use crate::error::{Error, ErrorCode, Result};
use crate::json::Limits;
use crate::logic::LogicName;
use crate::rules::{Aggregate, Config, HitPolicy, Rule, RuleSet};
use crate::text::{cell_text, check_keys};
use crate::value::Value;

pub use grid::{Cell, GridBuilder, HeaderItem};

/// A decision table's hit policy: the rule set policies and `output_order`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TablePolicy {
    /// The first row that fires.
    #[default]
    First,
    /// The row with the highest output priority.
    Priority,
    /// The only row that fires.
    Unique,
    /// The first row that fires; rows that fire must agree.
    Any,
    /// Every row that fires, in row order.
    RuleOrder,
    /// Every row that fires, in output priority order.
    OutputOrder,
    /// Every row that fires, in row order, optionally aggregated.
    Collect,
}

impl TablePolicy {
    /// The name in the format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TablePolicy::First => "first",
            TablePolicy::Priority => "priority",
            TablePolicy::Unique => "unique",
            TablePolicy::Any => "any",
            TablePolicy::RuleOrder => "rule_order",
            TablePolicy::OutputOrder => "output_order",
            TablePolicy::Collect => "collect",
        }
    }

    /// The policy with this name in the format.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            TablePolicy::First,
            TablePolicy::Priority,
            TablePolicy::Unique,
            TablePolicy::Any,
            TablePolicy::RuleOrder,
            TablePolicy::OutputOrder,
            TablePolicy::Collect,
        ]
        .into_iter()
        .find(|p| p.as_str() == name)
    }

    /// The policy and aggregate of a DMN letter (`U`, `A`, `P`, `F`, `R`, `O`,
    /// `C`, `C+`, `C<`, `C>`, `C#`).
    #[must_use]
    pub fn from_dmn(letter: &str) -> Option<(Self, Option<Aggregate>)> {
        Some(match letter.to_ascii_uppercase().as_str() {
            "U" => (TablePolicy::Unique, None),
            "A" => (TablePolicy::Any, None),
            "P" => (TablePolicy::Priority, None),
            "F" => (TablePolicy::First, None),
            "R" => (TablePolicy::RuleOrder, None),
            "O" => (TablePolicy::OutputOrder, None),
            "C" => (TablePolicy::Collect, None),
            "C+" => (TablePolicy::Collect, Some(Aggregate::Sum)),
            "C<" => (TablePolicy::Collect, Some(Aggregate::Min)),
            "C>" => (TablePolicy::Collect, Some(Aggregate::Max)),
            "C#" => (TablePolicy::Collect, Some(Aggregate::Count)),
            _ => return None,
        })
    }
}

/// How a table decides: its policy, aggregate, logic and strictness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableConfig {
    /// The hit policy.
    pub hit_policy: TablePolicy,
    /// The aggregate, only with `collect`.
    pub aggregate: Option<Aggregate>,
    /// The logic.
    pub logic: Option<LogicName>,
    /// Raise instead of returning unknown.
    pub strict: bool,
}

/// An output column: its payload key and its output values, highest priority
/// first.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TableOutput {
    /// The payload key.
    pub name: String,
    /// The values its cells may hold, in priority order; empty for any value.
    pub values: Vec<Value>,
}

impl TableOutput {
    /// An output column whose cells may hold any value.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: Vec::new(),
        }
    }

    /// The output values, highest priority first.
    #[must_use]
    pub fn values(mut self, values: Vec<Value>) -> Self {
        self.values = values;
        self
    }
}

/// A row: one rule.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct TableRow {
    /// Its rule's name; `row_<n>` when it has none.
    pub name: Option<String>,
    /// Its rule's priority.
    pub priority: Option<i64>,
    /// Its rule's description; never empty.
    pub description: Option<String>,
    /// One condition per input, testing that input; `None` for any value.
    pub inputs: Vec<Option<Node>>,
    /// One value per output; `None` for no value.
    pub outputs: Vec<Option<Node>>,
}

impl TableRow {
    /// A row with its cells: one condition per input (`None` for any value)
    /// and one value per output (`None` for no value).
    #[must_use]
    pub fn new(inputs: Vec<Option<Node>>, outputs: Vec<Option<Node>>) -> Self {
        Self {
            name: None,
            priority: None,
            description: None,
            inputs,
            outputs,
        }
    }

    /// Its rule's name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Its rule's priority.
    #[must_use]
    pub fn priority(mut self, priority: i64) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Its rule's description; an empty one is none.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        let description = description.into();
        self.description = (!description.is_empty()).then_some(description);
        self
    }
}

/// A decision table and the rule set it compiles into (`spec/tables.md`).
#[derive(Clone, Debug)]
pub struct DecisionTable {
    config: TableConfig,
    inputs: Vec<Node>,
    outputs: Vec<TableOutput>,
    rows: Vec<TableRow>,
    ruleset: RuleSet,
}

/// An error at `pointer`, relative to the table (`/rows/0/inputs/1`).
fn error(code: ErrorCode, pointer: String, message: impl Into<String>) -> Error {
    Error::new(code, message).at_pointer(pointer)
}

/// The nodes in `node`, itself included.
pub(crate) fn size(node: &Node) -> usize {
    1 + node.children().into_iter().map(size).sum::<usize>()
}

/// What reading a table may build, in cells and nodes (`spec/tables.md`,
/// *Limits*): every cell costs one, and every cell that is not empty also the
/// nodes of its condition (with its copy of the column) or value.
#[derive(Debug)]
pub(crate) struct Budget {
    left: usize,
    limit: usize,
}

impl Budget {
    pub(crate) fn new(limits: &Limits) -> Self {
        Self {
            left: limits.max_table_size,
            limit: limits.max_table_size,
        }
    }

    /// Spend `cost`, or `limit_exceeded`.
    pub(crate) fn spend(&mut self, cost: usize) -> Result<()> {
        match self.left.checked_sub(cost) {
            Some(left) => {
                self.left = left;
                Ok(())
            }
            None => Err(Error::new(
                ErrorCode::LimitExceeded,
                format!(
                    "the table holds more than {} cells and condition nodes",
                    self.limit
                ),
            )
            .with_hint("split the table, use shorter input expressions, or raise the limit if the table is trusted")),
        }
    }
}

/// An order of scalar values consistent with [`Value::any_equal`]: by group,
/// then by [`Value::compare`] within it.
fn value_order(a: &Value, b: &Value) -> Ordering {
    a.group()
        .cmp(&b.group())
        .then_with(|| a.compare(b).unwrap_or(Ordering::Equal))
}

/// An output's values, sorted so that a cell's position is found by binary
/// search.
struct ValueIndex<'a> {
    sorted: Vec<(&'a Value, usize)>,
}

impl<'a> ValueIndex<'a> {
    fn new(values: &'a [Value]) -> Self {
        let mut sorted: Vec<(&Value, usize)> = values.iter().zip(0..).collect();
        sorted.sort_by(|a, b| value_order(a.0, b.0).then(a.1.cmp(&b.1)));
        Self { sorted }
    }

    /// The position of the first value listed twice, if any.
    fn repeated(&self) -> Option<usize> {
        self.sorted
            .windows(2)
            .filter(|pair| value_order(pair[0].0, pair[1].0) == Ordering::Equal)
            .map(|pair| pair[1].1)
            .min()
    }

    /// The position of `value` in the output's values: the first equal value,
    /// as equal values are sorted by position.
    fn position(&self, value: &Value) -> Option<usize> {
        let at = self
            .sorted
            .partition_point(|(v, _)| value_order(v, value) == Ordering::Less);
        self.sorted
            .get(at)
            .filter(|(v, _)| v.any_equal(value))
            .map(|(_, i)| *i)
    }
}

/// A row as messages name it: its name, or `row_<n>`.
fn label(r: usize, row: &TableRow) -> String {
    row.name.clone().unwrap_or_else(|| format!("row_{}", r + 1))
}

impl DecisionTable {
    /// A table, checked and compiled (`spec/tables.md`, *Model* and
    /// *Compiling*).
    ///
    /// ```
    /// use cleanrules::ast::{CompareOp, Node};
    /// use cleanrules::table::{DecisionTable, TableConfig, TableOutput, TableRow};
    /// use cleanrules::{Segment, Value};
    ///
    /// let age = Node::field(vec![Segment::Key("age".into())])?;
    /// let minor = Node::compare(CompareOp::Lt, age.clone(), Node::constant(Value::Int(18))?)?;
    /// let band = |text: &str| Node::constant(Value::String(text.into()));
    /// let table = DecisionTable::new(
    ///     TableConfig::default(),
    ///     vec![age],
    ///     vec![TableOutput::new("band")],
    ///     vec![
    ///         TableRow::new(vec![Some(minor)], vec![Some(band("minor")?)]).name("minor"),
    ///         TableRow::new(vec![None], vec![Some(band("adult")?)]).name("adult"),
    ///     ],
    /// )?;
    /// assert_eq!(table.to_csv(), "HIT POLICY,first\nRULE,age,THEN band\nminor,< 18,'minor'\nadult,-,'adult'\n");
    /// # Ok::<(), cleanrules::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// With a JSON Pointer relative to the table: `format_error` for no inputs
    /// or a row with the wrong number of cells; `expression_error` for an input
    /// that is a condition; `unsupported` for a cell that is not a predicate on
    /// its column, or a field key with a control character, which have no
    /// table form (`spec/tables.md`, *Canonical text*); `rule_definition_error`
    /// for repeated output keys or values, an output value that is not a
    /// scalar, an output cell that is a condition or not one of its column's
    /// values, an empty row name, a row priority next to output values under
    /// `priority` or `output_order`, repeated rule names, or an aggregate
    /// without `collect`; and the errors of building rules.
    pub fn new(
        config: TableConfig,
        inputs: Vec<Node>,
        outputs: Vec<TableOutput>,
        mut rows: Vec<TableRow>,
    ) -> Result<Self> {
        check_columns(&inputs, &outputs)?;
        let indexes: Vec<ValueIndex<'_>> =
            outputs.iter().map(|o| ValueIndex::new(&o.values)).collect();
        for (r, row) in rows.iter_mut().enumerate() {
            row.description = row.description.take().filter(|d| !d.is_empty());
            check_row(r, row, &inputs, &outputs, &indexes)?;
        }
        if config.aggregate.is_some() && config.hit_policy != TablePolicy::Collect {
            return Err(error(
                ErrorCode::RuleDefinitionError,
                "/aggregate".into(),
                "an aggregate needs the collect hit policy",
            ));
        }
        let ruleset = compile(config, &outputs, &rows, &indexes)?;
        drop(indexes);
        Ok(Self {
            config,
            inputs,
            outputs,
            rows,
            ruleset,
        })
    }

    /// How the table decides.
    #[must_use]
    pub fn config(&self) -> &TableConfig {
        &self.config
    }

    /// The input expressions, in column order.
    #[must_use]
    pub fn inputs(&self) -> &[Node] {
        &self.inputs
    }

    /// The output columns, in order.
    #[must_use]
    pub fn outputs(&self) -> &[TableOutput] {
        &self.outputs
    }

    /// The rows, in order.
    #[must_use]
    pub fn rows(&self) -> &[TableRow] {
        &self.rows
    }

    /// The rule set the table compiles into, which evaluates it.
    #[must_use]
    pub fn ruleset(&self) -> &RuleSet {
        &self.ruleset
    }

    /// The compiled rule set.
    #[must_use]
    pub fn into_ruleset(self) -> RuleSet {
        self.ruleset
    }
}

/// `node` has a canonical text: no field key with a control character.
fn printable(node: &Node, pointer: impl Fn() -> String) -> Result<()> {
    check_keys(node).map_err(|e| relocate(e, pointer()))
}

/// The checks of the inputs and outputs, which readers make before reading
/// the rows (`spec/tables.md`, *JSON*).
pub(crate) fn check_columns(inputs: &[Node], outputs: &[TableOutput]) -> Result<()> {
    if inputs.is_empty() {
        return Err(error(
            ErrorCode::FormatError,
            "/inputs".into(),
            "a decision table has at least one input",
        ));
    }
    for (i, input) in inputs.iter().enumerate() {
        printable(input, || format!("/inputs/{i}"))?;
        if input.kind() == Kind::Condition {
            return Err(error(
                ErrorCode::ExpressionError,
                format!("/inputs/{i}"),
                format!("the input {input} is a condition, not a value"),
            )
            .with_hint("make the column the value and put the test in the cells: x, then > 5"));
        }
    }
    let mut names = BTreeSet::new();
    for (o, output) in outputs.iter().enumerate() {
        if !names.insert(output.name.as_str()) {
            return Err(error(
                ErrorCode::RuleDefinitionError,
                format!("/outputs/{o}/name"),
                format!("two outputs are named {:?}", output.name),
            ));
        }
        for (v, value) in output.values.iter().enumerate() {
            if value.is_null() || matches!(value, Value::List(_) | Value::Map(_) | Value::Opaque(_))
            {
                return Err(error(
                    ErrorCode::RuleDefinitionError,
                    format!("/outputs/{o}/values/{v}"),
                    "output values are scalars other than null",
                ));
            }
        }
        if let Some(v) = ValueIndex::new(&output.values).repeated() {
            return Err(error(
                ErrorCode::RuleDefinitionError,
                format!("/outputs/{o}/values/{v}"),
                format!("the output {:?} lists a value twice", output.name),
            ));
        }
    }
    Ok(())
}

fn check_row(
    r: usize,
    row: &TableRow,
    inputs: &[Node],
    outputs: &[TableOutput],
    indexes: &[ValueIndex<'_>],
) -> Result<()> {
    for (what, count, expected) in [
        ("inputs", row.inputs.len(), inputs.len()),
        ("outputs", row.outputs.len(), outputs.len()),
    ] {
        if count != expected {
            return Err(error(
                ErrorCode::FormatError,
                format!("/rows/{r}/{what}"),
                format!(
                    "the row {} has {count} {what}; the table has {expected}",
                    label(r, row)
                ),
            ));
        }
    }
    if row.name.as_deref() == Some("") {
        return Err(error(
            ErrorCode::RuleDefinitionError,
            format!("/rows/{r}/name"),
            format!("row {} has an empty name", r + 1),
        )
        .with_hint("leave the name out to call the row row_<n>"));
    }
    for (c, (cell, input)) in row.inputs.iter().zip(inputs).enumerate() {
        let Some(cell) = cell else { continue };
        let pointer = || format!("/rows/{r}/inputs/{c}");
        printable(cell, pointer)?;
        if cell_text(cell, input).is_none() {
            return Err(error(
                ErrorCode::Unsupported,
                pointer(),
                format!(
                    "the cell {cell} of row {} does not test its column {input}",
                    label(r, row)
                ),
            )
            .with_hint("a cell is a predicate on its column, such as > 5 or IN ('a', 'b')")
            .unsupported("decision table", "the cell has no table form"));
        }
    }
    for (c, ((cell, output), index)) in row.outputs.iter().zip(outputs).zip(indexes).enumerate() {
        if let Some(cell) = cell {
            let pointer = || format!("/rows/{r}/outputs/{c}");
            printable(cell, pointer)?;
            if cell.kind() == Kind::Condition {
                return Err(error(
                    ErrorCode::RuleDefinitionError,
                    pointer(),
                    format!(
                        "the output {:?} of row {} is the condition {cell}, not a value",
                        output.name,
                        label(r, row)
                    ),
                )
                .with_hint("outputs are values: a condition belongs in the input cells"));
            }
        }
        if output.values.is_empty() {
            continue;
        }
        if rank(cell.as_ref(), index).is_none() {
            return Err(error(
                ErrorCode::RuleDefinitionError,
                format!("/rows/{r}/outputs/{c}"),
                format!(
                    "the output {:?} of row {} takes one of its values",
                    output.name,
                    label(r, row)
                ),
            )
            .with_hint(match unquoted(cell.as_ref(), &output.values) {
                Some(name) => format!(
                    "{name} is a field; the string is written between single quotes: '{name}'"
                ),
                None => "write one of the values listed in the output's header".into(),
            }));
        }
    }
    Ok(())
}

/// The name of a field cell that is one of the string values when quoted:
/// `high` for `'high'`.
fn unquoted<'c>(cell: Option<&'c Node>, values: &[Value]) -> Option<&'c str> {
    let crate::ast::Expr::Field(path) = cell?.expr() else {
        return None;
    };
    let [crate::Segment::Key(name)] = path.as_slice() else {
        return None;
    };
    values
        .iter()
        .any(|v| matches!(v, Value::String(s) if s == name))
        .then_some(name.as_str())
}

/// The position of an output cell's constant in its column's values.
fn rank(cell: Option<&Node>, index: &ValueIndex<'_>) -> Option<usize> {
    match cell?.expr() {
        crate::ast::Expr::Const(value) => index.position(value),
        _ => None,
    }
}

/// The position of each output cell in its column's values, for the outputs
/// that have values: the row's output priority, smaller first.
fn output_rank(row: &TableRow, outputs: &[TableOutput], indexes: &[ValueIndex<'_>]) -> Vec<usize> {
    row.outputs
        .iter()
        .zip(outputs)
        .zip(indexes)
        .filter(|((_, output), _)| !output.values.is_empty())
        .map(|((cell, _), index)| rank(cell.as_ref(), index).unwrap_or(usize::MAX))
        .collect()
}

/// The rule set of a table (`spec/tables.md`, *Compiling*).
fn compile(
    config: TableConfig,
    outputs: &[TableOutput],
    rows: &[TableRow],
    indexes: &[ValueIndex<'_>],
) -> Result<RuleSet> {
    let ranked = matches!(
        config.hit_policy,
        TablePolicy::Priority | TablePolicy::OutputOrder
    ) && outputs.iter().any(|o| !o.values.is_empty());
    let ranks: Vec<Vec<usize>> = rows
        .iter()
        .map(|row| output_rank(row, outputs, indexes))
        .collect();
    let mut distinct = ranks.clone();
    distinct.sort();
    distinct.dedup();
    let mut rules = Vec::with_capacity(rows.len());
    let mut names = BTreeSet::new();
    for (r, row) in rows.iter().enumerate() {
        let at = |pointer: String| move |e: Error| relocate(e, pointer.clone());
        if ranked && row.priority.is_some() {
            return Err(error(
                ErrorCode::RuleDefinitionError,
                format!("/rows/{r}/priority"),
                format!(
                    "row {} has a priority of its own next to output values",
                    label(r, row)
                ),
            )
            .with_hint("rank rows by the output values or by their priorities, not both"));
        }
        let name = label(r, row);
        if !names.insert(name.clone()) {
            let pointer = if row.name.is_some() {
                format!("/rows/{r}/name")
            } else {
                format!("/rows/{r}")
            };
            return Err(error(
                ErrorCode::RuleDefinitionError,
                pointer,
                format!("two rows are named {name:?}"),
            )
            .with_hint("rule names must be unique"));
        }
        let mut tests: Vec<Node> = row.inputs.iter().flatten().cloned().collect();
        let when = match tests.len() {
            0 => Node::constant(Value::Bool(true)).map_err(at(format!("/rows/{r}")))?,
            1 => tests.remove(0),
            _ => Node::and(tests).map_err(at(format!("/rows/{r}")))?,
        };
        let then = row
            .outputs
            .iter()
            .zip(outputs)
            .filter_map(|(cell, output)| cell.clone().map(|node| (output.name.clone(), node)))
            .collect();
        let mut rule = Rule::with_payload(name, when, then).map_err(at(format!("/rows/{r}")))?;
        let priority = if ranked {
            let rank = distinct.binary_search(&ranks[r]).unwrap_or(0);
            i64::try_from(distinct.len() - rank).unwrap_or(i64::MAX)
        } else {
            row.priority.unwrap_or(0)
        };
        rule = rule.priority(priority);
        if let Some(description) = &row.description {
            rule = rule.description(description.clone());
        }
        rules.push(rule);
    }
    let hit_policy = match config.hit_policy {
        TablePolicy::First => HitPolicy::First,
        TablePolicy::Priority => HitPolicy::Priority,
        TablePolicy::Unique => HitPolicy::Unique,
        TablePolicy::Any => HitPolicy::Any,
        TablePolicy::RuleOrder => HitPolicy::RuleOrder,
        TablePolicy::Collect => HitPolicy::Collect,
        TablePolicy::OutputOrder => {
            // A stable sort: equal output priorities keep row order.
            rules.sort_by_key(|rule| std::cmp::Reverse(rule.get_priority()));
            HitPolicy::RuleOrder
        }
    };
    RuleSet::new(
        rules,
        Config {
            hit_policy,
            aggregate: config.aggregate,
            logic: config.logic,
            strict: config.strict,
        },
    )
}

/// `error` located at `pointer`, unless it already points somewhere.
fn relocate(mut error: Error, pointer: String) -> Error {
    if error.context().pointer.is_none() {
        error.context_mut().pointer = Some(pointer);
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::CompareOp;
    use crate::error::Segment;

    fn field(name: &str) -> Node {
        Node::field(vec![Segment::Key(name.into())]).unwrap()
    }

    fn gt(subject: Node, value: i64) -> Node {
        Node::compare(
            CompareOp::Gt,
            subject,
            Node::constant(Value::Int(value)).unwrap(),
        )
        .unwrap()
    }

    fn table(
        inputs: Vec<Node>,
        outputs: Vec<TableOutput>,
        rows: Vec<TableRow>,
    ) -> Result<DecisionTable> {
        DecisionTable::new(TableConfig::default(), inputs, outputs, rows)
    }

    #[test]
    fn host_built_tables_must_have_a_table_form() {
        // A cell testing another field than its column has no cell text.
        let row = TableRow::new(vec![Some(gt(field("y"), 1))], vec![]);
        let error = table(vec![field("x")], vec![], vec![row]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert_eq!(error.context().pointer.as_deref(), Some("/rows/0/inputs/0"));
        // A key with a control character has no text.
        let error = table(vec![field("a\nb")], vec![], vec![]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert_eq!(error.context().pointer.as_deref(), Some("/inputs/0"));
        // A cell that is not a predicate on its column, such as the column itself.
        let row = TableRow::new(vec![Some(field("x"))], vec![]);
        let error = table(vec![field("x")], vec![], vec![row]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
    }

    #[test]
    fn output_values_are_scalars() {
        let output = TableOutput::new("y").values(vec![Value::List(vec![])]);
        let error = table(vec![field("x")], vec![output], vec![]).unwrap_err();
        assert_eq!(
            error.context().pointer.as_deref(),
            Some("/outputs/0/values/0")
        );
    }

    #[test]
    fn a_table_gives_its_rule_set() {
        let row = TableRow::new(vec![Some(gt(field("x"), 1))], vec![]);
        let ruleset = table(vec![field("x")], vec![], vec![row])
            .unwrap()
            .into_ruleset();
        assert_eq!(ruleset.rules()[0].name(), "row_1");
        assert_eq!(TablePolicy::from_dmn("Z"), None);
    }

    #[test]
    fn rows_are_built_and_checked() {
        let row = TableRow::new(vec![None], vec![Some(field("y"))])
            .name("a")
            .priority(3)
            .description("");
        assert_eq!(
            (
                row.name.as_deref(),
                row.priority,
                row.description.as_deref()
            ),
            (Some("a"), Some(3), None)
        );
        // A row with the wrong number of cells names itself.
        let error = table(vec![field("x")], vec![], vec![row.clone()]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::FormatError);
        assert!(error.message().contains("row a"), "{}", error.message());
        // Under output values, a cell must be one of them: an expression is not.
        let ranked = TableOutput::new("z").values(vec![Value::Int(1)]);
        let error = table(
            vec![field("x")],
            vec![ranked],
            vec![TableRow::new(vec![None], vec![Some(field("y"))])],
        )
        .unwrap_err();
        assert_eq!(
            error.context().pointer.as_deref(),
            Some("/rows/0/outputs/0")
        );
    }
}
