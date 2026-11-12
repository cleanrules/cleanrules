//! A decision table's grid in HTML (`spec/visual.md`, *Decision tables*).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::model::{Class, Kind, line, too_large};
use super::render::{html_description, html_text, table_div};
use crate::error::{Error, ErrorCode, Result};
use crate::explain::Status;
use crate::explain::statuses;
use crate::explain::text::status;
use crate::json::Limits;
use crate::rules::Decision;
use crate::table::csv::output_header;
use crate::table::{DecisionTable, label};
use crate::text::{cell_text, table_text};

/// The status of each row's rule, in row order, or `decision_mismatch` when
/// the decision's rules are not the table's: another count, or a row whose
/// rule the decision lacks, named in the message.
fn row_statuses(table: &DecisionTable, decision: &Decision<'_>) -> Result<Vec<Status>> {
    let results = decision.results();
    let by_name: BTreeMap<&str, Status> = results
        .iter()
        .map(|r| r.rule.name())
        .zip(statuses(decision))
        .collect();
    let rows = table.rows();
    let mismatch = |why: String| {
        Error::new(
            ErrorCode::DecisionMismatch,
            format!("the decision is not of this table's rule set: {why}"),
        )
        .with_hint("draw the table with a decision of the rule set it compiles into")
    };
    let mut found = Vec::with_capacity(rows.len());
    for (r, row) in rows.iter().enumerate() {
        let name = label(r, row);
        match by_name.get(name.as_str()) {
            Some(&status) => found.push(status),
            None => {
                return Err(mismatch(format!(
                    "it has no rule {name:?}, the rule of row {}",
                    r + 1
                )));
            }
        }
    }
    if results.len() != rows.len() {
        return Err(mismatch(format!(
            "it has {} rules, and the table {} rows",
            results.len(),
            rows.len()
        )));
    }
    Ok(found)
}

/// A cell: a header (`th`) or data (`td`), its text bounded and escaped.
fn cell(out: &mut String, tag: &str, text: &str) {
    let _ = write!(out, "<{tag}>{}</{tag}>", html_text(&line(text)));
}

/// The grid in its `div`.
pub(super) fn grid(
    table: &DecisionTable,
    decision: Option<&Decision<'_>>,
    limits: &Limits,
) -> Result<String> {
    let statuses = decision.map(|d| row_statuses(table, d)).transpose()?;
    let rows = table.rows();
    let ranked = rows.iter().any(|r| r.priority.is_some());
    let described = rows.iter().any(|r| r.description.is_some());
    let columns = 1
        + usize::from(statuses.is_some())
        + usize::from(ranked)
        + table.inputs().len()
        + table.outputs().len()
        + usize::from(described);
    // Counted before anything is written: the header and every cell.
    let limit = limits.max_diagram_size;
    if (rows.len() + 1).saturating_mul(columns) > limit {
        return Err(too_large(Kind::Table, limit));
    }
    let config = table.config();
    let mut caption = format!("HIT POLICY {}", config.hit_policy.as_str());
    if let Some(aggregate) = config.aggregate {
        let _ = write!(caption, " AGGREGATE {}", aggregate.as_str());
    }
    let mut out = format!(
        "<table class=\"grid\">\n<caption>{}</caption>\n<thead><tr>",
        html_text(&line(&caption))
    );
    cell(&mut out, "th", "RULE");
    if statuses.is_some() {
        cell(&mut out, "th", "STATUS");
    }
    if ranked {
        cell(&mut out, "th", "PRIORITY");
    }
    for input in table.inputs() {
        cell(&mut out, "th", &table_text(input));
    }
    for output in table.outputs() {
        cell(&mut out, "th", &output_header(output));
    }
    if described {
        cell(&mut out, "th", "DESCRIPTION");
    }
    out.push_str("</tr></thead>\n<tbody>\n");
    for (r, row) in rows.iter().enumerate() {
        match &statuses {
            Some(statuses) => {
                let class = Class::of_status(statuses[r]);
                let _ = write!(
                    out,
                    "<tr class=\"{}\" style=\"{}\">",
                    class.name(),
                    class.css()
                );
            }
            None => out.push_str("<tr>"),
        }
        let _ = write!(
            out,
            "<th scope=\"row\">{}</th>",
            html_text(&line(&label(r, row)))
        );
        if let Some(statuses) = &statuses {
            cell(&mut out, "td", status(statuses[r]).1);
        }
        if ranked {
            let priority = row.priority.map(|p| p.to_string()).unwrap_or_default();
            cell(&mut out, "td", &priority);
        }
        for (input, subject) in row.inputs.iter().zip(table.inputs()) {
            // A table refuses, when it is built, an input cell with no cell
            // text (`spec/tables.md`, *Canonical text*), so the fallback, the
            // whole condition, is never written.
            let text = input.as_ref().map_or_else(
                || "-".to_owned(),
                |c| cell_text(c, subject).unwrap_or_else(|| c.to_string()),
            );
            cell(&mut out, "td", &text);
        }
        for output in &row.outputs {
            cell(
                &mut out,
                "td",
                &output.as_ref().map(table_text).unwrap_or_default(),
            );
        }
        if described {
            let _ = write!(
                out,
                "<td class=\"description\">{}</td>",
                row.description
                    .as_deref()
                    .map(html_description)
                    .unwrap_or_default()
            );
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</tbody>\n</table>\n");
    Ok(table_div(&out))
}
