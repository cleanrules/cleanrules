//! Reading limits and format names (`spec/json-format.md`, *Errors and
//! limits*), as JavaScript gives them.

use cleanrules::format::DocumentFormat;
use napi::bindgen_prelude::{Either, Uint8Array};
use napi::{Env, Result};
use napi_derive::napi;

use crate::hooks::range_error;

/// Limits for reading documents and inputs; each one left out keeps the core's
/// default. `maxSize` sets the largest text of every format (JSON, YAML, the
/// rule syntax and CSV); `maxYamlSize` and `maxTextSize` then lower or raise
/// YAML's and the rule syntax's.
#[napi(object)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    /// The largest document, in bytes of UTF-8.
    pub max_size: Option<f64>,
    /// The deepest nesting of arrays and objects.
    pub max_nesting: Option<f64>,
    /// The deepest condition or payload value, in nodes.
    pub max_depth: Option<f64>,
    /// The most cells and nodes a decision table may build.
    pub max_table_size: Option<f64>,
    /// The largest YAML text, in bytes.
    pub max_yaml_size: Option<f64>,
    /// The largest text in the rule syntax or CSV, in bytes.
    pub max_text_size: Option<f64>,
}

/// The largest integer a JavaScript number holds exactly.
const MAX_SAFE: f64 = 9_007_199_254_740_991.0;

fn size(env: &Env, name: &str, value: Option<f64>) -> Result<Option<usize>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !(value.is_finite() && value.fract() == 0.0 && (0.0..=MAX_SAFE).contains(&value)) {
        return Err(range_error(
            env,
            &format!("limits.{name} is {value}; a limit is a whole number from 0"),
        ));
    }
    // A limit beyond the address space is no limit at all.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Checked above.
    Ok(Some(usize::try_from(value as u64).unwrap_or(usize::MAX)))
}

/// The core's limits for `limits`, or its defaults.
pub(crate) fn of(env: &Env, limits: Option<Limits>) -> Result<cleanrules::json::Limits> {
    let mut out = cleanrules::json::Limits::default();
    let Some(limits) = limits else {
        return Ok(out);
    };
    if let Some(value) = size(env, "maxSize", limits.max_size)? {
        out = out.with_max_size(value);
    }
    if let Some(value) = size(env, "maxNesting", limits.max_nesting)? {
        out.max_nesting = value;
    }
    if let Some(value) = size(env, "maxDepth", limits.max_depth)? {
        out.max_depth = value;
    }
    if let Some(value) = size(env, "maxTableSize", limits.max_table_size)? {
        out.max_table_size = value;
    }
    if let Some(value) = size(env, "maxYamlSize", limits.max_yaml_size)? {
        out.max_yaml_size = value;
    }
    if let Some(value) = size(env, "maxTextSize", limits.max_text_size)? {
        out.max_text_size = value;
    }
    Ok(out)
}

/// The format named `name` (`json`, `yaml`, `text` or `csv`); `default` when
/// left out.
pub(crate) fn format(
    env: &Env,
    name: Option<&str>,
    default: DocumentFormat,
) -> Result<DocumentFormat> {
    let Some(name) = name else {
        return Ok(default);
    };
    DocumentFormat::from_name(name).ok_or_else(|| {
        range_error(
            env,
            &format!("unknown format {name:?}; use \"json\", \"yaml\", \"text\" or \"csv\""),
        )
    })
}

/// How a text is read: its format (JSON when left out) and limits.
#[napi(object)]
#[derive(Clone, Debug, Default)]
pub struct ReadOptions {
    /// The format: `"json"` (the default), `"yaml"`, `"text"` or `"csv"`.
    #[napi(ts_type = "Format")]
    pub format: Option<String>,
    /// The limits; each one left out keeps the core's default.
    pub limits: Option<Limits>,
}

/// The format and limits of `options`.
pub(crate) fn read_options(
    env: &Env,
    options: Option<ReadOptions>,
) -> Result<(DocumentFormat, cleanrules::json::Limits)> {
    let options = options.unwrap_or_default();
    Ok((
        format(env, options.format.as_deref(), DocumentFormat::Json)?,
        of(env, options.limits)?,
    ))
}

// String literal types for the declarations napi-rs writes: the values are
// passed and returned as strings, checked by the core's own name lookups.

/// The name of a format.
#[napi(string_enum)]
pub enum Format {
    /// JSON.
    #[napi(value = "json")]
    Json,
    /// YAML.
    #[napi(value = "yaml")]
    Yaml,
    /// The rule syntax.
    #[napi(value = "text")]
    Text,
    /// CSV, for decision tables.
    #[napi(value = "csv")]
    Csv,
}

/// The name of a hit policy (`spec/rules.md`).
#[napi(string_enum)]
pub enum HitPolicy {
    /// `first`.
    #[napi(value = "first")]
    First,
    /// `priority`.
    #[napi(value = "priority")]
    Priority,
    /// `unique`.
    #[napi(value = "unique")]
    Unique,
    /// `any`.
    #[napi(value = "any")]
    Any,
    /// `rule_order`.
    #[napi(value = "rule_order")]
    RuleOrder,
    /// `collect`.
    #[napi(value = "collect")]
    Collect,
}

/// The name of an aggregate of `collect`.
#[napi(string_enum)]
pub enum Aggregate {
    /// `sum`.
    #[napi(value = "sum")]
    Sum,
    /// `min`.
    #[napi(value = "min")]
    Min,
    /// `max`.
    #[napi(value = "max")]
    Max,
    /// `count`.
    #[napi(value = "count")]
    Count,
}

/// What a document holds.
#[napi(string_enum)]
pub enum DocumentKind {
    /// A condition.
    #[napi(value = "condition")]
    Condition,
    /// A rule.
    #[napi(value = "rule")]
    Rule,
    /// A rule set.
    #[napi(value = "ruleset")]
    Ruleset,
    /// A decision table.
    #[napi(value = "decision_table")]
    DecisionTable,
}

/// A crisp truth value.
#[napi(string_enum)]
pub enum Truth {
    /// `TRUE`.
    #[napi(value = "true")]
    True,
    /// `FALSE`.
    #[napi(value = "false")]
    False,
    /// `UNKNOWN`.
    #[napi(value = "unknown")]
    Unknown,
}

/// A text given as a string or as bytes. Bytes that are not UTF-8 are a
/// `format_error` at the line and column of the first invalid byte, as the
/// core reports them; a string is UTF-8 already.
pub(crate) type Text = Either<String, Uint8Array>;

/// The bytes of a text.
pub(crate) fn bytes(text: &Text) -> &[u8] {
    match text {
        Either::A(string) => string.as_bytes(),
        Either::B(bytes) => bytes,
    }
}
