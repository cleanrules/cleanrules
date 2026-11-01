//! Values: the data that conditions read and constants hold (`spec/data-model.md`).

mod bignum;
mod datetime;
mod decimal;
mod number;

use std::cmp::Ordering;
use std::fmt;

pub use datetime::{AwareDateTime, DateTimeError, Zone, check_date, check_naive};
pub use decimal::{Decimal, DecimalError, MAX_DIGITS};
use jiff::civil::{Date, DateTime};
pub use number::Number;

/// A value of the data model.
///
/// Floats are never NaN: build them with [`Value::float`], which turns NaN into
/// null, as the spec requires of data.
#[derive(Clone, Debug)]
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
    /// String keys mapped to values, in a fixed order.
    Map(Map),
    /// A native value with no equivalent.
    Opaque(Opaque),
}

/// Why a value is null (`spec/data-model.md`, *Nulls with a reason*).
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// A map with string keys, kept in insertion order, without duplicate keys.
///
/// A **record** is a map whose entries also have positions, such as a Python
/// namedtuple or a pandas row: an integer segment reads the entry at that
/// position (`spec/ast.md`, *Reading data*).
#[derive(Clone, Debug, Default)]
pub struct Map {
    entries: Vec<(String, Value)>,
    record: bool,
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
            record: true,
        }
    }

    /// Whether the map is a record.
    #[must_use]
    pub fn is_record(&self) -> bool {
        self.record
    }

    /// For a record, the value at a position; never for a plain map.
    #[must_use]
    pub fn get_index(&self, index: usize) -> Option<&Value> {
        if self.record {
            self.entries.get(index).map(|(_, v)| v)
        } else {
            None
        }
    }

    /// Insert a value, replacing the one under the same key in place.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// The value under `key`, if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The entries, in order.
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
}

impl FromIterator<(String, Value)> for Map {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(items: I) -> Self {
        let mut map = Map::new();
        for (key, value) in items {
            map.insert(key, value);
        }
        map
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
        match (self, other) {
            (Value::Null(_), Value::Null(_)) => true,
            (Value::List(a), Value::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.any_equal(y))
            }
            (Value::Map(a), Value::Map(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, x)| b.get(k).is_some_and(|y| x.any_equal(y)))
            }
            (Value::Opaque(a), Value::Opaque(b)) => a.token == b.token,
            _ => self.compare(other) == Some(Ordering::Equal),
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
    fn maps_keep_order_and_replace_in_place() {
        let mut map = Map::new();
        map.insert("b", Value::Int(1));
        map.insert("a", Value::Int(2));
        map.insert("b", Value::Int(3));
        let keys: Vec<&str> = map.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["b", "a"]);
        assert!(matches!(map.get("b"), Some(Value::Int(3))));
    }
}
