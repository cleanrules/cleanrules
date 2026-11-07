package cleanrules

import (
	"errors"
	"testing"

	// The zones the tests load, on every platform, whatever the system has.
	_ "time/tzdata"
)

// must fails the test on an error: must(f())(t).
func must[T any](v T, err error) func(t *testing.T) T {
	return func(t *testing.T) T {
		t.Helper()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		return v
	}
}

// ruleSet makes a rule set of rules made with NewRule.
func ruleSet(t *testing.T, options []RuleSetOption, rules ...*Rule) *RuleSet {
	t.Helper()
	return must(NewRuleSet(rules, options...))(t)
}

// rule makes a rule or fails.
func rule(t *testing.T, name string, when *Expr, options ...RuleOption) *Rule {
	t.Helper()
	return must(NewRule(name, when, options...))(t)
}

// echo is a rule set giving back the value at x as the output v.
func echo(t *testing.T) *RuleSet {
	t.Helper()
	return ruleSet(t, nil, rule(t, "echo", Const(true), Then("v", Field("x"))))
}

// echoed gives back what reading x of data gives.
func echoed(t *testing.T, data any) any {
	t.Helper()
	d := must(echo(t).Evaluate(data))(t)
	return d.Results()[0].Output["v"]
}

// codeOf is the code of a cleanrules error, or "".
func codeOf(err error) Code {
	var e *Error
	if errors.As(err, &e) {
		return e.Code
	}
	return ""
}

// firstCause is the first cause of the only rule of a decision.
func firstCause(t *testing.T, d *Decision) *Cause {
	t.Helper()
	explanation := must(d.Explain())(t)
	causes := explanation.Rules[0].Causes()
	if len(causes) == 0 {
		t.Fatal("no causes")
	}
	return causes[0]
}
