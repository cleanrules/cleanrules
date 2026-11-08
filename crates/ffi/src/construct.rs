//! Nodes built in code (`spec/ast.md`, *Construction rules*), through the
//! core's constructors, which apply every rule; and what a node says about
//! itself: its tag, kind, children, own data and structural identity. Bindings
//! build conditions with these, as Python and JavaScript do with the core's
//! constructors, so that every language gets the same checks and errors.

use std::ffi::{CStr, c_char};
use std::hash::{Hash, Hasher};

use cleanrules::ast::{ArithOp, Closed, CompareOp, Expr, Kind, Node, StringOp};
use cleanrules::{Error, ErrorCode, NullCause, Segment, Value};

use crate::boundary::{Failure, Outcome, bytes, call, clear, give, handle, utf8_arg};
use crate::cr_status;
use crate::error::cr_error;
use crate::node::cr_node;
use crate::ruleset::paths_value;
use crate::value::{borrow_value, cr_value, give_new_value};

fn expression(message: impl Into<String>, hint: &str) -> Failure {
    Failure::Core(Error::new(ErrorCode::ExpressionError, message).with_hint(hint))
}

/// What keeps a value from being a constant, before the core's own checks
/// (lists, maps): a null that stands for a native value out of range or
/// unreadable, or an opaque value (`spec/data-model.md`, *Constants*).
pub(crate) fn not_constant(value: &Value) -> Option<Failure> {
    match value {
        Value::Null(NullCause::IntOverflow) => Some(expression(
            "the integer constant does not fit in 64 bits",
            "use a decimal for larger exact numbers",
        )),
        Value::Null(NullCause::DecimalOverflow) => Some(expression(
            "the decimal constant does not fit in 38 digits",
            "decimals hold up to 38 significant digits and are never rounded",
        )),
        Value::Null(NullCause::ReadError(detail)) => Some(expression(
            format!("the constant could not be read: {detail}"),
            "constants are null, booleans, numbers, strings, dates and datetimes",
        )),
        Value::Opaque(opaque) => Some(expression(
            format!("a {} cannot be a constant", opaque.type_name),
            "constants are null, booleans, numbers, strings, dates and datetimes",
        )),
        _ => None,
    }
}

/// A name argument (an operator, `closed`): UTF-8 bytes.
///
/// # Safety
///
/// `name` is `NULL` or points to `len` readable bytes.
unsafe fn name<'a>(name: *const c_char, len: usize, what: &str) -> Outcome<&'a str> {
    // SAFETY: the caller's promise is `bytes`'.
    utf8_arg(unsafe { bytes(name, len, what) }?, what)
}

fn unknown(what: &str, name: &str, names: &'static str) -> Failure {
    Failure::Invalid(format!("unknown {what} {name:?}"), names)
}

/// A node argument, cloned for the core's constructor.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`.
unsafe fn operand(node: *const cr_node, what: &str) -> Outcome<Node> {
    // SAFETY: the caller's promise is `handle`'s.
    Ok(unsafe { handle(node, what) }?.node.clone())
}

