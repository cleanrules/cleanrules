//! The napi-rs side of documents, result trees, decision tables and bundles
//! for the idiomatic API (`lib/api/documents.js`, `trees.js`, `tables.js` and
//! `bundles.js`), over `common::documents`, `common::tables`,
//! `common::bundles` and `common::core`.
//!
//! Internal, like `build.rs`: the addon exports these functions and classes,
//! but no entry point does. The WebAssembly build has the same ones
//! (`src/wasm/documents.rs`), with the same checks and errors.

use cleanrules::table::{Cell, HeaderItem};
use napi::bindgen_prelude::{ClassInstance, FromNapiValue, Uint8Array, Unknown};
use napi::{Env, Result};
use napi_derive::napi;

use super::build::NativeRule;
use super::classes::{
    Decision, Document, Node, RuleSet, chained, is_undefined, make_bundle, optional, read,
};
use super::{OrFail, fail, raw, raw_limits, raw_options};
use crate::common::bundles;
use crate::common::core::{NodeCore, TreeData, TreeKind};
use crate::common::documents;
use crate::common::tables::{GridCore, TableCore};
use crate::common::{Problem, UNDEFINED_INPUT, no_constructor, tree_options};

fn node(core: cleanrules::ast::Node) -> Node {
    Node {
        core: NodeCore::new(core),
    }
}

fn nodes(list: Vec<cleanrules::ast::Node>) -> Vec<Node> {
    list.into_iter().map(node).collect()
}

/// A result tree (`common::core::TreeData`): its encoding and its nodes.
#[napi]
pub struct NativeTree {
    data: String,
    nodes: Vec<cleanrules::ast::Node>,
}

impl From<TreeData> for NativeTree {
    fn from(tree: TreeData) -> Self {
        Self {
            data: tree.data,
            nodes: tree.nodes,
        }
    }
}

#[napi]
impl NativeTree {
    /// Not a constructor: result trees come from `nodeTree` and `ruleTree`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, no_constructor("NativeTree")))
    }

    /// The tree's encoding and JSON form, `[form, json]`.
    #[napi]
    #[must_use]
    pub fn data(&self) -> String {
        self.data.clone()
    }

    /// The tree's nodes, in pre-order.
    #[napi]
    #[must_use]
    pub fn nodes(&self) -> Vec<Node> {
        nodes(self.nodes.clone())
    }
}

/// A decision table, immutable.
#[napi]
pub struct NativeTable {
    core: TableCore,
}

#[napi]
impl NativeTable {
    /// Not a constructor: tables come from documents and grids.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, no_constructor("NativeTable")))
    }

    /// `[hit policy, aggregate, logic, strict]`, as JSON.
    #[napi]
    #[must_use]
    pub fn config(&self) -> String {
        self.core.config()
    }

    /// The input expressions, in column order.
    #[napi]
    #[must_use]
    pub fn inputs(&self) -> Vec<Node> {
        nodes(self.core.inputs())
    }

    /// The output columns, as JSON.
    #[napi]
    #[must_use]
    pub fn outputs(&self) -> String {
        self.core.outputs()
    }

    /// The rows, as JSON.
    #[napi]
    #[must_use]
    pub fn rows(&self) -> String {
        self.core.rows()
    }

    /// The nodes of the rows' cells.
    #[napi]
    #[must_use]
    pub fn row_nodes(&self) -> Vec<Node> {
        nodes(self.core.row_nodes())
    }

    /// The number of rows.
    #[napi(getter)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A table never holds 2^32 rows.
    pub fn row_count(&self) -> u32 {
        self.core.row_count() as u32
    }

    /// The rule set the table compiles into.
    #[napi]
    #[must_use]
    pub fn ruleset(&self) -> RuleSet {
        RuleSet {
            core: self.core.ruleset(),
        }
    }

    /// The table document in `format`: JSON (the default), YAML or CSV.
    ///
    /// # Errors
    ///
    /// `UnsupportedError` for the rule syntax.
    #[napi(ts_args_type = "format?: Format | null")]
    pub fn write(&self, env: &Env, format: Option<Unknown<'_>>) -> Result<String> {
        self.core.write(&optional(format)?).or_fail(env)
    }

    /// The canonical bytes of the table's own document.
    #[napi]
    #[must_use]
    pub fn canonical_bytes(&self) -> Uint8Array {
        Uint8Array::new(self.core.canonical_bytes())
    }

    /// The digest of the table's own document; its rule set digest is
    /// `ruleset().digest()`.
    #[napi]
    #[must_use]
    pub fn digest(&self) -> String {
        self.core.digest()
    }
}

/// A decision table being built from a grid (`spec/tables.md`, *Grids*):
/// header items, then each row's cells.
#[napi]
pub struct NativeGrid {
    core: GridCore,
}

