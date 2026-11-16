//! Values: the data that conditions read and constants hold (`spec/data-model.md`).

mod bignum;
mod datetime;
mod decimal;
mod number;

use std::cmp::Ordering;
use std::fmt;

pub use datetime::{AwareDateTime, DateTimeError, Zone, check_date, check_naive};
pub use decimal::{ArithmeticError, Decimal, DecimalError, MAX_DIGITS, RoundingMode};
use jiff::civil::{Date, DateTime};
pub use number::Number;
pub(crate) use number::{ExactSum, float_literal, shortest_digits};

use crate::error::{Error, ErrorCode};
use crate::walk::post_order;

/// A value of the data model.
///
/// Floats are never NaN: build them with [`Value::float`], which turns NaN into
/// null, as the spec requires of data.
#[derive(Debug)]
pub enum Value {
    /// Nothing, with the reason it is null.
    Null(NullCause),
    /// `true` or `false`.
    Bool(bool),
    /// A signed 64-bit integer.
    Int(i64),
    /// A binary64 float, never NaN.
    Float(f64),
    /// An exact decimal.
    Decimal(Decimal),
    /// A sequence of Unicode scalar values.
    String(String),
    /// A day, years 1 to 9999.
    Date(Date),
    /// A date and a time of day, to the microsecond.
    NaiveDateTime(DateTime),
    /// An instant with an offset or a time zone.
    AwareDateTime(AwareDateTime),
    /// An ordered sequence of values.
    List(Vec<Value>),
    /// String keys mapped to values, without order (given back by key).
    Map(Map),
    /// A native value with no equivalent.
    Opaque(Opaque),
}

/// Why a value is null (`spec/data-model.md`, *Nulls with a reason*).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NullCause {
    /// A native null or a NaN.
    Null,
    /// A native integer outside the signed 64-bit range.
    IntOverflow,
    /// A native decimal that does not fit in 38 digits.
    DecimalOverflow,
    /// Reading the native value raised an error, described here.
    ReadError(String),
}

/// The error of native data beyond `max_values`: `limit_exceeded`, with the
/// context `name` set to `max_values` (`spec/data-model.md`, *Reading native
/// data*, *Limits*), without a hint: [`too_many_values`] and
/// [`too_many_constant_values`] add the one for inputs or constants.
fn beyond_max_values(limit: usize) -> Error {
    let mut error = Error::new(
        ErrorCode::LimitExceeded,
        format!("the native data holds more than {limit} values (max_values)"),
    );
    error.context_mut().name = Some("max_values".to_owned());
    error
}

/// The error of an input's native data beyond `max_values`. Bindings add the
/// `row` of a batch.
#[must_use]
pub fn too_many_values(limit: usize) -> Error {
    beyond_max_values(limit).with_hint(
        "raise max_values, or pass data that shares fewer objects: an object referenced from \
         several places counts once per reference",
    )
}

/// The error of a native constant or rule metadata beyond the default
/// `max_values`, under which they are always converted: the caller cannot
/// raise it. Bindings add the `rule` whose metadata or constant it is.
#[must_use]
pub fn too_many_constant_values(limit: usize) -> Error {
    beyond_max_values(limit).with_hint(
        "constants and rule metadata are converted under the default max_values: pass smaller \
         metadata, or data that shares fewer objects (an object referenced from several places \
         counts once per reference)",
    )
}

/// Counts the values a binding reads from the native data of one input,
/// against the limit `max_values` (`spec/data-model.md`, *Reading native
/// data*, *Limits*): each value placed where a path ends or stops, with
/// everything inside it, and each position of a list or record that the paths
/// go through.
///
/// ```
/// use cleanrules::value::ValueCount;
///
/// let mut count = ValueCount::new(3);
/// assert!(count.add(2).is_ok());
/// assert_eq!(count.remaining(), 1);
/// let error = count.add(2).unwrap_err();
/// assert_eq!(error.code().as_str(), "limit_exceeded");
/// assert_eq!(count.count(), 2);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueCount {
    limit: usize,
    count: usize,
    /// Whether it counts a constant or metadata, under the default limit.
    constant: bool,
}

