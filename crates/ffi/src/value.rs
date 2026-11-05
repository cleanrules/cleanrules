//! Native values (`spec/data-model.md`): built piece by piece from C data,
//! read back from outputs, and converted to and from JSON.
//!
//! A `cr_value *` is a boxed core `Value`. Children are built first and moved
//! into their list or map; a pointer read from inside a value
//! (`cr_value_list_get`, `cr_value_map_entry`, `cr_value_map_get`) borrows
//! from it until the value is changed, moved or freed.

use std::ffi::c_char;
use std::ptr;

use cleanrules::format::{ValueContext, read_value, write_value};
use cleanrules::jiff::civil::{Date, DateTime, Time};
use cleanrules::jiff::tz::{Offset, TimeZone};
use cleanrules::jiff::{Timestamp, ToSpan as _};
use cleanrules::json::{Limits, parse_bytes, to_string};
use cleanrules::value::{AwareDateTime, DecimalError, Opaque, Zone, check_date};
use cleanrules::{Decimal, Error, ErrorCode, Map, NullCause, Value};

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, clear_borrowed, free, give_text, handle, handle_mut,
    limits_of, utf8_arg, write_out,
};
use crate::cr_status;
use crate::error::cr_error;
use crate::limits::cr_limits;

/// A value: a null with a reason, a boolean, a number, a string, a date or
/// datetime, a list, a map or an opaque value.
pub struct cr_value {
    _opaque: [u8; 0],
}

/// What a value is (`CR_VALUE_*`).
pub type cr_value_type = i32;

/// A null, with its reason.
pub const CR_VALUE_NULL: cr_value_type = 1;
/// `true` or `false`.
pub const CR_VALUE_BOOL: cr_value_type = 2;
/// A signed 64-bit integer.
pub const CR_VALUE_INT: cr_value_type = 3;
/// A binary64 float, never NaN.
pub const CR_VALUE_FLOAT: cr_value_type = 4;
/// An exact decimal of up to 38 digits.
pub const CR_VALUE_DECIMAL: cr_value_type = 5;
/// A UTF-8 string.
pub const CR_VALUE_STRING: cr_value_type = 6;
/// A day, years 1 to 9999.
pub const CR_VALUE_DATE: cr_value_type = 7;
/// A date and a time of day, to the microsecond, without a zone.
pub const CR_VALUE_NAIVE_DATETIME: cr_value_type = 8;
/// An instant with a fixed offset or an IANA time zone.
pub const CR_VALUE_AWARE_DATETIME: cr_value_type = 9;
/// A list.
pub const CR_VALUE_LIST: cr_value_type = 10;
/// A map, or a record (`cr_value_is_record`).
pub const CR_VALUE_MAP: cr_value_type = 11;
/// A native value with no equivalent: a type name and a token.
pub const CR_VALUE_OPAQUE: cr_value_type = 12;

/// Why a value is null (`spec/data-model.md`, *Nulls with a reason*).
pub type cr_null_reason = i32;

/// A plain null (a native null, or a NaN).
pub const CR_REASON_NULL: cr_null_reason = 1;
/// A native integer outside 64 bits.
pub const CR_REASON_INT_OVERFLOW: cr_null_reason = 2;
/// A native decimal that does not fit in 38 digits.
pub const CR_REASON_DECIMAL_OVERFLOW: cr_null_reason = 3;
/// Reading the native value failed; the null keeps the message.
pub const CR_REASON_READ_ERROR: cr_null_reason = 4;

/// A new handle for a value.
fn boxed(value: Value) -> *mut cr_value {
    Box::into_raw(Box::new(value)).cast()
}

/// The value behind a handle.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`.
unsafe fn value_of<'a>(value: *const cr_value, what: &str) -> Outcome<&'a Value> {
    // SAFETY: a `cr_value *` is a `Value *` (see `boxed`); the caller
    // promises it is live when it is not null.
    unsafe { handle(value.cast::<Value>(), what) }
}

/// Take a value out of its handle, freeing the handle.
///
/// # Safety
///
/// `value` is `NULL` or a `cr_value` from `boxed`, owned by the caller, which
/// gives it up.
unsafe fn take(value: *mut cr_value, what: &str) -> Outcome<Value> {
    if value.is_null() {
        return Err(Failure::Invalid(
            format!("{what} is NULL"),
            "pass a value made by a cr_value_new_* function",
        ));
    }
    // SAFETY: the caller gives up a box made by `boxed`.
    Ok(*unsafe { Box::from_raw(value.cast::<Value>()) })
}

