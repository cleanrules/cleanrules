//! The text syntax of conditions, rules and rule sets (`spec/text.md`):
//!
//! ```text
//! RULE vip PRIORITY 10
//! IF customer.years > 5 AND order.total > 1000
//! THEN discount = 0.15
//! ```
//!
//! [`parse`] reads a text into a [`Document`] and [`parse_rule`] one rule, with
//! `rule_syntax_error` at the line and column of the first token they cannot
//! accept; [`to_string`] writes the canonical text back. Conditions also print
//! in this form with `Display`.

use std::fmt::{self, Write as _};

use crate::ast::{ArithOp, Closed, CompareOp, Expr, Node, StringOp};
use crate::error::{Error, ErrorCode, Result, Segment};
use crate::format::{Document, ValueContext, read_value, write_value};
use crate::json::{Json, JsonNumber, Limits};
use crate::logic::LogicName;
use crate::rules::{Aggregate, Config, HitPolicy, Rule, RuleSet};
use crate::value::{NullCause, Value};

// --- Tokens -----------------------------------------------------------------------------

const KEYWORDS: [&str; 31] = [
    "IF",
    "THEN",
    "RULE",
    "PRIORITY",
    "TAGS",
    "DESCRIPTION",
    "HIT",
    "POLICY",
    "AGGREGATE",
    "LOGIC",
    "STRICT",
    "AND",
    "OR",
    "NOT",
    "IN",
    "BETWEEN",
    "CLOSED",
    "IS",
    "NULL",
    "TRUE",
    "FALSE",
    "MATCHES",
    "STARTS",
    "ENDS",
    "WITH",
    "CONTAINS",
    "ABS",
    "DATE",
    "DATETIME",
    "DECIMAL",
    "FLOAT",
];

/// The keyword `word` is, in upper case.
fn keyword(word: &str) -> Option<&'static str> {
    KEYWORDS
        .iter()
        .copied()
        .find(|k| k.eq_ignore_ascii_case(word))
}

/// An ASCII identifier, keyword or not: what can follow a `.` unquoted.
fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub(crate) fn is_name(word: &str) -> bool {
    is_identifier(word) && keyword(word).is_none()
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// A keyword, in upper case, and as written.
    Keyword(&'static str, String),
    /// A name, plain or between backticks.
    Name(String),
    String(String),
    /// A number literal, as written, without a sign.
    Number(String),
    Symbol(&'static str),
    /// Text the lexer cannot read: its message and hint. Nothing accepts it, so
    /// the parser reports it where it meets it.
    Invalid(String, String),
    End,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    line: u64,
    column: u64,
}

fn syntax(line: u64, column: u64, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::RuleSyntaxError, message).at_position(line, column)
}

const SYMBOLS: [&str; 21] = [
    "==", "!=", "<>", "<=", ">=", "=", "<", ">", "(", ")", "[", "]", ".", ",", "+", "-", "*", "/",
    "&", "|", "~",
];

/// Reads tokens one at a time, keeping the line and column of each.
struct Lexer<'a> {
    text: &'a str,
    /// The byte offset of the next character.
    i: usize,
    line: u64,
    column: u64,
    /// After an invalid token, only the end follows.
    done: bool,
}

impl Lexer<'_> {
    fn get(&self, i: usize) -> Option<char> {
        self.text.get(i..).and_then(|rest| rest.chars().next())
    }

    /// Move to byte offset `to`; `\r\n`, `\r` and `\n` each end a line.
    fn advance(&mut self, to: usize) {
        for c in self.text[self.i..to].chars() {
            self.i += c.len_utf8();
            if c == '\n' || (c == '\r' && self.get(self.i) != Some('\n')) {
                self.line += 1;
                self.column = 1;
            } else if c != '\r' {
                self.column += 1;
            }
        }
    }

    /// The offset just past the run of characters from `from` that `keep` accepts.
    fn run(&self, from: usize, keep: impl Fn(char) -> bool) -> usize {
        self.text[from..]
            .char_indices()
            .find(|&(_, c)| !keep(c))
            .map_or(self.text.len(), |(j, _)| from + j)
    }

    /// The next token: the end after the text or after an invalid token.
    fn token(&mut self) -> Token {
        loop {
            match self.get(self.i) {
                Some(' ' | '\t' | '\n' | '\r') => self.advance(self.i + 1),
                // A byte order mark may start the text.
                Some('\u{feff}') if self.i == 0 => self.i += '\u{feff}'.len_utf8(),
                Some('#') => self.advance(self.run(self.i, |c| c != '\n' && c != '\r')),
                _ => break,
            }
        }
        let (line, column) = (self.line, self.column);
        let kind = match self.get(self.i) {
            _ if self.done => Kind::End,
            None => Kind::End,
            Some('a'..='z' | 'A'..='Z' | '_') => self.word(),
            Some('0'..='9') => self.number(),
            Some('"') => self.json_string(),
            Some('\'') => self.quoted('\'').map_or_else(|k| k, Kind::String),
            Some('`') => self.quoted('`').map_or_else(|k| k, Kind::Name),
            Some(c) => self.symbol(c),
        };
        if matches!(kind, Kind::Invalid(..)) {
            self.done = true;
        }
        Token { kind, line, column }
    }

    fn word(&mut self) -> Kind {
        let end = self.run(self.i, |c| c.is_ascii_alphanumeric() || c == '_');
        let word = self.text[self.i..end].to_owned();
        self.advance(end);
        match keyword(&word) {
            Some(upper) => Kind::Keyword(upper, word),
            None => Kind::Name(word),
        }
    }

    fn number(&mut self) -> Kind {
        let digit = |c: char| c.is_ascii_digit();
        let mut end = self.run(self.i, digit);
        if self.get(self.i) == Some('0') && end > self.i + 1 {
            return Kind::Invalid(
                "a number cannot start with 0 followed by digits".into(),
                "write 7, not 007, as in JSON".into(),
            );
        }
        if self.get(end) == Some('.') && self.get(end + 1).is_some_and(digit) {
            end = self.run(end + 1, digit);
        }
        if matches!(self.get(end), Some('e' | 'E')) {
            let mut exponent = end + 1;
            if matches!(self.get(exponent), Some('+' | '-')) {
                exponent += 1;
            }
            if self.get(exponent).is_some_and(digit) {
                end = self.run(exponent, digit);
            }
        }
        let number = self.text[self.i..end].to_owned();
        self.advance(end);
        Kind::Number(number)
    }

    /// A string between double quotes, with the escapes of JSON.
    fn json_string(&mut self) -> Kind {
        let bytes = self.text.as_bytes();
        let mut end = self.i + 1;
        while end < bytes.len() && bytes[end] != b'"' {
            end += if bytes[end] == b'\\' { 2 } else { 1 };
        }
        if end >= bytes.len() {
            return Kind::Invalid(
                "a string is not closed".into(),
                "end it with a double quote".into(),
            );
        }
        let Ok(Json::String(ref value)) =
            crate::json::parse(&self.text[self.i..=end], &Limits::default())
        else {
            return Kind::Invalid(
                "a string is not valid".into(),
                "strings use the escapes of JSON, such as \\n and \\u00e9, and no control characters"
                    .into(),
            );
        };
        let value = value.clone();
        self.advance(end + 1);
        Kind::String(value)
    }

    /// A text between single quotes or backticks, in which the quote is
    /// written twice; an invalid token when it is not closed or holds a
    /// control character.
    fn quoted(&mut self, quote: char) -> std::result::Result<String, Kind> {
        let mut value = String::new();
        let mut end = self.i + 1;
        loop {
            match self.get(end) {
                None => {
                    return Err(Kind::Invalid(
                        "a quoted text is not closed".into(),
                        format!("end it with {quote}, and write {quote}{quote} for {quote}"),
                    ));
                }
                Some(c) if c == quote && self.get(end + 1) == Some(quote) => {
                    value.push(quote);
                    end += 2;
                }
                Some(c) if c == quote => break,
                Some(c) if c < ' ' => {
                    return Err(Kind::Invalid(
                        format!("a quoted text holds the control character {c:?}"),
                        "write strings with control characters between double quotes, with JSON escapes such as \\n".into(),
                    ));
                }
                Some(c) => {
                    value.push(c);
                    end += c.len_utf8();
                }
            }
        }
        self.advance(end + 1);
        Ok(value)
    }

    fn symbol(&mut self, c: char) -> Kind {
        let rest = &self.text[self.i..];
        let Some(symbol) = SYMBOLS.iter().find(|s| rest.starts_with(**s)) else {
            return Kind::Invalid(format!("unexpected character {c:?}"), String::new());
        };
        self.advance(self.i + symbol.len());
        Kind::Symbol(symbol)
    }
}

