//! Sets of values of one subject (`spec/audit.md`, *Values of a subject*):
//! null, the `other` group, and a union of intervals for each orderable group
//! and for booleans.

use std::cmp::Ordering;

use jiff::SignedDuration;
use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::Offset;

use crate::value::{
    AwareDateTime, Decimal, Group, NullCause, Number, RoundingMode, Value, Zone, check_date,
    check_naive,
};

/// The groups whose values a set holds as intervals, in the order of
/// `spec/ast.md` (*Types*), which is also the order witnesses prefer.
pub(super) const ORDERED: [Group; 6] = [
    Group::Number,
    Group::String,
    Group::Boolean,
    Group::Date,
    Group::NaiveDatetime,
    Group::AwareDatetime,
];

fn slot(group: Group) -> Option<usize> {
    ORDERED.iter().position(|g| *g == group)
}

/// The exact order of two values of one group.
fn order(a: &Value, b: &Value) -> Ordering {
    // Two values of one orderable group, booleans included, always compare
    // (`Value::compare`): the sets never mix groups within a slot.
    a.compare(b).unwrap_or(Ordering::Equal)
}

/// The lower end of an interval.
#[derive(Clone, Debug)]
pub(super) enum Lo {
    /// From the group's first value: `-inf` for numbers, `""` for strings.
    Min,
    /// From this value, included.
    Incl(Value),
    /// From just above this value.
    Excl(Value),
}

/// The upper end of an interval.
#[derive(Clone, Debug)]
pub(super) enum Hi {
    /// To the group's last value, if it has one.
    Max,
    /// To this value, included.
    Incl(Value),
    /// To just below this value.
    Excl(Value),
}

/// The values of one group between two ends.
#[derive(Clone, Debug)]
pub(super) struct Interval {
    pub(super) lo: Lo,
    pub(super) hi: Hi,
}

