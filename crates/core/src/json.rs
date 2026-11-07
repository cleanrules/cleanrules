//! A strict JSON reader and writer for the format of `spec/json-format.md`.
//!
//! Generic JSON libraries lose what the format compares: they let the last
//! duplicate key win, read every number through a float, reorder object members
//! and report positions in bytes. This reader keeps member order and each
//! number's text, rejects duplicate keys, lone surrogates and byte order marks,
//! enforces the size and nesting limits, and reports syntax errors by 1-based line
//! and column in Unicode code points (`spec/json-format.md`, *Errors and limits*).
//! It is iterative, so no input within the limits can overflow the stack.

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::error::{Error, ErrorCode, Result};

/// A JSON value, with object members in document order and numbers as text.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, kept as its literal text.
    Number(JsonNumber),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object, members in document order, keys unique.
    Object(Vec<(String, Json)>),
}

/// Dropping is iterative, so a value nested as deep as the caller's limits allow
/// never overflows the stack. Because of this `Drop`, a value's children are taken
/// out with `std::mem::take` or read through references, not moved out by `match`.
impl Drop for Json {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut value) = pending.pop() {
            take_children(&mut value, &mut pending);
        }
    }
}

fn take_children(value: &mut Json, pending: &mut Vec<Json>) {
    match value {
        Json::Array(items) => pending.append(items),
        Json::Object(members) => {
            pending.extend(std::mem::take(members).into_iter().map(|(_, v)| v));
        }
        _ => {}
    }
}

/// A number literal, as written: `1` and `1.0` are different literals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonNumber {
    text: String,
}

impl JsonNumber {
    /// A number from literal text that the reader has already validated, or that
    /// a writer produced (`-0`, `42`, `1.5e-3`...).
    #[must_use]
    pub fn from_literal(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// The literal text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the literal has no fraction and no exponent.
    #[must_use]
    pub fn is_integer_literal(&self) -> bool {
        !self.text.contains(['.', 'e', 'E'])
    }

    /// The exact integer, when the literal is an integer within 128 bits.
    #[must_use]
    pub fn as_i128(&self) -> Option<i128> {
        if self.is_integer_literal() {
            self.text.parse().ok()
        } else {
            None
        }
    }

    /// The literal rounded to the nearest binary64, ties to even. Infinite when
    /// the literal rounds to infinity, and `0.0` (with the literal's sign) when it
    /// is too small.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        // Rust's float parsing is correctly rounded and accepts every JSON literal.
        self.text.parse().unwrap_or(f64::NAN)
    }
}

/// The reader's limits (`spec/json-format.md`, *Errors and limits*). Callers may
/// raise them, never disable them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The largest document, in bytes of UTF-8.
    pub max_size: usize,
    /// The deepest nesting of arrays and objects; the top-level value counts 1.
    pub max_nesting: usize,
    /// The deepest condition or payload value, in nodes (used by document readers).
    pub max_depth: usize,
    /// The largest YAML text, in bytes: lower than `max_size`, because the YAML
    /// parser holds a flow collection whole before reading it.
    pub max_yaml_size: usize,
    /// The largest text in the rule syntax, in bytes: lower than `max_size`,
    /// because a text holds up to one node in every two bytes (`a|a|…`).
    pub max_text_size: usize,
    /// The most cells and nodes a decision table may build: one per cell, and
    /// the nodes of each cell's condition or value, with the copy of its
    /// column that an input cell holds (`spec/tables.md`, *Limits*).
    pub max_table_size: usize,
}

impl Limits {
    /// The same limits with `size` as the largest text of every format: JSON,
    /// YAML, and the rule syntax and CSV (`max_text_size`). YAML's own limit
    /// is lower by default; raising it lets the YAML parser hold a larger flow
    /// collection whole.
    #[must_use]
    pub fn with_max_size(mut self, size: usize) -> Self {
        self.max_size = size;
        self.max_yaml_size = size;
        self.max_text_size = size;
        self
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_size: 16 * 1024 * 1024,
            max_nesting: 1024,
            max_depth: 256,
            max_yaml_size: 1024 * 1024,
            max_text_size: 4 * 1024 * 1024,
            max_table_size: 2 * 1024 * 1024,
        }
    }
}