// --- Syntax tree --------------------------------------------------------------------------

/// A node as written, with where it starts; built into a [`Node`] afterwards.
struct Syn {
    shape: Shape,
    line: u64,
    column: u64,
    /// Nodes on the longest path down, this one included: the depth the node
    /// reaches as a condition or payload value (`json-format.md`).
    height: usize,
}

enum Shape {
    Field(Vec<Segment>),
    Const(Value),
    Compare(CompareOp, Box<Syn>, Box<Syn>),
    And(Vec<Syn>),
    Or(Vec<Syn>),
    Not(Box<Syn>),
    In(Box<Syn>, Vec<Value>),
    Between(Box<Syn>, Box<Syn>, Box<Syn>, Closed),
    IsNull(Box<Syn>),
    Matches(Box<Syn>, String),
    StringTest(StringOp, Box<Syn>, String),
    Arith(ArithOp, Box<Syn>, Box<Syn>),
    Abs(Box<Syn>),
    Neg(Box<Syn>),
    /// A node already built: the column a decision table's cell tests.
    Built(Node),
}

impl Syn {
    fn new(shape: Shape, line: u64, column: u64) -> Self {
        let mut syn = Syn {
            shape,
            line,
            column,
            height: 1,
        };
        syn.height = 1 + syn.children().iter().map(|c| c.height).max().unwrap_or(0);
        syn
    }

    /// A node already built, as tall as it is deep.
    fn built(node: Node, line: u64, column: u64) -> Self {
        let height = depth(&node);
        Syn {
            shape: Shape::Built(node),
            line,
            column,
            height,
        }
    }

    /// The node, located elsewhere: a parenthesized node starts at its
    /// parenthesis.
    fn relocated(self, line: u64, column: u64) -> Self {
        Syn {
            line,
            column,
            ..self
        }
    }

    fn children(&self) -> Vec<&Syn> {
        match &self.shape {
            Shape::Field(_) | Shape::Const(_) | Shape::Built(_) => vec![],
            Shape::Compare(_, a, b) | Shape::Arith(_, a, b) => vec![a, b],
            Shape::And(items) | Shape::Or(items) => items.iter().collect(),
            Shape::Not(a)
            | Shape::In(a, _)
            | Shape::IsNull(a)
            | Shape::Matches(a, _)
            | Shape::StringTest(_, a, _)
            | Shape::Abs(a)
            | Shape::Neg(a) => vec![a],
            Shape::Between(a, b, c, _) => vec![a, b, c],
        }
    }

    /// The core node, children first, each error located where its node starts.
    fn build(self) -> Result<Node> {
        let (line, column) = (self.line, self.column);
        let at = |error| locate_at(error, line, column);
        match self.shape {
            Shape::Field(path) => Node::field(path).map_err(at),
            Shape::Const(value) => Node::constant(value).map_err(at),
            Shape::Compare(op, a, b) => Node::compare(op, a.build()?, b.build()?).map_err(at),
            Shape::And(items) => {
                Node::and(items.into_iter().map(Syn::build).collect::<Result<_>>()?).map_err(at)
            }
            Shape::Or(items) => {
                Node::or(items.into_iter().map(Syn::build).collect::<Result<_>>()?).map_err(at)
            }
            Shape::Not(a) => Node::not(a.build()?).map_err(at),
            Shape::In(a, values) => Node::is_in(a.build()?, values).map_err(at),
            Shape::Between(a, low, high, closed) => {
                Node::between(a.build()?, low.build()?, high.build()?, closed).map_err(at)
            }
            Shape::IsNull(a) => Node::is_null(a.build()?).map_err(at),
            Shape::Matches(a, pattern) => Node::matches(a.build()?, &pattern).map_err(at),
            Shape::StringTest(op, a, text) => Node::string_test(op, a.build()?, text).map_err(at),
            Shape::Arith(op, a, b) => Node::arith(op, a.build()?, b.build()?).map_err(at),
            Shape::Abs(a) => Node::abs(a.build()?).map_err(at),
            Shape::Neg(a) => Node::neg(a.build()?).map_err(at),
            Shape::Built(node) => Ok(node),
        }
    }
}

// --- Parser --------------------------------------------------------------------------------

struct Parser<'a> {
    lexer: Lexer<'a>,
    /// The next token and the one after it.
    current: Token,
    following: Token,
    /// How deep the grammar has recursed: once per `expr`, which bounds the
    /// stack before the depth check bounds the tree.
    recursion: usize,
    max_recursion: usize,
    max_depth: usize,
}

fn describe(kind: &Kind) -> String {
    match kind {
        Kind::Keyword(word, _) => (*word).to_owned(),
        Kind::Name(name) => format!("the name {name:?}"),
        Kind::String(_) => "a string".into(),
        Kind::Number(number) => format!("the number {number}"),
        Kind::Symbol(symbol) => format!("{symbol:?}"),
        Kind::Invalid(..) => "invalid text".into(),
        Kind::End => "the end of the text".into(),
    }
}

/// The nodes on the longest path down from `node`, itself included.
fn depth(node: &Node) -> usize {
    1 + node.children().into_iter().map(depth).max().unwrap_or(0)
}

