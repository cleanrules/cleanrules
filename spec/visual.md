# Diagrams

**Status:** draft. Normative for every implementation: the core writes every
diagram, so a diagram is the same text in every language (`CLAUDE.md`,
invariants 2 and 8).

Conditions, rules, rule sets, explanations and decision tables can be drawn as
diagrams. A diagram is plain text in one of four formats, written by the core
with no graphics library: drawing or showing it is up to the caller, such as
Graphviz for DOT, a Markdown viewer for Mermaid, or a browser or notebook for
HTML.

| Format | Name | What it is |
|---|---|---|
| DOT | `dot` | A Graphviz graph (`dot -Tsvg`) |
| Mermaid | `mermaid` | A Mermaid flowchart, which GitHub, GitLab and many documentation tools draw |
| HTML | `html` | A standalone HTML document: no script, no external resource |
| HTML fragment | `html_fragment` | The same content as an element and its style, to embed in a page or a notebook |

Every diagram is drawn in every format, except decision tables, which are
drawn in HTML only (*Decision tables*). The same object, format and limits
always give the same text, byte for byte, on every platform.

## Diagram content

A diagram is a list of **nodes** and a list of **edges**, in the orders given
below, and may have a **title**. Each node has an id, `n<k>` for the `k`-th
node from 0, one or more **lines** of text, and a **class**, which says how it
is drawn. Each edge goes from one node to another, and is solid or dashed.
Conditions, rules and explanations are trees, drawn top to bottom; rule sets
are drawn left to right.

Text in the nodes is written as the explanation's text writes it
(`explain.md`, *Text*): conditions, field paths, names and constants in the
canonical text (`text.md`, *Printing*), and values read from the data bounded
and escaped. So no line holds a control character.

### Conditions

The nodes of a condition are its nodes in depth-first pre-order, children in the
order of *Conditions* in `json-format.md`, except that only `and`, `or` and
`not` have nodes for their operands:

- `and` is a node of class `connective` with the line `all`, `or` one with
  `any`, and `not` one with `not`, each with an edge to each operand;
- every other node is a **leaf** of class `leaf`, whose line is its canonical
  text: `x > 10`, `country IN ("ES", "PT")`, `age BETWEEN 18 AND 65`. A leaf is
  not split further.

The edges go from each connective to its operands, in pre-order of the operands.

### Rules

A rule's diagram has, in order:

1. the rule's node, of class `rule`, with the line `RULE <name>`, and
   ` PRIORITY <p>` after it when the priority is not 0, as the text's header
   writes them;
2. the nodes of its condition, as above, with an edge from the rule's node to
   the condition's root;
3. a node of class `output` for each payload entry, in order, with the line
   `THEN <key> = <value>`, and a dashed edge from the rule's node;
4. a node of class `output` with the line `DO <action>` when the rule names its
   action, with a dashed edge from the rule's node.

The rule's description is shown in HTML only (*HTML*).

### Rule sets

A rule set's diagram shows the data of its summary (`explain.md`,
*Summaries*). Its title is the policy, `HIT POLICY <policy>`, followed by
` AGGREGATE <aggregate>` when there is one. The rules are listed in the
policy's **rank**: under `priority`, by priority from highest to lowest, ties
in rule order; under every other policy, in rule order. Its nodes are, in
order:

1. a node of class `field` for each field the rule set reads, in the summary's
   order, whose line is the field's path (`the input` for the empty path);
