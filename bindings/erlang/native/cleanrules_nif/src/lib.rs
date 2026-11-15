//! The NIF of cleanrules's Erlang binding (`rustler`), loaded by the internal
//! module `cleanrules_nif` of the OTP application `cleanrules`.
//!
//! It is a thin layer over the core (`CLAUDE.md`, invariant 2): it reads
//! Erlang terms as values (`terms`, `spec/data-model.md`, *Erlang*), hands
//! them to the core, and gives back what the core decides as terms (`out`),
//! with the core's errors as `{error, Map}` (`errors`). It never evaluates,
//! explains or decides anything itself.
//!
//! A rule set is an immutable resource, shared by every process that holds it.
//! One input is evaluated on the normal scheduler while it stays within a
//! budget of work, and on a dirty CPU scheduler beyond it; batches,
//! explanations, documents and conversions always run on dirty CPU
//! schedulers, so that no call keeps a normal scheduler for much more than a
//! millisecond. Panics are caught and returned as `{error, Map}` with the code
//! `internal_error`.

mod bignum;
mod errors;
mod out;
mod panics;
mod paths;
mod terms;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cleanrules::explain::{explain, summary};
use cleanrules::format::{
    Document, DocumentFormat, ValueContext, explanation_value, read_decision_document_as,
    read_document_as, read_value, write_decision_as, write_document_as, write_explanation_as,
    write_path, write_ruleset, write_summary, write_summary_as, write_value,
};
use cleanrules::json::{Limits, parse_bytes, to_string};
use cleanrules::rules::RuleSet;
use cleanrules::{Error, ErrorCode, NullCause, Segment, Value};
use rustler::types::map::MapIterator;
use rustler::{Binary, Encoder, Env, NifResult, ResourceArc, Term, TermType};

use crate::errors::{Failure, Outcome, guard, ok, path_term};
use crate::out::{decision_term, json_term, map_term, value_term};
use crate::paths::Paths;
use crate::terms::{Budget, Reader, Stop, Tree};

/// The atoms of the binding, made once when the library is loaded. Every
/// other atom it makes comes from a fixed set of names (error codes, map keys,
/// policies): data never creates an atom (`spec/data-model.md`, *Erlang*).
pub(crate) mod atoms {
    use rustler::{Atom, Encoder, Env, Term};

    rustler::atoms! {
        ok,
        error,
        null,
        undefined,
        nil,
        true_ = "true",
        false_ = "false",
        unknown,
        dirty,
        internal_error,
        cleanrules_float,
        cleanrules_decimal,
        cleanrules_datetime,
        rule,
        results,
        fired,
        truth,
        output,
        selected,
        aggregated,
        __struct__,
        __exception__,
        calendar,
        year,
        month,
        day,
        hour,
        minute,
        second,
        microsecond,
        time_zone,
        utc_offset,
        std_offset,
        sign,
        coef,
        exp,
        inf,
        nan = "NaN",
        qnan = "qNaN",
        snan = "sNaN",
        elixir_decimal = "Elixir.Decimal",
        elixir_date = "Elixir.Date",
        elixir_naive_datetime = "Elixir.NaiveDateTime",
        elixir_datetime = "Elixir.DateTime",
        elixir_calendar_iso = "Elixir.Calendar.ISO",
        elixir_map_set = "Elixir.MapSet",
        elixir_range = "Elixir.Range",
        elixir_regex = "Elixir.Regex",
        elixir_time = "Elixir.Time",
        elixir_duration = "Elixir.Duration",
    }

    /// The atom of a name from a fixed set (a static string: an error code, a
    /// map key, a policy), never of data.
    pub(crate) fn key<'a>(env: Env<'a>, name: &'static str) -> Term<'a> {
        Atom::from_str(env, name).map_or_else(|_| name.encode(env), |atom| atom.encode(env))
    }
}

/// A rule set, immutable and shared by every process that holds it.
pub struct RuleSetResource {
    ruleset: RuleSet,
    /// The paths its conditions and payloads read: the only ones read in data.
    paths: Paths,
    /// The size of its JSON form, in bytes: how much evaluating it costs.
    cost: usize,
    /// Set when a call on the normal scheduler took longer than
    /// [`NORMAL_TIME`]: its next calls go to a dirty scheduler at once.
    prefer_dirty: AtomicBool,
}

