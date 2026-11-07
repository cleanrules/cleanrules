package cleanrules

import (
	"encoding/json"
	"errors"
	"reflect"
	"testing"
)

// TestErrorHierarchy checks that each code matches its own sentinel and its
// ancestors' (spec/errors.md, Hierarchy), and no other.
func TestErrorHierarchy(t *testing.T) {
	sentinels := map[Code]error{
		CodeFormat: ErrFormat, CodeUnsupportedVersion: ErrUnsupportedVersion, CodeLimitExceeded: ErrLimitExceeded,
		CodeDecisionMismatch: ErrDecisionMismatch, CodeExpression: ErrExpression, CodeRuleDefinition: ErrRuleDefinition,
		CodeRuleSyntax: ErrRuleSyntax, CodeRegistry: ErrRegistry, CodeRuleNotFound: ErrRuleNotFound,
		CodeEvaluation: ErrEvaluation, CodeMissingField: ErrMissingField, CodeTypeMismatch: ErrTypeMismatch,
		CodeHitPolicy: ErrHitPolicy, CodeAction: ErrAction, CodeUnsupported: ErrUnsupported,
		CodeInvalidArgument: ErrInvalidArgument, CodeInternal: ErrInternal,
	}
	parents := map[Code]Code{
		CodeUnsupportedVersion: CodeFormat, CodeLimitExceeded: CodeFormat, CodeDecisionMismatch: CodeFormat,
		CodeMissingField: CodeEvaluation, CodeTypeMismatch: CodeEvaluation, CodeHitPolicy: CodeEvaluation,
	}
	for code := range sentinels {
		err := error(&Error{Code: code})
		for other, sentinel := range sentinels {
			want := other == code || parents[code] == other
			if got := errors.Is(err, sentinel); got != want {
				t.Errorf("errors.Is(%s, %s) = %v", code, other, got)
			}
		}
		if errors.Is(err, ErrClosed) {
			t.Errorf("%s is ErrClosed", code)
		}
	}
	if statusCodes[1] != CodeFormat || statusCodes[-1] != CodeInvalidArgument || statusCodes[-2] != CodeInternal || len(statusCodes) != 17 {
		t.Errorf("the library's codes: %v", statusCodes)
	}
	if ErrMissingField.Error() != "cleanrules: missing_field" {
		t.Error(ErrMissingField.Error())
	}
}

// TestStrictErrors checks the errors of strict mode, with their context.
func TestStrictErrors(t *testing.T) {
	rs := ruleSet(t, []RuleSetOption{WithStrict(true)},
		rule(t, "adult", Field("customer", "age").Ge(18)),
		rule(t, "named", Field("name").StartsWith("A")))
	_, err := rs.Evaluate(map[string]any{"customer": map[string]any{}})
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeMissingField || e.Rule != "adult" ||
		!reflect.DeepEqual(e.Path, []any{"customer", "age"}) || e.Row != -1 || e.Line != 0 {
		t.Fatalf("missing field: %#v", err)
	}
	if !errors.Is(err, ErrMissingField) || !errors.Is(err, ErrEvaluation) || errors.Is(err, ErrFormat) || errors.Unwrap(err) != nil {
		t.Error("missing_field is an evaluation_error")
	}
	if e.Node != `{"op":"field","path":["customer","age"]}` || e.Message == "" {
		t.Errorf("message %q, hint %q, node %q", e.Message, e.Hint, e.Node)
	}
	var form map[string]any
	if json.Unmarshal([]byte(e.JSON), &form) != nil || form["code"] != "missing_field" || form["rule"] != "adult" {
		t.Errorf("JSON form %s", e.JSON)
	}

	_, err = rs.Evaluate(map[string]any{"customer": map[string]any{"age": 20}, "name": 3})
	if !errors.As(err, &e) || e.Code != CodeTypeMismatch || e.Rule != "named" ||
		!reflect.DeepEqual(e.Types, []string{"number", "string"}) || !reflect.DeepEqual(e.Path, []any{"name"}) {
		t.Errorf("type mismatch: %#v", err)
	}

	// In a batch, the first input that fails, with its row.
	_, err = rs.EvaluateMany([]map[string]any{{"customer": map[string]any{"age": 20}, "name": "Ana"}, {}})
	if !errors.As(err, &e) || e.Code != CodeMissingField || e.Row != 1 {
		t.Errorf("batch: %#v", err)
	}
	// What is not a list of inputs.
	_, err = rs.EvaluateMany(map[string]any{})
	if !errors.As(err, &e) || e.Code != CodeEvaluation {
		t.Errorf("not a batch: %#v", err)
	}
}

// TestBatches checks EvaluateMany.
func TestBatches(t *testing.T) {
	rs := ruleSet(t, nil, rule(t, "a", Field("x").Gt(1), Then("x", Field("x"))))
	rows := [3]map[string]any{{"x": 2}, {"x": 0}, {}}
	decisions := must(rs.EvaluateMany(rows))(t)
	if len(decisions) != 3 || !decisions[0].Results()[0].Fired || decisions[1].Results()[0].Fired ||
		decisions[2].Results()[0].Truth != TruthUnknown || decisions[0].Results()[0].Output["x"] != int64(2) {
		t.Errorf("batch %v", decisions)
	}
	if empty := must(rs.EvaluateMany([]any{}))(t); len(empty) != 0 {
		t.Error("an empty batch")
	}
	// An opaque value in a batch comes back as itself.
	ch := make(chan int)
	opaque := ruleSet(t, nil, rule(t, "a", Const(true), Then("v", Field("v"))))
	got := must(opaque.EvaluateMany([]any{map[string]any{"v": 1}, map[string]any{"v": ch}}))(t)
	if got[1].Results()[0].Output["v"] != any(ch) {
		t.Errorf("opaque in a batch: %v", got[1].Results()[0].Output)
	}
}
