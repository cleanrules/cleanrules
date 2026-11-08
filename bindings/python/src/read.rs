//! Reading native Python data (`spec/data-model.md`, *Reading native data*).
//!
//! The binding reads only the paths a rule set reads, walking the native
//! structure segment by segment, each native object once, and hands the core a
//! sparse value tree. It reads public data only: map keys, record components
//! and objects' public data attributes, never a private name, a method, a class
//! or a module. A getter that raises gives a null with reason `read_error`,
//! except `AttributeError`, which means missing; the exception is kept so that
//! strict mode can chain it.
//!
//! What is read is counted against `max_values` (`spec/data-model.md`,
//! *Reading native data*, *Limits*): beyond it, reading stops at once and the
//! input is refused with the core's `limit_exceeded`.

use std::collections::{HashMap, HashSet};

use cleanrules::value::{ValueCount, too_many_values};
use cleanrules::{Error, Map, NullCause, Segment, Value};
use pyo3::exceptions::{PyAttributeError, PyKeyError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyByteArray, PyBytes, PyDate, PyDelta, PyDict, PyList, PyModule, PyString, PyTime, PyTuple,
    PyType,
};

use crate::convert::{self, MAX_DEPTH, Problem};
use crate::errors;

/// The paths a rule set reads, as a tree of segments.
#[derive(Clone, Debug, Default)]
pub(crate) struct Paths {
    /// A path ends here: the whole value is needed.
    end: bool,
    keys: Vec<(String, Paths)>,
    indices: Vec<(u64, Paths)>,
}

impl Paths {
    /// The tree of `paths`.
    pub(crate) fn new<'a>(paths: impl IntoIterator<Item = &'a [Segment]>) -> Self {
        let mut root = Paths::default();
        for path in paths {
            root.add(path);
        }
        root
    }

    fn add(&mut self, path: &[Segment]) {
        let Some((first, rest)) = path.split_first() else {
            self.end = true;
            return;
        };
        let child = match first {
            Segment::Key(key) => {
                let at = self
                    .keys
                    .iter()
                    .position(|(k, _)| k == key)
                    .unwrap_or_else(|| {
                        self.keys.push((key.clone(), Paths::default()));
                        self.keys.len() - 1
                    });
                &mut self.keys[at].1
            }
            Segment::Index(index) => {
                let at = self
                    .indices
                    .iter()
                    .position(|(i, _)| i == index)
                    .unwrap_or_else(|| {
                        self.indices.push((*index, Paths::default()));
                        self.indices.len() - 1
                    });
                &mut self.indices[at].1
            }
        };
        child.add(rest);
    }

    /// Both trees at once: a record field read by key and by position.
    fn merged(&self, other: &Paths) -> Paths {
        let mut out = self.clone();
        out.end |= other.end;
        for (key, sub) in &other.keys {
            match out.keys.iter_mut().find(|(k, _)| k == key) {
                Some((_, mine)) => *mine = mine.merged(sub),
                None => out.keys.push((key.clone(), sub.clone())),
            }
        }
        for (index, sub) in &other.indices {
            match out.indices.iter_mut().find(|(i, _)| i == index) {
                Some((_, mine)) => *mine = mine.merged(sub),
                None => out.indices.push((*index, sub.clone())),
            }
        }
        out
    }
}

/// One input, read: the sparse value, the native objects of its opaque values
/// (a token is a position here), and the exceptions raised while reading, by
/// full path.
pub(crate) struct Native {
    pub(crate) value: Value,
    pub(crate) objects: Vec<Py<PyAny>>,
    pub(crate) errors: Vec<(Vec<Segment>, PyErr)>,
}

/// How a native value is read.
enum Shape {
    /// A scalar, or something nothing may be read through (a class, a module,
    /// a routine, a frame...): it is converted whole.
    Scalar,
    /// A mapping: string segments read keys.
    Mapping,
    /// A record: its components, by name and by position. Namedtuple-like rows
    /// are read by attribute, objects with `keys()` by item.
    Record {
        keys: Vec<String>,
        by_attribute: bool,
    },
    /// A sequence of this length: integer segments read elements.
    Sequence(usize),
    /// Another object: string segments read public data attributes.
    Object,
}

/// What a lookup found.
enum Found<'py> {
    Value(Bound<'py, PyAny>),
    Missing,
    Raised(PyErr),
}

static MAPPING: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static SEQUENCE: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static NUMBER: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static ISROUTINE: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
static FIELDS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
static INTERNALS: PyOnceLock<Py<PyTuple>> = PyOnceLock::new();