#[rustler::resource_impl]
impl rustler::Resource for RuleSetResource {}

impl RuleSetResource {
    fn new(ruleset: RuleSet) -> Self {
        let paths = Paths::new(summary(&ruleset).fields.iter().map(|f| f.path.as_slice()));
        let cost = to_string(&write_ruleset(&ruleset)).len();
        Self {
            ruleset,
            paths,
            cost,
            prefer_dirty: AtomicBool::new(false),
        }
    }
}

/// The work one call on the normal scheduler may do (`terms::Budget`): the
/// worst case measured, a payload that copies a list of 4,900 integers (read,
/// evaluated and given back twice), takes 0.37 ms in a release build on an
/// Apple M-series CPU, and 1.9 ms on the same machine under heavy load
/// (`README.md`, *Schedulers*).
const NORMAL_UNITS: usize = 10_000;

/// The largest rule set evaluated on the normal scheduler, by the size of its
/// JSON form: 0.1 ms with a small input at 61 KiB, measured as above (0.6 ms
/// under heavy load at 94 KiB).
const NORMAL_COST: usize = 64 * 1024;

/// A call on the normal scheduler that takes longer than this sends the rule
/// set's next calls to a dirty scheduler: the budget is an estimate, and some
/// rule sets cost more per value read (regular expressions over long texts).
const NORMAL_TIME: Duration = Duration::from_millis(1);

/// The largest `max_depth` and `max_nesting` a caller may give: a dirty
/// scheduler's stack is 320 KiB by default, and in a release build reading,
/// evaluating, explaining, writing and freeing a document fails beyond about
/// 5,500 levels of either (measured on aarch64 macOS; `README.md`,
/// *Schedulers*). Debug builds fail beyond about 1,700.
const MAX_SAFE_DEPTH: usize = 2048;

// --- Arguments -------------------------------------------------------------

/// A non-negative integer that fits in a `usize`.
fn size_arg(term: Term<'_>) -> Outcome<usize> {
    let size: u64 = term.decode()?;
    usize::try_from(size).map_err(|_| Failure::BadArg)
}

/// A format by its atom: `json`, `yaml`, `text` or `csv`.
fn format_arg(term: Term<'_>) -> Outcome<DocumentFormat> {
    if !term.is_atom() {
        return Err(Failure::BadArg);
    }
    DocumentFormat::from_name(&term.atom_to_string()?).ok_or(Failure::BadArg)
}

/// Limits from a map with any of the atom keys `max_size`, `max_nesting`,
/// `max_depth`, `max_table_size`, `max_yaml_size`, `max_text_size`,
/// `max_values` and `max_path_segments`; `max_size` sets the largest text of
/// every format first. Paths have no cap of their own: the NIF reads, builds
/// and frees the data along them with explicit stacks.
/// `max_nesting` and `max_depth` above [`MAX_SAFE_DEPTH`] are refused.
fn limits_arg(term: Term<'_>) -> Outcome<Limits> {
    let mut given: Vec<(String, usize)> = Vec::new();
    for (key, value) in MapIterator::new(term).ok_or(Failure::BadArg)? {
        if !key.is_atom() {
            return Err(Failure::BadArg);
        }
        given.push((key.atom_to_string()?, size_arg(value)?));
    }
    let mut limits = Limits::default();
    if let Some((_, size)) = given.iter().find(|(name, _)| name == "max_size") {
        limits = limits.with_max_size(*size);
    }
    for (name, value) in given {
        match name.as_str() {
            "max_size" => {}
            "max_nesting" | "max_depth" if value > MAX_SAFE_DEPTH => return Err(Failure::BadArg),
            "max_nesting" => limits.max_nesting = value,
            "max_depth" => limits.max_depth = value,
            "max_table_size" => limits.max_table_size = value,
            "max_yaml_size" => limits.max_yaml_size = value,
            "max_text_size" => limits.max_text_size = value,
            "max_values" => limits.max_values = value,
            "max_path_segments" => limits.max_path_segments = value,
            _ => return Err(Failure::BadArg),
        }
    }
    Ok(limits)
}

