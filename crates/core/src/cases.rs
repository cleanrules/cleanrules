//! Test cases (`spec/tests.md`): identified inputs, and what evaluating each
//! one must give.
//!
//! A case file is a document (`{"version": 1, "cases": {"items": [...]}}`)
//! that holds a list of [`Case`]s, with an optional description and metadata,
//! read and written like every other document. Each case has an id, unique in
//! its file, an input, and an [`Expectation`] on the selected rules, the
//! outputs, the rules left unknown, or the error. Cases are run with
//! `evaluate`, never with `run`: a test never calls an action.
//!
//! ```
//! use cleanrules::cases::{Case, Cases, Expectation, ExpectedOutput};
//! use cleanrules::format::{DocumentFormat, write_document_as};
//! use cleanrules::{Decimal, Map, Value};
//!
//! let mut input = Map::new();
//! input.insert("years", Value::Int(7));
//! let mut output = Map::new();
//! output.insert("discount", Value::Decimal(Decimal::new(15, 2).unwrap()));
//! let expect = Expectation::new()
//!     .with_selected(["vip"])?
//!     .with_output(ExpectedOutput::Includes(output))?;
//! let case = Case::new("vip-gets-15", Value::Map(input))?.with_expect(expect);
//! let cases = Cases::new([case])?.with_description("discounts");
//! assert_eq!(cases.get("vip-gets-15").map(Case::id), Some("vip-gets-15"));
//!
//! let text = write_document_as(&cases.into_document(), DocumentFormat::Json)?;
//! assert!(text.starts_with(r#"{"version":1,"cases":{"description":"discounts","items":[{"id":"vip-gets-15""#));
//! # Ok::<(), cleanrules::Error>(())
//! ```

mod file;
mod run;
mod statistics;
mod text;

use std::collections::BTreeSet;

/// What a test run tested: a rule set, or a decision table. Shared with
/// audits.
pub use crate::rules::RulesSource;
pub use file::{CaseFile, CaseFileBytes};
pub use run::{
    CaseResult, CaseRun, CaseStatus, DEFAULT_MAX_EXPLAINED, ExpectationKey, Interrupted,
    Interruption, Mismatch, Runner, Summary, TestRun, TestRunFile, TestRunLimits, TestRunState,
};
pub use statistics::{
    ConditionStatistics, Coverage, CoverageMeasure, Missed, OutputStatistics,
    OutputValueStatistics, RuleStatistics, Statistics,
};

use crate::error::{Error, ErrorCode, Result};
use crate::format::Document;
use crate::value::{Map, NullCause, Value};

/// The error codes a case may expect (`spec/tests.md`, *Errors*): those that
/// evaluating an input given as JSON can raise. A slice, since [`ErrorCode`]
/// may gain codes.
pub const EXPECTED_ERROR_CODES: &[ErrorCode] = &[
    ErrorCode::MissingField,
    ErrorCode::TypeMismatch,
    ErrorCode::HitPolicyError,
];

fn format_error(message: impl Into<String>, hint: &str) -> Error {
    Error::new(ErrorCode::FormatError, message).with_hint(hint)
}

/// What a value may hold where a case file holds it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holding {
    /// An input: data, where a number out of range is null.
    Data,
    /// Metadata or an expected value: data, with no null out of range or
    /// unreadable.
    Exact,
}

/// Where in `value`, at `at`, the first value a case file cannot hold is,
/// said in words: an opaque value, or, where `holding` is exact, a null that
/// is not a plain null. Depth-first pre-order, with an explicit stack (data
/// nests as deep as the reader's limits allow).
fn not_held(value: &Value, at: &str, holding: Holding) -> Option<String> {
    let mut pending = vec![(value, at.to_owned())];
    while let Some((value, at)) = pending.pop() {
        match value {
            Value::Null(cause) if holding == Holding::Exact && *cause != NullCause::Null => {
                return Some(format!("{at} is out of range or unreadable"));
            }
            Value::Opaque(opaque) => {
                return Some(format!("{at} is not a data value: a {}", opaque.type_name));
            }
            Value::List(items) => pending.extend(
                items
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(i, item)| (item, format!("{at}[{i}]"))),
            ),
            Value::Map(map) => {
                let entries: Vec<_> = map.iter().collect();
                pending.extend(
                    entries
                        .into_iter()
                        .rev()
                        .map(|(key, item)| (item, format!("{at}[{key:?}]"))),
                );
            }
            _ => {}
        }
    }
    None
}

