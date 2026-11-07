//! Values in JSON (`spec/json-format.md`, *Values*): bare JSON where it is
//! unambiguous, and typed values (`{"$decimal": "0.10"}`) elsewhere.

use std::borrow::Cow;
use std::fmt::Write as _;

use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::{Offset, TimeZone};

use crate::error::{Error, ErrorCode, Result};
use crate::json::{Json, JsonNumber, pointer_token};
use crate::value::{
    AwareDateTime, Decimal, DecimalError, Map, NullCause, Opaque, Value, Zone, check_date,
    check_naive,
};
use crate::walk::post_order;

/// Where a value is read, which decides what is allowed and what an out-of-range
/// number becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueContext {
    /// A constant in a condition or payload: only scalars; out of range is an error.
    Constant,
    /// The input of an evaluation: lists and maps too; out of range is null with a
    /// reason.
    Data,
    /// A value recorded in a decision or an explanation: like data, plus opaque
    /// values.
    Recorded,
}

/// The largest int written bare: beyond it JavaScript and Go lose precision.
const SAFE_INT: i64 = (1 << 53) - 1;

fn format_error(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

/// Read a value at `pointer` (a JSON Pointer, for errors).
///
/// # Errors
///
/// `format_error`, pointing to the offending value or key, when the JSON is not a
/// value allowed in `context`.
pub fn read_value(json: &Json, pointer: &str, context: ValueContext) -> Result<Value> {
    if !matches!(json, Json::Array(_) | Json::Object(_)) {
        return read_scalar(json, pointer, context);
    }
    // Lists and maps nest as deep as the JSON nesting limit allows: they are read
    // with an explicit stack (`crate::walk`), children in order after their
    // container's own checks, as a recursive reader would.
    post_order(
        open_value(json, Cow::Borrowed(pointer), context)?,
        |reading, done| {
            let (at, item) = match &reading.items {
                Items::None(_) => return Ok(None),
                Items::List(items) => match items.get(done) {
                    Some(item) => (format!("{}/{done}", reading.pointer), item),
                    None => return Ok(None),
                },
                Items::Map(members) => match members.get(done) {
                    Some((key, item)) => {
                        (format!("{}/{}", reading.pointer, pointer_token(key)), item)
                    }
                    None => return Ok(None),
                },
            };
            open_value(item, Cow::Owned(at), context).map(Some)
        },
        |reading, values| {
            Ok(match reading.items {
                Items::None(value) => value,
                Items::List(_) => Value::List(values.collect()),
                Items::Map(members) => {
                    let mut map = Map::new();
                    for ((key, _), value) in members.iter().zip(values) {
                        map.insert(key.clone(), value);
                    }
                    Value::Map(map)
                }
            })
        },
    )
}

/// A value being read: its pointer (for a `$map`, that of its entries) and its
/// items still to read.
struct Reading<'a> {
    pointer: Cow<'a, str>,
    items: Items<'a>,
}

enum Items<'j> {
    /// A value without items, already read.
    None(Value),
    List(&'j [Json]),
    Map(&'j [(String, Json)]),
}

/// Check a value as a reader entering it would, and read it if it has no items.
fn open_value<'a>(
    json: &'a Json,
    pointer: Cow<'a, str>,
    context: ValueContext,
) -> Result<Reading<'a>> {
    let items = match json {
        Json::Array(items) => {
            if context == ValueContext::Constant {
                return Err(format_error(
                    &pointer,
                    "a constant must be a scalar, not a list",
                ));
            }
            Items::List(items)
        }
        Json::Object(members) => match read_object(members, &pointer, context)? {
            Object::Value(value) => Items::None(value),
            Object::Map(members, at) => {
                return Ok(Reading {
                    pointer: at.map_or(pointer, Cow::Owned),
                    items: Items::Map(members),
                });
            }
        },
        scalar => Items::None(read_scalar(scalar, &pointer, context)?),
    };
    Ok(Reading { pointer, items })
}

/// A value that is neither a list nor an object.
fn read_scalar(json: &Json, pointer: &str, context: ValueContext) -> Result<Value> {
    Ok(match json {
        Json::Bool(b) => Value::Bool(*b),
        Json::String(s) => Value::String(s.clone()),
        Json::Number(n) => return read_number(n, pointer, context),
        _ => Value::Null(NullCause::Null),
    })
}