fn binary_arg(term: Term<'_>) -> Outcome<Binary<'_>> {
    Ok(Binary::from_term(term)?)
}

// --- Information -----------------------------------------------------------

/// The version of the library, as a binary.
#[rustler::nif]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The version of the JSON format it reads and writes.
#[rustler::nif]
fn format_version() -> u32 {
    cleanrules::FORMAT_VERSION
}

/// The default limits, as the map `limits_arg` reads.
#[rustler::nif]
fn default_limits(env: Env<'_>) -> Term<'_> {
    let limits = Limits::default();
    let pairs = [
        ("max_size", limits.max_size),
        ("max_nesting", limits.max_nesting),
        ("max_depth", limits.max_depth),
        ("max_table_size", limits.max_table_size),
        ("max_yaml_size", limits.max_yaml_size),
        ("max_text_size", limits.max_text_size),
        ("max_values", limits.max_values),
        ("max_path_segments", limits.max_path_segments),
    ];
    let keys: Vec<Term<'_>> = pairs.iter().map(|(k, _)| atoms::key(env, k)).collect();
    let values: Vec<Term<'_>> = pairs.iter().map(|(_, v)| v.encode(env)).collect();
    map_term(env, &keys, &values)
}

/// The parent of an error code in the hierarchy (`spec/errors.md`), as the
/// core gives it: `format_error` for `limit_exceeded`, `evaluation_error` for
/// `missing_field`, and `undefined` for a code directly under the base and for
/// `internal_error`, which is outside the hierarchy.
#[rustler::nif]
fn error_parent<'a>(env: Env<'a>, code: Term<'a>) -> NifResult<Term<'a>> {
    guard(env, || {
        if !code.is_atom() {
            return Err(Failure::BadArg);
        }
        let name = code.atom_to_string()?;
        if name == "internal_error" {
            return Ok(atoms::undefined().encode(env));
        }
        let code = ErrorCode::ALL
            .into_iter()
            .find(|c| c.as_str() == name)
            .ok_or(Failure::BadArg)?;
        Ok(code.parent().map_or_else(
            || atoms::undefined().encode(env),
            |parent| atoms::key(env, parent.as_str()),
        ))
    })
}

/// Panic on purpose: the tests check with it that a panic comes back as
/// `{error, #{code => internal_error}}`, in every build that is shipped.
#[rustler::nif]
fn test_panic(env: Env<'_>) -> NifResult<Term<'_>> {
    #[allow(clippy::panic)]
    guard(env, || panic!("test_panic was called"))
}

// --- Rule sets -------------------------------------------------------------

/// Read a rule set from a document: a rule set, a rule, or a decision table
/// (`spec/json-format.md`, *A rule set from a document*).
#[rustler::nif(schedule = "DirtyCpu")]
fn ruleset_read<'a>(
    env: Env<'a>,
    text: Term<'a>,
    format: Term<'a>,
    limits: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let text = binary_arg(text)?;
        let format = format_arg(format)?;
        let limits = limits_arg(limits)?;
        let ruleset = read_document_as(text.as_slice(), format, &limits)?.into_ruleset()?;
        Ok(ok(
            env,
            ResourceArc::new(RuleSetResource::new(ruleset)).encode(env),
        ))
    })
}

/// Write a rule set as a document.
#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn ruleset_write<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    format: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let format = format_arg(format)?;
        let text = write_document_as(&Document::RuleSet(ruleset.ruleset.clone()), format)?;
        Ok(ok(env, text.encode(env)))
    })
}

/// What a rule set is: `#{hit_policy, aggregate, logic, strict, rules,
/// field_paths}`, the policies as atoms, `aggregate` `null` when there is
/// none, `rules` the names in order and `field_paths` the paths read, in order
/// of first appearance.
#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn ruleset_info(env: Env<'_>, ruleset: ResourceArc<RuleSetResource>) -> NifResult<Term<'_>> {
    guard(env, || {
        let rs = &ruleset.ruleset;
        let names: Vec<&str> = rs
            .rules()
            .iter()
            .map(cleanrules::rules::Rule::name)
            .collect();
        let field_paths: Vec<Term<'_>> = summary(rs)
            .fields
            .iter()
            .map(|f| path_term(env, &f.path))
            .collect();
        let keys = [
            "hit_policy",
            "aggregate",
            "logic",
            "strict",
            "rules",
            "field_paths",
        ]
        .map(|k| atoms::key(env, k));
        let values = [
            atoms::key(env, rs.hit_policy().as_str()),
            rs.aggregate().map_or_else(
                || atoms::null().encode(env),
                |a| atoms::key(env, a.as_str()),
            ),
            atoms::key(env, rs.logic().as_str()),
            rs.strict().encode(env),
            names.encode(env),
            field_paths.encode(env),
        ];
        Ok(map_term(env, &keys, &values))
    })
}

