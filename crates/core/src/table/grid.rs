//! Decision tables built from grids (`spec/tables.md`, *Grids*): a header and
//! rows of cells, as spreadsheets and data frames hold them.

use super::csv::{Column, Heading, column, heading};
use super::{Budget, DecisionTable, TableConfig, TableOutput, TableRow, check_columns, size};
use crate::ast::{CompareOp, Node};
use crate::error::{Error, ErrorCode, Result};
use crate::format::table::in_text;
use crate::json::Limits;
use crate::text::{parse_cell, parse_expression};
use crate::value::Value;

/// A column of a grid's header.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum HeaderItem {
    /// Text, read as a CSV header field: `RULE`, `PRIORITY`, `DESCRIPTION`,
    /// `THEN <key> [IN (…)]` or an input expression.
    Text(String),
    /// An input, built in a host language.
    Input(Node),
    /// An output, built in a host language.
    Output(TableOutput),
}

impl From<&str> for HeaderItem {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for HeaderItem {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Node> for HeaderItem {
    fn from(input: Node) -> Self {
        Self::Input(input)
    }
}

impl From<TableOutput> for HeaderItem {
    fn from(output: TableOutput) -> Self {
        Self::Output(output)
    }
}

/// A cell of a grid (`spec/tables.md`, *Grids*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Cell {
    /// Text, read as an unquoted CSV field: `> 5`, `-`, `'VIP'`.
    Text(String),
    /// A value. Null is an empty cell. Otherwise, under an input `x`, the
    /// condition `x = value`; under an output, the value; under `RULE` or
    /// `DESCRIPTION`, a string, read as text; under `PRIORITY`, an int or a
    /// float with an integral value.
    Value(Value),
    /// A node built in a host language: an input cell's condition, which tests
    /// its column, or an output cell's value.
    Node(Node),
}

impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Value> for Cell {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

impl From<Node> for Cell {
    fn from(node: Node) -> Self {
        Self::Node(node)
    }
}

impl Cell {
    fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.trim().is_empty(),
            Self::Value(value) => value.is_null(),
            Self::Node(_) => false,
        }
    }
}

fn format_error(pointer: String, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

/// The grid's columns, to read rows and to locate the table's errors.
#[derive(Debug)]
struct Columns(Vec<Column>);

impl Columns {
    fn position(&self, wanted: impl Fn(&Column) -> bool) -> Option<usize> {
        self.0.iter().position(wanted)
    }

    /// A table error's pointer (`/rows/0/inputs/1`) as a place in the grid
    /// (`/rows/0/3`).
    fn locate(&self, mut error: Error) -> Error {
        let Some(pointer) = error.context().pointer.clone() else {
            return error;
        };
        let parts: Vec<&str> = pointer.split('/').skip(1).collect();
        let number = |i: usize| parts.get(i).and_then(|p| p.parse::<usize>().ok());
        let input = |i| move |c: &Column| matches!(c, Column::Input(n) if Some(*n) == i);
        let output = |o| move |c: &Column| matches!(c, Column::Output(n) if Some(*n) == o);
        let located = match parts.as_slice() {
            ["inputs"] => Some("/header".to_owned()),
            ["inputs", _, ..] => self
                .position(input(number(1)))
                .map(|c| format!("/header/{c}")),
            ["outputs", _, ..] => self
                .position(output(number(1)))
                .map(|c| format!("/header/{c}")),
            ["rows", r, rest @ ..] => {
                let column = match rest {
                    ["inputs", _] => self.position(input(number(3))),
                    ["outputs", _] => self.position(output(number(3))),
                    ["name"] => self.position(|c| matches!(c, Column::Name)),
                    ["priority"] => self.position(|c| matches!(c, Column::Priority)),
                    _ => None,
                };
                Some(match column {
                    Some(c) => format!("/rows/{r}/{c}"),
                    None => format!("/rows/{r}"),
                })
            }
            _ => None,
        };
        if let Some(located) = located {
            error.context_mut().pointer = Some(located);
        }
        error
    }
}

/// A name or description: text, or a string, without the space around it.
fn label(cell: Cell, what: &str, pointer: impl Fn() -> String) -> Result<Option<String>> {
    let text = match cell {
        Cell::Text(text) | Cell::Value(Value::String(text)) => text,
        Cell::Value(value) if value.is_null() => return Ok(None),
        other => {
            return Err(format_error(
                pointer(),
                format!("{what} is text, not {other}"),
            ));
        }
    };
    let text = text.trim();
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

fn priority(cell: Cell, pointer: impl Fn() -> String) -> Result<Option<i64>> {
    let wrong = |shown: String| {
        format_error(
            pointer(),
            format!("a priority is a 64-bit integer, not {shown}"),
        )
    };
    match cell {
        Cell::Text(text) | Cell::Value(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() {
                return Ok(None);
            }
            text.parse()
                .map(Some)
                .map_err(|_| wrong(format!("{text:?}")))
        }
        Cell::Value(Value::Int(priority)) => Ok(Some(priority)),
        // Data frames store ints with missing values as floats. The bounds are
        // those of i64: -2^63 is exact, 2^63 is the first float above.
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        Cell::Value(Value::Float(float))
            if float.fract() == 0.0 && float >= i64::MIN as f64 && float < i64::MAX as f64 =>
        {
            Ok(Some(float as i64))
        }
        Cell::Value(value) if value.is_null() => Ok(None),
        other => Err(wrong(other.to_string())),
    }
}

impl std::fmt::Display for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(text) => write!(f, "{text:?}"),
            Self::Value(value) => match Node::constant(value.clone()) {
                Ok(node) => write!(f, "{node}"),
                Err(_) => f.write_str("a list, map or opaque value"),
            },
            Self::Node(node) => write!(f, "the expression {node}"),
        }
    }
}

