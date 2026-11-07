//! The text of explanations and summaries (`spec/explain.md`, *Text*).
//!
//! Conditions, field paths, names and constants are written as the canonical
//! text writes them (`spec/text.md`, *Printing*), so a condition in an
//! explanation reads back as the rule's own. Data values are bounded: long
//! strings and collections are cut, so one input cannot flood the text.

use std::cmp::Ordering;
use std::fmt::{self, Write as _};

use super::{
    Cause, Explanation, Gap, GroupKind, Note, NoteKind, Requirement, RuleExplanation, Status,
    Summary,
};
use crate::ast::CompareOp;
use crate::error::Segment;
use crate::eval::{Reason, ReasonCode};
use crate::json::Json;
use crate::logic::Truth;
use crate::text::{key, literal, path, positional};
use crate::value::Value;

/// The longest string value shown whole, in characters.
const MAX_STRING: usize = 60;
/// The most items of a list or map shown.
const MAX_ITEMS: usize = 6;
/// How deep lists and maps are shown before `[...]` and `{...}`.
const MAX_NESTING: usize = 2;
/// The longest condition in a summary's rule table, in characters.
const MAX_CONDITION: usize = 60;

/// A rule name, tag or payload key: as is when it is a name, quoted otherwise.
fn name(text: &str) -> String {
    key(text, '"')
}

fn names<'s>(items: impl IntoIterator<Item = &'s str>) -> String {
    items.into_iter().map(name).collect::<Vec<_>>().join(", ")
}

/// A field path; the empty path is the input itself.
fn field(segments: &[Segment]) -> String {
    if segments.is_empty() {
        "the input".into()
    } else {
        path(segments)
    }
}

fn quoted(text: &str) -> String {
    crate::json::to_string(&Json::String(text.to_owned()))
}

/// A string between double quotes with JSON escapes, its first
/// `MAX_STRING - 3` characters and `...` when it is longer than [`MAX_STRING`].
fn bounded(text: &str) -> String {
    if text.chars().count() <= MAX_STRING {
        return quoted(text);
    }
    let cut: String = text.chars().take(MAX_STRING - 3).collect();
    let shown = quoted(&cut);
    format!("{}...\"", &shown[..shown.len() - 1])
}

/// Text from outside the rules, such as a native type's name or an error
/// message: bounded and escaped like a string, without the quotes, so it stays
/// on one line.
fn plain(text: &str) -> String {
    let shown = bounded(text);
    shown[1..shown.len() - 1].to_owned()
}

/// A data value, as a constant of the text syntax when it is a scalar.
fn value(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out
}

fn write_value(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(&bounded(text)),
        Value::List(_) | Value::Map(_) if depth >= MAX_NESTING => {
            out.push_str(if matches!(value, Value::List(_)) {
                "[...]"
            } else {
                "{...}"
            });
        }
        Value::List(items) => {
            out.push('[');
            for (i, item) in items.iter().take(MAX_ITEMS).enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(item, depth + 1, out);
            }
            if items.len() > MAX_ITEMS {
                out.push_str(", ...");
            }
            out.push(']');
        }
        Value::Map(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().take(MAX_ITEMS).enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&bounded(k));
                out.push_str(": ");
                write_value(v, depth + 1, out);
            }
            if map.len() > MAX_ITEMS {
                out.push_str(", ...");
            }
            out.push('}');
        }
        Value::Opaque(opaque) => {
            let _ = write!(out, "<{}>", plain(&opaque.type_name));
        }
        // Lists, maps and opaque values, which `literal` does not take, are
        // handled above.
        scalar => out.push_str(&literal(scalar)),
    }
}

fn symbol(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "=",
        CompareOp::Ne => "!=",
        CompareOp::Lt => "<",
        CompareOp::Le => "<=",
        CompareOp::Gt => ">",
        CompareOp::Ge => ">=",
    }
}

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

/// A float gap, to 12 significant digits so float noise does not show
/// (`0.05`, not `0.05000000000000004`).
fn float_gap(gap: f64) -> String {
    let rounded: f64 = format!("{gap:.11e}").parse().unwrap_or(gap);
    positional(rounded).unwrap_or_else(|| format!("{rounded:e}"))
}