/// Hand a value to the caller.
///
/// # Safety
///
/// `out` passed `clear`.
unsafe fn give_value(out: *mut *mut cr_value, value: Value) {
    // SAFETY: `clear` checked that `out` is writable.
    unsafe { *out = boxed(value) };
}

/// How deep lists and maps nest in a value: 0 for a scalar, 1 for `[]`, as
/// JSON nesting counts (`spec/json-format.md`, *Errors and limits*).
pub(crate) fn nesting(value: &Value) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(value, 0)];
    while let Some((value, depth)) = stack.pop() {
        let children: Box<dyn Iterator<Item = &Value>> = match value {
            Value::List(items) => Box::new(items.iter()),
            Value::Map(map) => Box::new(map.iter().map(|(_, v)| v)),
            _ => continue,
        };
        deepest = deepest.max(depth + 1);
        stack.extend(children.map(|child| (child, depth + 1)));
    }
    deepest
}

/// A value nested deeper than JSON allows by default cannot be built: freeing,
/// copying and evaluating it recurse.
fn check_nesting(item: &Value) -> Outcome {
    let limit = Limits::default().max_nesting;
    let depth = nesting(item) + 1;
    if depth > limit {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            format!("the value would nest lists and maps {depth} deep, more than {limit}"),
        )
        .with_hint("flatten the data, or convert only the paths the rule set reads")
        .into());
    }
    Ok(())
}

pub(crate) fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null(_) => "a null",
        Value::Bool(_) => "a boolean",
        Value::Int(_) => "an int",
        Value::Float(_) => "a float",
        Value::Decimal(_) => "a decimal",
        Value::String(_) => "a string",
        Value::Date(_) => "a date",
        Value::NaiveDateTime(_) => "a naive datetime",
        Value::AwareDateTime(_) => "an aware datetime",
        Value::List(_) => "a list",
        Value::Map(_) => "a map",
        Value::Opaque(_) => "an opaque value",
    }
}

fn wrong(value: &Value, wanted: &str) -> Failure {
    Failure::Invalid(
        format!("the value is {}, not {wanted}", type_name(value)),
        "check the value's kind first (cr_value_kind)",
    )
}

/// The datetimes C cannot hand over: the program decides what to build
/// (`spec/data-model.md`, *Reading native data*).
fn unrepresentable(what: String) -> Failure {
    Failure::Invalid(
        what,
        "a value holds years 1 to 9999 and offsets within 18 hours; build an opaque value with \
         cr_value_new_opaque and your own token instead",
    )
}

/// A plain null.
#[unsafe(no_mangle)]
pub extern "C" fn cr_value_new_null() -> *mut cr_value {
    boxed(Value::Null(NullCause::Null))
}

/// A boolean.
#[unsafe(no_mangle)]
pub extern "C" fn cr_value_new_bool(value: bool) -> *mut cr_value {
    boxed(Value::Bool(value))
}

/// An int.
#[unsafe(no_mangle)]
pub extern "C" fn cr_value_new_int(value: i64) -> *mut cr_value {
    boxed(Value::Int(value))
}

/// A float; NaN is a plain null.
#[unsafe(no_mangle)]
pub extern "C" fn cr_value_new_float(value: f64) -> *mut cr_value {
    boxed(Value::float(value))
}

/// A list of `len` plain nulls, set with `cr_value_list_set`: positions that
/// are never set stay null, as unread positions of native data do. `NULL` when
/// `len` items cannot be allocated.
#[unsafe(no_mangle)]
pub extern "C" fn cr_value_new_list(len: usize) -> *mut cr_value {
    let mut items = Vec::new();
    if items.try_reserve_exact(len).is_err() {
        return ptr::null_mut();
    }
    items.resize(len, Value::Null(NullCause::Null));
    boxed(Value::List(items))
}

/// An empty map, filled with `cr_value_map_insert`.
#[unsafe(no_mangle)]
pub extern "C" fn cr_value_new_map() -> *mut cr_value {
    boxed(Value::Map(Map::new()))
}

/// An empty record: a map whose entries also have positions, their order of
/// insertion (`spec/ast.md`, *Reading data*).
#[unsafe(no_mangle)]
pub extern "C" fn cr_value_new_record() -> *mut cr_value {
    boxed(Value::Map(Map::record()))
}