/// Read a row's cells, in column order, into a table row.
fn row(
    r: usize,
    cells: Vec<Cell>,
    columns: &Columns,
    inputs: &[Node],
    outputs: usize,
    budget: &mut Budget,
    limits: &Limits,
) -> Result<TableRow> {
    let width = columns.0.len();
    if let Some(extra) = cells.iter().skip(width).position(|cell| !cell.is_empty()) {
        return Err(format_error(
            format!("/rows/{r}/{}", width + extra),
            "the row has more cells than the header",
        ));
    }
    budget
        .spend(width)
        .map_err(|e| e.at_pointer(format!("/rows/{r}")))?;
    let mut row = TableRow::new(vec![None; inputs.len()], vec![None; outputs]);
    for ((c, column), cell) in columns.0.iter().enumerate().zip(cells) {
        let here = || format!("/rows/{r}/{c}");
        match column {
            Column::Name => row.name = label(cell, "a rule name", here)?,
            Column::Description => row.description = label(cell, "a description", here)?,
            Column::Priority => row.priority = priority(cell, here)?,
            Column::Input(i) => {
                let subject = &inputs[*i];
                let condition = match cell {
                    Cell::Text(text) => {
                        parse_cell(&text, subject, limits).map_err(|e| in_text(&e, &here()))?
                    }
                    Cell::Value(value) if value.is_null() => None,
                    Cell::Value(value) => Some(
                        Node::constant(value)
                            .and_then(|v| Node::compare(CompareOp::Eq, subject.clone(), v))
                            .map_err(|e| e.at_pointer(here()))?,
                    ),
                    Cell::Node(node) => Some(node),
                };
                if let Some(condition) = &condition {
                    // The cell's condition holds a copy of its column.
                    budget
                        .spend(size(condition))
                        .map_err(|e| e.at_pointer(here()))?;
                }
                row.inputs[*i] = condition;
            }
            Column::Output(o) => {
                let value = match cell {
                    Cell::Text(text) if text.trim().is_empty() => None,
                    Cell::Text(text) => {
                        Some(parse_expression(&text, limits).map_err(|e| in_text(&e, &here()))?)
                    }
                    Cell::Value(value) if value.is_null() => None,
                    Cell::Value(value) => {
                        Some(Node::constant(value).map_err(|e| e.at_pointer(here()))?)
                    }
                    Cell::Node(node) => Some(node),
                };
                if let Some(value) = &value {
                    budget
                        .spend(size(value))
                        .map_err(|e| e.at_pointer(here()))?;
                }
                row.outputs[*o] = value;
            }
        }
    }
    Ok(row)
}

/// A table built from a grid one row at a time (`spec/tables.md`, *Grids*):
/// [`DecisionTable::grid`] reads the header, [`GridBuilder::push_row`] each
/// row, and [`GridBuilder::finish`] checks and compiles the table.
///
/// An error from `push_row` leaves the table unfinished: the caller reports it
/// and drops the builder.
#[derive(Debug)]
pub struct GridBuilder {
    config: TableConfig,
    columns: Columns,
    inputs: Vec<Node>,
    outputs: Vec<TableOutput>,
    rows: Vec<TableRow>,
    pushed: usize,
    budget: Budget,
    limits: Limits,
}