2. a node of class `rule` for each rule, in rank order, with the lines
   `<rank>. <name>` (the rank from 1), `PRIORITY <p>` under `priority` only, the
   condition, and `DO <action>` for a named action or `+action` for an action
   given as code (as the summary's text marks them), when it has one;
3. a node of class `output` for each payload key, in the summary's order, whose
   line is the key.

The edges are, for each rule in rank order: a solid edge from each field its
condition reads, in the fields' order; a dashed edge from each field that only
its payload values read, in the fields' order; and a solid edge to the output of
each of its payload keys, in payload order.

### Explanations

An explanation's diagram (`explain.md`) shows each rule's requirement as a
tree. Its title is the header of the explanation's text
(`HIT POLICY priority: selected e`). For each rule, in rule order, it has:

1. the rule's node, with the lines `<mark> <name> (<status>)`, as the text's
   first line writes them (`[x] e (selected)`, `[ ] a (not fired)`), then the
   condition, then the note when there is one, then
   `cannot fire by changing these conditions alone; they conflict` when the
   rule is not achievable. Its class is the status: `rule_selected`, which is
   highlighted, `rule_fired`, `rule_not_fired` or `rule_unknown`;
2. the nodes of its requirement in pre-order, with an edge from the rule's node
   to the requirement's root and from each group to its items:
   - a group is a node of class `connective` with the line `all of` or
     `any of`;
   - a cause is a node whose class is its truth value's (*Truth values*), with
     up to three lines: its condition; its truth value's label, then the values
     it read in parentheses (`false (x = 8)`), then the reason after a colon
     when it is unknown, or `, but the rule needs it false` when it does not
     hold and is negated; and its counterfactual, when it has one, as the text
     writes it after the semicolon (`short by 2`,
     `needs y > 3 (short by 1)`, `at the limit (needs x >= 5)`).

### Truth values

*Not checked by vectors until phase J: the degrees of graded logics come with
it, and the core's tests fix the labels.*

A cause's truth value gives its label and its class. Under Kleene's logic, the
label is `true`, `false` or `unknown`, and the class `truth_true`,
`truth_false` or `truth_unknown`.

Under a graded logic, a truth value is an interval `[lo, hi]` of degrees
(`ast.md`, *Truth values*), with `0 <= lo <= hi <= 1`. Each end is written
as a float gap is (`explain.md`, *Text*: 12 significant digits, `0.7`, `1`).
The label is the written `lo` when both ends are written the same, and
`[lo, hi]` otherwise (`[0, 1]`). The class is read from the written ends too,
so that it always agrees with the label: `truth_true` for `1` and `1`,
`truth_false` for `0` and `0`, `truth_unknown` for `0` and `1`, and
`truth_degree` otherwise.

## Decision tables

A decision table (`tables.md`) is drawn as its **grid**, in HTML only; DOT and
Mermaid are `unsupported` (its rule set and its decisions' explanations can be
drawn in every format). The grid is a table with the caption
`HIT POLICY <policy>`, the table's policy (`output_order` included), followed by
` AGGREGATE <aggregate>` when there is one, and one header row and one row per
table row, in row order. Its columns are, in order:

- `RULE`: the name of the row's rule (`row_<n>` when the row has none,
  `tables.md`, *Compiling*);
- `STATUS`, when a decision is given: the status of the row's rule, in the
  words of the explanation's text: `selected`, `fired, not selected`,
  `not fired` or `unknown`;
- `PRIORITY`, when a row has a priority: the row's, or empty;
- one column per input, headed by its canonical text, with each cell's
  canonical text, or `-` for any value (`tables.md`, *Canonical text*);
- one column per output, headed by `THEN <key>` or `THEN <key> IN (…)`, with
  each cell's canonical text, or empty;
- `DESCRIPTION`, when a row has one: the row's, or empty.

