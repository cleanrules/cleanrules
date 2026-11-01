//! YAML text (`spec/json-format.md`, *YAML*), behind the `yaml` feature: the
//! counterpart of [`crate::json`] for rule files written in YAML.
//!
//! A YAML text is read into the JSON model with the JSON subset of the YAML 1.2
//! core schema; [`crate::format::read_yaml_document`] then reads it as that JSON
//! document would be. Directives, anchors, aliases, tags, several documents,
//! non-scalar keys and raw control characters are rejected with their line and
//! column.

use std::collections::HashSet;
use std::fmt::Write as _;

use saphyr_parser::{Event, Parser, ScalarStyle, Span};

use crate::error::{Error, ErrorCode, Result};
use crate::json::{Json, JsonNumber, Limits};

fn at(span: &Span, message: impl Into<String>, hint: &str) -> Error {
    // saphyr counts lines from 1 and columns from 0, in characters.
    Error::new(ErrorCode::FormatError, message)
        .with_hint(hint)
        .at_position(span.start.line() as u64, span.start.col() as u64 + 1)
}

const NO_ANCHORS: &str = "anchors and aliases are not allowed: repeat the value instead";
const NO_TAGS: &str = "write typed values with their $-keys, such as {\"$date\": \"2026-10-31\"}";

/// The longest implicit mapping key YAML allows, in characters; longer keys are
/// written as explicit keys (`? key`).
const MAX_IMPLICIT_KEY: usize = 1024;

/// Where the text's line breaks put a character position (1-based line and
/// column; `\r\n`, `\r` and `\n` are line breaks, as in YAML).
fn position(text: &str, upto: usize) -> (u64, u64) {
    let (mut line, mut column) = (1u64, 1u64);
    let mut chars = text.chars().take(upto).peekable();
    while let Some(c) = chars.next() {
        if c == '\n' || (c == '\r' && chars.peek() != Some(&'\n')) {
            line += 1;
            column = 1;
        } else if c != '\r' {
            column += 1;
        }
    }
    (line, column)
}

