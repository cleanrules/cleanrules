//! Bundles (`spec/bundles.md`): reproducible packages of rules.
//!
//! A bundle holds the rules (a rule set or a decision table), the identifier
//! and revision its author gives them, their rule set digest, the engine and
//! configuration that made it, the contract of their input and output, and
//! references to files of test cases. Comparing two bundles tells the same
//! rules under another engine from other rules under the same engine.
//!
//! ```
//! use cleanrules::build::{constant, field};
//! use cleanrules::bundle::{Bundle, CaseFile, Relation};
//! use cleanrules::json::Limits;
//! use cleanrules::rules::{Config, Rule, RuleSet};
//!
//! let rule = Rule::new("adult", field("age").ge(constant(18)).build()?)?;
//! let rules = RuleSet::new(vec![rule], Config::default())?;
//! let cases = vec![CaseFile::new("cases/adults.json", b"[]")?];
//! let bundle = Bundle::new(rules, "acme/adults", "1", cases, &Limits::default())?;
//! assert_eq!(bundle.ruleset_digest(), bundle.ruleset().digest());
//! assert!(bundle.engine().is_current());
//!
//! let same = Bundle::new(bundle.ruleset().clone(), "acme/adults", "1", vec![], &Limits::default())?;
//! let comparison = bundle.compare(&same);
//! assert_eq!(comparison.relation(), Relation::SameRulesSameEngine);
//! assert_eq!(comparison.differences(), ["cases"]);
//! # Ok::<(), cleanrules::Error>(())
//! ```

use std::fmt;

use crate::digest::Digest;
use crate::error::{Error, ErrorCode, Result};
use crate::explain::summary;
use crate::format::{Document, write_table};
use crate::json::{Json, Limits, same};
use crate::rules::RuleSet;
use crate::table::DecisionTable;

/// The name this engine writes in the bundles it makes.
pub const ENGINE_NAME: &str = "cleanrules";

/// The version this engine writes in the bundles it makes: the version of the
/// `cleanrules` crate.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The largest limit a bundle records: 2⁵³ − 1, the largest integer every
/// language reads exactly.
pub const MAX_RECORDED_LIMIT: u64 = (1 << 53) - 1;

/// The most characters of an identifier, revision, engine name or version.
const MAX_NAME: usize = 128;

/// The most characters of a case file's path.
const MAX_PATH: usize = 1024;

/// The rules a bundle holds: a rule set, or a decision table.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum BundleRules {
    /// A rule set.
    RuleSet(RuleSet),
    /// A decision table, which gives the rule set it compiles to.
    DecisionTable(DecisionTable),
}

impl BundleRules {
    /// The rule set: the one held, or the one the table compiles to.
    #[must_use]
    pub fn ruleset(&self) -> &RuleSet {
        match self {
            Self::RuleSet(ruleset) => ruleset,
            Self::DecisionTable(table) => table.ruleset(),
        }
    }

    /// The digest of the document the rules come from: the rule set's
    /// document, or the table's own.
    fn source_digest(&self) -> Digest {
        match self {
            Self::RuleSet(ruleset) => ruleset.digest(),
            Self::DecisionTable(table) => Digest::of(&crate::digest::canonical(
                "decision_table",
                write_table(table),
            )),
        }
    }
}

impl From<RuleSet> for BundleRules {
    fn from(ruleset: RuleSet) -> Self {
        Self::RuleSet(ruleset)
    }
}

impl From<DecisionTable> for BundleRules {
    fn from(table: DecisionTable) -> Self {
        Self::DecisionTable(table)
    }
}

/// The limits a bundle records (`spec/bundles.md`, *Engine*): those that can
/// change a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct EngineLimits {
    /// The largest JSON document, in bytes.
    pub max_size: u64,
    /// The deepest nesting of arrays and objects.
    pub max_nesting: u64,
    /// The deepest condition or payload value.
    pub max_depth: u64,
    /// The longest field path.
    pub max_path_segments: u64,
    /// The most cells and nodes of a decision table.
    pub max_table_size: u64,
    /// The most values read from one input's native data.
    pub max_values: u64,
}

/// The names of the recorded limits, in the order of their table.
pub(crate) const LIMIT_NAMES: [&str; 6] = [
    "max_size",
    "max_nesting",
    "max_depth",
    "max_path_segments",
    "max_table_size",
    "max_values",
];

