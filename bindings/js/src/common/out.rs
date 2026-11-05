//! The encoding in which values, decisions and explanations come back to
//! JavaScript (`spec/data-model.md`, *JavaScript and TypeScript*, *Values
//! given back*), decoded by `lib/output.js`.
//!
//! Like the reader's encoding (`wire`), it is one JSON text of nested arrays,
//! private to the binding, so that both builds share it and JavaScript builds
//! its values in one pass. Every value takes one of these forms:
//!
//! | Form | Value |
//! |---|---|
//! | `null`, `true`, `false`, a string | null (any reason), a boolean, a string |
//! | `["i", "<digits>"]` | int |
//! | `["f", "<text>"]` | float: the shortest digits that read back, `"Infinity"`, `"-Infinity"`, `"-0e0"` |
//! | `["d", "<canonical text>"]` | decimal (`spec/json-format.md`, *Numbers*) |
//! | `["D", y, m, d]` | date |
//! | `["N", y, m, d, h, mi, s, us]` | naive datetime |
//! | `["A", "<text>", y, m, d, h, mi, s, us, <offset s>, <zone or null>, "<epoch µs>"]` | aware datetime: its text, local fields, offset, zone and instant |
//! | `["l", [items]]` | list |
//! | `["O", [[key, item], ...]]` | map, in order |
//! | `["X", "<type>", "<token>"]` | opaque |
//!
//! A result tree (`tree`) is `["t", truth, reason, children]` for a node with
//! a truth value and `["v", value, reason, children]` for any other, with its
//! nodes apart, in pre-order.

use cleanrules::ast::Node;
use cleanrules::eval::{Evaluated, Outcome, Reason};
use cleanrules::explain::{Cause, Explanation, Gap, GroupKind, Requirement};
use cleanrules::format::{write_node, write_path, write_value};
use cleanrules::jiff::civil::DateTime;
use cleanrules::jiff::tz::TimeZone;
use cleanrules::json::{self as json, Json, JsonNumber, to_string};
use cleanrules::logic::Truth;
use cleanrules::value::{AwareDateTime, Zone};
use cleanrules::{Map, Segment, Value};

fn number(value: impl ToString) -> Json {
    Json::Number(JsonNumber::from_literal(value.to_string()))
}

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

fn tagged(tag: &str, mut rest: Vec<Json>) -> Json {
    rest.insert(0, string(tag));
    Json::Array(rest)
}

fn nullable<T>(value: Option<T>, form: impl FnOnce(T) -> Json) -> Json {
    value.map_or(Json::Null, form)
}

/// A float's text for `Number()`: the shortest digits that read back as the
/// same float, `-0` included, or an infinity by name.
fn float_text(value: f64) -> String {
    if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else {
        format!("{value:e}")
    }
}

fn civil(datetime: DateTime) -> Vec<Json> {
    vec![
        number(datetime.year()),
        number(datetime.month()),
        number(datetime.day()),
        number(datetime.hour()),
        number(datetime.minute()),
        number(datetime.second()),
        number(datetime.subsec_nanosecond() / 1000),
    ]
}

/// An aware datetime: its text, its local date and time, its offset at that
/// instant, its zone's name (or `null` for a fixed offset) and the instant.
fn aware(value: &AwareDateTime) -> Json {
    let instant = value.timestamp();
    let (zone, name) = match value.zone() {
        Zone::Fixed(offset) => (TimeZone::fixed(*offset), None),
        // A named zone was checked against the bundled database when the
        // value was made; should the lookup fail, UTC still gives the instant.
        Zone::Named(name) => (
            cleanrules::jiff::tz::db()
                .get(name)
                .unwrap_or(TimeZone::UTC),
            Some(name.as_str()),
        ),
    };
    let offset = zone.to_offset(instant);
    // The JSON format's canonical text (RFC 9557), `{"$datetime": "..."}`.
    let written = write_value(&Value::AwareDateTime(value.clone()));
    let text = match &written {
        Json::Object(members) => members.iter().find_map(|(_, text)| match text {
            Json::String(text) => Some(text.clone()),
            _ => None,
        }),
        _ => None,
    }
    .unwrap_or_default();
    let mut form = vec![Json::String(text)];
    form.extend(civil(instant.to_zoned(zone).datetime()));
    form.push(number(offset.seconds()));
    form.push(nullable(name, string));
    form.push(string(&instant.as_microsecond().to_string()));
    tagged("A", form)
}

