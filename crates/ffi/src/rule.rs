//! Rules built in code (`spec/rules.md`, *Rules*): a name and a condition,
//! then one step at a time, each a new rule made by the core's constructors,
//! so that every step is checked as Python and JavaScript check it. And what
//! a rule holds, for the rules read from documents and rule sets, and one
//! rule read from text (`rule_from_text`).

use std::ffi::c_char;

use cleanrules::json::utf8_text;
use cleanrules::rules::Rule;
use cleanrules::{Error, ErrorCode, Map, Value};

use crate::boundary::{
    Failure, Outcome, bytes, call, clear, clear_borrowed, free, give, handle, limits_of, utf8_arg,
};
use crate::construct::build;
use crate::cr_status;
use crate::error::cr_error;
use crate::limits::cr_limits;
use crate::node::cr_node;
use crate::value::{borrow_text, borrow_value, cr_value, give_new_value};

/// A rule: a name, a condition, a payload and descriptive data. It never
/// changes: each `cr_rule_with_*` gives a new rule.
pub struct cr_rule {
    pub(crate) rule: Rule,
}

fn definition(rule: &str, message: impl Into<String>, hint: &str) -> Failure {
    let mut error = Error::new(ErrorCode::RuleDefinitionError, message).with_hint(hint);
    error.context_mut().rule = Some(rule.to_owned());
    Failure::Core(error)
}

/// A text argument, UTF-8.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes.
unsafe fn text<'a>(text: *const c_char, len: usize, what: &str) -> Outcome<&'a str> {
    // SAFETY: the caller's promise is `bytes`'.
    utf8_arg(unsafe { bytes(text, len, what) }?, what)
}

/// Run a step on `rule` and hand the new rule out.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `out` is `NULL` or points to a
/// writable `cr_rule *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
unsafe fn step(
    rule: *const cr_rule,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
    make: impl FnOnce(&Rule) -> Outcome<Rule>,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `rule` is NULL or live (# Safety).
        let rule = make(&unsafe { handle(rule, "rule") }?.rule)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_rule { rule }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// A rule with a name and a condition, and nothing else: priority 0, no
/// payload, tags, description, metadata or action. The condition is copied.
/// An empty name, or a value as the condition, is a
/// `CR_RULE_DEFINITION_ERROR`; the null constant, a `CR_EXPRESSION_ERROR`.
///
/// # Safety
///
/// `name` is `NULL` or points to `name_len` readable bytes; `when` is `NULL`
/// or a live `cr_node`; `out` is `NULL` or points to a writable `cr_rule *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_new(
    name: *const c_char,
    name_len: usize,
    when: *const cr_node,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `name` and `name_len` (# Safety).
        let name = unsafe { text(name, name_len, "name") }?;
        // SAFETY: `when` is NULL or live (# Safety).
        let when = unsafe { handle(when, "when") }?.node.clone();
        let rule = Rule::new(name, when)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_rule { rule }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The rule with `key` in its payload, given `value` (copied): a new key goes
/// last, and a key it has keeps its place and takes the new value. A
/// condition as the value is a `CR_RULE_DEFINITION_ERROR`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `key` is `NULL` or points to
/// `key_len` readable bytes; `value` is `NULL` or a live `cr_node`; `out` is
/// `NULL` or points to a writable `cr_rule *`; `error` is `NULL` or points to
/// a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_with_payload(
    rule: *const cr_rule,
    key: *const c_char,
    key_len: usize,
    value: *const cr_node,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = |rule: &Rule| {
        // SAFETY: `key` and `key_len` (# Safety).
        let key = unsafe { text(key, key_len, "key") }?;
        // SAFETY: `value` is NULL or live (# Safety).
        let value = unsafe { handle(value, "value") }?.node.clone();
        Ok(rule.clone().with_payload_entry(key, value)?)
    };
    // SAFETY: the caller's promises are `step`'s.
    unsafe { step(rule, out, error, make) }
}

/// The rule with a priority, for the `priority` hit policy.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `out` is `NULL` or points to a
/// writable `cr_rule *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_with_priority(
    rule: *const cr_rule,
    priority: i64,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = |rule: &Rule| Ok(rule.clone().priority(priority));
    // SAFETY: the caller's promises are `step`'s.
    unsafe { step(rule, out, error, make) }
}

/// The rule with these tags instead of its own: `tags` is a list of strings,
/// which the rule keeps sorted and without repeats (`cr_rule_tags`). An item
/// that is not a string is a `CR_RULE_DEFINITION_ERROR`; a `tags` that is not
/// a list, `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `tags` is `NULL` or a live
/// `cr_value`; `out` is `NULL` or points to a writable `cr_rule *`; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_with_tags(
    rule: *const cr_rule,
    tags: *const cr_value,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = |rule: &Rule| {
        // SAFETY: `tags` is NULL or live (# Safety).
        let Value::List(items) = (unsafe { borrow_value(tags, "tags") })? else {
            return Err(Failure::Invalid(
                "tags is not a list".into(),
                "pass a list of strings",
            ));
        };
        let mut names = Vec::with_capacity(items.len());
        for item in items {
            let Value::String(tag) = item else {
                return Err(definition(
                    rule.name(),
                    format!("a tag is a string, not {}", crate::value::type_name(item)),
                    "tags are names, such as \"pricing\"",
                ));
            };
            names.push(tag.clone());
        }
        Ok(rule.clone().tags(names))
    };
    // SAFETY: the caller's promises are `step`'s.
    unsafe { step(rule, out, error, make) }
}

