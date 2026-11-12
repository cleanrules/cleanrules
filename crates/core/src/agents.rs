//! Tools for AI agents (`spec/agents.md`).
//!
//! A [`Catalog`] of named rule sets answers a tool call, a tool name and its
//! arguments as JSON, with a result or an error, as JSON. The MCP server and
//! the adapters for agent frameworks pass both through, so every language
//! gives the same answer to the same call. Tools only read rule sets and
//! evaluate them: they never run actions and never change a rule set.
//!
//! ```
//! use cleanrules::agents::Catalog;
//! use cleanrules::json::{Limits, to_string};
//!
//! let text = "RULE adult IF age >= 18 THEN ok = TRUE";
//! let rules = cleanrules::text::parse(text, &Limits::default())?.into_ruleset()?;
//! let mut catalog = Catalog::new();
//! catalog.insert("adults", rules);
//! let result = catalog.call_text("evaluate", r#"{"ruleset": "adults", "input": {"age": 20}}"#);
//! assert!(!result.is_error);
//! assert_eq!(
//!     to_string(&result.content),
//!     r#"{"ruleset":"adults","selected":["adult"],"unknown":[],"output":{"ok":true},"outputs":[{"ok":true}]}"#
//! );
//! # Ok::<(), cleanrules::Error>(())
//! ```

mod compact;
mod schema;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use compact::budget_error;
pub use compact::{
    CompactExplanation, DEFAULT_BUDGET, MAX_BUDGET, MAX_LINE, MIN_BUDGET, compact_explanation,
};
pub use schema::{MAX_PREFIX_ITEMS, input_schema};

use crate::error::{Error, ErrorCode, Result, Segment};
use crate::explain::{Summary, explain, summary};
use crate::format::{
    Document, DocumentFormat, ValueContext, read_document_as, read_parsed_document, read_value,
    write_error, write_summary, write_value,
};
use crate::json::{Json, JsonNumber, Limits, parse, pointer_token};
use crate::logic::Truth;
use crate::rules::{Decision, RuleSet};
use crate::value::{Map, Value};

/// The tools of `spec/agents.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Tool {
    /// The rule sets of the catalog.
    ListRulesets,
    /// What a rule set reads and decides, and its input schema.
    Summary,
    /// A decision's outcome.
    Evaluate,
    /// A decision's outcome and its compact explanation.
    Explain,
    /// Whether a document reads, and what it holds.
    Validate,
}

impl Tool {
    /// Every tool, sorted by name.
    pub const ALL: &'static [Tool] = &[
        Tool::Evaluate,
        Tool::Explain,
        Tool::ListRulesets,
        Tool::Summary,
        Tool::Validate,
    ];

    /// The tool's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Tool::ListRulesets => "list_rulesets",
            Tool::Summary => "summary",
            Tool::Evaluate => "evaluate",
            Tool::Explain => "explain",
            Tool::Validate => "validate",
        }
    }

    /// The tool with this name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|t| t.as_str() == name)
    }

    /// The members its arguments take, in the order of the spec's table.
    fn members(self) -> &'static [Member] {
        const RULESET: Member = Member {
            key: "ruleset",
            shape: Shape::String,
            required: true,
        };
        const INPUT: Member = Member {
            key: "input",
            shape: Shape::Any,
            required: true,
        };
        match self {
            Tool::ListRulesets => &[],
            Tool::Summary => &[RULESET],
            Tool::Evaluate => &[RULESET, INPUT],
            Tool::Explain => &[
                RULESET,
                INPUT,
                Member {
                    key: "budget",
                    shape: Shape::Budget,
                    required: false,
                },
            ],
            Tool::Validate => &[
                Member {
                    key: "document",
                    shape: Shape::Document,
                    required: true,
                },
                Member {
                    key: "format",
                    shape: Shape::String,
                    required: false,
                },
            ],
        }
    }
}

/// What a tool call gives: a result, or a tool error (`{"error": ...}`).
///
/// An MCP server returns `content` as the result's structured content and its
/// JSON text as the text content, marking a tool error as one
/// (`spec/agents.md`, *Errors*).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ToolResult {
    /// Whether it is a tool error.
    pub is_error: bool,
    /// The result, or `{"error": <agent error>}`: always a JSON object.
    pub content: Json,
}

impl ToolResult {
    fn ok(content: Json) -> Self {
        Self {
            is_error: false,
            content,
        }
    }

    fn error(error: &Error) -> Self {
        Self {
            is_error: true,
            content: Json::Object(vec![("error".into(), agent_error(error))]),
        }
    }
}

