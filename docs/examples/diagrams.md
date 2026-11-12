# Diagrams

The core draws conditions, rules, rule sets, explanations and decision tables
as text: DOT for Graphviz, Mermaid for GitHub, GitLab and documentation sites,
and standalone HTML with no script and no external resource
([`spec/visual.md`](../../spec/visual.md)). The same object gives the same
text in every language. The rule set below is
[`discounts.rules`](discounts.rules).

In Rust:

```rust
use cleanrules::explain::{explain, summary};
use cleanrules::json::Limits;
use cleanrules::visual::{DiagramFormat, draw_explanation, draw_ruleset};

let limits = Limits::default();
let rules = cleanrules::text::parse(&std::fs::read_to_string("discounts.rules")?, &limits)?
    .into_ruleset()?;
println!("{}", draw_ruleset(&summary(&rules), DiagramFormat::Mermaid, &limits)?);

let decision = rules.evaluate(&order)?; // years 4, total 1500, not blocked
println!("{}", draw_explanation(&explain(&decision), DiagramFormat::Mermaid, &limits)?);
```

In C, through the C ABI (each text is the caller's, freed with
`cr_string_free`):

```c
char *mermaid = NULL;
if (cr_ruleset_draw(rules, NULL, 0, CR_DIAGRAM_MERMAID, NULL, &mermaid, NULL, &error) == CR_OK) {
    puts(mermaid);
    cr_string_free(mermaid);
}
```

`DiagramFormat::Dot`, `DiagramFormat::Html` and `DiagramFormat::HtmlFragment`
(`CR_DIAGRAM_DOT`, `CR_DIAGRAM_HTML`, `CR_DIAGRAM_HTML_FRAGMENT`) give the
other formats; the bindings draw them from phase V.2.

Mermaid renderers draw at most 50,000 characters (`maxTextSize`) and 500
edges (`maxEdges`) by default, and show an error instead of a larger diagram.
For a large rule set, draw one rule or one decision's explanation, use DOT or
HTML, or raise those settings where your renderer can be configured (GitHub's
cannot). Front-matter titles need Mermaid 10 or later.

## The rule set

Each rule with the fields it reads and the outputs it sets, in the order of
its policy (here, by priority):

```mermaid
---
title: "HIT POLICY priority"
---
flowchart LR
  accTitle: HIT POLICY priority
  n0(["customer.blocked"])
  n1(["customer.years"])
  n2(["order.total"])
  n3(["customer.newsletter"])
  n4("1. blocked<br>PRIORITY 99<br>customer.blocked")
  n5("2. vip<br>PRIORITY 10<br>customer.years #gt; 5 AND order.total #gt; 1000<br>DO notify_sales")
  n6("3. loyal<br>PRIORITY 5<br>customer.years #gt; 2 OR customer.newsletter")
  n7[["discount"]]
  n0 --> n4
  n4 --> n7
  n1 --> n5
  n2 --> n5
  n5 --> n7
  n1 --> n6
  n3 --> n6
  n6 --> n7
  classDef rule fill:#ffffff,stroke:#202124
  classDef field fill:#ffffff,stroke:#5f6368
  classDef output fill:#f3e8fd,stroke:#9334e6
  class n4,n5,n6 rule
  class n0,n1,n2,n3 field
  class n7 output
```

## An explanation

The decision for a customer of 4 years with an order of 1500: each rule with
the conditions that decide it, marked true (green) or false (red), how far a
value is from changing the outcome, and the selected rule highlighted:

```mermaid
---
title: "HIT POLICY priority: selected loyal"
---
flowchart TD
  accTitle: HIT POLICY priority: selected loyal
  n0("[ ] blocked (not fired)<br>customer.blocked")
  n1["customer.blocked<br>false (customer.blocked = FALSE)"]
  n2("[ ] vip (not fired)<br>customer.years #gt; 5 AND order.total #gt; 1000")
  n3["customer.years #gt; 5<br>false (customer.years = 4)<br>short by 1"]
  n4("[x] loyal (selected)<br>customer.years #gt; 2 OR customer.newsletter")
  n5["customer.years #gt; 2<br>true (customer.years = 4)"]
  n0 --> n1
  n2 --> n3
  n4 --> n5
  classDef rule_selected fill:#e8f0fe,stroke:#1a73e8,stroke-width:2px
  classDef rule_not_fired fill:#ffffff,stroke:#5f6368
  classDef truth_true fill:#e6f4ea,stroke:#1e8e3e
  classDef truth_false fill:#fce8e6,stroke:#d93025
  class n4 rule_selected
  class n0,n2 rule_not_fired
  class n5 truth_true
  class n1,n3 truth_false
```

## A decision table

A decision table is drawn as an HTML grid, with the rows that fired
highlighted when it is given a decision of its rule set
(`draw_table(&table, Some(&decision), DiagramFormat::Html, &limits)`).
