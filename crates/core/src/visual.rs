//! Diagrams of conditions, rules, rule sets, explanations and decision tables
//! (`spec/visual.md`), written as DOT, Mermaid or standalone HTML.
//!
//! The core writes every diagram as text, with no graphics library, so a
//! diagram is the same in every language; drawing it (Graphviz, a Mermaid
//! viewer, a browser) is up to the caller. Every label is escaped for its
//! format and bounded, and a diagram has at most
//! [`Limits::max_diagram_size`] elements.
//!
//! ```
//! use cleanrules::json::Limits;
//! use cleanrules::visual::{DiagramFormat, draw_condition};
//!
//! let condition = cleanrules::text::parse_node("x > 10 AND NOT y = 'a'", &Limits::default())?;
//! let mermaid = draw_condition(&condition, DiagramFormat::Mermaid, &Limits::default())?;
//! assert!(mermaid.starts_with("flowchart TD\n  n0([\"all\"])\n  n1[\"x #gt; 10\"]\n"));
//! # Ok::<(), cleanrules::Error>(())
//! ```

mod grid;
mod model;
mod render;

use crate::ast::Node;
use crate::error::{Error, ErrorCode, Result};
use crate::explain::{Explanation, Summary, summary};
use crate::format::Document;
use crate::json::Limits;
use crate::logic::Truth;
use crate::rules::{Decision, Rule};
use crate::table::DecisionTable;

/// The format a diagram is written in (`spec/visual.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagramFormat {
    /// A Graphviz graph.
    Dot,
    /// A Mermaid flowchart.
    Mermaid,
    /// A standalone HTML document, with no script and no external resource.
    Html,
    /// The HTML document's style and content, to embed in a page or a
    /// notebook.
    HtmlFragment,
}

impl DiagramFormat {
    /// The format's name: `dot`, `mermaid`, `html` or `html_fragment`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DiagramFormat::Dot => "dot",
            DiagramFormat::Mermaid => "mermaid",
            DiagramFormat::Html => "html",
            DiagramFormat::HtmlFragment => "html_fragment",
        }
    }

    /// The format for a name (lowercase, exact).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "dot" => DiagramFormat::Dot,
            "mermaid" => DiagramFormat::Mermaid,
            "html" => DiagramFormat::Html,
            "html_fragment" => DiagramFormat::HtmlFragment,
            _ => return None,
        })
    }
}

/// The class a truth value gives the node of a cause (`spec/visual.md`,
/// *Truth values*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TruthClass {
    /// True, or the degree 1.
    True,
    /// False, or the degree 0.
    False,
    /// Unknown, or the interval `[0, 1]`.
    Unknown,
    /// Any other degree or interval.
    Degree,
}

impl TruthClass {
    /// The class's name: `truth_true`, `truth_false`, `truth_unknown` or
    /// `truth_degree`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TruthClass::True => "truth_true",
            TruthClass::False => "truth_false",
            TruthClass::Unknown => "truth_unknown",
            TruthClass::Degree => "truth_degree",
        }
    }
}

/// How a truth value is drawn: its label and its class (`spec/visual.md`,
/// *Truth values*).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Mark {
    /// The class of the cause's node.
    pub class: TruthClass,
    /// The label written in the cause's node (`true`, `0.7`, `[0.2, 0.9]`).
    pub label: String,
}

impl Mark {
    /// The mark of a graded truth value, the interval `[lo, hi]` of degrees
    /// (`spec/visual.md`, *Truth values*): the degree when both ends print the
    /// same, else the interval, each written as a float gap is
    /// (`spec/explain.md`, *Text*: 12 significant digits). The class is read
    /// from the printed ends too, so the label and the class always agree.
    ///
    /// The ends must be finite, with `0 <= lo <= hi <= 1`, as a graded logic
    /// gives them; debug builds check it.
    ///
    /// ```
    /// use cleanrules::visual::{Mark, TruthClass};
    ///
    /// assert_eq!(Mark::degree(0.7, 0.7).label, "0.7");
    /// assert_eq!(Mark::degree(0.0, 1.0).class, TruthClass::Unknown);
    /// assert_eq!(Mark::degree(0.2, 0.9).label, "[0.2, 0.9]");
    /// ```
    #[must_use]
    pub fn degree(lo: f64, hi: f64) -> Self {
        debug_assert!(
            lo.is_finite() && hi.is_finite() && (0.0..=hi).contains(&lo) && hi <= 1.0,
            "a degree interval within [0, 1], not [{lo}, {hi}]"
        );
        let (low, high) = (
            crate::explain::text::float_gap(lo),
            crate::explain::text::float_gap(hi),
        );
        let class = match (low.as_str(), high.as_str()) {
            ("1", "1") => TruthClass::True,
            ("0", "0") => TruthClass::False,
            ("0", "1") => TruthClass::Unknown,
            _ => TruthClass::Degree,
        };
        let label = if low == high {
            low
        } else {
            format!("[{low}, {high}]")
        };
        Self { class, label }
    }
}