/// `NOT node` when `negated`, located where `node` starts.
fn negate(negated: bool, node: Syn) -> Syn {
    if negated {
        let (line, column) = (node.line, node.column);
        Syn::new(Shape::Not(Box::new(node)), line, column)
    } else {
        node
    }
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, limits: &Limits) -> Self {
        let mut lexer = Lexer {
            text,
            i: 0,
            line: 1,
            column: 1,
            done: false,
        };
        let current = lexer.token();
        let following = lexer.token();
        Parser {
            lexer,
            current,
            following,
            recursion: 0,
            // Each node costs at most two levels (its operator's and one
            // parenthesis), so a text at `max_depth` is never refused.
            max_recursion: limits.max_depth.saturating_mul(2).saturating_add(2),
            max_depth: limits.max_depth,
        }
    }

    fn peek(&self) -> &Token {
        &self.current
    }

    /// Take the next token; the end and invalid text stay.
    fn next(&mut self) -> Token {
        if matches!(self.current.kind, Kind::End | Kind::Invalid(..)) {
            return self.current.clone();
        }
        let after = self.lexer.token();
        let following = std::mem::replace(&mut self.following, after);
        std::mem::replace(&mut self.current, following)
    }

    fn is_keyword(&self, word: &str) -> bool {
        matches!(&self.peek().kind, Kind::Keyword(k, _) if *k == word)
    }

    fn is_symbol(&self, symbol: &str) -> bool {
        matches!(&self.peek().kind, Kind::Symbol(s) if *s == symbol)
    }

    fn accept_keyword(&mut self, word: &str) -> bool {
        let found = self.is_keyword(word);
        if found {
            self.next();
        }
        found
    }

    fn accept_symbol(&mut self, symbol: &str) -> bool {
        let found = self.is_symbol(symbol);
        if found {
            self.next();
        }
        found
    }

    /// The error at the next token, which is not what the grammar expects;
    /// for invalid text, the lexer's error.
    #[inline(never)]
    fn unexpected(&self, expected: &str) -> Error {
        let token = self.peek();
        if let Kind::Invalid(message, hint) = &token.kind {
            let error = syntax(token.line, token.column, message.clone());
            return if hint.is_empty() {
                error
            } else {
                error.with_hint(hint.clone())
            };
        }
        syntax(
            token.line,
            token.column,
            format!("expected {expected}, found {}", describe(&token.kind)),
        )
    }

    fn expect_keyword(&mut self, word: &str) -> Result<Token> {
        if self.is_keyword(word) {
            Ok(self.next())
        } else {
            Err(self.unexpected(word))
        }
    }

    fn expect_symbol(&mut self, symbol: &str) -> Result<Token> {
        if self.is_symbol(symbol) {
            Ok(self.next())
        } else {
            Err(self.unexpected(&format!("{symbol:?}")))
        }
    }

    fn enter(&mut self) -> Result<()> {
        self.recursion += 1;
        if self.recursion > self.max_recursion {
            let token = self.peek();
            return Err(Error::new(
                ErrorCode::LimitExceeded,
                format!(
                    "parentheses, NOT, - and ABS are nested more than {} deep",
                    self.max_recursion
                ),
            )
            .with_hint(
                "drop parentheses that change nothing, or raise max_depth if the text is trusted",
            )
            .at_position(token.line, token.column));
        }
        Ok(())
    }

    /// `syn`, unless it makes a condition or value deeper than `max_depth`:
    /// the error is then located where it starts. Checked as each node is
    /// made, so no tree taller than the limit is ever held.
    fn bounded(&self, syn: Syn) -> Result<Syn> {
        if syn.height > self.max_depth {
            return Err(Error::new(
                ErrorCode::LimitExceeded,
                format!(
                    "the condition is nested more than {} nodes deep",
                    self.max_depth
                ),
            )
            .at_position(syn.line, syn.column));
        }
        Ok(syn)
    }

    /// An expression whose operators bind at least as tightly as `min`
    /// (precedence climbing: two frames per level of nesting). The work that
    /// does not recurse is kept out of line, so that the frames stay small.
    fn expr(&mut self, min: u8) -> Result<Syn> {
        self.enter()?;
        let first = self.prefix(min)?;
        let mut left = self.bounded(first)?;
        let mut has_predicate = false;
        while let Some(level) = self.infix_level() {
            if level < min {
                break;
            }
            left = match level {
                OR | AND => self.chain(left, level)?,
                PREDICATE => {
                    if has_predicate {
                        if self.comparison().is_some() {
                            return Err(self.unexpected("one comparison at most").with_hint(
                                "write a range as x BETWEEN low AND high, or join comparisons with AND",
                            ));
                        }
                        break;
                    }
                    has_predicate = true;
                    self.predicate(left)?
                }
                _ => {
                    let op = match self.next().kind {
                        Kind::Symbol("+") => ArithOp::Add,
                        Kind::Symbol("-") => ArithOp::Sub,
                        Kind::Symbol("*") => ArithOp::Mul,
                        _ => ArithOp::Div,
                    };
                    let right = self.expr(level + 1)?;
                    let (line, column) = (left.line, left.column);
                    Syn::new(
                        Shape::Arith(op, Box::new(left), Box::new(right)),
                        line,
                        column,
                    )
                }
            };
            left = self.bounded(left)?;
        }
        self.recursion -= 1;
        Ok(left)
    }

    /// The level of the operator at the next token, if it is one.
    fn infix_level(&self) -> Option<u8> {
        match &self.peek().kind {
            Kind::Keyword(word, _) => match *word {
                "OR" => Some(OR),
                "AND" => Some(AND),
                "IS" | "IN" | "BETWEEN" | "MATCHES" | "STARTS" | "ENDS" | "CONTAINS" => {
                    Some(PREDICATE)
                }
                "NOT" if self.next_is_predicate_keyword() => Some(PREDICATE),
                _ => None,
            },
            Kind::Symbol("|") => Some(OR),
            Kind::Symbol("&") => Some(AND),
            Kind::Symbol("+" | "-") => Some(SUM),
            Kind::Symbol("*" | "/") => Some(PRODUCT),
            _ => self.comparison().map(|_| PREDICATE),
        }
    }

    /// `left OR …` or `left AND …`, flattened.
    #[inline(never)]
    fn chain(&mut self, left: Syn, level: u8) -> Result<Syn> {
        let (line, column) = (left.line, left.column);
        let mut items = vec![left];
        while self.infix_level() == Some(level) {
            self.next();
            items.push(self.expr(level + 1)?);
        }
        let shape = if level == OR {
            Shape::Or(items)
        } else {
            Shape::And(items)
        };
        Ok(Syn::new(shape, line, column))
    }

    /// What starts an expression: `NOT`, a minus, a parenthesis, `ABS` or a
    /// primary.
    fn prefix(&mut self, min: u8) -> Result<Syn> {
        let token = self.peek().clone();
        let located = |shape| Syn::new(shape, token.line, token.column);
        if min <= NOT && (self.accept_keyword("NOT") || self.accept_symbol("~")) {
            let operand = self.expr(NOT)?;
            return Ok(located(Shape::Not(Box::new(operand))));
        }
        if self.accept_symbol("-") {
            // A minus directly before a number is a negative constant.
            let next = self.peek();
            if matches!(next.kind, Kind::Number(_))
                && (next.line, next.column) == (token.line, token.column + 1)
            {
                return self.negative_number(&token);
            }
            let operand = self.expr(UNARY)?;
            return Ok(located(Shape::Neg(Box::new(operand))));
        }
        if self.accept_symbol("(") {
            let inner = self.expr(OR)?;
            self.expect_symbol(")")?;
            // A parenthesized node starts at its parenthesis.
            return Ok(inner.relocated(token.line, token.column));
        }
        if self.accept_keyword("ABS") {
            self.expect_symbol("(")?;
            let operand = self.expr(SUM)?;
            self.expect_symbol(")")?;
            return Ok(located(Shape::Abs(Box::new(operand))));
        }
        self.primary()
    }

    /// The negative constant after its minus.
    #[inline(never)]
    fn negative_number(&mut self, minus: &Token) -> Result<Syn> {
        let Kind::Number(number) = self.next().kind else {
            unreachable!("called before a number")
        };
        number_constant(&format!("-{number}"), minus)
    }

    /// A predicate after its operand: a comparison, `IS [NOT] NULL` or a
    /// predicate keyword, possibly after `NOT`.
    #[inline(never)]
    fn predicate(&mut self, left: Syn) -> Result<Syn> {
        if let Some(op) = self.comparison() {
            self.next();
            let right = self.expr(SUM)?;
            let (line, column) = (left.line, left.column);
            return Ok(Syn::new(
                Shape::Compare(op, Box::new(left), Box::new(right)),
                line,
                column,
            ));
        }
        if self.accept_keyword("IS") {
            let negated = self.accept_keyword("NOT");
            self.expect_keyword("NULL")?;
            let (line, column) = (left.line, left.column);
            let node = Syn::new(Shape::IsNull(Box::new(left)), line, column);
            return Ok(negate(negated, node));
        }
        let negated = self.accept_keyword("NOT");
        let node = self.test(left)?;
        Ok(negate(negated, node))
    }

    fn comparison(&self) -> Option<CompareOp> {
        match &self.peek().kind {
            Kind::Symbol("=" | "==") => Some(CompareOp::Eq),
            Kind::Symbol("!=" | "<>") => Some(CompareOp::Ne),
            Kind::Symbol("<") => Some(CompareOp::Lt),
            Kind::Symbol("<=") => Some(CompareOp::Le),
            Kind::Symbol(">") => Some(CompareOp::Gt),
            Kind::Symbol(">=") => Some(CompareOp::Ge),
            _ => None,
        }
    }

    /// The predicate that a keyword starts after `left`.
    fn test(&mut self, left: Syn) -> Result<Syn> {
        let (line, column) = (left.line, left.column);
        let left = Box::new(left);
        let shape = if self.accept_keyword("IN") {
            self.expect_symbol("(")?;
            let mut values = vec![self.constant()?];
            while self.accept_symbol(",") {
                values.push(self.constant()?);
            }
            self.expect_symbol(")")?;
            Shape::In(left, values)
        } else if self.accept_keyword("BETWEEN") {
            let low = self.expr(SUM)?;
            self.expect_keyword("AND")?;
            let high = self.expr(SUM)?;
            let closed = if self.accept_keyword("CLOSED") {
                self.closed()?
            } else {
                Closed::Both
            };
            Shape::Between(left, Box::new(low), Box::new(high), closed)
        } else if self.accept_keyword("MATCHES") {
            Shape::Matches(left, self.string()?)
        } else if self.accept_keyword("STARTS") {
            self.expect_keyword("WITH")?;
            Shape::StringTest(StringOp::StartsWith, left, self.string()?)
        } else if self.accept_keyword("ENDS") {
            self.expect_keyword("WITH")?;
            Shape::StringTest(StringOp::EndsWith, left, self.string()?)
        } else {
            self.expect_keyword("CONTAINS")?;
            Shape::StringTest(StringOp::Contains, left, self.string()?)
        };
        Ok(Syn::new(shape, line, column))
    }

    fn closed(&mut self) -> Result<Closed> {
        let found = match &self.peek().kind {
            Kind::Name(word) | Kind::Keyword(_, word) => {
                Closed::from_name(&word.to_ascii_lowercase())
            }
            _ => None,
        };
        let Some(closed) = found else {
            return Err(self.unexpected("BOTH, LEFT, RIGHT or NONE"));
        };
        self.next();
        Ok(closed)
    }

    fn next_is_predicate_keyword(&self) -> bool {
        matches!(
            &self.following.kind,
            Kind::Keyword(word, _) if matches!(
                *word,
                "IN" | "BETWEEN" | "MATCHES" | "STARTS" | "ENDS" | "CONTAINS"
            )
        )
    }

    /// The next token's value, if `pick` accepts it; else an error naming
    /// what was expected, at that token.
    fn take<T>(&mut self, expected: &str, pick: impl Fn(&Kind) -> Option<T>) -> Result<T> {
        match pick(&self.peek().kind) {
            Some(value) => {
                self.next();
                Ok(value)
            }
            None => Err(self.unexpected(expected)),
        }
    }

    fn string(&mut self) -> Result<String> {
        self.take("a string", |kind| match kind {
            Kind::String(text) => Some(text.clone()),
            _ => None,
        })
    }

    /// A constant: what `IN (…)` lists.
    fn constant(&mut self) -> Result<Value> {
        let token = self.peek().clone();
        match self.expr(UNARY)?.shape {
            Shape::Const(value) => Ok(value),
            _ => Err(syntax(token.line, token.column, "IN lists constants only")
                .with_hint("compare with fields one by one, joined with OR")),
        }
    }

    /// The constant of `DATE "…"` and the other typed literals, after the keyword.
    fn typed(&mut self, key: &str, token: &Token) -> Result<Syn> {
        let text_token = self.peek().clone();
        let text = self.string()?;
        // A finite float's form is a number, the others' a string.
        let inner = match crate::json::parse(&text, &Limits::default()) {
            Ok(number @ Json::Number(_)) if key == "$float" && text.trim() == text => number,
            _ => Json::String(text),
        };
        let json = Json::Object(vec![(key.into(), inner)]);
        let value = read_value(&json, "", ValueContext::Constant).map_err(|error| {
            // The messages of the typed forms, in the words of the text.
            let message = [
                ("$datetime", "DATETIME"),
                ("$decimal", "DECIMAL"),
                ("$float", "FLOAT"),
                ("$date", "DATE"),
            ]
            .iter()
            .fold(error.message().to_owned(), |m, (from, to)| {
                m.replace(from, to)
            });
            syntax(text_token.line, text_token.column, message)
        })?;
        Ok(Syn::new(Shape::Const(value), token.line, token.column))
    }

    #[inline(never)]
    fn primary(&mut self) -> Result<Syn> {
        let token = self.peek().clone();
        let located = |shape| Syn::new(shape, token.line, token.column);
        let constant = |value| Ok(located(Shape::Const(value)));
        match &token.kind {
            Kind::Number(number) => {
                self.next();
                number_constant(number, &token)
            }
            Kind::String(text) => {
                self.next();
                constant(Value::String(text.clone()))
            }
            Kind::Keyword(word, _) if PRIMARY_KEYWORDS.contains(word) => {
                self.next();
                match *word {
                    "TRUE" => constant(Value::Bool(true)),
                    "FALSE" => constant(Value::Bool(false)),
                    "NULL" => constant(Value::Null(NullCause::Null)),
                    "DATE" => self.typed("$date", &token),
                    "DATETIME" => self.typed("$datetime", &token),
                    "DECIMAL" => self.typed("$decimal", &token),
                    _ => self.typed("$float", &token),
                }
            }
            Kind::Name(name) => {
                self.next();
                let mut path = vec![Segment::Key(name.clone())];
                self.segments(&mut path)?;
                Ok(located(Shape::Field(path)))
            }
            Kind::Symbol("[") => {
                self.next();
                let mut path = vec![Segment::Index(self.index()?)];
                self.segments(&mut path)?;
                Ok(located(Shape::Field(path)))
            }
            _ => Err(self.unexpected("a field, a constant or a parenthesis")),
        }
    }

    /// An index after `[`, then the closing `]`.
    fn index(&mut self) -> Result<u64> {
        let token = self.peek().clone();
        let number = self.take("an index", |kind| match kind {
            Kind::Number(number) => Some(number.clone()),
            _ => None,
        })?;
        let index = number.parse::<u64>().map_err(|_| {
            syntax(
                token.line,
                token.column,
                "an index is a non-negative integer",
            )
        })?;
        self.expect_symbol("]")?;
        Ok(index)
    }

    fn segments(&mut self, path: &mut Vec<Segment>) -> Result<()> {
        loop {
            if self.accept_symbol(".") {
                // Only a key follows a dot, so a keyword is a key there.
                let name = self
                    .take("a key after \".\"", |kind| match kind {
                        Kind::Name(name) | Kind::Keyword(_, name) => Some(name.clone()),
                        _ => None,
                    })
                    .map_err(|error| {
                        error.with_hint("a key that is not an identifier goes between backticks")
                    })?;
                path.push(Segment::Key(name));
            } else if self.accept_symbol("[") {
                path.push(Segment::Index(self.index()?));
            } else {
                return Ok(());
            }
        }
    }

    // --- Rules -----------------------------------------------------------------------

    /// A name or a string: a rule name, a tag or a payload key.
    fn label(&mut self, what: &str) -> Result<String> {
        self.take(what, |kind| match kind {
            Kind::Name(name) | Kind::String(name) => Some(name.clone()),
            _ => None,
        })
    }

    fn priority(&mut self) -> Result<i64> {
        let token = self.peek().clone();
        let negative = self.accept_symbol("-");
        let number = self.take("an integer priority", |kind| match kind {
            Kind::Number(number) => Some(number.clone()),
            _ => None,
        })?;
        let text = if negative {
            format!("-{number}")
        } else {
            number
        };
        text.parse()
            .map_err(|_| syntax(token.line, token.column, "a priority is a 64-bit integer"))
    }

    fn rule(&mut self, position: usize) -> Result<(Rule, Token)> {
        let start = self.peek().clone();
        let mut name = format!("rule_{position}");
        let (mut priority, mut tags, mut description) = (0i64, Vec::new(), None);
        if self.accept_keyword("RULE") {
            name = self.label("a rule name")?;
            if self.accept_keyword("PRIORITY") {
                priority = self.priority()?;
            }
            if self.accept_keyword("TAGS") {
                tags.push(self.label("a tag")?);
                while self.accept_symbol(",") {
                    tags.push(self.label("a tag")?);
                }
            }
            if self.accept_keyword("DESCRIPTION") {
                description = Some(self.string()?);
            }
        }
        self.expect_keyword("IF")?;
        let when = self.expr(OR)?;
        let mut then: Vec<(String, Syn)> = Vec::new();
        if self.accept_keyword("THEN") {
            loop {
                let key_token = self.peek().clone();
                let key = self.label("a payload key")?;
                if then.iter().any(|(k, _)| *k == key) {
                    let mut error = Error::new(
                        ErrorCode::RuleDefinitionError,
                        format!("the payload key {key:?} appears twice in rule {name:?}"),
                    )
                    .with_hint("give each payload key one value")
                    .at_position(key_token.line, key_token.column);
                    error.context_mut().rule = Some(name);
                    return Err(error);
                }
                self.expect_symbol("=")?;
                // A condition here reads, so that it gets the error of JSON.
                let value = self.expr(OR)?;
                then.push((key, value));
                if !self.accept_symbol(",") {
                    break;
                }
            }
        }
        let positions: Vec<(u64, u64)> = std::iter::once(&when)
            .chain(then.iter().map(|(_, value)| value))
            .map(|syn| (syn.line, syn.column))
            .collect();
        let when = when.build()?;
        let payload = then
            .into_iter()
            .map(|(key, value)| value.build().map(|node| (key, node)))
            .collect::<Result<Vec<_>>>()?;
        // A rule error is located at the condition or payload value it is
        // about, else at the rule.
        let at = if when.is_null_constant() || when.kind() == crate::ast::Kind::Value {
            Some(positions[0])
        } else {
            payload
                .iter()
                .position(|(_, node)| node.kind() == crate::ast::Kind::Condition)
                .map(|i| positions[i + 1])
        };
        let mut rule = Rule::with_payload(name, when, payload)
            .map_err(|error| match at {
                Some((line, column)) => locate_at(error, line, column),
                None => locate(error, &start),
            })?
            .priority(priority)
            .tags(tags);
        if let Some(description) = description {
            rule = rule.description(description);
        }
        Ok((rule, start))
    }

    /// A name in a header line, in lower case.
    fn header_name(&mut self, what: &str) -> Result<(String, Token)> {
        let token = self.peek().clone();
        let name = self.take(what, |kind| match kind {
            Kind::Name(name) | Kind::Keyword(_, name) => Some(name.to_ascii_lowercase()),
            _ => None,
        })?;
        Ok((name, token))
    }

    fn header(&mut self) -> Result<Config> {
        let mut config = Config::default();
        if self.accept_keyword("HIT") {
            self.expect_keyword("POLICY")?;
            let (name, token) = self.header_name("a hit policy")?;
            config.hit_policy = HitPolicy::from_name(&name).ok_or_else(|| {
                syntax(
                    token.line,
                    token.column,
                    format!("unknown hit policy {name:?}"),
                )
                .with_hint("use first, priority, unique, any, rule_order or collect")
            })?;
        }
        if self.accept_keyword("AGGREGATE") {
            let (name, token) = self.header_name("an aggregate")?;
            config.aggregate = Some(Aggregate::from_name(&name).ok_or_else(|| {
                syntax(
                    token.line,
                    token.column,
                    format!("unknown aggregate {name:?}"),
                )
                .with_hint("use sum, min, max or count")
            })?);
        }
        if self.accept_keyword("LOGIC") {
            let (name, token) = self.header_name("a logic")?;
            config.logic =
                Some(LogicName::from_name(&name).map_err(|error| locate(error, &token))?);
        }
        if self.accept_keyword("STRICT") {
            config.strict = true;
        }
        Ok(config)
    }

    fn is_header_keyword(&self) -> bool {
        ["HIT", "AGGREGATE", "LOGIC", "STRICT"]
            .iter()
            .any(|k| self.is_keyword(k))
    }

    /// One rule and the end of the text.
    fn single_rule(&mut self) -> Result<Rule> {
        if !self.is_keyword("RULE") && !self.is_keyword("IF") {
            let hint = if self.is_header_keyword() {
                "a text with a header is a rule set: read it as a document"
            } else {
                "start a rule with IF, or with a RULE line; a condition alone is read as a document"
            };
            return Err(self.unexpected("RULE or IF").with_hint(hint));
        }
        let (rule, _) = self.rule(1)?;
        if !matches!(self.peek().kind, Kind::End) {
            return Err(self
                .unexpected("the end of the text")
                .with_hint("a text with several rules is a rule set: read it as a document"));
        }
        Ok(rule)
    }

    fn document(&mut self) -> Result<Document> {
        let first = self.peek().clone();
        let has_header = self.is_header_keyword();
        if !has_header && !self.is_keyword("RULE") && !self.is_keyword("IF") {
            return self.condition_document();
        }
        let config = self.header()?;
        let (mut rules, mut starts) = (Vec::new(), Vec::new());
        while !matches!(self.peek().kind, Kind::End) {
            if !self.is_keyword("RULE") && !self.is_keyword("IF") {
                let error = self.unexpected("RULE, IF or the end of the text");
                return Err(if self.is_header_keyword() {
                    error.with_hint(
                        "header lines come first, in this order: HIT POLICY, AGGREGATE, LOGIC, STRICT",
                    )
                } else {
                    error
                });
            }
            let (rule, start) = self.rule(rules.len() + 1)?;
            rules.push(rule);
            starts.push(start);
        }
        if !has_header && rules.len() == 1 {
            return Ok(Document::Rule(rules.remove(0)));
        }
        let names: Vec<String> = rules.iter().map(|r| r.name().to_owned()).collect();
        RuleSet::new(rules, config)
            .map(Document::RuleSet)
            .map_err(|error| {
                // A rule set error is located at the rule it names, else at the start.
                let token = error
                    .context()
                    .rule
                    .as_ref()
                    .and_then(|name| names.iter().rposition(|n| n == name))
                    .map_or(&first, |i| &starts[i]);
                locate(error, token)
            })
    }

    // --- Decision tables (`spec/tables.md`) -------------------------------------------

    fn end(&self, what: &str) -> Result<()> {
        if matches!(self.peek().kind, Kind::End) {
            Ok(())
        } else {
            Err(self.unexpected(&format!("the end of the {what}")))
        }
    }

    /// A whole text that is one expression: a table's input or output.
    fn expression(&mut self) -> Result<Node> {
        let syn = self.expr(OR)?;
        self.end("expression")?;
        syn.build()
    }

    /// An input cell testing `subject`: `None` for `-` or nothing, else the
    /// predicate, or the equality to a bare value.
    fn cell(&mut self, subject: &Node) -> Result<Option<Node>> {
        if matches!(self.peek().kind, Kind::End)
            || (self.is_symbol("-") && matches!(self.following.kind, Kind::End))
        {
            return Ok(None);
        }
        let start = self.peek().clone();
        let left = Syn::built(subject.clone(), start.line, start.column);
        let syn = if self.infix_level() == Some(PREDICATE) {
            self.predicate(left)?
        } else {
            let value = self.expr(SUM)?;
            Syn::new(
                Shape::Compare(CompareOp::Eq, Box::new(left), Box::new(value)),
                start.line,
                start.column,
            )
        };
        let syn = self.bounded(syn)?;
        if !matches!(self.peek().kind, Kind::End) {
            return Err(self
                .unexpected("the end of the cell")
                .with_hint("a cell leaves out its column: write > 5, IN ('a', 'b') or a value"));
        }
        syn.build().map(Some)
    }

    /// An output column after `THEN`: its key and its output values.
    fn output(&mut self) -> Result<(String, Vec<Value>)> {
        let key = self.label("an output key")?;
        let mut values = Vec::new();
        if self.accept_keyword("IN") {
            self.expect_symbol("(")?;
            values.push(self.constant()?);
            while self.accept_symbol(",") {
                values.push(self.constant()?);
            }
            self.expect_symbol(")")?;
        }
        self.end("output column")?;
        Ok((key, values))
    }

    fn condition_document(&mut self) -> Result<Document> {
        let condition = self.expr(OR)?;
        if !matches!(self.peek().kind, Kind::End) {
            return Err(self.unexpected("AND, OR or the end of the text"));
        }
        let (line, column) = (condition.line, condition.column);
        let node = condition.build()?;
        // As in a JSON condition document.
        if node.is_null_constant() || node.kind() == crate::ast::Kind::Value {
            return Err(Error::new(
                ErrorCode::ExpressionError,
                format!("'{}' is not a condition", node.tag()),
            )
            .at_position(line, column));
        }
        Ok(Document::Condition(node))
    }
}