fn cmp_lo(a: &Lo, b: &Lo) -> Ordering {
    match (a, b) {
        (Lo::Min, Lo::Min) => Ordering::Equal,
        (Lo::Min, _) => Ordering::Less,
        (_, Lo::Min) => Ordering::Greater,
        (Lo::Incl(x) | Lo::Excl(x), Lo::Incl(y) | Lo::Excl(y)) => {
            order(x, y).then_with(|| match (a, b) {
                (Lo::Incl(_), Lo::Excl(_)) => Ordering::Less,
                (Lo::Excl(_), Lo::Incl(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            })
        }
    }
}

fn cmp_hi(a: &Hi, b: &Hi) -> Ordering {
    match (a, b) {
        (Hi::Max, Hi::Max) => Ordering::Equal,
        (Hi::Max, _) => Ordering::Greater,
        (_, Hi::Max) => Ordering::Less,
        (Hi::Incl(x) | Hi::Excl(x), Hi::Incl(y) | Hi::Excl(y)) => {
            order(x, y).then_with(|| match (a, b) {
                (Hi::Excl(_), Hi::Incl(_)) => Ordering::Less,
                (Hi::Incl(_), Hi::Excl(_)) => Ordering::Greater,
                _ => Ordering::Equal,
            })
        }
    }
}

/// Whether an interval ending at `hi` and one starting at `lo`, no lower,
/// leave no value between them, so they merge.
fn touches(hi: &Hi, lo: &Lo) -> bool {
    match (hi, lo) {
        (Hi::Max, _) | (_, Lo::Min) => true,
        (Hi::Incl(x) | Hi::Excl(x), Lo::Incl(y) | Lo::Excl(y)) => match order(x, y) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => !matches!((hi, lo), (Hi::Excl(_), Lo::Excl(_))),
        },
    }
}

impl Interval {
    /// Every value of the group.
    pub(super) fn all() -> Self {
        Self {
            lo: Lo::Min,
            hi: Hi::Max,
        }
    }

    /// Whether `value`, of the interval's group, lies in it.
    fn admits(&self, value: &Value) -> bool {
        let above = match &self.lo {
            Lo::Min => true,
            Lo::Incl(a) => order(value, a) != Ordering::Less,
            Lo::Excl(a) => order(value, a) == Ordering::Greater,
        };
        let below = match &self.hi {
            Hi::Max => true,
            Hi::Incl(b) => order(value, b) != Ordering::Greater,
            Hi::Excl(b) => order(value, b) == Ordering::Less,
        };
        above && below
    }

    fn meet(&self, other: &Self) -> Self {
        let lo = if cmp_lo(&self.lo, &other.lo) == Ordering::Less {
            other.lo.clone()
        } else {
            self.lo.clone()
        };
        let hi = if cmp_hi(&self.hi, &other.hi) == Ordering::Greater {
            other.hi.clone()
        } else {
            self.hi.clone()
        };
        Self { lo, hi }
    }
}

/// A value found in a set, or why none was.
#[derive(Clone, Debug)]
pub(super) enum Pick {
    /// A value of the set.
    Found(Value),
    /// The set is certainly empty.
    Empty,
    /// No value was found, but the set may not be empty: numbers between two
    /// bounds too close for the candidates tried.
    Unknown,
}

/// A value of `group` in `interval`: one that exists whenever the interval is
/// not empty, but for numbers, which are dense and only tried.
fn pick(group: Group, interval: &Interval) -> Pick {
    let candidates = match group {
        Group::Number => return pick_number(interval),
        Group::String => match &interval.lo {
            Lo::Min => vec![Value::String(String::new())],
            Lo::Incl(v) => vec![v.clone()],
            // The least string above `s` is `s` followed by U+0000.
            Lo::Excl(Value::String(s)) => {
                vec![
                    Value::String(format!("{s}a")),
                    Value::String(format!("{s}\0")),
                ]
            }
            Lo::Excl(_) => Vec::new(),
        },
        Group::Boolean => match &interval.lo {
            Lo::Min => vec![Value::Bool(false)],
            Lo::Incl(v) => vec![v.clone()],
            Lo::Excl(Value::Bool(false)) => vec![Value::Bool(true)],
            Lo::Excl(_) => Vec::new(),
        },
        Group::Date => match &interval.lo {
            Lo::Min => vec![Value::Date(jiff::civil::date(1, 1, 1))],
            Lo::Incl(v) => vec![v.clone()],
            Lo::Excl(Value::Date(d)) => next_date(*d).into_iter().collect(),
            Lo::Excl(_) => Vec::new(),
        },
        Group::NaiveDatetime => match &interval.lo {
            Lo::Min => vec![Value::NaiveDateTime(jiff::civil::datetime(
                1, 1, 1, 0, 0, 0, 0,
            ))],
            Lo::Incl(v) => vec![v.clone()],
            Lo::Excl(Value::NaiveDateTime(t)) => later_naive(*t),
            Lo::Excl(_) => Vec::new(),
        },
        Group::AwareDatetime => match &interval.lo {
            Lo::Min => first_instant().into_iter().collect(),
            Lo::Incl(v) => vec![v.clone()],
            Lo::Excl(Value::AwareDateTime(t)) => later_aware(t.timestamp()),
            Lo::Excl(_) => Vec::new(),
        },
        Group::Other => Vec::new(),
    };
    // The last candidate is the least value above the lower end, so if it is
    // not below the upper end, no value is.
    candidates
        .into_iter()
        .find(|v| interval.admits(v))
        .map_or(Pick::Empty, Pick::Found)
}

fn next_date(date: Date) -> Option<Value> {
    date.tomorrow()
        .ok()
        .and_then(|d| check_date(d).ok())
        .map(Value::Date)
}

/// Later datetimes, nicest first; the last is the least one, a microsecond
/// later.
fn later_naive(datetime: DateTime) -> Vec<Value> {
    [
        SignedDuration::from_secs(86_400),
        SignedDuration::from_secs(1),
        SignedDuration::from_micros(1),
    ]
    .into_iter()
    .filter_map(|step| datetime.checked_add(step).ok())
    .filter_map(|t| check_naive(t).ok())
    .map(Value::NaiveDateTime)
    .collect()
}

fn utc(timestamp: Timestamp) -> Option<Value> {
    AwareDateTime::new(timestamp, Zone::Fixed(Offset::UTC))
        .ok()
        .map(Value::AwareDateTime)
}

/// The first instant of the data model: 0001-01-01T00:00:00Z.
fn first_instant() -> Option<Value> {
    Timestamp::from_second(-62_135_596_800).ok().and_then(utc)
}

fn later_aware(timestamp: Timestamp) -> Vec<Value> {
    [
        SignedDuration::from_secs(86_400),
        SignedDuration::from_secs(1),
        SignedDuration::from_micros(1),
    ]
    .into_iter()
    .filter_map(|step| timestamp.checked_add(step).ok())
    .filter_map(utc)
    .collect()
}

/// The number of a bound, with the ends of the number line as infinities.
fn lo_number(lo: &Lo) -> (Number, bool) {
    match lo {
        Lo::Min => (Number::Float(f64::NEG_INFINITY), true),
        Lo::Incl(v) => (v.as_number().unwrap_or(Number::Int(0)), true),
        Lo::Excl(v) => (v.as_number().unwrap_or(Number::Int(0)), false),
    }
}

fn hi_number(hi: &Hi) -> (Number, bool) {
    match hi {
        Hi::Max => (Number::Float(f64::INFINITY), true),
        Hi::Incl(v) => (v.as_number().unwrap_or(Number::Int(0)), true),
        Hi::Excl(v) => (v.as_number().unwrap_or(Number::Int(0)), false),
    }
}

/// The greatest int not above a finite number, if it is an int.
fn floor(number: Number) -> Option<i64> {
    match number {
        Number::Int(i) => Some(i),
        Number::Float(f) if f.is_finite() => {
            let floor = f.floor();
            let bound = 2f64.powi(63);
            // Within the int range, the floor of a float is an exact int.
            #[allow(clippy::cast_possible_truncation)]
            (-bound..bound).contains(&floor).then_some(floor as i64)
        }
        Number::Float(_) => None,
        Number::Decimal(d) => d
            .round(0, RoundingMode::Floor)
            .ok()
            .and_then(Decimal::parts)
            .and_then(|(c, _)| i64::try_from(c).ok()),
    }
}

/// The number as a decimal, when it is an int or a finite decimal.
fn decimal(number: Number) -> Option<Decimal> {
    match number {
        Number::Int(i) => Some(Decimal::from(i)),
        Number::Decimal(d) if !d.is_infinite() => Some(d),
        _ => None,
    }
}

fn number_value(number: Number) -> Value {
    match number {
        Number::Int(i) => Value::Int(i),
        Number::Float(f) => Value::float(f),
        Number::Decimal(d) => Value::Decimal(d),
    }
}

/// A number in `interval`: an end it includes, a small int near its ends, a
/// midpoint, or an infinity. Numbers are dense, so when none of these is in
/// it, it may still hold one, unless its ends cross or meet excluded.
fn pick_number(interval: &Interval) -> Pick {
    let (low, low_in) = lo_number(&interval.lo);
    let (high, high_in) = hi_number(&interval.hi);
    match low.compare(high) {
        Ordering::Greater => return Pick::Empty,
        Ordering::Equal if !(low_in && high_in) => return Pick::Empty,
        _ => {}
    }
    let mut candidates: Vec<Value> = Vec::new();
    if let Lo::Incl(v) = &interval.lo {
        candidates.push(v.clone());
    }
    if let Hi::Incl(v) = &interval.hi {
        candidates.push(v.clone());
    }
    candidates.push(Value::Int(0));
    for end in [low, high] {
        if let Some(k) = floor(end) {
            candidates.extend(
                [k.checked_add(1), Some(k), k.checked_sub(1)]
                    .into_iter()
                    .flatten()
                    .map(Value::Int),
            );
        }
    }
    if let (Some(a), Some(b)) = (decimal(low), decimal(high))
        && let Some(mid) = a
            .try_add(b)
            .ok()
            .and_then(|s| s.try_div(Decimal::from(2)).ok())
    {
        candidates.push(Value::Decimal(mid));
    }
    let (a, b) = (low.to_f64(), high.to_f64());
    if a.is_finite() && b.is_finite() {
        candidates.push(Value::float(a / 2.0 + b / 2.0));
    }
    candidates.push(number_value(low));
    candidates.push(number_value(high));
    candidates
        .into_iter()
        .find(|v| interval.admits(v))
        .map_or(Pick::Unknown, Pick::Found)
}

/// Keep the intervals that may hold a value, sorted, merging those that
/// overlap or touch.
fn normalize(group: Group, intervals: Vec<Interval>) -> Vec<Interval> {
    let mut kept: Vec<Interval> = intervals
        .into_iter()
        .filter(|i| !matches!(pick(group, i), Pick::Empty))
        .collect();
    kept.sort_by(|a, b| cmp_lo(&a.lo, &b.lo));
    let mut merged: Vec<Interval> = Vec::with_capacity(kept.len());
    for interval in kept {
        match merged.last_mut() {
            Some(last) if touches(&last.hi, &interval.lo) => {
                if cmp_hi(&interval.hi, &last.hi) == Ordering::Greater {
                    last.hi = interval.hi;
                }
            }
            _ => merged.push(interval),
        }
    }
    merged
}

/// The values of `group` outside sorted, disjoint `intervals`.
fn complement(group: Group, intervals: &[Interval]) -> Vec<Interval> {
    let mut gaps = Vec::new();
    let mut from = Some(Lo::Min);
    for interval in intervals {
        let Some(lo) = from.take() else { break };
        let hi = match &interval.lo {
            Lo::Min => None,
            Lo::Incl(v) => Some(Hi::Excl(v.clone())),
            Lo::Excl(v) => Some(Hi::Incl(v.clone())),
        };
        if let Some(hi) = hi {
            gaps.push(Interval { lo, hi });
        }
        from = match &interval.hi {
            Hi::Max => None,
            Hi::Incl(v) => Some(Lo::Excl(v.clone())),
            Hi::Excl(v) => Some(Lo::Incl(v.clone())),
        };
    }
    if let Some(lo) = from {
        gaps.push(Interval { lo, hi: Hi::Max });
    }
    normalize(group, gaps)
}

fn meet_all(group: Group, a: &[Interval], b: &[Interval]) -> Vec<Interval> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            out.push(x.meet(y));
        }
    }
    normalize(group, out)
}