#[napi]
impl NativeGrid {
    /// Not a constructor: grids come from `gridNew`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`, the same as the WebAssembly build's.
    #[napi(constructor)]
    pub fn new(env: &Env) -> Result<Self> {
        Err(fail(env, no_constructor("NativeGrid")))
    }

    /// A header item given as text.
    ///
    /// # Errors
    ///
    /// If the header was already read.
    #[napi]
    pub fn header_text(&mut self, env: &Env, text: String) -> Result<()> {
        self.core.header(HeaderItem::Text(text)).or_fail(env)
    }

    /// An input given as a node.
    ///
    /// # Errors
    ///
    /// If the header was already read.
    #[napi]
    pub fn header_input(&mut self, env: &Env, node: &Node) -> Result<()> {
        self.core
            .header(HeaderItem::Input(node.core.node().clone()))
            .or_fail(env)
    }

    /// An output: its key and its values, a list in the reader's encoding.
    ///
    /// # Errors
    ///
    /// `ExpressionError` for a value that is not a constant.
    #[napi]
    pub fn header_output(&mut self, env: &Env, name: String, wire: String) -> Result<()> {
        self.core.header_output(name, &wire).or_fail(env)
    }

    /// Read the header.
    ///
    /// # Errors
    ///
    /// As the core reports them, with pointers into the grid.
    #[napi]
    pub fn start(&mut self, env: &Env) -> Result<()> {
        self.core.start().or_fail(env)
    }

    /// The next cell, given as text.
    #[napi]
    pub fn cell_text(&mut self, text: String) {
        self.core.cell(Cell::Text(text));
    }

    /// The next cell, given as a node.
    #[napi]
    pub fn cell_node(&mut self, node: &Node) {
        self.core.cell(Cell::Node(node.core.node().clone()));
    }

    /// The next cell, a value in the reader's encoding.
    ///
    /// # Errors
    ///
    /// `ExpressionError` for a value that is not a constant.
    #[napi]
    pub fn cell_value(&mut self, env: &Env, wire: String) -> Result<()> {
        self.core.cell_value(&wire).or_fail(env)
    }

    /// End the row and read it.
    ///
    /// # Errors
    ///
    /// As the core reports them, with pointers into the grid.
    #[napi]
    pub fn end_row(&mut self, env: &Env) -> Result<()> {
        self.core.end_row().or_fail(env)
    }

    /// The table, checked and compiled.
    ///
    /// # Errors
    ///
    /// As the core reports them, with pointers into the grid.
    #[napi]
    pub fn finish(&mut self, env: &Env) -> Result<NativeTable> {
        self.core
            .finish()
            .map(|core| NativeTable { core })
            .or_fail(env)
    }
}

/// A grid with its settings and limits.
///
/// # Errors
///
/// `RuleDefinitionError` for an unknown hit policy or aggregate, or a
/// `strict` that is not a boolean; `RegistryError` for an unknown logic; a
/// `TypeError` or `RangeError` for bad limits.
#[napi]
pub fn grid_new(
    env: &Env,
    hit_policy: Unknown<'_>,
    aggregate: Unknown<'_>,
    logic: Unknown<'_>,
    strict: Unknown<'_>,
    limits: Unknown<'_>,
) -> Result<NativeGrid> {
    GridCore::new(
        &raw(&hit_policy)?,
        &raw(&aggregate)?,
        &raw(&logic)?,
        &raw(&strict)?,
        &raw_limits(&limits)?,
    )
    .map(|core| NativeGrid { core })
    .or_fail(env)
}

/// The condition of a condition document.
///
/// # Errors
///
/// For a document of another kind.
#[napi]
pub fn document_condition(env: &Env, document: &Document) -> Result<Node> {
    document.core.condition().map(node).or_fail(env)
}

/// The rule of a rule document.
///
/// # Errors
///
/// For a document of another kind.
#[napi]
pub fn document_rule(env: &Env, document: &Document) -> Result<NativeRule> {
    document
        .core
        .rule()
        .map(|rule| NativeRule { rule })
        .or_fail(env)
}

/// The table of a decision table document.
///
/// # Errors
///
/// For a document of another kind.
#[napi]
pub fn document_table(env: &Env, document: &Document) -> Result<NativeTable> {
    document
        .core
        .table()
        .map(|table| NativeTable {
            core: TableCore { table },
        })
        .or_fail(env)
}

/// One rule written in the text syntax, under `limits`.
///
/// # Errors
///
/// `RuleSyntaxError`, `ExpressionError` or `RuleDefinitionError`, as the core
/// reports them; `FormatError` beyond the limits.
#[napi(ts_args_type = "text: string | Uint8Array, limits?: Limits | null")]
pub fn rule_text(env: &Env, text: Unknown<'_>, limits: Unknown<'_>) -> Result<NativeRule> {
    let text = crate::common::text(raw(&text)?, "the text").or_fail(env)?;
    documents::rule_text(&text, &raw_limits(&limits)?)
        .map(|rule| NativeRule { rule })
        .or_fail(env)
}