fn read_number(n: &JsonNumber, pointer: &str, context: ValueContext) -> Result<Value> {
    if n.is_integer_literal() {
        return match n.as_i128().and_then(|i| i64::try_from(i).ok()) {
            Some(i) => Ok(Value::Int(i)),
            None if context == ValueContext::Constant => Err(format_error(
                pointer,
                format!(
                    "the integer {} is outside the signed 64-bit range",
                    n.text()
                ),
            )),
            None => Ok(Value::Null(NullCause::IntOverflow)),
        };
    }
    read_float_literal(n, pointer)
}

fn read_float_literal(n: &JsonNumber, pointer: &str) -> Result<Value> {
    let f = n.as_f64();
    if f.is_infinite() {
        return Err(
            format_error(pointer, format!("{} rounds to infinity", n.text()))
                .with_hint(r#"write {"$float": "inf"}"#),
        );
    }
    Ok(Value::Float(f))
}

/// An object read: a typed value, or a map's entries to read, at the object's
/// pointer or, for a `$map`, at this one.
enum Object<'j> {
    Value(Value),
    Map(&'j [(String, Json)], Option<String>),
}

fn read_object<'j>(
    members: &'j [(String, Json)],
    pointer: &str,
    context: ValueContext,
) -> Result<Object<'j>> {
    let Some(typed) = members.iter().position(|(k, _)| k.starts_with('$')) else {
        if context == ValueContext::Constant {
            return Err(format_error(
                pointer,
                "a constant must be a scalar, not a map",
            ));
        }
        return Ok(Object::Map(members, None));
    };
    let (tag, inner) = &members[typed];
    let at = format!("{pointer}/{}", pointer_token(tag));
    if !matches!(
        tag.as_str(),
        "$int" | "$float" | "$decimal" | "$date" | "$datetime" | "$map" | "$opaque"
    ) {
        return Err(format_error(&at, format!("{tag} is not a type of value")));
    }
    if let Some((extra, _)) = members.iter().find(|(k, _)| k != tag) {
        return Err(format_error(
            &format!("{pointer}/{}", pointer_token(extra)),
            format!("a typed value has the single key {tag}; {extra:?} is extra"),
        ));
    }
    if tag == "$map" {
        let Json::Object(entries) = inner else {
            return Err(format_error(&at, "$map takes an object"));
        };
        if context == ValueContext::Constant {
            return Err(format_error(
                pointer,
                "a constant must be a scalar, not a map",
            ));
        }
        return Ok(Object::Map(entries, Some(at)));
    }
    let value = match tag.as_str() {
        "$int" => read_typed_int(inner, &at, context),
        "$float" => match inner {
            Json::Number(n) => read_float_literal(n, &at),
            Json::String(s) if s == "inf" => Ok(Value::Float(f64::INFINITY)),
            Json::String(s) if s == "-inf" => Ok(Value::Float(f64::NEG_INFINITY)),
            _ => Err(format_error(
                &at,
                r#"$float takes a number, "inf" or "-inf""#,
            )),
        },
        "$decimal" => {
            let Json::String(text) = inner else {
                return Err(format_error(&at, "$decimal takes a string"));
            };
            match Decimal::parse(text) {
                Ok(d) => Ok(Value::Decimal(d)),
                Err(DecimalError::DoesNotFit) if context != ValueContext::Constant => {
                    Ok(Value::Null(NullCause::DecimalOverflow))
                }
                Err(error) => Err(format_error(&at, format!("{text:?}: {error}"))),
            }
        }
        "$date" => {
            let Json::String(text) = inner else {
                return Err(format_error(&at, "$date takes a string"));
            };
            parse_date(text).map(Value::Date).ok_or_else(|| {
                format_error(&at, format!("{text:?} is not a date of years 0001 to 9999"))
            })
        }
        "$datetime" => {
            let Json::String(text) = inner else {
                return Err(format_error(&at, "$datetime takes a string"));
            };
            parse_datetime(text).map_err(|why| format_error(&at, format!("{text:?}: {why}")))
        }
        // "$opaque"
        _ => match (inner, context) {
            (Json::String(name), ValueContext::Recorded) if !name.is_empty() => {
                Ok(Value::Opaque(Opaque {
                    type_name: name.clone(),
                    token: 0,
                }))
            }
            _ => Err(format_error(
                pointer,
                "an opaque value appears only in decisions and explanations",
            )),
        },
    };
    value.map(Object::Value)
}

