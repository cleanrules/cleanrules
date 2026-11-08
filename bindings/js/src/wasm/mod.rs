//! The wasm-bindgen front end: the WebAssembly module (`wasm32`), for
//! browsers, Deno and any other JavaScript runtime.
//!
//! It mirrors the napi-rs front end over the same common code, so both builds
//! offer the same classes, accept the same arguments and throw the same
//! errors. A WebAssembly instance has no N-API instance data: the JavaScript
//! half (`lib/`) is registered per module instance with [`set_hooks`], and an
//! instance runs on one thread (a worker loads its own).

pub(crate) mod build;
pub(crate) mod classes;
pub(crate) mod documents;

use std::cell::RefCell;

use cleanrules::format::write_error;
use cleanrules::json::to_string;
use js_sys::{Array, ArrayBuffer, Function, Object, RangeError, Reflect, TypeError, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::common::{LIMIT_NAMES, OBJECT, Outcome, Problem, Raw, RawLimits, RawOptions};

/// The hooks of the JavaScript half: `read`, `error`, `call` and `actions`.
struct Hooks {
    read: Function,
    error: Function,
    actions: Function,
    call: Function,
}

thread_local! {
    // One instance runs on one thread, so this is the instance's.
    static HOOKS: RefCell<Option<Hooks>> = const { RefCell::new(None) };
}

fn function(hooks: &JsValue, name: &str) -> Result<Function, JsValue> {
    Reflect::get(hooks, &JsValue::from_str(name))?
        .dyn_into::<Function>()
        .map_err(|_| TypeError::new(&format!("the hook {name} is not a function")).into())
}

/// Register the JavaScript half of the binding; the package's loader calls it
/// once per module instance.
///
/// # Errors
///
/// A `TypeError` if a hook is not a function.
#[wasm_bindgen(js_name = setHooks)]
pub fn set_hooks(hooks: &JsValue) -> Result<(), JsValue> {
    let hooks = Hooks {
        read: function(hooks, "read")?,
        error: function(hooks, "error")?,
        actions: function(hooks, "actions")?,
        call: function(hooks, "call")?,
    };
    HOOKS.with(|slot| *slot.borrow_mut() = Some(hooks));
    Ok(())
}

/// Run `body` with the hooks.
fn with_hooks<T>(body: impl FnOnce(&Hooks) -> Result<T, JsValue>) -> Result<T, JsValue> {
    HOOKS.with(|slot| match slot.borrow().as_ref() {
        Some(hooks) => body(hooks),
        None => Err(js_sys::Error::new(
            "the cleanrules module was loaded without its JavaScript half: import 'cleanrules' instead",
        )
        .into()),
    })
}

/// The version of the JSON format this build reads and writes.
#[wasm_bindgen(js_name = formatVersion)]
#[must_use]
pub fn format_version() -> u32 {
    cleanrules::FORMAT_VERSION
}

/// The version of the package.
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Why `text` is not a decimal literal, or `null` when it is one.
#[wasm_bindgen(js_name = decimalProblem)]
#[must_use]
pub fn decimal_problem(text: &str) -> JsValue {
    nullable(crate::common::decimal_problem(text))
}

/// The `TypeError` of an argument that should be a `Document`, for the check
/// of `lib/wasm-ready.js`.
#[wasm_bindgen(js_name = notADocument)]
#[must_use]
pub fn not_a_document(value: &JsValue) -> JsValue {
    fail(crate::common::not_a_document(&raw(value)))
}

/// A string, or `null` for `None` (as the napi-rs build gives it).
pub(crate) fn nullable(value: Option<String>) -> JsValue {
    value.map_or(JsValue::NULL, |text| JsValue::from_str(&text))
}

/// The JavaScript error of a core error, built by the `error` hook from its
/// JSON form (`spec/errors.md`), with what caused it and, for an
/// `action_error`, the decision.
pub(crate) fn core_error(
    error: &cleanrules::Error,
    cause: &JsValue,
    decision: &JsValue,
) -> JsValue {
    // `toJSON()` gives it as an object: floats that hold integers typed.
    let mut form = write_error(error);
    crate::common::out::stringifiable(&mut form);
    let json = JsValue::from_str(&to_string(&form));
    let built = with_hooks(|hooks| {
        hooks
            .error
            .call3(&JsValue::UNDEFINED, &json, cause, decision)
    });
    match built {
        Ok(value) => value,
        // The hook failed (a broken install): say what the error was.
        Err(_) => js_sys::Error::new(&format!(
            "{} ({}); building its JavaScript error failed",
            error.message(),
            error.code()
        ))
        .into(),
    }
}

/// The JavaScript error of a problem.
pub(crate) fn fail(problem: Problem) -> JsValue {
    match problem {
        Problem::Core(error) => core_error(&error, &JsValue::NULL, &JsValue::NULL),
        Problem::Range(message) => RangeError::new(&message).into(),
        Problem::Type(message) => TypeError::new(&message).into(),
        Problem::Internal(message) => {
            js_sys::Error::new(&format!("cleanrules: {message}; please report it")).into()
        }
    }
}

/// Results of the common code as JavaScript ones.
pub(crate) trait OrFail<T> {
    /// The value, or the problem as a JavaScript error.
    fn or_fail(self) -> Result<T, JsValue>;
}

impl<T> OrFail<T> for Outcome<T> {
    fn or_fail(self) -> Result<T, JsValue> {
        self.map_err(fail)
    }
}

impl<T> OrFail<T> for cleanrules::Result<T> {
    fn or_fail(self) -> Result<T, JsValue> {
        self.map_err(|error| core_error(&error, &JsValue::NULL, &JsValue::NULL))
    }
}

/// What a JavaScript value is, as far as the common code needs to tell.
pub(crate) fn raw(value: &JsValue) -> Raw {
    if value.is_undefined() {
        Raw::Undefined
    } else if value.is_null() {
        Raw::Null
    } else if let Some(boolean) = value.as_bool() {
        Raw::Bool(boolean)
    } else if let Some(number) = value.as_f64() {
        Raw::Number(number)
    } else if let Some(text) = value.as_string() {
        Raw::String(text)
    } else if Array::is_array(value) {
        Raw::Other("an array")
    } else if value.is_function() {
        Raw::Other("a function")
    } else if value.is_bigint() {
        Raw::Other("a bigint")
    } else if value.is_symbol() {
        Raw::Other("a symbol")
    } else if ArrayBuffer::is_view(value) {
        // By brand, as napi-rs tells them, so a view of another realm counts.
        BRANDS.with(|brands| match brands.typed_array_name(value).as_deref() {
            Some("Uint8Array") => Raw::Bytes(Uint8Array::new(value).to_vec()),
            Some(_) => Raw::Other("a typed array"),
            None if brands.is_data_view(value) => Raw::Other("a DataView"),
            None => Raw::Other(OBJECT),
        })
    } else {
        Raw::Other(OBJECT)
    }
}

/// Built-in getters that tell views apart in any realm.
struct Brands {
    /// `%TypedArray%.prototype[Symbol.toStringTag]`: a typed array's name.
    typed_array_name: Option<Function>,
    /// `DataView.prototype.byteLength`: throws for anything but a `DataView`.
    data_view_length: Option<Function>,
}

thread_local! {
    static BRANDS: Brands = Brands::new();
}

fn getter_of(prototype: &JsValue, key: &JsValue) -> Option<Function> {
    let descriptor = Object::get_own_property_descriptor(&Object::from(prototype.clone()), key);
    Reflect::get(&descriptor, &JsValue::from_str("get"))
        .ok()?
        .dyn_into::<Function>()
        .ok()
}

impl Brands {
    fn new() -> Self {
        let bytes: JsValue = Uint8Array::new_with_length(0).into();
        let typed_array = Object::get_prototype_of(&Object::get_prototype_of(&bytes));
        let data_view = Reflect::get(&js_sys::global(), &JsValue::from_str("DataView"))
            .and_then(|class| Reflect::get(&class, &JsValue::from_str("prototype")))
            .ok();
        Self {
            typed_array_name: getter_of(&typed_array, &js_sys::Symbol::to_string_tag()),
            data_view_length: data_view
                .and_then(|prototype| getter_of(&prototype, &JsValue::from_str("byteLength"))),
        }
    }

    fn typed_array_name(&self, value: &JsValue) -> Option<String> {
        self.typed_array_name
            .as_ref()?
            .call0(value)
            .ok()?
            .as_string()
    }

    fn is_data_view(&self, value: &JsValue) -> bool {
        self.data_view_length
            .as_ref()
            .is_some_and(|getter| getter.call0(value).is_ok())
    }
}

fn member(object: &JsValue, name: &str) -> Result<Raw, JsValue> {
    Ok(raw(&Reflect::get(object, &JsValue::from_str(name))?))
}

/// A limits object; a getter that throws is thrown as is.
pub(crate) fn raw_limits(value: &JsValue) -> Result<RawLimits, JsValue> {
    let kind = raw(value);
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(RawLimits::Absent);
    }
    if !matches!(kind, Raw::Other(OBJECT)) {
        return Ok(RawLimits::Wrong(kind.describe()));
    }
    let mut values: [Raw; 7] = Default::default();
    for (slot, name) in values.iter_mut().zip(LIMIT_NAMES) {
        *slot = member(value, name)?;
    }
    Ok(RawLimits::Given(Box::new(values)))
}

