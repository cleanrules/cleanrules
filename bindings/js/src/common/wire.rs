//! The encoding in which `lib/read.js` hands over the sparse value tree of an
//! input (`spec/data-model.md`, *Reading native data*).
//!
//! The reader walks the JavaScript data, where property access is fast, and
//! writes what it found as one JSON text (`JSON.stringify` of nested arrays),
//! which this module turns into a core `Value`. The encoding is private to the
//! binding and may change with it; `lib/read.js` documents each form:
//!
//! | Form | Value |
//! |---|---|
//! | `null`, `true`, `false`, a string | null, a boolean, a string |
//! | `["i", "<digits>"]` | int |
//! | `["f", "<shortest repr>"]` | float (`-0` and infinities included) |
//! | `["d", "<literal>", k]` | decimal, or null with `decimal_overflow`; opaque if malformed |
//! | `["D", y, m, d, k]` | date |
//! | `["N", y, m, d, h, mi, s, us, k]` | naive datetime |
//! | `["A", "<epoch µs>", <offset s>, <zone or null>, k]` | aware datetime |
//! | `["l", [items]]` | list, every element given |
//! | `["L", len, [[i, item], ...]]` | list of `len`, holes where no path reads |
//! | `["M"]` | a hole or an `undefined` element: null |
//! | `["O", [[key, item], ...]]` | map, in order |
//! | `["X", "<type>", token]` | opaque |
//! | `["E", "<detail>", k]` | null with `read_error`; `k` is the thrown value's index |
//! | `["IO"]`, `["DO"]` | null with `int_overflow`, `decimal_overflow` |
//!
//! `k`, optional in the scalar forms, is the object's token: a scalar the core
//! refuses (a forged marker, an instant beyond the core's range) is opaque
//! with it, so that the object comes back. Without it the opaque value gets a
//! token of its own.

use cleanrules::jiff::Timestamp;
use cleanrules::jiff::civil::{Date, DateTime, Time};
use cleanrules::jiff::tz::Offset;
use cleanrules::json::{Json, Limits, parse};
use cleanrules::value::{AwareDateTime, DecimalError, Opaque};
use cleanrules::{Decimal, Map, NullCause, Segment, Value};

/// An input as read: its value, and where each `read_error` was met with the
/// index of the value thrown there.
pub(crate) struct Read {
    pub(crate) value: Value,
    pub(crate) errors: Vec<(Vec<Segment>, u32)>,
}

/// Tokens for values the core refuses that came without the reader's token
/// (the scalar forms carry one; this is a fallback): above every token the
/// reader hands out, which are positions in its own list.
const FRESH_TOKENS: u64 = 1 << 53;

/// Decode the reader's text.
///
/// # Errors
///
/// A message naming the form that is wrong: a bug in `lib/read.js`.
pub(crate) fn decode(text: &str) -> Result<Read, String> {
    // The reader nests at most 256 containers, three arrays each.
    let limits = Limits {
        max_nesting: 1024,
        ..Limits::default()
    }
    .with_max_size(usize::MAX);
    let json = parse(text, &limits).map_err(|error| error.to_string())?;
    let mut decoder = Decoder {
        errors: Vec::new(),
        at: Vec::new(),
        fresh: FRESH_TOKENS,
    };
    let value = decoder.value(&json)?;
    Ok(Read {
        value,
        errors: decoder.errors,
    })
}

struct Decoder {
    errors: Vec<(Vec<Segment>, u32)>,
    at: Vec<Segment>,
    fresh: u64,
}

fn integer(json: Option<&Json>) -> Result<i128, String> {
    match json {
        Some(Json::Number(number)) => number
            .as_i128()
            .ok_or_else(|| format!("{} is not an integer", number.text())),
        Some(Json::String(text)) => text
            .parse::<i128>()
            .map_err(|_| format!("{text:?} is not an integer")),
        _ => Err("an integer is missing".to_owned()),
    }
}

fn small<T: TryFrom<i128>>(json: Option<&Json>) -> Result<T, String> {
    let number = integer(json)?;
    T::try_from(number).map_err(|_| format!("{number} is out of range"))
}

fn text(json: Option<&Json>) -> Result<&str, String> {
    match json {
        Some(Json::String(text)) => Ok(text),
        _ => Err("a string is missing".to_owned()),
    }
}

fn items(json: Option<&Json>) -> Result<&[Json], String> {
    match json {
        Some(Json::Array(items)) => Ok(items),
        _ => Err("an array is missing".to_owned()),
    }
}

fn pair(json: &Json) -> Result<(&Json, &Json), String> {
    match json {
        Json::Array(items) if items.len() == 2 => Ok((&items[0], &items[1])),
        _ => Err("an entry is not a pair".to_owned()),
    }
}

impl Decoder {
    fn value(&mut self, json: &Json) -> Result<Value, String> {
        Ok(match json {
            Json::Null => Value::Null(NullCause::Null),
            Json::Bool(boolean) => Value::Bool(*boolean),
            Json::String(text) => Value::String(text.clone()),
            Json::Array(form) => self.form(form)?,
            Json::Number(number) => return Err(format!("a bare number {}", number.text())),
            Json::Object(_) => return Err("an object".to_owned()),
        })
    }

