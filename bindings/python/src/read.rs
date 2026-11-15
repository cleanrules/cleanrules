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

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use cleanrules::value::ValueCount;
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
        // Segment by segment, not recursively: a path may be long.
        let mut node = self;
        for segment in path {
            node = match segment {
                Segment::Key(key) => {
                    let at = node
                        .keys
                        .iter()
                        .position(|(k, _)| k == key)
                        .unwrap_or_else(|| {
                            node.keys.push((key.clone(), Paths::default()));
                            node.keys.len() - 1
                        });
                    &mut node.keys[at].1
                }
                Segment::Index(index) => {
                    let at = node
                        .indices
                        .iter()
                        .position(|(i, _)| i == index)
                        .unwrap_or_else(|| {
                            node.indices.push((*index, Paths::default()));
                            node.indices.len() - 1
                        });
                    &mut node.indices[at].1
                }
            };
        }
        node.end = true;
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

/// What reading a value gives: the value, or a container whose elements are
/// read next.
enum Step<'py, 'p> {
    Done(Value),
    Open(Frame<'py, 'p>),
}

/// A container being read, with what is left of it and what it holds so far.
/// The segment of the element being read is on the path `at` until the
/// element is placed.
enum Frame<'py, 'p> {
    /// A map or an object walked along the keys of `paths`.
    WalkKeys {
        value: Bound<'py, PyAny>,
        shape: Shape,
        paths: Cow<'p, Paths>,
        next: usize,
        key: String,
        map: Map,
    },
    /// A record walked along its components, by key or by position.
    WalkRecord {
        value: Bound<'py, PyAny>,
        shape: Shape,
        keys: Vec<String>,
        paths: Cow<'p, Paths>,
        next: usize,
        key: String,
        map: Map,
    },
    /// A sequence walked along the indices of `paths`; `items` has its
    /// declared length.
    WalkList {
        value: Bound<'py, PyAny>,
        paths: Cow<'p, Paths>,
        next: usize,
        slot: usize,
        items: Vec<Value>,
    },
    /// A sequence converted whole.
    ConvertList {
        value: Bound<'py, PyAny>,
        id: usize,
        len: usize,
        items: Vec<Value>,
    },
    /// A mapping, record or object converted whole.
    ConvertMap {
        value: Bound<'py, PyAny>,
        id: usize,
        shape: Shape,
        keys: Vec<String>,
        next: usize,
        key: String,
        map: Map,
    },
}

/// The sub-tree that `pick` chooses in `paths`, borrowed when `paths` is.
fn sub<'p>(
    paths: &Cow<'p, Paths>,
    pick: impl for<'a> Fn(&'a Paths) -> &'a Paths,
) -> Cow<'p, Paths> {
    match paths {
        Cow::Borrowed(paths) => Cow::Borrowed(pick(paths)),
        Cow::Owned(paths) => Cow::Owned(pick(paths).clone()),
    }
}

impl Reader {
    /// Walk `value` along `paths`: depth first with an explicit stack, not
    /// recursion, since the data along a path is as deep as the rule set's
    /// longest path plus what is converted whole where it ends, and a host
    /// thread's stack may be small.
    fn walk(&mut self, value: &Bound<'_, PyAny>, paths: &Paths, at: &mut Vec<Segment>) -> Read {
        let first = self.walk_step(value.clone(), Cow::Borrowed(paths), at)?;
        self.run(first, at)
    }

    fn run<'py, 'p>(&mut self, first: Step<'py, 'p>, at: &mut Vec<Segment>) -> Read {
        let mut stack: Vec<Frame<'py, 'p>> = Vec::new();
        let mut step = first;
        loop {
            match step {
                Step::Open(frame) => stack.push(frame),
                Step::Done(value) => match stack.last_mut() {
                    None => return Ok(value),
                    Some(top) => place(top, value, at),
                },
            }
            // A frame was pushed or given its element, so the stack is not
            // empty; were it, the null below would end the loop.
            let next = match stack.last_mut() {
                Some(top) => self.next(top, at)?,
                None => None,
            };
            step = match next {
                Some(step) => step,
                None => Step::Done(match stack.pop() {
                    Some(done) => self.finish(done),
                    None => Value::Null(NullCause::Null),
                }),
            };
        }
    }

