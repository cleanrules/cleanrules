//! Python values and core values (`spec/data-model.md`, *Conversions by
//! language*, *Python*).
//!
//! This version converts constants. Reading native input data (sparse trees,
//! records, `read_error`) arrives in phase C.3.

use cleanrules::jiff::civil::{Date, DateTime, Time};
use cleanrules::jiff::tz::{Offset, TimeZone};
use cleanrules::value::{AwareDateTime, DecimalError, Zone};
use cleanrules::{Decimal, Map, NullCause, Value};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyBool, PyDate, PyDateAccess, PyDateTime, PyDelta, PyDeltaAccess, PyDict, PyFloat, PyInt,
    PyList, PyString, PyTimeAccess, PyTuple, PyType, PyTzInfo, PyTzInfoAccess,
};

use crate::errors;

/// Why a Python value has no value of the model.
pub(crate) enum Problem {
    /// An integer outside 64 bits.
    IntOverflow,
    /// A decimal that does not fit in 38 digits.
    DecimalOverflow,
    /// A NaN (or any value not equal to itself).
    NotANumber,
    /// A datetime finer than a microsecond (`pandas.Timestamp` nanoseconds).
    TooPrecise,
    /// A value with no equivalent, by its type name.
    Opaque(String),
}

static DECIMAL: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static INTEGRAL: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static REAL: PyOnceLock<Py<PyType>> = PyOnceLock::new();

pub(crate) fn type_name(value: &Bound<'_, PyAny>) -> String {
    value
        .get_type()
        .name()
        .map_or_else(|_| "object".to_owned(), |name| name.to_string())
}

/// A Python `Decimal` as a model decimal.
fn decimal(value: &Bound<'_, PyAny>) -> PyResult<Result<Value, Problem>> {
    if value.call_method0("is_nan")?.is_truthy()? {
        return Ok(Err(Problem::NotANumber));
    }
    if value.call_method0("is_infinite")?.is_truthy()? {
        let negative = value.call_method0("is_signed")?.is_truthy()?;
        return Ok(Ok(Value::Decimal(if negative {
            Decimal::NEG_INFINITY
        } else {
            Decimal::INFINITY
        })));
    }
    // (sign, digits, exponent) gives the exact value with its scale.
    let (sign, digits, exponent): (u8, Vec<u8>, i64) = value.call_method0("as_tuple")?.extract()?;
    let mut text = String::with_capacity(digits.len() + 8);
    if sign == 1 {
        text.push('-');
    }
    text.extend(digits.iter().map(|d| char::from(b'0' + d)));
    text.push('E');
    text.push_str(&exponent.to_string());
    Ok(match Decimal::parse(&text) {
        Ok(parsed) => Ok(Value::Decimal(parsed)),
        Err(DecimalError::Malformed) => Err(Problem::Opaque("Decimal".into())),
        Err(_) => Err(Problem::DecimalOverflow),
    })
}

fn civil_date(value: &impl PyDateAccess) -> PyResult<Date> {
    Date::new(
        value.get_year().try_into()?,
        value.get_month().try_into()?,
        value.get_day().try_into()?,
    )
    .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))
}

/// A `timedelta` offset as whole seconds, if it is whole seconds.
fn offset_seconds(delta: &Bound<'_, PyDelta>) -> Option<i32> {
    if delta.get_microseconds() != 0 {
        return None;
    }
    delta
        .get_days()
        .checked_mul(86_400)?
        .checked_add(delta.get_seconds())
}

/// The IANA name of a `tzinfo`, when it has one (`zoneinfo` keys, `pytz` zones).
fn zone_name(tzinfo: &Bound<'_, PyTzInfo>) -> Option<String> {
    for attribute in ["key", "zone"] {
        if let Ok(name) = tzinfo.getattr(attribute)
            && let Ok(name) = name.extract::<String>()
        {
            return Some(name);
        }
    }
    None
}