/// The trait's seal: only the truth values of the core's logics are drawn.
mod sealed {
    pub trait Sealed {}
    impl Sealed for crate::logic::Truth {}
}

/// A truth value that can be drawn as a cause's mark (`spec/visual.md`, *Truth
/// values*). Sealed: the core implements it for the truth values of its
/// logics, here, so that `logic` does not depend on `visual`. Kleene's truth
/// values are words; a graded logic's are drawn with [`Mark::degree`].
pub trait ToMark: sealed::Sealed {
    /// Its mark.
    fn to_mark(&self) -> Mark;
}

impl ToMark for Truth {
    fn to_mark(&self) -> Mark {
        let (class, label) = match self {
            Truth::True => (TruthClass::True, "true"),
            Truth::False => (TruthClass::False, "false"),
            Truth::Unknown => (TruthClass::Unknown, "unknown"),
        };
        Mark {
            class,
            label: label.to_owned(),
        }
    }
}

/// A condition's tree (`spec/visual.md`, *Conditions*).
///
/// # Errors
///
/// `limit_exceeded` for a diagram of more than `max_diagram_size` elements.
pub fn draw_condition(node: &Node, format: DiagramFormat, limits: &Limits) -> Result<String> {
    Ok(render::render(&model::condition(node, limits)?, format))
}

/// A rule: its condition's tree, its outputs and its action (`spec/visual.md`,
/// *Rules*).
///
/// # Errors
///
/// `limit_exceeded` for a diagram of more than `max_diagram_size` elements.
pub fn draw_rule(rule: &Rule, format: DiagramFormat, limits: &Limits) -> Result<String> {
    Ok(render::render(&model::rule(rule, limits)?, format))
}

/// A rule set, from its summary: the fields each rule reads, its outputs and
/// its rank under the policy (`spec/visual.md`, *Rule sets*). It takes the
/// [`Summary`] rather than the [`RuleSet`](crate::rules::RuleSet) because the
/// summary carries what the core cannot know, which rules have an action
/// given as code: a binding marks them with [`Summary::with_actions`], and
/// `draw_ruleset(&summary(&rules), …)` draws a rule set as the core sees it.
///
/// # Errors
///
/// `limit_exceeded` for a diagram of more than `max_diagram_size` elements.
pub fn draw_ruleset(
    summary: &Summary<'_>,
    format: DiagramFormat,
    limits: &Limits,
) -> Result<String> {
    Ok(render::render(&model::ruleset(summary, limits)?, format))
}

/// An explanation: each rule's requirement as a tree of causes, marked with
/// their truth values and counterfactuals, the selected rules highlighted
/// (`spec/visual.md`, *Explanations*).
///
/// ```
/// use cleanrules::explain::explain;
/// use cleanrules::json::Limits;
/// use cleanrules::visual::{DiagramFormat, draw_explanation};
/// use cleanrules::{Map, Value};
///
/// let rules = cleanrules::text::parse("RULE adult IF age >= 18", &Limits::default())?
///     .into_ruleset()?;
/// let mut person = Map::new();
/// person.insert("age", Value::Int(16));
/// let decision = rules.evaluate(&Value::Map(person))?;
/// let dot = draw_explanation(&explain(&decision), DiagramFormat::Dot, &Limits::default())?;
/// assert!(dot.contains(r#"n1 [label="age >= 18\nfalse (age = 16)\nshort by 2", class="truth_false""#));
/// # Ok::<(), cleanrules::Error>(())
/// ```
///
/// # Errors
///
/// `limit_exceeded` for a diagram of more than `max_diagram_size` elements.
pub fn draw_explanation(
    explanation: &Explanation<'_, '_>,
    format: DiagramFormat,
    limits: &Limits,
) -> Result<String> {
    Ok(render::render(
        &model::explanation(explanation, limits)?,
        format,
    ))
}