const PRIMARY_KEYWORDS: [&str; 7] = [
    "TRUE", "FALSE", "NULL", "DATE", "DATETIME", "DECIMAL", "FLOAT",
];

// The levels of `spec/text.md`, *Conditions and values*, loosest first.
const OR: u8 = 1;
const AND: u8 = 2;
const NOT: u8 = 3;
const PREDICATE: u8 = 4;
const SUM: u8 = 5;
const PRODUCT: u8 = 6;
const UNARY: u8 = 7;

/// `error` located at `token`, unless it already has a position.
fn locate(error: Error, token: &Token) -> Error {
    locate_at(error, token.line, token.column)
}

fn locate_at(error: Error, line: u64, column: u64) -> Error {
    if error.context().line.is_some() {
        error
    } else {
        error.at_position(line, column)
    }
}

/// An int, a decimal (with a fraction) or a float (with an exponent), as the
/// format reads them; `text` may start with `-`.
fn number_constant(text: &str, token: &Token) -> Result<Syn> {
    let json = if text.contains(['e', 'E']) {
        Json::Object(vec![(
            "$float".into(),
            Json::Number(JsonNumber::from_literal(text)),
        )])
    } else if text.contains('.') {
        Json::Object(vec![("$decimal".into(), Json::String(text.into()))])
    } else {
        Json::Number(JsonNumber::from_literal(text))
    };
    // A literal is a valid number, so only its range can be wrong.
    let value = read_value(&json, "", ValueContext::Constant).map_err(|error| {
        syntax(token.line, token.column, error.message().to_owned()).with_hint(
            if text.contains(['e', 'E']) {
                r#"write FLOAT "inf" for an infinite float"#
            } else if text.contains('.') {
                "a decimal holds at most 38 significant digits; write a float, with an exponent"
            } else {
                "an int has 64 bits; write a larger number as a decimal, with a fraction"
            },
        )
    })?;
    Ok(Syn::new(Shape::Const(value), token.line, token.column))
}

