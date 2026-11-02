//! Decision tables in JSON (`spec/tables.md`, *JSON*).

use crate::ast::Node;
use crate::error::{Error, Result};
use crate::format::rules::{at, format_error, get, no_unknown_keys};
use crate::format::value::{ValueContext, read_value, write_value};
use crate::json::{Json, Limits};
use crate::logic::LogicName;
use crate::rules::Aggregate;
use crate::table::{
    Budget, DecisionTable, TableConfig, TableOutput, TablePolicy, TableRow, check_columns, size,
};
use crate::text::{cell_text, parse_cell, parse_expression, table_text};
use crate::value::Value;

const TABLE_KEYS: [&str; 7] = [
    "hit_policy",
    "aggregate",
    "logic",
    "strict",
    "inputs",
    "outputs",
    "rows",
];
const OUTPUT_KEYS: [&str; 2] = ["name", "values"];
const ROW_KEYS: [&str; 5] = ["name", "priority", "description", "inputs", "outputs"];

/// A row whose shape is valid: its cells are not read yet.
struct RawRow<'j> {
    name: Option<String>,
    priority: Option<i64>,
    description: Option<String>,
    inputs: Vec<&'j str>,
    outputs: Vec<Option<&'j str>>,
}

fn strings<'j>(json: Option<&'j Json>, pointer: &str, what: &str) -> Result<Vec<&'j str>> {
    let Some(Json::Array(items)) = json else {
        return Err(format_error(
            pointer,
            format!("{what} must be an array of strings"),
        ));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| match item {
            Json::String(text) => Ok(text.as_str()),
            _ => Err(format_error(
                &format!("{pointer}/{i}"),
                format!("{what} holds strings"),
            )),
        })
        .collect()
}

fn optional_string(members: &[(String, Json)], key: &str, pointer: &str) -> Result<Option<String>> {
    match get(members, key) {
        None => Ok(None),
        Some(Json::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format_error(
            &at(pointer, key),
            format!("{key:?} must be a string"),
        )),
    }
}

/// The table's configuration, and the name of its logic, which is looked up
/// after the shape is checked, as for rule sets.
fn config<'j>(
    members: &'j [(String, Json)],
    pointer: &str,
) -> Result<(TableConfig, Option<&'j str>)> {
    let name = |key: &str| match get(members, key) {
        None => Ok(None),
        Some(Json::String(text)) => Ok(Some(text.as_str())),
        Some(_) => Err(format_error(
            &at(pointer, key),
            format!("{key:?} must be a string"),
        )),
    };
    let hit_policy = match name("hit_policy")? {
        None => TablePolicy::First,
        Some(text) => TablePolicy::from_name(text).ok_or_else(|| {
            format_error(
                &at(pointer, "hit_policy"),
                format!("unknown hit policy {text:?}"),
            )
        })?,
    };
    let aggregate = match name("aggregate")? {
        None => None,
        Some(text) => Some(Aggregate::from_name(text).ok_or_else(|| {
            format_error(
                &at(pointer, "aggregate"),
                format!("unknown aggregate {text:?}"),
            )
        })?),
    };
    let logic = name("logic")?;
    let strict = match get(members, "strict") {
        None => false,
        Some(Json::Bool(strict)) => *strict,
        Some(_) => {
            return Err(format_error(
                &at(pointer, "strict"),
                "\"strict\" must be a boolean",
            ));
        }
    };
    let config = TableConfig {
        hit_policy,
        aggregate,
        logic: None,
        strict,
    };
    Ok((config, logic))
}

fn outputs_shape(json: Option<&Json>, pointer: &str) -> Result<Vec<TableOutput>> {
    let Some(Json::Array(items)) = json else {
        return Err(format_error(
            pointer,
            "\"outputs\" must be an array of objects",
        ));
    };
    let mut outputs = Vec::new();
    for (o, item) in items.iter().enumerate() {
        let here = format!("{pointer}/{o}");
        let Json::Object(members) = item else {
            return Err(format_error(&here, "an output must be an object"));
        };
        let name = match get(members, "name") {
            Some(Json::String(name)) => name.clone(),
            Some(_) => {
                return Err(format_error(
                    &at(&here, "name"),
                    "\"name\" must be a string",
                ));
            }
            None => {
                return Err(format_error(
                    &at(&here, "name"),
                    "the key \"name\" is missing",
                ));
            }
        };
        let values = match get(members, "values") {
            None => Vec::new(),
            Some(Json::Array(values)) => values
                .iter()
                .enumerate()
                .map(|(v, value)| {
                    let here = format!("{}/{v}", at(&here, "values"));
                    if matches!(value, Json::Null) {
                        return Err(format_error(
                            &here,
                            "an output value is a constant other than null",
                        ));
                    }
                    read_value(value, &here, ValueContext::Constant)
                })
                .collect::<Result<_>>()?,
            Some(_) => {
                return Err(format_error(
                    &at(&here, "values"),
                    "\"values\" must be an array of constants",
                ));
            }
        };
        no_unknown_keys(members, &OUTPUT_KEYS, &here)?;
        outputs.push(TableOutput { name, values });
    }
    Ok(outputs)
}