/// A set of values of one subject.
#[derive(Clone, Debug, Default)]
pub(super) struct ValueSet {
    null: bool,
    other: bool,
    groups: [Vec<Interval>; 6],
}

impl ValueSet {
    /// No value.
    pub(super) fn none() -> Self {
        Self::default()
    }

    /// Every value, null included.
    pub(super) fn all() -> Self {
        Self {
            null: true,
            other: true,
            groups: std::array::from_fn(|_| vec![Interval::all()]),
        }
    }

    /// Null only.
    pub(super) fn null() -> Self {
        Self {
            null: true,
            ..Self::default()
        }
    }

    /// Every value but null.
    pub(super) fn not_null() -> Self {
        Self {
            null: false,
            ..Self::all()
        }
    }

    /// Every value of `group`.
    pub(super) fn group(group: Group) -> Self {
        Self::range(group, Interval::all())
    }

    /// The values of `group` in `interval`.
    pub(super) fn range(group: Group, interval: Interval) -> Self {
        let mut set = Self::default();
        match slot(group) {
            Some(at) => set.groups[at] = normalize(group, vec![interval]),
            None => set.other = true,
        }
        set
    }

    /// The values equal to `value`, which is not null.
    pub(super) fn point(value: &Value) -> Self {
        match value.group() {
            Some(group) => Self::range(
                group,
                Interval {
                    lo: Lo::Incl(value.clone()),
                    hi: Hi::Incl(value.clone()),
                },
            ),
            None => Self::null(),
        }
    }

