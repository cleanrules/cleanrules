//! What the napi-rs and wasm-bindgen front ends share: checking arguments as
//! JavaScript gives them, the reader's encoding (`wire`), the documents, rule
//! sets, decisions and nodes over the core (`core`), conditions and rules
//! built in code (`build`), documents and decision tables for the idiomatic
//! API (`documents`, `tables`), and the encoding of what comes back (`out`).
//!
//! Nothing here touches a JavaScript engine. Each front end turns its
//! arguments into [`Raw`] values and [`RawOptions`], calls this code, and turns
//! a [`Problem`] into a JavaScript error, so that both builds accept the same
//! arguments and throw the same errors.

pub(crate) mod build;
pub(crate) mod core;
pub(crate) mod documents;
pub(crate) mod out;
pub(crate) mod tables;
pub(crate) mod wire;

use cleanrules::format::DocumentFormat;
use cleanrules::json::Limits;

/// Why a call failed, before it becomes a JavaScript error.
#[derive(Debug)]
pub(crate) enum Problem {
    /// An error of the core: a `CleanRulesError` subclass (`spec/errors.md`).
    Core(cleanrules::Error),
    /// An argument outside the values the binding accepts: a `RangeError`.
    Range(String),
    /// An argument of the wrong kind: a `TypeError`.
    Type(String),
    /// A bug in the binding: an `Error`.
    Internal(String),
}

impl From<cleanrules::Error> for Problem {
    fn from(error: cleanrules::Error) -> Self {
        Self::Core(error)
    }
}

/// The result of a call.
pub(crate) type Outcome<T> = Result<T, Problem>;

/// A JavaScript argument, as far as the binding needs to tell.
#[derive(Clone, Debug, Default)]
pub(crate) enum Raw {
    /// `undefined`.
    #[default]
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// A `Uint8Array` (a `Buffer` is one).
    Bytes(Vec<u8>),
    /// Anything else, by what it is: `"an object"`, `"an array"`...
    Other(&'static str),
}

impl Raw {
    /// What the value is, for messages: `"a number"`, `"an array"`...
    pub(crate) fn describe(&self) -> &'static str {
        match self {
            Raw::Undefined => "undefined",
            Raw::Null => "null",
            Raw::Bool(_) => "a boolean",
            Raw::Number(_) => "a number",
            Raw::String(_) => "a string",
            Raw::Bytes(_) => "a Uint8Array",
            Raw::Other(kind) => kind,
        }
    }
}

/// How [`Raw::Other`] names a plain object (or class instance): the only
/// kind of value an options or limits argument may be.
pub(crate) const OBJECT: &str = "an object";

/// The names of the limits, in the order of [`RawOptions::limits`].
pub(crate) const LIMIT_NAMES: [&str; 7] = [
    "maxSize",
    "maxNesting",
    "maxDepth",
    "maxTableSize",
    "maxYamlSize",
    "maxTextSize",
    "maxValues",
];

/// An options object as JavaScript gives it: each known member, read once.
/// Members that a call does not use are ignored.
#[derive(Clone, Debug)]
pub(crate) struct RawOptions {
    /// `format`.
    pub(crate) format: Raw,
    /// `logic`.
    pub(crate) logic: Raw,
    /// `strict`.
    pub(crate) strict: Raw,
    /// `limits`, an object whose members are named by [`LIMIT_NAMES`].
    pub(crate) limits: RawLimits,
}