/// Raw control characters other than tab and line breaks: YAML forbids them,
/// as JSON does inside strings; they are rejected anywhere in the text.
fn check_characters(text: &str) -> Result<()> {
    if let Some(at) = text.chars().position(|c| {
        (c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{85}')) || c == '\u{7f}'
    }) {
        let (line, column) = position(text, at);
        return Err(
            Error::new(ErrorCode::FormatError, "a raw control character")
                .with_hint("write it with an escape in a double-quoted string, such as \\u0007")
                .at_position(line, column),
        );
    }
    Ok(())
}

/// Directives (`%YAML`, `%TAG`) can only come before the first `---`.
fn check_directives(text: &str, start: usize) -> Result<()> {
    let before: String = text.chars().take(start).collect();
    let mut offset = 0;
    for line in before.split_inclusive('\n') {
        if line.starts_with('%') {
            let (line, column) = position(text, offset);
            return Err(
                Error::new(ErrorCode::FormatError, "YAML directives are not allowed")
                    .with_hint(
                        "rule files are read as YAML 1.2 with the JSON subset of its core schema",
                    )
                    .at_position(line, column),
            );
        }
        offset += line.chars().count();
    }
    Ok(())
}

/// A container being read.
enum Open {
    Sequence(Vec<Json>),
    Mapping {
        members: Vec<(String, Json)>,
        /// The keys read so far, for duplicates in constant time.
        keys: HashSet<String>,
        /// The key read, waiting for its value.
        key: Option<String>,
    },
}

impl Open {
    fn wants_key(&self) -> bool {
        matches!(self, Open::Mapping { key: None, .. })
    }
}

/// A plain scalar, by the JSON subset of the YAML 1.2 core schema.
fn plain(text: &str) -> Json {
    match text {
        "" | "null" | "~" => Json::Null,
        "true" => Json::Bool(true),
        "false" => Json::Bool(false),
        _ if is_json_number(text) => Json::Number(JsonNumber::from_literal(text)),
        _ => Json::String(text.to_owned()),
    }
}

/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`
fn is_json_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = usize::from(bytes.first() == Some(&b'-'));
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    match bytes.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            digits(&mut i);
        }
        _ => return false,
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        if digits(&mut i) == 0 {
            return false;
        }
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == bytes.len()
}

/// Read a YAML text into JSON values, as [`crate::json::parse`] reads JSON.
///
/// # Errors
///
/// `limit_exceeded` beyond the YAML size limit (`Limits::max_yaml_size`) or the
/// nesting limit; `format_error` for a syntax error, a forbidden construct, a
/// raw control character or a duplicate key, with its line and column.
///
/// # Examples
///
/// ```
/// use cleanrules::json::{Json, Limits};
///
/// let value = cleanrules::yaml::parse("a: [yes, 2026-10-31]\n", &Limits::default())?;
/// let Json::Object(members) = &value else { unreachable!() };
/// let Json::Array(items) = &members[0].1 else { unreachable!() };
/// assert!(matches!(&items[0], Json::String(s) if s == "yes")); // never a boolean
/// assert!(matches!(&items[1], Json::String(s) if s == "2026-10-31")); // never a date
/// # Ok::<(), cleanrules::Error>(())
/// ```
#[allow(clippy::too_many_lines)] // One arm per YAML event, as the spec lists them.
pub fn parse(text: &str, limits: &Limits) -> Result<Json> {
    if text.len() > limits.max_yaml_size {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            format!(
                "the YAML text has {} bytes, more than the limit of {}",
                text.len(),
                limits.max_yaml_size
            ),
        )
        .with_hint("raise the YAML size limit if the file is trusted"));
    }
    check_characters(text)?;
    let mut open: Vec<Open> = Vec::new();
    let mut root: Option<Json> = None;
    let mut documents = 0;
    for event in Parser::new_from_str(text) {
        let (event, span) = event.map_err(|error| {
            let line = error.marker().line() as u64;
            let column = error.marker().col() as u64 + 1;
            let info = error.info();
            if info.contains("recursion limit") {
                // The parser nests flow collections at most 255 deep.
                return Error::new(
                    ErrorCode::LimitExceeded,
                    "flow mappings and sequences are nested more than 255 deep",
                )
                .with_hint("write deep structures in block style")
                .at_position(line, column);
            }
            if info.contains("unknown anchor") {
                return Error::new(ErrorCode::FormatError, "YAML aliases are not allowed")
                    .with_hint(NO_ANCHORS)
                    .at_position(line, column);
            }
            Error::new(ErrorCode::FormatError, format!("invalid YAML: {info}"))
                .with_hint("check the indentation and the quotes around this position")
                .at_position(line, column)
        })?;
        let value = match event {
            Event::DocumentStart(explicit) => {
                documents += 1;
                if documents > 1 {
                    return Err(at(
                        &span,
                        "a rule file holds one YAML document",
                        "keep one document per file",
                    ));
                }
                if explicit {
                    check_directives(text, span.start.index())?;
                }
                continue;
            }
            Event::Alias(_) => {
                return Err(at(&span, "YAML aliases are not allowed", NO_ANCHORS));
            }
            Event::Scalar(text, style, anchor, tag) => {
                if anchor != 0 {
                    return Err(at(&span, "YAML anchors are not allowed", NO_ANCHORS));
                }
                if tag.is_some() {
                    return Err(at(&span, "YAML tags are not allowed", NO_TAGS));
                }
                if let Some(Open::Mapping { keys, key, .. }) = open.last_mut()
                    && key.is_none()
                {
                    // A key is always a string: the text of its scalar.
                    if !keys.insert(text.to_string()) {
                        return Err(at(
                            &span,
                            format!("the key {:?} appears twice in a mapping", &*text),
                            "YAML readers disagree on which duplicate wins, so cleanrules rejects them",
                        ));
                    }
                    *key = Some(text.into_owned());
                    continue;
                }
                if style == ScalarStyle::Plain {
                    plain(&text)
                } else {
                    Json::String(text.into_owned())
                }
            }
            Event::SequenceStart(anchor, ref tag) | Event::MappingStart(anchor, ref tag) => {
                if anchor != 0 {
                    return Err(at(&span, "YAML anchors are not allowed", NO_ANCHORS));
                }
                if tag.is_some() {
                    return Err(at(&span, "YAML tags are not allowed", NO_TAGS));
                }
                if open.last().is_some_and(Open::wants_key) {
                    return Err(at(
                        &span,
                        "a mapping key must be a scalar",
                        "keys are strings, as in JSON",
                    ));
                }
                if open.len() + 1 > limits.max_nesting {
                    return Err(Error::new(
                        ErrorCode::LimitExceeded,
                        format!(
                            "mappings and sequences are nested more than {} deep",
                            limits.max_nesting
                        ),
                    )
                    .at_position(span.start.line() as u64, span.start.col() as u64 + 1));
                }
                open.push(if matches!(event, Event::SequenceStart(..)) {
                    Open::Sequence(Vec::new())
                } else {
                    Open::Mapping {
                        members: Vec::new(),
                        keys: HashSet::new(),
                        key: None,
                    }
                });
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => match open.pop() {
                Some(Open::Sequence(items)) => Json::Array(items),
                Some(Open::Mapping { members, .. }) => Json::Object(members),
                None => continue,
            },
            _ => continue,
        };
        match open.last_mut() {
            Some(Open::Sequence(items)) => items.push(value),
            Some(Open::Mapping { members, key, .. }) => {
                if let Some(key) = key.take() {
                    members.push((key, value));
                }
            }
            None => root = Some(value),
        }
    }
    root.ok_or_else(|| {
        // One past the last character, as for an unexpected end of a JSON text.
        let (line, column) = position(text, text.chars().count());
        Error::new(ErrorCode::FormatError, "the YAML text holds no document")
            .with_hint("a rule file holds one document: `version: 1` and its content")
            .at_position(line, column)
    })
}

