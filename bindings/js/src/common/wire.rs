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
/// A message naming the form that is wrong: a bug in `lib/read.js`, which
/// also counts what it reads against `max_values`. The lists of the form `L`
/// are checked against `max_values` before they are allocated, so that a
/// short text never makes a long list.
pub(crate) fn decode(text: &str, max_values: usize) -> Result<Read, String> {
    // The text is the reader's own: three arrays for each container of the
    // sparse tree, which is as deep as the rule set's longest path (at most
    // its `max_path_segments`) plus a value converted whole (at most 256
    // containers). The parser and the decoder use explicit stacks, so the
    // nesting needs no bound of its own here.
    let mut limits = Limits::default().with_max_size(usize::MAX);
    limits.max_nesting = usize::MAX;
    let json = parse(text, &limits).map_err(|error| error.to_string())?;
    let mut decoder = Decoder {
        errors: Vec::new(),
        at: Vec::new(),
        fresh: FRESH_TOKENS,
        positions: max_values,
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
    /// How many more positions the lists of the form `L` may declare.
    positions: usize,
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

/// A list or map being decoded: what is left of its elements, and what it
/// holds so far.
enum Frame<'j> {
    /// `l`: every element given.
    List {
        elements: std::slice::Iter<'j, Json>,
        list: Vec<Value>,
    },
    /// `L`: a list of a given length, and the elements a path reads.
    Sparse {
        entries: std::slice::Iter<'j, Json>,
        list: Vec<Value>,
        slot: usize,
    },
    /// `O`: a map, in order.
    Map {
        entries: std::slice::Iter<'j, Json>,
        map: Map,
        key: String,
    },
}

/// What a form starts: a value, or a container whose elements come next.
enum Start<'j> {
    Value(Value),
    Container(Frame<'j>),
}

impl<'j> Frame<'j> {
    /// Keep the element just decoded.
    fn place(&mut self, value: Value) {
        match self {
            Frame::List { list, .. } => list.push(value),
            Frame::Sparse { list, slot, .. } => list[*slot] = value,
            Frame::Map { map, key, .. } => map.insert(std::mem::take(key), value),
        }
    }

    /// The next element to decode, with its segment pushed onto `at`; `None`
    /// once every element is decoded.
    fn next(&mut self, at: &mut Vec<Segment>) -> Result<Option<&'j Json>, String> {
        Ok(match self {
            Frame::List { elements, list } => elements.next().inspect(|_| {
                at.push(Segment::Index(list.len() as u64));
            }),
            Frame::Sparse {
                entries,
                list,
                slot,
            } => match entries.next() {
                None => None,
                Some(entry) => {
                    let (index, element) = pair(entry)?;
                    let index: usize = small(Some(index))?;
                    if index >= list.len() {
                        return Err(format!("index {index} is beyond {}", list.len()));
                    }
                    *slot = index;
                    at.push(Segment::Index(index as u64));
                    Some(element)
                }
            },
            Frame::Map { entries, key, .. } => match entries.next() {
                None => None,
                Some(entry) => {
                    let (name, element) = pair(entry)?;
                    text(Some(name))?.clone_into(key);
                    at.push(Segment::Key(key.clone()));
                    Some(element)
                }
            },
        })
    }

    fn finish(self) -> Value {
        match self {
            Frame::List { list, .. } | Frame::Sparse { list, .. } => Value::List(list),
            Frame::Map { map, .. } => Value::Map(map),
        }
    }
}

impl Decoder {
    /// The value of a form, depth first with an explicit stack: the sparse
    /// tree is as deep as the rule set's longest path, and a host thread's
    /// stack (a worker's) may be small.
    fn value<'j>(&mut self, root: &'j Json) -> Result<Value, String> {
        let mut stack: Vec<Frame<'j>> = Vec::new();
        let mut pending = Some(root);
        loop {
            let mut done = match pending.take() {
                Some(json) => match self.start(json)? {
                    Start::Value(value) => Some(value),
                    Start::Container(frame) => {
                        stack.push(frame);
                        None
                    }
                },
                None => None,
            };
            // Give the value to its container, then find the container's next
            // element, closing the containers that have none left.
            loop {
                let Some(top) = stack.last_mut() else {
                    return done.ok_or_else(|| "nothing was decoded".to_owned());
                };
                if let Some(value) = done.take() {
                    top.place(value);
                    self.at.pop();
                }
                if let Some(json) = top.next(&mut self.at)? {
                    pending = Some(json);
                    break;
                }
                done = stack.pop().map(Frame::finish);
            }
        }
    }

    /// A scalar value, or a container to fill.
    fn start<'j>(&mut self, json: &'j Json) -> Result<Start<'j>, String> {
        Ok(Start::Value(match json {
            Json::Null => Value::Null(NullCause::Null),
            Json::Bool(boolean) => Value::Bool(*boolean),
            Json::String(text) => Value::String(text.clone()),
            Json::Array(form) => return self.form(form),
            Json::Number(number) => return Err(format!("a bare number {}", number.text())),
            Json::Object(_) => return Err("an object".to_owned()),
        }))
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

    fn form<'j>(&mut self, form: &'j [Json]) -> Result<Start<'j>, String> {
        let tag = text(form.first())?;
        let arg = |i: usize| form.get(i);
        Ok(Start::Value(match tag {
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
                Err(_) => self.opaque("Decimal", arg(2)),
            },
            "D" | "N" | "A" => self.datetime(tag, form)?,
            "l" => {
                let elements = items(arg(1))?;
                return Ok(Start::Container(Frame::List {
                    list: Vec::with_capacity(elements.len()),
                    elements: elements.iter(),
                }));
            }
            "L" => {
                let len: usize = small(arg(1))?;
                self.positions = self
                    .positions
                    .checked_sub(len)
                    .ok_or_else(|| format!("a list of {len} is beyond max_values"))?;
                // The list keeps its length; holes stand for elements no path reads.
                return Ok(Start::Container(Frame::Sparse {
                    entries: items(arg(2))?.iter(),
                    list: vec![Value::Null(NullCause::Null); len],
                    slot: 0,
                }));
            }
            // A hole or an `undefined` element: null, as a list cannot hold a
            // missing element (`spec/data-model.md`, *Lists and maps*).
            "M" => Value::Null(NullCause::Null),
            "O" => {
                return Ok(Start::Container(Frame::Map {
                    entries: items(arg(1))?.iter(),
                    map: Map::new(),
                    key: String::new(),
                }));
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
        }))
    }
}