impl ValueCount {
    /// A count of none, against `limit` (`Limits::max_values`), for an input.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            count: 0,
            constant: false,
        }
    }

    /// A count of none for a native constant or rule metadata, against the
    /// default `max_values`; beyond it, [`too_many_constant_values`].
    #[must_use]
    pub fn for_constants() -> Self {
        Self {
            limit: crate::json::Limits::default().max_values,
            count: 0,
            constant: true,
        }
    }

    /// The error of going beyond this count's limit: [`too_many_values`] for
    /// an input, [`too_many_constant_values`] for a constant.
    #[must_use]
    pub fn beyond(&self) -> Error {
        if self.constant {
            too_many_constant_values(self.limit)
        } else {
            too_many_values(self.limit)
        }
    }

    /// Count `n` more values. Check before building them, so that a list's
    /// declared length is refused before it is allocated.
    ///
    /// # Errors
    ///
    /// [`ValueCount::beyond`] when the count would pass the limit; the count
    /// is then left as it was.
    pub fn add(&mut self, n: usize) -> Result<(), Error> {
        if n > self.remaining() {
            return Err(self.beyond());
        }
        self.count += n;
        Ok(())
    }

    /// The values counted so far.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// How many more values fit.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.limit - self.count
    }

    /// The limit.
    #[must_use]
    pub fn limit(&self) -> usize {
        self.limit
    }
}

/// The comparison group of a value (`spec/ast.md`, *Types*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    /// Ints, floats and decimals.
    Number,
    /// Strings.
    String,
    /// Booleans.
    Boolean,
    /// Dates.
    Date,
    /// Naive datetimes.
    NaiveDatetime,
    /// Aware datetimes.
    AwareDatetime,
    /// Lists, maps and opaque values.
    Other,
}

impl Group {
    /// The name used in reasons and errors, such as `naive_datetime`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Group::Number => "number",
            Group::String => "string",
            Group::Boolean => "boolean",
            Group::Date => "date",
            Group::NaiveDatetime => "naive_datetime",
            Group::AwareDatetime => "aware_datetime",
            Group::Other => "other",
        }
    }
}

impl fmt::Display for Group {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A map with string keys, without duplicate keys: an association, whose
/// entries have no order of their own. They are kept, and given back, sorted
/// by key by Unicode code point (`spec/data-model.md`, *Lists and maps*), so a
/// map never depends on the order it was built in.
///
/// A **record** is a map whose entries also have positions, such as a Python
/// namedtuple or a pandas row: an integer segment reads the entry at that
/// position (`spec/ast.md`, *Reading data*). Positions are the order of
/// insertion; they serve reading only, and entries are still given back by key.
#[derive(Clone, Debug, Default)]
pub struct Map {
    /// Sorted by key; Rust compares strings by their UTF-8 bytes, which is
    /// the order of their code points.
    entries: Vec<(String, Value)>,
    /// For a record, the index in `entries` of the entry at each position.
    positions: Option<Vec<usize>>,
}

impl Map {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty record: its entries' positions are their insertion order.
    #[must_use]
    pub fn record() -> Self {
        Self {
            entries: Vec::new(),
            positions: Some(Vec::new()),
        }
    }

    /// Whether the map is a record.
    #[must_use]
    pub fn is_record(&self) -> bool {
        self.positions.is_some()
    }

    /// For a record, the value at a position; never for a plain map.
    #[must_use]
    pub fn get_index(&self, index: usize) -> Option<&Value> {
        let at = *self.positions.as_ref()?.get(index)?;
        self.entries.get(at).map(|(_, v)| v)
    }

    /// Insert a value, replacing the one under the same key (which, in a
    /// record, keeps its position).
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.find(&key) {
            Ok(at) => self.entries[at].1 = value,
            Err(at) => {
                self.entries.insert(at, (key, value));
                if let Some(positions) = &mut self.positions {
                    for position in positions.iter_mut().filter(|p| **p >= at) {
                        *position += 1;
                    }
                    positions.push(at);
                }
            }
        }
    }

    /// The value under `key`, if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.find(key).ok().map(|at| &self.entries[at].1)
    }

    /// The entries, sorted by key by Unicode code point.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Where `key` is in `entries`, or where it would go.
    fn find(&self, key: &str) -> std::result::Result<usize, usize> {
        self.entries.binary_search_by(|(k, _)| k.as_str().cmp(key))
    }
}