/// A rule set's summary (`spec/explain.md`, *Summaries*): as a term (`term`:
/// the summary document's content as maps with binary keys), as a document
/// (`json`, `yaml`) or as its text (`text`).
#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn ruleset_summary<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    format: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let summary = summary(&ruleset.ruleset);
        if atoms::key(env, "term") == format {
            return Ok(ok(env, json_term(env, &write_summary(&summary))));
        }
        let text = write_summary_as(&summary, format_arg(format)?)?;
        Ok(ok(env, text.encode(env)))
    })
}

// --- Evaluation ------------------------------------------------------------

/// Read one input along the rule set's paths; `row` is the input's position
/// in a batch, for the error of `max_values`.
fn read_input<'a>(
    env: Env<'a>,
    ruleset: &RuleSetResource,
    data: Term<'a>,
    max_values: usize,
    budget: Option<Budget>,
    row: Option<usize>,
) -> Result<(Tree, Vec<Term<'a>>), Stop> {
    let mut reader = Reader::new(env, max_values, budget);
    match reader.read(data, &ruleset.paths) {
        Ok(value) => Ok((value, reader.objects)),
        Err(Stop::Limit(mut error)) => {
            error.context_mut().row = row.and_then(|r| u64::try_from(r).ok());
            Err(Stop::Limit(error))
        }
        Err(Stop::Budget) => Err(Stop::Budget),
    }
}

/// The failure of reading without a budget: the input beyond `max_values`.
fn limit(stop: Stop) -> Failure {
    match stop {
        Stop::Limit(error) => Failure::Core(error),
        // Only a reader with a budget stops for it.
        Stop::Budget => Failure::Internal(
            "cleanrules failed (a reader without a budget ran out of it); please report it"
                .to_owned(),
        ),
    }
}

/// Evaluate one input and give back its decision.
fn decide<'a>(
    env: Env<'a>,
    ruleset: &RuleSetResource,
    input: &Tree,
    objects: &[Term<'a>],
) -> Outcome<Term<'a>> {
    let decision = ruleset.ruleset.evaluate(input.value())?;
    Ok(ok(env, decision_term(env, &decision, objects)?))
}

/// Tell the scheduler how much of its time slice (about a millisecond) the
/// call took.
fn consume(env: Env<'_>, start: Instant) {
    let micros = start.elapsed().as_micros();
    let percent = i32::try_from(micros / 10).unwrap_or(100).clamp(1, 100);
    rustler::schedule::consume_timeslice(env, percent);
}

/// Evaluate one input on the normal scheduler: `{ok, Decision}` or
/// `{error, Map}`, or the atom `dirty` when the rule set or the input is too
/// large for it, to call `evaluate_dirty/3` instead. A call that takes longer
/// than [`NORMAL_TIME`] sends the rule set's later calls to the dirty
/// scheduler at once.
#[rustler::nif]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn evaluate_normal<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    data: Term<'a>,
    max_values: Term<'a>,
) -> NifResult<Term<'a>> {
    let start = Instant::now();
    let result = guard(env, || {
        let max_values = size_arg(max_values)?;
        if ruleset.cost > NORMAL_COST || ruleset.prefer_dirty.load(Ordering::Relaxed) {
            return Ok(atoms::dirty().encode(env));
        }
        let budget = Budget {
            units: NORMAL_UNITS,
        };
        match read_input(env, &ruleset, data, max_values, Some(budget), None) {
            Ok((input, objects)) => decide(env, &ruleset, &input, &objects),
            Err(Stop::Budget) => Ok(atoms::dirty().encode(env)),
            Err(stop) => Err(limit(stop)),
        }
    });
    if start.elapsed() > NORMAL_TIME {
        ruleset.prefer_dirty.store(true, Ordering::Relaxed);
    }
    consume(env, start);
    result
}