/// A rule set of a catalog, with which of its rules have an action given as
/// code of the host language, which the core's rules never hold.
#[derive(Clone, Debug)]
struct Entry {
    ruleset: Arc<RuleSet>,
    actions: Vec<bool>,
}

impl Entry {
    fn summary(&self) -> Summary<'_> {
        summary(&self.ruleset).with_actions(self.actions.iter().copied())
    }
}

/// Named rule sets that answer tool calls (`spec/agents.md`, *Tools*). Names
/// are listed sorted by Unicode code point. A catalog offers every tool unless
/// [`Catalog::with_tools`] restricts it.
///
/// ```
/// use cleanrules::agents::{Catalog, Tool};
/// use cleanrules::json::Limits;
///
/// let rules = cleanrules::text::parse("RULE a IF x > 1", &Limits::default())?.into_ruleset()?;
/// let catalog: Catalog = [("checks", rules)].into_iter().collect();
/// let catalog = catalog.with_tools([Tool::Evaluate, Tool::ListRulesets]);
/// assert_eq!(catalog.names().collect::<Vec<_>>(), ["checks"]);
/// assert!(catalog.call_text("validate", r#"{"document": "x", "format": "text"}"#).is_error);
/// # Ok::<(), cleanrules::Error>(())
/// ```
#[derive(Clone, Debug)]
pub struct Catalog {
    rulesets: BTreeMap<String, Entry>,
    tools: Vec<Tool>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            rulesets: BTreeMap::new(),
            tools: Tool::ALL.to_vec(),
        }
    }
}

// A server shares one catalog between the threads that answer calls.
const _: fn() = || {
    fn shared<T: Send + Sync>() {}
    shared::<Catalog>();
};