/// A duration in microseconds: days, hours, minutes and seconds, the zero ones
/// left out (`1 day 2 hours`, `1.5 seconds`).
fn duration(micros: i64) -> String {
    const SECOND: u64 = 1_000_000;
    let total = micros.unsigned_abs();
    let (days, rest) = (total / (86_400 * SECOND), total % (86_400 * SECOND));
    let (hours, rest) = (rest / (3_600 * SECOND), rest % (3_600 * SECOND));
    let (minutes, rest) = (rest / (60 * SECOND), rest % (60 * SECOND));
    let (seconds, fraction) = (rest / SECOND, rest % SECOND);
    let mut parts = Vec::new();
    for (count, unit) in [(days, "day"), (hours, "hour"), (minutes, "minute")] {
        if count > 0 {
            parts.push(plural(count, unit));
        }
    }
    if fraction > 0 {
        let digits = format!("{fraction:06}");
        parts.push(format!(
            "{seconds}.{} seconds",
            digits.trim_end_matches('0')
        ));
    } else if seconds > 0 || parts.is_empty() {
        parts.push(plural(seconds, "second"));
    }
    let sign = if micros < 0 { "-" } else { "" };
    format!("{sign}{}", parts.join(" "))
}

fn gap(gap: &Gap) -> String {
    match gap {
        Gap::Number(Value::Float(f)) => float_gap(*f),
        Gap::Number(Value::Int(i)) => i.to_string(),
        Gap::Number(Value::Decimal(d)) => d.to_string(),
        Gap::Number(other) => value(other),
        Gap::Days(days) => {
            let sign = if *days < 0 { "-" } else { "" };
            format!("{sign}{}", plural(days.unsigned_abs(), "day"))
        }
        Gap::Microseconds(micros) => duration(*micros),
    }
}

fn is_zero(gap: &Gap) -> bool {
    match gap {
        Gap::Number(number) => number.compare(&Value::Int(0)) == Some(Ordering::Equal),
        Gap::Days(days) => *days == 0,
        Gap::Microseconds(micros) => *micros == 0,
    }
}

fn reason(reason: &Reason) -> String {
    let at = reason
        .paths
        .iter()
        .map(|p| field(p))
        .collect::<Vec<_>>()
        .join(", ");
    let within = |text: &str| {
        if reason.paths.is_empty() {
            text.to_owned()
        } else {
            format!("{text} in {at}")
        }
    };
    match reason.code {
        ReasonCode::MissingField => format!("missing field {at}"),
        ReasonCode::Null if reason.paths.is_empty() => "null value".into(),
        ReasonCode::Null => format!("{at} is null"),
        ReasonCode::TypeMismatch => {
            let types: Vec<&str> = reason.types.iter().map(|g| g.as_str()).collect();
            let shown = reason
                .detail
                .as_deref()
                .map_or_else(|| types.join(" vs "), plain);
            format!("type mismatch: {shown}")
        }
        ReasonCode::Unorderable => "cannot order booleans".into(),
        ReasonCode::DivisionByZero => "division by zero".into(),
        ReasonCode::IntOverflow => within("integer overflow"),
        ReasonCode::DecimalOverflow => within("decimal out of range"),
        ReasonCode::NanResult => "NaN result".into(),
        ReasonCode::ReadError => match &reason.detail {
            Some(detail) => format!("cannot read {at}: {}", plain(detail)),
            None => format!("cannot read {at}"),
        },
    }
}