/// Read a text in the rule syntax (`spec/text.md`): a condition, a rule or a
/// rule set.
///
/// # Errors
///
/// `rule_syntax_error` with the line and column of the first token that cannot
/// be accepted; `expression_error` and `rule_definition_error` located where
/// the node or rule starts; `registry_error` for an unknown logic;
/// `limit_exceeded` beyond the limits.
///
/// # Examples
///
/// ```
/// use cleanrules::format::Document;
/// use cleanrules::json::Limits;
///
/// let text = "x > 1 AND NOT blocked";
/// let Document::Condition(condition) = cleanrules::text::parse(text, &Limits::default())? else {
///     unreachable!()
/// };
/// assert_eq!(condition.to_string(), text);
/// # Ok::<(), cleanrules::Error>(())
/// ```
pub fn parse(text: &str, limits: &Limits) -> Result<Document> {
    check_size(text, limits)?;
    Parser::new(text, limits).document()
}

/// Read a text that holds exactly one rule.
///
/// # Errors
///
/// As [`parse`]; a condition or a rule set is a `rule_syntax_error` at the
/// token where the rule should start or the text end.
///
/// # Examples
///
/// ```
/// use cleanrules::format::Document;
/// use cleanrules::json::Limits;
///
/// let text = "RULE vip PRIORITY 10\nIF customer.years > 5 AND order.total > 1000\nTHEN discount = 0.15";
/// let rule = cleanrules::text::parse_rule(text, &Limits::default())?;
/// assert_eq!(rule.name(), "vip");
/// assert_eq!(cleanrules::text::to_string(&Document::Rule(rule))?, text);
/// # Ok::<(), cleanrules::Error>(())
/// ```
pub fn parse_rule(text: &str, limits: &Limits) -> Result<Rule> {
    check_size(text, limits)?;
    Parser::new(text, limits).single_rule()
}

