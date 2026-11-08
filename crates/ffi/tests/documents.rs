//! What rules, documents, decision tables and result trees hold, read through
//! the C ABI (G.1d): the rules of a rule set or a document, documents made of
//! a condition or a rule, one rule read from text, grids with nodes and an
//! open header, a table's settings, rows and cells, and result trees as
//! handles.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::ptr;

use cleanrules::json::same;
use cleanrules_ffi::*;
use common::{
    Document, Failure, Node, Value, arg, borrowed, evaluate, handle, json, kind, ok, ok_or,
    read_document, read_node, read_ruleset, ruleset_of, text, value_from_json, value_to_json,
    write_document,
};

/// An owned rule, freed on drop.
struct Rule(*mut cr_rule);

impl Drop for Rule {
    fn drop(&mut self) {
        // SAFETY: the rule came from the library and is freed once.
        unsafe { cr_rule_free(self.0) };
    }
}

/// An owned result tree, freed on drop.
struct Tree(*mut cr_tree);

impl Drop for Tree {
    fn drop(&mut self) {
        // SAFETY: the tree came from the library and is freed once.
        unsafe { cr_tree_free(self.0) };
    }
}

/// An owned grid, freed on drop unless finished.
struct Grid(*mut cr_grid);

impl Drop for Grid {
    fn drop(&mut self) {
        // SAFETY: the grid came from the library and is freed once.
        unsafe { cr_grid_free(self.0) };
    }
}

const RULES: &str = r#"HIT POLICY priority
RULE vip PRIORITY 10
DESCRIPTION "Loyal customers with big orders"
IF customer.years > 5 AND order.total > 1000
THEN discount = 0.15, label = "VIP"
DO notify_sales
RULE plain
IF true
"#;

fn utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

fn name(rule: &Rule) -> String {
    // SAFETY: a live rule; the name is copied before the rule is freed.
    utf8(borrowed(|t, l, e| unsafe { cr_rule_name(rule.0, t, l, e) }))
}

fn node_text(node: &Node) -> String {
    // SAFETY: a live node.
    text(|out, len, error| unsafe { cr_node_write(node.0, CR_FORMAT_TEXT, out, len, error) })
        .unwrap()
}

fn rule_at(ruleset: &common::RuleSet, position: usize) -> Result<Rule, Failure> {
    // SAFETY: a live rule set.
    handle(|out, error| unsafe { cr_ruleset_rule(ruleset.0, position, out, error) }).map(Rule)
}