/// Read `data` along `paths`, reading at most `max_values` values of it.
///
/// # Errors
///
/// `limit_exceeded` from the core beyond `max_values`, with `row` when it is
/// given (`spec/data-model.md`, *Reading native data*, *Limits*), and failures
/// of the binding itself; exceptions raised by the data become `read_error`
/// nulls.
pub(crate) fn read(
    data: &Bound<'_, PyAny>,
    paths: &Paths,
    max_values: usize,
    row: Option<usize>,
) -> PyResult<Native> {
    let mut reader = Reader {
        objects: Vec::new(),
        tokens: HashMap::new(),
        errors: Vec::new(),
        open: HashSet::new(),
        count: ValueCount::new(max_values),
    };
    match reader.walk(data, paths, &mut Vec::new()) {
        Ok(value) => Ok(Native {
            value,
            objects: reader.objects,
            errors: reader.errors,
        }),
        Err(Stop::Python(error)) => Err(error),
        Err(Stop::Limit(mut error)) => {
            error.context_mut().row = row.and_then(|r| u64::try_from(r).ok());
            Err(errors::to_py(data.py(), &error))
        }
    }
}

/// Why reading stopped: an error of the binding, or the input beyond
/// `max_values`.
pub(crate) enum Stop {
    Python(PyErr),
    Limit(Error),
}

impl From<PyErr> for Stop {
    fn from(error: PyErr) -> Self {
        Self::Python(error)
    }
}

impl From<Error> for Stop {
    fn from(error: Error) -> Self {
        Self::Limit(error)
    }
}

type Read<T = Value> = Result<T, Stop>;

struct Reader {
    objects: Vec<Py<PyAny>>,
    /// The token of each object in `objects`, by identity. Tokens are handed out
    /// in the order objects are met, never from addresses (invariant 7).
    tokens: HashMap<usize, u64>,
    errors: Vec<(Vec<Segment>, PyErr)>,
    /// The containers being converted, by identity, for cycles.
    open: HashSet<usize>,
    /// The values read, against `max_values`.
    count: ValueCount,
}

fn identity(value: &Bound<'_, PyAny>) -> usize {
    value.as_ptr() as usize
}

impl Reader {
    fn walk(&mut self, value: &Bound<'_, PyAny>, paths: &Paths, at: &mut Vec<Segment>) -> Read {
        let array = match convert::numpy_array_as_list(value) {
            Ok(array) => array,
            Err(error) => return self.placed_error(value.py(), error, at),
        };
        let value = array.as_ref().unwrap_or(value);
        if paths.end {
            return self.convert(value, at);
        }
        let shape = match shape(value) {
            Ok(shape) => shape,
            Err(error) => return self.placed_error(value.py(), error, at),
        };
        Ok(match &shape {
            Shape::Scalar => self.convert(value, at)?,
            Shape::Mapping | Shape::Object => {
                let mut map = Map::new();
                for (key, sub) in &paths.keys {
                    at.push(Segment::Key(key.clone()));
                    if let Some(item) = self.child(value, &shape, key, sub, at)? {
                        map.insert(key.clone(), item);
                    }
                    at.pop();
                }
                Value::Map(map)
            }
            Shape::Record { keys, .. } => {
                // The core holds a value for each position, read or not.
                self.count.add(keys.len())?;
                let mut map = Map::record();
                for (position, key) in keys.iter().enumerate() {
                    let by_key = paths.keys.iter().find(|(k, _)| k == key).map(|(_, p)| p);
                    let by_position = paths
                        .indices
                        .iter()
                        .find(|(i, _)| usize::try_from(*i).is_ok_and(|i| i == position))
                        .map(|(_, p)| p);
                    let sub = match (by_key, by_position) {
                        (Some(a), Some(b)) => a.merged(b),
                        (Some(p), None) | (None, Some(p)) => p.clone(),
                        // Unread components keep their position.
                        (None, None) => {
                            map.insert(key.clone(), Value::Null(NullCause::Null));
                            continue;
                        }
                    };
                    at.push(Segment::Key(key.clone()));
                    let item = self
                        .child(value, &shape, key, &sub, at)?
                        .unwrap_or(Value::Null(NullCause::Null));
                    at.pop();
                    map.insert(key.clone(), item);
                }
                Value::Map(map)
            }
            Shape::Sequence(len) => {
                // The list keeps its declared length, each position a value the
                // core holds: counted before it is allocated.
                self.count.add(*len)?;
                let mut items = vec![Value::Null(NullCause::Null); *len];
                for (index, sub) in &paths.indices {
                    let Some(i) = usize::try_from(*index).ok().filter(|i| i < len) else {
                        continue;
                    };
                    at.push(Segment::Index(*index));
                    items[i] = match value.get_item(i) {
                        Ok(item) => self.walk(&item, sub, at)?,
                        Err(error) => self.placed_error(value.py(), error, at)?,
                    };
                    at.pop();
                }
                Value::List(items)
            }
        })
    }