/// A plain map of the entries, in O(n log n): sorted by key (a stable sort),
/// a repeated key keeping its last value, as [`Map::insert`] does.
impl FromIterator<(String, Value)> for Map {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(items: I) -> Self {
        let mut items: Vec<(String, Value)> = items.into_iter().collect();
        items.sort_by(|(a, _), (b, _)| a.cmp(b));
        let mut entries: Vec<(String, Value)> = Vec::with_capacity(items.len());
        for (key, value) in items {
            match entries.last_mut() {
                // Equal keys are adjacent, in their original order: the later wins.
                Some(last) if last.0 == key => last.1 = value,
                _ => entries.push((key, value)),
            }
        }
        Self {
            entries,
            positions: None,
        }
    }
}

/// A native value with no equivalent: its type name, informative, and a token that
/// identifies the native object (`spec/data-model.md`, *Opaque values*).
#[derive(Clone, Debug)]
pub struct Opaque {
    /// The native type's name, such as `bytes`.
    pub type_name: String,
    /// Identifies the native object: equal tokens mean the same object.
    pub token: u64,
}

/// Lists and maps are copied with an explicit stack: data nests as deep as the
/// reader's limits allow, deeper than a small thread's stack would take
/// recursively (`spec/json-format.md`, *Errors and limits*).
impl Clone for Value {
    fn clone(&self) -> Self {
        if !matches!(self, Value::List(_) | Value::Map(_)) {
            return self.clone_scalar();
        }
        let copied: Result<Value, std::convert::Infallible> = post_order(
            self,
            |value, done| Ok(value.item(done)),
            |value, items| {
                Ok(match value {
                    Value::List(_) => Value::List(items.collect()),
                    Value::Map(map) => Value::Map(Map {
                        entries: map
                            .entries
                            .iter()
                            .map(|(k, _)| k.clone())
                            .zip(items)
                            .collect(),
                        positions: map.positions.clone(),
                    }),
                    scalar => scalar.clone_scalar(),
                })
            },
        );
        match copied {
            Ok(value) => value,
            Err(never) => match never {},
        }
    }
}

impl Value {
    /// A float value, or null when `value` is NaN.
    #[must_use]
    pub fn float(value: f64) -> Self {
        if value.is_nan() {
            Value::Null(NullCause::Null)
        } else {
            Value::Float(value)
        }
    }

