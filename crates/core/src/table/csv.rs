//! Decision tables as CSV (`spec/tables.md`, *CSV*).

use std::fmt::Write as _;

use super::{
    Budget, DecisionTable, TableConfig, TableOutput, TablePolicy, TableRow, check_columns, size,
};
use crate::ast::Node;
use crate::error::{Error, ErrorCode, Result};
use crate::json::Limits;
use crate::logic::LogicName;
use crate::rules::Aggregate;
use crate::text::{
    cell_text, is_name, parse_cell, parse_expression, parse_output, string_text, styled, table_text,
};

/// A field of a CSV record, with where it starts.
struct Field {
    text: String,
    line: u64,
    column: u64,
    quoted: bool,
}

fn format_error(line: u64, column: u64, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_position(line, column)
}

/// The separator: the first comma, semicolon or tab outside double quotes and
/// parentheses, in the first line that has one; a comma when there is none.
fn separator(text: &str) -> char {
    let (mut quoted, mut depth) = (false, 0usize);
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            // Parentheses are counted line by line.
            '\n' | '\r' if !quoted => depth = 0,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' | ';' | '\t' if !quoted && depth == 0 => return c,
            _ => {}
        }
    }
    ','
}

/// The records of a CSV text, one at a time, without those whose fields are all
/// empty or space.
struct Records<'t> {
    chars: std::iter::Peekable<std::str::Chars<'t>>,
    separator: char,
    line: u64,
    column: u64,
    done: bool,
}

impl<'t> Records<'t> {
    fn new(text: &'t str) -> Self {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        Self {
            chars: text.chars().peekable(),
            separator: separator(text),
            line: 1,
            column: 1,
            done: false,
        }
    }

    /// The next field, after a separator or at the start of a line.
    fn field(&mut self) -> Result<Field> {
        let mut field = Field {
            text: String::new(),
            line: self.line,
            column: self.column,
            quoted: false,
        };
        if self.chars.peek() != Some(&'"') {
            while let Some(&c) = self.chars.peek() {
                if c == self.separator || c == '\n' || c == '\r' {
                    break;
                }
                field.text.push(c);
                self.chars.next();
                self.column += 1;
            }
            return Ok(field);
        }
        field.quoted = true;
        self.chars.next();
        self.column += 1;
        loop {
            match self.chars.next() {
                None => {
                    return Err(format_error(
                        field.line,
                        field.column,
                        "a quoted field is not closed",
                    )
                    .with_hint("write a double quote in a quoted field as two: \"\""));
                }
                Some('"') if self.chars.peek() == Some(&'"') => {
                    self.chars.next();
                    self.column += 2;
                    field.text.push('"');
                }
                Some('"') => {
                    self.column += 1;
                    break;
                }
                Some(c) => {
                    if c == '\n' || (c == '\r' && self.chars.peek() != Some(&'\n')) {
                        (self.line, self.column) = (self.line + 1, 1);
                    } else {
                        self.column += 1;
                    }
                    field.text.push(c);
                }
            }
        }
        match self.chars.peek() {
            None | Some('\n' | '\r') => Ok(field),
            Some(&c) if c == self.separator => Ok(field),
            Some(_) => Err(format_error(
                self.line,
                self.column,
                "a quoted field goes on after its closing quote",
            )
            .with_hint("quote the whole field, writing its double quotes twice")),
        }
    }

    /// The next record, empty or not.
    fn record(&mut self) -> Result<Vec<Field>> {
        let mut record = Vec::new();
        loop {
            record.push(self.field()?);
            match self.chars.next() {
                Some(c) if c == self.separator => self.column += 1,
                end => {
                    if end == Some('\r') && self.chars.peek() == Some(&'\n') {
                        self.chars.next();
                    }
                    if end.is_none() {
                        self.done = true;
                    } else {
                        (self.line, self.column) = (self.line + 1, 1);
                    }
                    return Ok(record);
                }
            }
        }
    }
}

impl Iterator for Records<'_> {
    type Item = Result<Vec<Field>>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.done {
            match self.record() {
                Err(error) => {
                    self.done = true;
                    return Some(Err(error));
                }
                Ok(record) if record.iter().any(|f| !f.text.trim().is_empty()) => {
                    return Some(Ok(record));
                }
                Ok(_) => {}
            }
        }
        None
    }
}