    /// Read the component `key` and walk it; `None` when it is missing.
    fn child(
        &mut self,
        value: &Bound<'_, PyAny>,
        shape: &Shape,
        key: &str,
        paths: &Paths,
        at: &[Segment],
    ) -> Read<Option<Value>> {
        match lookup(value, shape, key) {
            Found::Value(item) => self.walk(&item, paths, &mut at.to_vec()).map(Some),
            Found::Missing => Ok(None),
            Found::Raised(error) => self.placed_error(value.py(), error, at).map(Some),
        }
    }

    /// A null with reason `read_error` placed in the tree: it counts as a value.
    fn placed_error(&mut self, py: Python<'_>, error: PyErr, at: &[Segment]) -> Read {
        self.count.add(1)?;
        Ok(self.raised(py, error, at))
    }

    fn raised(&mut self, py: Python<'_>, error: PyErr, at: &[Segment]) -> Value {
        let name = error
            .get_type(py)
            .name()
            .map_or_else(|_| "Exception".to_owned(), |n| n.to_string());
        let detail = format!("{name}: {}", error.value(py));
        self.errors.push((at.to_vec(), error));
        Value::Null(NullCause::ReadError(detail))
    }

    fn opaque(&mut self, value: &Bound<'_, PyAny>, type_name: String) -> Value {
        let next = self.objects.len() as u64;
        let token = *self.tokens.entry(identity(value)).or_insert_with(|| {
            self.objects.push(value.clone().unbind());
            next
        });
        Value::Opaque(cleanrules::value::Opaque { type_name, token })
    }

    /// The whole value, where a path ends (`spec/data-model.md`, *Python*):
    /// it counts one, and so does everything inside it.
    fn convert(&mut self, value: &Bound<'_, PyAny>, at: &mut Vec<Segment>) -> Read {
        self.count.add(1)?;
        let array = match convert::numpy_array_as_list(value) {
            Ok(array) => array,
            Err(error) => return Ok(self.raised(value.py(), error, at)),
        };
        let value = array.as_ref().unwrap_or(value);
        let shape = match shape(value) {
            Ok(shape) => shape,
            Err(error) => return Ok(self.raised(value.py(), error, at)),
        };
        if matches!(shape, Shape::Scalar) {
            return Ok(self.scalar(value)?);
        }
        let id = identity(value);
        if self.open.len() >= MAX_DEPTH || self.open.contains(&id) {
            return Ok(self.opaque(value, convert::type_name(value)));
        }
        self.open.insert(id);
        let converted = self.convert_container(value, &shape, at);
        self.open.remove(&id);
        converted
    }

    fn convert_container(
        &mut self,
        value: &Bound<'_, PyAny>,
        shape: &Shape,
        at: &mut Vec<Segment>,
    ) -> Read {
        Ok(match shape {
            Shape::Scalar => self.scalar(value)?,
            Shape::Sequence(len) => {
                // Each element counts as it is converted; a declared length
                // beyond what is left is refused before the list is allocated.
                if *len > self.count.remaining() {
                    return Err(too_many_values(self.count.limit()).into());
                }
                let mut items = Vec::with_capacity(*len);
                for i in 0..*len {
                    at.push(Segment::Index(i as u64));
                    items.push(match value.get_item(i) {
                        Ok(item) => self.convert(&item, at)?,
                        Err(error) => self.placed_error(value.py(), error, at)?,
                    });
                    at.pop();
                }
                Value::List(items)
            }
            Shape::Mapping | Shape::Record { .. } | Shape::Object => {
                let keys = match shape {
                    Shape::Record { keys, .. } => keys.clone(),
                    Shape::Mapping => match mapping_keys(value) {
                        Ok(keys) => keys,
                        Err(error) => return Ok(self.raised(value.py(), error, at)),
                    },
                    _ => match object_keys(value) {
                        Ok(Some(keys)) => keys,
                        Ok(None) => return Ok(self.opaque(value, convert::type_name(value))),
                        Err(error) => return Ok(self.raised(value.py(), error, at)),
                    },
                };
                let record = matches!(shape, Shape::Record { .. });
                let mut map = if record { Map::record() } else { Map::new() };
                for key in keys {
                    at.push(Segment::Key(key.clone()));
                    let item = match lookup(value, shape, &key) {
                        Found::Value(item) => Some(self.convert(&item, at)?),
                        Found::Missing if record => {
                            self.count.add(1)?;
                            Some(Value::Null(NullCause::Null))
                        }
                        Found::Missing => None,
                        Found::Raised(error) => Some(self.placed_error(value.py(), error, at)?),
                    };
                    at.pop();
                    if let Some(item) = item {
                        map.insert(key, item);
                    }
                }
                Value::Map(map)
            }
        })
    }