/// The first value of `map`, at `at`, that an exact holding refuses.
fn map_not_held(map: &Map, at: &str) -> Option<String> {
    map.iter()
        .find_map(|(key, item)| not_held(item, &format!("{at}[{key:?}]"), Holding::Exact))
}

/// A case file's content: its cases, in the order they are run, their ids
/// unique, with an optional description and metadata.
#[derive(Clone, Debug, Default)]
pub struct Cases {
    description: Option<String>,
    metadata: Map,
    items: Vec<Case>,
}

impl Cases {
    /// The cases `cases`, in this order, without description or metadata.
    ///
    /// # Errors
    ///
    /// `format_error` when two cases have the same id.
    pub fn new(cases: impl IntoIterator<Item = Case>) -> Result<Self> {
        let items: Vec<Case> = cases.into_iter().collect();
        let mut seen = BTreeSet::new();
        if let Some(case) = items.iter().find(|case| !seen.insert(case.id())) {
            return Err(repeated_id(case.id()));
        }
        Ok(Self {
            items,
            ..Self::default()
        })
    }

    /// A case file's content already checked by the reader.
    pub(crate) fn read(description: Option<String>, metadata: Map, items: Vec<Case>) -> Self {
        Self {
            description,
            metadata,
            items,
        }
    }

    /// These cases with a description of the file.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// These cases with the file's metadata: string keys mapped to data
    /// values, as a rule's metadata.
    ///
    /// # Errors
    ///
    /// `format_error` for a value that is not data: an opaque value, or a
    /// null out of range or unreadable.
    pub fn with_metadata(mut self, metadata: Map) -> Result<Self> {
        if let Some(problem) = map_not_held(&metadata, "metadata") {
            return Err(format_error(
                format!("the case file's {problem}"),
                "metadata holds data: null, booleans, numbers, strings, dates, lists and maps",
            ));
        }
        self.metadata = metadata;
        Ok(self)
    }

    /// The file's description, if it has one.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The file's metadata, empty when it has none.
    #[must_use]
    pub fn metadata(&self) -> &Map {
        &self.metadata
    }

    /// The cases, in order.
    #[must_use]
    pub fn items(&self) -> &[Case] {
        &self.items
    }

    /// The cases, in order.
    pub fn iter(&self) -> std::slice::Iter<'_, Case> {
        self.items.iter()
    }

    /// How many cases there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no cases.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The case with the id `id`, compared exactly.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Case> {
        self.items.iter().find(|case| case.id == id)
    }

    /// The case file that holds these cases.
    #[must_use]
    pub fn into_document(self) -> Document {
        Document::Cases(self)
    }
}

impl<'a> IntoIterator for &'a Cases {
    type Item = &'a Case;
    type IntoIter = std::slice::Iter<'a, Case>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

/// The error of a case id that repeats an earlier one.
pub(crate) fn repeated_id(id: &str) -> Error {
    format_error(
        format!("two cases have the id {id:?}"),
        "give each case of a file its own id",
    )
}

/// An empty case id is a `format_error`.
pub(crate) fn check_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(format_error(
            "a case id cannot be empty",
            "name the case, such as \"vip-gets-15\"",
        ));
    }
    Ok(())
}

/// One test case: an id, an input, and what evaluating it must give.
#[derive(Clone, Debug)]
pub struct Case {
    id: String,
    description: Option<String>,
    input: Value,
    expect: Expectation,
}

