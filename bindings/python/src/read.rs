//! Reading native Python data (`spec/data-model.md`, *Reading native data*).
//!
//! The binding reads only the paths a rule set reads, walking the native
//! structure segment by segment, each native object once, and hands the core a
//! sparse value tree. It reads public data only: map keys, record components
//! and objects' public data attributes, never a private name, a method, a class
//! or a module. A getter that raises gives a null with reason `read_error`,
//! except `AttributeError`, which means missing; the exception is kept so that
//! strict mode can chain it.

use cleanrules::{Map, NullCause, Segment, Value};
use pyo3::exceptions::PyAttributeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{
    PyByteArray, PyBytes, PyDate, PyDelta, PyDict, PyList, PyModule, PyString, PyTime, PyTuple,
    PyType,
};

use crate::convert::{self, MAX_DEPTH, Problem};

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
    /// A scalar, a class or a module: nothing is read through it.
    Scalar,
    /// A mapping: string segments read keys.
    Mapping,
    /// A record: its components, by name and by position.
    Record(Vec<String>),
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

/// Read `data` along `paths`.
///
/// # Errors
///
/// Only failures of the binding itself; exceptions raised by the data become
/// `read_error` nulls.
pub(crate) fn read(data: &Bound<'_, PyAny>, paths: &Paths) -> PyResult<Native> {
    let mut reader = Reader::default();
    let value = reader.walk(data, paths, &mut Vec::new())?;
    Ok(Native {
        value,
        objects: reader.objects,
        errors: reader.errors,
    })
}

#[derive(Default)]
struct Reader {
    objects: Vec<Py<PyAny>>,
    /// The identity of each object in `objects`, so one object has one token.
    ids: Vec<usize>,
    errors: Vec<(Vec<Segment>, PyErr)>,
    /// The containers being converted, by identity, for cycles and depth.
    open: Vec<usize>,
}

fn identity(value: &Bound<'_, PyAny>) -> usize {
    value.as_ptr() as usize
}