    fn scalar(&mut self, value: &Bound<'_, PyAny>) -> PyResult<Value> {
        Ok(match convert::scalar(value)? {
            Ok(converted) => converted,
            Err(Problem::IntOverflow) => Value::Null(NullCause::IntOverflow),
            Err(Problem::DecimalOverflow) => Value::Null(NullCause::DecimalOverflow),
            Err(Problem::NotANumber) => Value::Null(NullCause::Null),
            // Finer than a microsecond: truncated to the datetime's microseconds.
            Err(Problem::TooPrecise) => convert::truncated(value)?,
            Err(Problem::Opaque(type_name)) => self.opaque(value, type_name),
        })
    }
}

/// The interpreter's own objects, which expose the program through public
/// attributes (`tb_frame.f_globals`, `gi_frame.f_locals`): never read through.
fn internals(py: Python<'_>) -> PyResult<&Bound<'_, PyTuple>> {
    INTERNALS
        .get_or_try_init(py, || {
            let types = py.import("types")?;
            let names = [
                "FrameType",
                "TracebackType",
                "CodeType",
                "GeneratorType",
                "CoroutineType",
                "AsyncGeneratorType",
                "CellType",
            ];
            let classes = names
                .iter()
                .map(|name| types.getattr(*name))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyTuple::new(py, classes)?.unbind())
        })
        .map(|tuple| tuple.bind(py))
}

/// How `value` is read (`spec/data-model.md`, *Python*).
///
/// # Errors
///
/// An exception raised by the value while finding out (a raising `__len__` or
/// `keys()`): the caller makes it a `read_error`.
fn shape(value: &Bound<'_, PyAny>) -> PyResult<Shape> {
    let py = value.py();
    if value.is_none()
        || value.is_instance_of::<PyString>()
        || value.is_instance_of::<PyBytes>()
        || value.is_instance_of::<PyByteArray>()
        || value.is_instance_of::<PyDate>()
        || value.is_instance_of::<PyTime>()
        || value.is_instance_of::<PyDelta>()
        || value.is_instance(NUMBER.import(py, "numbers", "Number")?)?
        || value.is_instance(convert::decimal_class(py)?)?
        || convert::is_numpy_scalar(value)?
        || convert::is_missing_scalar(value)?
        || value.is_instance(internals(py)?)?
        || is_code(value)?
    {
        return Ok(Shape::Scalar);
    }
    if value.is_instance_of::<PyDict>()
        || value.is_instance(MAPPING.import(py, "collections.abc", "Mapping")?)?
    {
        return Ok(Shape::Mapping);
    }
    let class = value.get_type();
    let sequence = value.is_instance_of::<PyList>()
        || value.is_instance_of::<PyTuple>()
        || value.is_instance(SEQUENCE.import(py, "collections.abc", "Sequence")?)?
        || (class.hasattr("__len__")? && class.hasattr("__getitem__")?);
    // A namedtuple-like row: a sequence with `_fields` (namedtuples, SQLAlchemy rows).
    if sequence
        && let Ok(fields) = value.getattr("_fields")
        && let Ok(fields) = fields.extract::<Vec<String>>()
    {
        return Ok(Shape::Record {
            keys: fields,
            by_attribute: true,
        });
    }
    // An object whose type has keys(), such as a pandas row.
    if class.hasattr("keys")? {
        let keys = value
            .call_method0("keys")?
            .try_iter()?
            .map(|k| k.and_then(|k| k.str().map(|s| s.to_string())))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(Shape::Record {
            keys,
            by_attribute: false,
        });
    }
    if sequence {
        return Ok(Shape::Sequence(value.len()?));
    }
    Ok(Shape::Object)
}

