package cleanrules

import (
	"errors"
	"reflect"
	"slices"
	"strings"
	"testing"
)

// TestRuleOptions checks each option and what a rule gives back.
func TestRuleOptions(t *testing.T) {
	r := rule(t, "vip", Field("customer", "years").Gt(5),
		Then("discount", MustDecimal("0.15")),
		Then("total", Field("order", "total").Mul(0.9)),
		Then("label", "gold"),
		Then("discount", MustDecimal("0.20")), // takes the new value
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
	// A payload has no order: its keys come sorted by code point.
	if !reflect.DeepEqual(keys, []string{"discount", "label", "total"}) ||
		!reflect.DeepEqual(values, []string{"0.20", `"gold"`, "order.total * 0.9e0"}) {
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
		plain.ID() != "" || plain.EffectiveID() != "plain" ||
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

// TestRuleIdentity checks a rule's id, its effective id, and the checks of
// rule sets on effective ids (spec/rules.md, Identity), all made by the core.
func TestRuleIdentity(t *testing.T) {
	x := Field("x").Gt(1)
	gold := rule(t, "gold_customer", x, WithID("vip"), WithPriority(2))
	if gold.Name() != "gold_customer" || gold.ID() != "vip" || gold.EffectiveID() != "vip" {
		t.Errorf("name %q, id %q, effective id %q", gold.Name(), gold.ID(), gold.EffectiveID())
	}
	plain := rule(t, "plain", x)
	if plain.ID() != "" || plain.EffectiveID() != "plain" {
		t.Errorf("without an id: %q, effective %q", plain.ID(), plain.EffectiveID())
	}
	// An id may equal the rule's own name, and is taken literally.
	if self := rule(t, "a", x, WithID("a")); self.ID() != "a" || self.EffectiveID() != "a" {
		t.Errorf("an id equal to the name: %q", self.ID())
	}
	if spaced := rule(t, "a", x, WithID(" PRICING 12 ")); spaced.ID() != " PRICING 12 " {
		t.Errorf("an id is never normalized: %q", spaced.ID())
	}
	// With gives a new rule with another id; the old one keeps its own.
	renamed := must(gold.With(WithID("PRICING-12")))(t)
	if renamed.ID() != "PRICING-12" || gold.ID() != "vip" || renamed.Priority() != 2 {
		t.Errorf("With: %q, old %q", renamed.ID(), gold.ID())
	}
	var zero *Rule
	if zero.ID() != "" || zero.EffectiveID() != "" {
		t.Error("a nil rule has no id")
	}

	// An empty id is a rule_definition_error naming the rule.
	_, err := NewRule("r", x, WithID(""))
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeRuleDefinition || e.Rule != "r" || !errors.Is(err, ErrRuleDefinition) {
		t.Errorf("an empty id: %#v", err)
	}
	if _, err := gold.With(WithID("")); !errors.Is(err, ErrRuleDefinition) {
		t.Errorf("an empty id with With: %v", err)
	}

	// Effective ids are unique within a rule set; the error names the later rule.
	repeats := map[string]struct {
		rules []*Rule
		rule  string
	}{
		"two equal ids":           {[]*Rule{rule(t, "a", x, WithID("x")), rule(t, "b", x, WithID("x"))}, "b"},
		"id of an earlier name":   {[]*Rule{rule(t, "x", x), rule(t, "b", x, WithID("x"))}, "b"},
		"name of an earlier id":   {[]*Rule{rule(t, "a", x, WithID("x")), rule(t, "x", x)}, "x"},
		"names before ids":        {[]*Rule{rule(t, "a", x, WithID("x")), rule(t, "b", x, WithID("x")), rule(t, "a", x, WithID("y"))}, "a"},
		"the same rule twice":     {[]*Rule{gold, gold}, "gold_customer"},
		"an id equal to its name": {[]*Rule{rule(t, "a", x, WithID("a")), rule(t, "b", x, WithID("a"))}, "b"},
	}
	for name, tt := range repeats {
		t.Run(name, func(t *testing.T) {
			_, err := NewRuleSet(tt.rules)
			var e *Error
			if !errors.As(err, &e) || e.Code != CodeRuleDefinition || e.Rule != tt.rule || !errors.Is(err, ErrRuleDefinition) {
				t.Errorf("got %#v, want a rule_definition_error in rule %q", err, tt.rule)
			}
		})
	}
	// One rule's id may be another's name when that rule has an id of its own.
	a, b := rule(t, "a", Field("x"), WithID("b")), rule(t, "b", Field("y"), WithID("a"))
	swapped := ruleSet(t, nil, a, b)
	// Rules are looked up and named in outputs by name, never by id.
	if got := must(swapped.Rule("a"))(t); got != a {
		t.Errorf("Rule(a) = %v", got.Name())
	}
	d := must(swapped.Evaluate(map[string]any{"x": true, "y": false}))(t)
	if !slices.Equal(d.Fired(), []string{"a"}) {
		t.Errorf("fired %v", d.Fired())
	}
	if ids := []string{swapped.Rules()[0].EffectiveID(), swapped.Rules()[1].EffectiveID()}; !slices.Equal(ids, []string{"b", "a"}) {
		t.Errorf("effective ids %v", ids)
	}
}

// TestRuleIdentityDocuments checks that the id is written after the name and
// read back, in JSON, YAML and the rule syntax, alone and in a rule set.
func TestRuleIdentityDocuments(t *testing.T) {
	gold := rule(t, "gold_customer", Field("vip"), WithID("vip"), WithPriority(2))
	plain := rule(t, "default", Const(true))
	rs := ruleSet(t, nil, gold, plain)

	js := must(Dumps(gold))(t)
	if !strings.Contains(js, `"name":"gold_customer","id":"vip","when"`) {
		t.Errorf("JSON: %s", js)
	}
	if js := must(Dumps(plain))(t); strings.Contains(js, `"id"`) {
		t.Errorf("a rule without an id is written without one: %s", js)
	}
	text := must(Dumps(gold, WithFormat(FormatText)))(t)
	if text != "RULE gold_customer ID vip PRIORITY 2\nIF vip" {
		t.Errorf("text: %q", text)
	}
	if r := must(RuleFromText(text))(t); r.ID() != "vip" || r.Name() != "gold_customer" {
		t.Errorf("RuleFromText: %q %q", r.Name(), r.ID())
	}
	if r := must(RuleFromText(`RULE a ID "PRICING 12" IF x`))(t); r.ID() != "PRICING 12" {
		t.Errorf("a quoted id: %q", r.ID())
	}
	for _, format := range []Format{FormatJSON, FormatYAML, FormatText} {
		t.Run(string(format), func(t *testing.T) {
			back := must(Loads(must(Dumps(rs, WithFormat(format)))(t), WithFormat(format)))(t).(*RuleSet)
			rules := back.Rules()
			if len(rules) != 2 || rules[0].ID() != "vip" || rules[0].EffectiveID() != "vip" ||
				rules[1].ID() != "" || rules[1].EffectiveID() != "default" {
				t.Errorf("read back: %q %q", rules[0].ID(), rules[1].ID())
			}
			r := must(Loads(must(Dumps(gold, WithFormat(format)))(t), WithFormat(format)))(t).(*Rule)
			if r.ID() != "vip" {
				t.Errorf("rule read back: %q", r.ID())
			}
		})
	}
	// A document whose effective ids repeat is refused where the id is.
	_, err := Loads(`{"version":1,"ruleset":{"rules":[{"name":"x","when":{"op":"field","path":["p"]}},` +
		`{"name":"b","id":"x","when":{"op":"field","path":["q"]}}]}}`)
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeRuleDefinition || e.Pointer == nil || *e.Pointer != "/ruleset/rules/1/id" {
		t.Errorf("repeated effective ids in a document: %#v", err)
	}
}
