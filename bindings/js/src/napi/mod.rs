//! The napi-rs front end: the native Node addon (every target but wasm32).
//!
//! It turns JavaScript arguments into the common code's [`Raw`] values, calls
//! the common code, and throws its problems as JavaScript errors. The
//! JavaScript half (`lib/`) is registered per environment with
//! [`hooks::set_hooks`].

pub(crate) mod build;
pub(crate) mod cases;
pub(crate) mod classes;
pub(crate) mod documents;
pub(crate) mod hooks;

use napi::bindgen_prelude::{
    Array, ClassInstance, FromNapiValue, Function, JsObjectValue, JsValue, Object, Uint8Array,
    Unknown,
};
use napi::{Env, Result, ValueType};
use napi_derive::napi;

use crate::common::audit::{RawAuditLimits, RawAuditOptions};
use crate::common::bundles::RawCase;
use crate::common::cases::{RawCaseFile, RawTestLimits, RawTestOptions, cases_of};
use crate::common::{LIMIT_NAMES, LIMITS, OBJECT, Outcome, Problem, Raw, RawLimits, RawOptions};
use hooks::{range_error, throw, type_error};

/// The version of the JSON format this build reads and writes
/// (`spec/json-format.md`).
#[napi]
pub const FORMAT_VERSION: u32 = 1;

// The literal above is for the generated TypeScript declarations; it must be
// the core's.
const _: () = assert!(FORMAT_VERSION == cleanrules::FORMAT_VERSION);

/// The version of the package.
#[napi]
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Why `text` is not a decimal literal, or `null` when it is one; for the
/// `Decimal` marker of `lib/values.js`. A literal too large for 38 digits is a
/// decimal: as data it reads as null with reason `decimal_overflow`.
#[napi]
#[must_use]
pub fn decimal_problem(text: String) -> Option<String> {
    crate::common::decimal_problem(&text)
}

/// The JavaScript error of a problem, thrown.
pub(crate) fn fail(env: &Env, problem: Problem) -> napi::Error {
    match problem {
        Problem::Core(error) => throw(env, &error, None, None),
        Problem::Range(message) => range_error(env, &message),
        Problem::Type(message) => type_error(env, &message),
        Problem::Internal(message) => {
            napi::Error::from_reason(format!("cleanrules: {message}; please report it"))
        }
    }
}

/// Results of the common code as napi-rs ones.
pub(crate) trait OrFail<T> {
    /// The value, or the problem thrown as JavaScript.
    fn or_fail(self, env: &Env) -> Result<T>;
}

impl<T> OrFail<T> for Outcome<T> {
    fn or_fail(self, env: &Env) -> Result<T> {
        self.map_err(|problem| fail(env, problem))
    }
}

impl<T> OrFail<T> for cleanrules::Result<T> {
    fn or_fail(self, env: &Env) -> Result<T> {
        self.map_err(|error| throw(env, &error, None, None))
    }
}

/// What a JavaScript value is, as far as the common code needs to tell.
pub(crate) fn raw(value: &Unknown<'_>) -> Result<Raw> {
    Ok(match value.get_type()? {
        ValueType::Undefined => Raw::Undefined,
        ValueType::Null => Raw::Null,
        ValueType::Boolean => Raw::Bool(bool::from_unknown(*value)?),
        ValueType::Number => Raw::Number(f64::from_unknown(*value)?),
        ValueType::String => Raw::String(String::from_unknown(*value)?),
        ValueType::Object if value.is_array()? => Raw::Other("an array"),
        ValueType::Object if value.is_typedarray()? => match Uint8Array::from_unknown(*value) {
            Ok(bytes) => Raw::Bytes(bytes.to_vec()),
            Err(_) => Raw::Other("a typed array"),
        },
        ValueType::Object if value.is_dataview()? => Raw::Other("a DataView"),
        ValueType::Object => Raw::Other(OBJECT),
        ValueType::Function => Raw::Other("a function"),
        ValueType::BigInt => Raw::Other("a bigint"),
        ValueType::Symbol => Raw::Other("a symbol"),
        _ => Raw::Other("a value"),
    })
}