/// A table's input or output: one expression (`spec/tables.md`).
pub(crate) fn parse_expression(text: &str, limits: &Limits) -> Result<Node> {
    check_size(text, limits)?;
    Parser::new(text, limits).expression()
}

/// A table's input cell, testing `subject` (`spec/tables.md`, *Input cells*).
pub(crate) fn parse_cell(text: &str, subject: &Node, limits: &Limits) -> Result<Option<Node>> {
    check_size(text, limits)?;
    Parser::new(text, limits).cell(subject)
}

/// A table's output column, after `THEN`: `key [IN (constant, …)]`.
pub(crate) fn parse_output(text: &str, limits: &Limits) -> Result<(String, Vec<Value>)> {
    check_size(text, limits)?;
    Parser::new(text, limits).output()
}

fn check_size(text: &str, limits: &Limits) -> Result<()> {
    if text.len() > limits.max_text_size {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            format!(
                "the text has {} bytes, more than the limit of {}",
                text.len(),
                limits.max_text_size
            ),
        )
        .with_hint("raise the size limit if the text is trusted"));
    }
    Ok(())
}

// --- Printing -------------------------------------------------------------------------------

/// The precedence level of a node (`spec/text.md`, *Conditions and values*).
fn level(node: &Node) -> u8 {
    match node.expr() {
        Expr::Or(_) => 1,
        Expr::And(_) => 2,
        Expr::Not(_) => 3,
        Expr::Compare { .. }
        | Expr::In { .. }
        | Expr::Between { .. }
        | Expr::IsNull(_)
        | Expr::Matches { .. }
        | Expr::StringTest { .. } => 4,
        Expr::Arith {
            op: ArithOp::Add | ArithOp::Sub,
            ..
        } => 5,
        Expr::Arith { .. } => 6,
        Expr::Neg(_) => 7,
        _ => 8,
    }
}

pub(crate) fn key(text: &str, quote: char) -> String {
    if is_name(text) {
        text.to_owned()
    } else if quote == '`' {
        // A control character cannot be in a name: written with a JSON escape,
        // which keeps the text on one line but does not read back.
        let mut out = String::from("`");
        for c in text.chars() {
            match c {
                '`' => out.push_str("``"),
                c if c < ' ' => {
                    let _ = write!(out, "\\u{:04x}", u32::from(c));
                }
                c => out.push(c),
            }
        }
        out.push('`');
        out
    } else {
        crate::json::to_string(&Json::String(text.to_owned()))
    }
}

pub(crate) fn path(segments: &[Segment]) -> String {
    let mut out = String::new();
    for (i, segment) in segments.iter().enumerate() {
        match segment {
            // After a dot only a key can follow, so any identifier is plain.
            Segment::Key(name) if i > 0 && is_identifier(name) => {
                out.push('.');
                out.push_str(name);
            }
            Segment::Key(name) => {
                if i > 0 {
                    out.push('.');
                }
                out.push_str(&key(name, '`'));
            }
            Segment::Index(index) => {
                let _ = write!(out, "[{index}]");
            }
        }
    }
    out
}

/// A finite float with the shortest digits that read back: plain digits and
/// `e0` when its magnitude is 0 or within [1e-4, 1e16) (`0.7e0`, `42500e0`), as
/// `{:e}` writes it otherwise (`1e-7`).
fn float_literal(value: f64) -> String {
    match positional(value) {
        Some(plain) => plain + "e0",
        None => format!("{value:e}"),
    }
}

/// A finite float in plain positional notation, with the shortest digits that
/// read back, when its magnitude is 0 or within [1e-4, 1e16).
pub(crate) fn positional(value: f64) -> Option<String> {
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    if !(-4..16).contains(&exponent) {
        return None;
    }
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits = mantissa.replace('.', "");
    let point = usize::try_from(exponent + 1).unwrap_or(0);
    let body = if exponent < 0 {
        let zeros = "0".repeat(usize::try_from(-exponent - 1).unwrap_or(0));
        format!("0.{zeros}{digits}")
    } else if digits.len() <= point {
        format!("{digits}{}", "0".repeat(point - digits.len()))
    } else {
        format!("{}.{}", &digits[..point], &digits[point..])
    };
    Some(format!("{sign}{body}"))
}

pub(crate) fn literal(value: &Value) -> String {
    let typed = |name: &str| {
        let Json::Object(ref members) = write_value(value) else {
            return String::new();
        };
        let text = match members.first() {
            Some((_, Json::String(text))) => text.clone(),
            _ => String::new(),
        };
        format!("{name} {}", crate::json::to_string(&Json::String(text)))
    };
    match value {
        Value::Null(_) => "NULL".into(),
        Value::Bool(true) => "TRUE".into(),
        Value::Bool(false) => "FALSE".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) if f.is_finite() => float_literal(*f),
        Value::Float(_) => typed("FLOAT"),
        Value::Decimal(d) => {
            let text = d.to_string();
            if text.contains('.') {
                text
            } else {
                typed("DECIMAL")
            }
        }
        Value::String(s) => crate::json::to_string(&Json::String(s.clone())),
        Value::Date(_) => typed("DATE"),
        Value::NaiveDateTime(_) | Value::AwareDateTime(_) => typed("DATETIME"),
        // `Node::constant` takes scalars only (`ast.md`).
        Value::List(_) | Value::Map(_) | Value::Opaque(_) => {
            unreachable!("a constant is a scalar")
        }
    }
}

/// A string: between single quotes in the style of tables when it holds no
/// control character, else between double quotes with JSON escapes.
pub(crate) fn string_text(text: &str, single: bool) -> String {
    if single && !text.chars().any(|c| c < ' ') {
        format!("'{}'", text.replace('\'', "''"))
    } else {
        crate::json::to_string(&Json::String(text.to_owned()))
    }
}

/// A constant, with strings in the given style.
pub(crate) fn styled(value: &Value, single: bool) -> String {
    match value {
        Value::String(text) => string_text(text, single),
        _ => literal(value),
    }
}

fn child(node: &Node, min: u8, single: bool, out: &mut String) {
    if level(node) < min {
        out.push('(');
        write_node(node, single, out);
        out.push(')');
    } else {
        write_node(node, single, out);
    }
}

fn is_number_constant(node: &Node) -> bool {
    matches!(
        node.expr(),
        Expr::Const(Value::Int(_) | Value::Float(_) | Value::Decimal(_))
    )
}

