package cleanrules

import (
	"errors"
	"reflect"
	"slices"
	"testing"
)

// TestRuleOptions checks each option and what a rule gives back.
func TestRuleOptions(t *testing.T) {
	r := rule(t, "vip", Field("customer", "years").Gt(5),
		Then("discount", MustDecimal("0.15")),
		Then("total", Field("order", "total").Mul(0.9)),
		Then("label", "gold"),
		Then("discount", MustDecimal("0.20")), // keeps its place
		WithPriority(10),
		WithTags("loyalty", "b2c", "loyalty"),
		WithDescription("loyal customers"),
		WithMetadata(map[string]any{"owner": "sales", "since": Date{2020, 1, 1}, "n": nil}),
		WithAction("notify_sales"))
	if r.Name() != "vip" || r.When().String() != "customer.years > 5" {
		t.Errorf("name %q, when %v", r.Name(), r.When())
	}
	var keys, values []string
	for _, entry := range r.Payload() {
		keys = append(keys, entry.Key)
		values = append(values, entry.Value.String())
	}
	if !reflect.DeepEqual(keys, []string{"discount", "total", "label"}) ||
		!reflect.DeepEqual(values, []string{"0.20", "order.total * 0.9e0", `"gold"`}) {
		t.Errorf("payload %v = %v", keys, values)
	}
	if r.Priority() != 10 || !reflect.DeepEqual(r.Tags(), []string{"b2c", "loyalty"}) ||
		r.Description() != "loyal customers" || r.ActionName() != "notify_sales" {
		t.Errorf("priority %d, tags %v, description %q, action %q", r.Priority(), r.Tags(), r.Description(), r.ActionName())
	}
	want := map[string]any{"n": nil, "owner": "sales", "since": Date{2020, 1, 1}}
	if !reflect.DeepEqual(r.Metadata(), want) {
		t.Errorf("metadata %#v", r.Metadata())
	}
	// With gives a new rule; the old one does not change.
	r2 := must(r.With(WithPriority(1), Then("extra", 1)))(t)
	if r2.Priority() != 1 || len(r2.Payload()) != 4 || r.Priority() != 10 || len(r.Payload()) != 3 {
		t.Error("With changed the rule")
	}
	if same := must(r.With())(t); same != r {
		t.Error("With without options gives the rule itself")
	}
	plain := rule(t, "plain", Field("x"))
	if plain.Priority() != 0 || plain.Tags() != nil || plain.Description() != "" || plain.ActionName() != "" ||
		len(plain.Payload()) != 0 || len(plain.Metadata()) != 0 {
		t.Error("a rule without options")
	}
}