/// The rows' shape, with as many cells as the table has inputs and outputs.
fn rows_shape<'j>(
    json: Option<&'j Json>,
    pointer: &str,
    columns: (usize, usize),
) -> Result<Vec<RawRow<'j>>> {
    let Some(Json::Array(items)) = json else {
        return Err(format_error(
            pointer,
            "\"rows\" must be an array of objects",
        ));
    };
    let mut rows = Vec::new();
    for (r, item) in items.iter().enumerate() {
        let here = format!("{pointer}/{r}");
        let Json::Object(members) = item else {
            return Err(format_error(&here, "a row must be an object"));
        };
        let name = optional_string(members, "name", &here)?;
        let priority = match get(members, "priority") {
            None => None,
            Some(json) => match read_value(json, &at(&here, "priority"), ValueContext::Constant)? {
                Value::Int(i) => Some(i),
                _ => {
                    return Err(format_error(
                        &at(&here, "priority"),
                        "\"priority\" must be an integer",
                    ));
                }
            },
        };
        let description = optional_string(members, "description", &here)?;
        let inputs = strings(get(members, "inputs"), &at(&here, "inputs"), "\"inputs\"")?;
        let Some(Json::Array(cells)) = get(members, "outputs") else {
            return Err(format_error(
                &at(&here, "outputs"),
                "\"outputs\" must be an array of strings and nulls",
            ));
        };
        let outputs: Vec<Option<&str>> = cells
            .iter()
            .enumerate()
            .map(|(c, cell)| match cell {
                Json::String(text) => Ok(Some(text.as_str())),
                Json::Null => Ok(None),
                _ => Err(format_error(
                    &format!("{}/{c}", at(&here, "outputs")),
                    "an output cell is a string or null",
                )),
            })
            .collect::<Result<_>>()?;
        no_unknown_keys(members, &ROW_KEYS, &here)?;
        for (what, count, expected) in [
            ("inputs", inputs.len(), columns.0),
            ("outputs", outputs.len(), columns.1),
        ] {
            if count != expected {
                return Err(format_error(
                    &at(&here, what),
                    format!("row {} has {count} {what}; the table has {expected}", r + 1),
                ));
            }
        }
        rows.push(RawRow {
            name,
            priority,
            description,
            inputs,
            outputs,
        });
    }
    Ok(rows)
}

/// An error in the text of a header or cell, located by the string's pointer;
/// its position within the text goes into the message.
fn in_text(error: &Error, pointer: &str) -> Error {
    let context = error.context();
    let message = match (context.line, context.column) {
        (Some(1), Some(column)) => format!("{} (column {column} of the text)", error.message()),
        (Some(line), Some(column)) => {
            format!(
                "{} (line {line}, column {column} of the text)",
                error.message()
            )
        }
        _ => error.message().to_owned(),
    };
    let mut context = context.clone();
    context.line = None;
    context.column = None;
    context.pointer = Some(pointer.to_owned());
    let located = Error::new(error.code(), message).with_context(context);
    match error.hint() {
        Some(hint) => located.with_hint(hint.to_owned()),
        None => located,
    }
}

