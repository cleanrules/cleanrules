//! Documents (`spec/json-format.md`, *Documents*): a condition, a rule, a rule
//! set or a decision table, read and written in JSON, YAML, the rule syntax
//! (`spec/text.md`) or CSV (`spec/tables.md`).

use cleanrules::format::{
    Document as CoreDocument, DocumentFormat, read_document_as, write_document_as,
};
use napi::bindgen_prelude::{Either, Uint8Array};
use napi::{Env, Result};
use napi_derive::napi;

use crate::hooks::OrThrow;
use crate::limits::{self, ReadOptions, bytes};

/// A document read from a text; immutable.
#[napi]
pub struct Document {
    pub(crate) document: CoreDocument,
}

#[napi]
impl Document {
    /// Read a document in `options.format` (`"json"`, the default, `"yaml"`,
    /// `"text"` or `"csv"`) under `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError`, `RuleSyntaxError`, `ExpressionError`,
    /// `RuleDefinitionError`, `RegistryError` or `UnsupportedError`, as the core
    /// reports them; a `RangeError` for an unknown format or a bad limit.
    #[napi(factory)]
    pub fn read(
        env: &Env,
        text: Either<String, Uint8Array>,
        options: Option<ReadOptions>,
    ) -> Result<Self> {
        let (format, limits) = limits::read_options(env, options)?;
        let document = read_document_as(bytes(&text), format, &limits).or_throw(env)?;
        Ok(Self { document })
    }

    /// What the document holds: `"condition"`, `"rule"`, `"ruleset"` or
    /// `"decision_table"`.
    #[napi(getter, ts_return_type = "DocumentKind")]
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match &self.document {
            CoreDocument::Condition(_) => "condition",
            CoreDocument::Rule(_) => "rule",
            CoreDocument::RuleSet(_) => "ruleset",
            CoreDocument::DecisionTable(_) => "decision_table",
            // The core's documents are `#[non_exhaustive]`.
            _ => "unknown",
        }
    }

    /// The document as text in `format`: JSON (the default) or YAML for every
    /// document, the rule syntax for conditions, rules and rule sets, CSV for
    /// decision tables.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for a format the document has no form in.
    #[napi(ts_args_type = "format?: Format")]
    pub fn write(&self, env: &Env, format: Option<String>) -> Result<String> {
        let format = limits::format(env, format.as_deref(), DocumentFormat::Json)?;
        write_document_as(&self.document, format).or_throw(env)
    }
}