impl GridBuilder {
    /// Read the next row's cells, in column order.
    ///
    /// # Errors
    ///
    /// With a JSON Pointer into the grid (`/rows/<r>/<c>`, or `/rows/<r>`):
    /// `format_error` for more cells than the header, or a name, priority or
    /// description that is not one; the errors of the text syntax for a cell,
    /// with its column within the text in the message; the errors of building
    /// a cell's node; and `limit_exceeded` beyond `limits.max_table_size`.
    pub fn push_row(&mut self, cells: Vec<Cell>) -> Result<()> {
        let r = self.pushed;
        self.pushed += 1;
        let row = row(
            r,
            cells,
            &self.columns,
            &self.inputs,
            self.outputs.len(),
            &mut self.budget,
            &self.limits,
        )?;
        self.rows.push(row);
        Ok(())
    }

    /// The table, checked and compiled.
    ///
    /// # Errors
    ///
    /// The errors of [`DecisionTable::new`], with pointers into the grid.
    pub fn finish(self) -> Result<DecisionTable> {
        let Self {
            config,
            columns,
            inputs,
            outputs,
            rows,
            ..
        } = self;
        DecisionTable::new(config, inputs, outputs, rows).map_err(|e| columns.locate(e))
    }
}

impl DecisionTable {
    /// Start building a table from a grid: read its header, then push its
    /// rows (`spec/tables.md`, *Grids*).
    ///
    /// ```
    /// use cleanrules::json::Limits;
    /// use cleanrules::table::{DecisionTable, TableConfig};
    /// use cleanrules::{NullCause, Value};
    ///
    /// let mut grid = DecisionTable::grid(TableConfig::default(), ["RULE", "age", "THEN band"].map(Into::into), &Limits::default())?;
    /// grid.push_row(vec!["minor".into(), "< 18".into(), "'minor'".into()])?;
    /// grid.push_row(vec!["adult".into(), Value::Null(NullCause::Null).into(), "'adult'".into()])?;
    /// let table = grid.finish()?;
    /// assert_eq!(table.to_csv(), "HIT POLICY,first\nRULE,age,THEN band\nminor,< 18,'minor'\nadult,-,'adult'\n");
    /// # Ok::<(), cleanrules::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// With a JSON Pointer into the grid (`/header`, `/header/<c>`):
    /// `format_error` for an empty or repeated special column; the errors of
    /// the text syntax for a header, with its column within the text in the
    /// message; and the checks of the inputs and outputs of
    /// [`DecisionTable::new`], which are made before any row is read.
    pub fn grid(
        config: TableConfig,
        header: impl IntoIterator<Item = HeaderItem>,
        limits: &Limits,
    ) -> Result<GridBuilder> {
        let mut columns = Vec::new();
        let (mut inputs, mut outputs) = (Vec::new(), Vec::new());
        for (c, item) in header.into_iter().enumerate() {
            let here = || format!("/header/{c}");
            let read = match item {
                HeaderItem::Text(text) => {
                    heading(&text, limits).map_err(|e| in_text(&e, &here()))?
                }
                HeaderItem::Input(node) => Heading::Input(node),
                HeaderItem::Output(output) => Heading::Output(output),
            };
            let fail = |message: String| format_error(here(), message);
            let column = column(read, &columns, &mut inputs, &mut outputs, fail)?;
            columns.push(column);
        }
        let columns = Columns(columns);
        check_columns(&inputs, &outputs).map_err(|e| columns.locate(e))?;
        Ok(GridBuilder {
            config,
            columns,
            inputs,
            outputs,
            rows: Vec::new(),
            pushed: 0,
            budget: Budget::new(limits),
            limits: *limits,
        })
    }

    /// Build a table from a whole grid: [`DecisionTable::grid`], a
    /// [`GridBuilder::push_row`] for each row, and [`GridBuilder::finish`].
    ///
    /// # Errors
    ///
    /// The first error of those steps.
    pub fn from_grid(
        config: TableConfig,
        header: impl IntoIterator<Item = HeaderItem>,
        rows: impl IntoIterator<Item = Vec<Cell>>,
        limits: &Limits,
    ) -> Result<Self> {
        let mut grid = Self::grid(config, header, limits)?;
        for row in rows {
            grid.push_row(row)?;
        }
        grid.finish()
    }
}

#[cfg(test)]
mod tests {
    // A test: a panic is the failure report.
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::Segment;

    fn text(text: &str) -> Cell {
        Cell::Text(text.into())
    }

    fn headers(texts: &[&str]) -> Vec<HeaderItem> {
        texts.iter().map(|t| (*t).into()).collect()
    }

    fn build(header: Vec<HeaderItem>, rows: Vec<Vec<Cell>>) -> Result<DecisionTable> {
        DecisionTable::from_grid(TableConfig::default(), header, rows, &Limits::default())
    }

    fn pointer(error: &Error) -> &str {
        error.context().pointer.as_deref().unwrap()
    }