/// A null with a reason (`CR_REASON_*`); `detail`, the message of a
/// `CR_REASON_READ_ERROR`, is ignored for the others.
///
/// # Safety
///
/// `detail` is `NULL` or points to `len` readable bytes; `out` is `NULL` or
/// points to a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_new_null_reason(
    reason: cr_null_reason,
    detail: *const c_char,
    len: usize,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let cause = match reason {
            CR_REASON_NULL => NullCause::Null,
            CR_REASON_INT_OVERFLOW => NullCause::IntOverflow,
            CR_REASON_DECIMAL_OVERFLOW => NullCause::DecimalOverflow,
            CR_REASON_READ_ERROR => {
                // SAFETY: `detail` and `len` (# Safety).
                let detail = unsafe { bytes(detail, len, "detail") }?;
                NullCause::ReadError(utf8_arg(detail, "detail")?.to_owned())
            }
            other => {
                return Err(Failure::Invalid(
                    format!("reason {other} is not a CR_REASON_* constant"),
                    "use CR_REASON_NULL (1) to CR_REASON_READ_ERROR (4)",
                ));
            }
        };
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, Value::Null(cause)) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_new_null_reason", error, body) }
}

/// A decimal from its text (`-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?`, `inf` or
/// `-inf`); one that does not fit in 38 digits is a null with
/// `CR_REASON_DECIMAL_OVERFLOW`, as in data. Other text is a `CR_FORMAT_ERROR`.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes; `out` is `NULL` or
/// points to a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_new_decimal(
    text: *const c_char,
    len: usize,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `text` and `len` (# Safety).
        let text = utf8_arg(unsafe { bytes(text, len, "text") }?, "text")?;
        let value = match Decimal::parse(text) {
            Ok(decimal) => Value::Decimal(decimal),
            Err(DecimalError::DoesNotFit) => Value::Null(NullCause::DecimalOverflow),
            Err(DecimalError::Malformed) => {
                return Err(Failure::Argument(
                    Error::new(ErrorCode::FormatError, format!("{text:?} is not a decimal"))
                        .with_hint("write digits with an optional point and exponent: 0.15, 1e3"),
                ));
            }
        };
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, value) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_new_decimal", error, body) }
}

/// A string, copied. Bytes that are not UTF-8 are a `CR_FORMAT_ERROR`.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes; `out` is `NULL` or
/// points to a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_new_string(
    text: *const c_char,
    len: usize,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `text` and `len` (# Safety).
        let text = utf8_arg(unsafe { bytes(text, len, "text") }?, "text")?;
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, Value::String(text.to_owned())) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_new_string", error, body) }
}

/// A day of the proleptic Gregorian calendar, years 1 to 9999.
fn civil_date(year: i32, month: i32, day: i32) -> Outcome<Date> {
    let shown = format!("{year:04}-{month:02}-{day:02}");
    let invalid = || {
        Failure::Invalid(
            format!("{shown} is not a day of the calendar"),
            "pass a month from 1 to 12 and a day of that month",
        )
    };
    let month = i8::try_from(month).map_err(|_| invalid())?;
    let day = i8::try_from(day).map_err(|_| invalid())?;
    // The same year of the 400-year cycle has the same leap years.
    let same = i16::try_from(2000 + year.rem_euclid(400)).map_err(|_| invalid())?;
    Date::new(same, month, day).map_err(|_| invalid())?;
    i16::try_from(year)
        .ok()
        .and_then(|year| Date::new(year, month, day).ok())
        .and_then(|date| check_date(date).ok())
        .ok_or_else(|| unrepresentable(format!("{shown} is outside years 1 to 9999")))
}

/// A date, years 1 to 9999. Another day of the calendar, or an impossible one,
/// is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `cr_value *`; `error` is `NULL` or
/// points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_new_date(
    year: i32,
    month: i32,
    day: i32,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let date = civil_date(year, month, day)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, Value::Date(date)) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_new_date", error, body) }
}

/// A datetime without a zone, years 1 to 9999, to the microsecond (0 to
/// 999999). Anything else is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `cr_value *`; `error` is `NULL` or
/// points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // The parts of a datetime.
pub unsafe extern "C" fn cr_value_new_naive_datetime(
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
    microsecond: i32,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let date = civil_date(year, month, day)?;
        let invalid = || {
            Failure::Invalid(
                format!("{hour:02}:{minute:02}:{second:02}.{microsecond:06} is not a time of day"),
                "pass hours 0-23, minutes and seconds 0-59, microseconds 0-999999",
            )
        };
        let part = |n: i32| i8::try_from(n).map_err(|_| invalid());
        let nanos = microsecond
            .checked_mul(1000)
            .filter(|n| (0..1_000_000_000).contains(n))
            .ok_or_else(invalid)?;
        let time =
            Time::new(part(hour)?, part(minute)?, part(second)?, nanos).map_err(|_| invalid())?;
        let value = Value::NaiveDateTime(DateTime::from_parts(date, time));
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, value) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_new_naive_datetime", error, body) }
}