/// A limits object as JavaScript gives it.
#[derive(Clone, Debug)]
pub(crate) enum RawLimits {
    /// `undefined` or `null`: the core's defaults.
    Absent,
    /// An object: its members, named by [`LIMIT_NAMES`].
    Given(Box<[Raw; 7]>),
    /// Anything else.
    Wrong(&'static str),
}

/// The problem with an options argument that is not an object.
pub(crate) fn not_options(kind: &str) -> Problem {
    Problem::Type(format!(
        "options must be an object, such as {{ format: 'yaml' }}, not {kind}"
    ))
}

/// A text: a string or bytes. Bytes that are not UTF-8 are a `format_error`
/// at the line and column of the first invalid byte, as the core reports them.
pub(crate) fn text(raw: Raw, what: &str) -> Outcome<Vec<u8>> {
    match raw {
        Raw::String(text) => Ok(text.into_bytes()),
        Raw::Bytes(bytes) => Ok(bytes),
        other => Err(Problem::Type(format!(
            "{what} must be a string or bytes (a Uint8Array), not {}",
            other.describe()
        ))),
    }
}

/// The format named by `raw` (`json`, `yaml`, `text` or `csv`); `default`
/// when it is left out.
pub(crate) fn format(raw: &Raw, default: DocumentFormat) -> Outcome<DocumentFormat> {
    match raw {
        Raw::Undefined | Raw::Null => Ok(default),
        Raw::String(name) => DocumentFormat::from_name(name).ok_or_else(|| {
            Problem::Range(format!(
                "unknown format {name:?}; use \"json\", \"yaml\", \"text\" or \"csv\""
            ))
        }),
        other => Err(Problem::Type(format!(
            "a format is a string such as \"json\", not {}",
            other.describe()
        ))),
    }
}

/// The largest integer a JavaScript number holds exactly.
const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

fn size(name: &str, raw: &Raw) -> Outcome<Option<usize>> {
    let value = match raw {
        Raw::Undefined | Raw::Null => return Ok(None),
        Raw::Number(value) => *value,
        other => {
            return Err(Problem::Type(format!(
                "limits.{name} is a number, not {}",
                other.describe()
            )));
        }
    };
    if !(value.is_finite() && value.fract() == 0.0 && (0.0..=MAX_SAFE).contains(&value)) {
        return Err(Problem::Range(format!(
            "limits.{name} is {value}; a limit is a whole number from 0"
        )));
    }
    // A limit beyond the address space is no limit at all.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Checked above.
    Ok(Some(usize::try_from(value as u64).unwrap_or(usize::MAX)))
}

/// The core's limits for `raw`, or its defaults. `maxSize` sets the largest
/// text of every format; `maxYamlSize` and `maxTextSize` then lower or raise
/// YAML's and the rule syntax's.
pub(crate) fn limits(raw: &RawLimits) -> Outcome<Limits> {
    let mut out = Limits::default();
    let values = match raw {
        RawLimits::Absent => return Ok(out),
        RawLimits::Wrong(kind) => {
            return Err(Problem::Type(format!(
                "limits must be an object, such as {{ maxSize: 1000000 }}, not {kind}"
            )));
        }
        RawLimits::Given(values) => values,
    };
    let [size_, nesting, depth, table, yaml, text, values_] = &**values;
    if let Some(value) = size(LIMIT_NAMES[0], size_)? {
        out = out.with_max_size(value);
    }
    if let Some(value) = size(LIMIT_NAMES[1], nesting)? {
        out.max_nesting = value;
    }
    if let Some(value) = size(LIMIT_NAMES[2], depth)? {
        out.max_depth = value;
    }
    if let Some(value) = size(LIMIT_NAMES[3], table)? {
        out.max_table_size = value;
    }
    if let Some(value) = size(LIMIT_NAMES[4], yaml)? {
        out.max_yaml_size = value;
    }
    if let Some(value) = size(LIMIT_NAMES[5], text)? {
        out.max_text_size = value;
    }
    if let Some(value) = size(LIMIT_NAMES[6], values_)? {
        out.max_values = value;
    }
    Ok(out)
}

/// The format (JSON by default) and limits of a reading call's options.
pub(crate) fn read_options(options: Option<&RawOptions>) -> Outcome<(DocumentFormat, Limits)> {
    match options {
        None => Ok((DocumentFormat::Json, Limits::default())),
        Some(options) => Ok((
            format(&options.format, DocumentFormat::Json)?,
            limits(&options.limits)?,
        )),
    }
}

/// How one node is evaluated: the logic's name (boolean by default), whether
/// strictly, and the limits: `max_values` for native data, the others for an
/// input given as JSON.
pub(crate) struct TreeOptions {
    pub(crate) logic: Option<String>,
    pub(crate) strict: bool,
    pub(crate) limits: Limits,
}

/// The options of evaluating one node.
pub(crate) fn tree_options(options: Option<&RawOptions>) -> Outcome<TreeOptions> {
    let Some(options) = options else {
        return Ok(TreeOptions {
            logic: None,
            strict: false,
            limits: Limits::default(),
        });
    };
    let logic = match &options.logic {
        Raw::Undefined | Raw::Null => None,
        Raw::String(name) => Some(name.clone()),
        other => {
            return Err(Problem::Type(format!(
                "options.logic is a string such as \"boolean\", not {}",
                other.describe()
            )));
        }
    };
    let strict = match &options.strict {
        Raw::Undefined | Raw::Null => false,
        Raw::Bool(strict) => *strict,
        other => {
            return Err(Problem::Type(format!(
                "options.strict is a boolean, not {}",
                other.describe()
            )));
        }
    };
    Ok(TreeOptions {
        logic,
        strict,
        limits: limits(&options.limits)?,
    })
}

/// A rule's position, below `count`.
pub(crate) fn position(raw: &Raw, count: usize, what: &str) -> Outcome<usize> {
    let Raw::Number(value) = raw else {
        return Err(Problem::Type(format!(
            "a rule's position is a number, not {}",
            raw.describe()
        )));
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )] // Checked to be a whole number below `count`.
    if value.fract() == 0.0 && *value >= 0.0 && *value < count as f64 {
        Ok(*value as usize)
    } else {
        Err(Problem::Range(format!(
            "rule {value} is out of the {what}'s {count} rules"
        )))
    }
}