/// The differences of the limits, as `Comparison::differences` names them.
const LIMIT_DIFFERENCES: [&str; 6] = [
    "engine.limits.max_size",
    "engine.limits.max_nesting",
    "engine.limits.max_depth",
    "engine.limits.max_path_segments",
    "engine.limits.max_table_size",
    "engine.limits.max_values",
];

impl EngineLimits {
    /// The limits to record from a reader's limits.
    ///
    /// # Errors
    ///
    /// `unsupported` for a limit of 0 or above [`MAX_RECORDED_LIMIT`], which a
    /// bundle cannot record.
    pub fn of(limits: &Limits) -> Result<Self> {
        let recorded = |name: &str, limit: usize| {
            u64::try_from(limit)
                .ok()
                .filter(|&n| (1..=MAX_RECORDED_LIMIT).contains(&n))
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unsupported,
                        format!("the limit {name} is {limit}, which a bundle does not record"),
                    )
                    .with_hint("a bundle records limits from 1 to 2^53 - 1")
                    .unsupported("bundle", "limits of 0 or above 2^53 - 1 cannot be recorded")
                })
        };
        Ok(Self {
            max_size: recorded("max_size", limits.max_size)?,
            max_nesting: recorded("max_nesting", limits.max_nesting)?,
            max_depth: recorded("max_depth", limits.max_depth)?,
            max_path_segments: recorded("max_path_segments", limits.max_path_segments)?,
            max_table_size: recorded("max_table_size", limits.max_table_size)?,
            max_values: recorded("max_values", limits.max_values)?,
        })
    }

    /// Reader limits with these values, the others at their defaults; a value
    /// larger than this platform's `usize` saturates.
    #[must_use]
    pub fn to_limits(&self) -> Limits {
        let size = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
        Limits {
            max_size: size(self.max_size),
            max_nesting: size(self.max_nesting),
            max_depth: size(self.max_depth),
            max_path_segments: size(self.max_path_segments),
            max_table_size: size(self.max_table_size),
            max_values: size(self.max_values),
            ..Limits::default()
        }
    }

    /// The values in the order of [`LIMIT_NAMES`].
    pub(crate) fn values(&self) -> [u64; 6] {
        [
            self.max_size,
            self.max_nesting,
            self.max_depth,
            self.max_path_segments,
            self.max_table_size,
            self.max_values,
        ]
    }

    /// The limits from values in the order of [`LIMIT_NAMES`].
    pub(crate) fn from_values(values: [u64; 6]) -> Self {
        let [
            max_size,
            max_nesting,
            max_depth,
            max_path_segments,
            max_table_size,
            max_values,
        ] = values;
        Self {
            max_size,
            max_nesting,
            max_depth,
            max_path_segments,
            max_table_size,
            max_values,
        }
    }
}

impl Default for EngineLimits {
    /// The default limits of the readers.
    fn default() -> Self {
        let defaults = Limits::default();
        let size = |n: usize| u64::try_from(n).unwrap_or(MAX_RECORDED_LIMIT);
        Self {
            max_size: size(defaults.max_size),
            max_nesting: size(defaults.max_nesting),
            max_depth: size(defaults.max_depth),
            max_path_segments: size(defaults.max_path_segments),
            max_table_size: size(defaults.max_table_size),
            max_values: size(defaults.max_values),
        }
    }
}

/// The engine and configuration that made a bundle (`spec/bundles.md`,
/// *Engine*).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Engine {
    name: String,
    version: String,
    limits: EngineLimits,
}

impl Engine {
    /// This engine, with `limits`.
    ///
    /// # Errors
    ///
    /// `unsupported` for a limit a bundle cannot record ([`EngineLimits::of`]).
    pub fn current(limits: &Limits) -> Result<Self> {
        Ok(Self {
            name: ENGINE_NAME.to_owned(),
            version: ENGINE_VERSION.to_owned(),
            limits: EngineLimits::of(limits)?,
        })
    }

    /// An engine as a bundle records it.
    ///
    /// # Errors
    ///
    /// `format_error` for a name or a version that does not have its form
    /// (`spec/bundles.md`, *Engine*).
    pub fn new(name: &str, version: &str, limits: EngineLimits) -> Result<Self> {
        check_engine_name(name)?;
        check_version(version)?;
        if let Some(name) = LIMIT_NAMES
            .iter()
            .zip(limits.values())
            .find_map(|(name, n)| (n == 0 || n > MAX_RECORDED_LIMIT).then_some(name))
        {
            return Err(Error::new(
                ErrorCode::FormatError,
                format!("the limit {name} must be from 1 to 2^53 - 1"),
            )
            .with_hint("record limits from 1 to 2^53 - 1"));
        }
        Ok(Self {
            name: name.to_owned(),
            version: version.to_owned(),
            limits,
        })
    }

