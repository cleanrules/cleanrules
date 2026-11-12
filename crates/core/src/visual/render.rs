//! Writing a diagram as DOT, Mermaid or HTML (`spec/visual.md`, *Formats*).

use std::fmt::Write as _;

use super::DiagramFormat;
use super::model::{Class, Diagram, Kind, Shape, Stroke};
use crate::json::Json;

/// A diagram in a format.
pub(super) fn render(diagram: &Diagram, format: DiagramFormat) -> String {
    match format {
        DiagramFormat::Dot => dot(diagram),
        DiagramFormat::Mermaid => mermaid(diagram),
        DiagramFormat::Html | DiagramFormat::HtmlFragment => {
            let body = if diagram.kind == Kind::RuleSet {
                rule_table(diagram)
            } else {
                tree(diagram)
            };
            html(diagram.kind, &wrap(diagram, &body), format)
        }
    }
}

// --- DOT -----------------------------------------------------------------------------------

/// A label or title inside a DOT string: `\` and `"` escaped, so that no
/// escape of Graphviz applies.
fn dot_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

fn dot(diagram: &Diagram) -> String {
    let direction = if diagram.kind.across() { "LR" } else { "TB" };
    let mut out = String::from("digraph cleanrules {\n");
    let _ = write!(out, "  graph [rankdir={direction}, fontname=\"Helvetica\"");
    if let Some(title) = &diagram.title {
        let _ = write!(out, ", label=\"{}\", labelloc=t", dot_text(title));
    }
    out.push_str("];\n  node [fontname=\"Helvetica\", fontsize=11];\n");
    out.push_str("  edge [color=\"#5f6368\"];\n");
    for (i, node) in diagram.nodes.iter().enumerate() {
        let label: Vec<String> = node.lines.iter().map(|l| dot_text(l)).collect();
        let look = node.class.look();
        let shape = if look.shape == Shape::Round {
            "ellipse"
        } else {
            "box"
        };
        let mut style = Vec::new();
        if look.shape == Shape::Rounded {
            style.push("rounded");
        }
        style.push("filled");
        match look.stroke {
            Stroke::Bold => style.push("bold"),
            Stroke::Dashed => style.push("dashed"),
            Stroke::Plain => {}
        }
        let _ = writeln!(
            out,
            "  n{i} [label=\"{}\", class=\"{}\", shape={shape}, style=\"{}\", fillcolor=\"{}\", color=\"{}\"];",
            label.join("\\n"),
            node.class.name(),
            style.join(","),
            look.fill,
            look.color
        );
    }
    for edge in &diagram.edges {
        let style = if edge.dashed { " [style=dashed]" } else { "" };
        let _ = writeln!(out, "  n{} -> n{}{style};", edge.from, edge.to);
    }
    out.push_str("}\n");
    out
}

// --- Mermaid -------------------------------------------------------------------------------

/// A label inside a Mermaid string, with entity codes for the characters that
/// would end it, be read as markup, an entity or a line break (`\n`), or start
/// a Markdown string.
fn mermaid_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("#quot;"),
            '&' => out.push_str("#amp;"),
            '<' => out.push_str("#lt;"),
            '>' => out.push_str("#gt;"),
            '#' => out.push_str("#35;"),
            '`' => out.push_str("#96;"),
            // Mermaid reads `\n` as a line break.
            '\\' => out.push_str("#92;"),
            c => out.push(c),
        }
    }
    out
}