/// Where the error in the text of `field` is: its own line and column when
/// the field is written as it is, else the field.
fn locate(mut error: Error, field: &Field) -> Error {
    let context = error.context_mut();
    let (line, column) = match (context.line, context.column) {
        (Some(1), Some(column)) if !field.quoted => (field.line, field.column + column - 1),
        _ => (field.line, field.column),
    };
    context.line = Some(line);
    context.column = Some(column);
    error
}

/// A setting's keyword, upper case with single spaces.
fn keyword(field: &Field) -> String {
    field
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

/// What a column holds; inputs and outputs by position.
#[derive(Debug)]
pub(super) enum Column {
    Name,
    Priority,
    Description,
    Input(usize),
    Output(usize),
}

impl Column {
    fn keyword(&self) -> Option<&'static str> {
        match self {
            Self::Name => Some("RULE"),
            Self::Priority => Some("PRIORITY"),
            Self::Description => Some("DESCRIPTION"),
            _ => None,
        }
    }
}

/// Where each part of the table was read, to locate errors.
struct Places<'f> {
    settings: Vec<(&'static str, &'f Field)>,
    header: &'f [Field],
    columns: Vec<Column>,
    rows: &'f [Vec<Field>],
}

impl Places<'_> {
    fn header_of(&self, wanted: impl Fn(&Column) -> bool) -> Option<&Field> {
        self.columns
            .iter()
            .position(wanted)
            .and_then(|i| self.header.get(i))
    }

    fn cell(&self, row: usize, wanted: impl Fn(&Column) -> bool) -> Option<&Field> {
        let record = self.rows.get(row)?;
        let at = self.columns.iter().position(wanted);
        at.and_then(|i| record.get(i)).or_else(|| record.first())
    }

    /// The field that a table error's pointer (`/rows/0/inputs/1`) names.
    fn field(&self, pointer: &str) -> Option<&Field> {
        let parts: Vec<&str> = pointer.split('/').skip(1).collect();
        let number = |i: usize| parts.get(i).and_then(|p| p.parse::<usize>().ok());
        match parts.as_slice() {
            ["inputs", ..] => {
                self.header_of(|c| matches!(c, Column::Input(i) if Some(*i) == number(1)))
            }
            ["outputs", ..] => {
                self.header_of(|c| matches!(c, Column::Output(i) if Some(*i) == number(1)))
            }
            ["rows", _, "inputs", _] => self.cell(
                number(1)?,
                |c| matches!(c, Column::Input(i) if Some(*i) == number(3)),
            ),
            ["rows", _, "outputs", _] => self.cell(
                number(1)?,
                |c| matches!(c, Column::Output(i) if Some(*i) == number(3)),
            ),
            ["rows", _, "name"] => self.cell(number(1)?, |c| matches!(c, Column::Name)),
            ["rows", _, "priority"] => self.cell(number(1)?, |c| matches!(c, Column::Priority)),
            ["rows", _, ..] => self.cell(number(1)?, |_| false),
            ["aggregate"] => self
                .settings
                .iter()
                .find(|(k, _)| *k == "AGGREGATE")
                .or_else(|| self.settings.iter().find(|(k, _)| *k == "HIT POLICY"))
                .map(|(_, f)| *f),
            _ => None,
        }
    }

    /// A table error, located at its field instead of its pointer.
    fn locate(&self, mut error: Error) -> Error {
        let field = error
            .context()
            .pointer
            .as_deref()
            .and_then(|pointer| self.field(pointer))
            .map(|f| (f.line, f.column));
        if let Some((line, column)) = field {
            let context = error.context_mut();
            context.pointer = None;
            context.line = Some(line);
            context.column = Some(column);
        }
        error
    }
}

/// Whether a record's first field is a setting's keyword.
fn is_setting(first: &Field) -> bool {
    matches!(
        keyword(first).as_str(),
        "HIT POLICY" | "AGGREGATE" | "LOGIC" | "STRICT"
    )
}

