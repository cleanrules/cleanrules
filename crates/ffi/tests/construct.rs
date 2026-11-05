//! Conditions, rules and rule sets built through the C ABI's constructors,
//! with the core's checks and errors; what a node says about itself; the
//! lookup of a rule by name, the summary with actions and the explanation as
//! a value.

// A test: a panic is the failure report.
#![allow(clippy::unwrap_used, clippy::panic, clippy::many_single_char_names)]

mod common;

use std::ffi::{CStr, c_char};
use std::ptr;

use cleanrules::json::{Json, same};
use cleanrules_ffi::*;
use common::{
    Failure, Node, RuleSet, Value, arg, handle, json, text, value_from_json, value_to_json,
};

/// An owned rule, freed on drop.
struct Rule(*mut cr_rule);

impl Drop for Rule {
    fn drop(&mut self) {
        // SAFETY: the rule came from the library and is freed once.
        unsafe { cr_rule_free(self.0) };
    }
}

fn c_str(text: *const c_char) -> Option<&'static str> {
    // SAFETY: the library's static strings.
    (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_str().unwrap())
}

fn value(text: &str) -> Value {
    value_from_json(text).unwrap()
}

fn field(path: &str) -> Node {
    let path = value(path);
    // SAFETY: a live value.
    handle(|out, error| unsafe { cr_node_new_field(path.0, out, error) })
        .map(Node)
        .unwrap()
}

fn constant(json: &str) -> Result<Node, Failure> {
    let value = value(json);
    // SAFETY: a live value.
    handle(|out, error| unsafe { cr_node_new_constant(value.0, out, error) }).map(Node)
}

fn compare(op: &str, left: &Node, right: &Node) -> Result<Node, Failure> {
    let (p, n) = arg(op.as_bytes());
    // SAFETY: live nodes; the name outlives the call.
    handle(|out, error| unsafe { cr_node_new_compare(p, n, left.0, right.0, out, error) }).map(Node)
}

fn and(nodes: &[&Node]) -> Result<Node, Failure> {
    let pointers: Vec<*const cr_node> = nodes.iter().map(|n| n.0.cast_const()).collect();
    // SAFETY: an array of live nodes.
    handle(|out, error| unsafe { cr_node_new_and(pointers.as_ptr(), pointers.len(), out, error) })
        .map(Node)
}

fn write(node: &Node, format: i32) -> String {
    // SAFETY: a live node.
    text(|out, len, error| unsafe { cr_node_write(node.0, format, out, len, error) }).unwrap()
}

fn json_of(node: &Node) -> Json {
    json(&write(node, CR_FORMAT_JSON))
}

fn detail(node: &Node) -> String {
    // SAFETY: a live node.
    let detail = handle(|out, error| unsafe { cr_node_detail(node.0, out, error) })
        .map(Value)
        .unwrap();
    value_to_json(detail.0)
}

fn tag(node: &Node) -> &'static str {
    // SAFETY: a live node.
    c_str(unsafe { cr_node_tag(node.0) }).unwrap()
}

fn expression_error(failure: &Failure) {
    assert_eq!(failure.status, CR_EXPRESSION_ERROR, "{failure:?}");
    assert_eq!(failure.code(), "expression_error");
}

fn message(failure: &Failure) -> String {
    match &failure.json {
        Json::Object(members) => members
            .iter()
            .find_map(|(k, v)| match (k.as_str(), v) {
                ("message", Json::String(m)) => Some(m.clone()),
                _ => None,
            })
            .unwrap(),
        _ => panic!("an error that is not an object"),
    }
}

