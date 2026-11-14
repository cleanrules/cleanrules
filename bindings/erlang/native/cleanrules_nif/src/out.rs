//! Values given back to Erlang (`spec/data-model.md`, *Erlang*, *Values given
//! back*): null as the atom `null`, decimals, infinities and aware datetimes as
//! tagged tuples, dates and naive datetimes as `calendar` tuples, maps with
//! binary keys, and an opaque value as the term it was read from.
//!
//! Values nest as deep as the readers' limits allow, so they are built with an
//! explicit stack, never recursively (a scheduler thread's stack is small).
//!
//! Documents that are not values (a summary, the node of an error) come back
//! as their JSON form as a term ([`json_term`]).

use cleanrules::jiff::civil::DateTime;
use cleanrules::jiff::tz::TimeZone;
use cleanrules::json::Json;
use cleanrules::logic::Truth;
use cleanrules::rules::{Aggregated, Decision};
use cleanrules::value::{AwareDateTime, Zone};
use cleanrules::{Map, Value};
use rustler::{Encoder, Env, Term};

use crate::atoms;
use crate::errors::{Failure, Outcome, error_term, ok};

/// The term of `value`; `objects` holds the terms of its opaque values, by
/// token (an opaque value without one, from a document, is `null`).
///
/// # Errors
///
/// [`Failure::Internal`] for an aware datetime in a zone the bundled database
/// does not know: the core only builds them in zones it knows.
pub(crate) fn value_term<'a>(
    env: Env<'a>,
    value: &Value,
    objects: &[Term<'a>],
) -> Outcome<Term<'a>> {
    enum Task<'v> {
        Visit(&'v Value),
        List(usize),
        Map(&'v Map),
    }
    let mut tasks = vec![Task::Visit(value)];
    let mut done: Vec<Term<'a>> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(Value::List(items)) => {
                tasks.push(Task::List(items.len()));
                tasks.extend(items.iter().rev().map(Task::Visit));
            }
            Task::Visit(Value::Map(map)) => {
                tasks.push(Task::Map(map));
                let values: Vec<&Value> = map.iter().map(|(_, v)| v).collect();
                tasks.extend(values.into_iter().rev().map(Task::Visit));
            }
            Task::Visit(scalar) => done.push(scalar_term(env, scalar, objects)?),
            Task::List(len) => {
                let items = done.split_off(done.len() - len);
                done.push(items.encode(env));
            }
            Task::Map(map) => {
                let values = done.split_off(done.len() - map.len());
                let keys: Vec<Term<'a>> = map.iter().map(|(k, _)| k.encode(env)).collect();
                done.push(map_term(env, &keys, &values));
            }
        }
    }
    Ok(done.pop().unwrap_or_else(|| atoms::null().encode(env)))
}

/// A JSON document's content as a term, with an explicit stack: objects as
/// maps with binary keys, arrays as lists, strings as binaries, numbers as
/// integers or floats, `null` as the atom `null`, and `{"$int": Text}` (an
/// int beyond 2^53, `spec/json-format.md`, *Values*) as an integer.
pub(crate) fn json_term<'a>(env: Env<'a>, json: &Json) -> Term<'a> {
    enum Task<'j> {
        Visit(&'j Json),
        List(usize),
        Map(Vec<&'j str>),
    }
    let mut tasks = vec![Task::Visit(json)];
    let mut done: Vec<Term<'a>> = Vec::new();
    while let Some(task) = tasks.pop() {
        match task {
            Task::Visit(Json::Object(members)) => match &members[..] {
                [(tag, Json::String(text))] if tag == "$int" && text.parse::<i64>().is_ok() => {
                    done.push(text.parse::<i64>().unwrap_or_default().encode(env));
                }
                _ => {
                    tasks.push(Task::Map(members.iter().map(|(k, _)| k.as_str()).collect()));
                    tasks.extend(members.iter().rev().map(|(_, v)| Task::Visit(v)));
                }
            },
            Task::Visit(Json::Array(items)) => {
                tasks.push(Task::List(items.len()));
                tasks.extend(items.iter().rev().map(Task::Visit));
            }
            Task::Visit(Json::Null) => done.push(atoms::null().encode(env)),
            Task::Visit(Json::Bool(b)) => done.push(b.encode(env)),
            Task::Visit(Json::String(text)) => done.push(text.encode(env)),
            Task::Visit(Json::Number(number)) => {
                done.push(match number.as_i128().and_then(|i| i64::try_from(i).ok()) {
                    Some(int) => int.encode(env),
                    None => number
                        .text()
                        .parse::<f64>()
                        .map_or_else(|_| number.text().encode(env), |f| f.encode(env)),
                });
            }
            Task::List(len) => {
                let items = done.split_off(done.len() - len);
                done.push(items.encode(env));
            }
            Task::Map(keys) => {
                let values = done.split_off(done.len() - keys.len());
                let keys: Vec<Term<'a>> = keys.iter().map(|k| k.encode(env)).collect();
                done.push(map_term(env, &keys, &values));
            }
        }
    }
    done.pop().unwrap_or_else(|| atoms::null().encode(env))
}

/// A map of distinct keys, or, should two be equal, the last one's value.
pub(crate) fn map_term<'a>(env: Env<'a>, keys: &[Term<'a>], values: &[Term<'a>]) -> Term<'a> {
    Term::map_from_term_arrays(env, keys, values).unwrap_or_else(|_| {
        keys.iter()
            .zip(values)
            .fold(Term::map_new(env), |map, (k, v)| {
                map.map_put(*k, *v).unwrap_or(map)
            })
    })
}