/// Evaluate one input on a dirty CPU scheduler.
#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn evaluate_dirty<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    data: Term<'a>,
    max_values: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let max_values = size_arg(max_values)?;
        let (input, objects) =
            read_input(env, &ruleset, data, max_values, None, None).map_err(limit)?;
        decide(env, &ruleset, &input, &objects)
    })
}

/// Values dropped with an explicit stack ([`Value::drop_iteratively`]).
struct Values(Vec<Value>);

impl Drop for Values {
    fn drop(&mut self) {
        for value in self.0.drain(..) {
            value.drop_iteratively();
        }
    }
}

/// Evaluate a batch, a list of inputs: `{ok, [Decision]}` or the error of the
/// first input in order that fails, with its `row`.
#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn evaluate_many<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    inputs: Term<'a>,
    max_values: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let max_values = size_arg(max_values)?;
        if inputs.get_type() != TermType::List || inputs.list_length().is_err() {
            return Err(
                Error::new(ErrorCode::EvaluationError, "a batch is a list of inputs")
                    .with_hint("pass the inputs as the elements of a proper list")
                    .into(),
            );
        }
        // The core takes a slice of values: they are dropped with an explicit
        // stack however the call ends, a panic included.
        let mut values = Values(Vec::new());
        let mut objects = Vec::new();
        for (row, data) in inputs.into_list_iterator()?.enumerate() {
            let (mut tree, terms) =
                read_input(env, &ruleset, data, max_values, None, Some(row)).map_err(limit)?;
            values.0.push(tree.take());
            objects.push(terms);
        }
        let decisions = ruleset.ruleset.evaluate_many(&values.0)?;
        let mut terms: Vec<Term<'a>> = Vec::with_capacity(decisions.len());
        for (decision, objects) in decisions.iter().zip(&objects) {
            terms.push(decision_term(env, decision, objects)?);
        }
        Ok(ok(env, terms.encode(env)))
    })
}

/// Evaluate one input and explain its decision (`spec/explain.md`): as a term
/// (`term`: the explanation's structure as maps with binary keys, with the
/// values themselves), as a document (`json`, `yaml`) or as its text (`text`).
#[rustler::nif(schedule = "DirtyCpu", name = "explain")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn explain_input<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    data: Term<'a>,
    max_values: Term<'a>,
    format: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let max_values = size_arg(max_values)?;
        let format = if atoms::key(env, "term") == format {
            None
        } else {
            Some(format_arg(format)?)
        };
        let (input, objects) =
            read_input(env, &ruleset, data, max_values, None, None).map_err(limit)?;
        let decision = ruleset.ruleset.evaluate(input.value())?;
        let explanation = explain(&decision);
        let explained = match format {
            None => value_term(env, &explanation_value(&explanation), &objects)?,
            Some(format) => write_explanation_as(&explanation, format)?.encode(env),
        };
        Ok(ok(env, explained))
    })
}

/// Evaluate one input and write its decision as a document (`json` or
/// `yaml`, `spec/json-format.md`, *Decisions*), on a dirty CPU scheduler.
/// Opaque values are written as `{"$opaque": Type}`.
#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn decision_write<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    data: Term<'a>,
    max_values: Term<'a>,
    format: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let max_values = size_arg(max_values)?;
        let format = format_arg(format)?;
        let (input, _) = read_input(env, &ruleset, data, max_values, None, None).map_err(limit)?;
        let decision = ruleset.ruleset.evaluate(input.value())?;
        Ok(ok(env, write_decision_as(&decision, format)?.encode(env)))
    })
}

/// Read a recorded decision document with the rule set it was made with
/// (`spec/json-format.md`, *Reading a decision*), on a dirty CPU scheduler:
/// `{ok, Decision}`, whose opaque values come back as `null` (a document keeps
/// only their type's name).
#[rustler::nif(schedule = "DirtyCpu")]
#[allow(clippy::needless_pass_by_value)] // rustler decodes arguments by value.
fn decision_read<'a>(
    env: Env<'a>,
    ruleset: ResourceArc<RuleSetResource>,
    text: Term<'a>,
    format: Term<'a>,
    limits: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let text = binary_arg(text)?;
        let format = format_arg(format)?;
        let limits = limits_arg(limits)?;
        let decision =
            read_decision_document_as(text.as_slice(), format, &ruleset.ruleset, &limits)?;
        Ok(ok(env, decision_term(env, &decision, &[])?))
    })
}

