//! Reading Erlang terms as values (`spec/data-model.md`, *Erlang*).
//!
//! The binding reads only the paths a rule set reads, segment by segment, and
//! hands the core a sparse value tree (*Reading native data*): a string segment
//! reads a map's binary key, or else its atom key (never creating an atom), or
//! an Elixir struct's field; an integer segment indexes a proper list. Where a
//! path ends, the term is converted whole. Everything read counts against
//! `max_values` (*Limits*): beyond it, reading stops and the input is refused
//! with the core's `limit_exceeded`.
//!
//! Lists, maps and paths are walked with explicit stacks, never recursively: a
//! NIF runs on a scheduler thread, whose stack is small (dirty schedulers have
//! 320 KiB by default), and a path or a term may be very deep.
//!
//! On the normal scheduler, reading also has a [`Budget`] of work: beyond it,
//! reading stops with [`Stop::Budget`] and the call moves to a dirty scheduler,
//! so that one call stays near a millisecond.

use std::collections::HashMap;

use cleanrules::jiff::civil::{Date, DateTime};
use cleanrules::jiff::tz::Offset;
use cleanrules::value::{AwareDateTime, Decimal, DecimalError, Opaque, ValueCount, Zone};
use cleanrules::{Error, Map, NullCause, Value};
use rustler::types::map::MapIterator;
use rustler::types::tuple::get_tuple;
use rustler::{Atom, Binary, Encoder, Env, Term, TermType};

use crate::atoms;
use crate::bignum::{self, Integer};
use crate::paths::Paths;

/// How deep a list or map is converted where a path ends: deeper ones are
/// opaque at that place (`spec/data-model.md`, *Lists and maps*).
pub(crate) const MAX_DEPTH: usize = 256;

/// The bytes of text that cost one unit of the budget: text is charged more
/// than its copy costs, since the core may scan it again (`matches`,
/// `contains`).
const TEXT_BYTES: usize = 16;

/// The largest UTC offset, in seconds: 18 hours (*Datetimes*).
const MAX_OFFSET: i64 = 18 * 3600;

/// The work a call on the normal scheduler may do while reading its input, in
/// units: one per value read, per map entry looked at, and per
/// [`TEXT_BYTES`] bytes of text. Measured to keep a call near a millisecond
/// (`README.md`, *Schedulers*).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Budget {
    /// The units left.
    pub(crate) units: usize,
}

/// Why reading stopped.
#[derive(Debug)]
pub(crate) enum Stop {
    /// More values than `max_values`: the core's `limit_exceeded`.
    Limit(Error),
    /// More work than the normal scheduler's budget: read it again on a dirty
    /// scheduler.
    Budget,
}

impl From<Error> for Stop {
    fn from(error: Error) -> Self {
        Self::Limit(error)
    }
}

type Read<T = Value> = Result<T, Stop>;

/// A value dropped with an explicit stack ([`Value::drop_iteratively`]): the
/// sparse value tree of an input is as deep as the longest path the rule set
/// reads, and a scheduler thread's stack is small.
#[derive(Debug)]
pub(crate) struct Tree(Option<Value>);

impl Tree {
    pub(crate) fn new(value: Value) -> Self {
        Self(Some(value))
    }

    /// The value.
    pub(crate) fn value(&self) -> &Value {
        const NULL: &Value = &Value::Null(NullCause::Null);
        self.0.as_ref().unwrap_or(NULL)
    }

    fn into_value(mut self) -> Value {
        self.take()
    }

    /// Take the value out, to be dropped by the caller with an explicit stack.
    pub(crate) fn take(&mut self) -> Value {
        self.0.take().unwrap_or(Value::Null(NullCause::Null))
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            value.drop_iteratively();
        }
    }
}

/// How a term is read.
enum Shape {
    /// A value with no parts: converted at once.
    Scalar(Value),
    /// Nothing to read through: an opaque value with this type name.
    Opaque(String),
    /// A proper list of this length.
    List(usize),
    /// A map; `strip` for an Elixir struct, whose fields starting with `__`
    /// are never read.
    Map { strip: bool },
}