/// An aware datetime: an instant (`seconds` since 1970-01-01T00:00:00Z, and
/// `microsecond`, 0 to 999999, after it), its UTC offset at that instant, in
/// seconds, and its IANA zone's name when it has one (`NULL` with a length of 0
/// otherwise). The zone is kept when the bundled database knows the name,
/// spelled as it spells it, with that offset; otherwise the value keeps the
/// offset (`spec/data-model.md`, *Time zones*). An instant outside years 1 to
/// 9999, an offset beyond 18 hours or microseconds outside 0 to 999999 are
/// `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `zone` is `NULL` or points to `len` readable bytes; `out` is `NULL` or
/// points to a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_new_aware_datetime(
    seconds: i64,
    microsecond: i32,
    offset_seconds: i32,
    zone: *const c_char,
    len: usize,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `zone` and `len` (# Safety).
        let zone = utf8_arg(unsafe { bytes(zone, len, "zone") }?, "zone")?;
        if !(0..1_000_000).contains(&microsecond) {
            return Err(Failure::Invalid(
                format!("{microsecond} microseconds is outside 0 to 999999"),
                "carry whole seconds into `seconds`",
            ));
        }
        let offset = Offset::from_seconds(offset_seconds)
            .ok()
            .filter(|o| o.seconds().unsigned_abs() <= 18 * 3600)
            .ok_or_else(|| {
                unrepresentable(format!(
                    "an offset of {offset_seconds} seconds is beyond 18 hours"
                ))
            })?;
        let outside = || unrepresentable(format!("{seconds} seconds is outside years 1 to 9999"));
        let instant = Timestamp::from_second(seconds)
            .and_then(|t| t.checked_add(i64::from(microsecond).microseconds()))
            .map_err(|_| outside())?;
        let aware = AwareDateTime::from_native(instant, offset, (!zone.is_empty()).then_some(zone))
            .map_err(|_| outside())?;
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, Value::AwareDateTime(aware)) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_new_aware_datetime", error, body) }
}

/// An opaque value: a native value with no equivalent, by its type's name and a
/// token the program chooses, such as an index into its own table. Equal
/// tokens mean the same object (`ANY` compares them); an output gives the token
/// back, and the library never reads it.
///
/// # Safety
///
/// `type_name` is `NULL` or points to `len` readable bytes; `out` is `NULL` or
/// points to a writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_new_opaque(
    type_name: *const c_char,
    len: usize,
    token: u64,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `type_name` and `len` (# Safety).
        let type_name = utf8_arg(unsafe { bytes(type_name, len, "type_name") }?, "type_name")?;
        let value = Value::Opaque(Opaque {
            type_name: type_name.to_owned(),
            token,
        });
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, value) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_new_opaque", error, body) }
}

/// Take the item out of its handle for a parent, unless it is the parent.
///
/// # Safety
///
/// `item` is `NULL` or a `cr_value` owned by the caller.
unsafe fn take_item(item: *mut cr_value, parent: *mut cr_value) -> Outcome<Value> {
    if ptr::eq(item, parent) {
        return Err(Failure::Invalid(
            "the item is the value it would go into".into(),
            "build the item as a separate value",
        ));
    }
    // SAFETY: the caller gives up `item`.
    let item = unsafe { take(item, "item") }?;
    check_nesting(&item)?;
    Ok(item)
}

/// Move `item` into position `index` of `list`. `item` is consumed, even when
/// the call fails, unless it is `list` itself, which is refused. Pointers
/// borrowed from `list` are no longer valid afterwards.
///
/// # Safety
///
/// `list` is `NULL` or a live `cr_value` owned by the caller, not inside
/// another value, which no other thread uses during the call; `item` is `NULL`
/// or a `cr_value` owned by the caller, not inside another value; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_list_set(
    list: *mut cr_value,
    index: usize,
    item: *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `item` is NULL or owned by the caller, who gives it up.
        let item = unsafe { take_item(item, list) }?;
        // SAFETY: `list` is NULL or live and used by this call only (# Safety).
        let list = unsafe { handle_mut(list.cast::<Value>(), "list") }?;
        let Value::List(items) = list else {
            return Err(wrong(list, "a list"));
        };
        let len = items.len();
        let slot = items.get_mut(index).ok_or_else(|| {
            Failure::Invalid(
                format!("index {index} is out of the list of {len}"),
                "make the list long enough with cr_value_new_list",
            )
        })?;
        *slot = item;
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_list_set", error, body) }
}