/// The rule with a description.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `text` is `NULL` or points to `len`
/// readable bytes; `out` is `NULL` or points to a writable `cr_rule *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_with_description(
    rule: *const cr_rule,
    text: *const c_char,
    len: usize,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = |rule: &Rule| {
        // SAFETY: `text` and `len` (# Safety).
        let description = unsafe { self::text(text, len, "text") }?;
        Ok(rule.clone().description(description))
    };
    // SAFETY: the caller's promises are `step`'s.
    unsafe { step(rule, out, error, make) }
}

/// The rule with this metadata instead of its own: `metadata` is a map of
/// data values (copied), in its order. An opaque value or a null with a
/// reason other than `CR_REASON_NULL` anywhere in it is a
/// `CR_RULE_DEFINITION_ERROR` that names where; a `metadata` that is not a
/// map, `CR_INVALID_ARGUMENT`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `metadata` is `NULL` or a live
/// `cr_value`; `out` is `NULL` or points to a writable `cr_rule *`; `error` is
/// `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_with_metadata(
    rule: *const cr_rule,
    metadata: *const cr_value,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = |rule: &Rule| {
        // SAFETY: `metadata` is NULL or live (# Safety).
        let value = unsafe { borrow_value(metadata, "metadata") }?;
        let Value::Map(map) = value else {
            return Err(Failure::Invalid(
                "metadata is not a map".into(),
                "pass a map of data values",
            ));
        };
        let mut plain = Map::new();
        for (key, item) in map.iter() {
            plain.insert(key, item.clone());
        }
        Ok(rule.clone().try_metadata(plain)?)
    };
    // SAFETY: the caller's promises are `step`'s.
    unsafe { step(rule, out, error, make) }
}

/// The rule naming its action, which a program resolves in its registry
/// (`spec/rules.md`, *Actions*). An empty name is a
/// `CR_RULE_DEFINITION_ERROR`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `name` is `NULL` or points to `len`
/// readable bytes; `out` is `NULL` or points to a writable `cr_rule *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_with_action(
    rule: *const cr_rule,
    name: *const c_char,
    len: usize,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = |rule: &Rule| {
        // SAFETY: `name` and `len` (# Safety).
        let name = unsafe { text(name, len, "name") }?;
        Ok(rule.clone().action_name(name)?)
    };
    // SAFETY: the caller's promises are `step`'s.
    unsafe { step(rule, out, error, make) }
}

/// The rule's tags as the core keeps them, sorted and without repeats: a new
/// list of strings, freed with `cr_value_free`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_tags(
    rule: *const cr_rule,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `rule` is NULL or live (# Safety).
        let rule = &unsafe { handle(rule, "rule") }?.rule;
        let tags = rule
            .get_tags()
            .iter()
            .map(|tag| Value::String(tag.clone()))
            .collect();
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, Value::List(tags)) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The rule's metadata as the core keeps it: a new map, freed with
/// `cr_value_free`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `out` is `NULL` or points to a
/// writable `cr_value *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_metadata(
    rule: *const cr_rule,
    out: *mut *mut cr_value,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `rule` is NULL or live (# Safety).
        let rule = &unsafe { handle(rule, "rule") }?.rule;
        let metadata = Value::Map(rule.get_metadata().clone());
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { give_new_value(out, metadata) }
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// One rule written in the rule syntax (`spec/text.md`), as fuzzytool's
/// `rule_from_text` reads it: without a `RULE` line it is named `rule_1`. A
/// text that does not follow the syntax, or holds a condition or more than one
/// rule, is a `CR_RULE_SYNTAX_ERROR`; bytes that are not UTF-8, a
/// `CR_FORMAT_ERROR`. To read a condition or a rule set written as text, use
/// `cr_document_read` with `CR_FORMAT_TEXT`.
///
/// # Safety
///
/// `text` is `NULL` or points to `len` readable bytes; `limits` is `NULL` or a
/// live `cr_limits`; `out` is `NULL` or points to a writable `cr_rule *`;
/// `error` is `NULL` or points to a writable `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_from_text(
    text: *const c_char,
    len: usize,
    limits: *const cr_limits,
    out: *mut *mut cr_rule,
    error: *mut *mut cr_error,
) -> cr_status {
    let body = || {
        // SAFETY: `out` is NULL or writable (# Safety).
        unsafe { clear(out, "out") }?;
        // SAFETY: `text` and `len` (# Safety).
        let text = utf8_text(unsafe { bytes(text, len, "text") }?)?;
        // SAFETY: `limits` is NULL or live (# Safety).
        let limits = unsafe { limits_of(limits) };
        let rule = cleanrules::text::parse_rule(text, &limits)?;
        // SAFETY: `out` passed `clear`.
        unsafe { give(out, cr_rule { rule }) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// Lend a text of a rule: `pick` chooses it, `None` for a text the rule
/// lacks, which is `NULL` with a length of 0.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `text` and `len` are `NULL` or point
/// to a writable `const char *` and `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`.
unsafe fn lend(
    rule: *const cr_rule,
    text: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
    pick: impl FnOnce(&Rule) -> Outcome<Option<&str>>,
) -> cr_status {
    let body = || {
        // SAFETY: `text` and `len` are NULL or writable (# Safety).
        unsafe { clear_borrowed(text, len) };
        // SAFETY: `rule` is NULL or live (# Safety).
        let rule = &unsafe { handle(rule, "rule") }?.rule;
        let found = pick(rule)?;
        // SAFETY: `text` and `len` are NULL or writable (# Safety).
        unsafe { borrow_text(found, text, len) };
        Ok(())
    };
    // SAFETY: `error` is NULL or writable (# Safety).
    unsafe { call(error, body) }
}

/// The rule's name, borrowed from the rule and not NUL-terminated.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `name` and `len` are `NULL` or point
/// to a writable `const char *` and `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`. The name lives as long as the rule.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_name(
    rule: *const cr_rule,
    name: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `lend`'s.
    unsafe { lend(rule, name, len, error, |r| Ok(Some(r.name()))) }
}

/// The rule's description, borrowed from the rule and not NUL-terminated;
/// `NULL` with a length of 0 when it has none.
///
/// # Safety
///
/// As `cr_rule_name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_description(
    rule: *const cr_rule,
    text: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `lend`'s.
    unsafe { lend(rule, text, len, error, |r| Ok(r.get_description())) }
}

/// The name of the rule's action (`spec/rules.md`, *Actions*), borrowed from
/// the rule and not NUL-terminated; `NULL` with a length of 0 when it names
/// none.
///
/// # Safety
///
/// As `cr_rule_name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_action(
    rule: *const cr_rule,
    name: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `lend`'s.
    unsafe { lend(rule, name, len, error, |r| Ok(r.get_action_name())) }
}