    /// Whether the value is null.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null(_))
    }

    /// The comparison group, or `None` for null.
    #[must_use]
    pub fn group(&self) -> Option<Group> {
        Some(match self {
            Value::Null(_) => return None,
            Value::Bool(_) => Group::Boolean,
            Value::Int(_) | Value::Float(_) | Value::Decimal(_) => Group::Number,
            Value::String(_) => Group::String,
            Value::Date(_) => Group::Date,
            Value::NaiveDateTime(_) => Group::NaiveDatetime,
            Value::AwareDateTime(_) => Group::AwareDatetime,
            Value::List(_) | Value::Map(_) | Value::Opaque(_) => Group::Other,
        })
    }

    /// The value as a number, if it is one.
    #[must_use]
    pub fn as_number(&self) -> Option<Number> {
        match *self {
            Value::Int(i) => Some(Number::Int(i)),
            Value::Float(f) => Some(Number::Float(f)),
            Value::Decimal(d) => Some(Number::Decimal(d)),
            _ => None,
        }
    }

    /// How two non-null values compare, for `eq` … `ge` (`spec/ast.md`, *Types*).
    ///
    /// Returns `None` when they cannot be compared: different groups, or a value of
    /// the `other` group. Booleans compare for equality only; ordering them is the
    /// caller's `unorderable` case, so they still return an ordering here.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
            (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
            (Value::Date(a), Value::Date(b)) => Some(a.cmp(b)),
            (Value::NaiveDateTime(a), Value::NaiveDateTime(b)) => Some(a.cmp(b)),
            (Value::AwareDateTime(a), Value::AwareDateTime(b)) => Some(a.cmp(b)),
            _ => Some(self.as_number()?.compare(other.as_number()?)),
        }
    }

    /// Equality under `ANY` (`spec/rules.md` and `spec/data-model.md`, *Equality*):
    /// null equals null, values of one group compare as [`Value::compare`], lists
    /// element by element, maps key by key whatever their order, and opaque values
    /// by identity.
    #[must_use]
    pub fn any_equal(&self, other: &Self) -> bool {
        // Pairs still to compare, with an explicit stack (see `Clone`), which
        // two values without items never need.
        let mut pending = Vec::new();
        let mut next = Some((self, other));
        while let Some((a, b)) = next.take().or_else(|| pending.pop()) {
            let equal = match (a, b) {
                (Value::Null(_), Value::Null(_)) => true,
                (Value::List(a), Value::List(b)) => {
                    pending.extend(a.iter().zip(b));
                    a.len() == b.len()
                }
                (Value::Map(a), Value::Map(b)) => {
                    a.len() == b.len()
                        && a.iter().all(|(k, x)| {
                            b.get(k).is_some_and(|y| {
                                pending.push((x, y));
                                true
                            })
                        })
                }
                (Value::Opaque(a), Value::Opaque(b)) => a.token == b.token,
                _ => a.compare(b) == Some(Ordering::Equal),
            };
            if !equal {
                return false;
            }
        }
        true
    }

    /// Drop the value with an explicit stack instead of recursion.
    ///
    /// A value is dropped recursively, one stack frame per level, which a value
    /// nested deep enough overflows on a small stack: the sparse value tree
    /// a binding builds along a long field path through native data
    /// (`spec/data-model.md`, *Reading native data*) is as deep as the path, and
    /// a NIF runs on a BEAM scheduler, whose stack is a few hundred KiB. Bindings
    /// drop such values with this.
    ///
    /// ```
    /// use cleanrules::Value;
    ///
    /// let mut deep = Value::Int(1);
    /// for _ in 0..1_000_000 {
    ///     deep = Value::List(vec![deep]);
    /// }
    /// deep.drop_iteratively();
    /// ```
    pub fn drop_iteratively(self) {
        let mut pending = vec![self];
        while let Some(value) = pending.pop() {
            match value {
                Value::List(items) => pending.extend(items),
                Value::Map(map) => pending.extend(map.entries.into_iter().map(|(_, v)| v)),
                _ => {}
            }
        }
    }

    /// The `index`-th item of a list, or the value of a map's `index`-th entry:
    /// the step of the walks over nested data.
    pub(crate) fn item(&self, index: usize) -> Option<&Value> {
        match self {
            Value::List(items) => items.get(index),
            Value::Map(map) => map.entries.get(index).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A copy of a value without items; an empty copy of a list or map.
    fn clone_scalar(&self) -> Self {
        match self {
            Value::Null(cause) => Value::Null(cause.clone()),
            Value::Bool(b) => Value::Bool(*b),
            Value::Int(i) => Value::Int(*i),
            Value::Float(f) => Value::Float(*f),
            Value::Decimal(d) => Value::Decimal(*d),
            Value::String(s) => Value::String(s.clone()),
            Value::Date(d) => Value::Date(*d),
            Value::NaiveDateTime(dt) => Value::NaiveDateTime(*dt),
            Value::AwareDateTime(a) => Value::AwareDateTime(a.clone()),
            Value::List(_) => Value::List(Vec::new()),
            Value::Map(map) => Value::Map(Map {
                entries: Vec::new(),
                positions: map.positions.as_ref().map(|_| Vec::new()),
            }),
            Value::Opaque(o) => Value::Opaque(o.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering::{Equal, Greater, Less};

    use jiff::civil::date;

    use super::{Decimal, Group, Map, NullCause, Opaque, Value};

    fn s(text: &str) -> Value {
        Value::String(text.into())
    }

    #[test]
    fn groups_and_names() {
        assert_eq!(Value::Int(1).group(), Some(Group::Number));
        assert_eq!(Value::Null(NullCause::Null).group(), None);
        assert_eq!(
            Value::NaiveDateTime(date(2026, 1, 1).at(0, 0, 0, 0))
                .group()
                .unwrap()
                .as_str(),
            "naive_datetime"
        );
        assert_eq!(Value::List(vec![]).group(), Some(Group::Other));
        assert!(Value::float(f64::NAN).is_null());
    }

    #[test]
    fn comparisons() {
        assert_eq!(Value::Int(1).compare(&Value::Float(1.0)), Some(Equal));
        assert_eq!(s("Z").compare(&s("a")), Some(Less));
        assert_eq!(s("é").compare(&s("z")), Some(Greater));
        assert_eq!(s("\u{ff5a}").compare(&s("\u{1F600}")), Some(Less));
        assert_eq!(s("1").compare(&Value::Int(1)), None);
        assert_eq!(
            Value::Date(date(2026, 1, 1))
                .compare(&Value::NaiveDateTime(date(2026, 1, 1).at(0, 0, 0, 0))),
            None
        );
        assert_eq!(Value::List(vec![]).compare(&Value::List(vec![])), None);
        assert_eq!(
            Value::Bool(true).compare(&Value::Bool(false)),
            Some(Greater)
        );
    }

    #[test]
    fn any_equality() {
        let d = |t: &str| Value::Decimal(Decimal::parse(t).unwrap());
        assert!(Value::Int(1).any_equal(&Value::Float(1.0)));
        assert!(!Value::Int(1).any_equal(&Value::Bool(true)));
        assert!(!d("0.1").any_equal(&Value::Float(0.1)));
        assert!(Value::Null(NullCause::IntOverflow).any_equal(&Value::Null(NullCause::Null)));
        assert!(Value::List(vec![Value::Int(1)]).any_equal(&Value::List(vec![Value::Float(1.0)])));
        let ab: Map = [
            ("a".to_string(), Value::Int(1)),
            ("b".to_string(), Value::Int(2)),
        ]
        .into_iter()
        .collect();
        let ba: Map = [
            ("b".to_string(), Value::Int(2)),
            ("a".to_string(), Value::Int(1)),
        ]
        .into_iter()
        .collect();
        assert!(Value::Map(ab).any_equal(&Value::Map(ba)));
        let opaque = |token| {
            Value::Opaque(Opaque {
                type_name: "bytes".into(),
                token,
            })
        };
        assert!(opaque(7).any_equal(&opaque(7)));
        assert!(!opaque(7).any_equal(&opaque(8)));
    }

    #[test]
    fn deep_values_drop_on_a_small_stack() {
        let mut deep = Value::Int(1);
        for i in 0..200_000 {
            deep = if i % 2 == 0 {
                Value::List(vec![Value::Null(NullCause::Null), deep])
            } else {
                Value::Map([("k".to_string(), deep)].into_iter().collect())
            };
        }
        let dropped = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || deep.drop_iteratively())
            .unwrap()
            .join();
        assert!(dropped.is_ok());
    }

    #[test]
    fn maps_are_sorted_by_code_point_and_replace() {
        let mut map = Map::new();
        map.insert("b", Value::Int(1));
        map.insert("a", Value::Int(2));
        map.insert("b", Value::Int(3));
        // U+FF5A sorts before U+1F600 by code point, after it by UTF-16 unit.
        map.insert("\u{1F600}", Value::Int(4));
        map.insert("\u{ff5a}", Value::Int(5));
        map.insert("B", Value::Int(6));
        let keys: Vec<&str> = map.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["B", "a", "b", "\u{ff5a}", "\u{1F600}"]);
        assert!(matches!(map.get("b"), Some(Value::Int(3))));
        assert!(map.get("c").is_none());
        let collected: Map = [("b", 1), ("a", 2), ("b", 3), ("c", 4), ("a", 5)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), Value::Int(v)))
            .collect();
        let pairs: Vec<(&str, i64)> = collected
            .iter()
            .map(|(k, v)| (k, if let Value::Int(i) = v { *i } else { 0 }))
            .collect();
        assert_eq!(pairs, [("a", 5), ("b", 3), ("c", 4)]);
        assert!(!collected.is_record());
        assert!(map.get_index(0).is_none());
    }

    #[test]
    fn records_keep_positions_and_give_entries_by_key() {
        let mut row = Map::record();
        row.insert("name", s("ana"));
        row.insert("age", Value::Int(30));
        row.insert("city", s("vigo"));
        row.insert("name", s("eva"));
        let keys: Vec<&str> = row.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["age", "city", "name"]);
        let at = |i| match row.get_index(i) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Int(n)) => n.to_string(),
            _ => String::new(),
        };
        assert_eq!([at(0), at(1), at(2), at(3)], ["eva", "30", "vigo", ""]);
        let copy = Value::Map(row).clone();
        let Value::Map(copy) = copy else {
            unreachable!("a map clones to a map")
        };
        assert!(copy.is_record());
        assert!(matches!(copy.get_index(1), Some(Value::Int(30))));
    }
}