fn member(object: &Object<'_>, name: &str) -> Result<Raw> {
    raw(&object.get_named_property::<Unknown<'_>>(name)?)
}

/// A limits object; a getter that throws is thrown as is.
pub(crate) fn raw_limits(value: &Unknown<'_>) -> Result<RawLimits> {
    let kind = raw(value)?;
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(RawLimits::Absent);
    }
    if !matches!(kind, Raw::Other(OBJECT)) {
        return Ok(RawLimits::Wrong(kind.describe()));
    }
    let object = Object::from_unknown(*value)?;
    let mut values: [Raw; LIMITS] = Default::default();
    for (slot, name) in values.iter_mut().zip(LIMIT_NAMES) {
        *slot = member(&object, name)?;
    }
    Ok(RawLimits::Given(Box::new(values)))
}

/// An options object, read once; `None` when it is left out.
pub(crate) fn raw_options(env: &Env, value: &Unknown<'_>) -> Result<Option<RawOptions>> {
    let kind = raw(value)?;
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(None);
    }
    if !matches!(kind, Raw::Other(OBJECT)) {
        return Err(fail(env, crate::common::not_options(kind.describe())));
    }
    let object = Object::from_unknown(*value)?;
    Ok(Some(RawOptions {
        format: member(&object, "format")?,
        logic: member(&object, "logic")?,
        strict: member(&object, "strict")?,
        limits: raw_limits(&object.get_named_property::<Unknown<'_>>("limits")?)?,
    }))
}

/// The options of an audit, read once: the defaults when they are left out.
pub(crate) fn raw_audit_options(value: Option<Unknown<'_>>) -> Result<RawAuditOptions> {
    let Some(value) = value else {
        return Ok(RawAuditOptions::Absent);
    };
    let kind = raw(&value)?;
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(RawAuditOptions::Absent);
    }
    if !matches!(kind, Raw::Other(OBJECT)) {
        return Ok(RawAuditOptions::Wrong(kind.describe()));
    }
    let object = Object::from_unknown(value)?;
    let limits = object.get_named_property::<Unknown<'_>>("limits")?;
    let limits = match raw(&limits)? {
        Raw::Undefined | Raw::Null => RawAuditLimits::Absent,
        Raw::Other(OBJECT) => {
            let limits = Object::from_unknown(limits)?;
            RawAuditLimits::Given {
                max_terms: member(&limits, "maxTerms")?,
                max_steps: member(&limits, "maxSteps")?,
            }
        }
        other => RawAuditLimits::Wrong(other.describe()),
    };
    Ok(RawAuditOptions::Given {
        format: member(&object, "format")?,
        limits,
        ruleset_digest: member(&object, "rulesetDigest")?,
    })
}

/// The case files of `Document.test`, read once: none when they are left out.
pub(crate) fn raw_case_files(env: &Env, value: Option<Unknown<'_>>) -> Result<Vec<RawCaseFile>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let kind = raw(&value)?;
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(Vec::new());
    }
    if !value.is_array()? {
        return Err(fail(env, crate::common::not_a_list("files", &kind)));
    }
    let array = Array::from_unknown(value)?;
    (0..array.len())
        .map(|i| {
            let Some(item) = array.get::<Unknown<'_>>(i)? else {
                return Ok(RawCaseFile::Wrong("undefined"));
            };
            match raw(&item)? {
                Raw::Other(OBJECT) => {
                    let object = Object::from_unknown(item)?;
                    let cases = object.get_named_property::<Unknown<'_>>("cases")?;
                    let cases = match raw(&cases)? {
                        Raw::Undefined | Raw::Null => None,
                        other => Some(
                            match ClassInstance::<classes::Document>::from_unknown(cases) {
                                Ok(document) => cases_of(
                                    &document.core,
                                    usize::try_from(i).unwrap_or(usize::MAX),
                                ),
                                Err(_) => Err(Problem::Type(format!(
                                    "files[{i}].cases is a Document that holds a case file, not {}",
                                    match other {
                                        Raw::Other(OBJECT) => "another object",
                                        other => other.describe(),
                                    }
                                ))),
                            },
                        ),
                    };
                    Ok(RawCaseFile::Given {
                        path: member(&object, "path")?,
                        content: member(&object, "content")?,
                        cases,
                    })
                }
                other => Ok(RawCaseFile::Wrong(other.describe())),
            }
        })
        .collect()
}

