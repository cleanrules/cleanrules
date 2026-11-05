//! The wasm-bindgen side of documents, result trees and decision tables for
//! the idiomatic API, with the names, arguments and errors of the napi-rs
//! build's (`src/napi/documents.rs`). Internal: no entry point exports them.

use cleanrules::table::{Cell, HeaderItem};
use wasm_bindgen::prelude::*;

use super::build::NativeRule;
use super::classes::{Decision, Document, Node, RuleSet, chained, read, undefined_input};
use super::{OrFail, fail, raw, raw_limits, raw_options};
use crate::common::core::{NodeCore, TreeData, TreeKind};
use crate::common::documents;
use crate::common::tables::{GridCore, TableCore};
use crate::common::{no_constructor, tree_options};

fn node(core: cleanrules::ast::Node) -> Node {
    Node::from_core(NodeCore::new(core))
}

fn nodes(list: Vec<cleanrules::ast::Node>) -> Vec<Node> {
    list.into_iter().map(node).collect()
}

/// A result tree: its encoding and its nodes.
#[wasm_bindgen]
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

#[wasm_bindgen]
impl NativeTree {
    /// Not a constructor: result trees come from `nodeTree` and `ruleTree`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<NativeTree, JsValue> {
        Err(fail(no_constructor("NativeTree")))
    }

    /// The tree's encoding and JSON form, `[form, json]`.
    #[must_use]
    pub fn data(&self) -> String {
        self.data.clone()
    }

    /// The tree's nodes, in pre-order.
    #[must_use]
    pub fn nodes(&self) -> Vec<Node> {
        nodes(self.nodes.clone())
    }
}

/// A decision table, immutable.
#[wasm_bindgen]
pub struct NativeTable {
    core: TableCore,
}

#[wasm_bindgen]
impl NativeTable {
    /// Not a constructor: tables come from documents and grids.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<NativeTable, JsValue> {
        Err(fail(no_constructor("NativeTable")))
    }

    /// `[hit policy, aggregate, logic, strict]`, as JSON.
    #[must_use]
    pub fn config(&self) -> String {
        self.core.config()
    }

    /// The input expressions, in column order.
    #[must_use]
    pub fn inputs(&self) -> Vec<Node> {
        nodes(self.core.inputs())
    }

    /// The output columns, as JSON.
    #[must_use]
    pub fn outputs(&self) -> String {
        self.core.outputs()
    }

    /// The rows, as JSON.
    #[must_use]
    pub fn rows(&self) -> String {
        self.core.rows()
    }

    /// The nodes of the rows' cells.
    #[wasm_bindgen(js_name = rowNodes)]
    #[must_use]
    pub fn row_nodes(&self) -> Vec<Node> {
        nodes(self.core.row_nodes())
    }

    /// The number of rows.
    #[wasm_bindgen(getter, js_name = rowCount)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // A table never holds 2^32 rows.
    pub fn row_count(&self) -> u32 {
        self.core.row_count() as u32
    }

    /// The rule set the table compiles into.
    #[must_use]
    pub fn ruleset(&self) -> RuleSet {
        RuleSet::from_core(self.core.ruleset())
    }

    /// The table document in `format`.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn write(&self, format: &JsValue) -> Result<String, JsValue> {
        self.core.write(&raw(format)).or_fail()
    }
}

/// A decision table being built from a grid.
#[wasm_bindgen]
pub struct NativeGrid {
    core: GridCore,
}

#[wasm_bindgen]
impl NativeGrid {
    /// Not a constructor: grids come from `gridNew`.
    ///
    /// # Errors
    ///
    /// Always a `TypeError`.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<NativeGrid, JsValue> {
        Err(fail(no_constructor("NativeGrid")))
    }

    /// A header item given as text.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = headerText)]
    pub fn header_text(&mut self, text: String) -> Result<(), JsValue> {
        self.core.header(HeaderItem::Text(text)).or_fail()
    }

    /// An input given as a node.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = headerInput)]
    pub fn header_input(&mut self, node: &Node) -> Result<(), JsValue> {
        self.core
            .header(HeaderItem::Input(node.core.node().clone()))
            .or_fail()
    }

    /// An output: its key and its values, a list in the reader's encoding.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = headerOutput)]
    pub fn header_output(&mut self, name: String, wire: &str) -> Result<(), JsValue> {
        self.core.header_output(name, wire).or_fail()
    }

    /// Read the header.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn start(&mut self) -> Result<(), JsValue> {
        self.core.start().or_fail()
    }

    /// The next cell, given as text.
    #[wasm_bindgen(js_name = cellText)]
    pub fn cell_text(&mut self, text: String) {
        self.core.cell(Cell::Text(text));
    }

    /// The next cell, given as a node.
    #[wasm_bindgen(js_name = cellNode)]
    pub fn cell_node(&mut self, node: &Node) {
        self.core.cell(Cell::Node(node.core.node().clone()));
    }

    /// The next cell, a value in the reader's encoding.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = cellValue)]
    pub fn cell_value(&mut self, wire: &str) -> Result<(), JsValue> {
        self.core.cell_value(wire).or_fail()
    }

    /// End the row and read it.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    #[wasm_bindgen(js_name = endRow)]
    pub fn end_row(&mut self) -> Result<(), JsValue> {
        self.core.end_row().or_fail()
    }

    /// The table, checked and compiled.
    ///
    /// # Errors
    ///
    /// As in the napi-rs build.
    pub fn finish(&mut self) -> Result<NativeTable, JsValue> {
        self.core
            .finish()
            .map(|core| NativeTable { core })
            .or_fail()
    }
}

