package cleanrules

import (
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"
)

func pricing(t *testing.T, options ...RuleSetOption) *RuleSet {
	t.Helper()
	return ruleSet(t, options,
		rule(t, "vip", Field("customer", "years").Gt(5).And(Field("order", "total").Gt(1000)),
			Then("discount", MustDecimal("0.15")), WithPriority(10), WithTags("loyalty")),
		rule(t, "loyal", Field("customer", "years").Gt(2),
			Then("discount", MustDecimal("0.05")), WithPriority(5), WithTags("loyalty")),
		rule(t, "base", Const(true), Then("discount", MustDecimal("0"))))
}

// TestDecisions checks what a decision says, for each hit policy.
func TestDecisions(t *testing.T) {
	order := map[string]any{"customer": map[string]any{"years": 7}, "order": map[string]any{"total": 500}}
	d := must(pricing(t).Evaluate(order))(t)
	results := d.Results()
	if len(results) != 3 || results[0].Fired || results[0].Truth != TruthFalse || !results[1].Fired || results[1].Truth != TruthTrue {
		t.Fatalf("results %+v", results)
	}
	if results[0].Rule.Name() != "vip" || results[0].Output != nil {
		t.Errorf("vip did not fire, so its payload was not evaluated: %+v", results[0])
	}
	if !reflect.DeepEqual(d.Fired(), []string{"loyal", "base"}) {
		t.Errorf("fired %v", d.Fired())
	}
	if selected := d.Selected(); len(selected) != 1 || selected[0].Rule.Name() != "loyal" {
		t.Errorf("selected %+v", selected)
	}
	output := must(d.Output())(t)
	if !reflect.DeepEqual(output, map[string]any{"discount": MustDecimal("0.05")}) {
		t.Errorf("output %v", output)
	}
	if outputs := d.Outputs(); len(outputs) != 1 || d.Aggregated() != nil || d.HitPolicy() != HitPolicyFirst || d.RuleSet() == nil {
		t.Errorf("outputs %v", outputs)
	}

	// Priority picks the highest.
	if out := must(must(pricing(t, WithHitPolicy(HitPolicyPriority)).Evaluate(order))(t).Output())(t); out["discount"] != MustDecimal("0.05") {
		t.Errorf("priority: %v", out)
	}
	// Several outputs: Output is a hit_policy_error with no rules.
	collect := must(pricing(t, WithHitPolicy(HitPolicyCollect)).Evaluate(order))(t)
	if len(collect.Outputs()) != 2 {
		t.Errorf("collect outputs %v", collect.Outputs())
	}
	_, err := collect.Output()
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeHitPolicy || len(e.Rules) != 0 || !errors.Is(err, ErrEvaluation) {
		t.Errorf("collect output: %#v", err)
	}
	// Aggregates.
	sum := must(pricing(t, WithHitPolicy(HitPolicyCollect), WithAggregate(AggregateSum)).Evaluate(order))(t)
	if agg := sum.Aggregated(); !reflect.DeepEqual(agg, map[string]any{"discount": 0.05}) {
		t.Errorf("sum: %#v", agg)
	}
	count := must(pricing(t, WithHitPolicy(HitPolicyCollect), WithAggregate(AggregateCount)).Evaluate(order))(t)
	if agg := count.Aggregated(); !reflect.DeepEqual(agg, map[string]any{"discount": int64(2)}) {
		t.Errorf("count: %#v", agg)
	}
	// Unique with two rules firing.
	_, err = pricing(t, WithHitPolicy(HitPolicyUnique)).Evaluate(order)
	if !errors.As(err, &e) || e.Code != CodeHitPolicy || !reflect.DeepEqual(e.Rules, []string{"loyal", "base"}) {
		t.Errorf("unique: %#v", err)
	}
}