/// Read a JSON text from bytes, which must be UTF-8.
///
/// # Errors
///
/// `format_error` with a line and column for invalid UTF-8 or JSON, and
/// `limit_exceeded` for a document beyond the limits.
pub fn parse_bytes(bytes: &[u8], limits: &Limits) -> Result<Json> {
    if bytes.len() > limits.max_size {
        return Err(too_large(bytes.len(), limits));
    }
    parse(utf8_text(bytes)?, limits)
}

/// Bytes as UTF-8 text, for every format read from bytes (`spec/errors.md`,
/// *In each language*).
///
/// # Errors
///
/// `format_error` at the line and column of the first byte that is not UTF-8.
pub fn utf8_text(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|error| {
        let valid = &bytes[..error.valid_up_to()];
        // The valid prefix is UTF-8 by construction.
        let prefix = std::str::from_utf8(valid).unwrap_or_default();
        let (line, column) = position_after(prefix);
        syntax(line, column, "the text is not valid UTF-8")
    })
}

/// Read a JSON text.
///
/// # Errors
///
/// `format_error` with a line and column for invalid JSON, a duplicate key, a lone
/// surrogate escape or a byte order mark, and `limit_exceeded` for a document
/// beyond the limits.
pub fn parse(text: &str, limits: &Limits) -> Result<Json> {
    if text.len() > limits.max_size {
        return Err(too_large(text.len(), limits));
    }
    Parser {
        text,
        offset: 0,
        line: 1,
        column: 1,
        limits,
    }
    .document()
}

fn too_large(size: usize, limits: &Limits) -> Error {
    Error::new(
        ErrorCode::LimitExceeded,
        format!(
            "the document has {size} bytes, more than the limit of {}",
            limits.max_size
        ),
    )
    .with_hint("raise the size limit if the document is trusted")
}

fn syntax(line: u64, column: u64, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_position(line, column)
}