    /// The engine's name, `cleanrules` for this one.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The engine's exact version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The limits it ran with.
    #[must_use]
    pub fn limits(&self) -> &EngineLimits {
        &self.limits
    }

    /// Whether this is this engine: the same name and version, whatever the
    /// limits. A reader verifies the contract of such a bundle.
    #[must_use]
    pub fn is_current(&self) -> bool {
        self.name == ENGINE_NAME && self.version == ENGINE_VERSION
    }
}

/// The contract of a bundle's rules (`spec/bundles.md`, *Contract*): the
/// input schema, and the output keys.
#[derive(Clone, Debug)]
pub struct Contract {
    input: Json,
    output_keys: Vec<String>,
}

impl Contract {
    /// The contract this engine derives from a rule set: its input schema
    /// (`spec/agents.md`, *Input schemas*) and its output keys, sorted by
    /// Unicode code point.
    #[must_use]
    pub fn derive(ruleset: &RuleSet) -> Self {
        let overview = summary(ruleset);
        let mut output_keys = overview.outputs.clone();
        output_keys.sort();
        Self {
            input: crate::agents::input_schema(&overview),
            output_keys,
        }
    }

    /// A contract as a bundle records it; the output keys are sorted.
    pub(crate) fn recorded(input: Json, mut output_keys: Vec<String>) -> Self {
        output_keys.sort();
        Self { input, output_keys }
    }

    /// The input schema, a JSON Schema.
    #[must_use]
    pub fn input(&self) -> &Json {
        &self.input
    }

    /// The payload keys any rule may output, sorted by Unicode code point.
    #[must_use]
    pub fn output_keys(&self) -> &[String] {
        &self.output_keys
    }
}

/// Contracts compare as JSON values: the members of the input schema in any
/// order (`spec/json-format.md`, *Objects*).
impl PartialEq for Contract {
    fn eq(&self, other: &Self) -> bool {
        same(&self.input, &other.input) && self.output_keys == other.output_keys
    }
}

/// A reference to a file of test cases (`spec/bundles.md`, *Cases*): its path,
/// relative to the bundle's directory, and the digest of its bytes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CaseFile {
    path: String,
    digest: Digest,
}

impl CaseFile {
    /// The reference to the file at `path` whose bytes are `bytes`.
    ///
    /// # Errors
    ///
    /// `format_error` for a path that does not have its form.
    pub fn new(path: &str, bytes: &[u8]) -> Result<Self> {
        Self::with_digest(path, Digest::of(bytes))
    }

    /// The reference to the file at `path` with a digest already computed.
    ///
    /// # Errors
    ///
    /// `format_error` for a path that does not have its form.
    pub fn with_digest(path: &str, digest: Digest) -> Result<Self> {
        check_path(path)?;
        Ok(Self {
            path: path.to_owned(),
            digest,
        })
    }

    /// The path, relative to the bundle's directory.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The digest of the file's bytes.
    #[must_use]
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// Whether `bytes` are the file the bundle was made with.
    #[must_use]
    pub fn matches(&self, bytes: &[u8]) -> bool {
        Digest::of(bytes) == self.digest
    }
}

/// A bundle (`spec/bundles.md`).
#[derive(Clone, Debug)]
pub struct Bundle {
    id: String,
    revision: String,
    rules: BundleRules,
    engine: Engine,
    contract: Contract,
    cases: Vec<CaseFile>,
}

impl Bundle {
    /// Make a bundle of `rules` with this engine (`spec/bundles.md`, *Making a
    /// bundle*): the identifier and revision its author gives, references to
    /// its case files, and the limits it runs with, which the bundle records.
    /// The rule set digest and the contract are derived from the rules.
    ///
    /// # Errors
    ///
    /// `format_error` for an identifier, a revision or a repeated case path;
    /// `unsupported` for a limit of 0 or above [`MAX_RECORDED_LIMIT`].
    pub fn new(
        rules: impl Into<BundleRules>,
        id: &str,
        revision: &str,
        cases: Vec<CaseFile>,
        limits: &Limits,
    ) -> Result<Self> {
        check_id(id)?;
        check_revision(revision)?;
        if let Some(repeated) = repeated_path(&cases) {
            return Err(Error::new(
                ErrorCode::FormatError,
                format!("the case file {repeated:?} is given twice"),
            )
            .with_hint("give each case file once"));
        }
        let rules = rules.into();
        let contract = Contract::derive(rules.ruleset());
        Ok(Self {
            id: id.to_owned(),
            revision: revision.to_owned(),
            rules,
            engine: Engine::current(limits)?,
            contract,
            cases,
        })
    }