/// The options of `Document.test`, read once, and its `cancel` callback.
pub(crate) fn raw_test_options<'env>(
    env: &Env,
    value: Option<Unknown<'env>>,
) -> Result<(RawTestOptions, Option<Function<'env, (), Unknown<'env>>>)> {
    let Some(value) = value else {
        return Ok((RawTestOptions::Absent, None));
    };
    let kind = raw(&value)?;
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok((RawTestOptions::Absent, None));
    }
    if !matches!(kind, Raw::Other(OBJECT)) {
        return Ok((RawTestOptions::Wrong(kind.describe()), None));
    }
    let object = Object::from_unknown(value)?;
    let limits = object.get_named_property::<Unknown<'_>>("limits")?;
    let limits = match raw(&limits)? {
        Raw::Undefined | Raw::Null => RawTestLimits::Absent,
        Raw::Other(OBJECT) => {
            let limits = Object::from_unknown(limits)?;
            RawTestLimits::Given {
                max_results: member(&limits, "maxResults")?,
                max_steps: member(&limits, "maxSteps")?,
                max_explained: member(&limits, "maxExplained")?,
            }
        }
        other => RawTestLimits::Wrong(other.describe()),
    };
    let cancel = object.get_named_property::<Unknown<'env>>("cancel")?;
    let cancel = match raw(&cancel)? {
        Raw::Undefined | Raw::Null => None,
        Raw::Other("a function") => Some(Function::from_unknown(cancel)?),
        other => {
            return Err(type_error(
                env,
                &format!(
                    "options.cancel is a function that returns a boolean, not {}",
                    other.describe()
                ),
            ));
        }
    };
    Ok((
        RawTestOptions::Given {
            format: member(&object, "format")?,
            limits,
            ruleset_digest: member(&object, "rulesetDigest")?,
        },
        cancel,
    ))
}

/// The `rulesetDigest` member of a decision's write options, read once:
/// `undefined` when the options are left out.
pub(crate) fn raw_ruleset_digest(env: &Env, value: Option<Unknown<'_>>) -> Result<Raw> {
    let Some(value) = value else {
        return Ok(Raw::Undefined);
    };
    let kind = raw(&value)?;
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(Raw::Undefined);
    }
    if !matches!(kind, Raw::Other(OBJECT)) {
        return Err(fail(env, crate::common::not_write_options(kind.describe())));
    }
    member(&Object::from_unknown(value)?, "rulesetDigest")
}

/// The elements of an array argument.
pub(crate) fn raw_list(env: &Env, value: &Unknown<'_>, what: &str) -> Result<Vec<Raw>> {
    if !value.is_array()? {
        return Err(fail(env, crate::common::not_a_list(what, &raw(value)?)));
    }
    let array = Array::from_unknown(*value)?;
    (0..array.len())
        .map(|i| match array.get::<Unknown<'_>>(i)? {
            Some(item) => raw(&item),
            None => Ok(Raw::Undefined),
        })
        .collect()
}

/// The case files of a bundle, `[{ path, content }]`: none when left out.
pub(crate) fn raw_cases(env: &Env, value: Option<Unknown<'_>>) -> Result<Vec<RawCase>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let kind = raw(&value)?;
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(Vec::new());
    }
    if !value.is_array()? {
        return Err(fail(env, crate::common::not_a_list("cases", &kind)));
    }
    let array = Array::from_unknown(value)?;
    (0..array.len())
        .map(|i| {
            let Some(item) = array.get::<Unknown<'_>>(i)? else {
                return Ok(RawCase::Wrong("undefined"));
            };
            match raw(&item)? {
                Raw::Other(OBJECT) => {
                    let object = Object::from_unknown(item)?;
                    Ok(RawCase::Given {
                        path: member(&object, "path")?,
                        content: member(&object, "content")?,
                    })
                }
                other => Ok(RawCase::Wrong(other.describe())),
            }
        })
        .collect()
}