    /// The step of walking `value` along `paths`.
    fn walk_step<'py, 'p>(
        &mut self,
        value: Bound<'py, PyAny>,
        paths: Cow<'p, Paths>,
        at: &[Segment],
    ) -> Read<Step<'py, 'p>> {
        let value = match convert::numpy_array_as_list(&value) {
            Ok(array) => array.unwrap_or(value),
            Err(error) => return Ok(Step::Done(self.placed_error(value.py(), error, at)?)),
        };
        if paths.end {
            return self.convert_step(value, at);
        }
        let shape = match shape(&value) {
            Ok(shape) => shape,
            Err(error) => return Ok(Step::Done(self.placed_error(value.py(), error, at)?)),
        };
        Ok(match shape {
            Shape::Scalar => return self.convert_step(value, at),
            Shape::Mapping | Shape::Object => Step::Open(Frame::WalkKeys {
                value,
                shape,
                paths,
                next: 0,
                key: String::new(),
                map: Map::new(),
            }),
            Shape::Record { ref keys, .. } => {
                // The core holds a value for each position, read or not.
                self.count.add(keys.len())?;
                let keys = keys.clone();
                Step::Open(Frame::WalkRecord {
                    value,
                    shape,
                    keys,
                    paths,
                    next: 0,
                    key: String::new(),
                    map: Map::record(),
                })
            }
            Shape::Sequence(len) => {
                // The list keeps its declared length, each position a value the
                // core holds: counted before it is allocated.
                self.count.add(len)?;
                Step::Open(Frame::WalkList {
                    value,
                    paths,
                    next: 0,
                    slot: 0,
                    items: vec![Value::Null(NullCause::Null); len],
                })
            }
        })
    }

    /// The step of converting the whole value, where a path ends
    /// (`spec/data-model.md`, *Python*): it counts one, and so does everything
    /// inside it.
    fn convert_step<'py, 'p>(
        &mut self,
        value: Bound<'py, PyAny>,
        at: &[Segment],
    ) -> Read<Step<'py, 'p>> {
        self.count.add(1)?;
        let value = match convert::numpy_array_as_list(&value) {
            Ok(array) => array.unwrap_or(value),
            Err(error) => return Ok(Step::Done(self.raised(value.py(), error, at))),
        };
        let shape = match shape(&value) {
            Ok(shape) => shape,
            Err(error) => return Ok(Step::Done(self.raised(value.py(), error, at))),
        };
        if matches!(shape, Shape::Scalar) {
            return Ok(Step::Done(self.scalar(&value)?));
        }
        let id = identity(&value);
        if self.open.len() >= MAX_DEPTH || self.open.contains(&id) {
            let name = convert::type_name(&value);
            return Ok(Step::Done(self.opaque(&value, name)));
        }
        let keys = match &shape {
            Shape::Sequence(len) => {
                // Each element counts as it is converted; a declared length
                // beyond what is left is refused before the list is allocated.
                if *len > self.count.remaining() {
                    return Err(self.count.beyond().into());
                }
                self.open.insert(id);
                return Ok(Step::Open(Frame::ConvertList {
                    value,
                    id,
                    len: *len,
                    items: Vec::with_capacity(*len),
                }));
            }
            Shape::Record { keys, .. } => keys.clone(),
            Shape::Mapping => match mapping_keys(&value) {
                Ok(keys) => keys,
                Err(error) => return Ok(Step::Done(self.raised(value.py(), error, at))),
            },
            _ => match object_keys(&value) {
                Ok(Some(keys)) => keys,
                Ok(None) => {
                    let name = convert::type_name(&value);
                    return Ok(Step::Done(self.opaque(&value, name)));
                }
                Err(error) => return Ok(Step::Done(self.raised(value.py(), error, at))),
            },
        };
        let map = if matches!(shape, Shape::Record { .. }) {
            Map::record()
        } else {
            Map::new()
        };
        self.open.insert(id);
        Ok(Step::Open(Frame::ConvertMap {
            value,
            id,
            shape,
            keys,
            next: 0,
            key: String::new(),
            map,
        }))
    }

