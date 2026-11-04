//! Reading native JavaScript data (`spec/data-model.md`, *Reading native
//! data*), through the `read` hook of `lib/read.js`.
//!
//! The hook reads only the paths the rule set or node reads (it asks the
//! owner, a `RuleSet` or a `Node`, for its `fieldPaths()` once and keeps the
//! tree), each native object once, and returns the encoded sparse tree with the
//! values that getters threw. Those are kept for the duration of the call, so
//! that strict mode can raise `EvaluationError` with the thrown value as its
//! `cause`.

use cleanrules::{Error, ErrorCode, Segment, Value};
use napi::bindgen_prelude::{Array, FnArgs, Unknown};
use napi::{Env, Result, ValueType};

use crate::hooks::{hook, throw};
use crate::wire::decode;

/// One input, read.
pub(crate) struct Native<'env> {
    pub(crate) value: Value,
    /// Where each `read_error` was met, and the index of its thrown value.
    errors: Vec<(Vec<Segment>, u32)>,
    /// The values thrown while reading, in order.
    thrown: Option<Array<'env>>,
}

/// Read `data` along the paths of `owner`.
///
/// # Errors
///
/// A `TypeError` from the hook for an `undefined` input, or a failure of the
/// binding itself; values thrown by the data become `read_error` nulls.
pub(crate) fn read<'env>(
    env: &'env Env,
    data: Unknown<'env>,
    owner: Unknown<'env>,
) -> Result<Native<'env>> {
    let read = hook::<FnArgs<(Unknown<'env>, Unknown<'env>)>, Array<'env>>(env, "read")?;
    let result = read.call(FnArgs::from((data, owner)))?;
    let wire: String = result
        .get(0)?
        .ok_or_else(|| napi::Error::from_reason("the reader returned no encoding"))?;
    let thrown: Option<Array<'env>> = result.get(1)?;
    let read = decode(&wire).map_err(|problem| {
        napi::Error::from_reason(format!(
            "cleanrules: the reader's encoding is wrong ({problem}); please report it"
        ))
    })?;
    Ok(Native {
        value: read.value,
        errors: read.errors,
        thrown,
    })
}

impl Native<'_> {
    /// The value thrown while reading `path` or one of its prefixes.
    fn cause_of(&self, path: &[Segment]) -> Option<Unknown<'_>> {
        let (_, index) = self.errors.iter().find(|(at, _)| path.starts_with(at))?;
        let thrown = self.thrown.as_ref()?;
        let value: Option<Unknown<'_>> = thrown.get(*index).ok()?;
        value.filter(|v| v.get_type().is_ok_and(|t| t != ValueType::Undefined))
    }
}

/// The JavaScript error of a core error, thrown; a `read_error` in strict mode
/// gets the value that reading the data threw as its `cause`.
pub(crate) fn chained(env: &Env, error: &Error, native: &Native<'_>) -> napi::Error {
    let cause = match (&error.context().path, error.code()) {
        (Some(path), ErrorCode::EvaluationError) => native.cause_of(path),
        _ => None,
    };
    throw(env, error, cause, None)
}