/// A rule's name, priority, description and payload keys, as JSON.
#[napi]
#[must_use]
pub fn rule_parts(rule: &NativeRule) -> String {
    documents::rule_parts(&rule.rule)
}

/// A rule's condition and payload values.
#[napi]
#[must_use]
pub fn rule_nodes(rule: &NativeRule) -> Vec<Node> {
    nodes(documents::rule_nodes(&rule.rule))
}

/// A rule set's rules, in order.
#[napi]
#[must_use]
pub fn rule_set_rules(ruleset: &RuleSet) -> Vec<NativeRule> {
    ruleset
        .core
        .ruleset
        .rules()
        .iter()
        .map(|rule| NativeRule { rule: rule.clone() })
        .collect()
}

/// The condition document of a node, in `format`.
///
/// # Errors
///
/// `UnsupportedError` for CSV.
#[napi]
pub fn write_condition(env: &Env, node: &Node, format: Unknown<'_>) -> Result<String> {
    documents::write_condition(node.core.node(), &raw(&format)?).or_fail(env)
}

/// The rule document of a rule, in `format`.
///
/// # Errors
///
/// `UnsupportedError` for CSV, and in text for metadata.
#[napi]
pub fn write_rule(env: &Env, rule: &NativeRule, format: Unknown<'_>) -> Result<String> {
    documents::write_rule(&rule.rule, &raw(&format)?).or_fail(env)
}

/// The canonical bytes of the condition document of a node. The JavaScript
/// half refuses value nodes first, as `dumps` does.
#[napi]
#[must_use]
pub fn canonical_condition(node: &Node) -> Uint8Array {
    Uint8Array::new(documents::condition_canonical(node.core.node()))
}

/// The digest of the condition document of a node.
#[napi]
#[must_use]
pub fn digest_condition(node: &Node) -> String {
    documents::condition_digest(node.core.node())
}

/// The canonical bytes of the rule document of a rule.
#[napi]
#[must_use]
pub fn canonical_rule(rule: &NativeRule) -> Uint8Array {
    Uint8Array::new(documents::rule_canonical(&rule.rule))
}

/// The digest of the rule document of a rule.
#[napi]
#[must_use]
pub fn digest_rule(rule: &NativeRule) -> String {
    documents::rule_digest(&rule.rule)
}

/// The text of a document the core wrote, with each float that holds an
/// integer as `{"$float": n}`, for `toJSON()`: `JSON.stringify` of what
/// `JSON.parse` makes of it means the same document (`common::out::stringifiable`).
#[napi]
#[must_use]
pub fn stringifiable(text: String) -> String {
    crate::common::out::stringifiable_text(&text)
}

/// Check that a file of `size` bytes is within the size limit of `format`.
///
/// # Errors
///
/// `FormatError` (`limit_exceeded`) when it is not.
#[napi]
pub fn file_size_check(
    env: &Env,
    name: String,
    size: f64,
    format: Unknown<'_>,
    limits: Unknown<'_>,
) -> Result<()> {
    documents::size_check(&name, size, &raw(&format)?, &raw_limits(&limits)?).or_fail(env)
}

/// Evaluate `node` (a `Node`) on native data, as a condition or as a value:
/// its result tree.
///
/// # Errors
///
/// As `Node.evaluateTree` and `Node.evaluateValueTree`.
#[napi(ts_args_type = "node: Node, data: unknown, options: unknown, value: boolean")]
pub fn node_tree<'env>(
    env: &'env Env,
    node: Unknown<'env>,
    data: Unknown<'env>,
    options: Unknown<'env>,
    value: bool,
) -> Result<NativeTree> {
    let Ok(instance) = ClassInstance::<Node>::from_unknown(node) else {
        return Err(fail(
            env,
            Problem::Internal("nodeTree takes a Node".to_owned()),
        ));
    };
    let options = raw_options(env, &options)?;
    let options = tree_options(options.as_ref()).or_fail(env)?;
    let kind = if value {
        TreeKind::Value
    } else {
        TreeKind::Condition
    };
    NodeCore::check(&kind, &options).or_fail(env)?;
    if is_undefined(&data)? {
        return Err(fail(env, Problem::Type(UNDEFINED_INPUT.to_owned())));
    }
    let native = read(env, data, node, options.limits.max_values, None)?;
    instance
        .core
        .tree_data(&kind, &native.read.value, &options)
        .map(NativeTree::from)
        .map_err(|error| chained(env, &error, &native))
}