fn settings<'f>(
    records: &'f [Vec<Field>],
    places: &mut Vec<(&'static str, &'f Field)>,
) -> Result<(TableConfig, usize)> {
    let mut config = TableConfig::default();
    let mut at = 0;
    for record in records {
        let word = match keyword(&record[0]).as_str() {
            "HIT POLICY" => "HIT POLICY",
            "AGGREGATE" => "AGGREGATE",
            "LOGIC" => "LOGIC",
            "STRICT" => "STRICT",
            _ => break,
        };
        let first = &record[0];
        if places.iter().any(|(k, _)| *k == word) {
            return Err(format_error(
                first.line,
                first.column,
                format!("{word} is given twice"),
            ));
        }
        let takes_value = word != "STRICT";
        let skip = if takes_value { 2 } else { 1 };
        if let Some(extra) = record.iter().skip(skip).find(|f| !f.text.trim().is_empty()) {
            return Err(format_error(
                extra.line,
                extra.column,
                format!(
                    "{word} takes {}",
                    if takes_value { "one value" } else { "no value" }
                ),
            ));
        }
        let value = record.get(1).map(|f| f.text.trim()).unwrap_or_default();
        let place = record.get(1).unwrap_or(first);
        let wrong = |expected: &str| {
            format_error(
                place.line,
                place.column,
                format!("{word} takes {expected}, not {value:?}"),
            )
        };
        match word {
            "HIT POLICY" => {
                let (policy, aggregate) = TablePolicy::from_name(&value.to_ascii_lowercase())
                    .map(|p| (p, None))
                    .or_else(|| TablePolicy::from_dmn(value))
                    .ok_or_else(|| {
                        wrong("a policy: first, priority, unique, any, rule_order, output_order or collect, or a DMN letter")
                    })?;
                config.hit_policy = policy;
                if aggregate.is_some() {
                    if config.aggregate.is_some() {
                        return Err(format_error(
                            first.line,
                            first.column,
                            "the aggregate is given twice",
                        )
                        .with_hint("the DMN letters C+, C<, C> and C# already give one"));
                    }
                    config.aggregate = aggregate;
                }
            }
            "AGGREGATE" => {
                if config.aggregate.is_some() {
                    return Err(format_error(
                        first.line,
                        first.column,
                        "the aggregate is given twice",
                    )
                    .with_hint("the DMN letters C+, C<, C> and C# already give one"));
                }
                config.aggregate = Some(
                    Aggregate::from_name(&value.to_ascii_lowercase())
                        .ok_or_else(|| wrong("sum, min, max or count"))?,
                );
            }
            "LOGIC" => {
                config.logic = Some(
                    LogicName::from_name(&value.to_ascii_lowercase())
                        .map_err(|e| e.at_position(place.line, place.column))?,
                );
            }
            _ => config.strict = true,
        }
        places.push((word, first));
        at += 1;
    }
    Ok((config, at))
}

/// What a header field names (`spec/tables.md`, *CSV*).
pub(super) enum Heading {
    Name,
    Priority,
    Description,
    Input(Node),
    Output(TableOutput),
    /// Nothing: an empty or space-only field.
    Empty,
}

impl Heading {
    /// The keyword of a special column, for messages.
    pub(super) fn keyword(&self) -> Option<&'static str> {
        match self {
            Self::Name => Some("RULE"),
            Self::Priority => Some("PRIORITY"),
            Self::Description => Some("DESCRIPTION"),
            _ => None,
        }
    }
}

/// Read a header field's text; errors in it are located within `text`.
pub(super) fn heading(text: &str, limits: &Limits) -> Result<Heading> {
    let word = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    Ok(match word.as_str() {
        "RULE" => Heading::Name,
        "PRIORITY" => Heading::Priority,
        "DESCRIPTION" => Heading::Description,
        "" => Heading::Empty,
        _ if word == "THEN" || word.starts_with("THEN ") => {
            // The text after THEN, with errors located from the text's start.
            let start = text.len() - text.trim_start().len() + 4;
            let shift = u64::try_from(text[..start].chars().count()).unwrap_or(0);
            let (name, values) = parse_output(&text[start..], limits).map_err(|mut e| {
                let context = e.context_mut();
                if let (Some(1), Some(column)) = (context.line, context.column) {
                    context.column = Some(column + shift);
                }
                e
            })?;
            Heading::Output(TableOutput { name, values })
        }
        _ => Heading::Input(parse_expression(text, limits)?),
    })
}

/// The column of a heading, or the error of a repeated special column or an
/// empty heading, which `fail` locates.
pub(super) fn column(
    heading: Heading,
    columns: &[Column],
    inputs: &mut Vec<Node>,
    outputs: &mut Vec<TableOutput>,
    fail: impl Fn(String) -> Error,
) -> Result<Column> {
    if let Some(word) = heading.keyword()
        && columns.iter().any(|c| c.keyword() == Some(word))
    {
        return Err(fail(format!("{word} is a column twice")));
    }
    Ok(match heading {
        Heading::Name => Column::Name,
        Heading::Priority => Column::Priority,
        Heading::Description => Column::Description,
        Heading::Input(node) => {
            inputs.push(node);
            Column::Input(inputs.len() - 1)
        }
        Heading::Output(output) => {
            outputs.push(output);
            Column::Output(outputs.len() - 1)
        }
        Heading::Empty => {
            return Err(fail("a column has no header".into())
                .with_hint("name an input expression, or write THEN and an output key"));
        }
    })
}