impl Catalog {
    /// An empty catalog that offers every tool.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The same catalog offering only `tools` (sorted by name, each once): a
    /// call to another is a `registry_error` naming the ones it offers.
    #[must_use]
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        let mut tools: Vec<Tool> = tools.into_iter().collect();
        tools.sort_by_key(|t| t.as_str());
        tools.dedup();
        self.tools = tools;
        self
    }

    /// The tools it offers, sorted by name.
    #[must_use]
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// Add a rule set under `name`, whose actions are all named or absent; the
    /// one it replaces, if any.
    pub fn insert(
        &mut self,
        name: impl Into<String>,
        ruleset: impl Into<Arc<RuleSet>>,
    ) -> Option<Arc<RuleSet>> {
        self.insert_with_actions(name, ruleset, [])
    }

    /// Add a rule set under `name`, saying which of its rules, in rule order,
    /// have an action given as code of the host language (as
    /// [`Summary::with_actions`]): `summary` shows them. The one it replaces,
    /// if any.
    pub fn insert_with_actions(
        &mut self,
        name: impl Into<String>,
        ruleset: impl Into<Arc<RuleSet>>,
        actions: impl IntoIterator<Item = bool>,
    ) -> Option<Arc<RuleSet>> {
        let entry = Entry {
            ruleset: ruleset.into(),
            actions: actions.into_iter().collect(),
        };
        self.rulesets.insert(name.into(), entry).map(|e| e.ruleset)
    }

    /// Remove the rule set named `name`; it, if there was one.
    pub fn remove(&mut self, name: &str) -> Option<Arc<RuleSet>> {
        self.rulesets.remove(name).map(|e| e.ruleset)
    }

    /// The rule set named `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RuleSet> {
        self.rulesets.get(name).map(|e| e.ruleset.as_ref())
    }

    /// The names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.rulesets.keys().map(String::as_str)
    }

    /// How many rule sets it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rulesets.len()
    }

    /// Whether it holds no rule set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rulesets.is_empty()
    }

    /// Answer a call whose arguments are JSON text, parsed with the default
    /// limits as documents are (`spec/json-format.md`, *Errors and limits*): a
    /// syntax error is a tool error with its line and column.
    #[must_use]
    pub fn call_text(&self, tool: &str, arguments: &str) -> ToolResult {
        match parse(arguments, &Limits::default()) {
            Ok(json) => self.call(tool, &json),
            Err(error) => ToolResult::error(&error),
        }
    }

    /// Answer a call (`spec/agents.md`).
    #[must_use]
    pub fn call(&self, tool: &str, arguments: &Json) -> ToolResult {
        let Some(tool) = Tool::from_name(tool).filter(|t| self.tools.contains(t)) else {
            let mut error = Error::new(
                ErrorCode::RegistryError,
                format!("there is no tool {tool:?}"),
            );
            error.context_mut().name = Some(tool.to_owned());
            error.context_mut().available = self.tools.iter().map(|t| t.as_str().into()).collect();
            return ToolResult::error(&error);
        };
        match self.answer(tool, arguments) {
            Ok(content) => ToolResult::ok(content),
            Err(error) => ToolResult::error(&error),
        }
    }

    fn answer(&self, tool: Tool, arguments: &Json) -> Result<Json> {
        let args = Arguments::read(tool, arguments)?;
        match tool {
            Tool::ListRulesets => Ok(self.list()),
            Tool::Summary => {
                let (name, entry) = self.lookup(&args)?;
                let overview = entry.summary();
                Ok(object(vec![
                    ("ruleset", string(name)),
                    ("summary", write_summary(&overview)),
                    ("input_schema", input_schema(&overview)),
                ]))
            }
            Tool::Evaluate | Tool::Explain => {
                let (name, entry) = self.lookup(&args)?;
                let input_json = args.get("input").unwrap_or(&NULL);
                let input = read_value(input_json, "/input", ValueContext::Data)?;
                let decision = entry
                    .ruleset
                    .evaluate(&input)
                    .map_err(|error| located(error, input_json))?;
                let mut members = outcome(name, &decision);
                if tool == Tool::Explain {
                    let budget = args
                        .get("budget")
                        .and_then(budget)
                        .unwrap_or(DEFAULT_BUDGET);
                    let compact = compact_explanation(&explain(&decision), budget)?;
                    members.push((
                        "explanation".into(),
                        object(vec![
                            ("text", Json::String(compact.text)),
                            ("complete", Json::Bool(compact.complete)),
                        ]),
                    ));
                }
                Ok(Json::Object(members))
            }
            Tool::Validate => validate(&args),
        }
    }

    fn list(&self) -> Json {
        let entries = self.rulesets.iter().map(|(name, entry)| {
            let ruleset = &entry.ruleset;
            let mut members = vec![
                ("name", string(name)),
                ("rules", int(ruleset.rules().len() as u64)),
                ("hit_policy", string(ruleset.hit_policy().as_str())),
            ];
            if let Some(aggregate) = ruleset.aggregate() {
                members.push(("aggregate", string(aggregate.as_str())));
            }
            members.push(("logic", string(ruleset.logic().as_str())));
            if ruleset.strict() {
                members.push(("strict", Json::Bool(true)));
            }
            let outputs = entry.summary().outputs.iter().map(|k| string(k)).collect();
            members.push(("outputs", Json::Array(outputs)));
            object(members)
        });
        object(vec![("rulesets", Json::Array(entries.collect()))])
    }

    fn lookup<'s>(&'s self, args: &Arguments<'_>) -> Result<(&'s str, &'s Entry)> {
        let name = match args.get("ruleset") {
            Some(Json::String(name)) => name.as_str(),
            _ => "",
        };
        if let Some((name, entry)) = self.rulesets.get_key_value(name) {
            return Ok((name, entry));
        }
        let mut error = Error::new(
            ErrorCode::RegistryError,
            format!("there is no rule set {name:?}"),
        )
        .with_hint("call list_rulesets for the rule sets you can use")
        .at_pointer("/ruleset");
        error.context_mut().name = Some(name.to_owned());
        error.context_mut().available = self.rulesets.keys().cloned().collect();
        Err(error)
    }
}

impl<N: Into<String>, R: Into<Arc<RuleSet>>> Extend<(N, R)> for Catalog {
    fn extend<I: IntoIterator<Item = (N, R)>>(&mut self, iter: I) {
        for (name, ruleset) in iter {
            self.insert(name, ruleset);
        }
    }
}

impl<N: Into<String>, R: Into<Arc<RuleSet>>> FromIterator<(N, R)> for Catalog {
    fn from_iter<I: IntoIterator<Item = (N, R)>>(iter: I) -> Self {
        let mut catalog = Self::new();
        catalog.extend(iter);
        catalog
    }
}

/// The JSON type a member of the arguments has (`spec/agents.md`, *Arguments*).
#[derive(Clone, Copy)]
enum Shape {
    String,
    Any,
    Budget,
    Document,
}

struct Member {
    key: &'static str,
    shape: Shape,
    required: bool,
}

/// A call's arguments, checked for their shape: the members of the tool's
/// table, present when required and of their JSON type, and no others.
struct Arguments<'j> {
    members: &'j [(String, Json)],
}

fn format_error(pointer: &str, message: impl Into<String>) -> Error {
    Error::new(ErrorCode::FormatError, message).at_pointer(pointer)
}

// `budget` bounds floats by this value, which `f64::from` takes exactly.
const _: () = assert!(MAX_BUDGET == 1 << 20);