impl Case {
    /// A case with the id `id` and the input `input`, without expectations:
    /// it checks only that evaluation does not fail.
    ///
    /// # Errors
    ///
    /// `format_error` for an empty id, and for an input that holds an opaque
    /// value, which a case file cannot hold.
    pub fn new(id: impl Into<String>, input: Value) -> Result<Self> {
        let id = id.into();
        check_id(&id)?;
        if let Some(problem) = not_held(&input, "input", Holding::Data) {
            return Err(format_error(
                format!("in the case {id:?}, {problem}"),
                "an input holds data: null, booleans, numbers, strings, dates, lists and maps",
            ));
        }
        Ok(Self::read(id, None, input, Expectation::new()))
    }

    /// A case already checked by the reader.
    pub(crate) fn read(
        id: String,
        description: Option<String>,
        input: Value,
        expect: Expectation,
    ) -> Self {
        Self {
            id,
            description,
            input,
            expect,
        }
    }

    /// This case with a description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// This case with the expectation `expect`.
    #[must_use]
    pub fn with_expect(mut self, expect: Expectation) -> Self {
        self.expect = expect;
        self
    }

    /// The case's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The case's description, if it has one.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The input the rules evaluate.
    #[must_use]
    pub fn input(&self) -> &Value {
        &self.input
    }

    /// What evaluating the input must give.
    #[must_use]
    pub fn expect(&self) -> &Expectation {
        &self.expect
    }
}

/// An expected `output` (`spec/tests.md`, *Matching*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ExpectedOutput {
    /// `null`: no rule is selected, so there is no output.
    NothingSelected,
    /// An object: the output has each of these payload keys, with an equal
    /// value; its other keys are not checked.
    Includes(Map),
}

/// What evaluating a case's input must give (`spec/tests.md`,
/// *Expectations*). A part that is not given is not checked; an empty
/// expectation checks only that evaluation does not fail. An expected error
/// is given alone.
#[derive(Clone, Debug, Default)]
pub struct Expectation {
    selected: Option<Vec<String>>,
    output: Option<ExpectedOutput>,
    outputs: Option<Vec<Map>>,
    unknown: Option<BTreeSet<String>>,
    error: Option<ErrorCode>,
}

/// The parts of an expectation, as the reader found them.
pub(crate) struct ExpectationParts {
    pub(crate) selected: Option<Vec<String>>,
    pub(crate) output: Option<ExpectedOutput>,
    pub(crate) outputs: Option<Vec<Map>>,
    pub(crate) unknown: Option<BTreeSet<String>>,
    pub(crate) error: Option<ErrorCode>,
}

/// The error of an expected value a case file cannot hold.
fn check_expected(map: &Map, at: &str) -> Result<()> {
    match map_not_held(map, at) {
        Some(problem) => Err(format_error(
            format!("the expected {problem}"),
            "expect the value the rules give, such as a decimal, or null",
        )),
        None => Ok(()),
    }
}

impl Expectation {
    /// An empty expectation: evaluation does not fail.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An expectation already checked by the reader.
    pub(crate) fn read(parts: ExpectationParts) -> Self {
        let ExpectationParts {
            selected,
            output,
            outputs,
            unknown,
            error,
        } = parts;
        Self {
            selected,
            output,
            outputs,
            unknown,
            error,
        }
    }

    /// This expectation with the rules expected to be selected, by effective
    /// id, in selection order.
    ///
    /// # Errors
    ///
    /// `format_error` for an empty or repeated id, and with an expected error.
    pub fn with_selected<S: Into<String>>(
        mut self,
        rules: impl IntoIterator<Item = S>,
    ) -> Result<Self> {
        self.not_with_error()?;
        let rules: Vec<String> = rules.into_iter().map(Into::into).collect();
        check_rule_ids(&rules, "selected")?;
        self.selected = Some(rules);
        Ok(self)
    }