    /// Whether the set certainly holds no value.
    pub(super) fn is_empty(&self) -> bool {
        !self.null && !self.other && self.groups.iter().all(Vec::is_empty)
    }

    /// The values in both sets.
    pub(super) fn meet(&self, other: &Self) -> Self {
        Self {
            null: self.null && other.null,
            other: self.other && other.other,
            groups: std::array::from_fn(|i| {
                meet_all(ORDERED[i], &self.groups[i], &other.groups[i])
            }),
        }
    }

    /// The values in either set.
    pub(super) fn join(&self, other: &Self) -> Self {
        Self {
            null: self.null || other.null,
            other: self.other || other.other,
            groups: std::array::from_fn(|i| {
                let both = self.groups[i]
                    .iter()
                    .chain(&other.groups[i])
                    .cloned()
                    .collect();
                normalize(ORDERED[i], both)
            }),
        }
    }

    /// The values of this set that are not in `other`.
    pub(super) fn minus(&self, other: &Self) -> Self {
        Self {
            null: self.null && !other.null,
            other: self.other && !other.other,
            groups: std::array::from_fn(|i| {
                let outside = complement(ORDERED[i], &other.groups[i]);
                meet_all(ORDERED[i], &self.groups[i], &outside)
            }),
        }
    }

    /// A value of the set: of the first group in [`ORDERED`] order that has
    /// one, then of the `other` group (an empty list), then null.
    pub(super) fn pick(&self) -> Pick {
        let mut unknown = false;
        for (group, intervals) in ORDERED.iter().zip(&self.groups) {
            for interval in intervals {
                match pick(*group, interval) {
                    Pick::Found(value) => return Pick::Found(value),
                    Pick::Unknown => unknown = true,
                    Pick::Empty => {}
                }
            }
        }
        if self.other {
            return Pick::Found(Value::List(Vec::new()));
        }
        if self.null {
            return Pick::Found(Value::Null(NullCause::Null));
        }
        if unknown { Pick::Unknown } else { Pick::Empty }
    }