    /// A bundle as read, checked by the reader.
    pub(crate) fn read(
        id: String,
        revision: String,
        rules: BundleRules,
        engine: Engine,
        contract: Contract,
        cases: Vec<CaseFile>,
    ) -> Self {
        Self {
            id,
            revision,
            rules,
            engine,
            contract,
            cases,
        }
    }

    /// The stable identifier of the rules.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The revision of the rules.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// The rules, as given: a rule set or a decision table.
    #[must_use]
    pub fn rules(&self) -> &BundleRules {
        &self.rules
    }

    /// The rule set the bundle gives: the one it holds, or the one its table
    /// compiles to.
    #[must_use]
    pub fn ruleset(&self) -> &RuleSet {
        self.rules.ruleset()
    }

    /// The rule set digest of its rules.
    #[must_use]
    pub fn ruleset_digest(&self) -> Digest {
        self.ruleset().digest()
    }

    /// The engine and configuration that made it.
    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The contract of the input and the output.
    #[must_use]
    pub fn contract(&self) -> &Contract {
        &self.contract
    }

    /// The references to its case files, in order.
    #[must_use]
    pub fn cases(&self) -> &[CaseFile] {
        &self.cases
    }

    /// Whether `bytes` are the case file at `path` the bundle was made with;
    /// `None` when the bundle refers to no file at `path`.
    #[must_use]
    pub fn case_matches(&self, path: &str, bytes: &[u8]) -> Option<bool> {
        self.cases
            .iter()
            .find(|case| case.path == path)
            .map(|case| case.matches(bytes))
    }

    /// The bundle's digest: that of its document (`spec/bundles.md`, *Digest of
    /// a bundle*).
    #[must_use]
    pub fn digest(&self) -> Digest {
        Digest::of(&self.canonical_bytes())
    }

    /// The canonical bytes of its document.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        crate::digest::canonical("bundle", crate::format::write_bundle(self))
    }

    /// Compare with another bundle (`spec/bundles.md`, *Comparing bundles*).
    #[must_use]
    pub fn compare(&self, other: &Bundle) -> Comparison {
        compare(self, other)
    }

    /// Hold the bundle in a document, to write it in any format.
    #[must_use]
    pub fn into_document(self) -> Document {
        Document::Bundle(Box::new(self))
    }
}

/// How two bundles relate (`spec/bundles.md`, *Comparing bundles*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Relation {
    /// The same identifier and revision with other rules: a revision is not
    /// immutable.
    Conflict,
    /// The same rules on the same engine and configuration.
    SameRulesSameEngine,
    /// The same rules on another engine or configuration.
    SameRulesOtherEngine,
    /// Other rules on the same engine and configuration.
    OtherRulesSameEngine,
    /// Other rules on another engine or configuration.
    OtherRulesOtherEngine,
}

impl Relation {
    /// Its name in JSON, such as `same_rules_other_engine`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conflict => "conflict",
            Self::SameRulesSameEngine => "same_rules_same_engine",
            Self::SameRulesOtherEngine => "same_rules_other_engine",
            Self::OtherRulesSameEngine => "other_rules_same_engine",
            Self::OtherRulesOtherEngine => "other_rules_other_engine",
        }
    }
}

impl fmt::Display for Relation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The comparison of two bundles (`spec/bundles.md`, *Comparing bundles*).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    relation: Relation,
    differences: Vec<&'static str>,
}

impl Comparison {
    /// How the bundles relate.
    #[must_use]
    pub fn relation(&self) -> Relation {
        self.relation
    }

    /// Whether the bundles are the same: no difference, the same digest.
    #[must_use]
    pub fn is_identical(&self) -> bool {
        self.differences.is_empty()
    }

    /// What differs, in the order of the specification: `id`, `revision`,
    /// `rules`, `source`, `engine.name`, `engine.version`,
    /// `engine.limits.<name>`, `contract.input`, `contract.output`, `cases`.
    #[must_use]
    pub fn differences(&self) -> &[&'static str] {
        &self.differences
    }