// TestExplanations checks the structure of an explanation against the
// core's JSON document of it, and its text.
func TestExplanations(t *testing.T) {
	rs := pricing(t)
	order := map[string]any{"customer": map[string]any{"years": 7}, "order": map[string]any{"total": 500}}
	d := must(rs.Evaluate(order))(t)
	explanation := must(d.Explain())(t)
	if again := must(d.Explain())(t); again != explanation {
		t.Error("the explanation is not kept")
	}
	if explanation.HitPolicy != HitPolicyFirst || explanation.Logic != "boolean" || !reflect.DeepEqual(explanation.Selected, []string{"loyal"}) {
		t.Errorf("header %+v", explanation)
	}
	vip := explanation.Rules[0]
	if vip.Rule != "vip" || vip.Status != StatusNotFired || vip.Achievable == nil || !*vip.Achievable || vip.Note != nil {
		t.Errorf("vip %+v", vip)
	}
	causes := vip.Causes()
	if len(causes) != 1 {
		t.Fatalf("causes %v", causes)
	}
	cause := causes[0]
	if cause.Truth != TruthFalse || cause.Negated || cause.Holds || cause.Reason != nil || cause.At == "" {
		t.Errorf("cause %+v", cause)
	}
	if node := must(cause.Node())(t); node.String() != "order.total > 1000" {
		t.Errorf("node %v", node)
	}
	if len(cause.Values) != 1 || !reflect.DeepEqual(cause.Values[0], FieldValue{[]any{"order", "total"}, int64(500)}) {
		t.Errorf("values %+v", cause.Values)
	}
	cf := cause.Counterfactual
	if cf == nil || !reflect.DeepEqual(cf.Path, []any{"order", "total"}) || cf.Op != "gt" || cf.Target != int64(1000) || cf.Gap != int64(500) {
		t.Errorf("counterfactual %+v", cf)
	}
	if !strings.Contains(cause.String(), "order.total > 1000 is false") || !strings.Contains(vip.String(), "vip") {
		t.Errorf("texts %q / %q", cause, vip)
	}
	base := explanation.Rules[2]
	if base.Status != StatusFired || base.Note == nil || base.Note.Kind != NoteEarlierRule || base.Note.Winner != "loyal" || base.Achievable != nil {
		t.Errorf("base %+v %+v", base, base.Note)
	}
	text := explanation.String()
	for _, part := range []string{"HIT POLICY first: selected loyal", vip.String(), base.String()} {
		if !strings.Contains(text, part) {
			t.Errorf("text lacks %q:\n%s", part, text)
		}
	}
	why := must(d.WhyNot("vip"))(t)
	if why != vip {
		t.Error("WhyNot is the rule's explanation")
	}
	if _, err := d.WhyNot("none"); codeOf(err) != CodeRuleNotFound {
		t.Errorf("WhyNot(none): %v", err)
	}

	// Groups, negations, unknowns, priorities and gaps in days.
	rs2 := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyPriority)},
		rule(t, "a", Field("x").Gt(1).And(Not(Field("y").IsNull()), Field("due").Lt(Date{2026, 1, 1})), WithPriority(1)),
		rule(t, "b", Const(true), WithPriority(9)),
		rule(t, "c", Const(true), WithPriority(1)),
		rule(t, "u", Field("missing").Gt(1)))
	d2 := must(rs2.Evaluate(map[string]any{"x": 0, "y": nil, "due": Date{2026, 1, 11}}))(t)
	e2 := must(d2.Explain())(t)
	group, ok := e2.Rules[0].Requirement.(*CauseGroup)
	if !ok || len(group.Items) != 3 {
		t.Fatalf("requirement %#v", e2.Rules[0].Requirement)
	}
	if group.Kind != GroupAll && group.Kind != GroupAny {
		t.Errorf("group kind %q", group.Kind)
	}
	causes = e2.Rules[0].Causes()
	if !causes[1].Negated || causes[1].Truth != TruthTrue || causes[1].Holds {
		t.Errorf("negated cause %+v", causes[1])
	}
	if gap := causes[2].Counterfactual; gap == nil || gap.Gap != GapDays(10) || gap.Target != (Date{2026, 1, 1}) {
		t.Errorf("gap in days %+v", gap)
	}
	if note := e2.Rules[2].Note; note == nil || note.Kind != NoteLowerPriority || *note.Priority != 1 || *note.WinnerPriority != 9 {
		t.Errorf("note %+v", note)
	}
	unknown := e2.Rules[3]
	if unknown.Status != StatusUnknown || unknown.Causes()[0].Reason.Code != "missing_field" ||
		!reflect.DeepEqual(unknown.Causes()[0].Reason.Paths, [][]any{{"missing"}}) {
		t.Errorf("unknown %+v", unknown.Causes()[0].Reason)
	}
	// Gaps between datetimes are in microseconds.
	when := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	rs3 := ruleSet(t, nil, rule(t, "t", Field("at").Gt(when)))
	d3 := must(rs3.Evaluate(map[string]any{"at": when.Add(-1500 * time.Microsecond)}))(t)
	if gap := firstCause(t, d3).Counterfactual.Gap; gap != GapMicroseconds(1500) {
		t.Errorf("gap in microseconds %#v", gap)
	}
}

// TestSummaries checks a summary against the core's JSON document.
func TestSummaries(t *testing.T) {
	rs := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyCollect), WithAggregate(AggregateSum), WithStrict(true)},
		rule(t, "vip", Field("customer", "years").Gt(5), Then("discount", Field("rate")), WithPriority(10), WithTags("gold", "a"), WithAction("notify")),
		rule(t, "loyal", Field("customer", "years").Gt(2).And(Field("customer").IsNull().Not()), Then("discount", 0.05)))
	s := must(rs.Summary())(t)
	want := &Summary{
		HitPolicy: HitPolicyCollect, Aggregate: AggregateSum, Logic: "boolean", Strict: true,
		Rules: []RuleSummary{
			{Name: "vip", Priority: 10, Outputs: []string{"discount"}, Action: true, ActionName: "notify"},
			{Name: "loyal", Priority: 0, Outputs: []string{"discount"}},
		},
		Fields: []FieldUsage{
			{Path: []any{"customer", "years"}, Conditions: []string{"vip", "loyal"}, Payloads: []PayloadUse{}},
			{Path: []any{"rate"}, Conditions: []string{}, Payloads: []PayloadUse{{"vip", "discount"}}},
			{Path: []any{"customer"}, Conditions: []string{"loyal"}, Payloads: []PayloadUse{}},
		},
		Outputs: []string{"discount"},
		Tags:    []TagUsage{{"a", []string{"vip"}}, {"gold", []string{"vip"}}},
		json:    s.json, text: s.text,
	}
	if !reflect.DeepEqual(s, want) {
		t.Errorf("got  %+v\nwant %+v", s, want)
	}
	var doc map[string]any
	if err := json.Unmarshal([]byte(s.JSON()), &doc); err != nil || doc["summary"] == nil {
		t.Errorf("JSON %s", s.JSON())
	}
	if !strings.HasPrefix(s.String(), "Rule set: 2 rules, hit policy collect, aggregate sum, logic boolean, strict") {
		t.Errorf("text %s", s)
	}
}