    /// The step of `frame`'s next element, its segment pushed onto `at`; `None`
    /// once every element is read. Elements read at once (missing, raised)
    /// are placed here.
    #[allow(clippy::too_many_lines)] // One arm per kind of container.
    fn next<'py, 'p>(
        &mut self,
        frame: &mut Frame<'py, 'p>,
        at: &mut Vec<Segment>,
    ) -> Read<Option<Step<'py, 'p>>> {
        match frame {
            Frame::WalkKeys {
                value,
                shape,
                paths,
                next,
                key,
                map,
            } => {
                while *next < paths.keys.len() {
                    let i = *next;
                    *next += 1;
                    let name = paths.keys[i].0.clone();
                    at.push(Segment::Key(name.clone()));
                    match lookup(value, shape, &name) {
                        Found::Value(item) => {
                            *key = name;
                            let sub = sub(paths, |p| &p.keys[i].1);
                            return self.walk_step(item, sub, at).map(Some);
                        }
                        Found::Missing => {
                            at.pop();
                        }
                        Found::Raised(error) => {
                            let item = self.placed_error(value.py(), error, at)?;
                            at.pop();
                            map.insert(name, item);
                        }
                    }
                }
                Ok(None)
            }
            Frame::WalkRecord {
                value,
                shape,
                keys,
                paths,
                next,
                key,
                map,
            } => {
                while *next < keys.len() {
                    let position = *next;
                    *next += 1;
                    let name = keys[position].clone();
                    let by_key = paths.keys.iter().position(|(k, _)| *k == name);
                    let by_position = paths
                        .indices
                        .iter()
                        .position(|(i, _)| usize::try_from(*i).is_ok_and(|i| i == position));
                    let sub = match (by_key, by_position) {
                        (Some(a), Some(b)) => {
                            Cow::Owned(paths.keys[a].1.merged(&paths.indices[b].1))
                        }
                        (Some(a), None) => sub(paths, |p| &p.keys[a].1),
                        (None, Some(b)) => sub(paths, |p| &p.indices[b].1),
                        // Unread components keep their position.
                        (None, None) => {
                            map.insert(name, Value::Null(NullCause::Null));
                            continue;
                        }
                    };
                    at.push(Segment::Key(name.clone()));
                    match lookup(value, shape, &name) {
                        Found::Value(item) => {
                            *key = name;
                            return self.walk_step(item, sub, at).map(Some);
                        }
                        Found::Missing => {
                            at.pop();
                            map.insert(name, Value::Null(NullCause::Null));
                        }
                        Found::Raised(error) => {
                            let item = self.placed_error(value.py(), error, at)?;
                            at.pop();
                            map.insert(name, item);
                        }
                    }
                }
                Ok(None)
            }
            Frame::WalkList {
                value,
                paths,
                next,
                slot,
                items,
            } => {
                while *next < paths.indices.len() {
                    let at_index = *next;
                    *next += 1;
                    let index = paths.indices[at_index].0;
                    let Some(i) = usize::try_from(index).ok().filter(|i| *i < items.len()) else {
                        continue;
                    };
                    at.push(Segment::Index(index));
                    match value.get_item(i) {
                        Ok(item) => {
                            *slot = i;
                            let sub = sub(paths, |p| &p.indices[at_index].1);
                            return self.walk_step(item, sub, at).map(Some);
                        }
                        Err(error) => {
                            items[i] = self.placed_error(value.py(), error, at)?;
                            at.pop();
                        }
                    }
                }
                Ok(None)
            }
            Frame::ConvertList {
                value, len, items, ..
            } => {
                while items.len() < *len {
                    let i = items.len();
                    at.push(Segment::Index(i as u64));
                    match value.get_item(i) {
                        Ok(item) => return self.convert_step(item, at).map(Some),
                        Err(error) => {
                            let item = self.placed_error(value.py(), error, at)?;
                            at.pop();
                            items.push(item);
                        }
                    }
                }
                Ok(None)
            }
            Frame::ConvertMap {
                value,
                shape,
                keys,
                next,
                key,
                map,
                ..
            } => {
                let record = matches!(shape, Shape::Record { .. });
                while *next < keys.len() {
                    let name = keys[*next].clone();
                    *next += 1;
                    at.push(Segment::Key(name.clone()));
                    match lookup(value, shape, &name) {
                        Found::Value(item) => {
                            *key = name;
                            return self.convert_step(item, at).map(Some);
                        }
                        Found::Missing if record => {
                            self.count.add(1)?;
                            at.pop();
                            map.insert(name, Value::Null(NullCause::Null));
                        }
                        Found::Missing => {
                            at.pop();
                        }
                        Found::Raised(error) => {
                            let item = self.placed_error(value.py(), error, at)?;
                            at.pop();
                            map.insert(name, item);
                        }
                    }
                }
                Ok(None)
            }
        }
    }

    /// The value of a container whose elements are all read.
    fn finish(&mut self, frame: Frame<'_, '_>) -> Value {
        match frame {
            Frame::WalkKeys { map, .. } | Frame::WalkRecord { map, .. } => Value::Map(map),
            Frame::WalkList { items, .. } => Value::List(items),
            Frame::ConvertList { id, items, .. } => {
                self.open.remove(&id);
                Value::List(items)
            }
            Frame::ConvertMap { id, map, .. } => {
                self.open.remove(&id);
                Value::Map(map)
            }
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

/// Keep the element just read in `frame`, and take its segment off `at`.
fn place(frame: &mut Frame<'_, '_>, value: Value, at: &mut Vec<Segment>) {
    at.pop();
    match frame {
        Frame::WalkKeys { key, map, .. }
        | Frame::WalkRecord { key, map, .. }
        | Frame::ConvertMap { key, map, .. } => map.insert(std::mem::take(key), value),
        Frame::WalkList { slot, items, .. } => items[*slot] = value,
        Frame::ConvertList { items, .. } => items.push(value),
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