fn datetime(value: &Bound<'_, PyDateTime>) -> PyResult<Result<Value, Problem>> {
    // pandas.Timestamp keeps nanoseconds beyond the datetime's microseconds.
    if let Ok(nanosecond) = value.getattr("nanosecond")
        && nanosecond.extract::<i64>().is_ok_and(|n| n != 0)
    {
        return Ok(Err(Problem::TooPrecise));
    }
    let date = civil_date(value)?;
    let civil = DateTime::from_parts(
        date,
        Time::new(
            value.get_hour().try_into()?,
            value.get_minute().try_into()?,
            value.get_second().try_into()?,
            i32::try_from(value.get_microsecond())? * 1000,
        )
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?,
    );
    let offset = match value.get_tzinfo() {
        None => None,
        Some(tzinfo) => {
            let delta = tzinfo.call_method1("utcoffset", (value,))?;
            if delta.is_none() {
                None
            } else {
                Some((tzinfo, delta.cast_into::<PyDelta>()?))
            }
        }
    };
    let Some((tzinfo, delta)) = offset else {
        return Ok(Ok(Value::NaiveDateTime(civil)));
    };
    let opaque = || Ok(Err(Problem::Opaque(type_name(value))));
    let Some(seconds) = offset_seconds(&delta) else {
        return opaque();
    };
    let Ok(offset) = Offset::from_seconds(seconds) else {
        return opaque();
    };
    // The instant is the local time minus its offset.
    let Ok(timestamp) = civil
        .to_zoned(offset.to_time_zone())
        .map(|zoned| zoned.timestamp())
    else {
        return opaque();
    };
    let name = zone_name(&tzinfo);
    Ok(
        match AwareDateTime::from_native(timestamp, offset, name.as_deref()) {
            Ok(aware) => Ok(Value::AwareDateTime(aware)),
            Err(_) => Err(Problem::Opaque(type_name(value))),
        },
    )
}

/// A Python scalar as a value, or why it has none.
///
/// # Errors
///
/// A Python exception raised while reading the value (a `utcoffset()` that
/// raises, for example).
pub(crate) fn scalar(value: &Bound<'_, PyAny>) -> PyResult<Result<Value, Problem>> {
    let py = value.py();
    if value.is_none() {
        return Ok(Ok(Value::Null(NullCause::Null)));
    }
    if let Some(converted) = numpy_or_pandas(value)? {
        return Ok(converted);
    }
    if let Ok(boolean) = value.cast::<PyBool>() {
        return Ok(Ok(Value::Bool(boolean.is_true())));
    }
    // numpy.bool_ is not a bool and not an int.
    let class = value.get_type();
    if class.module()?.to_str()? == "numpy" && class.name()?.to_str()?.starts_with("bool") {
        return Ok(Ok(Value::Bool(value.is_truthy()?)));
    }
    if value.is_instance_of::<PyInt>()
        || value.is_instance(INTEGRAL.import(py, "numbers", "Integral")?)?
    {
        let index = value.call_method0("__index__")?;
        return Ok(index
            .extract::<i64>()
            .map(Value::Int)
            .map_err(|_| Problem::IntOverflow));
    }
    if value.is_instance(DECIMAL.import(py, "decimal", "Decimal")?)? {
        return decimal(value);
    }
    if value.is_instance_of::<PyFloat>()
        || value.is_instance(REAL.import(py, "numbers", "Real")?)?
    {
        let number: f64 = value.call_method0("__float__")?.extract()?;
        return Ok(if number.is_nan() {
            Err(Problem::NotANumber)
        } else {
            Ok(Value::float(number))
        });
    }
    if let Ok(text) = value.cast::<PyString>() {
        // A string with a lone surrogate has no UTF-8 form.
        return Ok(text
            .to_str()
            .map(|s| Value::String(s.to_owned()))
            .map_err(|_| Problem::Opaque("str".into())));
    }
    if let Ok(aware_or_naive) = value.cast::<PyDateTime>() {
        return datetime(aware_or_naive);
    }
    if let Ok(date) = value.cast::<PyDate>() {
        return Ok(Ok(Value::Date(civil_date(date)?)));
    }
    Ok(Err(Problem::Opaque(type_name(value))))
}

fn is_pandas_missing(module: &str, name: &str) -> bool {
    module.starts_with("pandas") && (name == "NAType" || name == "NaTType")
}

/// Whether `value` is `pandas.NA` or `pandas.NaT`, which read as null scalars.
///
/// # Errors
///
/// If the type's module or name cannot be read.
pub(crate) fn is_missing_scalar(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    let class = value.get_type();
    Ok(is_pandas_missing(
        class.module()?.to_str()?,
        class.name()?.to_str()?,
    ))
}

/// The numpy and pandas values the generic rules would get wrong: their
/// missing values (`pandas.NA`, `pandas.NaT`, numpy's `NaT`) are null,
/// `numpy.datetime64` is a naive datetime (truncated to the microsecond), and
/// `numpy.timedelta64`, which numpy makes an integer, is opaque.
fn numpy_or_pandas(value: &Bound<'_, PyAny>) -> PyResult<Option<Result<Value, Problem>>> {
    let class = value.get_type();
    let module = class.module()?;
    let module = module.to_str()?;
    let name = class.name()?;
    let name = name.to_str()?;
    if is_pandas_missing(module, name) {
        return Ok(Some(Ok(Value::Null(NullCause::Null))));
    }
    if module != "numpy" || !(name == "datetime64" || name == "timedelta64") {
        return Ok(None);
    }
    // NaT is the only value not equal to itself.
    if !value.eq(value)? {
        return Ok(Some(Ok(Value::Null(NullCause::Null))));
    }
    if name == "timedelta64" {
        return Ok(Some(Err(Problem::Opaque(name.to_owned()))));
    }
    // Microseconds give a datetime.datetime; beyond its years numpy gives an int.
    let item = value
        .call_method1("astype", ("datetime64[us]",))?
        .call_method0("item")?;
    Ok(Some(if item.is_instance_of::<PyDateTime>() {
        scalar(&item)?
    } else {
        Err(Problem::Opaque(name.to_owned()))
    }))
}