/// A mapping key that can be written plain (`spec/json-format.md`, *YAML*).
fn plain_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.' | '-'))
        && !matches!(key, "null" | "true" | "false")
}

/// A string as a double-quoted scalar: its JSON form, with every control
/// character the JSON writer leaves raw (DEL, C1, NEL) escaped, as the reader
/// rejects raw control characters.
fn quoted(text: &str) -> String {
    let json = crate::json::to_string(&Json::String(text.to_owned()));
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if c.is_control() {
            // Writing to a String cannot fail.
            let _ = write!(out, "\\u{:04X}", u32::from(c));
        } else {
            out.push(c);
        }
    }
    out
}

fn scalar(value: &Json) -> Option<String> {
    match value {
        Json::Array(items) if items.is_empty() => Some("[]".to_owned()),
        Json::Object(members) if members.is_empty() => Some("{}".to_owned()),
        Json::Array(_) | Json::Object(_) => None,
        Json::String(text) => Some(quoted(text)),
        other => Some(crate::json::to_string(other)),
    }
}

fn key_text(key: &str) -> String {
    if plain_key(key) {
        key.to_owned()
    } else {
        quoted(key)
    }
}

fn write_value(value: &Json, indent: usize, out: &mut String) {
    match value {
        Json::Object(members) if !members.is_empty() => {
            for (i, (key, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    out.push_str(&" ".repeat(indent));
                }
                write_member(key, item, indent, out);
            }
        }
        Json::Array(items) if !items.is_empty() => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    out.push_str(&" ".repeat(indent));
                }
                out.push_str("- ");
                match scalar(item) {
                    Some(text) => out.push_str(&text),
                    None => write_value(item, indent + 2, out),
                }
            }
        }
        other => out.push_str(&scalar(other).unwrap_or_default()),
    }
}

fn write_member(key: &str, item: &Json, indent: usize, out: &mut String) {
    let key = key_text(key);
    if key.chars().count() > MAX_IMPLICIT_KEY {
        // An explicit key: `? key`, then `: value` on the next line.
        out.push_str("? ");
        out.push_str(&key);
        out.push('\n');
        out.push_str(&" ".repeat(indent));
    } else {
        out.push_str(&key);
    }
    out.push(':');
    if let Some(text) = scalar(item) {
        out.push(' ');
        out.push_str(&text);
    } else {
        out.push('\n');
        out.push_str(&" ".repeat(indent + 2));
        write_value(item, indent + 2, out);
    }
}

