//! Bundles (`spec/bundles.md`) for both front ends: making one from a rule set
//! or a decision table, what the idiomatic API reads of one, checking a case
//! file against its digest, and comparing two. The core makes, reads, writes,
//! checks and compares every bundle; this code turns JavaScript's arguments
//! into the core's and its answers into JSON for `lib/api/bundles.js`.
//!
//! A bundle is held as a document of the core (`DocumentCore`), so that the
//! native layer's `Document` writes it, and gives its canonical bytes and
//! digest, as it does any other document.

use cleanrules::bundle::{
    Bundle, BundleId, BundleRules, CaseFileRef, EngineLimit, EngineLimits, Revision,
};
use cleanrules::digest::Digest;
use cleanrules::format::Document;
use cleanrules::json::{Json, JsonNumber, to_string};
use cleanrules::rules::RuleSet;
use cleanrules::table::DecisionTable;

use super::core::{DocumentCore, RulesCore};
use super::tables::TableCore;
use super::{LIMIT_NAMES, LIMITS, Outcome, Problem, Raw, RawLimits, text};

/// One item of a bundle's `cases`, as JavaScript gives it: `{ path, content }`.
pub(crate) enum RawCase {
    /// An object: its `path` and its `content`.
    Given { path: Raw, content: Raw },
    /// Anything else, by what it is.
    Wrong(&'static str),
}

/// The limits a bundle records, by the binding's names, in the order of their
/// table (`spec/bundles.md`, *Engine*).
const RECORDED: [(EngineLimit, &str); 6] = [
    (EngineLimit::MaxSize, "maxSize"),
    (EngineLimit::MaxNesting, "maxNesting"),
    (EngineLimit::MaxDepth, "maxDepth"),
    (EngineLimit::MaxPathSegments, "maxPathSegments"),
    (EngineLimit::MaxTableSize, "maxTableSize"),
    (EngineLimit::MaxValues, "maxValues"),
];

/// The limits that bound the reading of texts, not decisions, which a bundle
/// does not record.
const NOT_RECORDED: [&str; 2] = ["maxYamlSize", "maxTextSize"];

/// The member `name` of a limits object.
fn member<'a>(values: &'a [Raw; LIMITS], name: &str) -> Outcome<&'a Raw> {
    LIMIT_NAMES
        .iter()
        .position(|known| *known == name)
        .map(|index| &values[index])
        .ok_or_else(|| Problem::Internal(format!("no limit is named {name}")))
}