fn date_term(env: Env<'_>, date: cleanrules::jiff::civil::Date) -> Term<'_> {
    (
        i64::from(date.year()),
        i64::from(date.month()),
        i64::from(date.day()),
    )
        .encode(env)
}

/// `{{Y, Mo, D}, {H, Mi, S}}`, with `Micro` when it is not 0.
fn naive_term(env: Env<'_>, datetime: DateTime) -> Term<'_> {
    let (h, mi, s) = (
        i64::from(datetime.hour()),
        i64::from(datetime.minute()),
        i64::from(datetime.second()),
    );
    let micro = i64::from(datetime.subsec_nanosecond() / 1000);
    let time = if micro == 0 {
        (h, mi, s).encode(env)
    } else {
        (h, mi, s, micro).encode(env)
    };
    (date_term(env, datetime.date()), time).encode(env)
}

/// `{cleanrules_datetime, Local, Offset, Zone}`.
fn aware_term<'a>(env: Env<'a>, aware: &AwareDateTime) -> Outcome<Term<'a>> {
    let instant = aware.timestamp();
    let (zoned, name) = match aware.zone() {
        Zone::Fixed(offset) => (instant.to_zoned(TimeZone::fixed(*offset)), None),
        Zone::Named(name) => match cleanrules::jiff::tz::db().get(name) {
            Ok(zone) => (instant.to_zoned(zone), Some(name.as_str())),
            // The core keeps a zone only when its database knows it.
            Err(_) => {
                return Err(Failure::Internal(format!(
                    "cleanrules failed (the time zone {name} is not in the bundled database); \
                     please report it"
                )));
            }
        },
    };
    let zone = match name {
        Some(name) => name.encode(env),
        None => atoms::undefined().encode(env),
    };
    Ok((
        atoms::cleanrules_datetime(),
        naive_term(env, zoned.datetime()),
        i64::from(zoned.offset().seconds()),
        zone,
    )
        .encode(env))
}

fn scalar_term<'a>(env: Env<'a>, value: &Value, objects: &[Term<'a>]) -> Outcome<Term<'a>> {
    Ok(match value {
        Value::Bool(b) => b.encode(env),
        Value::Int(i) => i.encode(env),
        Value::Float(f) if f.is_infinite() => {
            let text = if *f > 0.0 { "inf" } else { "-inf" };
            (atoms::cleanrules_float(), text).encode(env)
        }
        Value::Float(f) => f.encode(env),
        Value::Decimal(d) => (atoms::cleanrules_decimal(), d.to_string()).encode(env),
        Value::String(s) => s.encode(env),
        Value::Date(d) => date_term(env, *d),
        Value::NaiveDateTime(dt) => naive_term(env, *dt),
        Value::AwareDateTime(a) => aware_term(env, a)?,
        Value::Opaque(o) => usize::try_from(o.token)
            .ok()
            .and_then(|token| objects.get(token).copied())
            .unwrap_or_else(|| atoms::null().encode(env)),
        // Lists and maps are built by `value_term`; null whatever its reason.
        Value::Null(_) | Value::List(_) | Value::Map(_) => atoms::null().encode(env),
    })
}

fn truth_atom(truth: Option<Truth>) -> rustler::Atom {
    match truth {
        Some(Truth::True) => atoms::true_(),
        Some(Truth::False) => atoms::false_(),
        _ => atoms::unknown(),
    }
}

fn pairs_term<'a>(
    env: Env<'a>,
    pairs: Vec<(String, Value)>,
    objects: &[Term<'a>],
) -> Outcome<Term<'a>> {
    value_term(env, &Value::Map(pairs.into_iter().collect()), objects)
}

/// A decision as a map (`README.md`, *The NIF*):
///
/// ```erlang
/// #{results := [#{rule := binary(), fired := boolean(),
///                 truth := true | false | unknown, output => map()}],
///   selected := [binary()],
///   output := {ok, map() | null} | {error, map()},
///   aggregated => map() | null}
/// ```
///
/// A rule's `output` is there when its payload was evaluated; `aggregated`
/// when the rule set has an aggregate.
///
/// # Errors
///
/// As [`value_term`].
pub(crate) fn decision_term<'a>(
    env: Env<'a>,
    decision: &Decision<'_>,
    objects: &[Term<'a>],
) -> Outcome<Term<'a>> {
    let mut results: Vec<Term<'a>> = Vec::new();
    for result in decision.results() {
        let mut keys = vec![
            atoms::rule().encode(env),
            atoms::fired().encode(env),
            atoms::truth().encode(env),
        ];
        let mut values = vec![
            result.rule.name().encode(env),
            result.fired.encode(env),
            truth_atom(result.when.truth()).encode(env),
        ];
        if result.then.is_some() {
            keys.push(atoms::output().encode(env));
            values.push(pairs_term(env, result.output(), objects)?);
        }
        results.push(map_term(env, &keys, &values));
    }
    let selected: Vec<&str> = decision.selected().map(|r| r.rule.name()).collect();
    let output = match decision.output() {
        Ok(Some(value)) => ok(env, value_term(env, &value, objects)?),
        Ok(None) => ok(env, atoms::null().encode(env)),
        Err(error) => (atoms::error(), error_term(env, &error)).encode(env),
    };
    let mut keys = vec![
        atoms::results().encode(env),
        atoms::selected().encode(env),
        atoms::output().encode(env),
    ];
    let mut values = vec![results.encode(env), selected.encode(env), output];
    if decision.ruleset().aggregate().is_some() {
        keys.push(atoms::aggregated().encode(env));
        values.push(match decision.aggregated() {
            Some(Aggregated::Values(map)) => value_term(env, &Value::Map(map.clone()), objects)?,
            _ => atoms::null().encode(env),
        });
    }
    Ok(map_term(env, &keys, &values))
}