/// The rule's priority; 0 for `NULL`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_priority(rule: *const cr_rule) -> i64 {
    // SAFETY: `rule` is NULL or live (# Safety).
    unsafe { rule.as_ref() }.map_or(0, |r| r.rule.get_priority())
}

/// A copy of the rule's condition, freed with `cr_node_free`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_when(
    rule: *const cr_rule,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: `rule` is NULL or live (# Safety).
    let make = || Ok(unsafe { handle(rule, "rule") }?.rule.when().clone());
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// The number of entries in the rule's payload; 0 for `NULL`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_payload_len(rule: *const cr_rule) -> usize {
    // SAFETY: `rule` is NULL or live (# Safety).
    unsafe { rule.as_ref() }.map_or(0, |r| r.rule.then().len())
}

/// The payload entry at `index`, in the payload's order.
fn entry(rule: &Rule, index: usize) -> Outcome<&(String, cleanrules::ast::Node)> {
    let payload = rule.then();
    payload.get(index).ok_or_else(|| {
        Failure::Invalid(
            format!(
                "entry {index} is out of the payload of {} entries",
                payload.len()
            ),
            "pass an index below cr_rule_payload_len",
        )
    })
}

/// The key of the payload entry at `index`, in order, borrowed from the rule
/// and not NUL-terminated.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `key` and `len` are `NULL` or point
/// to a writable `const char *` and `size_t`; `error` is `NULL` or points to a
/// writable `cr_error *` set to `NULL`. The key lives as long as the rule.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_payload_key(
    rule: *const cr_rule,
    index: usize,
    key: *mut *const c_char,
    len: *mut usize,
    error: *mut *mut cr_error,
) -> cr_status {
    // SAFETY: the caller's promises are `lend`'s.
    unsafe {
        lend(rule, key, len, error, |r| {
            Ok(Some(entry(r, index)?.0.as_str()))
        })
    }
}

/// A copy of the value node of the payload entry at `index`, in order, freed
/// with `cr_node_free`.
///
/// # Safety
///
/// `rule` is `NULL` or a live `cr_rule`; `out` is `NULL` or points to a
/// writable `cr_node *`; `error` is `NULL` or points to a writable
/// `cr_error *` set to `NULL`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_payload_value(
    rule: *const cr_rule,
    index: usize,
    out: *mut *mut cr_node,
    error: *mut *mut cr_error,
) -> cr_status {
    let make = || {
        // SAFETY: `rule` is NULL or live (# Safety).
        let rule = &unsafe { handle(rule, "rule") }?.rule;
        Ok(entry(rule, index)?.1.clone())
    };
    // SAFETY: `out` and `error` (# Safety).
    unsafe { build(out, error, make) }
}

/// Free a rule. `NULL` is ignored. Rule sets made with it keep their own copy.
///
/// # Safety
///
/// `rule` is `NULL` or a `cr_rule` from this library, not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cr_rule_free(rule: *mut cr_rule) {
    // SAFETY: the caller's promise is `free`'s.
    unsafe { free(rule) };
}