/// An options object, read once; `None` when it is left out.
pub(crate) fn raw_options(value: &JsValue) -> Result<Option<RawOptions>, JsValue> {
    let kind = raw(value);
    if matches!(kind, Raw::Undefined | Raw::Null) {
        return Ok(None);
    }
    if !matches!(kind, Raw::Other(OBJECT)) {
        return Err(fail(crate::common::not_options(kind.describe())));
    }
    Ok(Some(RawOptions {
        format: member(value, "format")?,
        logic: member(value, "logic")?,
        strict: member(value, "strict")?,
        limits: raw_limits(&Reflect::get(value, &JsValue::from_str("limits"))?)?,
    }))
}

/// The elements of an array argument.
pub(crate) fn raw_list(value: &JsValue, what: &str) -> Result<Vec<Raw>, JsValue> {
    if !Array::is_array(value) {
        return Err(fail(crate::common::not_a_list(what, &raw(value))));
    }
    Ok(Array::from(value).iter().map(|item| raw(&item)).collect())
}

/// A plain JavaScript object with these members, in order.
pub(crate) fn object(members: &[(&str, JsValue)]) -> Result<Object, JsValue> {
    let object = Object::new();
    for (name, value) in members {
        Reflect::set(&object, &JsValue::from_str(name), value)?;
    }
    Ok(object)
}

/// The `read` hook: `[encoding, thrown]` for `data` along `owner`'s paths,
/// reading at most `max_values` values.
pub(crate) fn call_read(
    data: &JsValue,
    owner: &JsValue,
    max_values: usize,
) -> Result<Array, JsValue> {
    #[allow(clippy::cast_precision_loss)] // Limits beyond 2^53 are no limit at all.
    let max_values = JsValue::from_f64(max_values as f64);
    let result = with_hooks(|hooks| {
        hooks
            .read
            .call3(&JsValue::UNDEFINED, data, owner, &max_values)
    })?;
    Ok(Array::from(&result))
}

/// The `actions` hook: `[names, functions]`.
pub(crate) fn call_actions(registry: &JsValue) -> Result<Array, JsValue> {
    let result = with_hooks(|hooks| hooks.actions.call1(&JsValue::UNDEFINED, registry))?;
    Ok(Array::from(&result))
}

/// The `call` hook: `undefined`, or `[kind, thrown, description]`.
pub(crate) fn call_action(action: &JsValue, argument: &JsValue) -> Result<JsValue, JsValue> {
    with_hooks(|hooks| hooks.call.call2(&JsValue::UNDEFINED, action, argument))
}