/// A Python constant as a value (`spec/data-model.md`, *Constants*).
///
/// # Errors
///
/// `ExpressionError` for a value that is not a constant.
pub(crate) fn constant(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let py = value.py();
    let shown = value
        .repr()
        .map_or_else(|_| "?".to_owned(), |r| r.to_string());
    match scalar(value)? {
        Ok(converted) => Ok(converted),
        Err(Problem::IntOverflow) => Err(errors::expression(
            py,
            format!("the integer {shown} does not fit in 64 bits"),
            "use a Decimal for larger exact numbers",
        )),
        Err(Problem::DecimalOverflow) => Err(errors::expression(
            py,
            format!("the decimal {shown} does not fit in 38 digits"),
            "decimals hold up to 38 significant digits and are never rounded",
        )),
        Err(Problem::NotANumber) => Err(errors::expression(
            py,
            format!("a constant cannot be NaN ({shown})"),
            "test for missing values with .is_null()",
        )),
        Err(Problem::TooPrecise) => Err(errors::expression(
            py,
            format!("the datetime {shown} is finer than a microsecond"),
            "datetimes hold microseconds; round the constant first",
        )),
        Err(Problem::Opaque(name)) => Err(errors::expression(
            py,
            format!("a {name} cannot be a constant: {shown}"),
            "constants are None, bool, int, float, Decimal, str, date or datetime",
        )),
    }
}

fn timezone<'py>(py: Python<'py>, zone: &Zone) -> PyResult<Bound<'py, PyTzInfo>> {
    match zone {
        Zone::Fixed(offset) if offset.seconds() == 0 => {
            PyTzInfo::utc(py).map(pyo3::Borrowed::to_owned)
        }
        Zone::Fixed(offset) => {
            let delta = PyDelta::new(py, 0, offset.seconds(), 0, true)?;
            PyTzInfo::fixed_offset(py, delta)
        }
        Zone::Named(name) => PyTzInfo::timezone(py, name),
    }
}

/// A value as a Python object.
///
/// # Errors
///
/// A Python exception raised while building it (an unknown zone name in
/// `zoneinfo`, for example).
pub(crate) fn to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    to_py_with(py, value, &[])
}

/// A value as a Python object; an opaque value gives back its native object from
/// `objects`, by token.
///
/// # Errors
///
/// As [`to_py`].
pub(crate) fn to_py_with<'py>(
    py: Python<'py>,
    value: &Value,
    objects: &[Py<PyAny>],
) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::Null(_) => py.None().into_bound(py),
        // The native object read from the input, kept by the reader.
        Value::Opaque(opaque) => usize::try_from(opaque.token)
            .ok()
            .and_then(|i| objects.get(i))
            .map_or_else(
                || py.None().into_bound(py),
                |object| object.bind(py).clone(),
            ),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any(),
        Value::Int(i) => i.into_pyobject(py)?.into_any(),
        Value::Float(f) => f.into_pyobject(py)?.into_any(),
        Value::Decimal(d) => DECIMAL
            .import(py, "decimal", "Decimal")?
            .call1((d.to_string(),))?,
        Value::String(s) => PyString::new(py, s).into_any(),
        Value::Date(d) => PyDate::new(
            py,
            d.year().into(),
            d.month().cast_unsigned(),
            d.day().cast_unsigned(),
        )?
        .into_any(),
        Value::NaiveDateTime(dt) => naive(py, *dt, None)?.into_any(),
        Value::AwareDateTime(aware) => {
            let utc = aware.timestamp().to_zoned(TimeZone::UTC).datetime();
            let tz = PyTzInfo::utc(py)?.to_owned();
            let instant = naive(py, utc, Some(&tz))?;
            instant.call_method1("astimezone", (timezone(py, aware.zone())?,))?
        }
        Value::List(items) => {
            let list = PyList::empty(py);
            for item in items {
                list.append(to_py_with(py, item, objects)?)?;
            }
            list.into_any()
        }
        Value::Map(map) => {
            let dict = PyDict::new(py);
            for (key, item) in map.iter() {
                dict.set_item(key, to_py_with(py, item, objects)?)?;
            }
            dict.into_any()
        }
    })
}