    /// An opaque value: with `token` (the reader's, for the native object),
    /// or with a token of its own.
    fn opaque(&mut self, type_name: &str, token: Option<&Json>) -> Value {
        let token = if let Some(token) = token.and_then(|json| integer(Some(json)).ok()) {
            u64::try_from(token).unwrap_or(u64::MAX)
        } else {
            self.fresh += 1;
            self.fresh
        };
        Value::Opaque(Opaque {
            type_name: type_name.to_owned(),
            token,
        })
    }

    /// A date (`D`), a naive datetime (`N`) or an aware datetime (`A`); one
    /// the core cannot hold is opaque.
    fn datetime(&mut self, tag: &str, form: &[Json]) -> Result<Value, String> {
        let arg = |i: usize| form.get(i);
        Ok(match tag {
            "D" => {
                let year: i16 = small(arg(1))?;
                match Date::new(year, small(arg(2))?, small(arg(3))?) {
                    Ok(date) if (1..=9999).contains(&year) => Value::Date(date),
                    _ => self.opaque("date", arg(4)),
                }
            }
            "N" => {
                let year: i16 = small(arg(1))?;
                let date = Date::new(year, small(arg(2))?, small(arg(3))?);
                let micros: i32 = small(arg(7))?;
                let time = Time::new(
                    small(arg(4))?,
                    small(arg(5))?,
                    small(arg(6))?,
                    micros.saturating_mul(1000),
                );
                match (date, time) {
                    (Ok(date), Ok(time)) if (1..=9999).contains(&year) => {
                        Value::NaiveDateTime(DateTime::from_parts(date, time))
                    }
                    _ => self.opaque("naive datetime", arg(8)),
                }
            }
            "A" => {
                let micros: i64 = small(arg(1))?;
                let offset: i32 = small(arg(2))?;
                let zone = match arg(3) {
                    Some(Json::String(name)) => Some(name.as_str()),
                    _ => None,
                };
                let aware = Timestamp::from_microsecond(micros)
                    .ok()
                    .zip(Offset::from_seconds(offset).ok())
                    .and_then(|(instant, offset)| {
                        AwareDateTime::from_native(instant, offset, zone).ok()
                    });
                match aware {
                    Some(aware) => Value::AwareDateTime(aware),
                    None => self.opaque("aware datetime", arg(4)),
                }
            }
            _ => return Err(format!("{tag:?} is not a date or datetime")),
        })
    }

    fn form(&mut self, form: &[Json]) -> Result<Value, String> {
        let tag = text(form.first())?;
        let arg = |i: usize| form.get(i);
        Ok(match tag {
            "i" => Value::Int(small(arg(1))?),
            "f" => {
                let repr = text(arg(1))?;
                let float = repr
                    .parse::<f64>()
                    .map_err(|_| format!("{repr:?} is not a float"))?;
                Value::float(float)
            }
            "d" => match Decimal::parse(text(arg(1))?) {
                Ok(decimal) => Value::Decimal(decimal),
                Err(DecimalError::DoesNotFit) => Value::Null(NullCause::DecimalOverflow),
                // A marker of another copy of the package, or a forged one.
                Err(DecimalError::Malformed) => self.opaque("Decimal", arg(2)),
            },
            "D" | "N" | "A" => self.datetime(tag, form)?,
            "l" => {
                let elements = items(arg(1))?;
                let mut list = Vec::with_capacity(elements.len());
                for (index, element) in elements.iter().enumerate() {
                    self.at.push(Segment::Index(index as u64));
                    list.push(self.value(element)?);
                    self.at.pop();
                }
                Value::List(list)
            }
            "L" => {
                let len: usize = small(arg(1))?;
                // The list keeps its length; holes stand for elements no path reads.
                let mut list = vec![Value::Null(NullCause::Null); len];
                for entry in items(arg(2))? {
                    let (index, element) = pair(entry)?;
                    let index: usize = small(Some(index))?;
                    let slot = list
                        .get_mut(index)
                        .ok_or_else(|| format!("index {index} is beyond {len}"))?;
                    self.at.push(Segment::Index(index as u64));
                    *slot = self.value(element)?;
                    self.at.pop();
                }
                Value::List(list)
            }
            // A hole or an `undefined` element: null, as a list cannot hold a
            // missing element (`spec/data-model.md`, *Lists and maps*).
            "M" => Value::Null(NullCause::Null),
            "O" => {
                let mut map = Map::new();
                for entry in items(arg(1))? {
                    let (key, element) = pair(entry)?;
                    let key = text(Some(key))?.to_owned();
                    self.at.push(Segment::Key(key.clone()));
                    let element = self.value(element)?;
                    self.at.pop();
                    map.insert(key, element);
                }
                Value::Map(map)
            }
            "X" => Value::Opaque(Opaque {
                type_name: text(arg(1))?.to_owned(),
                token: small(arg(2))?,
            }),
            "E" => {
                let detail = text(arg(1))?.to_owned();
                self.errors.push((self.at.clone(), small(arg(2))?));
                Value::Null(NullCause::ReadError(detail))
            }
            "IO" => Value::Null(NullCause::IntOverflow),
            "DO" => Value::Null(NullCause::DecimalOverflow),
            other => return Err(format!("an unknown form {other:?}")),
        })
    }
}