/// Move `item` into `map` under `key`, replacing a value under the same key in
/// place. `item` is consumed, even when the call fails, unless it is `map`
/// itself, which is refused. Pointers borrowed from `map` are no longer valid
/// afterwards.
///
/// # Safety
///
/// `map` is `NULL` or a live `cr_value` owned by the caller, not inside another
/// value, which no other thread uses during the call; `key` is `NULL` or points
/// to `len` readable bytes; `item` is `NULL` or a `cr_value` owned by the
/// caller, not inside another value; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_map_insert(
    map: *mut cr_value,
    key: *const c_char,
    len: usize,
    item: *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `item` is NULL or owned by the caller, who gives it up.
        let item = unsafe { take_item(item, map) }?;
        // SAFETY: `key` and `len` (# Safety).
        let key = utf8_arg(unsafe { bytes(key, len, "key") }?, "key")?;
        // SAFETY: `map` is NULL or live and used by this call only (# Safety).
        let map = unsafe { handle_mut(map.cast::<Value>(), "map") }?;
        let Value::Map(entries) = map else {
            return Err(wrong(map, "a map"));
        };
        entries.insert(key, item);
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_map_insert", error, body) }
}

/// Read a value from JSON (`spec/json-format.md`, *Values*), as data. Writing
/// a value and reading it back is not always the same value: null reasons are
/// written as plain nulls, and opaque values cannot be read.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes; `limits` is `NULL` or a
/// live `cr_limits`; `out` is `NULL` or points to a writable `cr_value *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_from_json(
    text: *const c_char,
    len: usize,
    limits: *const cr_limits,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        // SAFETY: `text` and `len` (# Safety).
        let json = parse_bytes(unsafe { bytes(text, len, "text") }?, &limits)?;
        let value = read_value(&json, "", ValueContext::Data)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, value) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_from_json", error, body) }
}

/// A deep copy of a value, freed with `cr_value_free`: the same value, null
/// reasons and opaque tokens included, which JSON would not keep.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_clone(
    value: *const cr_value,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?.clone();
        // SAFETY: `out` passed `clear`.
        unsafe { give_value(out, value) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_clone", error, body) }
}

/// Write a value as JSON (`spec/json-format.md`, *Values*), freed with
/// `cr_string_free`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_to_json(
    value: *const cr_value,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, to_string(&write_value(value))) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_to_json", error, body) }
}

/// What a value is (`CR_VALUE_*`); 0 for `NULL`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_kind(value: *const cr_value) -> cr_value_type {
    // SAFETY: `value` is NULL or live (# Safety).
    match unsafe { value.cast::<Value>().as_ref() } {
        None => 0,
        Some(Value::Null(_)) => CR_VALUE_NULL,
        Some(Value::Bool(_)) => CR_VALUE_BOOL,
        Some(Value::Int(_)) => CR_VALUE_INT,
        Some(Value::Float(_)) => CR_VALUE_FLOAT,
        Some(Value::Decimal(_)) => CR_VALUE_DECIMAL,
        Some(Value::String(_)) => CR_VALUE_STRING,
        Some(Value::Date(_)) => CR_VALUE_DATE,
        Some(Value::NaiveDateTime(_)) => CR_VALUE_NAIVE_DATETIME,
        Some(Value::AwareDateTime(_)) => CR_VALUE_AWARE_DATETIME,
        Some(Value::List(_)) => CR_VALUE_LIST,
        Some(Value::Map(_)) => CR_VALUE_MAP,
        Some(Value::Opaque(_)) => CR_VALUE_OPAQUE,
    }
}

/// Write a borrowed text through optional out-parameters; an empty text points
/// to a static empty string, never to a dangling address.
///
/// # Safety
///
/// `out` and `len` are `NULL` or point to a writable `const char *` and
/// `size_t`.
pub(crate) unsafe fn borrow_text(text: Option<&str>, out: *mut *const c_char, len: *mut usize) {
    if !out.is_null() {
        let pointer = match text {
            None => ptr::null(),
            Some("") => c"".as_ptr(),
            Some(text) => text.as_ptr().cast::<c_char>(),
        };
        // SAFETY: the caller promises that a non-null `out` is writable.
        unsafe { *out = pointer };
    }
    if !len.is_null() {
        // SAFETY: the caller promises that a non-null `len` is writable.
        unsafe { *len = text.map_or(0, str::len) };
    }
}