/// Read a decision table at `pointer` (`spec/tables.md`, *JSON*): its shape,
/// then its headers and cells, then the table.
pub(crate) fn read_table(json: &Json, pointer: &str, limits: &Limits) -> Result<DecisionTable> {
    let Json::Object(members) = json else {
        return Err(format_error(pointer, "a decision table must be an object"));
    };
    let relocated = |mut error: Error| {
        let relative = error.context().pointer.clone().unwrap_or_default();
        error.context_mut().pointer = Some(format!("{pointer}{relative}"));
        error
    };
    let (mut config, logic) = config(members, pointer)?;
    let input_texts = strings(get(members, "inputs"), &at(pointer, "inputs"), "\"inputs\"")?;
    let outputs = outputs_shape(get(members, "outputs"), &at(pointer, "outputs"))?;
    let raw_rows = rows_shape(
        get(members, "rows"),
        &at(pointer, "rows"),
        (input_texts.len(), outputs.len()),
    )?;
    no_unknown_keys(members, &TABLE_KEYS, pointer)?;
    let inputs = input_texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            parse_expression(text, limits)
                .map_err(|e| in_text(&e, &format!("{pointer}/inputs/{i}")))
        })
        .collect::<Result<Vec<Node>>>()?;
    // The columns are checked before any row is read.
    check_columns(&inputs, &outputs).map_err(relocated)?;
    let mut budget = Budget::new(limits);
    let mut rows = Vec::with_capacity(raw_rows.len());
    for (r, raw) in raw_rows.into_iter().enumerate() {
        let here = format!("{pointer}/rows/{r}");
        budget
            .spend(inputs.len() + outputs.len())
            .map_err(|e| e.at_pointer(here.clone()))?;
        let mut row = TableRow::new(Vec::with_capacity(inputs.len()), Vec::new());
        row.name = raw.name;
        row.priority = raw.priority;
        row.description = raw.description;
        for ((c, text), subject) in raw.inputs.iter().enumerate().zip(&inputs) {
            let cell = parse_cell(text, subject, limits)
                .map_err(|e| in_text(&e, &format!("{here}/inputs/{c}")))?;
            if let Some(cell) = &cell {
                // The cell's condition holds a copy of its column.
                budget
                    .spend(size(cell))
                    .map_err(|e| e.at_pointer(format!("{here}/inputs/{c}")))?;
            }
            row.inputs.push(cell);
        }
        for (c, text) in raw.outputs.iter().enumerate() {
            let cell = match text {
                Some(text) if !text.trim().is_empty() => {
                    let pointer = format!("{here}/outputs/{c}");
                    let value =
                        parse_expression(text, limits).map_err(|e| in_text(&e, &pointer))?;
                    budget
                        .spend(size(&value))
                        .map_err(|e| e.at_pointer(pointer))?;
                    Some(value)
                }
                _ => None,
            };
            row.outputs.push(cell);
        }
        rows.push(row);
    }
    config.logic = logic
        .map(|text| LogicName::from_name(text).map_err(|e| e.at_pointer(at(pointer, "logic"))))
        .transpose()?;
    DecisionTable::new(config, inputs, outputs, rows).map_err(relocated)
}

/// Write a decision table's content (the object under `"decision_table"`).
#[must_use]
pub fn write_table(table: &DecisionTable) -> Json {
    let config = table.config();
    let mut members = vec![(
        "hit_policy".to_owned(),
        Json::String(config.hit_policy.as_str().into()),
    )];
    if let Some(aggregate) = config.aggregate {
        members.push(("aggregate".into(), Json::String(aggregate.as_str().into())));
    }
    members.push((
        "logic".into(),
        Json::String(config.logic.unwrap_or(LogicName::Boolean).as_str().into()),
    ));
    if config.strict {
        members.push(("strict".into(), Json::Bool(true)));
    }
    members.push((
        "inputs".into(),
        Json::Array(
            table
                .inputs()
                .iter()
                .map(|i| Json::String(table_text(i)))
                .collect(),
        ),
    ));
    let outputs = table
        .outputs()
        .iter()
        .map(|output| {
            let mut fields = vec![("name".to_owned(), Json::String(output.name.clone()))];
            if !output.values.is_empty() {
                fields.push((
                    "values".into(),
                    Json::Array(output.values.iter().map(write_value).collect()),
                ));
            }
            Json::Object(fields)
        })
        .collect();
    members.push(("outputs".into(), Json::Array(outputs)));
    let rows = table
        .rows()
        .iter()
        .map(|row| {
            let mut fields = Vec::new();
            if let Some(name) = &row.name {
                fields.push(("name".to_owned(), Json::String(name.clone())));
            }
            if let Some(priority) = row.priority {
                fields.push(("priority".to_owned(), write_value(&Value::Int(priority))));
            }
            if let Some(description) = &row.description {
                fields.push(("description".to_owned(), Json::String(description.clone())));
            }
            let inputs = row
                .inputs
                .iter()
                .zip(table.inputs())
                .map(|(cell, input)| {
                    Json::String(cell.as_ref().map_or_else(
                        || "-".to_owned(),
                        |c| cell_text(c, input).unwrap_or_default(),
                    ))
                })
                .collect();
            fields.push(("inputs".into(), Json::Array(inputs)));
            let outputs = row
                .outputs
                .iter()
                .map(|cell| {
                    cell.as_ref()
                        .map_or(Json::Null, |c| Json::String(table_text(c)))
                })
                .collect();
            fields.push(("outputs".into(), Json::Array(outputs)));
            Json::Object(fields)
        })
        .collect();
    members.push(("rows".into(), Json::Array(rows)));
    Json::Object(members)
}