impl Reader {
    fn walk(
        &mut self,
        value: &Bound<'_, PyAny>,
        paths: &Paths,
        at: &mut Vec<Segment>,
    ) -> PyResult<Value> {
        if paths.end {
            return self.convert(value, at);
        }
        Ok(match shape(value)? {
            Shape::Scalar => self.convert(value, at)?,
            Shape::Mapping | Shape::Object => {
                let mut map = Map::new();
                for (key, sub) in &paths.keys {
                    at.push(Segment::Key(key.clone()));
                    if let Some(item) = self.child(value, key, sub, at)? {
                        map.insert(key.clone(), item);
                    }
                    at.pop();
                }
                Value::Map(map)
            }
            Shape::Record(keys) => {
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
                        .child(value, key, &sub, at)?
                        .unwrap_or(Value::Null(NullCause::Null));
                    at.pop();
                    map.insert(key.clone(), item);
                }
                Value::Map(map)
            }
            Shape::Sequence(len) => {
                // Holes stand for elements no path reads; none is read past the last.
                let last = paths
                    .indices
                    .iter()
                    .filter_map(|(i, _)| usize::try_from(*i).ok())
                    .filter(|i| *i < len)
                    .max();
                let mut items = vec![Value::Null(NullCause::Null); last.map_or(0, |i| i + 1)];
                for (index, sub) in &paths.indices {
                    let Some(i) = usize::try_from(*index).ok().filter(|i| *i < len) else {
                        continue;
                    };
                    at.push(Segment::Index(*index));
                    items[i] = match value.get_item(i) {
                        Ok(item) => self.walk(&item, sub, at)?,
                        Err(error) => self.raised(value.py(), error, at),
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
        key: &str,
        paths: &Paths,
        at: &[Segment],
    ) -> PyResult<Option<Value>> {
        match lookup(value, key)? {
            Found::Value(item) => self.walk(&item, paths, &mut at.to_vec()).map(Some),
            Found::Missing => Ok(None),
            Found::Raised(error) => Ok(Some(self.raised(value.py(), error, at))),
        }
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
        let id = identity(value);
        let token = self.ids.iter().position(|&i| i == id).unwrap_or_else(|| {
            self.ids.push(id);
            self.objects.push(value.clone().unbind());
            self.ids.len() - 1
        });
        Value::Opaque(cleanrules::value::Opaque {
            type_name,
            token: token as u64,
        })
    }

    /// The whole value, where a path ends (`spec/data-model.md`, *Python*).
    fn convert(&mut self, value: &Bound<'_, PyAny>, at: &mut Vec<Segment>) -> PyResult<Value> {
        let shape = shape(value)?;
        if matches!(shape, Shape::Scalar) {
            return self.scalar(value);
        }
        let id = identity(value);
        if self.open.len() >= MAX_DEPTH || self.open.contains(&id) {
            return Ok(self.opaque(value, convert::type_name(value)));
        }
        self.open.push(id);
        let converted = match shape {
            Shape::Scalar => unreachable!("handled above"),
            Shape::Sequence(len) => {
                let mut items = Vec::with_capacity(len);
                for i in 0..len {
                    at.push(Segment::Index(i as u64));
                    items.push(match value.get_item(i) {
                        Ok(item) => self.convert(&item, at)?,
                        Err(error) => self.raised(value.py(), error, at),
                    });
                    at.pop();
                }
                Value::List(items)
            }
            Shape::Mapping | Shape::Record(_) | Shape::Object => {
                let (keys, record) = match &shape {
                    Shape::Record(keys) => (keys.clone(), true),
                    Shape::Mapping => (mapping_keys(value)?, false),
                    _ => {
                        let Some(keys) = object_keys(value)? else {
                            self.open.pop();
                            return Ok(self.opaque(value, convert::type_name(value)));
                        };
                        (keys, false)
                    }
                };
                let mut map = if record { Map::record() } else { Map::new() };
                for key in keys {
                    at.push(Segment::Key(key.clone()));
                    let item = match lookup(value, &key)? {
                        Found::Value(item) => Some(self.convert(&item, at)?),
                        Found::Missing if record => Some(Value::Null(NullCause::Null)),
                        Found::Missing => None,
                        Found::Raised(error) => Some(self.raised(value.py(), error, at)),
                    };
                    at.pop();
                    if let Some(item) = item {
                        map.insert(key, item);
                    }
                }
                Value::Map(map)
            }
        };
        self.open.pop();
        Ok(converted)
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

/// How `value` is read (`spec/data-model.md`, *Python*).
fn shape(value: &Bound<'_, PyAny>) -> PyResult<Shape> {
    let py = value.py();
    if value.is_none()
        || value.is_instance_of::<PyString>()
        || value.is_instance_of::<PyBytes>()
        || value.is_instance_of::<PyByteArray>()
        || value.is_instance_of::<PyDate>()
        || value.is_instance_of::<PyTime>()
        || value.is_instance_of::<PyDelta>()
        || value.is_instance_of::<PyType>()
        || value.is_instance_of::<PyModule>()
        || value.is_instance(NUMBER.import(py, "numbers", "Number")?)?
        || value.is_instance(convert::decimal_class(py)?)?
        || convert::is_numpy_scalar(value)?
    {
        return Ok(Shape::Scalar);
    }
    if value.is_instance_of::<PyDict>()
        || value.is_instance(MAPPING.import(py, "collections.abc", "Mapping")?)?
    {
        return Ok(Shape::Mapping);
    }
    // A namedtuple-like record: a sequence with `_fields`.
    if value.is_instance_of::<PyTuple>()
        && let Ok(fields) = value.getattr("_fields")
        && let Ok(fields) = fields.extract::<Vec<String>>()
    {
        return Ok(Shape::Record(fields));
    }
    // An object whose type has keys(), such as a pandas row.
    if value.get_type().hasattr("keys")?
        && let Ok(keys) = value.call_method0("keys")
        && let Ok(keys) = keys.try_iter().and_then(|keys| {
            keys.map(|k| k.and_then(|k| k.str().map(|s| s.to_string())))
                .collect::<PyResult<Vec<_>>>()
        })
    {
        return Ok(Shape::Record(keys));
    }
    if value.is_instance_of::<PyList>()
        || value.is_instance_of::<PyTuple>()
        || value.is_instance(SEQUENCE.import(py, "collections.abc", "Sequence")?)?
        || (value.get_type().hasattr("__len__")? && value.get_type().hasattr("__getitem__")?)
    {
        return Ok(Shape::Sequence(value.len()?));
    }
    Ok(Shape::Object)
}

/// Read the component `key` of a mapping, a record or an object, by the rules
/// of `spec/data-model.md`: a mapping is never asked for a missing key (so
/// `__missing__` never runs), and an object gives only public data attributes.
fn lookup<'py>(value: &Bound<'py, PyAny>, key: &str) -> PyResult<Found<'py>> {
    let found = match shape(value)? {
        Shape::Scalar | Shape::Sequence(_) => return Ok(Found::Missing),
        Shape::Mapping => match value.contains(key) {
            Ok(false) => return Ok(Found::Missing),
            Ok(true) => value.get_item(key),
            Err(error) => Err(error),
        },
        Shape::Record(keys) => {
            if !keys.iter().any(|k| k == key) {
                return Ok(Found::Missing);
            }
            // A namedtuple by attribute; a keys() object by item.
            if value.is_instance_of::<PyTuple>() {
                value.getattr(key)
            } else {
                value.get_item(key)
            }
        }
        Shape::Object => {
            if key.starts_with('_') {
                return Ok(Found::Missing);
            }
            match value.getattr(key) {
                Ok(item) if is_code(&item)? => return Ok(Found::Missing),
                other => other,
            }
        }
    };
    Ok(match found {
        Ok(item) => Found::Value(item),
        Err(error) if error.is_instance_of::<PyAttributeError>(value.py()) => Found::Missing,
        // A missing key in a mapping that raised KeyError anyway is missing too.
        Err(error) if error.is_instance_of::<pyo3::exceptions::PyKeyError>(value.py()) => {
            Found::Missing
        }
        Err(error) => Found::Raised(error),
    })
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