    #[test]
    fn values_nodes_and_text_build_the_same_table() {
        let age = Node::field(vec![Segment::Key("age".into())]).unwrap();
        let minor = Node::compare(
            CompareOp::Lt,
            age.clone(),
            Node::constant(Value::Int(18)).unwrap(),
        )
        .unwrap();
        let built = build(
            vec![
                HeaderItem::Text("RULE".into()),
                HeaderItem::Input(age),
                HeaderItem::Output(TableOutput::new("band")),
                HeaderItem::Text("PRIORITY".into()),
            ],
            vec![
                vec![
                    Cell::Value(Value::String(" minor ".into())),
                    Cell::Node(minor),
                    Cell::Value(Value::String("minor".into())),
                    Cell::Value(Value::Float(2.0)),
                ],
                vec![text("ten"), Cell::Value(Value::Int(10)), text("'ten'")],
            ],
        )
        .unwrap();
        let read = DecisionTable::from_csv(
            "RULE,age,THEN band,PRIORITY\nminor,< 18,'minor',2\nten,10,'ten'\n",
            &Limits::default(),
        )
        .unwrap();
        assert_eq!(built.to_csv(), read.to_csv());
    }

    /// A header, its rows, and where the error points.
    type Case = (Vec<HeaderItem>, Vec<Vec<Cell>>, &'static str);

    #[test]
    fn errors_point_into_the_grid() {
        let cases: [Case; 8] = [
            (headers(&["RULE", "x", "rule"]), vec![], "/header/2"),
            (headers(&["x", " "]), vec![], "/header/1"),
            (headers(&["x", "THEN a", "THEN a"]), vec![], "/header/2"),
            (
                headers(&["x"]),
                vec![vec![text("1"), text(" "), text("2")]],
                "/rows/0/2",
            ),
            (
                headers(&["RULE", "x"]),
                vec![vec![Cell::Value(Value::Int(1))]],
                "/rows/0/0",
            ),
            (
                headers(&["x", "PRIORITY"]),
                vec![vec![text("1"), Cell::Value(Value::Float(1.5))]],
                "/rows/0/1",
            ),
            (
                headers(&["x", "THEN a IN (1, 2)"]),
                vec![vec![text("1"), text("3")]],
                "/rows/0/1",
            ),
            (
                headers(&["RULE", "x"]),
                vec![vec![text("a"), text("1")], vec![text("a"), text("2")]],
                "/rows/1/0",
            ),
        ];
        for (header, rows, wanted) in cases {
            let error = build(header, rows).unwrap_err();
            assert_eq!(pointer(&error), wanted, "{error}");
        }
    }

    #[test]
    fn a_text_error_gives_its_column() {
        let error = build(headers(&["x"]), vec![vec![text("> )")]]).unwrap_err();
        assert_eq!(pointer(&error), "/rows/0/0");
        assert!(
            error.message().contains("(column 3 of the text)"),
            "{error}"
        );
        let error = build(headers(&["x", "THEN a IN (1,"]), vec![]).unwrap_err();
        assert_eq!(pointer(&error), "/header/1");
        assert!(
            error.message().contains("(column 14 of the text)"),
            "{error}"
        );
    }

    #[test]
    fn rows_are_numbered_as_pushed() {
        let mut grid =
            DecisionTable::grid(TableConfig::default(), headers(&["x"]), &Limits::default())
                .unwrap();
        grid.push_row(vec![text("1")]).unwrap();
        let error = grid.push_row(vec![text("> )")]).unwrap_err();
        assert_eq!(pointer(&error), "/rows/1/0");
        let error = grid.push_row(vec![text("1"), text("2")]).unwrap_err();
        assert_eq!(pointer(&error), "/rows/2/1");
        assert_eq!(grid.finish().unwrap().rows().len(), 1);
    }

    #[test]
    fn the_size_limit_counts_cells() {
        let limits = Limits {
            max_table_size: 5,
            ..Limits::default()
        };
        let rows = (0..3).map(|_| vec![text("> 1")]);
        let error =
            DecisionTable::from_grid(TableConfig::default(), headers(&["x"]), rows, &limits)
                .unwrap_err();
        assert_eq!(error.code(), ErrorCode::LimitExceeded);
        assert_eq!(pointer(&error), "/rows/1/0");
    }

    #[test]
    fn an_unquoted_string_gets_a_hint() {
        let header = headers(&["x", "THEN risk IN ('high', 'low')"]);
        let error = build(header, vec![vec![text("1"), text("high")]]).unwrap_err();
        assert_eq!(
            error.hint(),
            Some("high is a field; the string is written between single quotes: 'high'")
        );
        let header = headers(&["x", "THEN risk IN ('high', 'low')"]);
        let error = build(header, vec![vec![text("1"), text("medium")]]).unwrap_err();
        assert_eq!(
            error.hint(),
            Some("write one of the values listed in the output's header")
        );
    }
}