fn mermaid(diagram: &Diagram) -> String {
    let mut out = String::new();
    if let Some(title) = &diagram.title {
        // A JSON string is a YAML double-quoted string.
        let quoted = crate::json::to_string(&Json::String(title.clone()));
        let _ = write!(out, "---\ntitle: {quoted}\n---\n");
    }
    let direction = if diagram.kind.across() { "LR" } else { "TD" };
    let _ = writeln!(out, "flowchart {direction}");
    if let Some(title) = &diagram.title {
        // For screen readers, escaped as labels are: Mermaid decodes entity
        // codes in it, and keeps it exact (the front-matter title is shown
        // after an HTML sanitizer, which may alter `<`, `>` and `&`).
        let _ = writeln!(out, "  accTitle: {}", mermaid_text(title));
    }
    for (i, node) in diagram.nodes.iter().enumerate() {
        let label: Vec<String> = node.lines.iter().map(|l| mermaid_text(l)).collect();
        let (open, close) = match node.class.look().shape {
            Shape::Round => ("([", "])"),
            Shape::Box => ("[", "]"),
            Shape::Rounded => ("(", ")"),
            Shape::Output => ("[[", "]]"),
        };
        let _ = writeln!(out, "  n{i}{open}\"{}\"{close}", label.join("<br>"));
    }
    for edge in &diagram.edges {
        let arrow = if edge.dashed { "-.->" } else { "-->" };
        let _ = writeln!(out, "  n{} {arrow} n{}", edge.from, edge.to);
    }
    let used: Vec<(Class, Vec<String>)> = Class::ALL
        .iter()
        .map(|&class| {
            let ids = diagram
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.class == class)
                .map(|(i, _)| format!("n{i}"))
                .collect();
            (class, ids)
        })
        .filter(|(_, ids): &(Class, Vec<String>)| !ids.is_empty())
        .collect();
    for (class, _) in &used {
        let look = class.look();
        let stroke = match look.stroke {
            Stroke::Plain => "",
            Stroke::Bold => ",stroke-width:2px",
            Stroke::Dashed => ",stroke-dasharray:4 3",
        };
        let _ = writeln!(
            out,
            "  classDef {} fill:{},stroke:{}{stroke}",
            class.name(),
            look.fill,
            look.color
        );
    }
    for (class, ids) in &used {
        let _ = writeln!(out, "  class {} {}", ids.join(","), class.name());
    }
    out
}

// --- HTML ------------------------------------------------------------------------------------

/// Text in HTML, escaped.
pub(super) fn html_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A description in HTML: escaped, its line breaks kept (`\r\n` and `\r`
/// read as `\n`), and any other control character but the tab as U+FFFD.
pub(super) fn html_description(text: &str) -> String {
    let lines = text.replace("\r\n", "\n").replace('\r', "\n");
    let clean: String = lines
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect();
    html_text(&clean)
}

/// The style of every diagram, scoped to `.cleanrules`.
fn style() -> String {
    let mut css = String::from(
        ".cleanrules{font-family:Helvetica,Arial,sans-serif;font-size:13px;color:#202124}\n\
         .cleanrules .title{font-weight:bold;margin:0 0 8px}\n\
         .cleanrules ul.tree,.cleanrules ul.tree ul{list-style:none;margin:0;padding-left:24px}\n\
         .cleanrules ul.tree{padding-left:0}\n\
         .cleanrules ul.tree ul{border-left:1px solid #dadce0}\n\
         .cleanrules li{margin:4px 0}\n\
         .cleanrules .node{display:inline-block;border-radius:3px;padding:2px 6px}\n\
         .cleanrules .description{white-space:pre-wrap;color:#5f6368;font-style:italic}\n\
         .cleanrules .payload{font-style:italic}\n\
         .cleanrules table{border-collapse:collapse}\n\
         .cleanrules caption{font-weight:bold;text-align:left;margin-bottom:4px}\n\
         .cleanrules th,.cleanrules td{border:1px solid #dadce0;padding:2px 6px;text-align:left;vertical-align:top}\n",
    );
    for class in Class::ALL {
        let _ = writeln!(css, ".cleanrules .{}{{{}}}", class.name(), class.css());
    }
    css
}

/// A diagram's body in its `div`, with its title.
fn wrap(diagram: &Diagram, content: &str) -> String {
    let mut out = format!(
        "<div class=\"cleanrules cleanrules-{}\">\n",
        diagram.kind.css()
    );
    if let Some(title) = &diagram.title {
        let _ = writeln!(out, "<p class=\"title\">{}</p>", html_text(title));
    }
    out.push_str(content);
    out.push_str("</div>\n");
    out
}

/// A node's box: its lines, and its description.
fn node_box(diagram: &Diagram, i: usize) -> String {
    let node = &diagram.nodes[i];
    let mut out = format!(
        "<div class=\"node {}\" style=\"{}\">",
        node.class.name(),
        node.class.css()
    );
    for line in &node.lines {
        let _ = write!(out, "<div class=\"line\">{}</div>", html_text(line));
    }
    if let Some(description) = &node.description {
        let _ = write!(
            out,
            "<div class=\"description\">{}</div>",
            html_description(description)
        );
    }
    out.push_str("</div>");
    out
}

/// What is left to write of a tree: a node, or the end of a node with
/// children.
enum Step {
    Open(usize),
    Close,
}

/// A tree (or forest) as nested lists.
fn tree(diagram: &Diagram) -> String {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); diagram.nodes.len()];
    let mut has_parent = vec![false; diagram.nodes.len()];
    for edge in &diagram.edges {
        children[edge.from].push(edge.to);
        has_parent[edge.to] = true;
    }
    let mut out = String::from("<ul class=\"tree\">\n");
    let roots = (0..diagram.nodes.len()).filter(|&i| !has_parent[i]);
    // An explicit stack: a tree is as deep as its condition.
    let mut pending: Vec<Step> = roots.rev().map(Step::Open).collect();
    while let Some(step) = pending.pop() {
        match step {
            Step::Open(i) => {
                let _ = write!(out, "<li>{}", node_box(diagram, i));
                if children[i].is_empty() {
                    out.push_str("</li>\n");
                } else {
                    out.push_str("\n<ul>\n");
                    pending.push(Step::Close);
                    pending.extend(children[i].iter().rev().map(|&c| Step::Open(c)));
                }
            }
            Step::Close => out.push_str("</ul>\n</li>\n"),
        }
    }
    out.push_str("</ul>\n");
    out
}