fn header(fields: &[Field], limits: &Limits) -> Result<(Vec<Column>, Vec<Node>, Vec<TableOutput>)> {
    let (mut columns, mut inputs, mut outputs) = (Vec::new(), Vec::new(), Vec::new());
    for field in fields {
        let heading = heading(&field.text, limits).map_err(|e| locate(e, field))?;
        let fail = |message: String| format_error(field.line, field.column, message);
        let column = column(heading, &columns, &mut inputs, &mut outputs, fail)?;
        columns.push(column);
    }
    Ok((columns, inputs, outputs))
}

/// A name or description: a quoted field as it is, any other without the
/// space around it; `None` when empty.
fn as_written(field: &Field) -> Option<String> {
    let text = if field.quoted {
        field.text.as_str()
    } else {
        field.text.trim()
    };
    (!text.is_empty()).then(|| text.to_owned())
}

fn row(
    record: &[Field],
    columns: &[Column],
    inputs: &[Node],
    budget: &mut Budget,
    limits: &Limits,
) -> Result<TableRow> {
    if let Some(extra) = record
        .iter()
        .skip(columns.len())
        .find(|f| !f.text.trim().is_empty())
    {
        return Err(format_error(
            extra.line,
            extra.column,
            "the row has more fields than the header",
        ));
    }
    let first = &record[0];
    budget
        .spend(columns.len())
        .map_err(|e| e.at_position(first.line, first.column))?;
    let mut row = TableRow::new(vec![None; inputs.len()], Vec::new());
    let mut outputs = Vec::new();
    for (i, column) in columns.iter().enumerate() {
        let Some(field) = record.get(i) else {
            if matches!(column, Column::Output(_)) {
                outputs.push(None);
            }
            continue;
        };
        let text = field.text.trim();
        match column {
            Column::Name => row.name = as_written(field),
            Column::Description => row.description = as_written(field),
            Column::Priority if text.is_empty() => {}
            Column::Priority => {
                row.priority = Some(text.parse().map_err(|_| {
                    format_error(
                        field.line,
                        field.column,
                        format!("a priority is a 64-bit integer, not {text:?}"),
                    )
                })?);
            }
            Column::Input(c) => {
                row.inputs[*c] =
                    parse_cell(&field.text, &inputs[*c], limits).map_err(|e| locate(e, field))?;
                if let Some(cell) = &row.inputs[*c] {
                    // The cell's condition holds a copy of its column.
                    budget
                        .spend(size(cell))
                        .map_err(|e| e.at_position(field.line, field.column))?;
                }
            }
            Column::Output(_) if text.is_empty() => outputs.push(None),
            Column::Output(_) => {
                let value = parse_expression(&field.text, limits).map_err(|e| locate(e, field))?;
                budget
                    .spend(size(&value))
                    .map_err(|e| e.at_position(field.line, field.column))?;
                outputs.push(Some(value));
            }
        }
    }
    row.outputs = outputs;
    Ok(row)
}