/// `x > 10 is false (x = 8); short by 2`.
impl fmt::Display for Cause<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let truth = self.truth();
        let word = match truth {
            Truth::True => "true",
            Truth::False => "false",
            Truth::Unknown => "unknown",
        };
        write!(f, "{} is {word}", self.result.node)?;
        if !self.values.is_empty() {
            let shown: Vec<String> = self
                .values
                .iter()
                .map(|(p, v)| format!("{} = {}", field(p), value(v)))
                .collect();
            write!(f, " ({})", shown.join(", "))?;
        }
        if let Some(why) = self.reason() {
            write!(f, ": {}", reason(why))?;
        } else if !self.holds() && self.negated {
            f.write_str(", but the rule needs it false")?;
        }
        if let Some(counterfactual) = &self.counterfactual {
            let needs = format!(
                "needs {} {} {}",
                field(&counterfactual.path),
                symbol(counterfactual.op),
                value(&counterfactual.target)
            );
            match &counterfactual.gap {
                None => write!(f, "; {needs}")?,
                Some(g) if is_zero(g) => write!(f, "; at the limit ({needs})")?,
                Some(g) => {
                    let side = if matches!(counterfactual.op, CompareOp::Gt | CompareOp::Ge) {
                        "short"
                    } else {
                        "over"
                    };
                    let distance = format!("{side} by {}", gap(g));
                    if self.negated {
                        write!(f, "; {needs} ({distance})")?;
                    } else {
                        write!(f, "; {distance}")?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn requirement(
    requirement: &Requirement<'_, '_>,
    indent: &str,
    top: bool,
    fired: bool,
    out: &mut Vec<String>,
) {
    // Lines in pre-order, with an explicit stack: a requirement is as deep as
    // its condition. Each entry is a requirement, its indent and whether it
    // is the top one.
    let mut pending = vec![(requirement, indent.to_owned(), top)];
    while let Some((requirement, indent, top)) = pending.pop() {
        match requirement {
            Requirement::Cause(cause) => out.push(format!("{indent}- {cause}")),
            Requirement::Group(kind, items) => {
                let kind = match kind {
                    GroupKind::All => "all",
                    GroupKind::Any => "any",
                };
                let inner = if top {
                    let (lead, verb) = if fired {
                        ("because", "of these hold:")
                    } else {
                        ("would fire if", "of these changed:")
                    };
                    out.push(format!("{indent}{lead} {kind} {verb}"));
                    indent
                } else {
                    out.push(format!("{indent}- {kind} of:"));
                    format!("{indent}  ")
                };
                pending.extend(items.iter().rev().map(|item| (item, inner.clone(), false)));
            }
        }
    }
}

fn note(note: &Note) -> String {
    let winner = name(&note.winner);
    match (note.kind, note.priorities) {
        (NoteKind::LowerPriority, Some((mine, theirs))) => {
            format!("fired, but {winner} has a higher priority ({theirs} > {mine})")
        }
        (NoteKind::SamePriority, Some((mine, _))) => {
            format!("fired, but {winner} has the same priority ({mine}) and comes first")
        }
        (NoteKind::AnyAgrees, _) => {
            format!("fired; its output agrees with {winner}, which comes first")
        }
        _ => format!("fired, but {winner} comes first"),
    }
}

/// One rule: its mark, name, status and condition, then its description, note
/// and requirement, each on its own line.
impl fmt::Display for RuleExplanation<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (mark, label) = match self.status {
            Status::Selected => ("[x]", "selected"),
            Status::Fired => ("[x]", "fired, not selected"),
            Status::NotFired => ("[ ]", "not fired"),
            Status::Unknown => ("[?]", "unknown"),
        };
        let mut lines = vec![format!(
            "{mark} {} ({label}): {}",
            name(self.rule.name()),
            self.rule.when()
        )];
        if let Some(description) = self.rule.get_description() {
            lines.extend(
                description
                    .lines()
                    .map(|line| format!("    {line}").trim_end().to_owned()),
            );
        }
        if let Some(n) = &self.note {
            lines.push(format!("    {}", note(n)));
        }
        let fired = matches!(self.status, Status::Selected | Status::Fired);
        if self.achievable == Some(false) {
            lines.push("    cannot fire by changing these conditions alone; they conflict:".into());
            requirement(&self.requirement, "    ", false, fired, &mut lines);
        } else {
            requirement(&self.requirement, "    ", true, fired, &mut lines);
        }
        f.write_str(&lines.join("\n"))
    }
}

/// The header (`HIT POLICY priority: selected e`, with `AGGREGATE` when there
/// is one), then every rule in order.
impl fmt::Display for Explanation<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let selected: Vec<&str> = self.decision.selected().map(|r| r.rule.name()).collect();
        let chosen = if selected.is_empty() {
            "no rule".to_owned()
        } else {
            names(selected)
        };
        let ruleset = self.decision.ruleset();
        write!(f, "HIT POLICY {}", ruleset.hit_policy().as_str())?;
        if let Some(aggregate) = ruleset.aggregate() {
            write!(f, " AGGREGATE {}", aggregate.as_str())?;
        }
        write!(f, ": selected {chosen}")?;
        for rule in &self.rules {
            write!(f, "\n{rule}")?;
        }
        Ok(())
    }
}

fn width(text: &str) -> usize {
    text.chars().count()
}

fn pad(text: &str, to: usize) -> String {
    format!("{text}{}", " ".repeat(to.saturating_sub(width(text))))
}