fn read_typed_int(inner: &Json, at: &str, context: ValueContext) -> Result<Value> {
    let parsed = match inner {
        Json::Number(n) if n.is_integer_literal() => n.as_i128(),
        Json::String(s)
            if !s.is_empty()
                && s.strip_prefix('-')
                    .unwrap_or(s)
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                && s != "-" =>
        {
            // Leading zeros are allowed; a value beyond 128 bits is out of range anyway.
            Some(s.parse::<i128>().unwrap_or(i128::MAX))
        }
        _ => {
            return Err(format_error(
                at,
                "$int takes an integer literal or a string of digits",
            ));
        }
    };
    match parsed.and_then(|i| i64::try_from(i).ok()) {
        Some(i) => Ok(Value::Int(i)),
        None if context == ValueContext::Constant => Err(format_error(
            at,
            "the integer is outside the signed 64-bit range",
        )),
        None => Ok(Value::Null(NullCause::IntOverflow)),
    }
}

/// Read `YYYY-MM-DD`, years 0001 to 9999.
fn parse_date(text: &str) -> Option<Date> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = digits(&text[0..4])?;
    let month = digits(&text[5..7])?;
    let day = digits(&text[8..10])?;
    let date = Date::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
    )
    .ok()?;
    check_date(date).ok()
}

fn digits(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Read a datetime (`spec/json-format.md`, *Dates and datetimes*).
fn parse_datetime(text: &str) -> std::result::Result<Value, &'static str> {
    const MALFORMED: &str = "not a datetime of the form YYYY-MM-DDTHH:MM:SS";
    if text.len() < 19 || text.as_bytes()[10] != b'T' {
        return Err(MALFORMED);
    }
    let date = parse_date(&text[..10]).ok_or("not a valid day of years 0001 to 9999")?;
    let time = &text[11..19];
    let tb = time.as_bytes();
    if tb[2] != b':' || tb[5] != b':' {
        return Err(MALFORMED);
    }
    let (hour, minute, second) = (
        digits(&time[0..2]).ok_or(MALFORMED)?,
        digits(&time[3..5]).ok_or(MALFORMED)?,
        digits(&time[6..8]).ok_or(MALFORMED)?,
    );
    let mut rest = &text[19..];
    let mut micros = 0_u32;
    if let Some(after) = rest.strip_prefix('.') {
        let len = after.bytes().take_while(u8::is_ascii_digit).count();
        if len == 0 {
            return Err(MALFORMED);
        }
        if len > 6 {
            return Err("more than six fraction digits (microseconds)");
        }
        micros = digits(&after[..len]).ok_or(MALFORMED)?
            * 10_u32.pow(6 - u32::try_from(len).unwrap_or(6));
        rest = &after[len..];
    }
    let civil = DateTime::new(
        date.year(),
        date.month(),
        date.day(),
        i8::try_from(hour).map_err(|_| MALFORMED)?,
        i8::try_from(minute).map_err(|_| MALFORMED)?,
        i8::try_from(second).map_err(|_| MALFORMED)?,
        i32::try_from(micros * 1000).map_err(|_| MALFORMED)?,
    )
    .map_err(|_| "not a valid time (no leap seconds)")?;
    if rest.is_empty() {
        return check_naive(civil)
            .map(Value::NaiveDateTime)
            .map_err(|_| "out of range");
    }
    if rest == "Z" {
        return aware(civil, Offset::UTC, Zone::Fixed(Offset::UTC));
    }
    let (offset, rest) =
        parse_offset(rest).ok_or("not a valid offset (Z or ±HH:MM within 18 hours)")?;
    if rest.is_empty() {
        return aware(civil, offset, Zone::Fixed(offset));
    }
    let name = rest
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .filter(|n| {
            !n.is_empty()
                && n.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-' | b'/'))
        })
        .ok_or("a zone must be one IANA name in brackets, without flags or tags")?;
    let zone = jiff::tz::db().get(name).map_err(|_| "unknown time zone")?;
    if zone.iana_name() != Some(name) {
        return Err("unknown time zone (names are case-sensitive)");
    }
    let timestamp = civil
        .to_zoned(TimeZone::fixed(offset))
        .map_err(|_| "out of range")?
        .timestamp();
    if zone.to_offset(timestamp) != offset {
        return Err("the offset is not the zone's offset at that instant");
    }
    AwareDateTime::new(timestamp, Zone::Named(name.to_owned()))
        .map(Value::AwareDateTime)
        .map_err(|_| "out of range")
}