// One arm per node reads best as a single dispatch.
#[allow(clippy::too_many_lines)]
fn write_node(node: &Node, single: bool, out: &mut String) {
    match node.expr() {
        Expr::Field(segments) => out.push_str(&path(segments)),
        Expr::Const(value) => out.push_str(&styled(value, single)),
        Expr::Compare { op, left, right } => {
            child(left, 5, single, out);
            let symbol = match op {
                CompareOp::Eq => " = ",
                CompareOp::Ne => " != ",
                CompareOp::Lt => " < ",
                CompareOp::Le => " <= ",
                CompareOp::Gt => " > ",
                CompareOp::Ge => " >= ",
            };
            out.push_str(symbol);
            child(right, 5, single, out);
        }
        Expr::And(items) | Expr::Or(items) => {
            let (separator, min) = if matches!(node.expr(), Expr::And(_)) {
                (" AND ", 3)
            } else {
                (" OR ", 2)
            };
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(separator);
                }
                child(item, min, single, out);
            }
        }
        Expr::Not(operand) => {
            out.push_str("NOT ");
            child(operand, 3, single, out);
        }
        Expr::In { operand, values } => {
            child(operand, 5, single, out);
            out.push_str(" IN (");
            let items: Vec<String> = values.iter().map(|v| styled(v, single)).collect();
            out.push_str(&items.join(", "));
            out.push(')');
        }
        Expr::Between {
            operand,
            low,
            high,
            closed,
        } => {
            child(operand, 5, single, out);
            out.push_str(" BETWEEN ");
            child(low, 5, single, out);
            out.push_str(" AND ");
            child(high, 5, single, out);
            if *closed != Closed::Both {
                out.push_str(" CLOSED ");
                out.push_str(&closed.as_str().to_ascii_uppercase());
            }
        }
        Expr::IsNull(operand) => {
            child(operand, 5, single, out);
            out.push_str(" IS NULL");
        }
        Expr::Matches { operand, pattern } => {
            child(operand, 5, single, out);
            out.push_str(" MATCHES ");
            out.push_str(&string_text(pattern.source(), single));
        }
        Expr::StringTest { op, operand, text } => {
            child(operand, 5, single, out);
            out.push_str(match op {
                StringOp::StartsWith => " STARTS WITH ",
                StringOp::EndsWith => " ENDS WITH ",
                StringOp::Contains => " CONTAINS ",
            });
            out.push_str(&string_text(text, single));
        }
        Expr::Arith { op, left, right } => {
            let (symbol, own) = match op {
                ArithOp::Add => (" + ", 5),
                ArithOp::Sub => (" - ", 5),
                ArithOp::Mul => (" * ", 6),
                ArithOp::Div => (" / ", 6),
            };
            child(left, own, single, out);
            out.push_str(symbol);
            child(right, own + 1, single, out);
        }
        Expr::Abs(operand) => {
            out.push_str("ABS(");
            child(operand, 5, single, out);
            out.push(')');
        }
        Expr::Neg(operand) => {
            out.push('-');
            if is_number_constant(operand) {
                // `-5` would read back as the constant -5.
                out.push('(');
                write_node(operand, single, out);
                out.push(')');
            } else {
                child(operand, 7, single, out);
            }
        }
    }
}

/// A condition's canonical text (`spec/text.md`, *Printing*). A field key with
/// a control character, which [`to_string`] refuses, is written with JSON
/// escapes (`` `k\u000ay` ``), so the text stays on one line but does not read
/// back.
impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        write_node(self, false, &mut out);
        f.write_str(&out)
    }
}

/// An expression in the style of tables: strings between single quotes
/// (`spec/tables.md`, *Canonical text*).
pub(crate) fn table_text(node: &Node) -> String {
    let mut out = String::new();
    write_node(node, true, &mut out);
    out
}

/// An input cell's canonical text, without its column `subject`, or `None` when
/// the node has no cell form (`spec/tables.md`, *Canonical text*).
pub(crate) fn cell_text(cell: &Node, subject: &Node) -> Option<String> {
    let (negated, inner) = match cell.expr() {
        Expr::Not(operand) => (true, &**operand),
        _ => (false, cell),
    };
    let not = if negated { "NOT " } else { "" };
    let mut out = String::new();
    match inner.expr() {
        Expr::Compare { op, left, right } if !negated && **left == *subject => {
            if *op != CompareOp::Eq {
                out.push_str(match op {
                    CompareOp::Ne => "!= ",
                    CompareOp::Lt => "< ",
                    CompareOp::Le => "<= ",
                    CompareOp::Gt => "> ",
                    CompareOp::Ge => ">= ",
                    CompareOp::Eq => "",
                });
            }
            child(right, 5, true, &mut out);
        }
        Expr::In { operand, values } if **operand == *subject => {
            let items: Vec<String> = values.iter().map(|v| styled(v, true)).collect();
            let _ = write!(out, "{not}IN ({})", items.join(", "));
        }
        Expr::Between {
            operand,
            low,
            high,
            closed,
        } if **operand == *subject => {
            let _ = write!(out, "{not}BETWEEN ");
            child(low, 5, true, &mut out);
            out.push_str(" AND ");
            child(high, 5, true, &mut out);
            if *closed != Closed::Both {
                out.push_str(" CLOSED ");
                out.push_str(&closed.as_str().to_ascii_uppercase());
            }
        }
        Expr::IsNull(operand) if **operand == *subject => {
            out.push_str(if negated { "IS NOT NULL" } else { "IS NULL" });
        }
        Expr::Matches { operand, pattern } if **operand == *subject => {
            let _ = write!(out, "{not}MATCHES {}", string_text(pattern.source(), true));
        }
        Expr::StringTest { op, operand, text } if **operand == *subject => {
            let word = match op {
                StringOp::StartsWith => "STARTS WITH",
                StringOp::EndsWith => "ENDS WITH",
                StringOp::Contains => "CONTAINS",
            };
            let _ = write!(out, "{not}{word} {}", string_text(text, true));
        }
        _ => return None,
    }
    Some(out)
}

fn rule_text(rule: &Rule) -> Result<String> {
    if !rule.get_metadata().is_empty() {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "rule {:?} has metadata, which text cannot hold",
                rule.name()
            ),
        )
        .with_hint("write rules with metadata as JSON or YAML")
        .unsupported("text", "metadata cannot be written as text"));
    }
    check_keys(rule.when())?;
    for (_, value) in rule.then() {
        check_keys(value)?;
    }
    let mut out = format!("RULE {}", key(rule.name(), '"'));
    if rule.get_priority() != 0 {
        let _ = write!(out, " PRIORITY {}", rule.get_priority());
    }
    if !rule.get_tags().is_empty() {
        let tags: Vec<String> = rule.get_tags().iter().map(|t| key(t, '"')).collect();
        let _ = write!(out, " TAGS {}", tags.join(", "));
    }
    if let Some(description) = rule.get_description() {
        out.push_str(" DESCRIPTION ");
        out.push_str(&crate::json::to_string(&Json::String(description.into())));
    }
    let _ = write!(out, "\nIF {}", rule.when());
    if !rule.then().is_empty() {
        let items: Vec<String> = rule
            .then()
            .iter()
            .map(|(name, value)| format!("{} = {value}", key(name, '"')))
            .collect();
        let _ = write!(out, "\nTHEN {}", items.join(", "));
    }
    Ok(out)
}

/// A field key with a control character, which a name between backticks
/// cannot hold.
pub(crate) fn check_keys(node: &Node) -> Result<()> {
    if let Expr::Field(segments) = node.expr() {
        for segment in segments {
            if let Segment::Key(key) = segment
                && key.chars().any(|c| c < ' ')
            {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    format!(
                        "the field key {key:?} has a control character, which text cannot hold"
                    ),
                )
                .with_hint("write it as JSON or YAML")
                .unsupported("text", "names cannot hold control characters"));
            }
        }
    }
    node.children()
        .iter()
        .try_for_each(|child| check_keys(child))
}