/// A null's reason (`CR_REASON_*`) and, for `CR_REASON_READ_ERROR`, its
/// message, borrowed from the value and not NUL-terminated (`NULL` with a
/// length of 0 for the others). Every out-parameter may be `NULL`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `reason` is `NULL` or points to a
/// writable `cr_null_reason`; `detail` and `len` are `NULL` or point to a
/// writable `const char *` and `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_null(
    value: *const cr_value,
    reason: *mut cr_null_reason,
    detail: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `detail` and `len` are NULL or writable (# Safety).
        unsafe { clear_borrowed(detail, len) };
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::Null(cause) = value else {
            return Err(wrong(value, "a null"));
        };
        let (code, text) = match cause {
            NullCause::Null => (CR_REASON_NULL, None),
            NullCause::IntOverflow => (CR_REASON_INT_OVERFLOW, None),
            NullCause::DecimalOverflow => (CR_REASON_DECIMAL_OVERFLOW, None),
            NullCause::ReadError(detail) => (CR_REASON_READ_ERROR, Some(detail.as_str())),
        };
        if !reason.is_null() {
            // SAFETY: a non-null `reason` is writable (# Safety).
            unsafe { *reason = code };
        }
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe { borrow_text(text, detail, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_get_null", error, body) }
}

/// Read one scalar of a value through `out`: `read` gives it, or `None` when
/// the value is not `wanted`.
///
/// # Safety
///
/// As the exported getters below.
unsafe fn get<T>(
    name: &str,
    value: *const cr_value,
    out: *mut T,
    error: *mut *mut cr_error,
    wanted: &str,
    read: impl FnOnce(&Value) -> Option<T>,
) -> cr_status {
    let body = || {
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let read = read(value).ok_or_else(|| wrong(value, wanted))?;
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { write_out(out, read, "out") }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(name, error, body) }
}

// The getters are written out, not made by a macro, so that cbindgen sees
// them and the header declares them.

/// A boolean's value.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` points to a writable `bool`
/// (`NULL` is `CR_INVALID_ARGUMENT`); `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_bool(
    value: *const cr_value,
    out: *mut bool,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `get`'s.
    unsafe {
        get(
            "cr_value_get_bool",
            value,
            out,
            error,
            "a boolean",
            |v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            },
        )
    }
}

/// An int's value.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` points to a writable
/// `int64_t` (`NULL` is `CR_INVALID_ARGUMENT`); `error` is `NULL` or points to
/// a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_int(
    value: *const cr_value,
    out: *mut i64,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `get`'s.
    unsafe {
        get(
            "cr_value_get_int",
            value,
            out,
            error,
            "an int",
            |v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            },
        )
    }
}

/// A float's value.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` points to a writable
/// `double` (`NULL` is `CR_INVALID_ARGUMENT`); `error` is `NULL` or points to
/// a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_float(
    value: *const cr_value,
    out: *mut f64,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `get`'s.
    unsafe {
        get(
            "cr_value_get_float",
            value,
            out,
            error,
            "a float",
            |v| match v {
                Value::Float(f) => Some(*f),
                _ => None,
            },
        )
    }
}

/// An opaque value's token.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` points to a writable
/// `uint64_t` (`NULL` is `CR_INVALID_ARGUMENT`); `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_opaque_token(
    value: *const cr_value,
    out: *mut u64,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `get`'s.
    unsafe {
        get(
            "cr_value_get_opaque_token",
            value,
            out,
            error,
            "an opaque value",
            |v| match v {
                Value::Opaque(o) => Some(o.token),
                _ => None,
            },
        )
    }
}

/// A decimal's canonical text (`spec/json-format.md`, *Numbers*), such as
/// `0.15`: an owned text, freed with `cr_string_free`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` is `NULL` or points to a
/// writable `char *`; `len` is `NULL` or points to a writable `size_t`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_decimal(
    value: *const cr_value,
    out: *mut *mut c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::Decimal(decimal) = value else {
            return Err(wrong(value, "a decimal"));
        };
        // SAFETY: `out` passed `clear`; `len` is NULL or writable (# Safety).
        unsafe { give_text(out, len, decimal.to_string()) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_get_decimal", error, body) }
}