impl DecisionTable {
    /// Read a table from CSV (`spec/tables.md`, *CSV*).
    ///
    /// # Errors
    ///
    /// `limit_exceeded` for a text over `limits.max_text_size`; `format_error`
    /// for CSV that is not well formed, a misplaced setting or column, or a row
    /// with too many fields; the errors of the text syntax for a header or a
    /// cell; and the errors of [`DecisionTable::new`]. All with the line and
    /// column where the problem is.
    pub fn from_csv(text: &str, limits: &Limits) -> Result<Self> {
        if text.len() > limits.max_text_size {
            return Err(Error::new(
                ErrorCode::LimitExceeded,
                format!(
                    "the text has {} bytes, more than the limit of {}",
                    text.len(),
                    limits.max_text_size
                ),
            )
            .with_hint("raise the size limit if the text is trusted"));
        }
        // The settings and the header: the records up to the first that is not a
        // setting. Rows are read one at a time, and not kept as records.
        let mut records = Records::new(text);
        let mut head = Vec::new();
        for record in records.by_ref() {
            let record = record?;
            let setting = is_setting(&record[0]);
            head.push(record);
            if !setting {
                break;
            }
        }
        let mut setting_places = Vec::new();
        let (config, at) = settings(&head, &mut setting_places)?;
        let Some(header_record) = head.get(at) else {
            let (line, column) = head.last().map_or((1, 1), |r| (r[0].line + 1, 1));
            return Err(format_error(line, column, "the table has no header line")
                .with_hint("name the columns: RULE, inputs, then THEN and an output key"));
        };
        // Spreadsheets may end lines with empty fields.
        let used = header_record
            .iter()
            .rposition(|f| !f.text.trim().is_empty())
            .map_or(0, |i| i + 1);
        let header_fields = &header_record[..used];
        let (columns, inputs, outputs) = header(header_fields, limits)?;
        let places = Places {
            settings: setting_places,
            header: header_fields,
            columns,
            rows: &[],
        };
        // The columns are checked before any row is read.
        check_columns(&inputs, &outputs).map_err(|e| places.locate(e))?;
        let mut budget = Budget::new(limits);
        let mut rows = Vec::new();
        for record in records {
            rows.push(row(
                &record?,
                &places.columns,
                &inputs,
                &mut budget,
                limits,
            )?);
        }
        Self::new(config, inputs, outputs, rows).map_err(|error| {
            // Rare: the records are read again to find where the error is.
            match Records::new(text).collect::<Result<Vec<_>>>() {
                Ok(all) => Places {
                    rows: all.get(at + 1..).unwrap_or_default(),
                    ..places
                }
                .locate(error),
                Err(_) => error,
            }
        })
    }

    /// The table's canonical CSV, each line ending with `\n` (`spec/tables.md`,
    /// *CSV*).
    #[must_use]
    pub fn to_csv(&self) -> String {
        let mut lines = vec![vec![
            "HIT POLICY".to_owned(),
            self.config.hit_policy.as_str().to_owned(),
        ]];
        if let Some(aggregate) = self.config.aggregate {
            lines.push(vec!["AGGREGATE".into(), aggregate.as_str().into()]);
        }
        if let Some(logic) = self.config.logic.filter(|l| *l != LogicName::Boolean) {
            lines.push(vec!["LOGIC".into(), logic.as_str().into()]);
        }
        if self.config.strict {
            lines.push(vec!["STRICT".into()]);
        }
        let named = self.rows.iter().any(|r| r.name.is_some());
        let ranked = self.rows.iter().any(|r| r.priority.is_some());
        let described = self.rows.iter().any(|r| r.description.is_some());
        let mut header = Vec::new();
        if named {
            header.push("RULE".to_owned());
        }
        if ranked {
            header.push("PRIORITY".to_owned());
        }
        header.extend(self.inputs.iter().map(table_text));
        header.extend(self.outputs.iter().map(output_header));
        if described {
            header.push("DESCRIPTION".to_owned());
        }
        lines.push(header);
        for row in &self.rows {
            let mut fields = Vec::new();
            if named {
                fields.push(row.name.clone().unwrap_or_default());
            }
            if ranked {
                fields.push(row.priority.map(|p| p.to_string()).unwrap_or_default());
            }
            for (cell, input) in row.inputs.iter().zip(&self.inputs) {
                fields.push(cell.as_ref().map_or_else(
                    || "-".to_owned(),
                    |c| cell_text(c, input).unwrap_or_default(),
                ));
            }
            for cell in &row.outputs {
                fields.push(cell.as_ref().map(table_text).unwrap_or_default());
            }
            if described {
                fields.push(row.description.clone().unwrap_or_default());
            }
            lines.push(fields);
        }
        let mut out = String::new();
        for fields in lines {
            let quoted: Vec<String> = fields.iter().map(|f| quote(f)).collect();
            let _ = writeln!(out, "{}", quoted.join(","));
        }
        out
    }
}

/// `THEN key` or `THEN key IN (value, …)`, in the style of tables.
pub(super) fn output_header(output: &TableOutput) -> String {
    let key = if is_name(&output.name) {
        output.name.clone()
    } else {
        string_text(&output.name, true)
    };
    if output.values.is_empty() {
        format!("THEN {key}")
    } else {
        let values: Vec<String> = output.values.iter().map(|v| styled(v, true)).collect();
        format!("THEN {key} IN ({})", values.join(", "))
    }
}

/// A field, between double quotes when it holds a comma, a double quote, a
/// line break, or space at either end.
fn quote(field: &str) -> String {
    let needs = field.contains([',', '"', '\n', '\r'])
        || field.starts_with(char::is_whitespace)
        || field.ends_with(char::is_whitespace);
    if needs {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}