/// Run a constructor and hand its node out.
///
/// # Safety
///
/// `out` is `NULL` or points to a writable `cr_node *`; `error` is `NULL` or
/// points to a writable `cr_error *` set to `NULL`.
pub(crate) unsafe fn build(
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
    make: impl FnOnce() -> Outcome<Node>,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        let node = make()?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_node { node }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A field (`spec/ast.md`, `field`): `path` is a list of strings (keys, never
/// split at dots) and ints from 0 (indexes), as `cr_node_field_paths` gives
/// them; it stays the caller's. An empty path, an empty key, or a segment of
/// another kind (a negative int, a boolean...) is a `CR_EXPRESSION_ERROR`; a
/// `path` that is not a list is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `path` is `NULL` or a live `cr_value`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_field(
    path: *const cr_value,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `path` is NULL or live (# Safety).
        let Value::List(items) = (unsafe { borrow_value(path, "path") })? else {
            return Err(Failure::Invalid(
                "path is not a list".into(),
                "pass a list of strings (keys) and ints (indexes)",
            ));
        };
        let mut segments = Vec::with_capacity(items.len());
        for item in items {
            segments.push(match item {
                Value::String(key) => Segment::Key(key.clone()),
                Value::Int(index) if *index >= 0 => {
                    Segment::Index(u64::try_from(*index).unwrap_or_default())
                }
                Value::Int(index) => {
                    return Err(expression(
                        format!("a field path segment cannot be the negative index {index}"),
                        "indexes count from 0",
                    ));
                }
                other => {
                    return Err(expression(
                        format!(
                            "a field path segment cannot be {}",
                            crate::value::type_name(other)
                        ),
                        "use non-empty keys (strings) and indexes (ints from 0)",
                    ));
                }
            });
        }
        Ok(Node::field(segments)?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// A constant (`spec/ast.md`, `const`): a copy of `value`, which stays the
/// caller's. A list, a map, an opaque value, or a null with the reason
/// `CR_REASON_INT_OVERFLOW`, `CR_REASON_DECIMAL_OVERFLOW` or
/// `CR_REASON_READ_ERROR` is a `CR_EXPRESSION_ERROR` (`spec/data-model.md`,
/// *Constants*). A value cannot hold a NaN or a datetime finer than a
/// microsecond, which a binding rejects before it builds the value.
///
/// # Safety
///
/// `value` is `NULL` or a live `cr_value`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_constant(
    value: *const cr_value,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { borrow_value(value, "value") }?;
        if let Some(problem) = not_constant(value) {
            return Err(problem);
        }
        Ok(Node::constant(value.clone())?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// A comparison `left op right`, with `op` one of `eq`, `ne`, `lt`, `le`,
/// `gt`, `ge` (`CR_INVALID_ARGUMENT` otherwise). The operands are copied and
/// stay the caller's, as in every constructor.
///
/// # Safety
///
/// `op` is `NULL` or points to `op_len` readable bytes; `left` and `right` are
/// `NULL` or live `cr_node`s; `out` is `NULL` or points to a writable
/// `cr_node *`; `error` is `NULL` or points to a writable `cr_error *` set to
/// `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_compare(
    op: *const c_char,
    op_len: usize,
    left: *const cr_node,
    right: *const cr_node,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `op` and `op_len` (# Safety).
        let op = unsafe { name(op, op_len, "op") }?;
        let op = CompareOp::from_tag(op)
            .ok_or_else(|| unknown("comparison", op, "use eq, ne, lt, le, gt or ge"))?;
        // SAFETY: `left` and `right` are NULL or live (# Safety).
        let (left, right) = unsafe { (operand(left, "left")?, operand(right, "right")?) };
        Ok(Node::compare(op, left, right)?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// The operands of `and` and `or`, copied.
///
/// # Safety
///
/// `operands` is `NULL` or points to `len` readable pointers, each `NULL` or
/// a live `cr_node`.
unsafe fn operands(operands: *const *const cr_node, len: usize) -> Outcome<Vec<Node>> {
    if operands.is_null() {
        return if len == 0 {
            Ok(Vec::new())
        } else {
            Err(Failure::Invalid(
                format!("operands is NULL with a length of {len}"),
                "pass an array of nodes, or a length of 0",
            ))
        };
    }
    // SAFETY: the caller promises `len` readable pointers at `operands`.
    let pointers = unsafe { std::slice::from_raw_parts(operands, len) };
    pointers
        .iter()
        // SAFETY: each pointer is NULL or a live node (# Safety).
        .map(|&node| unsafe { operand(node, "an operand") })
        .collect()
}

/// The conjunction of `len` conditions, at least two after flattening: an
/// operand that is an `and` is spliced in (`spec/ast.md`, `and`, `or`).
///
/// # Safety
///
/// `operands` is `NULL` or points to `len` readable pointers, each `NULL` or a
/// live `cr_node`; `out` is `NULL` or points to a writable `cr_node *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_and(
    operands: *const *const cr_node,
    len: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: `operands` and `len` (# Safety).
    let make = || Ok(Node::and(unsafe { self::operands(operands, len) }?)?);
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// The disjunction of `len` conditions, at least two after flattening.
///
/// # Safety
///
/// As `cr_node_new_and`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_or(
    operands: *const *const cr_node,
    len: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: `operands` and `len` (# Safety).
    let make = || Ok(Node::or(unsafe { self::operands(operands, len) }?)?);
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// A node of one operand: `not`, `is_null`, `abs` or `neg`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `out` and `error` as `build`.
unsafe fn unary(
    node: *const cr_node,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
    constructor: fn(Node) -> cleanrules::Result<Node>,
) -> cr_status {
    // SAFETY: `node` is NULL or live (# Safety).
    let make = || Ok(constructor(unsafe { operand(node, "operand") }?)?);
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// The negation of a condition.
///
/// # Safety
///
/// `operand` is `NULL` or a live `cr_node`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_not(
    operand: *const cr_node,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `unary`'s.
    unsafe { unary(operand, out, error, Node::not) }
}

/// Whether a value is null: never unknown, and the only place for the null
/// constant.
///
/// # Safety
///
/// As `cr_node_new_not`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_is_null(
    operand: *const cr_node,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `unary`'s.
    unsafe { unary(operand, out, error, Node::is_null) }
}

/// The absolute value of a number.
///
/// # Safety
///
/// As `cr_node_new_not`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_abs(
    operand: *const cr_node,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `unary`'s.
    unsafe { unary(operand, out, error, Node::abs) }
}

/// The negation of a number.
///
/// # Safety
///
/// As `cr_node_new_not`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_neg(
    operand: *const cr_node,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `unary`'s.
    unsafe { unary(operand, out, error, Node::neg) }
}

/// Membership in constants (`spec/ast.md`, `in`): `values` is a list, copied,
/// whose items are checked as `cr_node_new_constant` checks one, then by the core
/// (non-empty, no null, one type group); they are kept sorted and without
/// repeats. A `values` that is not a list is `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `operand` is `NULL` or a live `cr_node`; `values` is `NULL` or a live
/// `cr_value`; `out` is `NULL` or points to a writable `cr_node *`; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_is_in(
    operand: *const cr_node,
    values: *const cr_value,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `operand` is NULL or live (# Safety).
        let operand = unsafe { self::operand(operand, "operand") }?;
        // SAFETY: `values` is NULL or live (# Safety).
        let Value::List(items) = (unsafe { borrow_value(values, "values") })? else {
            return Err(Failure::Invalid(
                "values is not a list".into(),
                "pass a list of constants",
            ));
        };
        if let Some(problem) = items.iter().find_map(not_constant) {
            return Err(problem);
        }
        Ok(Node::is_in(operand, items.clone())?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// A range test `low <= operand <= high`; `closed` names the ends included:
/// `both` (also for `NULL`), `left`, `right` or `none` (`CR_INVALID_ARGUMENT`
/// otherwise).
///
/// # Safety
///
/// `operand`, `low` and `high` are `NULL` or live `cr_node`s; `closed` is
/// `NULL` or points to `closed_len` readable bytes; `out` is `NULL` or points
/// to a writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_between(
    operand: *const cr_node,
    low: *const cr_node,
    high: *const cr_node,
    closed: *const c_char,
    closed_len: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        let closed = if closed.is_null() {
            Closed::Both
        } else {
            // SAFETY: `closed` and `closed_len` (# Safety).
            let name = unsafe { name(closed, closed_len, "closed") }?;
            Closed::from_name(name)
                .ok_or_else(|| unknown("closed", name, "use both, left, right or none"))?
        };
        // SAFETY: the three nodes are NULL or live (# Safety).
        let (operand, low, high) = unsafe {
            (
                self::operand(operand, "operand")?,
                self::operand(low, "low")?,
                self::operand(high, "high")?,
            )
        };
        Ok(Node::between(operand, low, high, closed)?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// A regular expression search, in the syntax of Rust's `regex` crate: no
/// lookaround, no backreferences (a pattern that does not compile is a
/// `CR_EXPRESSION_ERROR`).
///
/// # Safety
///
/// `operand` is `NULL` or a live `cr_node`; `pattern` is `NULL` or points to
/// `len` readable bytes; `out` is `NULL` or points to a writable `cr_node *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_matches(
    operand: *const cr_node,
    pattern: *const c_char,
    len: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `pattern` and `len` (# Safety).
        let pattern = unsafe { name(pattern, len, "pattern") }?;
        // SAFETY: `operand` is NULL or live (# Safety).
        Ok(Node::matches(
            unsafe { self::operand(operand, "operand") }?,
            pattern,
        )?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// A literal string test, `op` one of `starts_with`, `ends_with` and
/// `contains` (`CR_INVALID_ARGUMENT` otherwise).
///
/// # Safety
///
/// `op` is `NULL` or points to `op_len` readable bytes; `operand` is `NULL` or
/// a live `cr_node`; `text` is `NULL` or points to `len` readable bytes; `out`
/// is `NULL` or points to a writable `cr_node *`; `error` is `NULL` or points
/// to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_string_test(
    op: *const c_char,
    op_len: usize,
    operand: *const cr_node,
    text: *const c_char,
    len: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `op` and `op_len` (# Safety).
        let op = unsafe { name(op, op_len, "op") }?;
        let op = StringOp::from_tag(op)
            .ok_or_else(|| unknown("string test", op, "use starts_with, ends_with or contains"))?;
        // SAFETY: `text` and `len` (# Safety).
        let text = unsafe { name(text, len, "text") }?;
        // SAFETY: `operand` is NULL or live (# Safety).
        let operand = unsafe { self::operand(operand, "operand") }?;
        Ok(Node::string_test(op, operand, text)?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// Arithmetic `left op right`, `op` one of `add`, `sub`, `mul` and `div`
/// (`CR_INVALID_ARGUMENT` otherwise).
///
/// # Safety
///
/// As `cr_node_new_compare`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_new_arith(
    op: *const c_char,
    op_len: usize,
    left: *const cr_node,
    right: *const cr_node,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `op` and `op_len` (# Safety).
        let op = unsafe { name(op, op_len, "op") }?;
        let op = ArithOp::from_tag(op)
            .ok_or_else(|| unknown("operator", op, "use add, sub, mul or div"))?;
        // SAFETY: `left` and `right` are NULL or live (# Safety).
        let (left, right) = unsafe { (operand(left, "left")?, operand(right, "right")?) };
        Ok(Node::arith(op, left, right)?)
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

// ---- What a node says about itself.

/// The static C string of a tag: one for every tag of the catalogue.
fn tag_name(tag: &str) -> &'static CStr {
    const TAGS: [&CStr; 24] = [
        c"field",
        c"const",
        c"eq",
        c"ne",
        c"lt",
        c"le",
        c"gt",
        c"ge",
        c"and",
        c"or",
        c"not",
        c"in",
        c"between",
        c"is_null",
        c"matches",
        c"starts_with",
        c"ends_with",
        c"contains",
        c"add",
        c"sub",
        c"mul",
        c"div",
        c"abs",
        c"neg",
    ];
    TAGS.iter()
        .find(|name| name.to_bytes() == tag.as_bytes())
        .copied()
        // Every tag is listed: a test checks each node's.
        .unwrap_or(c"")
}

/// The node's tag (`spec/ast.md`, *Catalogue*), such as `gt` or `and`, a
/// static string; `NULL` for `NULL`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_tag(node: *const cr_node) -> *const c_char {
    // SAFETY: `node` is NULL or live (# Safety).
    unsafe { node.as_ref() }.map_or(std::ptr::null(), |n| tag_name(n.node.tag()).as_ptr())
}

/// Where the node may appear (`spec/ast.md`, *Node kinds*): `condition`,
/// `value` or `any` (a field or a boolean constant), a static string; `NULL`
/// for `NULL`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_kind(node: *const cr_node) -> *const c_char {
    // SAFETY: `node` is NULL or live (# Safety).
    let Some(node) = (unsafe { node.as_ref() }) else {
        return std::ptr::null();
    };
    match node.node.kind() {
        Kind::Condition => c"condition",
        Kind::Value => c"value",
        Kind::Any => c"any",
    }
    .as_ptr()
}

/// The number of the node's direct children; 0 for `NULL`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_children_len(node: *const cr_node) -> usize {
    // SAFETY: `node` is NULL or live (# Safety).
    unsafe { node.as_ref() }.map_or(0, |n| n.node.children().len())
}

/// A copy of the node's child at `index`, in order (a comparison's left then
/// right, `between`'s operand, low and high), freed with `cr_node_free`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_child(
    node: *const cr_node,
    index: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `node` is NULL or live (# Safety).
        let children = unsafe { handle(node, "node") }?.node.children();
        children
            .get(index)
            .map(|&child| child.clone())
            .ok_or_else(|| {
                Failure::Invalid(
                    format!("child {index} is out of the node's {}", children.len()),
                    "pass an index below cr_node_children_len",
                )
            })
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// Whether two nodes are structurally identical (`spec/ast.md`, *Structural
/// identity*): the same shape, tags and constants, compared by type and value
/// (`0.0` and `-0.0`, or the decimals `1.0` and `1.00`, are the same). False
/// when either is `NULL`.
///
/// # Safety
///
/// `left` and `right` are `NULL` or live `cr_node`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_same(left: *const cr_node, right: *const cr_node) -> bool {
    // SAFETY: `left` and `right` are NULL or live (# Safety).
    match unsafe { (left.as_ref(), right.as_ref()) } {
        (Some(left), Some(right)) => left.node == right.node,
        _ => false,
    }
}

/// A hasher with a fixed algorithm and no keys, so that a node's hash is the
/// same on every platform and in every run: 64-bit FNV-1a over the bytes the
/// core's `Hash` implementations write, with every integer written in 8
/// little-endian bytes (`usize` lengths and discriminants included), never in
/// the platform's own width or order.
struct Stable(u64);

impl Stable {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
}

impl Hasher for Stable {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(Self::PRIME);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write_u64(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.write_u64(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.write(&value.to_le_bytes());
    }

    fn write_u128(&mut self, value: u128) {
        self.write(&value.to_le_bytes());
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    fn write_i8(&mut self, value: i8) {
        self.write_i64(i64::from(value));
    }

    fn write_i16(&mut self, value: i16) {
        self.write_i64(i64::from(value));
    }

    fn write_i32(&mut self, value: i32) {
        self.write_i64(i64::from(value));
    }

    #[allow(clippy::cast_sign_loss)] // The bits, as they are.
    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    fn write_i128(&mut self, value: i128) {
        self.write(&value.to_le_bytes());
    }

    fn write_isize(&mut self, value: isize) {
        self.write_i64(value as i64);
    }
}

/// A hash of the node's structure, equal for nodes that `cr_node_same` finds
/// identical (so `0.0` and `-0.0`, or the decimals `1.0` and `1.00`, hash
/// alike). It is the same on every platform and in every run for one version
/// of the library, but may change between versions: never store it. 0 for
/// `NULL`.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_hash(node: *const cr_node) -> u64 {
    // SAFETY: `node` is NULL or live (# Safety).
    unsafe { node.as_ref() }.map_or(0, |n| {
        let mut hasher = Stable(Stable::OFFSET);
        n.node.hash(&mut hasher);
        hasher.finish()
    })
}

/// The node's own data, as a new value freed with `cr_value_free`: a field's
/// path (a list of strings and ints), a constant's value, the values of an
/// `in` (a list, sorted), `between`'s `closed` (`both`, `left`, `right` or
/// `none`), the pattern of `matches` and the text of a string test (strings);
/// a null for every other node.
///
/// # Safety
///
/// `node` is `NULL` or a live `cr_node`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_node_detail(
    node: *const cr_node,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `node` is NULL or live (# Safety).
        let node = &unsafe { handle(node, "node") }?.node;
        let detail = match node.expr() {
            Expr::Field(path) => match paths_value(std::iter::once(&path[..])) {
                Value::List(mut paths) => paths.pop().unwrap_or(Value::List(Vec::new())),
                other => other,
            },
            Expr::Const(value) => value.clone(),
            Expr::In { values, .. } => Value::List(values.clone()),
            Expr::Between { closed, .. } => Value::String(closed.as_str().to_owned()),
            Expr::Matches { pattern, .. } => Value::String(pattern.source().to_owned()),
            Expr::StringTest { text, .. } => Value::String(text.clone()),
            _ => Value::Null(NullCause::Null),
        };
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, detail) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

#[cfg(test)]
mod tests {
    use std::hash::Hasher;

    use super::{Stable, tag_name};

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn the_hash_is_fnv_1a_with_fixed_width_integers() {
        let mut empty = Stable(Stable::OFFSET);
        empty.write(b"");
        assert_eq!(empty.finish(), 0xcbf2_9ce4_8422_2325);
        let mut a = Stable(Stable::OFFSET);
        a.write(b"a");
        // The published FNV-1a 64 of "a".
        assert_eq!(a.finish(), 0xaf63_dc4c_8601_ec8c);
        let (mut narrow, mut wide) = (Stable(Stable::OFFSET), Stable(Stable::OFFSET));
        narrow.write_u8(7);
        wide.write_u64(7);
        assert_eq!(narrow.finish(), wide.finish());
        let (mut size, mut long) = (Stable(Stable::OFFSET), Stable(Stable::OFFSET));
        size.write_usize(9);
        long.write_u64(9);
        assert_eq!(size.finish(), long.finish());
        let (mut small, mut signed) = (Stable(Stable::OFFSET), Stable(Stable::OFFSET));
        small.write_i32(-1);
        signed.write_isize(-1);
        assert_eq!(small.finish(), signed.finish());
        let (mut x, mut y) = (Stable(Stable::OFFSET), Stable(Stable::OFFSET));
        x.write_u16(1);
        y.write_i8(1);
        let (mut p, mut q) = (Stable(Stable::OFFSET), Stable(Stable::OFFSET));
        p.write_u32(1);
        q.write_i16(1);
        assert_eq!(x.finish(), p.finish());
        assert_eq!(y.finish(), q.finish());
        let (mut big, mut bigger) = (Stable(Stable::OFFSET), Stable(Stable::OFFSET));
        big.write_u128(1);
        bigger.write_i128(1);
        assert_eq!(big.finish(), bigger.finish());
    }

    #[test]
    fn every_tag_has_a_static_name() {
        for tag in [
            "field",
            "const",
            "eq",
            "ne",
            "lt",
            "le",
            "gt",
            "ge",
            "and",
            "or",
            "not",
            "in",
            "between",
            "is_null",
            "matches",
            "starts_with",
            "ends_with",
            "contains",
            "add",
            "sub",
            "mul",
            "div",
            "abs",
            "neg",
        ] {
            assert_eq!(tag_name(tag).to_str().ok(), Some(tag));
        }
        assert_eq!(tag_name("nope").to_bytes(), b"");
    }
}
