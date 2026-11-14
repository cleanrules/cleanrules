//! Errors at the boundary: the core's errors as `{error, Map}`, panics caught
//! and returned the same way, and arguments of the wrong kind as `badarg`
//! (`spec/errors.md`, *In each language*).
//!
//! The map has the atom keys of the error's JSON form (`spec/errors.md`, *JSON
//! form*), each only when it applies: `code` (an atom), `message`, `hint`,
//! `pointer`, `line`, `column`, `rule`, `rules`, `path`, `types`, `row`,
//! `name`, `available`, `executed`, `node` (its JSON form as a term, as
//! `out::json_term` makes it), `operation`,
//! `reason` and `cause`. Texts are binaries. A panic is `internal_error`, as in
//! the C ABI: a bug in cleanrules, never a mistake of the caller.

use cleanrules::format::write_node;
use cleanrules::{Error, Segment};
use rustler::{Encoder, Env, NifResult, Term};

use crate::atoms;
use crate::out::json_term;
use crate::panics::catch;
pub(crate) use crate::panics::{Failure, Outcome};

impl From<rustler::Error> for Failure {
    fn from(_: rustler::Error) -> Self {
        Self::BadArg
    }
}

/// Run the body of a NIF: its term, `{error, Map}` for an error of the core
/// or a panic, or the `badarg` exception.
pub(crate) fn guard<'a>(
    env: Env<'a>,
    body: impl FnOnce() -> Outcome<Term<'a>>,
) -> NifResult<Term<'a>> {
    match catch(body) {
        Ok(term) => Ok(term),
        Err(Failure::BadArg) => Err(rustler::Error::BadArg),
        Err(Failure::Core(error)) => Ok((atoms::error(), error_term(env, &error)).encode(env)),
        Err(Failure::Internal(message)) => {
            Ok((atoms::error(), internal(env, &message)).encode(env))
        }
    }
}

/// `{ok, Term}`.
pub(crate) fn ok<'a>(env: Env<'a>, term: Term<'a>) -> Term<'a> {
    (atoms::ok(), term).encode(env)
}

fn internal<'a>(env: Env<'a>, message: &str) -> Term<'a> {
    let mut map = Builder::new(env);
    map.put("code", atoms::internal_error().encode(env));
    map.put("message", message.encode(env));
    map.finish()
}

/// A map built entry by entry, with atom keys from a fixed set.
struct Builder<'a> {
    env: Env<'a>,
    keys: Vec<Term<'a>>,
    values: Vec<Term<'a>>,
}

impl<'a> Builder<'a> {
    fn new(env: Env<'a>) -> Self {
        Self {
            env,
            keys: Vec::new(),
            values: Vec::new(),
        }
    }

    fn put(&mut self, key: &'static str, value: Term<'a>) {
        self.keys.push(atoms::key(self.env, key));
        self.values.push(value);
    }

    fn finish(self) -> Term<'a> {
        // The keys are distinct: each is put once.
        Term::map_from_term_arrays(self.env, &self.keys, &self.values)
            .unwrap_or_else(|_| Term::map_new(self.env))
    }
}

fn strings<'a, 's>(env: Env<'a>, items: impl IntoIterator<Item = &'s str>) -> Term<'a> {
    items.into_iter().collect::<Vec<&str>>().encode(env)
}

/// A field path: binaries for keys, integers for indices.
pub(crate) fn path_term<'a>(env: Env<'a>, path: &[Segment]) -> Term<'a> {
    path.iter()
        .map(|segment| match segment {
            Segment::Key(key) => key.encode(env),
            Segment::Index(index) => index.encode(env),
        })
        .collect::<Vec<Term<'a>>>()
        .encode(env)
}

/// The map of a core error (module documentation).
pub(crate) fn error_term<'a>(env: Env<'a>, error: &Error) -> Term<'a> {
    let mut map = Builder::new(env);
    map.put("code", atoms::key(env, error.code().as_str()));
    map.put("message", error.message().encode(env));
    if let Some(hint) = error.hint() {
        map.put("hint", hint.encode(env));
    }
    let context = error.context();
    if let Some(pointer) = &context.pointer {
        map.put("pointer", pointer.encode(env));
    }
    if let Some(line) = context.line {
        map.put("line", line.encode(env));
    }
    if let Some(column) = context.column {
        map.put("column", column.encode(env));
    }
    if let Some(rule) = &context.rule {
        map.put("rule", rule.encode(env));
    }
    if !context.rules.is_empty() {
        map.put(
            "rules",
            strings(env, context.rules.iter().map(String::as_str)),
        );
    }
    if let Some(path) = &context.path {
        map.put("path", path_term(env, path));
    }
    if !context.types.is_empty() {
        map.put(
            "types",
            strings(env, context.types.iter().map(|g| g.as_str())),
        );
    }
    if let Some(row) = context.row {
        map.put("row", row.encode(env));
    }
    if let Some(name) = &context.name {
        map.put("name", name.encode(env));
    }
    if !context.available.is_empty() {
        map.put(
            "available",
            strings(env, context.available.iter().map(String::as_str)),
        );
    }
    if !context.executed.is_empty() {
        map.put(
            "executed",
            strings(env, context.executed.iter().map(String::as_str)),
        );
    }
    if let Some(node) = &context.node {
        map.put("node", json_term(env, &write_node(node)));
    }
    if let Some(operation) = &context.operation {
        map.put("operation", operation.encode(env));
    }
    if let Some(reason) = &context.reason {
        map.put("reason", reason.encode(env));
    }
    if let Some(cause) = std::error::Error::source(error) {
        map.put("cause", cause.to_string().encode(env));
    }
    map.finish()
}
