//! Single condition and value nodes (`spec/ast.md`, *Evaluating one node*),
//! read in JSON, YAML or the rule syntax, and evaluated with their result
//! trees.

use cleanrules::ast::Node as CoreNode;
use cleanrules::eval::{Options, evaluate_tree, evaluate_value_tree};
use cleanrules::format::{
    DocumentFormat, ValueContext, parse_as, read_node, read_value, write_node, write_result,
};
use cleanrules::json::{parse_bytes, to_string, utf8_text};
use cleanrules::logic::{Kleene, LogicName};
use cleanrules::{Error, ErrorCode, Value};
use napi::bindgen_prelude::{Array, Either, JsValue, This, Uint8Array, Unknown};
use napi::{Env, Result};
use napi_derive::napi;

use crate::hooks::OrThrow;
use crate::limits::{self, Limits, ReadOptions, Text, bytes};
use crate::read::{chained, read};
use crate::ruleset::paths_to_js;

/// A condition or value node; immutable.
#[napi]
pub struct Node {
    node: CoreNode,
}

fn not_csv(format: DocumentFormat) -> Error {
    Error::new(
        ErrorCode::Unsupported,
        format!("a node has no {} form", format.as_str()),
    )
    .with_hint("use JSON, YAML or text")
    .unsupported(format.as_str(), "CSV holds decision tables")
}

/// The logic that evaluates one node: only `"boolean"` for now
/// (`spec/ast.md`, *Evaluating one node*).
fn logic(name: Option<&str>) -> cleanrules::Result<Kleene> {
    let name = name.unwrap_or("boolean");
    match LogicName::from_name(name)? {
        LogicName::Boolean => Ok(Kleene),
        #[allow(unreachable_patterns)] // Graded logics arrive in phase J.
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            format!("the logic {name:?} cannot evaluate one node yet"),
        )
        .unsupported(
            "evaluating one node",
            "only boolean logic evaluates one node",
        )),
    }
}

impl Node {
    /// The result tree in JSON: of a condition with `logic`, or of a value
    /// when `logic` is `None`.
    fn tree(
        &self,
        input: &Value,
        logic: Option<Kleene>,
        strict: bool,
    ) -> cleanrules::Result<String> {
        let options = Options { strict, rule: None };
        let tree = match logic {
            Some(logic) => evaluate_tree(&self.node, input, &logic, options)?,
            None => evaluate_value_tree(&self.node, input, &Kleene, options)?,
        };
        Ok(to_string(&write_result(&tree)))
    }

    fn native<'env>(
        &self,
        env: &'env Env,
        this: &This<'env>,
        data: Unknown<'env>,
        logic: Option<Kleene>,
        strict: bool,
    ) -> Result<String> {
        let native = read(env, data, JsValue::to_unknown(&**this))?;
        self.tree(&native.value, logic, strict)
            .map_err(|error| chained(env, &error, &native))
    }

    fn json(
        &self,
        env: &Env,
        input: &Text,
        limits: Option<Limits>,
        logic: Option<Kleene>,
        strict: bool,
    ) -> Result<String> {
        let limits = limits::of(env, limits)?;
        let input = parse_bytes(bytes(input), &limits)
            .and_then(|json| read_value(&json, "", ValueContext::Data))
            .or_throw(env)?;
        self.tree(&input, logic, strict).or_throw(env)
    }
}

/// How one condition node is evaluated (`spec/ast.md`, *Evaluating one
/// node*).
#[napi(object)]
#[derive(Clone, Debug, Default)]
pub struct TreeOptions {
    /// The logic: only `"boolean"`, the default, for now.
    pub logic: Option<String>,
    /// Whether evaluation is strict; `false` by default.
    pub strict: Option<bool>,
    /// The limits for an input given as JSON; ignored for native data.
    pub limits: Option<Limits>,
}

/// How one value node is evaluated.
#[napi(object)]
#[derive(Clone, Debug, Default)]
pub struct ValueTreeOptions {
    /// Whether evaluation is strict; `false` by default.
    pub strict: Option<bool>,
    /// The limits for an input given as JSON; ignored for native data.
    pub limits: Option<Limits>,
}

