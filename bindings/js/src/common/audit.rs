//! The static audit (`spec/audit.md`) for both front ends: the core audits a
//! rule set or a decision table; this code reads the options and writes the
//! report, as a document or text for the native layer (`RuleSet.audit`,
//! `Document.audit`), and with its witnesses in the encoding of `out` for the
//! idiomatic API (`lib/api/results.js`, `Audit`).

use cleanrules::audit::{Audit, AuditLimits, Severity, audit, audit_document, audit_table};
use cleanrules::format::{DocumentFormat, write_audit, write_audit_as};
use cleanrules::json::{Json, JsonNumber, to_string};
use cleanrules::rules::RuleSet;
use cleanrules::table::DecisionTable;

use super::core::DocumentCore;
use super::{Outcome, Problem, Raw, flag, format, out, whole};

/// The options of an audit as JavaScript gives them, each member read once:
/// `{ format, limits: { maxTerms, maxSteps }, rulesetDigest }`.
#[derive(Clone, Debug, Default)]
pub(crate) enum RawAuditOptions {
    /// `undefined` or `null`: the defaults.
    #[default]
    Absent,
    /// An object: its members.
    Given {
        format: Raw,
        limits: RawAuditLimits,
        ruleset_digest: Raw,
    },
    /// Anything else, by what it is.
    Wrong(&'static str),
}

/// The `limits` of an audit's options, as JavaScript gives them.
#[derive(Clone, Debug, Default)]
pub(crate) enum RawAuditLimits {
    /// `undefined` or `null`: the defaults.
    #[default]
    Absent,
    /// An object: `maxTerms` and `maxSteps`, read once; other members are
    /// ignored, as in every limits object.
    Given { max_terms: Raw, max_steps: Raw },
    /// Anything else, by what it is.
    Wrong(&'static str),
}

/// An audit's options, checked.
struct AuditOptions {
    format: DocumentFormat,
    limits: AuditLimits,
    ruleset_digest: bool,
}

fn options(raw: &RawAuditOptions) -> Outcome<AuditOptions> {
    let mut checked = AuditOptions {
        format: DocumentFormat::Text,
        limits: AuditLimits::default(),
        ruleset_digest: false,
    };
    match raw {
        RawAuditOptions::Absent => {}
        RawAuditOptions::Wrong(kind) => {
            return Err(Problem::Type(format!(
                "options must be an object, such as {{ limits: {{ maxSteps: 10000 }} }}, not {kind}"
            )));
        }
        RawAuditOptions::Given {
            format: given,
            limits,
            ruleset_digest,
        } => {
            checked.format = format(given, DocumentFormat::Text)?;
            match limits {
                RawAuditLimits::Absent => {}
                RawAuditLimits::Wrong(kind) => {
                    return Err(Problem::Type(format!(
                        "limits must be an object, such as {{ maxSteps: 10000 }}, not {kind}"
                    )));
                }
                RawAuditLimits::Given {
                    max_terms,
                    max_steps,
                } => {
                    if let Some(terms) = whole("limits.maxTerms", max_terms)? {
                        checked.limits = checked.limits.with_max_terms(terms);
                    }
                    if let Some(steps) = whole("limits.maxSteps", max_steps)? {
                        checked.limits = checked.limits.with_max_steps(steps);
                    }
                }
            }
            checked.ruleset_digest = flag(ruleset_digest, "options.rulesetDigest")?;
        }
    }
    Ok(checked)
}

/// The report of a rule set's audit.
fn of_ruleset(ruleset: &RuleSet, options: &AuditOptions) -> Outcome<Audit> {
    Ok(audit(ruleset, &options.limits)?.with_ruleset_digest(options.ruleset_digest))
}

/// The report of a decision table's audit.
fn of_table(table: &DecisionTable, options: &AuditOptions) -> Outcome<Audit> {
    Ok(audit_table(table, &options.limits)?.with_ruleset_digest(options.ruleset_digest))
}

/// `RuleSet.audit` of the native layer: the text (the default) or an audit
/// document in JSON or YAML.
pub(crate) fn ruleset_as(ruleset: &RuleSet, raw: &RawAuditOptions) -> Outcome<String> {
    let options = options(raw)?;
    Ok(write_audit_as(
        &of_ruleset(ruleset, &options)?,
        options.format,
    )?)
}

/// `Document.audit` of the native layer: the core's audit of the rules the
/// document holds (`audit_document`), whose pointers point into it: a rule
/// set, a decision table, a bundle (under `/bundle`) or a rule (under
/// `/rule`); `unsupported` for a condition or a case file.
pub(crate) fn document_as(document: &DocumentCore, raw: &RawAuditOptions) -> Outcome<String> {
    let options = options(raw)?;
    let report = audit_document(&document.document, &options.limits)?
        .with_ruleset_digest(options.ruleset_digest);
    Ok(write_audit_as(&report, options.format)?)
}

/// A rule set's audit for the idiomatic API (`data`).
pub(crate) fn ruleset_data(ruleset: &RuleSet, raw: &RawAuditOptions) -> Outcome<String> {
    Ok(data(&of_ruleset(ruleset, &options(raw)?)?))
}

/// A decision table's audit for the idiomatic API (`data`).
pub(crate) fn table_data(table: &DecisionTable, raw: &RawAuditOptions) -> Outcome<String> {
    Ok(data(&of_table(table, &options(raw)?)?))
}

/// What the idiomatic API reads of a report, as one JSON text:
/// `[document, witnesses, maxSeverity, text, texts, atLeast]`, with the
/// audit document made stringifiable (`out::stringifiable`), each
/// diagnostic's witness in the encoding of `out` (or `null`), the most
/// serious severity (or `null`), the text, each diagnostic's text, and
/// whether the report is at least each severity (`{"info": true, ...}`), as
/// the core orders them.
fn data(report: &Audit) -> String {
    let mut document = Json::Object(vec![
        (
            "version".to_owned(),
            Json::Number(JsonNumber::from_literal(
                cleanrules::FORMAT_VERSION.to_string(),
            )),
        ),
        ("audit".to_owned(), write_audit(report)),
    ]);
    out::stringifiable(&mut document);
    let witnesses = report
        .diagnostics
        .iter()
        .map(|d| d.witness.as_ref().map_or(Json::Null, out::value))
        .collect();
    let severity = report
        .max_severity()
        .map_or(Json::Null, |s| Json::String(s.as_str().to_owned()));
    let texts = report
        .diagnostics
        .iter()
        .map(|d| Json::String(report.diagnostic_text(d)))
        .collect();
    let at_least = [Severity::Info, Severity::Warning, Severity::Error]
        .into_iter()
        .map(|s| (s.as_str().to_owned(), Json::Bool(report.at_least(s))))
        .collect();
    to_string(&Json::Array(vec![
        document,
        Json::Array(witnesses),
        severity,
        Json::String(report.to_string()),
        Json::Array(texts),
        Json::Object(at_least),
    ]))
}