/// Read the component `key` of a value of the given shape, by the rules of
/// `spec/data-model.md`: a mapping is never asked for a missing key (so
/// `__missing__` never runs), and an object gives only public data attributes.
/// Only `AttributeError` from an attribute, or `KeyError` from an item, means
/// missing; anything else raised is a `read_error`.
fn lookup<'py>(value: &Bound<'py, PyAny>, shape: &Shape, key: &str) -> Found<'py> {
    let py = value.py();
    let (found, by_attribute) = match shape {
        Shape::Scalar | Shape::Sequence(_) => return Found::Missing,
        Shape::Mapping => match value.contains(key) {
            Ok(false) => return Found::Missing,
            Ok(true) => (value.get_item(key), false),
            Err(error) => (Err(error), false),
        },
        Shape::Record { keys, by_attribute } => {
            if !keys.iter().any(|k| k == key) || (*by_attribute && key.starts_with('_')) {
                return Found::Missing;
            }
            if *by_attribute {
                (value.getattr(key), true)
            } else {
                (value.get_item(key), false)
            }
        }
        Shape::Object => {
            if key.starts_with('_') {
                return Found::Missing;
            }
            match value.getattr(key) {
                Ok(item) => match is_code(&item) {
                    Ok(true) => return Found::Missing,
                    Ok(false) => (Ok(item), true),
                    Err(error) => (Err(error), true),
                },
                Err(error) => (Err(error), true),
            }
        }
    };
    match found {
        Ok(item) => Found::Value(item),
        Err(error) if by_attribute && error.is_instance_of::<PyAttributeError>(py) => {
            Found::Missing
        }
        Err(error) if !by_attribute && error.is_instance_of::<PyKeyError>(py) => Found::Missing,
        Err(error) => Found::Raised(error),
    }
}

/// A method, a function, a class or a module: never data.
fn is_code(value: &Bound<'_, PyAny>) -> PyResult<bool> {
    let py = value.py();
    Ok(value.is_instance_of::<PyType>()
        || value.is_instance_of::<PyModule>()
        || ISROUTINE
            .import(py, "inspect", "isroutine")?
            .call1((value,))?
            .is_truthy()?)
}

/// The string keys of a mapping, in its order; other keys cannot be read.
fn mapping_keys(value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    let mut keys = Vec::new();
    for key in value.call_method0("keys")?.try_iter()? {
        if let Ok(key) = key?.extract::<String>() {
            keys.push(key);
        }
    }
    Ok(keys)
}

/// The public data attributes of an object, when it says which they are:
/// dataclass and attrs fields, Pydantic fields (with extra and computed ones),
/// or its `__dict__`. `None` for an object that does not say: it is opaque.
fn object_keys(value: &Bound<'_, PyAny>) -> PyResult<Option<Vec<String>>> {
    let py = value.py();
    let class = value.get_type();
    let mut keys: Vec<String> = Vec::new();
    let mut push = |key: String| {
        if !key.starts_with('_') && !keys.contains(&key) {
            keys.push(key);
        }
    };
    if class.hasattr("__dataclass_fields__")? {
        for field in FIELDS
            .import(py, "dataclasses", "fields")?
            .call1((value,))?
            .try_iter()?
        {
            push(field?.getattr("name")?.extract()?);
        }
    } else if class.hasattr("__attrs_attrs__")? {
        for field in class.getattr("__attrs_attrs__")?.try_iter()? {
            push(field?.getattr("name")?.extract()?);
        }
    } else if class.hasattr("model_fields")? {
        for name in class
            .getattr("model_fields")?
            .call_method0("keys")?
            .try_iter()?
        {
            push(name?.extract()?);
        }
        if let Ok(extra) = value.getattr("model_extra")
            && !extra.is_none()
        {
            for name in extra.call_method0("keys")?.try_iter()? {
                push(name?.extract()?);
            }
        }
        if let Ok(computed) = class.getattr("model_computed_fields") {
            for name in computed.call_method0("keys")?.try_iter()? {
                push(name?.extract()?);
            }
        }
    } else if let Ok(attributes) = value.getattr("__dict__")
        && let Ok(attributes) = attributes.cast::<PyDict>()
    {
        for name in attributes.keys() {
            if let Ok(name) = name.extract::<String>() {
                push(name);
            }
        }
    } else {
        return Ok(None);
    }
    Ok(Some(keys))
}