These are the header and cells of the table's CSV (`tables.md`, *CSV*), with
the rule names always written. When a decision is given, each row has the
class of its rule's status (`rule_selected`, `rule_fired`, `rule_not_fired`,
`rule_unknown`), so the rows that fired are highlighted. The decision must be
one of the table's rule set: one whose rules are the table's, by count and by
name; any other is a `decision_mismatch`, whose message names the first row
whose rule the decision lacks, or the two counts. A document is drawn with a
decision only when it is a decision table; a decision with any other document
is `unsupported` (draw the decision's explanation instead).

## Formats

Every format ends with a line break, and every line ends with `\n`.

### Classes

Each class is drawn with a fill colour, a stroke colour, a shape and a style,
the same in every format:

| Class | Fill | Stroke | DOT shape | Mermaid shape | Style |
|---|---|---|---|---|---|
| `connective` | `#ffffff` | `#5f6368` | `ellipse` | stadium `([…])` | |
| `leaf` | `#f8f9fa` | `#5f6368` | `box` | rectangle `[…]` | |
| `rule` | `#ffffff` | `#202124` | `box`, rounded | rounded `(…)` | |
| `rule_selected` | `#e8f0fe` | `#1a73e8` | `box`, rounded | rounded `(…)` | bold |
| `rule_fired` | `#ffffff` | `#1a73e8` | `box`, rounded | rounded `(…)` | |
| `rule_not_fired` | `#ffffff` | `#5f6368` | `box`, rounded | rounded `(…)` | |
| `rule_unknown` | `#f1f3f4` | `#80868b` | `box`, rounded | rounded `(…)` | dashed |
| `truth_true` | `#e6f4ea` | `#1e8e3e` | `box` | rectangle `[…]` | |
| `truth_false` | `#fce8e6` | `#d93025` | `box` | rectangle `[…]` | |
| `truth_unknown` | `#f1f3f4` | `#80868b` | `box` | rectangle `[…]` | dashed |
| `truth_degree` | `#fef7e0` | `#f9ab00` | `box` | rectangle `[…]` | |
| `field` | `#ffffff` | `#5f6368` | `ellipse` | stadium `([…])` | |
| `output` | `#f3e8fd` | `#9334e6` | `box` | subroutine `[[…]]` | |

Colour is never the only mark: a rule's status and a cause's truth value are
also written in their lines.

### DOT

```text
digraph cleanrules {
  graph [rankdir=TB, fontname="Helvetica", label="HIT POLICY first: selected a", labelloc=t];
  node [fontname="Helvetica", fontsize=11];
  edge [color="#5f6368"];
  n0 [label="[x] a (selected)\nx > 0", class="rule_selected", shape=box, style="rounded,filled,bold", fillcolor="#e8f0fe", color="#1a73e8"];
  n1 [label="x > 0\ntrue (x = 8)", class="truth_true", shape=box, style="filled", fillcolor="#e6f4ea", color="#1e8e3e"];
  n0 -> n1;
}
```

- The graph is `digraph cleanrules`, with `rankdir=TB` or `rankdir=LR`, and the
  title as its `label` with `labelloc=t` when it has one.
- Each node is a statement with its `label`, its `class` (which Graphviz's SVG
  output keeps), its shape, its style (`rounded` for rounded shapes, `filled`,
  then `bold` or `dashed`) and its colours, in that order.
- Each edge is `n<i> -> n<j>;`, or `n<i> -> n<j> [style=dashed];`.
- Labels and the title are double-quoted strings: `\` is written `\\` and `"`
  is written `\"`, so that no escape of Graphviz (`\N`, `\l`) applies, and the
  lines of a label are joined with `\n`.

### Mermaid

```text
---
title: "HIT POLICY first: selected a"
---
flowchart TD
  accTitle: HIT POLICY first: selected a
  n0("[x] a (selected)<br>x #gt; 0")
  n1["x #gt; 0<br>true (x = 8)"]
  n0 --> n1
  classDef rule_selected fill:#e8f0fe,stroke:#1a73e8,stroke-width:2px
  classDef truth_true fill:#e6f4ea,stroke:#1e8e3e
  class n0 rule_selected
  class n1 truth_true
```

- A title, when there is one, is front matter: `title:` and the title as a JSON
  string, which YAML reads as a double-quoted string. Mermaid 10 or later draws
  it (GitHub and GitLab do). Mermaid shows it after an HTML sanitizer, so a
  title holding `<`, `>` or `&` (which only a quoted rule name can bring) may
  be shown altered, never as markup; the title is also given exactly, for
  screen readers, as `accTitle:` with the label escaping below, right after
  the `flowchart` line.
- Then `flowchart TD` or `flowchart LR`, the `accTitle:` line, the nodes with
  their shape, the edges
  (`-->`, or `-.->` for dashed ones), one `classDef` for each class used, in
  the order of the table above (`stroke-width:2px` for bold,
  `stroke-dasharray:4 3` for dashed), and one `class` statement for each class
  used, in the same order, with the ids of its nodes in node order, joined by
  commas.
- Labels are double-quoted, and these characters are written as Mermaid's
  entity codes: `"` as `#quot;`, `&` as `#amp;`, `<` as `#lt;`, `>` as `#gt;`,
  `#` as `#35;`, `` ` `` as `#96;` and `\` as `#92;`. So no label holds a
  quote, markup, an entity of its own, a Markdown string or `\n`, which
  Mermaid draws as a line break. The lines of a label are joined with `<br>`.
- Mermaid renderers bound what they draw: by default a text of at most 50,000
  characters (`maxTextSize`) and 500 edges (`maxEdges`), and they show an
  error instead of a larger diagram. Large rule sets and explanations exceed
  them well before `max_diagram_size`: draw a part (one rule, one decision's
  explanation), use DOT or HTML, or raise those settings where the renderer
  can be configured (not on GitHub).

### HTML

An HTML diagram is a document with no script and no external resource, which
can be opened from a file, attached to an e-mail or shown in a notebook:

```text
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>cleanrules: explanation</title>
<style>…</style>
</head>
<body>
<div class="cleanrules cleanrules-explanation">
<p class="title">HIT POLICY first: selected a</p>
<ul class="tree">
<li><div class="node rule_selected" style="background:#e8f0fe;border:2px solid #1a73e8;border-radius:8px">…</div>
…
</div>
</body>
</html>
```

- The `<title>` names the kind of diagram: `condition`, `rule`, `rule set`,
  `explanation` or `decision table`. The style is the same for every diagram.
- The fragment (`html_fragment`) is the `<style>` element, then the
  `<div class="cleanrules cleanrules-<kind>">` element, each on its own line,
  as in the document's head and body. The kind is written the same way in
  both places: `condition`, `rule`, `rule set`, `explanation` and
  `decision table` in the `<title>`, with hyphens in the class
  (`cleanrules-rule-set`, `cleanrules-decision-table`).
- Each class has CSS declarations made from its row of *Classes*:
  `background:<fill>;border:<width> <line> <stroke>`, with a width of `2px`
  for bold and `1px` otherwise, a line `dashed` for dashed and `solid`
  otherwise, and `;border-radius:12px` for an ellipse or stadium or
  `;border-radius:8px` for a rounded box. The `<style>` element gives them as
  `.cleanrules .<class>{…}`, and every element with a class of the table (a
  node's `div`, a grid's row) also carries them in its `style` attribute, so
  that the colours survive where the `<style>` element is removed, as some
  notebook and e-mail sanitizers do.
- Inside the `div`: the title, when there is one, as `<p class="title">`; then
  a tree diagram as nested lists (`<ul class="tree">`, an `<li>` per node with
  `<div class="node <class>">` and its lines as `<div class="line">`, and its
  children in a nested `<ul>`); a rule set as a table with a row per rule: the
  rule's lines, the fields with an edge to it (those of dashed edges marked
  `(payload)`), and the outputs it has an edge to; a decision table as its
  grid.
