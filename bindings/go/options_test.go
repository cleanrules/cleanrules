package cleanrules

import (
	"errors"
	"strings"
	"testing"
)

// TestNilOptions checks that a nil option of every kind is the same error,
// whose hint lists the options of its function only.
func TestNilOptions(t *testing.T) {
	x := Field("x").Gt(1)
	rs := echo(t)
	cases := []struct {
		name, hint string
		err        error
	}{
		{"NewRule", ruleOptions, func() error { _, err := NewRule("r", x, nil); return err }()},
		{"Rule.With", ruleOptions, func() error { _, err := rule(t, "r", x).With(nil); return err }()},
		{"NewRuleSet", ruleSetOptions, func() error { _, err := NewRuleSet(nil, nil); return err }()},
		{"NewDecisionTable", tableOptions, func() error { _, err := NewDecisionTable([]any{"x", "THEN y"}, nil, nil); return err }()},
		{"EvaluateTree", treeOptions, func() error { _, err := x.EvaluateTree(nil, nil); return err }()},
		{"EvaluateValueTree", valueTreeOptions, func() error { _, err := Field("x").EvaluateValueTree(nil, nil); return err }()},
		{"Loads", loadOptions, func() error { _, err := Loads("{}", nil); return err }()},
		{"Load", loadOptions, func() error { _, err := Load("x.json", nil); return err }()},
		{"RuleFromText", textOptions, func() error { _, err := RuleFromText("IF x > 1 THEN y = 1", nil); return err }()},
		{"Dumps", dumpOptions, func() error { _, err := Dumps(x, nil); return err }()},
		{"Dump", dumpOptions, func() error { return Dump(x, "x.json", nil) }()},
		{"ReadDecision", readOptions, func() error { _, err := rs.ReadDecision("{}", nil); return err }()},
	}
	for _, c := range cases {
		var e *Error
		if !errors.As(c.err, &e) || e.Code != CodeRuleDefinition || e.Message != "a nil option sets nothing" || e.Hint != "make options with "+c.hint {
			t.Errorf("%s: %#v", c.name, c.err)
		}
	}
	if strings.Contains(textOptions, "WithFormat") || strings.Contains(valueTreeOptions, "WithLogic") {
		t.Error("a hint lists an option its function does not take")
	}
}