/// The rule table: priority, name, condition (cut to [`MAX_CONDITION`]
/// characters) and outputs, in aligned columns.
fn rule_table(summary: &Summary<'_>, lines: &mut Vec<String>) {
    let rules = summary.ruleset.rules();
    if rules.is_empty() {
        lines.push("  (no rules)".into());
        return;
    }
    let mut rows = vec![[
        "priority".to_owned(),
        "name".to_owned(),
        "condition".to_owned(),
        "outputs".to_owned(),
    ]];
    for (i, rule) in rules.iter().enumerate() {
        let mut condition = rule.when().to_string();
        if width(&condition) > MAX_CONDITION {
            condition = condition
                .chars()
                .take(MAX_CONDITION - 3)
                .collect::<String>()
                + "...";
        }
        let mut outputs = names(rule.then().iter().map(|(k, _)| k.as_str()));
        if summary.actions.get(i).copied().unwrap_or(false) {
            if !outputs.is_empty() {
                outputs.push(' ');
            }
            match rule.get_action_name() {
                Some(action) => {
                    outputs.push_str("DO ");
                    outputs.push_str(&name(action));
                }
                None => outputs.push_str("+action"),
            }
        }
        if outputs.is_empty() {
            outputs.push('-');
        }
        rows.push([
            rule.get_priority().to_string(),
            name(rule.name()),
            condition,
            outputs,
        ]);
    }
    let widths: Vec<usize> = (0..3)
        .map(|c| rows.iter().map(|r| width(&r[c])).max().unwrap_or(0))
        .collect();
    for row in &rows {
        let priority = " ".repeat(widths[0] - width(&row[0])) + &row[0];
        let line = format!(
            "  {priority}  {}  {}  {}",
            pad(&row[1], widths[1]),
            pad(&row[2], widths[2]),
            row[3]
        );
        lines.push(line.trim_end().to_owned());
    }
}

/// Each field read, with the conditions and payload values that read it.
fn field_table(summary: &Summary<'_>, lines: &mut Vec<String>) {
    if summary.fields.is_empty() {
        return;
    }
    lines.push(format!("Fields read ({}):", summary.fields.len()));
    let paths: Vec<String> = summary.fields.iter().map(|u| field(&u.path)).collect();
    let column = paths.iter().map(|p| width(p)).max().unwrap_or(0);
    for (usage, shown) in summary.fields.iter().zip(&paths) {
        let mut places = Vec::new();
        if !usage.conditions.is_empty() {
            places.push(format!(
                "conditions of {}",
                names(usage.conditions.iter().map(String::as_str))
            ));
        }
        if !usage.payloads.is_empty() {
            let pairs: Vec<String> = usage
                .payloads
                .iter()
                .map(|(rule, k)| format!("{} ({})", name(rule), name(k)))
                .collect();
            places.push(format!("payloads of {}", pairs.join(", ")));
        }
        lines.push(format!("  {}  {}", pad(shown, column), places.join("; ")));
    }
}

