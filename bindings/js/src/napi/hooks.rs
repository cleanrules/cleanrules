//! The JavaScript half of the binding, registered once per environment, and
//! the errors this layer throws.
//!
//! Each Node environment (the main thread and every worker) loads the addon
//! and keeps its own hooks, so nothing here is shared between threads.

use cleanrules::format::write_error;
use cleanrules::json::to_string;
use napi::bindgen_prelude::{FnArgs, Function, JsObjectValue, Object, ObjectRef, Unknown};
use napi::{Env, Error, Result, Status};
use napi_derive::napi;

/// The hooks object of `index.js`: `read`, `error`, `call` and `actions`.
struct Hooks {
    // Released when the environment is torn down, with the hooks themselves.
    object: ObjectRef<false>,
}

/// Register the JavaScript half of the binding; `index.js` calls it once when
/// the package loads.
///
/// # Errors
///
/// If the environment cannot hold the hooks.
#[napi]
pub fn set_hooks(env: &Env, hooks: Object<'_>) -> Result<()> {
    let object = hooks.create_ref::<false>()?;
    env.set_instance_data(Hooks { object }, (), |_| {})
}

/// A hook by name.
pub(crate) fn hook<'env, Args, Return>(
    env: &'env Env,
    name: &str,
) -> Result<Function<'env, Args, Return>>
where
    Args: napi::bindgen_prelude::JsValuesTupleIntoVec,
    Return: napi::bindgen_prelude::FromNapiValue,
{
    let Some(hooks) = env.get_instance_data::<Hooks>()? else {
        return Err(Error::from_reason(
            "the cleanrules addon was loaded without its JavaScript half: require('cleanrules') instead",
        ));
    };
    hooks.object.get_value(env)?.get_named_property(name)
}

/// Throw `value` and say so to napi-rs, which then leaves it pending.
fn thrown(env: &Env, value: Unknown<'_>) -> Error {
    match env.throw(value) {
        Ok(()) => Error::new(Status::PendingException, String::new()),
        Err(error) => error,
    }
}

/// The JavaScript error of a core error, built by the `error` hook from its
/// JSON form (`spec/errors.md`), thrown. `cause` is the JavaScript value that
/// caused it (a getter or an action that threw), and `decision` the decision
/// of an `action_error`.
pub(crate) fn throw(
    env: &Env,
    error: &cleanrules::Error,
    cause: Option<Unknown<'_>>,
    decision: Option<Unknown<'_>>,
) -> Error {
    let build = || -> Result<Unknown<'_>> {
        let json = to_string(&write_error(error));
        let hook = hook::<FnArgs<(String, Option<Unknown<'_>>, Option<Unknown<'_>>)>, Unknown<'_>>(
            env, "error",
        )?;
        hook.call(FnArgs::from((json, cause, decision)))
    };
    match build() {
        Ok(value) => thrown(env, value),
        // The hook failed (a broken install): say what the error was.
        Err(failure) => Error::from_reason(format!(
            "{} ({}); building its JavaScript error failed: {failure}",
            error.message(),
            error.code()
        )),
    }
}

/// A `RangeError`: an argument outside the values this layer accepts, such as
/// an unknown format name.
pub(crate) fn range_error(env: &Env, message: &str) -> Error {
    match env.throw_range_error(message, None) {
        Ok(()) => Error::new(Status::PendingException, String::new()),
        Err(error) => error,
    }
}

/// A `TypeError`: an argument of the wrong kind.
pub(crate) fn type_error(env: &Env, message: &str) -> Error {
    match env.throw_type_error(message, None) {
        Ok(()) => Error::new(Status::PendingException, String::new()),
        Err(error) => error,
    }
}
