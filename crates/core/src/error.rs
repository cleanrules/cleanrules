//! The core's error: a stable code, a message, a hint and context
//! (`spec/errors.md`).

use std::fmt;

use crate::value::Group;

/// The stable code of an error, the same in every language.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ErrorCode {
    /// A document's JSON or shape is invalid.
    FormatError,
    /// A document's `version` is missing or unknown.
    UnsupportedVersion,
    /// A document is too large or nested too deep.
    LimitExceeded,
    /// A decision does not belong to the rule set it is read with.
    DecisionMismatch,
    /// A condition node breaks a construction rule.
    ExpressionError,
    /// A rule or rule set breaks a construction rule.
    RuleDefinitionError,
    /// The text DSL cannot be parsed.
    RuleSyntaxError,
    /// A name is not registered.
    RegistryError,
    /// A rule is looked up by a name the rule set does not have.
    RuleNotFound,
    /// Strict mode met a value it cannot read, or a batch is not a collection.
    EvaluationError,
    /// Strict mode met a field whose path cannot be resolved.
    MissingField,
    /// Strict mode met values that cannot be compared.
    TypeMismatch,
    /// The hit policy cannot decide.
    HitPolicyError,
    /// An action raised during `run`.
    ActionError,
    /// This build, this logic or this format version cannot do the operation.
    Unsupported,
}

impl ErrorCode {
    /// Every code, in the order of `spec/errors.md`.
    pub const ALL: [ErrorCode; 15] = [
        ErrorCode::FormatError,
        ErrorCode::UnsupportedVersion,
        ErrorCode::LimitExceeded,
        ErrorCode::DecisionMismatch,
        ErrorCode::ExpressionError,
        ErrorCode::RuleDefinitionError,
        ErrorCode::RuleSyntaxError,
        ErrorCode::RegistryError,
        ErrorCode::RuleNotFound,
        ErrorCode::EvaluationError,
        ErrorCode::MissingField,
        ErrorCode::TypeMismatch,
        ErrorCode::HitPolicyError,
        ErrorCode::ActionError,
        ErrorCode::Unsupported,
    ];

    /// The code as written in the spec and in conformance vectors.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::FormatError => "format_error",
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::LimitExceeded => "limit_exceeded",
            ErrorCode::DecisionMismatch => "decision_mismatch",
            ErrorCode::ExpressionError => "expression_error",
            ErrorCode::RuleDefinitionError => "rule_definition_error",
            ErrorCode::RuleSyntaxError => "rule_syntax_error",
            ErrorCode::RegistryError => "registry_error",
            ErrorCode::RuleNotFound => "rule_not_found",
            ErrorCode::EvaluationError => "evaluation_error",
            ErrorCode::MissingField => "missing_field",
            ErrorCode::TypeMismatch => "type_mismatch",
            ErrorCode::HitPolicyError => "hit_policy_error",
            ErrorCode::ActionError => "action_error",
            ErrorCode::Unsupported => "unsupported",
        }
    }

    /// The code of the parent class in the hierarchy, below `CleanRulesError`:
    /// `missing_field` is an `evaluation_error`, `unsupported_version` a
    /// `format_error`. `None` for the codes directly under the base.
    #[must_use]
    pub fn parent(self) -> Option<ErrorCode> {
        match self {
            ErrorCode::UnsupportedVersion
            | ErrorCode::LimitExceeded
            | ErrorCode::DecisionMismatch => Some(ErrorCode::FormatError),
            ErrorCode::MissingField | ErrorCode::TypeMismatch | ErrorCode::HitPolicyError => {
                Some(ErrorCode::EvaluationError)
            }
            _ => None,
        }
    }

    /// Whether this code is `ancestor` or below it in the hierarchy.
    #[must_use]
    pub fn is_a(self, ancestor: ErrorCode) -> bool {
        self == ancestor || self.parent().is_some_and(|parent| parent.is_a(ancestor))
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One segment of a field path: a map key or a list index.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Segment {
    /// A map key, or a record's component.
    Key(String),
    /// A list index, or a record's position.
    Index(u64),
}

/// The context of an error; which fields apply depends on the code
/// (`spec/errors.md`, *Codes*).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Context {
    /// A JSON Pointer into the document as written.
    pub pointer: Option<String>,
    /// 1-based line, in a text.
    pub line: Option<u64>,
    /// 1-based column, in Unicode code points.
    pub column: Option<u64>,
    /// The rule concerned.
    pub rule: Option<String>,
    /// The rules concerned, in rule order.
    pub rules: Vec<String>,
    /// The full path of the field being read.
    pub path: Option<Vec<Segment>>,
    /// The type groups involved.
    pub types: Vec<Group>,
    /// The position of the input in a batch.
    pub row: Option<u64>,
    /// The name looked up.
    pub name: Option<String>,
    /// The names that exist, sorted.
    pub available: Vec<String>,
    /// The rules whose actions had run.
    pub executed: Vec<String>,
}

/// An error of cleanrules.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    code: ErrorCode,
    message: String,
    hint: Option<String>,
    context: Context,
    #[source]
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl Error {
    /// A new error with a code and a message that names what failed.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            context: Context::default(),
            source: None,
        }
    }

    /// Add a hint: how to fix it.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Set the context.
    #[must_use]
    pub fn with_context(mut self, context: Context) -> Self {
        self.context = context;
        self
    }

    /// Set the JSON Pointer.
    #[must_use]
    pub fn at_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.context.pointer = Some(pointer.into());
        self
    }

    /// Set the 1-based line and column.
    #[must_use]
    pub fn at_position(mut self, line: u64, column: u64) -> Self {
        self.context.line = Some(line);
        self.context.column = Some(column);
        self
    }

    /// Keep the error that caused this one.
    #[must_use]
    pub fn caused_by(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// The stable code.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The hint, if any.
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The context.
    #[must_use]
    pub fn context(&self) -> &Context {
        &self.context
    }
}

/// The result type of the core.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::{Error, ErrorCode};

    #[test]
    fn codes_match_the_spec() {
        let names: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            names,
            [
                "format_error",
                "unsupported_version",
                "limit_exceeded",
                "decision_mismatch",
                "expression_error",
                "rule_definition_error",
                "rule_syntax_error",
                "registry_error",
                "rule_not_found",
                "evaluation_error",
                "missing_field",
                "type_mismatch",
                "hit_policy_error",
                "action_error",
                "unsupported",
            ]
        );
    }

    #[test]
    fn hierarchy() {
        assert!(ErrorCode::MissingField.is_a(ErrorCode::EvaluationError));
        assert!(ErrorCode::HitPolicyError.is_a(ErrorCode::EvaluationError));
        assert!(ErrorCode::UnsupportedVersion.is_a(ErrorCode::FormatError));
        assert!(!ErrorCode::ExpressionError.is_a(ErrorCode::FormatError));
        assert_eq!(ErrorCode::Unsupported.parent(), None);
    }

    #[test]
    fn context_and_source() {
        let cause = std::io::Error::other("disk on fire");
        let error = Error::new(ErrorCode::EvaluationError, "cannot read customer.age")
            .with_hint("check the data")
            .at_pointer("/condition")
            .caused_by(cause);
        assert_eq!(error.to_string(), "cannot read customer.age");
        assert_eq!(error.hint(), Some("check the data"));
        assert_eq!(error.context().pointer.as_deref(), Some("/condition"));
        assert!(std::error::Error::source(&error).is_some());
    }
}