/// The rule set's configuration, a table of its rules, the fields it reads, its
/// outputs and its tags.
impl fmt::Display for Summary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ruleset = self.ruleset;
        let mut config = vec![
            plural(ruleset.rules().len() as u64, "rule"),
            format!("hit policy {}", ruleset.hit_policy().as_str()),
        ];
        if let Some(aggregate) = ruleset.aggregate() {
            config.push(format!("aggregate {}", aggregate.as_str()));
        }
        config.push(format!("logic {}", ruleset.logic().as_str()));
        if ruleset.strict() {
            config.push("strict".into());
        }
        let mut lines = vec![format!("Rule set: {}", config.join(", "))];
        rule_table(self, &mut lines);
        field_table(self, &mut lines);
        if !self.outputs.is_empty() {
            lines.push(format!(
                "Outputs: {}",
                names(self.outputs.iter().map(String::as_str))
            ));
        }
        if !self.tags.is_empty() {
            let shown: Vec<String> = self
                .tags
                .iter()
                .map(|(tag, rules)| {
                    format!(
                        "{} ({})",
                        name(tag),
                        names(rules.iter().map(String::as_str))
                    )
                })
                .collect();
            lines.push(format!("Tags: {}", shown.join("; ")));
        }
        f.write_str(&lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_name_their_units() {
        assert_eq!(duration(0), "0 seconds");
        assert_eq!(duration(1_000_000), "1 second");
        assert_eq!(duration(1_500_000), "1.5 seconds");
        assert_eq!(duration(500), "0.0005 seconds");
        assert_eq!(duration(3_600_000_000), "1 hour");
        assert_eq!(
            duration(93_784_000_000),
            "1 day 2 hours 3 minutes 4 seconds"
        );
        assert_eq!(duration(-60_000_000), "-1 minute");
        assert_eq!(
            duration(i64::MIN),
            "-106751991 days 4 hours 54.775808 seconds"
        );
    }

    #[test]
    fn float_gaps_hide_float_noise() {
        assert_eq!(float_gap(0.7 - 0.65), "0.05");
        assert_eq!(float_gap(2500.0), "2500");
        assert_eq!(float_gap(1e-7), "1e-7");
        assert_eq!(float_gap(1.5e20), "1.5e20");
    }

    #[test]
    fn reasons_name_their_paths() {
        let at = |code, paths: Vec<Vec<Segment>>, detail: Option<&str>| Reason {
            code,
            paths,
            types: Vec::new(),
            detail: detail.map(str::to_owned),
        };
        let x = vec![Segment::Key("x".into())];
        assert_eq!(
            reason(&at(ReasonCode::DecimalOverflow, vec![x.clone()], None)),
            "decimal out of range in x"
        );
        assert_eq!(
            reason(&at(ReasonCode::IntOverflow, vec![], None)),
            "integer overflow"
        );
        assert_eq!(
            reason(&at(ReasonCode::NanResult, vec![], None)),
            "NaN result"
        );
        assert_eq!(reason(&at(ReasonCode::Null, vec![], None)), "null value");
        assert_eq!(
            reason(&at(ReasonCode::Unorderable, vec![], None)),
            "cannot order booleans"
        );
        assert_eq!(
            reason(&at(
                ReasonCode::ReadError,
                vec![x.clone()],
                Some("getter raised")
            )),
            "cannot read x: getter raised"
        );
        assert_eq!(
            reason(&at(ReasonCode::ReadError, vec![vec![]], None)),
            "cannot read the input"
        );
        assert_eq!(
            reason(&at(
                ReasonCode::TypeMismatch,
                vec![x],
                Some("a string is not a number")
            )),
            "type mismatch: a string is not a number"
        );
    }

    #[test]
    fn strings_are_cut_at_sixty_characters() {
        let sixty = "é".repeat(60);
        assert_eq!(bounded(&sixty), format!("\"{sixty}\""));
        let sixty_one = "é".repeat(61);
        assert_eq!(bounded(&sixty_one), format!("\"{}...\"", "é".repeat(57)));
        // The cut counts characters, not escapes.
        let quotes = format!("{}\"{}", "a".repeat(56), "b".repeat(10));
        assert_eq!(bounded(&quotes), format!("\"{}\\\"...\"", "a".repeat(56)));
        assert_eq!(plain("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn error_details_stay_on_one_line() {
        let read = Reason {
            code: ReasonCode::ReadError,
            paths: vec![vec![Segment::Key("x".into())]],
            types: Vec::new(),
            detail: Some(format!("ValueError: line1\nline2 {}", "z".repeat(200))),
        };
        let shown = reason(&read);
        assert!(!shown.contains('\n'), "{shown}");
        assert!(shown.starts_with("cannot read x: ValueError: line1\\nline2 zz"));
        assert!(shown.ends_with("z..."), "{shown}");
        let opaque = Value::Opaque(crate::value::Opaque {
            type_name: "odd\ntype".into(),
            token: 1,
        });
        assert_eq!(value(&opaque), "<odd\\ntype>");
    }

    #[test]
    fn opaque_values_and_odd_gaps() {
        let opaque = Value::Opaque(crate::value::Opaque {
            type_name: "bytes".into(),
            token: 7,
        });
        assert_eq!(value(&opaque), "<bytes>");
        let map: crate::Map = (0..8).map(|i| (format!("k{i}"), Value::Int(i))).collect();
        let shown = value(&Value::Map(map.clone()));
        assert!(
            shown.starts_with("{\"k0\": 0, ") && shown.ends_with(", ...}"),
            "{shown}"
        );
        let deep = Value::List(vec![Value::List(vec![Value::Map(map)])]);
        assert_eq!(value(&deep), "[[{...}]]");
        assert_eq!(gap(&Gap::Number(Value::Bool(true))), "TRUE");
        assert_eq!(gap(&Gap::Days(-2)), "-2 days");
    }

    #[test]
    fn values_are_bounded() {
        let long = Value::String("a".repeat(100));
        assert_eq!(value(&long), format!("\"{}...\"", "a".repeat(57)));
        let list = Value::List((0..10).map(Value::Int).collect());
        assert_eq!(value(&list), "[0, 1, 2, 3, 4, 5, ...]");
        let nested = Value::List(vec![Value::List(vec![Value::List(vec![])])]);
        assert_eq!(value(&nested), "[[[...]]]");
        assert_eq!(value(&Value::Float(8.5)), "8.5e0");
    }
}