/// Reads the terms of one input (or one constant), keeping the terms of its
/// opaque values: an opaque value's token is its position in `objects`.
pub(crate) struct Reader<'a> {
    env: Env<'a>,
    /// The terms of the opaque values, by token.
    pub(crate) objects: Vec<Term<'a>>,
    /// The token of each term in `objects`, by exact equality (`=:=`): only
    /// looked up, never iterated (`CLAUDE.md`, invariant 7). Tokens are handed
    /// out in the order terms are met.
    tokens: HashMap<Term<'a>, u64>,
    count: ValueCount,
    budget: Option<Budget>,
}

impl<'a> Reader<'a> {
    /// A reader of at most `max_values` values, with an optional budget.
    pub(crate) fn new(env: Env<'a>, max_values: usize, budget: Option<Budget>) -> Self {
        Self {
            env,
            objects: Vec::new(),
            tokens: HashMap::new(),
            count: ValueCount::new(max_values),
            budget,
        }
    }

    /// Spend `units` of the budget, if there is one.
    fn work(&mut self, units: usize) -> Read<()> {
        if let Some(budget) = &mut self.budget {
            if units > budget.units {
                return Err(Stop::Budget);
            }
            budget.units -= units;
        }
        Ok(())
    }

    /// Count `n` values against `max_values`, and spend them.
    fn count(&mut self, n: usize) -> Read<()> {
        self.count.add(n)?;
        self.work(n)
    }

    fn opaque(&mut self, term: Term<'a>, type_name: String) -> Value {
        let next = self.objects.len() as u64;
        let token = *self.tokens.entry(term).or_insert_with(|| {
            self.objects.push(term);
            next
        });
        Value::Opaque(Opaque { type_name, token })
    }