    /// This expectation with the expected single output.
    ///
    /// # Errors
    ///
    /// `format_error` with an expected error, and for a value a case file
    /// cannot hold: an opaque value, or a null out of range or unreadable.
    pub fn with_output(mut self, output: ExpectedOutput) -> Result<Self> {
        self.not_with_error()?;
        if let ExpectedOutput::Includes(map) = &output {
            check_expected(map, "output")?;
        }
        self.output = Some(output);
        Ok(self)
    }

    /// This expectation with the expected outputs of the selected rules, in
    /// selection order.
    ///
    /// # Errors
    ///
    /// As [`Expectation::with_output`].
    pub fn with_outputs(mut self, outputs: impl IntoIterator<Item = Map>) -> Result<Self> {
        self.not_with_error()?;
        let outputs: Vec<Map> = outputs.into_iter().collect();
        for (i, map) in outputs.iter().enumerate() {
            check_expected(map, &format!("outputs[{i}]"))?;
        }
        self.outputs = Some(outputs);
        Ok(self)
    }

    /// This expectation with the rules expected to be unknown, by effective
    /// id, in any order.
    ///
    /// # Errors
    ///
    /// `format_error` for an empty or repeated id, and with an expected error.
    pub fn with_unknown<S: Into<String>>(
        mut self,
        rules: impl IntoIterator<Item = S>,
    ) -> Result<Self> {
        self.not_with_error()?;
        let rules: Vec<String> = rules.into_iter().map(Into::into).collect();
        check_rule_ids(&rules, "unknown")?;
        self.unknown = Some(rules.into_iter().collect());
        Ok(self)
    }

    /// This expectation with the error that evaluation must fail with, given
    /// alone.
    ///
    /// # Errors
    ///
    /// `format_error` for a code evaluation cannot raise
    /// ([`EXPECTED_ERROR_CODES`]), and when another part is expected.
    pub fn with_error(mut self, code: ErrorCode) -> Result<Self> {
        check_error_code(code)?;
        if !self.is_empty() {
            return Err(alone());
        }
        self.error = Some(code);
        Ok(self)
    }

    fn not_with_error(&self) -> Result<()> {
        if self.error.is_some() {
            return Err(alone());
        }
        Ok(())
    }

    /// The rules expected to be selected, by effective id, in selection
    /// order; `None` when not checked.
    #[must_use]
    pub fn selected(&self) -> Option<&[String]> {
        self.selected.as_deref()
    }

    /// The expected single output; `None` when not checked.
    #[must_use]
    pub fn output(&self) -> Option<&ExpectedOutput> {
        self.output.as_ref()
    }

    /// The expected outputs of the selected rules; `None` when not checked.
    #[must_use]
    pub fn outputs(&self) -> Option<&[Map]> {
        self.outputs.as_deref()
    }

    /// The rules expected to be unknown, by effective id, sorted by code
    /// point; `None` when not checked.
    #[must_use]
    pub fn unknown(&self) -> Option<&BTreeSet<String>> {
        self.unknown.as_ref()
    }

    /// The code of the error evaluation must fail with; `None` when it must
    /// not fail.
    #[must_use]
    pub fn error(&self) -> Option<ErrorCode> {
        self.error
    }

    /// Whether nothing is checked but that evaluation does not fail.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.selected.is_none()
            && self.output.is_none()
            && self.outputs.is_none()
            && self.unknown.is_none()
            && self.error.is_none()
    }
}

/// The error of an expected error given with other expectations.
pub(crate) fn alone() -> Error {
    format_error(
        "an expected error is given alone",
        "expect either the error, or what the decision gives",
    )
}

/// The error of an expected error code that evaluation cannot raise.
pub(crate) fn check_error_code(code: ErrorCode) -> Result<()> {
    if EXPECTED_ERROR_CODES.contains(&code) {
        return Ok(());
    }
    Err(unexpected_code(code.as_str()))
}