/// A grid with its settings and limits.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = gridNew)]
pub fn grid_new(
    hit_policy: &JsValue,
    aggregate: &JsValue,
    logic: &JsValue,
    strict: &JsValue,
    limits: &JsValue,
) -> Result<NativeGrid, JsValue> {
    GridCore::new(
        &raw(hit_policy),
        &raw(aggregate),
        &raw(logic),
        &raw(strict),
        &raw_limits(limits)?,
    )
    .map(|core| NativeGrid { core })
    .or_fail()
}

/// The condition of a condition document.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = documentCondition)]
pub fn document_condition(document: &Document) -> Result<Node, JsValue> {
    document.core.condition().map(node).or_fail()
}

/// The rule of a rule document.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = documentRule)]
pub fn document_rule(document: &Document) -> Result<NativeRule, JsValue> {
    document
        .core
        .rule()
        .map(|rule| NativeRule { rule })
        .or_fail()
}

/// The table of a decision table document.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = documentTable)]
pub fn document_table(document: &Document) -> Result<NativeTable, JsValue> {
    document
        .core
        .table()
        .map(|table| NativeTable {
            core: TableCore { table },
        })
        .or_fail()
}

/// One rule written in the text syntax, under `limits`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = ruleText)]
pub fn rule_text(text: &JsValue, limits: &JsValue) -> Result<NativeRule, JsValue> {
    let text = crate::common::text(raw(text), "the text").or_fail()?;
    documents::rule_text(&text, &raw_limits(limits)?)
        .map(|rule| NativeRule { rule })
        .or_fail()
}

/// A rule's name, priority, description and payload keys, as JSON.
#[wasm_bindgen(js_name = ruleParts)]
#[must_use]
pub fn rule_parts(rule: &NativeRule) -> String {
    documents::rule_parts(&rule.rule)
}

/// A rule's condition and payload values.
#[wasm_bindgen(js_name = ruleNodes)]
#[must_use]
pub fn rule_nodes(rule: &NativeRule) -> Vec<Node> {
    nodes(documents::rule_nodes(&rule.rule))
}

/// A rule set's rules, in order.
#[wasm_bindgen(js_name = ruleSetRules)]
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
/// As in the napi-rs build.
#[wasm_bindgen(js_name = writeCondition)]
pub fn write_condition(node: &Node, format: &JsValue) -> Result<String, JsValue> {
    documents::write_condition(node.core.node(), &raw(format)).or_fail()
}

/// The rule document of a rule, in `format`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = writeRule)]
pub fn write_rule(rule: &NativeRule, format: &JsValue) -> Result<String, JsValue> {
    documents::write_rule(&rule.rule, &raw(format)).or_fail()
}

/// As in the napi-rs build.
#[wasm_bindgen]
#[must_use]
pub fn stringifiable(text: &str) -> String {
    crate::common::out::stringifiable_text(text)
}

/// Check that a file of `size` bytes is within the size limit of `format`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = fileSizeCheck)]
pub fn file_size_check(
    name: &str,
    size: f64,
    format: &JsValue,
    limits: &JsValue,
) -> Result<(), JsValue> {
    documents::size_check(name, size, &raw(format), &raw_limits(limits)?).or_fail()
}

/// Evaluate `node` on native data, as a condition or as a value: its result
/// tree.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = nodeTree)]
pub fn node_tree(
    node: &Node,
    data: &JsValue,
    options: &JsValue,
    value: bool,
) -> Result<NativeTree, JsValue> {
    let options = raw_options(options)?;
    let options = tree_options(options.as_ref()).or_fail()?;
    let kind = if value {
        TreeKind::Value
    } else {
        TreeKind::Condition
    };
    NodeCore::check(&kind, &options).or_fail()?;
    if data.is_undefined() {
        return Err(undefined_input());
    }
    let native = read(data, node.paths())?;
    node.core
        .tree_data(&kind, &native.read.value, &options)
        .map(NativeTree::from)
        .map_err(|error| chained(&error, &native))
}

/// The result tree of the condition of the rule at `position` in `decision`.
///
/// # Errors
///
/// As in the napi-rs build.
#[wasm_bindgen(js_name = ruleTree)]
pub fn rule_tree(decision: &Decision, position: &JsValue) -> Result<NativeTree, JsValue> {
    decision
        .core
        .rule_tree(&raw(position))
        .map(NativeTree::from)
        .or_fail()
}