/// A value in the encoding.
pub(crate) fn value(value: &Value) -> Json {
    match value {
        Value::Null(_) => Json::Null,
        Value::Bool(boolean) => Json::Bool(*boolean),
        Value::Int(int) => tagged("i", vec![string(&int.to_string())]),
        Value::Float(float) => tagged("f", vec![string(&float_text(*float))]),
        Value::Decimal(decimal) => tagged("d", vec![string(&decimal.to_string())]),
        Value::String(text) => string(text),
        Value::Date(date) => tagged(
            "D",
            vec![
                number(date.year()),
                number(date.month()),
                number(date.day()),
            ],
        ),
        Value::NaiveDateTime(datetime) => tagged("N", civil(*datetime)),
        Value::AwareDateTime(datetime) => aware(datetime),
        Value::List(items) => tagged(
            "l",
            vec![Json::Array(items.iter().map(self::value).collect())],
        ),
        Value::Map(map) => tagged("O", vec![entries(map.iter())]),
        Value::Opaque(opaque) => tagged(
            "X",
            vec![string(&opaque.type_name), string(&opaque.token.to_string())],
        ),
    }
}

fn entries<'a>(pairs: impl Iterator<Item = (&'a str, &'a Value)>) -> Json {
    Json::Array(
        pairs
            .map(|(key, item)| Json::Array(vec![string(key), value(item)]))
            .collect(),
    )
}

/// A payload or output, given as pairs, as a map form.
pub(crate) fn pairs(pairs: &[(String, Value)]) -> Json {
    tagged(
        "O",
        vec![entries(
            pairs.iter().map(|(key, item)| (key.as_str(), item)),
        )],
    )
}

/// A map as a map form.
pub(crate) fn map(map: &Map) -> Json {
    tagged("O", vec![entries(map.iter())])
}

/// An int, as `["i", "<digits>"]`.
pub(crate) fn int(value: i64) -> Json {
    tagged("i", vec![string(&value.to_string())])
}

/// A truth value's name.
pub(crate) fn truth_name(truth: Truth) -> &'static str {
    match truth {
        Truth::True => "true",
        Truth::False => "false",
        Truth::Unknown => "unknown",
    }
}

fn path(path: &[Segment]) -> Json {
    write_path(path)
}

fn gap(gap: &Gap) -> Json {
    match gap {
        Gap::Number(number) => value(number),
        Gap::Days(days) => tagged("days", vec![string(&days.to_string())]),
        Gap::Microseconds(micros) => tagged("us", vec![string(&micros.to_string())]),
    }
}

/// Why a result is unknown or null: `[code, paths, types, detail]`.
fn reason(reason: &Reason) -> Json {
    Json::Array(vec![
        string(reason.code.as_str()),
        Json::Array(reason.paths.iter().map(|p| path(p)).collect()),
        Json::Array(reason.types.iter().map(|g| string(g.as_str())).collect()),
        nullable(reason.detail.as_deref(), string),
    ])
}

/// A result tree (`spec/json-format.md`, *Result trees*): `["t", truth,
/// reason, children]` or `["v", value, reason, children]`; its nodes are
/// pushed to `nodes` in pre-order, so that JavaScript wraps them as they are.
pub(crate) fn tree(tree: &Evaluated<'_, Truth>, nodes: &mut Vec<Node>) -> Json {
    nodes.push(Node::clone(&tree.node));
    let (tag, outcome) = match &tree.outcome {
        Outcome::Truth(truth) => ("t", string(truth_name(*truth))),
        Outcome::Value(found) => ("v", value(found)),
    };
    let children = tree
        .children
        .iter()
        .map(|child| self::tree(child, nodes))
        .collect();
    tagged(
        tag,
        vec![
            outcome,
            nullable(tree.reason.as_ref(), reason),
            Json::Array(children),
        ],
    )
}