/// The error of an expected error code `code` that evaluating an input given
/// as JSON cannot raise.
pub(crate) fn unexpected_code(code: &str) -> Error {
    let codes: Vec<&str> = EXPECTED_ERROR_CODES
        .iter()
        .map(|code| code.as_str())
        .collect();
    let hint = if code == ErrorCode::EvaluationError.as_str() {
        "expect the specific code, such as missing_field".to_owned()
    } else {
        format!("expect one of {}", codes.join(", "))
    };
    format_error(
        format!(
            "a case cannot expect {code:?}: evaluating an input given as JSON raises only {}",
            codes.join(", ")
        ),
        &hint,
    )
}

/// The error of an empty string among the rules named in `key`.
pub(crate) fn empty_rule_id(key: &str) -> Error {
    format_error(
        format!("a rule in {key:?} cannot be an empty string"),
        "name each rule by its id, or by its name when it has none",
    )
}

/// The error of the rule `id` named twice in `key`.
pub(crate) fn repeated_rule_id(id: &str, key: &str) -> Error {
    format_error(
        format!("the rule {id:?} is named twice in {key:?}"),
        "name each rule once: a rule is selected, or unknown, only once",
    )
}

fn check_rule_ids(rules: &[String], key: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in rules {
        if id.is_empty() {
            return Err(empty_rule_id(key));
        }
        if !seen.insert(id.as_str()) {
            return Err(repeated_rule_id(id, key));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Opaque;

    fn input() -> Value {
        Value::Map(Map::new())
    }

    fn opaque() -> Value {
        Value::Opaque(Opaque {
            type_name: "bytes".into(),
            token: 1,
        })
    }

    #[test]
    fn ids_are_not_empty_and_unique() {
        assert_eq!(
            Case::new("", input()).unwrap_err().code(),
            ErrorCode::FormatError
        );
        let a = Case::new("a", input()).unwrap();
        let again = Case::new("a", input()).unwrap();
        let error = Cases::new([a.clone(), again]).unwrap_err();
        assert_eq!(error.code(), ErrorCode::FormatError);
        let cases = Cases::new([a, Case::new("b", input()).unwrap()]).unwrap();
        assert_eq!(cases.len(), 2);
        assert!(!cases.is_empty());
        assert_eq!(
            cases.iter().map(Case::id).collect::<Vec<_>>(),
            ["a", "b"],
            "in order"
        );
        assert_eq!(cases.items().len(), 2);
        assert_eq!((&cases).into_iter().count(), 2);
        assert!(cases.get("c").is_none());
        assert!(Cases::default().is_empty());
    }

    #[test]
    fn a_file_has_a_description_and_metadata() {
        let mut metadata = Map::new();
        metadata.insert("owner", Value::String("risk".into()));
        let cases = Cases::new([])
            .unwrap()
            .with_description("discounts")
            .with_metadata(metadata)
            .unwrap();
        assert_eq!(cases.description(), Some("discounts"));
        assert_eq!(cases.metadata().len(), 1);
        let mut bad = Map::new();
        bad.insert("blob", opaque());
        let error = Cases::new([]).unwrap().with_metadata(bad).unwrap_err();
        assert!(error.message().contains("metadata[\"blob\"]"), "{error}");
        let mut overflow = Map::new();
        overflow.insert("n", Value::Null(NullCause::IntOverflow));
        assert!(Cases::new([]).unwrap().with_metadata(overflow).is_err());
    }

    #[test]
    fn cases_carry_their_parts() {
        let case = Case::new("a", input())
            .unwrap()
            .with_description("the first")
            .with_expect(
                Expectation::new()
                    .with_error(ErrorCode::MissingField)
                    .unwrap(),
            );
        assert_eq!(case.description(), Some("the first"));
        assert!(matches!(case.input(), Value::Map(map) if map.is_empty()));
        assert_eq!(case.expect().error(), Some(ErrorCode::MissingField));
        assert!(!case.expect().is_empty());
        let plain = Case::new("b", input()).unwrap();
        assert!(plain.expect().is_empty());
        assert_eq!(plain.description(), None);
    }

    #[test]
    fn inputs_hold_data_and_overflow_reads_as_null() {
        let mut deep = Map::new();
        deep.insert("x", Value::List(vec![Value::Int(1), opaque()]));
        let error = Case::new("blob", Value::Map(deep)).unwrap_err();
        assert_eq!(error.code(), ErrorCode::FormatError);
        assert!(error.message().contains("\"blob\""), "{error}");
        assert!(error.message().contains("input[\"x\"][1]"), "{error}");
        assert!(Case::new("n", Value::Null(NullCause::IntOverflow)).is_ok());
    }

    #[test]
    fn expected_values_hold_data_within_range() {
        let mut overflow = Map::new();
        overflow.insert("n", Value::Null(NullCause::DecimalOverflow));
        let error = Expectation::new()
            .with_output(ExpectedOutput::Includes(overflow.clone()))
            .unwrap_err();
        assert!(error.message().contains("output[\"n\"]"), "{error}");
        let error = Expectation::new()
            .with_outputs([Map::new(), overflow])
            .unwrap_err();
        assert!(error.message().contains("outputs[1][\"n\"]"), "{error}");
        let mut blob = Map::new();
        blob.insert("b", opaque());
        assert!(
            Expectation::new()
                .with_output(ExpectedOutput::Includes(blob))
                .is_err()
        );
        let mut null = Map::new();
        null.insert("n", Value::Null(NullCause::Null));
        assert!(
            Expectation::new()
                .with_output(ExpectedOutput::Includes(null))
                .is_ok()
        );
    }

    #[test]
    fn an_error_is_expected_alone() {
        let error = Expectation::new()
            .with_error(ErrorCode::HitPolicyError)
            .unwrap();
        for result in [
            error.clone().with_selected(["a"]),
            error.clone().with_output(ExpectedOutput::NothingSelected),
            error.clone().with_outputs([]),
            error.clone().with_unknown(["a"]),
        ] {
            assert_eq!(result.unwrap_err().code(), ErrorCode::FormatError);
        }
        let selected = Expectation::new().with_selected(["a"]).unwrap();
        assert!(selected.with_error(ErrorCode::MissingField).is_err());
    }

    #[test]
    fn only_evaluation_errors_are_expected() {
        for code in ErrorCode::ALL {
            assert_eq!(
                Expectation::new().with_error(code).is_ok(),
                EXPECTED_ERROR_CODES.contains(&code),
                "{code}"
            );
        }
        let error = Expectation::new()
            .with_error(ErrorCode::EvaluationError)
            .unwrap_err();
        assert!(error.message().contains("raises only"), "{error}");
        assert_eq!(
            error.hint(),
            Some("expect the specific code, such as missing_field")
        );
    }

    #[test]
    fn rules_are_named_once_and_not_empty() {
        assert!(Expectation::new().with_selected(["a", "a"]).is_err());
        assert!(Expectation::new().with_selected([""]).is_err());
        assert!(Expectation::new().with_unknown(["b", "b"]).is_err());
        let expect = Expectation::new()
            .with_selected(["b", "a"])
            .unwrap()
            .with_unknown(["z", "c"])
            .unwrap()
            .with_outputs([Map::new()])
            .unwrap()
            .with_output(ExpectedOutput::Includes(Map::new()))
            .unwrap();
        assert_eq!(
            expect.selected(),
            Some(&["b".to_owned(), "a".to_owned()][..])
        );
        assert_eq!(
            expect.unknown().unwrap().iter().collect::<Vec<_>>(),
            ["c", "z"],
            "sorted by code point"
        );
        assert_eq!(expect.outputs().map(<[Map]>::len), Some(1));
        assert!(matches!(
            expect.output(),
            Some(ExpectedOutput::Includes(map)) if map.is_empty()
        ));
        assert_eq!(expect.error(), None);
    }
}