/// A rule set as a table: a row per rule, with the fields it reads and its
/// outputs.
fn rule_table(diagram: &Diagram) -> String {
    let count = diagram.nodes.len();
    let mut into: Vec<Vec<(usize, bool)>> = vec![Vec::new(); count];
    let mut from: Vec<Vec<usize>> = vec![Vec::new(); count];
    for edge in &diagram.edges {
        into[edge.to].push((edge.from, edge.dashed));
        from[edge.from].push(edge.to);
    }
    let label = |i: usize| html_text(&diagram.nodes[i].lines.join(" "));
    let mut out = String::from(
        "<table>\n<thead><tr><th>rule</th><th>reads</th><th>outputs</th></tr></thead>\n<tbody>\n",
    );
    for (i, node) in diagram.nodes.iter().enumerate() {
        if node.class != Class::Rule {
            continue;
        }
        let _ = write!(out, "<tr><td>{}</td><td>", node_box(diagram, i));
        for &(field, payload) in &into[i] {
            if payload {
                let _ = write!(
                    out,
                    "<div class=\"line payload\">{} (payload)</div>",
                    label(field)
                );
            } else {
                let _ = write!(out, "<div class=\"line\">{}</div>", label(field));
            }
        }
        out.push_str("</td><td>");
        for &output in &from[i] {
            let _ = write!(out, "<div class=\"line\">{}</div>", label(output));
        }
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody>\n</table>\n");
    out
}

/// A diagram's HTML body as a standalone document or a fragment.
pub(super) fn html(kind: Kind, body: &str, format: DiagramFormat) -> String {
    let style = style();
    if format == DiagramFormat::HtmlFragment {
        return format!("<style>\n{style}</style>\n{body}");
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>cleanrules: {}</title>\n<style>\n{style}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        kind.title()
    )
}

/// The `div` of a table's grid.
pub(super) fn table_div(content: &str) -> String {
    format!(
        "<div class=\"cleanrules cleanrules-{}\">\n{content}</div>\n",
        Kind::Table.css()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_strings_escape_backslashes_and_quotes() {
        assert_eq!(dot_text(r#"a "b" \N \l"#), r#"a \"b\" \\N \\l"#);
        assert_eq!(dot_text("{x|y} <p> é😀"), "{x|y} <p> é😀");
    }

    #[test]
    fn mermaid_labels_use_entity_codes() {
        assert_eq!(
            mermaid_text(r#"a "b" <i> & #x; `md` [c] (d) {e} |f| é \n"#),
            "a #quot;b#quot; #lt;i#gt; #amp; #35;x; #96;md#96; [c] (d) {e} |f| é #92;n"
        );
    }

    #[test]
    fn html_text_is_escaped() {
        assert_eq!(
            html_text(r#"<script>alert('x' & "y")</script>"#),
            "&lt;script&gt;alert(&#39;x&#39; &amp; &quot;y&quot;)&lt;/script&gt;"
        );
        assert_eq!(
            html_description("a\r\nb\rc\td\u{0}e\u{1b}"),
            "a\nb\nc\td\u{fffd}e\u{fffd}"
        );
    }
}
