package cleanrules

import (
	"errors"
	"strings"
	"testing"
)

// TestJSONEntryPoints checks the unexported JSON entry points beyond what
// the vectors reach: a logic by name, text that is not JSON, and a closed
// rule set or node.
func TestJSONEntryPoints(t *testing.T) {
	x := Field("x").Gt(1)
	for _, logic := range []string{"", "boolean"} {
		tree, err := x.evaluateTreeJSON(`{"x":2}`, logic, false)
		if err != nil || !strings.Contains(tree, `"truth":true`) {
			t.Errorf("logic %q: %s, %v", logic, tree, err)
		}
	}
	if _, err := x.evaluateTreeJSON(`{"x":2}`, "nope", false); codeOf(err) != CodeRegistry {
		t.Errorf("an unknown logic: %v", err)
	}
	rs := ruleSet(t, nil, rule(t, "r", x))
	for name, err := range map[string]error{
		"evaluateJSON":          second(rs.evaluateJSON(`{"x":`)),
		"evaluateManyJSON":      second(rs.evaluateManyJSON(`[{"x":2},`)),
		"evaluateTreeJSON":      second(x.evaluateTreeJSON(`{"x":2`, "", false)),
		"evaluateValueTreeJSON": second(Field("x").evaluateValueTreeJSON(`{"x":2`, false)),
	} {
		var e *Error
		if !errors.As(err, &e) || e.Code != CodeFormat || e.Line != 1 {
			t.Errorf("%s of text that is not JSON: %v", name, err)
		}
	}
	if _, err := rs.evaluateManyJSON(`{"x":2}`); codeOf(err) != CodeEvaluation {
		t.Errorf("a batch that is not a list: %v", err)
	}
	decisions, err := rs.evaluateManyJSON(`[{"x":2},{"x":0}]`)
	if err != nil || len(decisions) != 2 || !decisions[0].Results()[0].Fired || decisions[1].Results()[0].Fired {
		t.Errorf("a batch: %v, %v", decisions, err)
	}
	rs.Close()
	if _, err := rs.evaluateJSON(`{"x":2}`); !errors.Is(err, ErrClosed) {
		t.Errorf("after Close: %v", err)
	}
	if _, err := rs.evaluateManyJSON(`[]`); !errors.Is(err, ErrClosed) {
		t.Errorf("after Close: %v", err)
	}
	if _, err := (&Expr{}).evaluateValueTreeJSON(`{}`, false); codeOf(err) != CodeExpression {
		t.Errorf("a zero Expr: %v", err)
	}
}

// second is the error of a call that gives a value and an error.
func second[T any](_ T, err error) error { return err }