/// A budget: any integral number in its range, so `2000.0` is `2000`.
fn budget(json: &Json) -> Option<usize> {
    let Json::Number(n) = json else { return None };
    let n = if n.is_integer_literal() {
        usize::try_from(n.as_i128()?).ok()?
    } else {
        let f = n.as_f64();
        // Within the range, the float is exact, and so is the cast back.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole =
            (f.fract() == 0.0 && (0.0..=f64::from(1_u32 << 20)).contains(&f)).then_some(f as usize);
        whole?
    };
    (MIN_BUDGET..=MAX_BUDGET).contains(&n).then_some(n)
}

impl<'j> Arguments<'j> {
    fn read(tool: Tool, json: &'j Json) -> Result<Self> {
        let Json::Object(members) = json else {
            return Err(
                format_error("", "the arguments are a JSON object").with_hint(format!(
                    "call {} with an object, such as {{}}",
                    tool.as_str()
                )),
            );
        };
        let args = Self { members };
        let table = tool.members();
        for member in table {
            let pointer = format!("/{}", member.key);
            let Some(value) = args.get(member.key) else {
                let needed = member.required
                    || (member.key == "format"
                        && matches!(args.get("document"), Some(Json::String(_))));
                if needed {
                    let error = format_error(
                        &pointer,
                        format!("{} needs the argument {:?}", tool.as_str(), member.key),
                    );
                    return Err(if member.key == "format" {
                        error.with_hint("a document given as a string needs its format: json, yaml, text or csv")
                    } else {
                        error
                    });
                }
                continue;
            };
            let fits = match member.shape {
                Shape::String => matches!(value, Json::String(_)),
                Shape::Any => true,
                Shape::Budget => budget(value).is_some(),
                Shape::Document => matches!(value, Json::String(_) | Json::Object(_)),
            };
            if !fits {
                let expected = match member.shape {
                    Shape::String => "a string".to_owned(),
                    Shape::Budget => return Err(budget_error()),
                    Shape::Document | Shape::Any => "a JSON object or a string".to_owned(),
                };
                return Err(format_error(
                    &pointer,
                    format!("the argument {:?} is {expected}", member.key),
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for (key, _) in members {
            let pointer = format!("/{}", pointer_token(key));
            if !table.iter().any(|m| m.key == key) {
                let known: Vec<&str> = table.iter().map(|m| m.key).collect();
                let hint = if known.is_empty() {
                    format!("{} takes no arguments", tool.as_str())
                } else {
                    format!("{} takes {}", tool.as_str(), known.join(", "))
                };
                return Err(
                    format_error(&pointer, format!("unknown argument {key:?}")).with_hint(hint)
                );
            }
            if !seen.insert(key.as_str()) {
                return Err(format_error(
                    &pointer,
                    format!("the argument {key:?} is given twice"),
                ));
            }
        }
        Ok(args)
    }

    fn get(&self, key: &str) -> Option<&'j Json> {
        self.members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// What a member the shape check found missing reads as.
static NULL: Json = Json::Null;

fn string(text: &str) -> Json {
    Json::String(text.to_owned())
}

fn int(n: u64) -> Json {
    Json::Number(JsonNumber::from_literal(n.to_string()))
}

fn object(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

fn names<'a>(items: impl Iterator<Item = &'a str>) -> Json {
    Json::Array(items.map(string).collect())
}

/// The members of `evaluate`'s result (`spec/agents.md`, *`evaluate`*).
fn outcome(name: &str, decision: &Decision<'_>) -> Vec<(String, Json)> {
    let mut members = vec![
        ("ruleset".to_owned(), string(name)),
        (
            "selected".to_owned(),
            names(decision.selected().map(|r| r.rule.name())),
        ),
        (
            "unknown".to_owned(),
            names(
                decision
                    .results()
                    .iter()
                    .filter(|r| r.when.truth() == Some(Truth::Unknown))
                    .map(|r| r.rule.name()),
            ),
        ),
    ];
    // `output` exists for single-hit policies and aggregates only.
    if let Ok(output) = decision.output() {
        let output = output.map_or(Json::Null, |value| write_value(&value));
        members.push(("output".into(), output));
    }
    let outputs = decision
        .outputs()
        .into_iter()
        .map(|payload| write_value(&Value::Map(payload.into_iter().collect::<Map>())));
    members.push(("outputs".into(), Json::Array(outputs.collect())));
    members
}

/// `validate`'s result (`spec/agents.md`, *`validate`*).
fn validate(args: &Arguments<'_>) -> Result<Json> {
    let document = args.get("document").unwrap_or(&NULL);
    let format = match args.get("format") {
        Some(Json::String(name)) => {
            let format = DocumentFormat::from_name(name).ok_or_else(|| {
                format_error("/format", format!("unknown format {name:?}"))
                    .with_hint("use json, yaml, text or csv")
            })?;
            if matches!(document, Json::Object(_)) && format != DocumentFormat::Json {
                return Err(format_error(
                    "/format",
                    format!("a document given as an object is JSON, not {name}"),
                )
                .with_hint("leave the format out, or give the document's text as a string"));
            }
            format
        }
        _ => DocumentFormat::Json,
    };
    let limits = Limits::default();
    let read = match document {
        Json::String(text) => read_document_as(text.as_bytes(), format, &limits),
        other => read_parsed_document(other, &limits),
    };
    let document = match read {
        Ok(document) => document,
        Err(error) => {
            return Ok(object(vec![
                ("valid", Json::Bool(false)),
                ("error", agent_error(&error)),
            ]));
        }
    };
    let content = match &document {
        Document::Condition(_) => "condition",
        Document::Rule(_) => "rule",
        Document::RuleSet(_) => "ruleset",
        Document::DecisionTable(_) => "decision_table",
    };
    let mut members = vec![("valid", Json::Bool(true)), ("content", string(content))];
    if !matches!(document, Document::Condition(_)) {
        let ruleset = document.into_ruleset()?;
        let overview = summary(&ruleset);
        members.push(("summary", write_summary(&overview)));
        members.push(("input_schema", input_schema(&overview)));
    }
    Ok(object(members))
}

/// An evaluation error with a path, located in the call's input: the pointer
/// to the value at that path as the input is written (through `$map`), or to
/// where it would be.
fn located(mut error: Error, input: &Json) -> Error {
    let Some(path) = error.context().path.clone() else {
        return error;
    };
    let mut pointer = String::from("/input");
    let mut here = Some(input);
    for segment in &path {
        if let Some(Json::Object(members)) = here
            && let [(key, inner)] = members.as_slice()
            && key == "$map"
        {
            pointer.push_str("/$map");
            here = Some(inner);
        }
        match segment {
            Segment::Key(key) => {
                pointer.push('/');
                pointer.push_str(&pointer_token(key));
                here = match here {
                    Some(Json::Object(members)) => {
                        members.iter().find(|(k, _)| k == key).map(|(_, v)| v)
                    }
                    _ => None,
                };
            }
            Segment::Index(index) => {
                pointer.push('/');
                pointer.push_str(&index.to_string());
                here = match here {
                    Some(Json::Array(items)) => {
                        usize::try_from(*index).ok().and_then(|i| items.get(i))
                    }
                    _ => None,
                };
            }
        }
    }
    error.context_mut().pointer = Some(pointer);
    error
}

/// The hint an agent error has when the core's error has none
/// (`spec/agents.md`, *Default hints*).
fn default_hint(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::FormatError => {
            "fix the value at the pointer so that it has the form the format (or the tool's input schema) gives"
        }
        ErrorCode::UnsupportedVersion => "write \"version\": 1",
        ErrorCode::LimitExceeded => "send a smaller document or input",
        ErrorCode::DecisionMismatch => "read the decision with the rule set that made it",
        ErrorCode::ExpressionError => {
            "fix the condition at the pointer: compare values, and join conditions with and, or and not"
        }
        ErrorCode::RuleDefinitionError => "fix the rule at the pointer",
        ErrorCode::RuleSyntaxError => "fix the text at the line and column",
        ErrorCode::RegistryError | ErrorCode::RuleNotFound => "use one of the available names",
        ErrorCode::EvaluationError => "fix the value at the pointer in the input",
        ErrorCode::MissingField => {
            "add the field at the pointer to the input; the rule set's input schema lists what it reads"
        }
        ErrorCode::TypeMismatch => {
            "give the field at the pointer a value of the type the rule set's input schema gives"
        }
        ErrorCode::HitPolicyError => {
            "the rule set cannot decide on this input: change the input, or ask the rule set's owner"
        }
        ErrorCode::ActionError => "tools never run actions; report this as a bug",
        ErrorCode::Unsupported => {
            "this build or this version cannot do it; try another format or tool"
        }
    }
}

/// An agent error (`spec/agents.md`, *Errors*): the error's JSON form
/// (`spec/errors.md`, *JSON form*), always with a hint.
#[must_use]
pub fn agent_error(error: &Error) -> Json {
    let mut json = write_error(error);
    if error.hint().is_none()
        && let Json::Object(members) = &mut json
    {
        // After `code` and `message`, as the JSON form orders its members.
        let at = members.len().min(2);
        members.insert(at, ("hint".into(), string(default_hint(error.code()))));
    }
    json
}