    /// Read `data` along `paths`: the sparse value tree of one input.
    ///
    /// # Errors
    ///
    /// [`Stop::Limit`] beyond `max_values`, [`Stop::Budget`] beyond the budget.
    pub(crate) fn read(&mut self, data: Term<'a>, paths: &Paths) -> Read<Tree> {
        enum Task<'t, 'p> {
            Walk(Term<'t>, &'p Paths),
            Map(Vec<&'p str>),
            List(usize, Vec<usize>),
        }
        let mut tasks = vec![Task::Walk(data, paths)];
        // Finished parts, deep or not: dropped with an explicit stack should
        // reading stop.
        let mut done: Vec<Tree> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Walk(term, paths) if paths.end => done.push(Tree::new(self.convert(term)?)),
                Task::Walk(term, paths) => match self.shape(term)? {
                    Shape::Map { strip } => {
                        let mut found = Vec::new();
                        let mut children = Vec::new();
                        for (key, sub) in &paths.keys {
                            self.work(1)?;
                            if let Some(child) = self.lookup(term, key, strip) {
                                found.push(key.as_str());
                                children.push(Task::Walk(child, sub));
                            }
                        }
                        tasks.push(Task::Map(found));
                        tasks.extend(children.into_iter().rev());
                    }
                    Shape::List(len) => {
                        // The list keeps its declared length, each position a
                        // value the core holds: counted before it is allocated.
                        self.count(len)?;
                        let items: Vec<Term<'a>> = list_items(term);
                        let mut positions = Vec::new();
                        let mut children = Vec::new();
                        for (index, sub) in &paths.indices {
                            if let Some(i) = usize::try_from(*index).ok().filter(|i| *i < len) {
                                positions.push(i);
                                children.push(Task::Walk(items[i], sub));
                            }
                        }
                        tasks.push(Task::List(len, positions));
                        tasks.extend(children.into_iter().rev());
                    }
                    // A path that stops early: what is found there counts.
                    Shape::Scalar(_) | Shape::Opaque(_) => {
                        done.push(Tree::new(self.convert(term)?));
                    }
                },
                Task::Map(keys) => {
                    let values = done.split_off(done.len() - keys.len());
                    let mut map = Map::new();
                    for (key, value) in keys.into_iter().zip(values) {
                        map.insert(key, value.into_value());
                    }
                    done.push(Tree::new(Value::Map(map)));
                }
                Task::List(len, positions) => {
                    let values = done.split_off(done.len() - positions.len());
                    let mut items = vec![Value::Null(NullCause::Null); len];
                    for (i, value) in positions.into_iter().zip(values) {
                        items[i] = value.into_value();
                    }
                    done.push(Tree::new(Value::List(items)));
                }
            }
        }
        Ok(done
            .pop()
            .unwrap_or_else(|| Tree::new(Value::Null(NullCause::Null))))
    }

    /// The whole term, where a path ends (`spec/data-model.md`, *Erlang*): it
    /// counts one, and so does everything inside it.
    ///
    /// # Errors
    ///
    /// As [`Reader::read`].
    pub(crate) fn convert(&mut self, root: Term<'a>) -> Read {
        enum Task<'t> {
            Visit(Term<'t>, usize),
            List(usize),
            Map(Vec<String>),
        }
        let mut tasks = vec![Task::Visit(root, 0)];
        let mut done: Vec<Value> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Visit(term, depth) => {
                    self.count(1)?;
                    match self.shape(term)? {
                        Shape::Scalar(value) => done.push(value),
                        Shape::Opaque(type_name) => done.push(self.opaque(term, type_name)),
                        Shape::List(_) | Shape::Map { .. } if depth >= MAX_DEPTH => {
                            let name = if term.is_map() { "map" } else { "list" };
                            done.push(self.opaque(term, name.to_owned()));
                        }
                        Shape::List(len) => {
                            // Each element counts as it is converted; a length
                            // beyond what is left is refused before the list is
                            // allocated.
                            if len > self.count.remaining() {
                                return Err(self.count.beyond().into());
                            }
                            self.work(len)?;
                            tasks.push(Task::List(len));
                            let items = list_items(term);
                            tasks
                                .extend(items.into_iter().rev().map(|t| Task::Visit(t, depth + 1)));
                        }
                        Shape::Map { strip } => {
                            let entries = self.entries(term, strip)?;
                            let (keys, values): (Vec<String>, Vec<Term<'a>>) =
                                entries.into_iter().unzip();
                            tasks.push(Task::Map(keys));
                            tasks.extend(
                                values.into_iter().rev().map(|t| Task::Visit(t, depth + 1)),
                            );
                        }
                    }
                }
                Task::List(len) => {
                    let items = done.split_off(done.len() - len);
                    done.push(Value::List(items));
                }
                Task::Map(keys) => {
                    let values = done.split_off(done.len() - keys.len());
                    done.push(Value::Map(keys.into_iter().zip(values).collect()));
                }
            }
        }
        Ok(done.pop().unwrap_or(Value::Null(NullCause::Null)))
    }

    /// The entries of a map with a binary (UTF-8) or atom key, sorted by key;
    /// a binary key wins over an atom with the same text. For a struct, never
    /// the fields starting with `__`.
    fn entries(&mut self, map: Term<'a>, strip: bool) -> Read<Vec<(String, Term<'a>)>> {
        let mut entries: Vec<(String, bool, Term<'a>)> = Vec::new();
        for (key, value) in MapIterator::new(map).into_iter().flatten() {
            self.work(1)?;
            let (text, binary) = match key.get_type() {
                TermType::Binary => {
                    let Ok(binary) = Binary::from_term(key) else {
                        continue;
                    };
                    // Charged before the key is copied.
                    self.work(binary.len() / TEXT_BYTES)?;
                    let Ok(text) = std::str::from_utf8(binary.as_slice()) else {
                        continue;
                    };
                    if strip && text.starts_with("__") {
                        continue;
                    }
                    (text.to_owned(), true)
                }
                TermType::Atom => match key.atom_to_string() {
                    Ok(text) if !(strip && text.starts_with("__")) => (text, false),
                    _ => continue,
                },
                _ => continue,
            };
            entries.push((text, binary, value));
        }
        // By key (UTF-8 bytes sort as code points), the binary first.
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        entries.dedup_by(|later, earlier| later.0 == earlier.0);
        Ok(entries
            .into_iter()
            .map(|(text, _, value)| (text, value))
            .collect())
    }

    /// A string segment on a map: the binary key, or else the atom key if the
    /// atom exists (never created). On a struct, never a field starting with
    /// `__`.
    fn lookup(&self, map: Term<'a>, key: &str, strip: bool) -> Option<Term<'a>> {
        if strip && key.starts_with("__") {
            return None;
        }
        if let Ok(found) = map.map_get(key.encode(self.env)) {
            return Some(found);
        }
        let atom = Atom::existing_from_str(self.env, key).ok()?;
        map.map_get(atom).ok()
    }

    /// How a term is read, and its value when it has no parts.
    fn shape(&mut self, term: Term<'a>) -> Read<Shape> {
        Ok(match term.get_type() {
            TermType::Atom => atom_value(term),
            TermType::Integer => Shape::Scalar(
                term.decode::<i64>()
                    .map_or(Value::Null(NullCause::IntOverflow), Value::Int),
            ),
            TermType::Float => Shape::Scalar(
                term.decode::<f64>()
                    .map_or_else(|_| Value::Null(NullCause::Null), Value::float),
            ),
            TermType::Binary => match Binary::from_term(term) {
                Ok(binary) => {
                    self.work(binary.len() / TEXT_BYTES)?;
                    match std::str::from_utf8(binary.as_slice()) {
                        Ok(text) => Shape::Scalar(Value::String(text.to_owned())),
                        Err(_) => Shape::Opaque("binary".to_owned()),
                    }
                }
                Err(_) => Shape::Opaque("bitstring".to_owned()),
            },
            TermType::List => match self.budget {
                // Within the budget, the list is walked only as far as it may
                // be read: a longer one goes to a dirty scheduler.
                Some(budget) => match bounded_length(term, budget.units) {
                    Length::Proper(len) => Shape::List(len),
                    Length::Improper => Shape::Opaque("improper_list".to_owned()),
                    Length::Longer => return Err(Stop::Budget),
                },
                None => match term.list_length() {
                    Ok(len) => Shape::List(len),
                    Err(_) => Shape::Opaque("improper_list".to_owned()),
                },
            },
            // A `%Decimal{}` with parts beyond 64 bits is read through the
            // external term format, whose cost the budget cannot bound.
            TermType::Map if self.budget.is_some() && big_decimal_parts(term) => {
                return Err(Stop::Budget);
            }
            TermType::Map => map_shape(term),
            TermType::Tuple => tuple_shape(term),
            TermType::Fun => Shape::Opaque("fun".to_owned()),
            TermType::Pid => Shape::Opaque("pid".to_owned()),
            TermType::Port => Shape::Opaque("port".to_owned()),
            TermType::Ref => Shape::Opaque("reference".to_owned()),
            TermType::Unknown => Shape::Opaque("term".to_owned()),
        })
    }
}

/// The elements of a proper list.
fn list_items(list: Term<'_>) -> Vec<Term<'_>> {
    // `list_length` succeeded on it, so the iterator never meets an improper tail.
    list.into_list_iterator()
        .map(Iterator::collect)
        .unwrap_or_default()
}

/// How long a list is, walked at most `limit` cells.
enum Length {
    Proper(usize),
    Improper,
    Longer,
}

fn bounded_length(list: Term<'_>, limit: usize) -> Length {
    let mut tail = list;
    let mut len = 0;
    loop {
        if tail.is_empty_list() {
            return Length::Proper(len);
        }
        match tail.list_get_cell() {
            Ok((_, rest)) => {
                len += 1;
                if len > limit {
                    return Length::Longer;
                }
                tail = rest;
            }
            Err(_) => return Length::Improper,
        }
    }
}

/// Whether a map is a `%Decimal{}` whose `coef` or `exp` is an integer beyond
/// 64 bits.
fn big_decimal_parts(map: Term<'_>) -> bool {
    let big = |name| {
        field(map, name).is_some_and(|part| {
            part.get_type() == TermType::Integer
                && part.decode::<i64>().is_err()
                && part.decode::<u64>().is_err()
        })
    };
    map.map_get(atoms::__struct__())
        .is_ok_and(|module| atoms::elixir_decimal() == module)
        && (big(atoms::coef()) || big(atoms::exp()))
}

fn atom_value(term: Term<'_>) -> Shape {
    if atoms::true_() == term {
        Shape::Scalar(Value::Bool(true))
    } else if atoms::false_() == term {
        Shape::Scalar(Value::Bool(false))
    } else if atoms::null() == term || atoms::undefined() == term || atoms::nil() == term {
        Shape::Scalar(Value::Null(NullCause::Null))
    } else {
        match term.atom_to_string() {
            Ok(name) => Shape::Scalar(Value::String(name)),
            Err(_) => Shape::Opaque("atom".to_owned()),
        }
    }
}

fn opaque(type_name: &str) -> Shape {
    Shape::Opaque(type_name.to_owned())
}

/// An integer that fits in an `i64`, else `None`.
fn int(term: Term<'_>) -> Option<i64> {
    if term.get_type() == TermType::Integer {
        term.decode::<i64>().ok()
    } else {
        None
    }
}

/// A valid `calendar` date, years 1 to 9999.
fn date(year: Term<'_>, month: Term<'_>, day: Term<'_>) -> Option<Date> {
    let year = int(year).filter(|y| (1..=9999).contains(y))?;
    Date::new(
        i16::try_from(year).ok()?,
        i8::try_from(int(month)?).ok()?,
        i8::try_from(int(day)?).ok()?,
    )
    .ok()
}

/// A valid date and time: hours 0 to 23, minutes and seconds 0 to 59 (no leap
/// seconds) and microseconds 0 to 999,999.
fn datetime(date: Date, hour: i64, minute: i64, second: i64, micro: i64) -> Option<DateTime> {
    if !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
        || !(0..=999_999).contains(&micro)
    {
        return None;
    }
    DateTime::new(
        date.year(),
        date.month(),
        date.day(),
        i8::try_from(hour).ok()?,
        i8::try_from(minute).ok()?,
        i8::try_from(second).ok()?,
        i32::try_from(micro * 1000).ok()?,
    )
    .ok()
}

/// `{{Y, Mo, D}, {H, Mi, S}}` or `{{Y, Mo, D}, {H, Mi, S, Micro}}`.
fn calendar_datetime(term: Term<'_>) -> Option<DateTime> {
    let [day, time] = get_tuple(term).ok()?[..] else {
        return None;
    };
    let [y, mo, d] = get_tuple(day).ok()?[..] else {
        return None;
    };
    let day = date(y, mo, d)?;
    match get_tuple(time).ok()?[..] {
        [h, mi, s] => datetime(day, int(h)?, int(mi)?, int(s)?, 0),
        [h, mi, s, micro] => datetime(day, int(h)?, int(mi)?, int(s)?, int(micro)?),
        _ => None,
    }
}

/// An aware datetime from its local date and time and its UTC offset, in the
/// zone `name` when the core's database agrees (*Time zones*); `None` when the
/// offset or the instant is out of range.
fn aware(local: DateTime, offset: i64, name: Option<&str>) -> Option<AwareDateTime> {
    if offset.abs() > MAX_OFFSET {
        return None;
    }
    let offset = Offset::from_seconds(i32::try_from(offset).ok()?).ok()?;
    let instant = offset.to_timestamp(local).ok()?;
    AwareDateTime::from_native(instant, offset, name).ok()
}

fn tuple_shape(term: Term<'_>) -> Shape {
    let Ok(items) = get_tuple(term) else {
        return opaque("tuple");
    };
    let value = match items[..] {
        [tag, text] if atoms::cleanrules_float() == tag => {
            match Binary::from_term(text).map(|b| b.as_slice()) {
                Ok(b"inf") => Some(Value::Float(f64::INFINITY)),
                Ok(b"-inf") => Some(Value::Float(f64::NEG_INFINITY)),
                _ => None,
            }
        }
        [tag, text] if atoms::cleanrules_decimal() == tag => Binary::from_term(text)
            .ok()
            .and_then(|b| std::str::from_utf8(b.as_slice()).ok().map(Decimal::parse))
            .and_then(|parsed| match parsed {
                Ok(decimal) => Some(Value::Decimal(decimal)),
                Err(DecimalError::DoesNotFit) => Some(Value::Null(NullCause::DecimalOverflow)),
                Err(DecimalError::Malformed) => None,
            }),
        [tag, local, offset, zone] if atoms::cleanrules_datetime() == tag => {
            tagged_datetime(local, offset, zone)
        }
        [y, mo, d] => date(y, mo, d).map(Value::Date),
        [_, _] => calendar_datetime(term).map(Value::NaiveDateTime),
        _ => None,
    };
    value.map_or_else(|| opaque("tuple"), Shape::Scalar)
}

/// `{cleanrules_datetime, Local, Offset, Zone}`.
fn tagged_datetime(local: Term<'_>, offset: Term<'_>, zone: Term<'_>) -> Option<Value> {
    let local = calendar_datetime(local)?;
    let offset = int(offset)?;
    let name = if atoms::undefined() == zone {
        None
    } else {
        let binary = Binary::from_term(zone).ok()?;
        Some(std::str::from_utf8(binary.as_slice()).ok()?.to_owned())
    };
    aware(local, offset, name.as_deref()).map(Value::AwareDateTime)
}

/// A map, or an Elixir struct: a map with the atom key `__struct__` whose
/// value is an atom, the module.
fn map_shape(term: Term<'_>) -> Shape {
    let Some(module) = term
        .map_get(atoms::__struct__())
        .ok()
        .filter(|m| m.is_atom())
    else {
        return Shape::Map { strip: false };
    };
    let name = || {
        module
            .atom_to_string()
            .unwrap_or_else(|_| "struct".to_owned())
    };
    if term
        .map_get(atoms::__exception__())
        .is_ok_and(|e| atoms::true_() == e)
    {
        return Shape::Opaque(name());
    }
    let value = if atoms::elixir_decimal() == module {
        elixir_decimal(term)
    } else if atoms::elixir_date() == module {
        iso(term).and_then(|()| struct_date(term)).map(Value::Date)
    } else if atoms::elixir_naive_datetime() == module {
        iso(term)
            .and_then(|()| struct_datetime(term))
            .map(Value::NaiveDateTime)
    } else if atoms::elixir_datetime() == module {
        iso(term).and_then(|()| elixir_datetime(term))
    } else if [
        atoms::elixir_map_set(),
        atoms::elixir_range(),
        atoms::elixir_regex(),
        atoms::elixir_time(),
        atoms::elixir_duration(),
    ]
    .iter()
    .any(|m| *m == module)
    {
        None
    } else {
        return Shape::Map { strip: true };
    };
    value.map_or_else(|| Shape::Opaque(name()), Shape::Scalar)
}

/// `Some(())` when the struct's calendar is `Calendar.ISO`.
fn iso(term: Term<'_>) -> Option<()> {
    term.map_get(atoms::calendar())
        .ok()
        .filter(|c| atoms::elixir_calendar_iso() == *c)
        .map(|_| ())
}

fn field(term: Term<'_>, name: Atom) -> Option<Term<'_>> {
    term.map_get(name).ok()
}

fn struct_date(term: Term<'_>) -> Option<Date> {
    date(
        field(term, atoms::year())?,
        field(term, atoms::month())?,
        field(term, atoms::day())?,
    )
}

/// The date and time of a `%NaiveDateTime{}` or `%DateTime{}`, whose
/// `microsecond` is `{Value, Precision}`, the precision 0 to 6.
fn struct_datetime(term: Term<'_>) -> Option<DateTime> {
    let day = struct_date(term)?;
    let [micro, precision] = get_tuple(field(term, atoms::microsecond())?).ok()?[..] else {
        return None;
    };
    int(precision).filter(|p| (0..=6).contains(p))?;
    datetime(
        day,
        int(field(term, atoms::hour())?)?,
        int(field(term, atoms::minute())?)?,
        int(field(term, atoms::second())?)?,
        int(micro)?,
    )
}

/// A `%DateTime{}`: its local date and time, at `utc_offset` plus
/// `std_offset`, in `time_zone`; `Etc/UTC` with offset 0 is `Z`.
fn elixir_datetime(term: Term<'_>) -> Option<Value> {
    let local = struct_datetime(term)?;
    let zone = Binary::from_term(field(term, atoms::time_zone())?).ok()?;
    let zone = std::str::from_utf8(zone.as_slice()).ok()?;
    let offset = int(field(term, atoms::utc_offset())?)?
        .checked_add(int(field(term, atoms::std_offset())?)?)?;
    let value = if zone == "Etc/UTC" && offset == 0 {
        let instant = Offset::UTC.to_timestamp(local).ok()?;
        AwareDateTime::new(instant, Zone::Fixed(Offset::UTC)).ok()?
    } else {
        aware(local, offset, Some(zone))?
    };
    Some(Value::AwareDateTime(value))
}

/// A `%Decimal{}`: `sign × coef × 10^exp` (`spec/data-model.md`, *Erlang*).
fn elixir_decimal(term: Term<'_>) -> Option<Value> {
    let negative = match int(field(term, atoms::sign())?)? {
        1 => false,
        -1 => true,
        _ => return None,
    };
    let coef = field(term, atoms::coef())?;
    let exp = field(term, atoms::exp())?;
    if exp.get_type() != TermType::Integer {
        return None;
    }
    if coef.is_atom() {
        return if atoms::inf() == coef {
            Some(Value::Decimal(if negative {
                Decimal::NEG_INFINITY
            } else {
                Decimal::INFINITY
            }))
        } else if [atoms::nan(), atoms::qnan(), atoms::snan()]
            .iter()
            .any(|n| *n == coef)
        {
            Some(Value::Null(NullCause::Null))
        } else {
            None
        };
    }
    let digits = match coef.get_type() {
        TermType::Integer => match coef.decode::<u64>() {
            Ok(small) => small.to_string(),
            Err(_) => match bignum::from_external(coef.to_binary().as_slice())? {
                Integer::Digits(false, digits) => digits,
                Integer::Digits(true, _) => return None,
                Integer::TooLong => return Some(Value::Null(NullCause::DecimalOverflow)),
            },
        },
        _ => return None,
    };
    let zero = digits == "0";
    let exp = match exp.decode::<i64>() {
        // Zero is zero at any exponent: only its scale, at most 38, depends on it.
        Ok(exp) if zero => exp.clamp(-100, 100),
        Ok(exp) => exp,
        // An exponent beyond 64 bits: zero, or a value that cannot fit.
        Err(_) if zero => {
            if exp.decode::<u64>().is_ok()
                || matches!(
                    bignum::from_external(exp.to_binary().as_slice()),
                    Some(Integer::Digits(false, _))
                )
            {
                100
            } else {
                -100
            }
        }
        Err(_) => return Some(Value::Null(NullCause::DecimalOverflow)),
    };
    let sign = if negative { "-" } else { "" };
    match Decimal::parse(&format!("{sign}{digits}E{exp}")) {
        Ok(decimal) => Some(Value::Decimal(decimal)),
        Err(DecimalError::DoesNotFit) => Some(Value::Null(NullCause::DecimalOverflow)),
        Err(DecimalError::Malformed) => None,
    }
}