    /// A value of the set, of `preferred` if the two meet.
    pub(super) fn pick_within(&self, preferred: &Self) -> Pick {
        match self.meet(preferred).pick() {
            Pick::Found(value) => Pick::Found(value),
            _ => self.pick(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::{Group, Hi, Interval, Lo, Pick, ValueSet};
    use crate::value::{Decimal, Value};

    fn found(pick: Pick) -> Value {
        match pick {
            Pick::Found(v) => v,
            other => panic!("expected a value, got {other:?}"),
        }
    }

    fn int(i: i64) -> Value {
        Value::Int(i)
    }

    fn open(group: Group, a: Value, b: Value) -> ValueSet {
        ValueSet::range(
            group,
            Interval {
                lo: Lo::Excl(a),
                hi: Hi::Excl(b),
            },
        )
    }

    #[test]
    fn numbers_between_two_ints_are_found() {
        let set = open(Group::Number, int(1), int(2));
        let Value::Decimal(d) = found(set.pick()) else {
            panic!()
        };
        assert_eq!(d, Decimal::parse("1.5").unwrap());
        assert!(open(Group::Number, int(1), int(1)).is_empty());
        let above = ValueSet::range(
            Group::Number,
            Interval {
                lo: Lo::Excl(int(5)),
                hi: Hi::Max,
            },
        );
        assert!(matches!(found(above.pick()), Value::Int(6)));
        // Below minus infinity there is nothing.
        let below = ValueSet::range(
            Group::Number,
            Interval {
                lo: Lo::Min,
                hi: Hi::Excl(Value::Float(f64::NEG_INFINITY)),
            },
        );
        assert!(below.is_empty());
    }

    #[test]
    fn numbers_too_close_are_unknown() {
        let a = Value::Decimal(Decimal::parse("0.99999999999999999999999999999999999999").unwrap());
        let set = open(Group::Number, a, int(1));
        assert!(!set.is_empty());
        assert!(matches!(set.pick(), Pick::Unknown));
    }

    #[test]
    fn discrete_groups_are_exact() {
        let s = |t: &str| Value::String(t.into());
        assert!(open(Group::String, s("a"), s("a\0")).is_empty());
        assert!(
            matches!(found(open(Group::String, s("a"), s("b")).pick()), Value::String(t) if t == "aa")
        );
        assert!(
            matches!(found(open(Group::String, s("a"), s("aa")).pick()), Value::String(t) if t == "a\0")
        );
        let d = |day| Value::Date(jiff::civil::date(2026, 1, day));
        assert!(open(Group::Date, d(1), d(2)).is_empty());
        assert!(
            matches!(found(open(Group::Date, d(1), d(3)).pick()), Value::Date(x) if x.day() == 2)
        );
        let last = Value::Date(jiff::civil::date(9999, 12, 31));
        let after = ValueSet::range(
            Group::Date,
            Interval {
                lo: Lo::Excl(last),
                hi: Hi::Max,
            },
        );
        assert!(after.is_empty());
        assert!(open(Group::Boolean, Value::Bool(false), Value::Bool(true)).is_empty());
        let t = |s| Value::NaiveDateTime(jiff::civil::datetime(2026, 1, 1, 0, 0, s, 0));
        assert!(
            matches!(found(open(Group::NaiveDatetime, t(0), t(2)).pick()), Value::NaiveDateTime(x) if x.second() == 1)
        );
        let micro = Value::NaiveDateTime(jiff::civil::datetime(2026, 1, 1, 0, 0, 0, 1000));
        assert!(open(Group::NaiveDatetime, t(0), micro).is_empty());
    }

    #[test]
    fn set_operations() {
        let five = ValueSet::point(&int(5));
        let numbers = ValueSet::group(Group::Number);
        let not_five = numbers.minus(&five);
        assert!(not_five.meet(&five).is_empty());
        assert!(not_five.join(&five).minus(&numbers).is_empty());
        assert!(numbers.minus(&not_five.join(&five)).is_empty());
        assert!(
            ValueSet::all()
                .minus(&ValueSet::not_null())
                .meet(&ValueSet::null())
                .null
        );
        assert!(matches!(
            ValueSet::null().pick(),
            Pick::Found(Value::Null(_))
        ));
        let mut other = ValueSet::none();
        other.other = true;
        assert!(matches!(other.pick(), Pick::Found(Value::List(_))));
        assert!(matches!(ValueSet::none().pick(), Pick::Empty));
        let strings = ValueSet::group(Group::String);
        assert!(matches!(
            found(strings.pick_within(&numbers)),
            Value::String(_)
        ));
        assert!(matches!(
            found(not_five.pick_within(&numbers)),
            Value::Int(0)
        ));
    }
}