/// A string's bytes, UTF-8, borrowed from the value and not NUL-terminated.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `text` and `len` are `NULL` or point
/// to a writable `const char *` and `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_string(
    value: *const cr_value,
    text: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `text` and `len` are NULL or writable (# Safety).
        unsafe { clear_borrowed(text, len) };
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::String(string) = value else {
            return Err(wrong(value, "a string"));
        };
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe { borrow_text(Some(string), text, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_get_string", error, body) }
}

/// An opaque value's type name, borrowed from the value and not NUL-terminated.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `text` and `len` are `NULL` or point
/// to a writable `const char *` and `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_opaque_type(
    value: *const cr_value,
    text: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `text` and `len` are NULL or writable (# Safety).
        unsafe { clear_borrowed(text, len) };
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::Opaque(opaque) = value else {
            return Err(wrong(value, "an opaque value"));
        };
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe { borrow_text(Some(&opaque.type_name), text, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_get_opaque_type", error, body) }
}

/// Write the parts of a datetime through optional out-parameters.
///
/// # Safety
///
/// Each out-parameter is `NULL` or points to a writable `int32_t`.
unsafe fn write_parts(parts: &[(*mut i32, i32)]) {
    for &(out, part) in parts {
        if !out.is_null() {
            // SAFETY: the caller promises that a non-null `out` is writable.
            unsafe { *out = part };
        }
    }
}

/// The parts of a date. Every out-parameter may be `NULL`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; each out-parameter is `NULL` or
/// points to a writable `int32_t`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_date(
    value: *const cr_value,
    year: *mut i32,
    month: *mut i32,
    day: *mut i32,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::Date(date) = value else {
            return Err(wrong(value, "a date"));
        };
        let parts = [
            (year, i32::from(date.year())),
            (month, i32::from(date.month())),
            (day, i32::from(date.day())),
        ];
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe { write_parts(&parts) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_get_date", error, body) }
}

/// The parts of a datetime without a zone. Every out-parameter may be `NULL`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; each out-parameter is `NULL` or
/// points to a writable `int32_t`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // The parts of a datetime.
pub unsafe extern "C" fn cr_value_get_naive_datetime(
    value: *const cr_value,
    year: *mut i32,
    month: *mut i32,
    day: *mut i32,
    hour: *mut i32,
    minute: *mut i32,
    second: *mut i32,
    microsecond: *mut i32,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::NaiveDateTime(datetime) = value else {
            return Err(wrong(value, "a naive datetime"));
        };
        let parts = [
            (year, i32::from(datetime.year())),
            (month, i32::from(datetime.month())),
            (day, i32::from(datetime.day())),
            (hour, i32::from(datetime.hour())),
            (minute, i32::from(datetime.minute())),
            (second, i32::from(datetime.second())),
            (microsecond, datetime.subsec_nanosecond() / 1000),
        ];
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe { write_parts(&parts) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_get_naive_datetime", error, body) }
}

/// An aware datetime's instant (`seconds` since 1970-01-01T00:00:00Z, and
/// `microsecond`, 0 to 999999, after it, also before 1970), its UTC offset at
/// that instant, in seconds, and its IANA zone's name, borrowed from the value
/// and not NUL-terminated (`NULL` with a length of 0 for a fixed offset). Every
/// out-parameter may be `NULL`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; each out-parameter is `NULL` or
/// points to a writable value of its type; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_get_aware_datetime(
    value: *const cr_value,
    seconds: *mut i64,
    microsecond: *mut i32,
    offset_seconds: *mut i32,
    zone: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `zone` and `len` are NULL or writable (# Safety).
        unsafe { clear_borrowed(zone, len) };
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::AwareDateTime(aware) = value else {
            return Err(wrong(value, "an aware datetime"));
        };
        let instant = aware.timestamp();
        let (offset, name) = match aware.zone() {
            Zone::Fixed(offset) => (*offset, None),
            Zone::Named(name) => {
                let zone = TimeZone::get(name).unwrap_or(TimeZone::UTC);
                (zone.to_offset(instant), Some(name.as_str()))
            }
        };
        // Floored: the microseconds count forward from the second before.
        let (mut whole, mut micro) = (instant.as_second(), instant.subsec_microsecond());
        if micro < 0 {
            whole -= 1;
            micro += 1_000_000;
        }
        if !seconds.is_null() {
            // SAFETY: a non-null out-parameter is writable (# Safety).
            unsafe { *seconds = whole };
        }
        if !microsecond.is_null() {
            // SAFETY: a non-null out-parameter is writable (# Safety).
            unsafe { *microsecond = micro };
        }
        if !offset_seconds.is_null() {
            // SAFETY: a non-null out-parameter is writable (# Safety).
            unsafe { *offset_seconds = offset.seconds() };
        }
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe { borrow_text(name, zone, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_get_aware_datetime", error, body) }
}

/// The number of items of a list or entries of a map; 0 for anything else.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_len(value: *const cr_value) -> usize {
    // SAFETY: `value` is NULL or live (# Safety).
    match unsafe { value.cast::<Value>().as_ref() } {
        Some(Value::List(items)) => items.len(),
        Some(Value::Map(map)) => map.len(),
        _ => 0,
    }
}

/// Whether a value is a record: a map whose entries also have positions.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_is_record(value: *const cr_value) -> bool {
    // SAFETY: `value` is NULL or live (# Safety).
    matches!(unsafe { value.cast::<Value>().as_ref() }, Some(Value::Map(map)) if map.is_record())
}

/// A list's item, borrowed from the list; `NULL` when `value` is not a list or
/// `index` is out of it.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`. The item is valid until the list, or
/// a value that holds it, is changed, moved or freed, and must never be freed
/// or changed itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_list_get(
    value: *const cr_value,
    index: usize,
) -> *const cr_value {
    // SAFETY: `value` is NULL or live (# Safety).
    match unsafe { value.cast::<Value>().as_ref() } {
        Some(Value::List(items)) => items
            .get(index)
            .map_or(ptr::null(), |item| ptr::from_ref(item).cast()),
        _ => ptr::null(),
    }
}

/// A map's entry by position: its key, borrowed and not NUL-terminated, and its
/// value, borrowed. Every out-parameter may be `NULL`.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; each out-parameter is `NULL` or
/// points to a writable value of its type; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`. The key and the item are valid until
/// the map, or a value that holds it, is changed, moved or freed; the item must
/// never be freed or changed itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_map_entry(
    value: *const cr_value,
    index: usize,
    key: *mut *const c_char,
    len: *mut usize,
    item: *mut *const cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe {
            clear_borrowed(key, len);
            clear_borrowed(item, ptr::null_mut());
        }
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { value_of(value, "value") }?;
        let Value::Map(map) = value else {
            return Err(wrong(value, "a map"));
        };
        let size = map.len();
        let (name, entry) = map.iter().nth(index).ok_or_else(|| {
            Failure::Invalid(
                format!("index {index} is out of the map of {size}"),
                "read up to cr_value_len entries",
            )
        })?;
        if !item.is_null() {
            // SAFETY: a non-null out-parameter is writable (# Safety).
            unsafe { *item = ptr::from_ref(entry).cast() };
        }
        // SAFETY: the out-parameters are NULL or writable (# Safety).
        unsafe { borrow_text(Some(name), key, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call("cr_value_map_entry", error, body) }
}

/// A map's value under `key`, borrowed from the map; `NULL` when `value` is not
/// a map or has no such key.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `key` is `NULL` or points to `len`
/// readable bytes. The item is valid until the map, or a value that holds it,
/// is changed, moved or freed, and must never be freed or changed itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_map_get(
    value: *const cr_value,
    key: *const c_char,
    len: usize,
) -> *const cr_value {
    // SAFETY: `value` is NULL or live (# Safety).
    let Some(Value::Map(map)) = (unsafe { value.cast::<Value>().as_ref() }) else {
        return ptr::null();
    };
    // SAFETY: `key` and `len` (# Safety).
    let Ok(key) = (unsafe { bytes(key, len, "key") }) else {
        return ptr::null();
    };
    std::str::from_utf8(key)
        .ok()
        .and_then(|key| map.get(key))
        .map_or(ptr::null(), |item| ptr::from_ref(item).cast())
}

/// Free a value made by a `cr_value_new_*` function, `cr_value_from_json` or a
/// decision. `NULL` is ignored. Never free a value moved into a list or map,
/// or one borrowed from another value.
///
/// # Safety
///
/// `value` is `NULL` or a `cr_value` owned by the caller, not freed or moved
/// before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_value_free(value: *mut cr_value) {
    // SAFETY: an owned `cr_value *` is a box made by `boxed`.
    unsafe { free(value.cast::<Value>()) };
}

/// Hand a value to the caller through `out`, from the decision functions.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `cr_value *`.
pub(crate) unsafe fn give_new_value(out: *mut *mut cr_value, value: Value) -> Outcome {
    // SAFETY: the caller's promise is `clear`'s.
    unsafe { clear(out, "out") }?;
    // SAFETY: `out` passed `clear`.
    unsafe { give_value(out, value) };
    Ok(())
}

/// The value behind a handle argument named `what`, for the other modules.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`.
pub(crate) unsafe fn borrow_value<'a>(value: *const cr_value, what: &str) -> Outcome<&'a Value> {
    // SAFETY: the caller's promise is `value_of`'s.
    unsafe { value_of(value, what) }
}