fn naive<'py>(
    py: Python<'py>,
    dt: DateTime,
    tzinfo: Option<&Bound<'py, PyTzInfo>>,
) -> PyResult<Bound<'py, PyDateTime>> {
    PyDateTime::new(
        py,
        dt.year().into(),
        dt.month().cast_unsigned(),
        dt.day().cast_unsigned(),
        dt.hour().cast_unsigned(),
        dt.minute().cast_unsigned(),
        dt.second().cast_unsigned(),
        u32::try_from(dt.subsec_nanosecond() / 1000)?,
        tzinfo,
    )
}

/// How deep input data and metadata are converted: the format's node depth.
pub(crate) const MAX_DEPTH: usize = 256;

/// Rule metadata as a value: data values only, converted with the rules of
/// constants (`spec/json-format.md`: no NaN, nothing opaque).
///
/// # Errors
///
/// A message naming the key path of the first value that is not data.
pub(crate) fn metadata(value: &Bound<'_, PyAny>) -> PyResult<Result<Value, String>> {
    metadata_at(value, "metadata", &mut Vec::new())
}

fn metadata_at(
    value: &Bound<'_, PyAny>,
    at: &str,
    open: &mut Vec<usize>,
) -> PyResult<Result<Value, String>> {
    let is_dict = value.cast::<PyDict>().is_ok();
    let is_sequence = value.cast::<PyList>().is_ok() || value.cast::<PyTuple>().is_ok();
    if is_dict || is_sequence {
        let id = value.as_ptr() as usize;
        if open.contains(&id) {
            return Ok(Err(format!("{at} contains itself")));
        }
        if open.len() >= MAX_DEPTH {
            return Ok(Err(format!(
                "{at} is nested deeper than {MAX_DEPTH} levels"
            )));
        }
        open.push(id);
        let converted = if let Ok(dict) = value.cast::<PyDict>() {
            let mut map = Map::new();
            for (key, item) in dict.iter() {
                let Ok(key) = key.extract::<String>() else {
                    return Ok(Err(format!("{at} has a key that is not a string: {key}")));
                };
                match metadata_at(&item, &format!("{at}[{key:?}]"), open)? {
                    Ok(item) => map.insert(key, item),
                    Err(problem) => return Ok(Err(problem)),
                }
            }
            Value::Map(map)
        } else {
            let mut items = Vec::new();
            for (index, item) in value.try_iter()?.enumerate() {
                match metadata_at(&item?, &format!("{at}[{index}]"), open)? {
                    Ok(item) => items.push(item),
                    Err(problem) => return Ok(Err(problem)),
                }
            }
            Value::List(items)
        };
        open.pop();
        return Ok(Ok(converted));
    }
    let shown = value
        .repr()
        .map_or_else(|_| "?".to_owned(), |r| r.to_string());
    Ok(match scalar(value)? {
        Ok(converted) => Ok(converted),
        Err(Problem::NotANumber) => Err(format!("{at} is NaN, which is not data")),
        Err(_) => Err(format!("{at} is not a data value: {shown}")),
    })
}

/// The `decimal.Decimal` class.
///
/// # Errors
///
/// If `decimal` cannot be imported.
pub(crate) fn decimal_class(py: Python<'_>) -> PyResult<&Bound<'_, PyType>> {
    DECIMAL.import(py, "decimal", "Decimal")
}

/// Whether `value` is a numpy scalar (not an array): `numpy.bool_`,
/// `numpy.datetime64` and the others, read as scalars.
///
/// # Errors
///
/// If the type's module cannot be read.
pub(crate) fn is_numpy_scalar(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    let class = value.get_type();
    Ok(class.module()?.to_str()? == "numpy" && class.name()?.to_str()? != "ndarray")
}

/// A datetime finer than a microsecond (`pandas.Timestamp`), truncated to its
/// microseconds as data is (`spec/data-model.md`, *Datetimes*).
///
/// # Errors
///
/// If the datetime's parts cannot be read.
pub(crate) fn truncated(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let datetime = value.cast::<PyDateTime>()?;
    let plain = PyDateTime::new(
        value.py(),
        datetime.get_year(),
        datetime.get_month(),
        datetime.get_day(),
        datetime.get_hour(),
        datetime.get_minute(),
        datetime.get_second(),
        datetime.get_microsecond(),
        datetime.get_tzinfo().as_ref(),
    )?;
    Ok(match scalar(&plain)? {
        Ok(converted) => converted,
        Err(_) => Value::Null(NullCause::Null),
    })
}
