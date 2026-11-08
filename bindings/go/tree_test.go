package cleanrules

import (
	"encoding/json"
	"errors"
	"reflect"
	"testing"
)

// TestEvaluateTree checks a condition's result tree: truth values, values,
// paths, reasons, children and nodes.
func TestEvaluateTree(t *testing.T) {
	cond := Field("x").Gt(1).And(Field("y").Between(1, 3)).And(Not(Field("flag")))
	tree := must(cond.EvaluateTree(map[string]any{"x": 2, "flag": false}))(t)
	if tree.Tag != "and" || tree.Truth == nil || *tree.Truth != TruthUnknown || tree.Value != nil ||
		tree.Reason == nil || tree.Reason.Code != "missing_field" || len(tree.Children) != 3 {
		t.Fatalf("root: %+v", tree)
	}
	if !tree.Node().Equal(cond) || tree.String() != cond.String()+": unknown" {
		t.Errorf("root node: %v", tree)
	}
	gt := tree.Children[0]
	if gt.Tag != "gt" || *gt.Truth != TruthTrue || gt.Reason != nil || len(gt.Children) != 2 {
		t.Errorf("gt: %+v", gt)
	}
	field, constant := gt.Children[0], gt.Children[1]
	if field.Tag != "field" || !reflect.DeepEqual(field.Path, []any{"x"}) || field.Truth != nil || field.Value != int64(2) ||
		field.String() != "x: 2" || constant.Value != int64(1) || constant.Path != nil {
		t.Errorf("operands: %+v %+v", field, constant)
	}
	between := tree.Children[1]
	if between.Tag != "between" || len(between.Children) != 2 || between.Children[0].Node().String() != "y >= 1" ||
		between.Children[1].Tag != "le" {
		t.Errorf("between: %+v", between)
	}
	missing := between.Children[0].Children[0]
	if missing.Value != nil || missing.Reason == nil || !reflect.DeepEqual(missing.Reason.Paths, [][]any{{"y"}}) || missing.String() != "y: null" {
		t.Errorf("missing field: %+v", missing)
	}
	// A field in a condition position has a truth value and one child.
	flag := tree.Children[2].Children[0]
	if flag.Tag != "field" || flag.Truth == nil || *flag.Truth != TruthFalse || len(flag.Children) != 1 || flag.Children[0].Value != false {
		t.Errorf("flag: %+v", flag)
	}

	// JSON: the core's form of each subtree, which the root holds.
	var root map[string]any
	if err := json.Unmarshal([]byte(tree.JSON()), &root); err != nil {
		t.Fatal(err)
	}
	var child any
	if err := json.Unmarshal([]byte(between.JSON()), &child); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(root["children"].([]any)[1], child) || root["op"] != "and" {
		t.Errorf("JSON: %s\n%s", tree.JSON(), between.JSON())
	}

	// Options.
	if _, err := cond.EvaluateTree(map[string]any{}, WithStrict(true)); codeOf(err) != CodeMissingField {
		t.Errorf("strict: %v", err)
	}
	if _, err := cond.EvaluateTree(nil, WithLogic("boolean")); err != nil {
		t.Error(err)
	}
	if _, err := cond.EvaluateTree(nil, WithLogic("nope")); codeOf(err) != CodeRegistry {
		t.Errorf("logic: %v", err)
	}
	if _, err := cond.EvaluateTree(nil, nil); codeOf(err) != CodeRuleDefinition {
		t.Errorf("nil option: %v", err)
	}
	if _, err := Field("x").Add(1).EvaluateTree(nil); codeOf(err) != CodeExpression {
		t.Errorf("value node: %v", err)
	}
	if _, err := Field("x").Add("a").EvaluateTree(nil); codeOf(err) != CodeExpression {
		t.Errorf("broken node: %v", err)
	}
	if _, err := (*Expr)(nil).EvaluateTree(nil); codeOf(err) != CodeExpression {
		t.Errorf("nil node: %v", err)
	}
}

// TestEvaluateValueTree checks a value's result tree, Go data read along the
// node's paths only, and opaque values given back as themselves.
func TestEvaluateValueTree(t *testing.T) {
	type order struct {
		Total int `json:"total"`
		Note  chan int
	}
	note := make(chan int)
	tree := must(Field("order", "total").Mul(2).EvaluateValueTree(map[string]any{"order": order{Total: 21, Note: note}}))(t)
	if tree.Tag != "mul" || tree.Truth != nil || tree.Value != int64(42) || tree.Children[0].Value != int64(21) {
		t.Errorf("mul: %+v", tree)
	}
	opaque := must(Field("order", "Note").EvaluateValueTree(map[string]any{"order": &order{Note: note}}))(t)
	if opaque.Value != note {
		t.Errorf("opaque: %#v", opaque.Value)
	}
	if _, err := Field("x").Gt(1).EvaluateValueTree(nil); codeOf(err) != CodeExpression {
		t.Errorf("condition: %v", err)
	}
	if _, err := Field("x").Add(1).EvaluateValueTree(map[string]any{}, WithStrict(true)); codeOf(err) != CodeMissingField {
		t.Errorf("strict: %v", err)
	}
	if _, err := Field("x").EvaluateValueTree(nil, nil); codeOf(err) != CodeRuleDefinition {
		t.Errorf("nil option: %v", err)
	}
	var zero ResultTree
	if zero.Node().Err() == nil || zero.JSON() != "" || (*ResultTree)(nil).String() != "<nil>" {
		t.Error("a zero tree")
	}
}

// TestActionContextTree reads the result tree of an action's rule.
func TestActionContextTree(t *testing.T) {
	var seen []string
	var kept *ActionContext
	rs := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyRuleOrder)},
		rule(t, "a", Field("x").Gt(1), WithAction("look")),
		rule(t, "b", Field("x").Gt(0).And(Field("y").IsNull()), WithAction("look")))
	bound := must(rs.Bind(Actions{"look": func(ctx *ActionContext) error {
		tree, err := ctx.Tree()
		if err != nil {
			return err
		}
		again, _ := ctx.Tree()
		if again != tree {
			return errors.New("the tree was made twice")
		}
		seen = append(seen, tree.String())
		kept = ctx
		return nil
	}}))(t)
	d := must(bound.Run(map[string]any{"x": 2}))(t)
	want := []string{"x > 1: true", "x > 0 AND y IS NULL: true"}
	if !reflect.DeepEqual(seen, want) {
		t.Errorf("trees: %q", seen)
	}
	// Made before, it stays after Close; a context that never made it gets
	// ErrClosed.
	d.Close()
	if tree, err := kept.Tree(); err != nil || tree.String() != want[1] {
		t.Errorf("after Close: %v %v", tree, err)
	}
	fresh := &ActionContext{decision: d, position: 0}
	if _, err := fresh.Tree(); !errors.Is(err, ErrClosed) {
		t.Errorf("closed: %v", err)
	}
}