/// The 1-based line and column one past the end of `text`.
fn position_after(text: &str) -> (u64, u64) {
    let mut line = 1;
    let mut column = 1;
    for c in text.chars() {
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

/// A container being read, with what is needed to finish it.
enum Frame {
    Array(Vec<Json>),
    Object {
        members: Vec<(String, Json)>,
        keys: HashSet<String>,
        key: Option<String>,
    },
}

struct Parser<'a> {
    text: &'a str,
    offset: usize,
    line: u64,
    column: u64,
    limits: &'a Limits,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.text[self.offset..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.offset += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn here(&self, message: impl Into<String>) -> Error {
        syntax(self.line, self.column, message)
    }

    fn unexpected(&self, expected: &str) -> Error {
        match self.peek() {
            Some(c) => self.here(format!("expected {expected}, found {c:?}")),
            None => self.here(format!(
                "expected {expected}, found the end of the document"
            )),
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    fn document(mut self) -> Result<Json> {
        if self.peek() == Some('\u{feff}') {
            return Err(self.here("a JSON document must not start with a byte order mark"));
        }
        let value = self.value()?;
        self.skip_whitespace();
        if self.peek().is_some() {
            return Err(self.unexpected("the end of the document"));
        }
        Ok(value)
    }

    /// Read one value, iteratively: containers are kept on an explicit stack.
    fn value(&mut self) -> Result<Json> {
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            // Read the start of a value: a scalar, or an opening bracket.
            self.skip_whitespace();
            let mut done = match self.peek() {
                Some('{' | '[') => {
                    if stack.len() + 1 > self.limits.max_nesting {
                        return Err(Error::new(
                            ErrorCode::LimitExceeded,
                            format!(
                                "arrays and objects are nested more than {} deep",
                                self.limits.max_nesting
                            ),
                        )
                        .at_position(self.line, self.column));
                    }
                    let open = self.bump();
                    self.skip_whitespace();
                    if open == Some('[') {
                        if self.peek() == Some(']') {
                            self.bump();
                            Json::Array(Vec::new())
                        } else {
                            stack.push(Frame::Array(Vec::new()));
                            continue;
                        }
                    } else if self.peek() == Some('}') {
                        self.bump();
                        Json::Object(Vec::new())
                    } else {
                        let key = self.key(&HashSet::new())?;
                        stack.push(Frame::Object {
                            members: Vec::new(),
                            keys: HashSet::from([key.clone()]),
                            key: Some(key),
                        });
                        continue;
                    }
                }
                _ => self.scalar()?,
            };
            // Attach the finished value to its container, closing containers as needed.
            loop {
                let Some(frame) = stack.last_mut() else {
                    return Ok(done);
                };
                self.skip_whitespace();
                match frame {
                    Frame::Array(items) => {
                        items.push(done);
                        match self.peek() {
                            Some(',') => {
                                self.bump();
                                break;
                            }
                            Some(']') => {
                                self.bump();
                            }
                            _ => return Err(self.unexpected("',' or ']'")),
                        }
                    }
                    Frame::Object { members, keys, key } => {
                        let name = key.take().unwrap_or_default();
                        members.push((name, done));
                        match self.peek() {
                            Some(',') => {
                                self.bump();
                                self.skip_whitespace();
                                let next = self.key(keys)?;
                                keys.insert(next.clone());
                                *key = Some(next);
                                break;
                            }
                            Some('}') => {
                                self.bump();
                            }
                            _ => return Err(self.unexpected("',' or '}'")),
                        }
                    }
                }
                // The container is closed: it becomes the finished value.
                done = match stack.pop() {
                    Some(Frame::Array(items)) => Json::Array(items),
                    Some(Frame::Object { members, .. }) => Json::Object(members),
                    // Unreachable: `stack.last_mut()` just returned a frame.
                    None => Json::Null,
                };
            }
        }
    }

    /// Read `"key":`, rejecting a key already in the object.
    fn key(&mut self, keys: &HashSet<String>) -> Result<String> {
        if self.peek() != Some('"') {
            return Err(self.unexpected("a string key"));
        }
        let (line, column) = (self.line, self.column);
        let key = self.string()?;
        if keys.contains(&key) {
            return Err(syntax(
                line,
                column,
                format!("the key {key:?} appears twice in an object"),
            )
            .with_hint(
                "JSON readers disagree on which duplicate wins, so cleanrules rejects them",
            ));
        }
        self.skip_whitespace();
        if self.peek() != Some(':') {
            return Err(self.unexpected("':'"));
        }
        self.bump();
        Ok(key)
    }

    fn scalar(&mut self) -> Result<Json> {
        match self.peek() {
            Some('"') => Ok(Json::String(self.string()?)),
            Some('t') => self.literal("true", Json::Bool(true)),
            Some('f') => self.literal("false", Json::Bool(false)),
            Some('n') => self.literal("null", Json::Null),
            Some('-' | '0'..='9') => Ok(Json::Number(self.number()?)),
            _ => Err(self.unexpected("a value")),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json> {
        for expected in word.chars() {
            if self.peek() != Some(expected) {
                return Err(self.unexpected(&format!("{word:?}")));
            }
            self.bump();
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<JsonNumber> {
        let start = self.offset;
        if self.peek() == Some('-') {
            self.bump();
        }
        match self.peek() {
            Some('0') => {
                self.bump();
            }
            Some('1'..='9') => self.digits(),
            _ => return Err(self.unexpected("a digit")),
        }
        if self.peek() == Some('.') {
            self.bump();
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.unexpected("a digit"));
            }
            self.digits();
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.unexpected("a digit"));
            }
            self.digits();
        }
        Ok(JsonNumber {
            text: self.text[start..self.offset].to_owned(),
        })
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some('0'..='9')) {
            self.bump();
        }
    }

    fn string(&mut self) -> Result<String> {
        self.bump(); // the opening quote
        let mut out = String::new();
        loop {
            let (line, column) = (self.line, self.column);
            match self.bump() {
                None => return Err(self.here("a string is not closed")),
                Some('"') => return Ok(out),
                Some('\\') => {
                    let escaped = match self.bump() {
                        Some('"') => '"',
                        Some('\\') => '\\',
                        Some('/') => '/',
                        Some('b') => '\u{8}',
                        Some('f') => '\u{c}',
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some('u') => self.unicode_escape(line, column)?,
                        _ => return Err(syntax(line, column, "an invalid escape in a string")),
                    };
                    out.push(escaped);
                }
                Some(c) if c < '\u{20}' => {
                    return Err(syntax(
                        line,
                        column,
                        "a control character must be escaped in a string",
                    ));
                }
                Some(c) => out.push(c),
            }
        }
    }

    /// The character of a `\u` escape, with the surrogate pair it may start.
    /// `line` and `column` locate the escape's backslash.
    fn unicode_escape(&mut self, line: u64, column: u64) -> Result<char> {
        let high = self.hex4(line, column)?;
        if (0xDC00..0xE000).contains(&high) {
            return Err(lone_surrogate(line, column));
        }
        if !(0xD800..0xDC00).contains(&high) {
            return char::from_u32(high).ok_or_else(|| lone_surrogate(line, column));
        }
        // A high surrogate must be followed by a low one.
        let rest = &self.text[self.offset..];
        if !rest.starts_with("\\u") {
            return Err(lone_surrogate(line, column));
        }
        let (low_line, low_column) = (self.line, self.column);
        self.bump();
        self.bump();
        let low = self.hex4(low_line, low_column)?;
        if !(0xDC00..0xE000).contains(&low) {
            return Err(lone_surrogate(line, column));
        }
        let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
        char::from_u32(code).ok_or_else(|| lone_surrogate(line, column))
    }

    fn hex4(&mut self, line: u64, column: u64) -> Result<u32> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self.bump().and_then(|c| c.to_digit(16));
            let Some(digit) = digit else {
                return Err(syntax(
                    line,
                    column,
                    "a \\u escape needs four hexadecimal digits",
                ));
            };
            value = value * 16 + digit;
        }
        Ok(value)
    }
}