/// A list of strings, such as action names.
pub(crate) fn strings(items: Vec<Raw>, what: &str) -> Outcome<Vec<String>> {
    items
        .into_iter()
        .map(|item| match item {
            Raw::String(text) => Ok(text),
            other => Err(Problem::Type(format!(
                "{what} are strings, not {}",
                other.describe()
            ))),
        })
        .collect()
}

/// The problem with a list argument that is not an array.
pub(crate) fn not_a_list(what: &str, raw: &Raw) -> Problem {
    Problem::Type(format!("{what} must be an array, not {}", raw.describe()))
}

/// Why `text` is not a decimal literal, or `None` when it is one; for the
/// `Decimal` marker of `lib/values.js`. A literal too large for 38 digits is a
/// decimal: as data it reads as null with reason `decimal_overflow`.
pub(crate) fn decimal_problem(text: &str) -> Option<String> {
    match cleanrules::Decimal::parse(text) {
        Err(cleanrules::value::DecimalError::Malformed) => {
            Some("not a decimal literal, such as \"0.10\", \"-3\", \"1E+3\" or \"inf\"".to_owned())
        }
        _ => None,
    }
}

/// The error of calling a class of the engine with `new`: its objects come
/// from its factories, the same in both builds.
pub(crate) fn no_constructor(class: &str) -> Problem {
    let how = match class {
        "Decision" => "decisions come from RuleSet.evaluate".to_owned(),
        "NativeRule" => "rules come from the Rule class of cleanrules".to_owned(),
        "NativeTable" | "NativeGrid" => {
            "tables come from the DecisionTable class of cleanrules".to_owned()
        }
        "NativeTree" => "result trees come from evaluateTree of cleanrules".to_owned(),
        _ => format!("use {class}.read"),
    };
    Problem::Type(format!("{class} has no public constructor; {how}"))
}

/// The error of an argument that should be a `Document` of the engine.
pub(crate) fn not_a_document(raw: &Raw) -> Problem {
    Problem::Type(format!(
        "document must be a Document (from Document.read), not {}",
        match raw {
            Raw::Other(OBJECT) => "another object",
            other => other.describe(),
        }
    ))
}

/// The message of an undefined input.
pub(crate) const UNDEFINED_INPUT: &str =
    "the input is undefined; pass the data to evaluate (null for none)";