// TestRuleErrors checks the errors of rules and rule sets, with their codes
// and context, made by the core.
func TestRuleErrors(t *testing.T) {
	x := Field("x").Gt(1)
	tests := []struct {
		name string
		err  func() error
		code Code
		rule string
	}{
		{"empty name", func() error { _, err := NewRule("", x); return err }, CodeRuleDefinition, ""},
		{"value as condition", func() error { _, err := NewRule("r", Field("x").Add(1)); return err }, CodeRuleDefinition, "r"},
		{"null condition", func() error { _, err := NewRule("r", Const(nil)); return err }, CodeExpression, ""},
		{"nil condition", func() error { _, err := NewRule("r", nil); return err }, CodeExpression, ""},
		{"condition in payload", func() error { _, err := NewRule("r", x, Then("k", x)); return err }, CodeRuleDefinition, "r"},
		{"invalid payload value", func() error { _, err := NewRule("r", x, Then("k", Field(""))); return err }, CodeExpression, ""},
		{"opaque metadata", func() error {
			_, err := NewRule("r", x, WithMetadata(map[string]any{"f": complex(1, 1)}))
			return err
		}, CodeRuleDefinition, "r"},
		{"empty action name", func() error { _, err := NewRule("r", x, WithAction("")); return err }, CodeRuleDefinition, "r"},
		{"repeated names", func() error {
			_, err := NewRuleSet([]*Rule{rule(t, "a", x), rule(t, "a", x)})
			return err
		}, CodeRuleDefinition, "a"},
		{"unknown hit policy", func() error {
			_, err := NewRuleSet(nil, WithHitPolicy("best"))
			return err
		}, CodeRuleDefinition, ""},
		{"aggregate without collect", func() error {
			_, err := NewRuleSet(nil, WithAggregate(AggregateSum))
			return err
		}, CodeRuleDefinition, ""},
		{"unknown aggregate", func() error {
			_, err := NewRuleSet(nil, WithHitPolicy(HitPolicyCollect), WithAggregate("avg"))
			return err
		}, CodeRuleDefinition, ""},
		{"unknown logic", func() error { _, err := NewRuleSet(nil, WithLogic("fuzzy")); return err }, CodeRegistry, ""},
		{"empty logic", func() error { _, err := NewRuleSet(nil, WithLogic("")); return err }, CodeRegistry, ""},
		{"nil rule", func() error { _, err := NewRuleSet([]*Rule{nil}); return err }, CodeRuleDefinition, ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := tt.err()
			var e *Error
			if !errors.As(err, &e) || e.Code != tt.code || e.Rule != tt.rule {
				t.Fatalf("got %#v, want %s in rule %q", err, tt.code, tt.rule)
			}
			if e.Message == "" || e.JSON == "" {
				t.Errorf("no message or JSON form: %+v", e)
			}
		})
	}
	// An unknown logic names what exists.
	_, err := NewRuleSet(nil, WithLogic("fuzzy"))
	if e := err.(*Error); e.Name != "fuzzy" || !slices.IsSorted(e.Available) ||
		!slices.Contains(e.Available, "boolean") || !slices.Contains(e.Available, "lukasiewicz") {
		t.Errorf("registry error %+v", e)
	}
}

// TestRuleSets checks a rule set's settings and lookups.
func TestRuleSets(t *testing.T) {
	a := rule(t, "a", Field("x").Gt(1), Then("n", Field("y")))
	b := rule(t, "b", Field("z"))
	rs := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyCollect), WithAggregate(AggregateSum), WithLogic("boolean"), WithStrict(true)}, a, b)
	if rs.Len() != 2 || rs.HitPolicy() != HitPolicyCollect || rs.Aggregate() != AggregateSum || rs.Logic() != "boolean" || !rs.Strict() {
		t.Errorf("settings %d %s %s %s %v", rs.Len(), rs.HitPolicy(), rs.Aggregate(), rs.Logic(), rs.Strict())
	}
	if rules := rs.Rules(); len(rules) != 2 || rules[0] != a || rules[1] != b {
		t.Errorf("rules %v", rules)
	}
	if got := must(rs.Rule("b"))(t); got != b {
		t.Errorf("Rule(b) = %v", got)
	}
	_, err := rs.Rule("c")
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeRuleNotFound || e.Name != "c" ||
		!reflect.DeepEqual(e.Available, []string{"a", "b"}) || !errors.Is(err, ErrRuleNotFound) {
		t.Errorf("Rule(c): %#v", err)
	}
	if paths := must(rs.FieldPaths())(t); !reflect.DeepEqual(paths, [][]any{{"x"}, {"y"}, {"z"}}) {
		t.Errorf("paths %v", paths)
	}
	defaults := ruleSet(t, nil, a)
	if defaults.HitPolicy() != HitPolicyFirst || defaults.Aggregate() != "" || defaults.Logic() != "boolean" || defaults.Strict() {
		t.Error("default settings")
	}
	empty := ruleSet(t, nil)
	if d := must(empty.Evaluate(map[string]any{}))(t); len(d.Results()) != 0 {
		t.Error("an empty rule set")
	}
}