fn aware(civil: DateTime, offset: Offset, zone: Zone) -> std::result::Result<Value, &'static str> {
    let timestamp = civil
        .to_zoned(TimeZone::fixed(offset))
        .map_err(|_| "out of range")?
        .timestamp();
    AwareDateTime::new(timestamp, zone)
        .map(Value::AwareDateTime)
        .map_err(|_| "out of range")
}

/// `±HH:MM` or `±HH:MM:SS` within 18 hours; `-00:00` is zero.
fn parse_offset(text: &str) -> Option<(Offset, &str)> {
    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let b = text.as_bytes();
    if b.len() < 6 || b[3] != b':' {
        return None;
    }
    let hours = digits(&text[1..3])?;
    let minutes = digits(&text[4..6])?;
    let (seconds, rest) = if b.len() >= 9 && b[6] == b':' {
        (digits(&text[7..9])?, &text[9..])
    } else {
        (0, &text[6..])
    };
    if minutes > 59 || seconds > 59 {
        return None;
    }
    let total = hours * 3600 + minutes * 60 + seconds;
    if total > 18 * 3600 {
        return None;
    }
    let offset = Offset::from_seconds(sign * i32::try_from(total).ok()?).ok()?;
    Some((offset, rest))
}

/// Write a value in canonical form.
#[must_use]
pub fn write_value(value: &Value) -> Json {
    if !matches!(value, Value::List(_) | Value::Map(_)) {
        return write_scalar(value);
    }
    // Lists and maps nest as deep as the reader's limits allow: written with an
    // explicit stack (`crate::walk`).
    let written: std::result::Result<Json, std::convert::Infallible> = post_order(
        value,
        |value, done| Ok(value.item(done)),
        |value, items| {
            Ok(match value {
                Value::List(_) => Json::Array(items.collect()),
                Value::Map(map) => {
                    let members: Vec<(String, Json)> =
                        map.iter().map(|(k, _)| k.to_owned()).zip(items).collect();
                    if map.iter().any(|(k, _)| k.starts_with('$')) {
                        typed("$map", Json::Object(members))
                    } else {
                        Json::Object(members)
                    }
                }
                scalar => write_scalar(scalar),
            })
        },
    );
    match written {
        Ok(json) => json,
        Err(never) => match never {},
    }
}

/// A value without items in canonical form (a list or map as if empty).
fn write_scalar(value: &Value) -> Json {
    match value {
        Value::Null(_) => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Int(i) if i.unsigned_abs() <= SAFE_INT.unsigned_abs() => {
            Json::Number(JsonNumber::from_literal(i.to_string()))
        }
        Value::Int(i) => typed("$int", Json::String(i.to_string())),
        Value::Float(f) if f.is_infinite() => typed(
            "$float",
            Json::String(if *f > 0.0 { "inf" } else { "-inf" }.into()),
        ),
        // Debug writes the shortest round-trip digits with a `.` or an exponent.
        Value::Float(f) => Json::Number(JsonNumber::from_literal(format!("{f:?}"))),
        Value::Decimal(d) => typed("$decimal", Json::String(d.to_string())),
        Value::String(s) => Json::String(s.clone()),
        Value::Date(d) => typed("$date", Json::String(date_text(*d))),
        Value::NaiveDateTime(dt) => typed("$datetime", Json::String(naive_text(*dt))),
        Value::AwareDateTime(a) => typed("$datetime", Json::String(aware_text(a))),
        Value::List(_) => Json::Array(Vec::new()),
        Value::Map(_) => Json::Object(Vec::new()),
        Value::Opaque(o) => typed("$opaque", Json::String(o.type_name.clone())),
    }
}