/// One recorded limit as JavaScript gives it: `None` when left out; a
/// `TypeError` for anything but a number, a `RangeError` for a number that is
/// not a whole number from 0. A number beyond `u64` saturates: the core then
/// refuses it, with any other above 2^53 - 1, as `unsupported`.
fn recorded(name: &str, raw: &Raw) -> Outcome<Option<u64>> {
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
    if !(value.is_finite() && value.fract() == 0.0 && value >= 0.0) {
        return Err(Problem::Range(format!(
            "limits.{name} is {value}; a limit is a whole number from 0"
        )));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // A whole number from 0; saturates.
    Ok(Some(value as u64))
}

/// The limits a bundle is made with: those of `raw`, the others at their
/// defaults, read directly as the bundle records them (`u64`), so that every
/// build, whatever its `usize`, records the same. `maxYamlSize` and
/// `maxTextSize` bound the reading of texts, not decisions, so a bundle does
/// not record them (`spec/bundles.md`, *Engine*): giving one is a `TypeError`
/// rather than a limit silently dropped. The core checks the values
/// (`Bundle::with_engine_limits`).
fn bundle_limits(raw: &RawLimits) -> Outcome<EngineLimits> {
    let values = match raw {
        RawLimits::Absent => return Ok(EngineLimits::default()),
        RawLimits::Wrong(kind) => {
            return Err(Problem::Type(format!(
                "limits must be an object, such as {{ maxDepth: 64 }}, not {kind}"
            )));
        }
        RawLimits::Given(values) => values,
    };
    for name in NOT_RECORDED {
        if !matches!(member(values, name)?, Raw::Undefined | Raw::Null) {
            return Err(Problem::Type(format!(
                "a bundle does not record limits.{name}, which bounds the reading of texts, not decisions; \
                 it records maxSize, maxNesting, maxDepth, maxPathSegments, maxTableSize and maxValues"
            )));
        }
    }
    let mut limits = EngineLimits::default();
    for (limit, name) in RECORDED {
        if let Some(value) = recorded(name, member(values, name)?)? {
            limits = limits.with(limit, value);
        }
    }
    Ok(limits)
}

/// A string argument, such as a bundle's `id`.
fn name(raw: &Raw, what: &str, example: &str) -> Outcome<String> {
    match raw {
        Raw::String(text) => Ok(text.clone()),
        other => Err(Problem::Type(format!(
            "{what} must be a string, such as {example:?}, not {}",
            other.describe()
        ))),
    }
}

/// The case files of `cases`, in order: each path checked by the core, each
/// content's bytes digested (a string as its UTF-8 bytes).
fn case_files(cases: Vec<RawCase>) -> Outcome<Vec<CaseFileRef>> {
    cases
        .into_iter()
        .enumerate()
        .map(|(i, case)| match case {
            RawCase::Given { path, content } => {
                let path = name(&path, &format!("cases[{i}].path"), "cases/basic.json")?;
                let bytes = text(content, &format!("cases[{i}].content"))?;
                Ok(CaseFileRef::new(&path, &bytes)?)
            }
            RawCase::Wrong(kind) => Err(Problem::Type(format!(
                "cases[{i}] must be an object {{ path, content }}, not {kind}"
            ))),
        })
        .collect()
}

/// Make a bundle of `rules` with this engine (`spec/bundles.md`, *Making a
/// bundle*): the arguments' types are checked in order (`id`, `revision`,
/// `limits`, `cases`, each case path checked by the core as it is read), then
/// the core checks the identifier, the revision, that no path is given twice,
/// and the limits.
pub(crate) fn make(
    rules: BundleRules,
    id: &Raw,
    revision: &Raw,
    cases: Vec<RawCase>,
    raw_limits: &RawLimits,
) -> Outcome<DocumentCore> {
    let id = name(id, "id", "acme/discounts")?;
    let revision = name(revision, "revision", "2026-11-16.1")?;
    let limits = bundle_limits(raw_limits)?;
    let cases = case_files(cases)?;
    let bundle = Bundle::new(rules, BundleId::new(&id)?, Revision::new(&revision)?)
        .with_cases(cases)?
        .with_engine_limits(limits)?;
    Ok(DocumentCore {
        document: bundle.into_document(),
    })
}

/// The case file of `path` and `digest` (`spec/bundles.md`, *Cases*), both
/// checked: a `TypeError` for one that is not a string, then a `FormatError`
/// for a digest or a path without its form.
fn case_file(path: &Raw, digest: &Raw) -> Outcome<CaseFileRef> {
    let path = name(path, "path", "cases/basic.json")?;
    let digest = name(digest, "digest", "sha256:e3b0c442…")?;
    let digest: Digest = digest.parse()?;
    Ok(CaseFileRef::with_digest(&path, digest)?)
}

/// Check the path and digest of a `CaseFileRef` of the idiomatic API.
pub(crate) fn check_case_file_ref(path: &Raw, digest: &Raw) -> Outcome<()> {
    case_file(path, digest).map(drop)
}

/// Whether `content` (a string as its UTF-8 bytes, or bytes) is the case file
/// of `path` and `digest`: whether its digest is `digest`.
pub(crate) fn case_file_ref_digest_matches(
    path: &Raw,
    digest: &Raw,
    content: Raw,
) -> Outcome<bool> {
    let case = case_file(path, digest)?;
    Ok(case.digest_matches(&text(content, "content")?))
}

/// The rules of a document, to make a bundle of: its rule set or its decision
/// table. Any other document is a `TypeError`.
pub(crate) fn rules_of(document: &DocumentCore) -> Outcome<BundleRules> {
    match &document.document {
        Document::RuleSet(ruleset) => Ok(ruleset.clone().into()),
        Document::DecisionTable(table) => Ok(table.clone().into()),
        _ => Err(Problem::Type(format!(
            "a bundle holds a rule set or a decision table, not a {} document; \
             a single rule is given as a rule set of that rule",
            document.kind()
        ))),
    }
}

/// The rules of a rule set of the binding.
pub(crate) fn ruleset_rules(ruleset: &RulesCore) -> BundleRules {
    RuleSet::clone(&ruleset.ruleset).into()
}

/// The rules of a decision table of the binding.
pub(crate) fn table_rules(table: &TableCore) -> BundleRules {
    DecisionTable::clone(&table.table).into()
}

/// A count or a limit as a JSON number.
fn number(n: u64) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

impl DocumentCore {
    /// The bundle a bundle document holds; the JavaScript half asks only for
    /// those, so any other document is a bug.
    pub(crate) fn held(&self) -> Outcome<&Bundle> {
        match &self.document {
            Document::Bundle(bundle) => Ok(bundle),
            _ => Err(Problem::Internal(
                "the document does not hold a bundle".to_owned(),
            )),
        }
    }

    /// The bundle of a document that should hold one, for the native layer: a
    /// `TypeError` for any other document.
    fn bundle_argument(&self, what: &str) -> Outcome<&Bundle> {
        match &self.document {
            Document::Bundle(bundle) => Ok(bundle),
            _ => Err(Problem::Type(format!(
                "{what} must be a bundle document, not a {} document",
                self.kind()
            ))),
        }
    }

    /// What the idiomatic `Bundle` reads, as JSON: `[id, revision, rules,
    /// rulesetDigest, [engineName, engineVersion, limits, isCurrent],
    /// contract, cases]`, with `rules` the kind of rules it holds
    /// (`"ruleset"` or `"decision_table"`), `limits` the six recorded limits
    /// in the order of `spec/bundles.md`, `contract` its JSON form and
    /// `cases` an array of `[path, digest]`.
    pub(crate) fn bundle_data(&self) -> Outcome<String> {
        let bundle = self.held()?;
        let string = |text: &str| Json::String(text.to_owned());
        let rules = match bundle.rules() {
            BundleRules::DecisionTable(_) => "decision_table",
            // The core's rules are `#[non_exhaustive]`: a rule set otherwise.
            _ => "ruleset",
        };
        let engine = bundle.engine();
        let limits = engine.limits().iter().map(|(_, n)| number(n)).collect();
        let cases = bundle
            .cases()
            .iter()
            .map(|case| {
                Json::Array(vec![
                    string(case.path()),
                    string(&case.digest().to_string()),
                ])
            })
            .collect();
        Ok(to_string(&Json::Array(vec![
            string(bundle.id().as_str()),
            string(bundle.revision().as_str()),
            string(rules),
            string(&bundle.ruleset_digest().to_string()),
            Json::Array(vec![
                string(engine.name()),
                string(engine.version()),
                Json::Array(limits),
                Json::Bool(engine.is_current()),
            ]),
            bundle.contract().to_json(),
            Json::Array(cases),
        ])))
    }

    /// The rule set the bundle gives: the one it holds, or the one its table
    /// compiles to.
    pub(crate) fn bundle_ruleset(&self) -> Outcome<RulesCore> {
        Ok(RulesCore::new(self.held()?.ruleset().clone()))
    }

    /// The decision table the bundle holds.
    pub(crate) fn bundle_table(&self) -> Outcome<TableCore> {
        match self.held()?.rules() {
            BundleRules::DecisionTable(table) => Ok(TableCore {
                table: table.clone(),
            }),
            _ => Err(Problem::Internal(
                "the bundle holds no decision table".to_owned(),
            )),
        }
    }

    /// Whether `bytes` are the case file at `path` the bundle was made with; a
    /// `RangeError` for a path the bundle does not refer to.
    pub(crate) fn case_digest_matches(&self, path: &Raw, bytes: Raw) -> Outcome<bool> {
        let bundle = self.held()?;
        let Raw::String(path) = path else {
            return Err(Problem::Type(format!(
                "path must be a string, such as \"cases/basic.json\", not {}",
                path.describe()
            )));
        };
        let bytes = text(bytes, "the case file's content")?;
        bundle.case_digest_matches(path, &bytes).ok_or_else(|| {
            let known: Vec<String> = bundle
                .cases()
                .iter()
                .map(|case| format!("{:?}", case.path()))
                .collect();
            Problem::Range(if known.is_empty() {
                format!("the bundle refers to no case file {path:?}: it has none")
            } else {
                format!(
                    "the bundle refers to no case file {path:?}; its case files are {}",
                    known.join(", ")
                )
            })
        })
    }

    /// The comparison of this bundle with `other` (`spec/bundles.md`,
    /// *Comparing bundles*), as JSON: `relation`, `identical` and
    /// `differences`. A document that is not a bundle is a `TypeError`.
    pub(crate) fn compare_bundle(&self, other: &DocumentCore) -> Outcome<String> {
        let a = self.bundle_argument("this document")?;
        let b = other.bundle_argument("other")?;
        Ok(to_string(&a.compare(b).to_json()))
    }
}