fn member<'a>(failure: &'a Failure, key: &str) -> Option<&'a Json> {
    match &failure.json {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

#[test]
fn fields_take_keys_and_indexes() {
    let node = field(r#"["items", 0, "a.b"]"#);
    assert_eq!(write(&node, CR_FORMAT_TEXT), "items[0].`a.b`");
    assert_eq!(detail(&node), r#"["items",0,"a.b"]"#);
    for (path, wanted) in [
        ("[]", "a field path cannot be empty"),
        (r#"[""]"#, "a field path segment cannot be an empty string"),
        (
            "[-1]",
            "a field path segment cannot be the negative index -1",
        ),
        ("[true]", "a field path segment cannot be a boolean"),
        ("[1.5]", "a field path segment cannot be a float"),
    ] {
        let path = value(path);
        // SAFETY: a live value.
        let failure = handle(|out, error| unsafe { cr_node_new_field(path.0, out, error) })
            .map(Node)
            .err()
            .unwrap();
        expression_error(&failure);
        assert_eq!(message(&failure), wanted);
    }
    let scalar = value("1");
    // SAFETY: a live value.
    let failure = handle(|out, error| unsafe { cr_node_new_field(scalar.0, out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
}

#[test]
fn constants_follow_the_constant_rules() {
    let decimal = constant(r#"{"$decimal": "0.10"}"#).unwrap();
    assert_eq!(write(&decimal, CR_FORMAT_TEXT), "0.10");
    assert_eq!(detail(&decimal), r#"{"$decimal":"0.10"}"#);
    let null = constant("null").unwrap();
    assert_eq!(tag(&null), "const");
    for list in ["[1]", r#"{"a": 1}"#] {
        let failure = constant(list).err().unwrap();
        expression_error(&failure);
        assert_eq!(message(&failure), "a constant must be a scalar");
    }
    for (reason, wanted) in [
        (CR_REASON_INT_OVERFLOW, "does not fit in 64 bits"),
        (CR_REASON_DECIMAL_OVERFLOW, "does not fit in 38 digits"),
        (CR_REASON_READ_ERROR, "could not be read: boom"),
    ] {
        let (p, n) = arg(b"boom");
        // SAFETY: the detail outlives the call.
        let null =
            handle(|out, error| unsafe { cr_value_new_null_reason(reason, p, n, out, error) })
                .map(Value)
                .unwrap();
        // SAFETY: a live value.
        let failure = handle(|out, error| unsafe { cr_node_new_constant(null.0, out, error) })
            .map(Node)
            .err()
            .unwrap();
        expression_error(&failure);
        assert!(message(&failure).contains(wanted), "{}", message(&failure));
    }
    let (p, n) = arg(b"Thread");
    // SAFETY: the type name outlives the call.
    let opaque = handle(|out, error| unsafe { cr_value_new_opaque(p, n, 7, out, error) })
        .map(Value)
        .unwrap();
    // SAFETY: a live value.
    let failure = handle(|out, error| unsafe { cr_node_new_constant(opaque.0, out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(message(&failure), "a Thread cannot be a constant");
}

#[test]
fn every_operator_builds_its_node() {
    let x = field(r#"["x"]"#);
    let name = field(r#"["name"]"#);
    let one = constant("1").unwrap();
    let two = constant("2").unwrap();
    let gt = compare("gt", &x, &one).unwrap();
    assert_eq!(write(&gt, CR_FORMAT_TEXT), "x > 1");
    let le = compare("le", &x, &two).unwrap();
    let both = and(&[&gt, &le]).unwrap();
    assert_eq!(write(&both, CR_FORMAT_TEXT), "x > 1 AND x <= 2");
    let pointers = [gt.0.cast_const(), both.0.cast_const()];
    // SAFETY: an array of live nodes.
    let either = handle(|out, error| unsafe { cr_node_new_or(pointers.as_ptr(), 2, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(write(&either, CR_FORMAT_TEXT), "x > 1 OR x > 1 AND x <= 2");
    // SAFETY: a live node.
    let not = handle(|out, error| unsafe { cr_node_new_not(gt.0, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(write(&not, CR_FORMAT_TEXT), "NOT x > 1");
    let values = value(r#"["PT", "ES", "PT"]"#);
    // SAFETY: live handles.
    let is_in = handle(|out, error| unsafe { cr_node_new_is_in(name.0, values.0, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(write(&is_in, CR_FORMAT_TEXT), "name IN (\"ES\", \"PT\")");
    assert_eq!(detail(&is_in), r#"["ES","PT"]"#);
    let (p, n) = arg(b"left");
    // SAFETY: live nodes; the name outlives the call.
    let between =
        handle(|out, error| unsafe { cr_node_new_between(x.0, one.0, two.0, p, n, out, error) })
            .map(Node)
            .unwrap();
    assert_eq!(detail(&between), r#""left""#);
    // SAFETY: live nodes; `NULL` is `both`.
    let closed = handle(|out, error| unsafe {
        cr_node_new_between(x.0, one.0, two.0, ptr::null(), 0, out, error)
    })
    .map(Node)
    .unwrap();
    assert_eq!(write(&closed, CR_FORMAT_TEXT), "x BETWEEN 1 AND 2");
    // SAFETY: a live node.
    let is_null = handle(|out, error| unsafe { cr_node_new_is_null(x.0, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(write(&is_null, CR_FORMAT_TEXT), "x IS NULL");
    let (p, n) = arg(b"^a+$");
    // SAFETY: a live node; the pattern outlives the call.
    let matches = handle(|out, error| unsafe { cr_node_new_matches(name.0, p, n, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(detail(&matches), r#""^a+$""#);
    let (op, op_len) = arg(b"starts_with");
    let (t, t_len) = arg(b"Jo");
    // SAFETY: a live node; the texts outlive the call.
    let starts = handle(|out, error| unsafe {
        cr_node_new_string_test(op, op_len, name.0, t, t_len, out, error)
    })
    .map(Node)
    .unwrap();
    assert_eq!(write(&starts, CR_FORMAT_TEXT), "name STARTS WITH \"Jo\"");
    let (op, op_len) = arg(b"mul");
    // SAFETY: live nodes; the name outlives the call.
    let product =
        handle(|out, error| unsafe { cr_node_new_arith(op, op_len, x.0, two.0, out, error) })
            .map(Node)
            .unwrap();
    // SAFETY: a live node.
    let abs = handle(|out, error| unsafe { cr_node_new_abs(product.0, out, error) })
        .map(Node)
        .unwrap();
    // SAFETY: a live node.
    let neg = handle(|out, error| unsafe { cr_node_new_neg(abs.0, out, error) })
        .map(Node)
        .unwrap();
    assert_eq!(write(&neg, CR_FORMAT_TEXT), "-ABS(x * 2)");
    for (node, wanted) in [
        (&gt, "gt"),
        (&both, "and"),
        (&either, "or"),
        (&not, "not"),
        (&is_in, "in"),
        (&between, "between"),
        (&is_null, "is_null"),
        (&matches, "matches"),
        (&starts, "starts_with"),
        (&product, "mul"),
        (&abs, "abs"),
        (&neg, "neg"),
    ] {
        assert_eq!(tag(node), wanted);
        assert_ne!(detail(node), "");
    }
}

#[test]
fn constructors_report_the_core_errors() {
    let x = field(r#"["x"]"#);
    let one = constant("1").unwrap();
    let null = constant("null").unwrap();
    let text = constant(r#""a""#).unwrap();
    let gt = compare("gt", &x, &one).unwrap();
    // A condition compared, the null constant, one operand, a bad pattern.
    expression_error(&compare("eq", &gt, &one).err().unwrap());
    expression_error(&compare("eq", &x, &null).err().unwrap());
    expression_error(&and(&[&gt]).err().unwrap());
    let (p, n) = arg(b"(?=a)");
    // SAFETY: a live node; the pattern outlives the call.
    let failure = handle(|out, error| unsafe { cr_node_new_matches(x.0, p, n, out, error) })
        .map(Node)
        .err()
        .unwrap();
    expression_error(&failure);
    // SAFETY: a live node.
    let failure = handle(|out, error| unsafe { cr_node_new_neg(text.0, out, error) })
        .map(Node)
        .err()
        .unwrap();
    expression_error(&failure);
    let mixed = value(r#"[1, "a"]"#);
    // SAFETY: live handles.
    let failure = handle(|out, error| unsafe { cr_node_new_is_in(x.0, mixed.0, out, error) })
        .map(Node)
        .err()
        .unwrap();
    expression_error(&failure);
    let (op, op_len) = arg(b"near");
    // SAFETY: live nodes; the name outlives the call.
    let failure =
        handle(|out, error| unsafe { cr_node_new_compare(op, op_len, x.0, one.0, out, error) })
            .map(Node)
            .err()
            .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    assert!(message(&failure).starts_with("cr_node_new_compare: unknown comparison \"near\""));
    for (call, op) in [("arith", "pow"), ("string", "has"), ("closed", "half")] {
        let (p, n) = arg(op.as_bytes());
        // SAFETY: live nodes; the names outlive the calls.
        let failure = handle(|out, error| unsafe {
            match call {
                "arith" => cr_node_new_arith(p, n, x.0, one.0, out, error),
                "string" => cr_node_new_string_test(p, n, x.0, p, n, out, error),
                _ => cr_node_new_between(x.0, one.0, one.0, p, n, out, error),
            }
        })
        .map(Node)
        .err()
        .unwrap();
        assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    }
    // SAFETY: NULL operands.
    let failure = handle(|out, error| unsafe { cr_node_new_and(ptr::null(), 2, out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let pointers = [x.0.cast_const(), ptr::null()];
    // SAFETY: an array of a live node and a NULL.
    let failure = handle(|out, error| unsafe { cr_node_new_and(pointers.as_ptr(), 2, out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    // SAFETY: NULL operands and none: fewer than two.
    let failure = handle(|out, error| unsafe { cr_node_new_or(ptr::null(), 0, out, error) })
        .map(Node)
        .err()
        .unwrap();
    expression_error(&failure);
}

#[test]
fn nodes_describe_themselves() {
    let x = field(r#"["x"]"#);
    let one = constant("1").unwrap();
    let float = constant("1.0").unwrap();
    let zero = constant("0.0").unwrap();
    let negative_zero = constant("-0.0").unwrap();
    let truth = constant("true").unwrap();
    let gt = compare("gt", &x, &one).unwrap();
    let again = compare("gt", &x, &one).unwrap();
    // SAFETY: live nodes, or NULL.
    unsafe {
        assert_eq!(c_str(cr_node_kind(gt.0)), Some("condition"));
        assert_eq!(c_str(cr_node_kind(one.0)), Some("value"));
        assert_eq!(c_str(cr_node_kind(x.0)), Some("any"));
        assert_eq!(c_str(cr_node_kind(truth.0)), Some("any"));
        assert_eq!(cr_node_kind(ptr::null()), ptr::null());
        assert_eq!(cr_node_tag(ptr::null()), ptr::null());
        assert_eq!(cr_node_children_len(gt.0), 2);
        assert_eq!(cr_node_children_len(x.0), 0);
        assert_eq!(cr_node_children_len(ptr::null()), 0);
        assert!(cr_node_same(gt.0, again.0));
        assert_eq!(cr_node_hash(gt.0), cr_node_hash(again.0));
        assert!(!cr_node_same(one.0, float.0));
        assert!(cr_node_same(zero.0, negative_zero.0));
        assert_eq!(cr_node_hash(zero.0), cr_node_hash(negative_zero.0));
        assert!(!cr_node_same(gt.0, ptr::null()));
        assert_eq!(cr_node_hash(ptr::null()), 0);
    }
    // SAFETY: a live node.
    let right = handle(|out, error| unsafe { cr_node_child(gt.0, 1, out, error) })
        .map(Node)
        .unwrap();
    // SAFETY: live nodes.
    assert!(unsafe { cr_node_same(right.0, one.0) });
    // SAFETY: a live node.
    let failure = handle(|out, error| unsafe { cr_node_child(gt.0, 2, out, error) })
        .map(Node)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    assert_eq!(detail(&gt), "null");
    assert!(same(
        &json_of(&gt),
        &json(r#"{"op":"gt","left":{"op":"field","path":["x"]},"right":{"op":"const","value":1}}"#)
    ));
}

fn rule(name: &str, when: &Node) -> Result<Rule, Failure> {
    let (p, n) = arg(name.as_bytes());
    // SAFETY: a live node; the name outlives the call.
    handle(|out, error| unsafe { cr_rule_new(p, n, when.0, out, error) }).map(Rule)
}

fn with_payload(rule: &Rule, key: &str, value: &Node) -> Result<Rule, Failure> {
    let (p, n) = arg(key.as_bytes());
    // SAFETY: live handles; the key outlives the call.
    handle(|out, error| unsafe { cr_rule_with_payload(rule.0, p, n, value.0, out, error) })
        .map(Rule)
}

fn ruleset(
    rules: &[&Rule],
    policy: Option<&str>,
    aggregate: Option<&str>,
) -> Result<RuleSet, Failure> {
    let pointers: Vec<*const cr_rule> = rules.iter().map(|r| r.0.cast_const()).collect();
    let (pp, pn) = policy.map_or((ptr::null(), 0), |p| arg(p.as_bytes()));
    let (ap, an) = aggregate.map_or((ptr::null(), 0), |a| arg(a.as_bytes()));
    // SAFETY: an array of live rules; the names outlive the call.
    handle(|out, error| unsafe {
        cr_ruleset_new(
            pointers.as_ptr(),
            pointers.len(),
            pp,
            pn,
            ap,
            an,
            ptr::null(),
            0,
            false,
            out,
            error,
        )
    })
    .map(RuleSet)
}

#[test]
fn rules_are_built_step_by_step() {
    let years = field(r#"["years"]"#);
    let five = constant("5").unwrap();
    let when = compare("gt", &years, &five).unwrap();
    let discount = constant(r#"{"$decimal": "0.15"}"#).unwrap();
    let vip = rule("vip", &when).unwrap();
    let vip = with_payload(&vip, "discount", &discount).unwrap();
    let vip = with_payload(&vip, "years", &years).unwrap();
    let vip = with_payload(&vip, "discount", &five).unwrap();
    // SAFETY: a live rule.
    let vip = handle(|out, error| unsafe { cr_rule_with_priority(vip.0, 10, out, error) })
        .map(Rule)
        .unwrap();
    let tags = value(r#"["b", "a", "b"]"#);
    // SAFETY: live handles.
    let vip = handle(|out, error| unsafe { cr_rule_with_tags(vip.0, tags.0, out, error) })
        .map(Rule)
        .unwrap();
    let (p, n) = arg(b"Loyal customers");
    // SAFETY: a live rule; the text outlives the call.
    let vip = handle(|out, error| unsafe { cr_rule_with_description(vip.0, p, n, out, error) })
        .map(Rule)
        .unwrap();
    let metadata = value(r#"{"owner": "sales", "ticket": [1, {"$date": "2026-11-05"}]}"#);
    // SAFETY: live handles.
    let vip = handle(|out, error| unsafe { cr_rule_with_metadata(vip.0, metadata.0, out, error) })
        .map(Rule)
        .unwrap();
    let (p, n) = arg(b"notify");
    // SAFETY: a live rule; the name outlives the call.
    let vip = handle(|out, error| unsafe { cr_rule_with_action(vip.0, p, n, out, error) })
        .map(Rule)
        .unwrap();
    // SAFETY: a live rule.
    let tags = handle(|out, error| unsafe { cr_rule_tags(vip.0, out, error) })
        .map(Value)
        .unwrap();
    assert_eq!(value_to_json(tags.0), r#"["a","b"]"#);
    // SAFETY: a live rule.
    let metadata = handle(|out, error| unsafe { cr_rule_metadata(vip.0, out, error) })
        .map(Value)
        .unwrap();
    assert_eq!(
        value_to_json(metadata.0),
        r#"{"owner":"sales","ticket":[1,{"$date":"2026-11-05"}]}"#
    );
    let set = ruleset(&[&vip], Some("priority"), None).unwrap();
    let written = common::write_ruleset(&set, CR_FORMAT_JSON).unwrap();
    let wanted = r#"{"version":1,"ruleset":{"hit_policy":"priority","logic":"boolean","rules":[
        {"name":"vip",
         "when":{"op":"gt","left":{"op":"field","path":["years"]},"right":{"op":"const","value":5}},
         "then":{"discount":{"op":"const","value":5},"years":{"op":"field","path":["years"]}},
         "priority":10,"tags":["a","b"],"description":"Loyal customers",
         "metadata":{"owner":"sales","ticket":[1,{"$date":"2026-11-05"}]},"do":"notify"}]}}"#;
    assert!(same(&json(&written), &json(wanted)), "{written}");
}

#[test]
fn rules_report_the_core_errors() {
    let x = field(r#"["x"]"#);
    let one = constant("1").unwrap();
    let null = constant("null").unwrap();
    let gt = compare("gt", &x, &one).unwrap();
    let failure = rule("", &gt).err().unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    let failure = rule("r", &one).err().unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    assert_eq!(member(&failure, "rule"), Some(&Json::String("r".into())));
    expression_error(&rule("r", &null).err().unwrap());
    let r = rule("r", &gt).unwrap();
    let failure = with_payload(&r, "ok", &gt).err().unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    let (p, n) = arg(b"");
    // SAFETY: a live rule; an empty name.
    let failure = handle(|out, error| unsafe { cr_rule_with_action(r.0, p, n, out, error) })
        .map(Rule)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    let tags = value(r#"["a", 1]"#);
    // SAFETY: live handles.
    let failure = handle(|out, error| unsafe { cr_rule_with_tags(r.0, tags.0, out, error) })
        .map(Rule)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    assert_eq!(message(&failure), "a tag is a string, not an int");
    let (p, n) = arg(b"Thread");
    // SAFETY: the type name outlives the call.
    let opaque = handle(|out, error| unsafe { cr_value_new_opaque(p, n, 1, out, error) })
        .map(Value)
        .unwrap();
    let list = Value(cr_value_new_list(1));
    let map = Value(cr_value_new_map());
    // SAFETY: live values; each is moved into its parent once.
    unsafe {
        assert_eq!(
            cr_value_list_set(list.0, 0, opaque.0, ptr::null_mut()),
            CR_OK
        );
        std::mem::forget(opaque);
        let (k, kn) = arg(b"who");
        assert_eq!(
            cr_value_map_insert(map.0, k, kn, list.0, ptr::null_mut()),
            CR_OK
        );
        std::mem::forget(list);
    }
    // SAFETY: live handles.
    let failure = handle(|out, error| unsafe { cr_rule_with_metadata(r.0, map.0, out, error) })
        .map(Rule)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    assert_eq!(
        message(&failure),
        "metadata[\"who\"][0] is not a data value: a Thread"
    );
    let scalar = value("1");
    // SAFETY: live handles.
    let failure = handle(|out, error| unsafe { cr_rule_with_metadata(r.0, scalar.0, out, error) })
        .map(Rule)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    // SAFETY: live handles.
    let failure = handle(|out, error| unsafe { cr_rule_with_tags(r.0, scalar.0, out, error) })
        .map(Rule)
        .err()
        .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
}

#[test]
fn rule_sets_are_built_and_looked_up() {
    let x = field(r#"["x"]"#);
    let one = constant("1").unwrap();
    let gt = compare("gt", &x, &one).unwrap();
    let a = with_payload(&rule("a", &gt).unwrap(), "n", &one).unwrap();
    let b = with_payload(&rule("b", &gt).unwrap(), "n", &one).unwrap();
    let failure = ruleset(&[&a, &a], None, None).err().unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    let failure = ruleset(&[&a], None, Some("sum")).err().unwrap();
    assert_eq!(failure.status, CR_RULE_DEFINITION_ERROR);
    let failure = ruleset(&[&a], Some("best"), None).err().unwrap();
    assert_eq!(message(&failure), "unknown hit policy \"best\"");
    let failure = ruleset(&[&a], Some("collect"), Some("avg")).err().unwrap();
    assert_eq!(message(&failure), "unknown aggregate \"avg\"");
    let (l, ln) = arg(b"fuzzy");
    // SAFETY: an empty rule list; the name outlives the call.
    let failure = handle(|out, error| unsafe {
        cr_ruleset_new(
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            l,
            ln,
            true,
            out,
            error,
        )
    })
    .map(RuleSet)
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_REGISTRY_ERROR);
    // SAFETY: NULL rules with a length.
    let failure = handle(|out, error| unsafe {
        cr_ruleset_new(
            ptr::null(),
            1,
            ptr::null(),
            0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            false,
            out,
            error,
        )
    })
    .map(RuleSet)
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    let set = ruleset(&[&a, &b], Some("collect"), Some("sum")).unwrap();
    // SAFETY: a live rule set.
    unsafe {
        assert_eq!(c_str(cr_ruleset_aggregate(set.0)), Some("sum"));
        assert!(!cr_ruleset_strict(set.0));
    }
    let mut position = usize::MAX;
    let (p, n) = arg(b"b");
    // SAFETY: a live rule set; the name outlives the call.
    let status =
        unsafe { cr_ruleset_rule_position(set.0, p, n, &raw mut position, ptr::null_mut()) };
    assert_eq!((status, position), (CR_OK, 1));
    let (p, n) = arg(b"zz");
    let failure = common::ok_or(|error| {
        // SAFETY: a live rule set; the name outlives the call.
        unsafe { cr_ruleset_rule_position(set.0, p, n, &raw mut position, error) }
    })
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_RULE_NOT_FOUND);
    assert_eq!(message(&failure), "no rule named \"zz\"");
    assert_eq!(member(&failure, "available"), Some(&json(r#"["a", "b"]"#)));
}

#[test]
fn summaries_mark_the_rules_with_code_actions() {
    let x = field(r#"["x"]"#);
    let one = constant("1").unwrap();
    let gt = compare("gt", &x, &one).unwrap();
    let a = rule("a", &gt).unwrap();
    let (p, n) = arg(b"log");
    let plain = rule("b", &gt).unwrap();
    // SAFETY: a live rule; the name outlives the call.
    let b = handle(|out, error| unsafe { cr_rule_with_action(plain.0, p, n, out, error) })
        .map(Rule)
        .unwrap();
    let set = ruleset(&[&a, &b], None, None).unwrap();
    let flags = [true, false];
    // SAFETY: a live rule set and one flag per rule.
    let summary = text(|out, len, error| unsafe {
        cr_ruleset_summary_actions(set.0, flags.as_ptr(), 2, CR_FORMAT_JSON, out, len, error)
    })
    .unwrap();
    for (pointer, count) in [(flags.as_ptr(), 1), (ptr::null(), 2)] {
        // SAFETY: a live rule set; one flag of two, or NULL with two.
        let failure = text(|out, len, error| unsafe {
            cr_ruleset_summary_actions(set.0, pointer, count, CR_FORMAT_JSON, out, len, error)
        })
        .err()
        .unwrap();
        assert_eq!(failure.status, CR_INVALID_ARGUMENT);
    }
    let wanted = r#"{"version":1,"summary":{"hit_policy":"first","logic":"boolean",
        "rules":[{"name":"a","priority":0,"outputs":[],"action":true},
                 {"name":"b","priority":0,"outputs":[],"action":true,"do":"log"}],
        "fields":[{"path":["x"],"conditions":["a","b"],"payloads":[]}],"outputs":[],"tags":[]}}"#;
    assert!(same(&json(&summary), &json(wanted)), "{summary}");
    // SAFETY: a live rule set and no flags.
    let plain = text(|out, len, error| unsafe {
        cr_ruleset_summary_actions(set.0, ptr::null(), 0, CR_FORMAT_TEXT, out, len, error)
    })
    .unwrap();
    assert_eq!(plain, common::summary(&set, CR_FORMAT_TEXT).unwrap());
    // A rule that names its action is marked without a flag.
    assert!(plain.contains("log"), "{plain}");
}

#[test]
fn explanations_come_as_values() {
    let rules = r#"{"version":1,"ruleset":{"hit_policy":"priority","rules":[
        {"name":"big","priority":1,"when":{"op":"and","operands":[
            {"op":"gt","left":{"op":"field","path":["x"]},"right":{"op":"const","value":10}},
            {"op":"field","path":["ok"]}]}},
        {"name":"any","priority":2,"when":{"op":"const","value":true},"then":{"n":{"op":"const","value":1}}},
        {"name":"low","priority":0,"when":{"op":"const","value":true}},
        {"name":"date","when":{"op":"lt","left":{"op":"field","path":["d"]},"right":{"op":"const","value":{"$date":"2026-01-01"}}}}]}}"#;
    let set = common::read_ruleset(rules.as_bytes(), CR_FORMAT_JSON).unwrap();
    let input = value(r#"{"x": 8, "d": {"$date": "2026-01-03"}}"#);
    let decision = common::evaluate_value(&set, &input).unwrap();
    // SAFETY: a live decision.
    let explanation =
        handle(|out, error| unsafe { cr_decision_explain_value(decision.0, out, error) })
            .map(Value)
            .unwrap();
    let mut explained = json(&value_to_json(explanation.0));
    // The texts and `holds` are the value's additions to the document.
    let mut texts = Vec::new();
    let mut holds = Vec::new();
    strip(&mut explained, &mut texts, &mut holds);
    let document = json(&common::explain(&decision, CR_FORMAT_JSON).unwrap());
    let Json::Object(members) = &document else {
        panic!("not a document")
    };
    let content = members
        .iter()
        .find_map(|(k, v)| (k == "explanation").then_some(v))
        .unwrap();
    assert!(same(&explained, content), "{explained:?}");
    assert_eq!(
        texts,
        [
            "[ ] big (not fired): x > 10 AND ok\n    would fire if all of these changed:\n    - x > 10 is false (x = 8); short by 2\n    - ok is unknown: missing field ok",
            "x > 10 is false (x = 8); short by 2",
            "ok is unknown: missing field ok",
            "[x] any (selected): TRUE\n    - TRUE is true",
            "TRUE is true",
            "[x] low (fired, not selected): TRUE\n    fired, but any has a higher priority (2 > 0)\n    - TRUE is true",
            "TRUE is true",
            "[ ] \"date\" (not fired): d < DATE \"2026-01-01\"\n    - d < DATE \"2026-01-01\" is false (d = DATE \"2026-01-03\"); over by 2 days",
            "d < DATE \"2026-01-01\" is false (d = DATE \"2026-01-03\"); over by 2 days",
        ]
    );
    assert_eq!(holds, [false, false, true, true, false]);
}

/// Take the `text` and `holds` members out of `value`, in document order, and
/// read each `condition`, a node's JSON text, as JSON.
fn strip(value: &mut Json, texts: &mut Vec<String>, holds: &mut Vec<bool>) {
    match value {
        Json::Object(members) => {
            let mut text = None;
            members.retain(|(key, item)| match (key.as_str(), item) {
                ("text", Json::String(t)) => {
                    text = Some(t.clone());
                    false
                }
                ("holds", Json::Bool(h)) => {
                    holds.push(*h);
                    false
                }
                _ => true,
            });
            let at = texts.len();
            for (key, item) in members.iter_mut() {
                if key == "condition"
                    && let Json::String(node) = item
                {
                    *item = json(node);
                } else {
                    strip(item, texts, holds);
                }
            }
            // A rule's text comes before its causes', as in the text.
            if let Some(text) = text {
                if members.iter().any(|(k, _)| k == "rule") {
                    texts.insert(at, text);
                } else {
                    texts.push(text);
                }
            }
        }
        Json::Array(items) => {
            for item in items {
                strip(item, texts, holds);
            }
        }
        _ => {}
    }
}

#[test]
fn action_errors_come_from_the_core() {
    let (r, rn) = arg(b"vip");
    let (a, an) = arg(b"notify");
    let (d, dn) = arg(b"IOException: down");
    let executed = value(r#"["first"]"#);
    let mut made = ptr::null_mut();
    // SAFETY: the texts and the value outlive the call; `made` is writable.
    let status = unsafe {
        cr_error_new_action(
            r,
            rn,
            a,
            an,
            d,
            dn,
            executed.0,
            &raw mut made,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, CR_OK);
    let failure = common::take(made);
    assert_eq!(failure.status, CR_ACTION_ERROR);
    assert_eq!(
        message(&failure),
        "the action \"notify\" of rule \"vip\" threw IOException: down"
    );
    assert_eq!(member(&failure, "executed"), Some(&json(r#"["first"]"#)));
    assert_eq!(
        member(&failure, "cause"),
        Some(&Json::String("IOException: down".into()))
    );
    let mut made = ptr::null_mut();
    // SAFETY: as above, with no action name and no executed rules.
    let status = unsafe {
        cr_error_new_action(
            r,
            rn,
            ptr::null(),
            0,
            d,
            dn,
            ptr::null(),
            &raw mut made,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, CR_OK);
    assert_eq!(
        message(&common::take(made)),
        "the action of rule \"vip\" threw IOException: down"
    );
    let numbers = value("[1]");
    let failure = handle(|out, error| {
        // SAFETY: as above, with a list that is not of names.
        unsafe { cr_error_new_action(r, rn, ptr::null(), 0, d, dn, numbers.0, out, error) }
    })
    .err()
    .unwrap();
    assert_eq!(failure.status, CR_INVALID_ARGUMENT);
}

#[test]
fn node_hashes_are_the_same_everywhere() {
    let x = field(r#"["x"]"#);
    let one = constant("1").unwrap();
    let gt = compare("gt", &x, &one).unwrap();
    // Pinned: FNV-1a over fixed-width little-endian integers is the same on
    // every platform; a new version of the library may change it.
    // SAFETY: a live node.
    let hash = unsafe { cr_node_hash(gt.0) };
    assert_eq!(hash, PINNED_HASH);
}

const PINNED_HASH: u64 = 0x46f5_31bc_746b_7fef;