#[napi]
impl Node {
    /// Read a node (`spec/json-format.md`, *Conditions*, not a document) in
    /// `options.format` (`"json"`, the default, `"yaml"` or `"text"`, an
    /// expression of any kind) under `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError`, `RuleSyntaxError` or `ExpressionError`;
    /// `UnsupportedError` for CSV.
    #[napi(factory)]
    pub fn read(
        env: &Env,
        text: Either<String, Uint8Array>,
        options: Option<ReadOptions>,
    ) -> Result<Self> {
        let (format, limits) = limits::read_options(env, options)?;
        let text = bytes(&text);
        let node = match format {
            DocumentFormat::Text => {
                utf8_text(text).and_then(|text| cleanrules::text::parse_node(text, &limits))
            }
            DocumentFormat::Csv => Err(not_csv(format)),
            other => parse_as(text, other, &limits).and_then(|json| read_node(&json, "", &limits)),
        }
        .or_throw(env)?;
        Ok(Self { node })
    }

    /// The node in `format`: JSON (the default), YAML or the rule syntax.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for CSV.
    #[napi(ts_args_type = "format?: Format")]
    pub fn write(&self, env: &Env, format: Option<String>) -> Result<String> {
        Ok(
            match limits::format(env, format.as_deref(), DocumentFormat::Json)? {
                DocumentFormat::Json => to_string(&write_node(&self.node)),
                DocumentFormat::Yaml => cleanrules::yaml::to_string(&write_node(&self.node)),
                DocumentFormat::Text => self.node.to_string(),
                other => return Err(not_csv(other)).or_throw(env),
            },
        )
    }

    /// The field paths the node reads, in order of first appearance.
    ///
    /// # Errors
    ///
    /// If the arrays cannot be made.
    #[napi(ts_return_type = "Array<Array<string | number>>")]
    pub fn field_paths<'env>(&self, env: &'env Env) -> Result<Array<'env>> {
        paths_to_js(env, self.node.field_paths())
    }

    /// Evaluate the node as a condition on native data: its result tree in
    /// JSON (`spec/json-format.md`, *Result trees*).
    ///
    /// # Errors
    ///
    /// `RegistryError` for an unknown logic, `UnsupportedError` for a graded
    /// one; in strict mode, `EvaluationError` and its subclasses.
    #[napi(ts_args_type = "data: unknown, options?: TreeOptions")]
    pub fn evaluate_tree<'env>(
        &self,
        env: &'env Env,
        this: This<'env>,
        data: Unknown<'env>,
        options: Option<TreeOptions>,
    ) -> Result<String> {
        let options = options.unwrap_or_default();
        let logic = self::logic(options.logic.as_deref()).or_throw(env)?;
        self.native(
            env,
            &this,
            data,
            Some(logic),
            options.strict.unwrap_or(false),
        )
    }

    /// Evaluate the node as a value on native data: its result tree in JSON.
    ///
    /// # Errors
    ///
    /// In strict mode, `EvaluationError` and its subclasses.
    #[napi(ts_args_type = "data: unknown, options?: ValueTreeOptions")]
    pub fn evaluate_value_tree<'env>(
        &self,
        env: &'env Env,
        this: This<'env>,
        data: Unknown<'env>,
        options: Option<ValueTreeOptions>,
    ) -> Result<String> {
        let strict = options.and_then(|o| o.strict).unwrap_or(false);
        self.native(env, &this, data, None, strict)
    }

    /// `evaluateTree` on an input given as a JSON value, under
    /// `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, and as `evaluateTree`.
    #[napi]
    pub fn evaluate_tree_json(
        &self,
        env: &Env,
        input: Either<String, Uint8Array>,
        options: Option<TreeOptions>,
    ) -> Result<String> {
        let options = options.unwrap_or_default();
        let logic = self::logic(options.logic.as_deref()).or_throw(env)?;
        let strict = options.strict.unwrap_or(false);
        self.json(env, &input, options.limits, Some(logic), strict)
    }

    /// `evaluateValueTree` on an input given as a JSON value, under
    /// `options.limits`.
    ///
    /// # Errors
    ///
    /// `FormatError` for an invalid text, and as `evaluateValueTree`.
    #[napi]
    pub fn evaluate_value_tree_json(
        &self,
        env: &Env,
        input: Either<String, Uint8Array>,
        options: Option<ValueTreeOptions>,
    ) -> Result<String> {
        let options = options.unwrap_or_default();
        let strict = options.strict.unwrap_or(false);
        self.json(env, &input, options.limits, None, strict)
    }
}