/// The canonical YAML of a JSON document (`spec/json-format.md`, *YAML*),
/// ending with a newline; reading it back gives the same document.
///
/// # Examples
///
/// ```
/// use cleanrules::json::{Limits, parse};
///
/// let json = parse(r#"{"version": 1, "tags": ["a", "2026-10-31"]}"#, &Limits::default())?;
/// assert_eq!(
///     cleanrules::yaml::to_string(&json),
///     "version: 1\ntags:\n  - \"a\"\n  - \"2026-10-31\"\n"
/// );
/// # Ok::<(), cleanrules::Error>(())
/// ```
#[must_use]
pub fn to_string(json: &Json) -> String {
    let mut out = String::new();
    write_value(json, 0, &mut out);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::fmt::Write as _;

    use super::{is_json_number, parse, to_string as to_yaml};
    use crate::error::ErrorCode;
    use crate::json::{Limits, parse as parse_json, same, to_string};

    #[test]
    fn json_numbers() {
        for yes in ["0", "-0", "42", "-7", "1.5", "1e-7", "2E+3", "-0.0"] {
            assert!(is_json_number(yes), "{yes}");
        }
        for no in [
            "", "-", "01", "1.", ".5", "1e", "0x1F", "1_000", "+1", ".inf", "1.5.2",
        ] {
            assert!(!is_json_number(no), "{no}");
        }
    }

    fn error(text: &str) -> (ErrorCode, Option<u64>, Option<u64>) {
        let error = parse(text, &Limits::default()).unwrap_err();
        (error.code(), error.context().line, error.context().column)
    }

    #[test]
    fn forbidden_constructs_are_located() {
        let format = ErrorCode::FormatError;
        assert_eq!(
            error("%YAML 1.1\n---\na: yes\n"),
            (format, Some(1), Some(1))
        );
        assert_eq!(
            error("# c\n%TAG ! x\n---\na: 1\n"),
            (format, Some(2), Some(1))
        );
        assert_eq!(error("a: \"x\u{7}y\"\n"), (format, Some(1), Some(6)));
        assert_eq!(error("a: *b\n"), (format, Some(1), Some(4)));
        // `\r` alone is a line break, as in YAML.
        assert_eq!(error("# a\r# b\r"), (format, Some(3), Some(1)));
    }

    #[test]
    fn limits() {
        let deep = format!("{}{}", "[".repeat(300), "]".repeat(300));
        let mut limits = Limits {
            max_nesting: 10_000,
            ..Limits::default()
        };
        let error = parse(&deep, &limits).unwrap_err();
        assert_eq!(error.code(), ErrorCode::LimitExceeded); // flow collections stop at 255
        let large = format!("a: \"{}\"\n", "x".repeat(1024 * 1024));
        assert_eq!(
            parse(&large, &Limits::default()).unwrap_err().code(),
            ErrorCode::LimitExceeded
        );
        limits.max_yaml_size = 2 * 1024 * 1024;
        assert!(parse(&large, &limits).is_ok());
    }

    #[test]
    fn many_keys_are_read_in_linear_time() {
        let mut text = String::new();
        for i in 0..100_000 {
            let _ = writeln!(text, "k{i}: {i}");
        }
        let start = std::time::Instant::now();
        let limits = Limits {
            max_yaml_size: text.len(),
            ..Limits::default()
        };
        assert!(parse(&text, &limits).is_ok());
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    #[test]
    fn canonical_yaml() {
        let text = r#"{"version":1,"rule":{"name":"r","when":{"op":"field","path":["a",0]},"metadata":{"null":null,"true":[],"weird key":{},"n":-0.0}}}"#;
        let json = parse_json(text, &Limits::default()).unwrap();
        assert_eq!(
            to_yaml(&json),
            "version: 1\nrule:\n  name: \"r\"\n  when:\n    op: \"field\"\n    path:\n      - \"a\"\n      - 0\n  metadata:\n    \"null\": null\n    \"true\": []\n    \"weird key\": {}\n    n: -0.0\n"
        );
    }

    #[test]
    fn long_keys_are_written_as_explicit_keys() {
        let key = "k".repeat(1025);
        let text = format!(r#"{{"{key}":1,"short":{{"{key}":[1]}}}}"#);
        let json = parse_json(&text, &Limits::default()).unwrap();
        let yaml = to_yaml(&json);
        assert!(yaml.starts_with("? "));
        let back = parse(&yaml, &Limits::default()).unwrap();
        assert!(same(&back, &json), "{yaml}");
    }

    #[test]
    fn canonical_yaml_reads_back_as_the_same_document() {
        let text = r#"{"version":1,"del":"a\u007fb\u0085c\u2028d","ruleset":{"hit_policy":"first","rules":[{"name":"vip","when":{"op":"and","operands":[{"op":"gt","left":{"op":"field","path":["customer","years"]},"right":{"op":"const","value":5}},{"op":"eq","left":{"op":"field","path":["since"]},"right":{"op":"const","value":{"$date":"2026-10-31"}}}]},"then":{"discount":{"op":"const","value":{"$decimal":"0.15"}},"note":{"op":"const","value":"yes\nno \"quoted\""}},"tags":["a b",""],"metadata":{"1":[],"x":{},"weird key":-0.0}}]}}"#;
        let json = parse_json(text, &Limits::default()).unwrap();
        let yaml = to_yaml(&json);
        let back = parse(&yaml, &Limits::default()).unwrap();
        assert!(same(&back, &json), "{yaml}\n{}", to_string(&back));
    }
}