#[test]
fn a_rule_set_gives_its_rules_with_all_they_hold() {
    let ruleset = read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let vip = rule_at(&ruleset, 0).unwrap();
    assert_eq!(name(&vip), "vip");
    // SAFETY: a live rule.
    unsafe {
        assert_eq!(cr_rule_priority(vip.0), 10);
        assert_eq!(cr_rule_payload_len(vip.0), 2);
    }
    // SAFETY: as above; texts are copied while the rule lives.
    let (description, action) = unsafe {
        (
            utf8(borrowed(|t, l, e| cr_rule_description(vip.0, t, l, e))),
            utf8(borrowed(|t, l, e| cr_rule_action(vip.0, t, l, e))),
        )
    };
    assert_eq!(description, "Loyal customers with big orders");
    assert_eq!(action, "notify_sales");
    // SAFETY: a live rule.
    let when = handle(|out, error| unsafe { cr_rule_when(vip.0, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(
        node_text(&when),
        "customer.years > 5 AND order.total > 1000"
    );
    for (i, (key, value)) in [("discount", "0.15"), ("label", "\"VIP\"")]
        .into_iter()
        .enumerate()
    {
        // SAFETY: a live rule.
        let found = utf8(borrowed(|t, l, e| unsafe {
            cr_rule_payload_key(vip.0, i, t, l, e)
        }));
        assert_eq!(found, key);
        // SAFETY: a live rule.
        let node = handle(|out, error| unsafe { cr_rule_payload_value(vip.0, i, out, error) })
            .map(Node)
            .unwrap();
        assert_eq!(node_text(&node), value);
    }
    // A rule without a description or an action lends NULL for them.
    let plain = rule_at(&ruleset, 1).unwrap();
    let (mut text_out, mut len) = (c"x".as_ptr(), 9);
    // SAFETY: a live rule and writable out-parameters.
    ok(|e| unsafe { cr_rule_description(plain.0, &raw mut text_out, &raw mut len, e) });
    assert!(text_out.is_null() && len == 0);
    // SAFETY: as above.
    ok(|e| unsafe { cr_rule_action(plain.0, &raw mut text_out, &raw mut len, e) });
    assert!(text_out.is_null() && len == 0);
    // SAFETY: a live rule; NULL is 0.
    unsafe {
        assert_eq!(cr_rule_priority(plain.0), 0);
        assert_eq!(cr_rule_payload_len(plain.0), 0);
        assert_eq!(cr_rule_priority(ptr::null()), 0);
        assert_eq!(cr_rule_payload_len(ptr::null()), 0);
    }
}

#[test]
fn positions_out_of_range_are_invalid_arguments() {
    let ruleset = read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let failure = rule_at(&ruleset, 2).err().unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let vip = rule_at(&ruleset, 0).unwrap();
    // SAFETY: a live rule.
    let failure = handle(|out, error| unsafe { cr_rule_payload_value(vip.0, 2, out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let (mut key, mut len) = (ptr::null(), 0);
    // SAFETY: a live rule and writable out-parameters.
    let failure =
        ok_or(|e| unsafe { cr_rule_payload_key(vip.0, 5, &raw mut key, &raw mut len, e) })
            .err()
            .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    assert!(key.is_null());
    // SAFETY: NULL handles.
    let failure = handle(|out, error| unsafe { cr_rule_when(ptr::null(), out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
}

#[test]
fn documents_give_their_condition_or_rule_and_are_made_of_them() {
    let rule_json = r#"{"version":1,"rule":{"name":"adult","when":{"op":"ge","left":{"op":"field","path":["age"]},"right":{"op":"const","value":18}},"then":{"band":{"op":"const","value":"adult"}}}}"#;
    let document = read_document(rule_json.as_bytes(), CR_FORMAT_JSON, None).unwrap();
    assert_eq!(kind(&document), CR_DOCUMENT_RULE);
    // SAFETY: a live document.
    let rule = handle(|out, error| unsafe { cr_document_rule(document.0, out, error) })
        .map(Rule)
        .unwrap();
    assert_eq!(name(&rule), "adult");
    // Written back exactly.
    // SAFETY: a live rule.
    let again = handle(|out, error| unsafe { cr_document_new_rule(rule.0, out, error) })
        .map(Document)
        .unwrap();
    assert_eq!(write_document(&again, CR_FORMAT_JSON).unwrap(), rule_json);
    // The wrong kind is an invalid argument.
    // SAFETY: a live document.
    let failure = handle(|out, error| unsafe { cr_document_condition(document.0, out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);

    let condition = read_document(b"age >= 18", CR_FORMAT_TEXT, None).unwrap();
    assert_eq!(kind(&condition), CR_DOCUMENT_CONDITION);
    // SAFETY: a live document.
    let node = handle(|out, error| unsafe { cr_document_condition(condition.0, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(node_text(&node), "age >= 18");
    // SAFETY: a live node.
    let made = handle(|out, error| unsafe { cr_document_new_condition(node.0, out, error) })
        .map(Document)
        .unwrap();
    assert_eq!(
        write_document(&made, CR_FORMAT_JSON).unwrap(),
        write_document(&condition, CR_FORMAT_JSON).unwrap()
    );
    // SAFETY: a live document.
    let failure = handle(|out, error| unsafe { cr_document_rule(condition.0, out, error) })
        .map(Rule)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);

    // A value node is no document.
    let value = read_node("price * 2", CR_FORMAT_TEXT).unwrap();
    // SAFETY: a live node.
    let failure = handle(|out, error| unsafe { cr_document_new_condition(value.0, out, error) })
        .map(Document)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_EXPRESSION_ERROR);
    assert!(
        cleanrules::json::to_string(&failure.json)
            .contains("only a condition can be a document, not the value price * 2")
    );
}

fn from_text(text: &str) -> Result<Rule, Failure> {
    let (p, n) = arg(text.as_bytes());
    // SAFETY: the text outlives the call.
    handle(|out, error| unsafe { cr_rule_from_text(p, n, ptr::null(), out, error) }).map(Rule)
}

#[test]
fn one_rule_is_read_from_text() {
    let rule = from_text("RULE vip IF customer.years > 5 THEN discount = 0.15").unwrap();
    assert_eq!(name(&rule), "vip");
    let unnamed = from_text("IF x > 1 THEN y = 2").unwrap();
    assert_eq!(name(&unnamed), "rule_1");
    for wrong in ["x > 1", "RULE a IF x RULE b IF y", "IF x >"] {
        let failure = from_text(wrong).err().unwrap();
        assert_eq!(failure.status, CR_RULE_SYNTAX_ERROR, "{wrong}");
    }
    let bytes = [b'I', b'F', b' ', 0xff];
    // SAFETY: the bytes outlive the call.
    let failure = handle(|out, error| unsafe {
        cr_rule_from_text(bytes.as_ptr().cast(), bytes.len(), ptr::null(), out, error)
    })
    .map(Rule)
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_FORMAT_ERROR);
}

fn open_grid(policy: &str) -> Grid {
    let (p, n) = arg(policy.as_bytes());
    // SAFETY: the name outlives the call; NULL names and limits.
    handle(|out, error| unsafe {
        cr_grid_new_open(
            p,
            n,
            ptr::null(),
            0,
            ptr::null(),
            0,
            false,
            ptr::null(),
            out,
            error,
        )
    })
    .map(Grid)
    .unwrap()
}

fn add_text(grid: &Grid, text: &str) -> Result<(), Failure> {
    let (p, n) = arg(text.as_bytes());
    // SAFETY: a live grid; the text outlives the call.
    ok_or(|e| unsafe { cr_grid_add_column_text(grid.0, p, n, e) })
}

fn finish(grid: Grid) -> Result<Document, Failure> {
    let raw = grid.0;
    std::mem::forget(grid);
    // SAFETY: the grid is given up to the call.
    handle(|out, error| unsafe { cr_grid_finish(raw, out, error) }).map(Document)
}

fn push_nodes(grid: &Grid, row: &str, nodes: &[Option<&Node>]) -> Result<(), Failure> {
    let row = value_from_json(row).unwrap();
    let pointers: Vec<*const cr_node> = nodes
        .iter()
        .map(|n| n.map_or(ptr::null(), |n| n.0.cast_const()))
        .collect();
    // SAFETY: a live grid, a live row and an array of NULL or live nodes.
    ok_or(|e| unsafe {
        cr_grid_push_row_nodes(grid.0, row.0, pointers.as_ptr(), pointers.len(), e)
    })
}

fn push_cells(
    grid: &Grid,
    row: &str,
    kinds: &[cr_cell_kind],
    nodes: Option<&[Option<&Node>]>,
) -> Result<(), Failure> {
    let row = value_from_json(row).unwrap();
    let pointers: Option<Vec<*const cr_node>> = nodes.map(|nodes| {
        nodes
            .iter()
            .map(|n| n.map_or(ptr::null(), |n| n.0.cast_const()))
            .collect()
    });
    let nodes = pointers.as_ref().map_or(ptr::null(), Vec::as_ptr);
    // SAFETY: a live grid and row, `kinds.len()` kinds, and NULL or as many
    // NULL or live nodes.
    ok_or(|e| unsafe {
        cr_grid_push_row_cells(grid.0, row.0, kinds.as_ptr(), nodes, kinds.len(), e)
    })
}

#[test]
fn typed_cells_make_a_string_a_constant_a_name_or_a_description_by_column() {
    let grid = open_grid("first");
    for text in ["RULE", "x", "THEN t", "DESCRIPTION", "PRIORITY"] {
        add_text(&grid, text).unwrap();
    }
    let (t, v, n) = (CR_CELL_TEXT, CR_CELL_VALUE, CR_CELL_NODE);
    // A string value: the name and description's text, the constant under
    // the input and the output.
    push_cells(
        &grid,
        r#"["a", "HIGH", "HIGH", "HIGH", 3]"#,
        &[v, v, v, v, v],
        None,
    )
    .unwrap();
    let low = read_node("x = 'LOW'", CR_FORMAT_TEXT).unwrap();
    push_cells(
        &grid,
        r#"["b", null, "'LOW'", null, "2"]"#,
        &[t, n, t, v, t],
        Some(&[None, Some(&low), None, None, None]),
    )
    .unwrap();
    let table = finish(grid).unwrap();
    assert_eq!(
        write_document(&table, CR_FORMAT_CSV).unwrap(),
        "HIT POLICY,first\nRULE,PRIORITY,x,THEN t,DESCRIPTION\na,3,'HIGH','HIGH',HIGH\nb,2,'LOW','LOW',\n"
    );

    // Under PRIORITY, a string value is not a number.
    let grid = open_grid("first");
    add_text(&grid, "x").unwrap();
    add_text(&grid, "PRIORITY").unwrap();
    let failure = push_cells(&grid, r#"["1", "5"]"#, &[t, v], None)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_FORMAT_ERROR);
    assert!(cleanrules::json::to_string(&failure.json).contains("/rows/0/1"));

    // Misuse: a text cell that is not a string, a node cell without a node,
    // an unknown kind, a length that is not the row's, NULL kinds.
    let grid = open_grid("first");
    add_text(&grid, "x").unwrap();
    for (row, kinds, nodes) in [
        ("[1]", vec![t], None),
        ("[null]", vec![n], None),
        ("[null]", vec![n], Some(&[None][..])),
        ("[null]", vec![9], None),
        ("[null, null]", vec![v], None),
    ] {
        let failure = push_cells(&grid, row, &kinds, nodes).err().unwrap();
        assert_eq!(failure.status, CR_INVALID_ARGUMENT, "{row} {kinds:?}");
    }
    let row = value_from_json("[null]").unwrap();
    // SAFETY: a live grid and row; NULL kinds, which the call refuses.
    let status = unsafe {
        cr_grid_push_row_cells(grid.0, row.0, ptr::null(), ptr::null(), 1, ptr::null_mut())
    };
    assert_eq!(status, CR_INVALID_ARGUMENT);
    // Those misuses did not spoil the grid; an empty row is fine.
    push_cells(&grid, "[]", &[], None).unwrap();
    push_cells(&grid, r#"["> 1"]"#, &[t], None).unwrap();
    finish(grid).unwrap();
}

#[test]
fn an_open_grid_takes_nodes_and_outputs_and_the_table_says_what_it_holds() {
    let grid = open_grid("priority");
    add_text(&grid, "RULE").unwrap();
    let age = read_node("age", CR_FORMAT_TEXT).unwrap();
    // SAFETY: a live grid and node.
    ok(|e| unsafe { cr_grid_add_column_input(grid.0, age.0, e) });
    let values = value_from_json(r#"["high", "low"]"#).unwrap();
    let (p, n) = arg(b"risk");
    // SAFETY: a live grid and list; the name outlives the call.
    ok(|e| unsafe { cr_grid_add_column_output(grid.0, p, n, values.0, e) });
    let (p, n) = arg(b"note");
    // SAFETY: a live grid; NULL values are any value.
    ok(|e| unsafe { cr_grid_add_column_output(grid.0, p, n, ptr::null(), e) });
    let minor = read_node("age < 18", CR_FORMAT_TEXT).unwrap();
    push_nodes(
        &grid,
        r#"["minor", null, "'high'", "'young'"]"#,
        &[None, Some(&minor), None, None],
    )
    .unwrap();
    push_nodes(&grid, r#"["adult", "-", "'low'", null]"#, &[None; 4]).unwrap();
    // After a row, the header is closed.
    let failure = add_text(&grid, "x").err().unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    // One node per cell.
    let failure = push_nodes(&grid, r#"["x"]"#, &[]).err().unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let table = finish(grid).unwrap();
    assert_eq!(kind(&table), CR_DOCUMENT_DECISION_TABLE);
    assert_eq!(
        write_document(&table, CR_FORMAT_CSV).unwrap(),
        "HIT POLICY,priority\nRULE,age,\"THEN risk IN ('high', 'low')\",THEN note\nminor,< 18,'high','young'\nadult,-,'low',\n"
    );

    // SAFETY: a live document.
    let settings = handle(|out, error| unsafe { cr_document_table_value(table.0, out, error) })
        .map(Value)
        .unwrap();
    assert!(same(
        &json(&value_to_json(settings.0)),
        &json(
            r#"{"hit_policy":"priority","aggregate":null,"logic":"boolean","strict":false,"input_count":1,"outputs":[{"name":"risk","values":["high","low"]},{"name":"note","values":[]}],"row_count":2}"#
        )
    ));
    // SAFETY: a live document.
    let row = handle(|out, error| unsafe { cr_document_table_row(table.0, 0, out, error) })
        .map(Value)
        .unwrap();
    assert_eq!(
        value_to_json(row.0),
        r#"{"name":"minor","priority":null,"description":null}"#
    );
    // SAFETY: a live document.
    let input = handle(|out, error| unsafe { cr_document_table_input(table.0, 0, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(node_text(&input), "age");
    let cell = |r: usize, c: usize| -> Result<Option<String>, Failure> {
        let mut out = ptr::null_mut();
        // SAFETY: a live document and a writable out-parameter.
        ok_or(|e| unsafe { cr_document_table_cell(table.0, r, c, &raw mut out, e) })?;
        Ok((!out.is_null()).then(|| node_text(&Node(out))))
    };
    assert_eq!(cell(0, 0).unwrap().as_deref(), Some("age < 18"));
    assert_eq!(cell(0, 1).unwrap().as_deref(), Some("\"high\""));
    assert_eq!(cell(1, 0).unwrap(), None);
    assert_eq!(cell(1, 2).unwrap(), None);
    for (r, c) in [(2, 0), (0, 3)] {
        assert_eq!(cell(r, c).err().unwrap().status, CR_INVALID_ARGUMENT);
    }
    // A document that is not a table.
    let other = read_document(b"x > 1", CR_FORMAT_TEXT, None).unwrap();
    // SAFETY: a live document.
    let failure = handle(|out, error| unsafe { cr_document_table_value(other.0, out, error) })
        .map(Value)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
}

#[test]
fn an_open_header_is_checked_at_the_first_row_with_pointers() {
    let grid = open_grid("first");
    add_text(&grid, "RULE").unwrap();
    add_text(&grid, "age >").unwrap();
    let failure = push_nodes(&grid, r#"["a", "1"]"#, &[None, None])
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_RULE_SYNTAX_ERROR);
    assert!(cleanrules::json::to_string(&failure.json).contains("/header/1"));
    // The grid is spoiled.
    let failure = push_nodes(&grid, r#"["a", "1"]"#, &[None, None])
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    assert_eq!(finish(grid).err().unwrap().status, CR_INVALID_ARGUMENT);

    // A header with no rows is read when the grid is finished.
    let grid = open_grid("first");
    let condition = read_node("age > 1", CR_FORMAT_TEXT).unwrap();
    // SAFETY: a live grid and node.
    ok(|e| unsafe { cr_grid_add_column_input(grid.0, condition.0, e) });
    let failure = finish(grid).err().unwrap();
    assert_eq!(failure.status, CR_EXPRESSION_ERROR);
    assert!(cleanrules::json::to_string(&failure.json).contains("/header/0"));

    // A row error points into the grid.
    let grid = open_grid("first");
    add_text(&grid, "age").unwrap();
    let bad = read_node("x > 1", CR_FORMAT_TEXT).unwrap();
    push_nodes(&grid, "[null]", &[Some(&bad)]).unwrap();
    let failure = finish(grid).err().unwrap();
    assert_eq!(failure.status, CR_UNSUPPORTED);
    assert!(
        cleanrules::json::to_string(&failure.json).contains("/rows/0/0"),
        "{failure:?}"
    );
}

fn tree_json(tree: &Tree) -> String {
    // SAFETY: a live tree.
    let value = handle(|out, error| unsafe { cr_tree_value(tree.0, out, error) })
        .map(Value)
        .unwrap();
    value_to_json(value.0)
}

fn tree_node(tree: &Tree, index: usize) -> Result<Node, Failure> {
    // SAFETY: a live tree.
    handle(|out, error| unsafe { cr_tree_node(tree.0, index, out, error) }).map(Node)
}

#[test]
fn result_trees_are_handles_with_their_value_and_nodes() {
    let node = read_node("age >= 18 AND country IN (\"ES\")", CR_FORMAT_TEXT).unwrap();
    let input = value_from_json(r#"{"age": 20}"#).unwrap();
    // SAFETY: live handles; NULL logic is boolean.
    let tree = handle(|out, error| unsafe {
        cr_tree_new_condition(node.0, input.0, ptr::null(), 0, false, out, error)
    })
    .map(Tree)
    .unwrap();
    // The value writes as the JSON result tree.
    let wanted = common::evaluate_node(&node, &input, None, false, false).unwrap();
    assert_eq!(tree_json(&tree), wanted);
    // SAFETY: a live tree.
    let len = unsafe { cr_tree_len(tree.0) };
    assert_eq!(len, 6);
    let texts: Vec<String> = (0..len)
        .map(|i| node_text(&tree_node(&tree, i).unwrap()))
        .collect();
    assert_eq!(
        texts,
        [
            "age >= 18 AND country IN (\"ES\")",
            "age >= 18",
            "age",
            "18",
            "country IN (\"ES\")",
            "country",
        ]
    );
    assert_eq!(
        tree_node(&tree, len).err().unwrap().status,
        CR_INVALID_ARGUMENT
    );
    // SAFETY: NULL is 0.
    assert_eq!(unsafe { cr_tree_len(ptr::null()) }, 0);

    // A value tree; a condition is refused, as is a value as a condition.
    let price = read_node("price * 2", CR_FORMAT_TEXT).unwrap();
    let input = value_from_json(r#"{"price": 2.5}"#).unwrap();
    // SAFETY: live handles.
    let tree =
        handle(|out, error| unsafe { cr_tree_new_value(price.0, input.0, false, out, error) })
            .map(Tree)
            .unwrap();
    assert_eq!(
        tree_json(&tree),
        common::evaluate_node(&price, &input, None, false, true).unwrap()
    );
    // SAFETY: live handles.
    let failure =
        handle(|out, error| unsafe { cr_tree_new_value(node.0, input.0, false, out, error) })
            .map(Tree)
            .err()
            .unwrap();
    assert_eq!(failure.status, CR_EXPRESSION_ERROR);
    let (p, n) = arg(b"nope");
    // SAFETY: live handles; the name outlives the call.
    let failure = handle(|out, error| unsafe {
        cr_tree_new_condition(node.0, input.0, p, n, false, out, error)
    })
    .map(Tree)
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_REGISTRY_ERROR);
    // Strict: a missing field is an error.
    let empty = value_from_json("{}").unwrap();
    // SAFETY: live handles.
    let failure = handle(|out, error| unsafe {
        cr_tree_new_condition(node.0, empty.0, ptr::null(), 0, true, out, error)
    })
    .map(Tree)
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_MISSING_FIELD);
}

#[test]
fn a_decision_gives_each_rules_tree() {
    let ruleset = read_ruleset(RULES.as_bytes(), CR_FORMAT_TEXT).unwrap();
    let decision = evaluate(
        &ruleset,
        br#"{"customer": {"years": 7}, "order": {"total": 900}}"#,
    )
    .unwrap();
    // SAFETY: a live decision.
    let tree = handle(|out, error| unsafe { cr_decision_rule_tree(decision.0, 0, out, error) })
        .map(Tree)
        .unwrap();
    let rule = rule_at(&ruleset, 0).unwrap();
    // SAFETY: a live rule.
    let when = handle(|out, error| unsafe { cr_rule_when(rule.0, out, error) })
        .map(Node)
        .unwrap();
    let input = value_from_json(r#"{"customer": {"years": 7}, "order": {"total": 900}}"#).unwrap();
    assert_eq!(
        tree_json(&tree),
        common::evaluate_node(&when, &input, None, false, false).unwrap()
    );
    // SAFETY: a live decision.
    let failure = handle(|out, error| unsafe { cr_decision_rule_tree(decision.0, 2, out, error) })
        .map(Tree)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    // A table compiled from a document gives its rule set too.
    let table = read_document(
        b"HIT POLICY,first\nRULE,age,THEN band\nminor,< 18,'minor'\n",
        CR_FORMAT_CSV,
        None,
    )
    .unwrap();
    let compiled = ruleset_of(&table).unwrap();
    assert_eq!(name(&rule_at(&compiled, 0).unwrap()), "minor");
}

#[test]
fn opaque_values_keep_their_token_in_a_tree() {
    let node = read_node("x", CR_FORMAT_TEXT).unwrap();
    let (p, n) = arg(b"Thing");
    let mut opaque = ptr::null_mut();
    // SAFETY: the name outlives the call; a writable out-parameter.
    ok(|e| unsafe { cr_value_new_opaque(p, n, 7, &raw mut opaque, e) });
    let input = Value(cr_value_new_map());
    let (k, kn) = arg(b"x");
    // SAFETY: a live map that takes the opaque value.
    ok(|e| unsafe { cr_value_map_insert(input.0, k, kn, opaque, e) });
    // SAFETY: live handles.
    let tree =
        handle(|out, error| unsafe { cr_tree_new_value(node.0, input.0, false, out, error) })
            .map(Tree)
            .unwrap();
    // SAFETY: a live tree.
    let value = handle(|out, error| unsafe { cr_tree_value(tree.0, out, error) })
        .map(Value)
        .unwrap();
    let (mut key, mut len, mut item) = (ptr::null(), 0, ptr::null());
    let mut found = None;
    // SAFETY: a live map; the entry is borrowed while it lives.
    unsafe {
        for i in 0..cr_value_len(value.0) {
            ok(|e| cr_value_map_entry(value.0, i, &raw mut key, &raw mut len, &raw mut item, e));
            if std::slice::from_raw_parts(key.cast::<u8>(), len) == b"value" {
                let mut token = 0;
                ok(|e| cr_value_get_opaque_token(item, &raw mut token, e));
                found = Some(token);
            }
        }
    }
    assert_eq!(found, Some(7));
}