    /// Its JSON form: `relation`, `identical` and `differences`.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::Object(vec![
            (
                "relation".into(),
                Json::String(self.relation.as_str().into()),
            ),
            ("identical".into(), Json::Bool(self.is_identical())),
            (
                "differences".into(),
                Json::Array(
                    self.differences
                        .iter()
                        .map(|d| Json::String((*d).into()))
                        .collect(),
                ),
            ),
        ])
    }
}

fn compare(a: &Bundle, b: &Bundle) -> Comparison {
    let same_rules = a.ruleset_digest() == b.ruleset_digest();
    let mut differences = Vec::new();
    let mut differ = |different: bool, name: &'static str| {
        if different {
            differences.push(name);
        }
    };
    differ(a.id != b.id, "id");
    differ(a.revision != b.revision, "revision");
    differ(!same_rules, "rules");
    differ(
        same_rules && a.rules.source_digest() != b.rules.source_digest(),
        "source",
    );
    differ(a.engine.name != b.engine.name, "engine.name");
    differ(a.engine.version != b.engine.version, "engine.version");
    for ((x, y), name) in a
        .engine
        .limits
        .values()
        .into_iter()
        .zip(b.engine.limits.values())
        .zip(LIMIT_DIFFERENCES)
    {
        differ(x != y, name);
    }
    differ(
        !same(&a.contract.input, &b.contract.input),
        "contract.input",
    );
    differ(
        a.contract.output_keys != b.contract.output_keys,
        "contract.output",
    );
    differ(a.cases != b.cases, "cases");
    let same_engine = a.engine == b.engine;
    let relation = if a.id == b.id && a.revision == b.revision && !same_rules {
        Relation::Conflict
    } else {
        match (same_rules, same_engine) {
            (true, true) => Relation::SameRulesSameEngine,
            (true, false) => Relation::SameRulesOtherEngine,
            (false, true) => Relation::OtherRulesSameEngine,
            (false, false) => Relation::OtherRulesOtherEngine,
        }
    };
    Comparison {
        relation,
        differences,
    }
}

// --- Forms -------------------------------------------------------------------

fn malformed(what: &str, text: &str, hint: &str) -> Error {
    let shown: String = text.chars().take(80).collect();
    let more = if shown.len() < text.len() { "…" } else { "" };
    Error::new(
        ErrorCode::FormatError,
        format!("{shown:?}{more} is not {what}"),
    )
    .with_hint(hint.to_owned())
}

fn lower_or_digit(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// One segment of an identifier, or an engine name: `[a-z0-9][a-z0-9._-]*`.
fn is_name_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars.next().is_some_and(lower_or_digit)
        && chars.all(|c| lower_or_digit(c) || matches!(c, '.' | '_' | '-'))
}

/// Check a bundle's identifier (`spec/bundles.md`, *Identifier and revision*).
///
/// # Errors
///
/// `format_error` when it does not have its form.
pub fn check_id(id: &str) -> Result<()> {
    if id.len() <= MAX_NAME && id.split('/').all(is_name_segment) {
        Ok(())
    } else {
        Err(malformed(
            "a bundle identifier",
            id,
            "use lowercase letters, digits, '.', '_' and '-', in segments separated by '/' (at most 128 characters), such as acme/discounts",
        ))
    }
}

/// Check a bundle's revision (`spec/bundles.md`, *Identifier and revision*).
///
/// # Errors
///
/// `format_error` when it does not have its form.
pub fn check_revision(revision: &str) -> Result<()> {
    let mut chars = revision.chars();
    if revision.len() <= MAX_NAME
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        Ok(())
    } else {
        Err(malformed(
            "a revision",
            revision,
            "use letters, digits, '.', '_', '+' and '-', starting with a letter or a digit (at most 128 characters), such as 3 or 2026-11-16.1",
        ))
    }
}

fn check_engine_name(name: &str) -> Result<()> {
    if name.len() <= MAX_NAME && is_name_segment(name) {
        Ok(())
    } else {
        Err(malformed(
            "an engine name",
            name,
            "use lowercase letters, digits, '.', '_' and '-' (at most 128 characters), such as cleanrules",
        ))
    }
}

/// Whether `version` is a Semantic Versioning 2.0.0 version.
fn is_semver(version: &str) -> bool {
    let numeric = |s: &str| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
    };
    let identifier =
        |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let (rest, build) = match version.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (version, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| numeric(p))
        && pre.is_none_or(|pre| {
            pre.split('.')
                .all(|p| identifier(p) && (p.bytes().any(|b| !b.is_ascii_digit()) || numeric(p)))
        })
        && build.is_none_or(|build| build.split('.').all(identifier))
}