// --- Values ----------------------------------------------------------------

/// What makes a converted constant not data: the first opaque value or null
/// with a reason, in order, and where it is. One path is kept and grown or
/// shrunk as the walk goes in and out, never copied per element.
fn not_data(value: &Value) -> Option<(Vec<Segment>, String)> {
    enum Task<'v> {
        Visit(Option<Segment>, &'v Value),
        Leave,
    }
    let mut tasks = vec![Task::Visit(None, value)];
    let mut at: Vec<Segment> = Vec::new();
    while let Some(task) = tasks.pop() {
        let (segment, value) = match task {
            Task::Leave => {
                at.pop();
                continue;
            }
            Task::Visit(segment, value) => (segment, value),
        };
        let entered = segment.is_some();
        if let Some(segment) = segment {
            at.push(segment);
        }
        let what = match value {
            Value::Opaque(opaque) => format!("an opaque value ({})", opaque.type_name),
            Value::Null(NullCause::IntOverflow) => "an integer beyond 64 bits".to_owned(),
            Value::Null(NullCause::DecimalOverflow) => "a decimal beyond 38 digits".to_owned(),
            Value::Null(NullCause::ReadError(detail)) => {
                format!("a value that failed to read ({detail})")
            }
            Value::List(items) => {
                if entered {
                    tasks.push(Task::Leave);
                }
                for (i, item) in items.iter().enumerate().rev() {
                    tasks.push(Task::Visit(Some(Segment::Index(i as u64)), item));
                }
                continue;
            }
            Value::Map(map) => {
                if entered {
                    tasks.push(Task::Leave);
                }
                let entries: Vec<(&str, &Value)> = map.iter().collect();
                for (key, item) in entries.into_iter().rev() {
                    tasks.push(Task::Visit(Some(Segment::Key(key.to_owned())), item));
                }
                continue;
            }
            _ => {
                if entered {
                    at.pop();
                }
                continue;
            }
        };
        return Some((at, what));
    }
    None
}

/// Convert a term as data (`data`: as at the end of a path, opaque values and
/// nulls with a reason allowed) or as a constant (`constant`: those are an
/// `expression_error`, `spec/data-model.md`, *Constants*), reading at most
/// `max_values` values, and give back its JSON text (`spec/json-format.md`,
/// *Values*).
#[rustler::nif(schedule = "DirtyCpu")]
fn value_to_json<'a>(
    env: Env<'a>,
    term: Term<'a>,
    mode: Term<'a>,
    max_values: Term<'a>,
) -> NifResult<Term<'a>> {
    guard(env, || {
        let constant = if atoms::key(env, "constant") == mode {
            true
        } else if atoms::key(env, "data") == mode {
            false
        } else {
            return Err(Failure::BadArg);
        };
        let max_values = size_arg(max_values)?;
        let value = Tree::new(
            Reader::new(env, max_values, None)
                .convert(term)
                .map_err(limit)?,
        );
        if constant && let Some((at, what)) = not_data(value.value()) {
            let place = if at.is_empty() {
                "the constant".to_owned()
            } else {
                format!("the constant's part at {}", to_string(&write_path(&at)))
            };
            return Err(Error::new(
                ErrorCode::ExpressionError,
                format!("{place} is {what}, which a constant cannot hold"),
            )
            .with_hint(
                "a constant holds null, booleans, integers of 64 bits, floats, decimals, \
                 UTF-8 binaries, dates and datetimes, and lists and maps of them",
            )
            .into());
        }
        Ok(ok(env, to_string(&write_value(value.value())).encode(env)))
    })
}

/// A value in the JSON format (`spec/json-format.md`, *Values*), read as data,
/// as an Erlang term.
#[rustler::nif(schedule = "DirtyCpu")]
fn value_from_json<'a>(env: Env<'a>, json: Term<'a>) -> NifResult<Term<'a>> {
    guard(env, || {
        let text = binary_arg(json)?;
        let parsed = parse_bytes(text.as_slice(), &Limits::default())?;
        let value = Tree::new(read_value(&parsed, "", ValueContext::Data)?);
        Ok(ok(env, value_term(env, value.value(), &[])?))
    })
}

rustler::init!("cleanrules_nif");