/// A document's canonical text (`spec/text.md`, *Printing*), without a final
/// line break.
///
/// # Errors
///
/// `unsupported` for a rule with metadata, a field key with a control
/// character, which text cannot hold, or a decision table.
///
/// # Examples
///
/// ```
/// use cleanrules::format::{Document, read_document};
/// use cleanrules::json::Limits;
///
/// let json = r#"{"version": 1, "condition": {"op": "gt", "left": {"op": "field", "path": ["order", "date"]}, "right": {"op": "const", "value": {"$date": "2026-01-01"}}}}"#;
/// let document = read_document(json, &Limits::default())?;
/// assert_eq!(cleanrules::text::to_string(&document)?, r#"order.date > DATE "2026-01-01""#);
/// # Ok::<(), cleanrules::Error>(())
/// ```
pub fn to_string(document: &Document) -> Result<String> {
    match document {
        Document::DecisionTable(_) => Err(Error::new(
            ErrorCode::Unsupported,
            "a decision table has no text form",
        )
        .with_hint("write it as CSV, JSON or YAML, or write its rule set as text")
        .unsupported("text", "decision tables are written as CSV, JSON or YAML")),
        Document::Condition(node) => {
            check_keys(node)?;
            Ok(node.to_string())
        }
        Document::Rule(rule) => rule_text(rule),
        Document::RuleSet(ruleset) => {
            let mut out = format!("HIT POLICY {}", ruleset.hit_policy().as_str());
            if let Some(aggregate) = ruleset.aggregate() {
                let _ = write!(out, "\nAGGREGATE {}", aggregate.as_str());
            }
            if ruleset.logic() != LogicName::Boolean {
                let _ = write!(out, "\nLOGIC {}", ruleset.logic().as_str());
            }
            if ruleset.strict() {
                out.push_str("\nSTRICT");
            }
            for rule in ruleset.rules() {
                out.push_str("\n\n");
                out.push_str(&rule_text(rule)?);
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{parse, parse_rule, to_string};
    use crate::error::ErrorCode;
    use crate::format::{Document, read_document};
    use crate::json::Limits;

    fn error(text: &str) -> crate::Error {
        parse(text, &Limits::default()).unwrap_err()
    }

    #[test]
    fn errors_say_what_was_expected() {
        let e = error("x > 1 AND");
        assert_eq!(e.code(), ErrorCode::RuleSyntaxError);
        assert_eq!(
            e.message(),
            "expected a field, a constant or a parenthesis, found the end of the text"
        );
        assert!(error("0 < x < 10").hint().unwrap().contains("BETWEEN"));
        assert!(error("a.`b").message().contains("not closed"));
        assert!(error("a.5 > 1").hint().unwrap().contains("backticks"));
        assert!(error("x > 1e999").hint().unwrap().contains("FLOAT \"inf\""));
        assert!(error("x IN (y)").message().contains("constants"));
        assert_eq!(error("x = \"a").message(), "a string is not closed");
        assert_eq!(error("x = 'a").message(), "a quoted text is not closed");
        assert_eq!(error("x ; y").message(), "unexpected character ';'");
        assert!(
            error("HIT POLICY x")
                .message()
                .contains("unknown hit policy")
        );
        assert!(
            error("AGGREGATE x\nIF a")
                .message()
                .contains("unknown aggregate")
        );
        assert!(
            error("RULE r PRIORITY x IF a")
                .message()
                .contains("integer priority")
        );
        assert!(
            error("RULE r PRIORITY 99999999999999999999 IF a")
                .message()
                .contains("64-bit")
        );
        assert!(
            error("x BETWEEN 1 AND 2 CLOSED sometimes")
                .message()
                .contains("BOTH")
        );
        assert!(error("x IS 5").message().contains("NULL"));
        assert!(
            error("RULE r IF a b")
                .message()
                .contains("RULE, IF or the end")
        );
        assert!(error("RULE 5 IF a").message().contains("rule name"));
        assert!(error("a[x] > 1").message().contains("an index"));
        assert!(error("x MATCHES 5").message().contains("a string"));
        assert!(
            error("RULE r DESCRIPTION x IF a")
                .message()
                .contains("a string")
        );
        assert!(error("IF a THEN 5 = 1").message().contains("payload key"));
        assert!(error("IF a THEN x 1").message().contains("\"=\""));
        assert!(error("HIT first").message().contains("POLICY"));
        assert!(error("HIT POLICY 5").message().contains("hit policy"));
        assert!(error("x = NOT y").message().contains("found NOT"));
        assert!(error("ABS x").message().contains("\"(\""));
        assert!(error("(x > 1").message().contains("\")\""));
        assert!(error("x STARTS y").message().contains("WITH"));
        assert!(error("x NOT y").message().contains("AND, OR or the end"));
        assert!(
            error("a > 1 IS NULL")
                .message()
                .contains("AND, OR or the end")
        );
        assert!(
            error("x = 99999999999999999999")
                .hint()
                .unwrap()
                .contains("decimal")
        );
    }

    #[test]
    fn lone_carriage_returns_end_lines() {
        let e = error("a\rAND\r\n>");
        assert_eq!((e.context().line, e.context().column), (Some(3), Some(1)));
    }

    #[test]
    fn deep_texts_stop_before_the_stack_does() {
        // At the default limits the worst texts need under 640 KiB of stack in
        // release builds; debug builds have frames several times larger.
        let worst = std::thread::Builder::new()
            .stack_size(8 << 20)
            .spawn(|| {
                let n = 100_000;
                [
                    format!("{}x{}", "(".repeat(n), ")".repeat(n)),
                    format!("{}x", "NOT ".repeat(n)),
                    format!("{}x{}", "NOT (".repeat(n), ")".repeat(n)),
                    format!("{}x > 0", "- ".repeat(n)),
                    format!("{}x{} > 0", "ABS(".repeat(n), ")".repeat(n)),
                ]
                .map(|text| error(&text).code())
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(worst, [ErrorCode::LimitExceeded; 5]);
        // A text at the default depth limit reads.
        let text = format!("{}x", "NOT ".repeat(255));
        assert!(parse(&text, &Limits::default()).is_ok());
    }

    #[test]
    fn long_chains_stop_at_the_depth_limit() {
        // A left-deep chain is read without recursion: the depth check stops
        // it, so it needs no more stack than any other text.
        let chains = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(|| {
                let terms = "+x".repeat(1_000_000);
                [
                    format!("x{terms} > 0"),
                    format!("IF a THEN y = x{terms}"),
                    format!("x{} > 0", "*x".repeat(1_000_000)),
                ]
                .map(|text| error(&text).code())
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(chains, [ErrorCode::LimitExceeded; 3]);
        let e = error(&format!("x{} > 0", "+x".repeat(300)));
        assert_eq!((e.context().line, e.context().column), (Some(1), Some(1)));
    }

    #[test]
    fn huge_limits_do_not_overflow() {
        let limits = Limits {
            max_depth: usize::MAX,
            ..Limits::default()
        };
        assert!(parse("NOT NOT NOT a", &limits).is_ok());
    }

    #[test]
    fn one_rule() {
        let rule = parse_rule("IF a > 1 THEN x = 1", &Limits::default()).unwrap();
        assert_eq!(rule.name(), "rule_1");
        let condition = parse_rule("a > 1", &Limits::default()).unwrap_err();
        assert_eq!(condition.code(), ErrorCode::RuleSyntaxError);
        assert_eq!(
            (condition.context().line, condition.context().column),
            (Some(1), Some(1))
        );
        assert!(condition.hint().unwrap().contains("condition"));
        let two = parse_rule("IF a\nIF b", &Limits::default()).unwrap_err();
        assert_eq!(
            (two.context().line, two.context().column),
            (Some(2), Some(1))
        );
        assert!(two.hint().unwrap().contains("rule set"));
        let header = parse_rule("HIT POLICY first\nIF a", &Limits::default()).unwrap_err();
        assert!(header.hint().unwrap().contains("header"));
        let size = Limits {
            max_text_size: 3,
            ..Limits::default()
        };
        assert_eq!(
            parse_rule("IF a", &size).unwrap_err().code(),
            ErrorCode::LimitExceeded
        );
    }

    #[test]
    fn error_messages_use_the_words_of_the_text() {
        assert!(error("x = FLOAT \"nan\"").message().contains("FLOAT takes"));
        let decimal = error("x = 1.0000000000000000000000000000000000000001");
        assert!(decimal.hint().unwrap().contains("38"));
        assert!(
            error("STRICT HIT POLICY first IF a")
                .hint()
                .unwrap()
                .contains("order")
        );
        let limits = Limits {
            max_depth: 10,
            ..Limits::default()
        };
        let text = format!("{}x{}", "(".repeat(30), ")".repeat(30));
        let nesting = parse(&text, &limits).unwrap_err();
        assert!(nesting.message().contains("22 deep"));
    }

    #[test]
    fn keys_with_control_characters_have_no_text() {
        let json = r#"{"version": 1, "rule": {"name": "r", "when": {"op": "field", "path": ["a\u0001"]}}}"#;
        let document = read_document(json, &Limits::default()).unwrap();
        assert_eq!(
            to_string(&document).unwrap_err().code(),
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn rules_with_metadata_have_no_text() {
        let json = r#"{"version": 1, "rule": {"name": "r", "when": {"op": "field", "path": ["a"]}, "metadata": {"k": 1}}}"#;
        let document = read_document(json, &Limits::default()).unwrap();
        let e = to_string(&document).unwrap_err();
        assert_eq!(e.code(), ErrorCode::Unsupported);
        assert!(e.hint().unwrap().contains("JSON or YAML"));
    }

    #[test]
    fn a_rule_set_reads_back_from_its_text() {
        let text = "HIT POLICY collect\nAGGREGATE max\nSTRICT\n\nRULE a\nIF x > 1\nTHEN n = 1";
        let document = parse(text, &Limits::default()).unwrap();
        assert!(matches!(document, Document::RuleSet(_)));
        assert_eq!(to_string(&document).unwrap(), text);
    }
}