fn check_version(version: &str) -> Result<()> {
    if version.len() <= MAX_NAME && is_semver(version) {
        Ok(())
    } else {
        Err(malformed(
            "an engine version",
            version,
            "use a Semantic Versioning 2.0.0 version, such as 0.4.0 or 1.0.0-rc.1",
        ))
    }
}

fn check_path(path: &str) -> Result<()> {
    // Lowercase, so that two paths are two files on file systems that ignore
    // case too; no segment ends with '.' (so neither '.' nor '..'), which
    // Windows would drop.
    let segment = |s: &str| {
        !s.is_empty()
            && !s.ends_with('.')
            && s.chars()
                .all(|c| lower_or_digit(c) || matches!(c, '.' | '_' | '-'))
    };
    if path.len() <= MAX_PATH && path.split('/').all(segment) {
        Ok(())
    } else {
        Err(malformed(
            "a case file path",
            path,
            "use a relative path of lowercase letters, digits, '.', '_' and '-' in segments separated by '/', none of them ending with '.', such as cases/basic.json",
        ))
    }
}

/// Check the form of an engine name and version, for readers.
pub(crate) fn check_engine(name: &str, version: &str) -> (Result<()>, Result<()>) {
    (check_engine_name(name), check_version(version))
}

/// Check a case file's path, for readers.
pub(crate) fn check_case_path(path: &str) -> Result<()> {
    check_path(path)
}