/// A cause: `["c", node, truth, negated, holds, values, reason, counterfactual,
/// text]`, the node as its JSON text: nested, `JSON.parse` would turn a float
/// constant such as `0.0` into the int `0`.
fn cause(cause: &Cause<'_, '_>) -> Json {
    let values = cause
        .values
        .iter()
        .map(|(at, read)| Json::Array(vec![path(at), value(read)]))
        .collect();
    let reason = nullable(cause.reason(), reason);
    let counterfactual = nullable(cause.counterfactual.as_ref(), |counterfactual| {
        Json::Array(vec![
            path(&counterfactual.path),
            string(counterfactual.op.tag()),
            value(&counterfactual.target),
            nullable(counterfactual.gap.as_ref(), gap),
        ])
    });
    tagged(
        "c",
        vec![
            string(&to_string(&write_node(&cause.result.node))),
            string(truth_name(cause.truth())),
            Json::Bool(cause.negated),
            Json::Bool(cause.holds()),
            Json::Array(values),
            reason,
            counterfactual,
            string(&cause.to_string()),
        ],
    )
}

/// A requirement: a cause, or `["all" | "any", [requirements]]`.
fn requirement(requirement: &Requirement<'_, '_>) -> Json {
    match requirement {
        Requirement::Cause(found) => cause(found),
        Requirement::Group(kind, items) => tagged(
            if *kind == GroupKind::All {
                "all"
            } else {
                "any"
            },
            vec![Json::Array(items.iter().map(self::requirement).collect())],
        ),
    }
}

/// An explanation: `[text, [rule, ...]]`, each rule `[name, status, note,
/// achievable, requirement, text]`, a note `[kind, winner, priority,
/// winner priority]` (priorities as ints, or `null`).
pub(crate) fn explanation(explanation: &Explanation<'_, '_>) -> String {
    let rules = explanation
        .rules
        .iter()
        .map(|entry| {
            let note = nullable(entry.note.as_ref(), |note| {
                let (priority, winner) = note.priorities.unzip();
                Json::Array(vec![
                    string(note.kind.as_str()),
                    string(&note.winner),
                    nullable(priority, int),
                    nullable(winner, int),
                ])
            });
            Json::Array(vec![
                string(entry.rule.name()),
                string(entry.status.as_str()),
                note,
                nullable(entry.achievable, Json::Bool),
                requirement(&entry.requirement),
                string(&entry.to_string()),
            ])
        })
        .collect();
    to_string(&Json::Array(vec![
        string(&explanation.to_string()),
        Json::Array(rules),
    ]))
}

/// A document the core wrote, made to keep its meaning through `JSON.parse`
/// and `JSON.stringify`, for `toJSON()`: each float literal that holds an
/// integer becomes `{"$float": <literal>}`, the form `spec/json-format.md`
/// (*Values*) asks of writers that cannot write `1.0` (JavaScript writes the
/// number `1.0` as `1`, an int). The canonical form already writes ints beyond
/// ±(2⁵³ − 1) as `{"$int": "…"}`, infinities as `{"$float": "inf"}` and maps
/// with `$` keys inside `{"$map": …}`, which `JSON.parse` keeps. In version 1
/// of the format a number literal with a fraction or an exponent is always a
/// float value (truth values are booleans; graded truths arrive in phase J),
/// so the class of each literal, which the core's writer chose, says which
/// numbers are floats. A float zero keeps its class but not its sign:
/// `JSON.stringify(-0)` writes `0`.
pub(crate) fn stringifiable(document: &mut Json) {
    let mut pending = vec![document];
    while let Some(value) = pending.pop() {
        let literal = match &*value {
            Json::Number(n) if !n.is_integer_literal() => {
                let x = n.as_f64();
                (x.is_finite() && x.fract() == 0.0).then(|| n.clone())
            }
            _ => None,
        };
        if let Some(literal) = literal {
            *value = Json::Object(vec![("$float".to_owned(), Json::Number(literal))]);
            continue;
        }
        match value {
            Json::Array(items) => pending.extend(items.iter_mut()),
            Json::Object(members) => pending.extend(members.iter_mut().map(|(_, item)| item)),
            _ => {}
        }
    }
}

/// `stringifiable` on the text of a document the core wrote.
pub(crate) fn stringifiable_text(text: &str) -> String {
    let limits = json::Limits {
        max_size: text.len(),
        max_nesting: usize::MAX,
        ..json::Limits::default()
    };
    // The core wrote the text, so it reads back; were it not to, it is
    // returned as it is.
    match json::parse(text, &limits) {
        Ok(mut document) => {
            stringifiable(&mut document);
            to_string(&document)
        }
        Err(_) => text.to_owned(),
    }
}