/// The result tree of the condition of the rule at `position` in `decision`.
///
/// # Errors
///
/// A `RangeError` for a position out of the rule set.
#[napi(ts_args_type = "decision: Decision, position: number")]
pub fn rule_tree(env: &Env, decision: &Decision, position: Unknown<'_>) -> Result<NativeTree> {
    decision
        .core
        .rule_tree(&raw(&position)?)
        .map(NativeTree::from)
        .or_fail(env)
}

/// A bundle of a rule set, made with this engine (`Document.makeBundle`).
///
/// # Errors
///
/// As `Document.makeBundle`.
#[napi(
    ts_args_type = "ruleset: RuleSet, id: string, revision: string, cases?: BundleCaseInput[] | null, limits?: Limits | null"
)]
pub fn bundle_of_ruleset(
    env: &Env,
    ruleset: &RuleSet,
    id: Unknown<'_>,
    revision: Unknown<'_>,
    cases: Option<Unknown<'_>>,
    limits: Option<Unknown<'_>>,
) -> Result<Document> {
    let rules = bundles::ruleset_rules(&ruleset.core);
    make_bundle(env, rules, &id, &revision, cases, limits).map(|core| Document { core })
}

/// A bundle of a decision table, made with this engine (`Document.makeBundle`).
///
/// # Errors
///
/// As `Document.makeBundle`.
#[napi(
    ts_args_type = "table: NativeTable, id: string, revision: string, cases?: BundleCaseInput[] | null, limits?: Limits | null"
)]
pub fn bundle_of_table(
    env: &Env,
    table: &NativeTable,
    id: Unknown<'_>,
    revision: Unknown<'_>,
    cases: Option<Unknown<'_>>,
    limits: Option<Unknown<'_>>,
) -> Result<Document> {
    let rules = bundles::table_rules(&table.core);
    make_bundle(env, rules, &id, &revision, cases, limits).map(|core| Document { core })
}

/// What the idiomatic `Bundle` reads of a bundle document, as JSON
/// (`common::bundles`).
///
/// # Errors
///
/// For a document of another kind.
#[napi]
pub fn bundle_data(env: &Env, document: &Document) -> Result<String> {
    document.core.bundle_data().or_fail(env)
}

/// The rule set a bundle gives.
///
/// # Errors
///
/// For a document of another kind.
#[napi]
pub fn bundle_ruleset(env: &Env, document: &Document) -> Result<RuleSet> {
    document
        .core
        .bundle_ruleset()
        .map(|core| RuleSet { core })
        .or_fail(env)
}

/// The decision table a bundle holds.
///
/// # Errors
///
/// For a bundle of a rule set, or a document of another kind.
#[napi]
pub fn bundle_table(env: &Env, document: &Document) -> Result<NativeTable> {
    document
        .core
        .bundle_table()
        .map(|core| NativeTable { core })
        .or_fail(env)
}

/// Whether `bytes` are the case file at `path` the bundle was made with.
///
/// # Errors
///
/// A `RangeError` for a path the bundle does not refer to; a `TypeError` for
/// arguments of the wrong type.
#[napi(ts_args_type = "document: Document, path: string, bytes: string | Uint8Array")]
pub fn bundle_case_digest_matches(
    env: &Env,
    document: &Document,
    path: Unknown<'_>,
    bytes: Unknown<'_>,
) -> Result<bool> {
    document
        .core
        .case_digest_matches(&raw(&path)?, raw(&bytes)?)
        .or_fail(env)
}

/// Check the path and digest of a case file (`spec/bundles.md`, *Cases*), for
/// the idiomatic `CaseFileRef`.
///
/// # Errors
///
/// A `TypeError` for a path or digest that is not a string; a `FormatError`
/// for a digest or a path without its form.
#[napi(ts_args_type = "path: string, digest: string")]
pub fn check_case_file_ref(env: &Env, path: Unknown<'_>, digest: Unknown<'_>) -> Result<()> {
    bundles::check_case_file_ref(&raw(&path)?, &raw(&digest)?).or_fail(env)
}

/// Whether `content` is the case file of `path` and `digest`: whether the
/// SHA-256 digest of its bytes (a string's UTF-8 bytes) is `digest`.
///
/// # Errors
///
/// As `checkCaseFileRef`, and a `TypeError` for content that is neither a string
/// nor bytes.
#[napi(ts_args_type = "path: string, digest: string, content: string | Uint8Array")]
pub fn case_file_ref_digest_matches(
    env: &Env,
    path: Unknown<'_>,
    digest: Unknown<'_>,
    content: Unknown<'_>,
) -> Result<bool> {
    bundles::case_file_ref_digest_matches(&raw(&path)?, &raw(&digest)?, raw(&content)?).or_fail(env)
}