/// The first path given twice.
fn repeated_path(cases: &[CaseFile]) -> Option<&str> {
    let mut seen = std::collections::BTreeSet::new();
    cases
        .iter()
        .map(CaseFile::path)
        .find(|path| !seen.insert(*path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{constant, field};
    use crate::rules::{Config, Rule};

    fn rules(threshold: i64) -> RuleSet {
        let rule = Rule::new(
            "vip",
            field("years").gt(constant(threshold)).build().unwrap(),
        )
        .unwrap()
        .with_payload_entry("discount", constant(1).build().unwrap())
        .unwrap();
        RuleSet::new(vec![rule], Config::default()).unwrap()
    }

    #[test]
    fn identifiers_and_revisions_have_their_forms() {
        for good in [
            "a",
            "acme/discounts",
            "loan-approval",
            "a.b_c-d/0",
            &"a".repeat(128),
        ] {
            assert!(check_id(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "Acme",
            "/a",
            "a/",
            "a//b",
            "-a",
            "a b",
            "é",
            &"a".repeat(129),
        ] {
            assert_eq!(
                check_id(bad).unwrap_err().code(),
                ErrorCode::FormatError,
                "{bad}"
            );
        }
        for good in ["1", "v1.2.0", "2026-11-16.1", "1.0.0+build.5", "A_b"] {
            assert!(check_revision(good).is_ok(), "{good}");
        }
        for bad in ["", "-1", ".1", "a b", "a/b", "é", &"1".repeat(129)] {
            assert!(check_revision(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn versions_are_semantic_versions() {
        for good in [
            "0.0.0",
            "1.2.3",
            "10.20.30",
            "1.0.0-rc.1",
            "1.0.0-alpha.beta-1",
            "1.0.0-0a.1",
            "1.0.0+build.5",
            "1.0.0-rc.1+sha.0abc",
            "1.0.0+001",
        ] {
            assert!(is_semver(good), "{good}");
        }
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "v1.2.3",
            "1.2.3-",
            "1.2.3-01",
            "1.2.3-a..b",
            "1.2.3+",
            "1.2.3+a..b",
            "1.2.3-é",
            " 1.2.3",
            "1.2.-3",
        ] {
            assert!(!is_semver(bad), "{bad}");
        }
        assert!(is_semver(ENGINE_VERSION));
    }

    #[test]
    fn case_paths_stay_inside_the_directory() {
        for good in ["a", "cases/a.json", "..a/b", "a/.b", "a-b_c.d", ".a"] {
            assert!(check_path(good).is_ok(), "{good}");
        }
        for bad in [
            "", "/a", "a/", "a//b", ".", "..", "a/../b", "./a", "a\\b", "c:/a", "a b", "A.json",
            "cases/A", "a./b", "a.",
        ] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
        assert!(check_path(&"a".repeat(1025)).is_err());
    }

    #[test]
    fn a_bundle_records_its_engine_and_contract() {
        let bundle = Bundle::new(rules(5), "acme/vip", "1", vec![], &Limits::default()).unwrap();
        assert_eq!(bundle.id(), "acme/vip");
        assert_eq!(bundle.revision(), "1");
        assert!(bundle.engine().is_current());
        assert_eq!(bundle.engine().name(), ENGINE_NAME);
        assert_eq!(bundle.engine().limits(), &EngineLimits::default());
        assert_eq!(bundle.contract(), &Contract::derive(bundle.ruleset()));
        assert_eq!(bundle.contract().output_keys(), ["discount"]);
        assert_eq!(bundle.ruleset_digest(), bundle.ruleset().digest());
        assert_eq!(
            bundle.digest(),
            bundle.clone().into_document().digest(),
            "the bundle's digest is its document's"
        );
    }

    #[test]
    fn making_checks_what_the_author_gives() {
        let make = |id: &str, revision: &str, cases: Vec<CaseFile>| {
            Bundle::new(rules(5), id, revision, cases, &Limits::default())
        };
        assert!(make("Bad", "1", vec![]).is_err());
        assert!(make("ok", " ", vec![]).is_err());
        let case = CaseFile::new("a.json", b"x").unwrap();
        let error = make("ok", "1", vec![case.clone(), case]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::FormatError);
        assert!(CaseFile::new("../a", b"x").is_err());
        let mut limits = Limits {
            max_values: usize::MAX,
            ..Limits::default()
        };
        let error = Bundle::new(rules(5), "ok", "1", vec![], &limits).unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
        limits.max_values = 0;
        let error = Bundle::new(rules(5), "ok", "1", vec![], &limits).unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
        limits.max_values = 7;
        let bundle = Bundle::new(rules(5), "ok", "1", vec![], &limits).unwrap();
        assert_eq!(bundle.engine().limits().max_values, 7);
        assert_eq!(bundle.engine().limits().to_limits().max_values, 7);
    }

    #[test]
    fn case_files_match_their_bytes() {
        let case = CaseFile::new("cases/a.json", b"[1]").unwrap();
        assert!(case.matches(b"[1]"));
        let bundle =
            Bundle::new(rules(5), "r", "1", vec![case.clone()], &Limits::default()).unwrap();
        assert_eq!(bundle.case_matches("cases/a.json", b"[1]"), Some(true));
        assert_eq!(bundle.case_matches("cases/a.json", b"[2]"), Some(false));
        assert_eq!(bundle.case_matches("cases/b.json", b"[1]"), None);
        // Contracts compare without member order.
        let reordered = match bundle.contract().input() {
            Json::Object(members) => Json::Object(members.iter().rev().cloned().collect()),
            _ => panic!("an object"),
        };
        let contract = Contract::recorded(reordered, bundle.contract().output_keys().to_vec());
        assert_eq!(&contract, bundle.contract());
        assert!(!case.matches(b"[1] "));
        assert_eq!(case.digest(), Digest::of(b"[1]"));
        assert_eq!(case.path(), "cases/a.json");
    }

    #[test]
    fn engines_as_recorded() {
        let engine = Engine::new("other", "1.0.0", EngineLimits::default()).unwrap();
        assert!(!engine.is_current());
        assert!(Engine::new("Other", "1.0.0", EngineLimits::default()).is_err());
        assert!(Engine::new("other", "1.0", EngineLimits::default()).is_err());
        let mut limits = EngineLimits {
            max_depth: 0,
            ..EngineLimits::default()
        };
        assert!(Engine::new("other", "1.0.0", limits).is_err());
        limits.max_depth = MAX_RECORDED_LIMIT + 1;
        assert!(Engine::new("other", "1.0.0", limits).is_err());
        let current = Engine::current(&Limits::default()).unwrap();
        assert!(Engine::new(current.name(), "9.9.9", *current.limits()).is_ok());
    }

    #[test]
    fn comparisons_tell_the_two_questions_apart() {
        let limits = Limits::default();
        let a = Bundle::new(rules(5), "r", "1", vec![], &limits).unwrap();
        let other_engine = Bundle {
            engine: Engine::new(ENGINE_NAME, "99.0.0", EngineLimits::default()).unwrap(),
            ..a.clone()
        };
        let other_rules = Bundle::new(rules(6), "r", "2", vec![], &limits).unwrap();
        let both = Bundle {
            engine: other_engine.engine.clone(),
            ..other_rules.clone()
        };
        let conflict = Bundle::new(rules(6), "r", "1", vec![], &limits).unwrap();

        let same = a.compare(&a);
        assert_eq!(same.relation(), Relation::SameRulesSameEngine);
        assert!(same.is_identical());
        assert_eq!(
            a.compare(&other_engine).relation(),
            Relation::SameRulesOtherEngine
        );
        assert_eq!(a.compare(&other_engine).differences(), ["engine.version"]);
        assert_eq!(
            a.compare(&other_rules).relation(),
            Relation::OtherRulesSameEngine
        );
        assert_eq!(a.compare(&other_rules).differences(), ["revision", "rules"]);
        assert_eq!(a.compare(&both).relation(), Relation::OtherRulesOtherEngine);
        assert_eq!(a.compare(&conflict).relation(), Relation::Conflict);
        assert_eq!(conflict.compare(&a).relation(), Relation::Conflict);

        let mut tighter = limits;
        tighter.max_values = 10;
        let configured = Bundle::new(rules(5), "r", "1", vec![], &tighter).unwrap();
        let comparison = a.compare(&configured);
        assert_eq!(comparison.relation(), Relation::SameRulesOtherEngine);
        assert_eq!(comparison.differences(), ["engine.limits.max_values"]);
        assert_eq!(
            crate::json::to_string(&comparison.to_json()),
            r#"{"relation":"same_rules_other_engine","identical":false,"differences":["engine.limits.max_values"]}"#
        );
        assert_eq!(Relation::Conflict.to_string(), "conflict");
    }
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use crate::agents::Catalog;
    use crate::format::{DocumentFormat, read_document, write_document_as};
    use crate::visual::{DiagramFormat, draw_document};

    const TABLE: &str = r#"{"version": 1, "decision_table": {"hit_policy": "first", "logic": "boolean", "inputs": ["x"], "outputs": [{"name": "y"}], "rows": [{"name": "big", "inputs": ["> 1"], "outputs": ["2"]}]}}"#;

    fn table_bundle() -> Bundle {
        let Document::DecisionTable(table) = read_document(TABLE, &Limits::default()).unwrap()
        else {
            panic!("a table")
        };
        Bundle::new(table, "t", "1", vec![], &Limits::default()).unwrap()
    }

    #[test]
    fn a_bundle_has_no_text_or_csv_form() {
        let document = table_bundle().into_document();
        for format in [DocumentFormat::Text, DocumentFormat::Csv] {
            let error = write_document_as(&document, format).unwrap_err();
            assert_eq!(error.code(), ErrorCode::Unsupported, "{format:?}");
        }
    }

    #[test]
    fn a_bundle_gives_and_draws_its_rules() {
        let bundle = table_bundle();
        let BundleRules::DecisionTable(table) = bundle.rules() else {
            panic!("a table")
        };
        let ruleset = bundle.clone().into_document().into_ruleset().unwrap();
        assert_eq!(ruleset.digest(), bundle.ruleset_digest());
        let limits = Limits::default();
        let drawn = |document: &Document| {
            draw_document(document, None, DiagramFormat::Html, &limits).unwrap()
        };
        assert_eq!(
            drawn(&bundle.clone().into_document()),
            drawn(&Document::DecisionTable(table.clone()))
        );
        let decision = ruleset
            .evaluate(&crate::Value::Map(crate::Map::default()))
            .unwrap();
        assert!(
            draw_document(
                &bundle.clone().into_document(),
                Some(&decision),
                DiagramFormat::Html,
                &limits
            )
            .is_ok()
        );
        let of_rules = Bundle::new(ruleset.clone(), "r", "1", vec![], &limits).unwrap();
        assert_eq!(
            draw_document(
                &of_rules.into_document(),
                None,
                DiagramFormat::Mermaid,
                &limits
            )
            .unwrap(),
            draw_document(
                &Document::RuleSet(ruleset.clone()),
                None,
                DiagramFormat::Mermaid,
                &limits
            )
            .unwrap()
        );
        let of_rules = Bundle::new(ruleset.clone(), "r", "1", vec![], &limits).unwrap();
        let error = draw_document(
            &of_rules.into_document(),
            Some(&decision),
            DiagramFormat::Html,
            &limits,
        )
        .unwrap_err();
        assert_eq!(error.code(), ErrorCode::Unsupported);
    }

    #[test]
    fn validate_reads_a_bundle() {
        let document = crate::format::write_document(&table_bundle().into_document());
        let arguments = crate::json::Json::Object(vec![("document".into(), document)]);
        let result = Catalog::new().call("validate", &arguments);
        let output = crate::json::to_string(&result.content);
        assert!(output.contains(r#""content":"bundle""#), "{output}");
        assert!(output.contains(r#""input_schema""#), "{output}");
    }
}