fn typed(tag: &str, inner: Json) -> Json {
    Json::Object(vec![(tag.to_owned(), inner)])
}

fn date_text(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())
}

fn naive_text(dt: DateTime) -> String {
    let mut text = format!(
        "{}T{:02}:{:02}:{:02}",
        date_text(dt.date()),
        dt.hour(),
        dt.minute(),
        dt.second()
    );
    let micros = dt.subsec_nanosecond() / 1000;
    if micros != 0 {
        // Writing to a String cannot fail.
        let _ = write!(text, ".{micros:06}");
    }
    text
}

fn offset_text(offset: Offset) -> String {
    let seconds = offset.seconds();
    let sign = if seconds < 0 { '-' } else { '+' };
    let total = seconds.unsigned_abs();
    let (hours, minutes, secs) = (total / 3600, total % 3600 / 60, total % 60);
    if secs == 0 {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}:{minutes:02}:{secs:02}")
    }
}

fn aware_text(a: &AwareDateTime) -> String {
    let timestamp: Timestamp = a.timestamp();
    match a.zone() {
        Zone::Fixed(offset) => {
            let local = timestamp.to_zoned(TimeZone::fixed(*offset)).datetime();
            let suffix = if offset.seconds() == 0 {
                "Z".to_owned()
            } else {
                offset_text(*offset)
            };
            format!("{}{suffix}", naive_text(local))
        }
        Zone::Named(name) => match jiff::tz::db().get(name) {
            Ok(zone) => {
                let zoned = timestamp.to_zoned(zone);
                format!(
                    "{}{}[{name}]",
                    naive_text(zoned.datetime()),
                    offset_text(zoned.offset())
                )
            }
            // A zone that was valid when read is in the bundled database.
            Err(_) => format!(
                "{}Z",
                naive_text(timestamp.to_zoned(TimeZone::UTC).datetime())
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{ValueContext, read_value, write_value};
    use crate::json::{Limits, parse, to_string};
    use crate::value::{NullCause, Value};

    fn read(text: &str, context: ValueContext) -> crate::Result<Value> {
        read_value(&parse(text, &Limits::default()).unwrap(), "", context)
    }

    #[test]
    fn data_reads_out_of_range_as_null_with_a_reason() {
        let big = read("123456789012345678901234567890", ValueContext::Data).unwrap();
        assert!(matches!(big, Value::Null(NullCause::IntOverflow)));
        let decimal = read(
            &format!(r#"{{"$decimal": "1{}"}}"#, "0".repeat(38)),
            ValueContext::Data,
        )
        .unwrap();
        assert!(matches!(decimal, Value::Null(NullCause::DecimalOverflow)));
        assert!(read("123456789012345678901234567890", ValueContext::Constant).is_err());
    }

    #[test]
    fn lists_and_maps_in_data_round_trip() {
        let text = r#"{"b":[1,2.5,null],"a":{"$map":{"$ref":{"$date":"2026-10-31"}}},"c":{"$int":"9007199254740992"}}"#;
        let value = read(text, ValueContext::Data).unwrap();
        assert_eq!(to_string(&write_value(&value)), text);
        assert!(read("[1]", ValueContext::Constant).is_err());
        assert!(read(r#"{"a": 1}"#, ValueContext::Constant).is_err());
    }

    #[test]
    fn opaque_only_where_recorded() {
        let text = r#"{"$opaque":"bytes"}"#;
        assert_eq!(
            to_string(&write_value(&read(text, ValueContext::Recorded).unwrap())),
            text
        );
        assert!(read(text, ValueContext::Data).is_err());
        assert!(read(text, ValueContext::Constant).is_err());
    }

    #[test]
    fn floats_are_written_with_a_point_or_exponent() {
        for (value, written) in [
            (100.0, "100.0"),
            (-0.0, "-0.0"),
            (1e-7, "1e-7"),
            (0.1, "0.1"),
        ] {
            assert_eq!(to_string(&write_value(&Value::Float(value))), written);
        }
        assert_eq!(
            to_string(&write_value(&Value::Float(f64::INFINITY))),
            r#"{"$float":"inf"}"#
        );
    }
}