fn lone_surrogate(line: u64, column: u64) -> Error {
    syntax(
        line,
        column,
        "a lone surrogate escape is not a Unicode character",
    )
}

/// Write JSON text: compact, members in order, numbers as their literals, and
/// strings escaping only `"`, `\` and control characters.
#[must_use]
pub fn to_string(value: &Json) -> String {
    let mut out = String::new();
    // The open arrays and objects, each with its items still to write and
    // whether it has written one: an explicit stack, so that no value within
    // the limits overflows the call stack.
    let mut open: Vec<Open<'_>> = Vec::new();
    let mut value = value;
    loop {
        match value {
            Json::Array(items) => {
                out.push('[');
                open.push(Open::Array(items.iter(), true));
            }
            Json::Object(members) => {
                out.push('{');
                open.push(Open::Object(members.iter(), true));
            }
            scalar => write_scalar(&mut out, scalar),
        }
        // The next item of the innermost open collection, closing those done.
        loop {
            let Some(top) = open.last_mut() else {
                return out;
            };
            let (next, first, close) = match top {
                Open::Array(items, first) => {
                    let next = items.next();
                    if next.is_some() && !*first {
                        out.push(',');
                    }
                    (next, first, ']')
                }
                Open::Object(members, first) => {
                    let next = members.next().map(|(key, item)| {
                        if !*first {
                            out.push(',');
                        }
                        write_string(&mut out, key);
                        out.push(':');
                        item
                    });
                    (next, first, '}')
                }
            };
            if let Some(item) = next {
                *first = false;
                value = item;
                break;
            }
            out.push(close);
            open.pop();
        }
    }
}