/// A decision table's grid, in HTML, with the status of each row's rule when a
/// decision of its rule set is given (`spec/visual.md`, *Decision tables*).
///
/// ```
/// use cleanrules::json::Limits;
/// use cleanrules::table::DecisionTable;
/// use cleanrules::visual::{DiagramFormat, draw_table};
/// use cleanrules::{Map, Value};
///
/// let limits = Limits::default();
/// let table = DecisionTable::from_csv("HIT POLICY,first\nRULE,age,THEN ok\nadult,>= 18,TRUE\n", &limits)?;
/// let mut person = Map::new();
/// person.insert("age", Value::Int(20));
/// let decision = table.ruleset().evaluate(&Value::Map(person))?;
/// let html = draw_table(&table, Some(&decision), DiagramFormat::HtmlFragment, &limits)?;
/// assert!(html.contains(r#"<th scope="row">adult</th><td>selected</td>"#));
/// assert!(draw_table(&table, None, DiagramFormat::Mermaid, &limits).is_err());
/// # Ok::<(), cleanrules::Error>(())
/// ```
///
/// # Errors
///
/// `unsupported` for DOT and Mermaid, `decision_mismatch` for a decision of
/// another rule set, and `limit_exceeded` for a grid of more than
/// `max_diagram_size` cells.
pub fn draw_table(
    table: &DecisionTable,
    decision: Option<&Decision<'_>>,
    format: DiagramFormat,
    limits: &Limits,
) -> Result<String> {
    if !matches!(format, DiagramFormat::Html | DiagramFormat::HtmlFragment) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!("a decision table cannot be drawn as {}", format.as_str()),
        )
        .with_hint("draw it as HTML, or draw its rule set or the explanation of a decision")
        .unsupported(format.as_str(), "decision tables are drawn as HTML grids"));
    }
    let body = grid::grid(table, decision, limits)?;
    Ok(render::html(model::Kind::Table, &body, format))
}

/// A document's diagram: a condition's, a rule's, a rule set's (from its
/// summary) or a decision table's grid, with the statuses of `decision` when
/// one is given for a table.
///
/// # Errors
///
/// As the function for its content; `unsupported` for a decision given with
/// a document that is not a decision table.
pub fn draw_document(
    document: &Document,
    decision: Option<&Decision<'_>>,
    format: DiagramFormat,
    limits: &Limits,
) -> Result<String> {
    if let (Some(_), false) = (decision, matches!(document, Document::DecisionTable(_))) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "a decision is drawn with a decision table's grid only",
        )
        .with_hint("draw the document without the decision, or draw the decision's explanation")
        .unsupported("draw", "only a decision table's grid shows a decision"));
    }
    match document {
        Document::Condition(node) => draw_condition(node, format, limits),
        Document::Rule(rule) => draw_rule(rule, format, limits),
        Document::RuleSet(ruleset) => draw_ruleset(&summary(ruleset), format, limits),
        Document::DecisionTable(table) => draw_table(table, decision, format, limits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_have_names() {
        for format in [
            DiagramFormat::Dot,
            DiagramFormat::Mermaid,
            DiagramFormat::Html,
            DiagramFormat::HtmlFragment,
        ] {
            assert_eq!(DiagramFormat::from_name(format.as_str()), Some(format));
        }
        assert_eq!(DiagramFormat::from_name("svg"), None);
        assert_eq!(DiagramFormat::from_name("DOT"), None);
    }

    #[test]
    fn degrees_are_marked_by_their_interval() {
        let mark = |lo, hi| {
            let m = Mark::degree(lo, hi);
            (m.class.as_str(), m.label)
        };
        assert_eq!(mark(1.0, 1.0), ("truth_true", "1".to_owned()));
        assert_eq!(mark(0.0, 0.0), ("truth_false", "0".to_owned()));
        assert_eq!(mark(0.0, 1.0), ("truth_unknown", "[0, 1]".to_owned()));
        assert_eq!(mark(0.7, 0.7), ("truth_degree", "0.7".to_owned()));
        assert_eq!(
            mark(0.2, 0.900_000_000_000_000_1),
            ("truth_degree", "[0.2, 0.9]".to_owned())
        );
        assert_eq!(mark(1e-7, 1e-7), ("truth_degree", "1e-7".to_owned()));
    }

    #[test]
    fn kleene_truth_values_are_words() {
        let words: Vec<(&str, String)> = [Truth::True, Truth::False, Truth::Unknown]
            .iter()
            .map(|t| {
                let m = t.to_mark();
                (m.class.as_str(), m.label)
            })
            .collect();
        assert_eq!(
            words,
            [
                ("truth_true", "true".to_owned()),
                ("truth_false", "false".to_owned()),
                ("truth_unknown", "unknown".to_owned())
            ]
        );
    }
}