- A rule's description, which may span lines, is shown under its lines in the
  rule's diagram, its rule set's rows and its explanation, as
  `<div class="description">`, and in the grid's `DESCRIPTION` column, with
  its line breaks and spaces kept (`white-space: pre-wrap`).
- All text is escaped: `&` as `&amp;`, `<` as `&lt;`, `>` as `&gt;`, `"` as
  `&quot;` and `'` as `&#39;`. In a description, `\r\n` and `\r` are line
  breaks, and any other control character but the tab is written as U+FFFD.

## Limits

A diagram is bounded, so that a large rule set or an input cannot make it
large:

- each line of a node, the title, and each cell of a grid, is at most **100**
  characters, counted as Unicode scalar values, before escaping: a longer one
  is cut to its first 97 and `...`, so an escape is never cut in half. Before
  that, any control character (Unicode category `Cc`: U+0000 to U+001F and
  U+007F to U+009F), which the text of labels never holds (*Diagram
  content*), is replaced by U+FFFD. Descriptions are not cut (*HTML*).
- a diagram has at most **`max_diagram_size`** elements, **20,000** by
  default: its nodes and edges, or the header and cells of a grid. A larger
  one is not drawn: `limit_exceeded`, with `name` set to `max_diagram_size`
  (`errors.md`), and a message that names the kind of diagram. The count is
  the same in every format, and the caller can raise the limit, never disable
  it.

So, apart from descriptions, whose length is that of the rules, every
element (a node, an edge or a cell) writes at most 4 lines of at most 100
characters, each character in at most 6 bytes (`&quot;`, `#quot;`), and less
than 400 bytes of markup: with the default limit a diagram is under 64 MB,
and most are a few kilobytes. Values read from the data are already bounded as
in the explanation's text. Drawing never overflows the stack, whatever the
depth of a condition.