/// An array or object being written: its items still to write, and whether
/// none has been written yet.
enum Open<'a> {
    Array(std::slice::Iter<'a, Json>, bool),
    Object(std::slice::Iter<'a, (String, Json)>, bool),
}

fn write_scalar(out: &mut String, value: &Json) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Number(n) => out.push_str(n.text()),
        Json::String(s) => write_string(out, s),
        // Collections are written by `to_string`.
        Json::Array(_) | Json::Object(_) => {}
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c < '\u{20}' => {
                // Writing to a String cannot fail.
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Typed equality of JSON trees (`spec/json-format.md`, *Comparing documents*):
/// members in order, numbers by lexical class and exact value, and the sign of a
/// float zero.
#[must_use]
pub fn same(a: &Json, b: &Json) -> bool {
    // Pairs still to compare, with an explicit stack (see `to_string`), which
    // two values without items never need.
    let mut pending = Vec::new();
    let mut next = Some((a, b));
    while let Some((a, b)) = next.take().or_else(|| pending.pop()) {
        let equal = match (a, b) {
            (Json::Number(x), Json::Number(y)) => {
                x.is_integer_literal() == y.is_integer_literal()
                    && if x.is_integer_literal() {
                        x.as_i128().is_some() && x.as_i128() == y.as_i128()
                    } else {
                        x.as_f64().to_bits() == y.as_f64().to_bits()
                    }
            }
            (Json::Array(x), Json::Array(y)) => {
                pending.extend(x.iter().zip(y));
                x.len() == y.len()
            }
            (Json::Object(x), Json::Object(y)) => {
                x.len() == y.len()
                    && x.iter().zip(y).all(|((k, p), (l, q))| {
                        pending.push((p, q));
                        k == l
                    })
            }
            (Json::Null, Json::Null) => true,
            (Json::Bool(x), Json::Bool(y)) => x == y,
            (Json::String(x), Json::String(y)) => x == y,
            _ => false,
        };
        if !equal {
            return false;
        }
    }
    true
}

/// Escape a key for a JSON Pointer: `~` becomes `~0` and `/` becomes `~1`.
#[must_use]
pub fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::{Json, Limits, parse, parse_bytes, pointer_token, to_string};
    use crate::ErrorCode;

    fn ok(text: &str) -> Json {
        parse(text, &Limits::default()).unwrap()
    }

    fn at(text: &str) -> (ErrorCode, Option<u64>, Option<u64>) {
        let error = parse(text, &Limits::default()).unwrap_err();
        (error.code(), error.context().line, error.context().column)
    }

    #[test]
    fn values_round_trip_in_order() {
        let text = r#"{"b":1,"a":[true,false,null,"x\n\"y\"",-0,1.0,1e2],"10":{},"2":[]}"#;
        assert_eq!(to_string(&ok(text)), text);
    }

    #[test]
    fn numbers_keep_their_text() {
        let Json::Array(items) = &ok("[1, 1.0, -0, 9223372036854775808, 1e400]") else {
            panic!()
        };
        let numbers: Vec<_> = items
            .iter()
            .map(|v| match v {
                Json::Number(n) => n.clone(),
                _ => panic!(),
            })
            .collect();
        assert!(numbers[0].is_integer_literal() && !numbers[1].is_integer_literal());
        assert_eq!(numbers[2].as_i128(), Some(0));
        assert_eq!(numbers[3].as_i128(), Some(9_223_372_036_854_775_808));
        assert!(numbers[4].as_f64().is_infinite());
        assert_eq!(numbers[1].as_i128(), None);
    }

    #[test]
    fn surrogate_pairs_and_escapes() {
        assert_eq!(ok(r#""😀é\/""#), Json::String("😀é/".into()));
        assert_eq!(
            at(r#"["x\ud800"]"#),
            (ErrorCode::FormatError, Some(1), Some(4))
        );
        assert_eq!(
            at(r#"["\udc00"]"#),
            (ErrorCode::FormatError, Some(1), Some(3))
        );
        assert_eq!(
            at(r#"["\ud800A"]"#),
            (ErrorCode::FormatError, Some(1), Some(3))
        );
    }

    #[test]
    fn syntax_error_locations() {
        assert_eq!(
            at(r#"{"a":1,"a":2}"#),
            (ErrorCode::FormatError, Some(1), Some(8))
        );
        assert_eq!(at("\u{feff}{}"), (ErrorCode::FormatError, Some(1), Some(1)));
        assert_eq!(at("{} x"), (ErrorCode::FormatError, Some(1), Some(4)));
        assert_eq!(
            at(r#"{"version":1,"#),
            (ErrorCode::FormatError, Some(1), Some(14))
        );
        assert_eq!(at("[\"😀é\",]"), (ErrorCode::FormatError, Some(1), Some(7)));
        assert_eq!(
            at("{\r\n\"a\":[1,]\r\n}"),
            (ErrorCode::FormatError, Some(2), Some(8))
        );
        assert_eq!(at("[01]"), (ErrorCode::FormatError, Some(1), Some(3)));
        assert_eq!(at("[1.]"), (ErrorCode::FormatError, Some(1), Some(4)));
        assert_eq!(at("[\"a\tb\"]"), (ErrorCode::FormatError, Some(1), Some(4)));
        assert_eq!(at("[tru]"), (ErrorCode::FormatError, Some(1), Some(5)));
        assert_eq!(at(r#"{"a" 1}"#), (ErrorCode::FormatError, Some(1), Some(6)));
        assert_eq!(
            at(r#"{"a":1 "b":2}"#),
            (ErrorCode::FormatError, Some(1), Some(8))
        );
        assert_eq!(at("[1e]"), (ErrorCode::FormatError, Some(1), Some(4)));
        assert_eq!(at("[1e+]"), (ErrorCode::FormatError, Some(1), Some(5)));
        assert_eq!(
            at(r#"["\u12g4"]"#),
            (ErrorCode::FormatError, Some(1), Some(3))
        );
        assert_eq!(
            at(r#"["\ud800\u0041"]"#),
            (ErrorCode::FormatError, Some(1), Some(3))
        );
    }

    #[test]
    fn control_characters_are_escaped() {
        let text = r#""\b\f\u0001""#;
        assert_eq!(ok(text), Json::String("\u{8}\u{c}\u{1}".into()));
        assert_eq!(to_string(&ok(text)), text);
    }

    #[test]
    fn limits() {
        let deep = format!("{}{}", "[".repeat(5), "]".repeat(5));
        let limits = Limits {
            max_nesting: 4,
            ..Limits::default()
        };
        let error = parse(&deep, &limits).unwrap_err();
        assert_eq!(
            (error.code(), error.context().column),
            (ErrorCode::LimitExceeded, Some(5))
        );
        assert!(
            parse(
                &deep,
                &Limits {
                    max_nesting: 5,
                    ..Limits::default()
                }
            )
            .is_ok()
        );
        let small = Limits {
            max_size: 3,
            ..Limits::default()
        };
        assert_eq!(
            parse("[1,2]", &small).unwrap_err().code(),
            ErrorCode::LimitExceeded
        );
        // Far deeper than recursion allows: reading and dropping are iterative.
        let very_deep = format!("{}{}", "[".repeat(200_000), "]".repeat(200_000));
        let huge = Limits {
            max_nesting: 1_000_000,
            ..Limits::default()
        };
        assert!(parse(&very_deep, &huge).is_ok());
    }

    #[test]
    fn invalid_utf8() {
        let error = parse_bytes(b"[\"a\xff\"]", &Limits::default()).unwrap_err();
        assert_eq!(
            (error.code(), error.context().column),
            (ErrorCode::FormatError, Some(4))
        );
        let error = parse_bytes(b"[\n\"\xff\"]", &Limits::default()).unwrap_err();
        assert_eq!(
            (error.context().line, error.context().column),
            (Some(2), Some(2))
        );
        assert_eq!(parse_bytes(b"[1]", &Limits::default()).unwrap(), ok("[1]"));
        let small = Limits {
            max_size: 2,
            ..Limits::default()
        };
        assert_eq!(
            parse_bytes(b"[1]", &small).unwrap_err().code(),
            ErrorCode::LimitExceeded
        );
    }

    #[test]
    fn pointer_tokens() {
        assert_eq!(pointer_token("a/b~c"), "a~1b~0c");
    }
}
