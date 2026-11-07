package cleanrules

import (
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"
)

// TestMisc covers the small parts of the API.
func TestMisc(t *testing.T) {
	if v := Version(); strings.Count(v, ".") != 2 || ABIVersion != 1 || FormatVersion != 1 {
		t.Errorf("version %q", v)
	}
	for truth, want := range map[Truth]string{TruthTrue: "true", TruthFalse: "false", TruthUnknown: "unknown"} {
		if truth.String() != want {
			t.Errorf("%d is %s", truth, truth)
		}
	}
	func() {
		defer func() {
			if err, _ := recover().(error); !errors.Is(err, ErrRuleDefinition) {
				t.Errorf("Must panicked with %v", err)
			}
		}()
		Must(NewRule("", Field("x")))
	}()
	var nilRule *Rule
	if nilRule.Name() != "" {
		t.Error("a nil rule's name")
	}
	// An impossible datetime is opaque, so it comes back as itself.
	bad := DateTime{2026, 2, 30, 0, 0, 0, 0}
	if got := echoed(t, map[string]any{"x": bad}); got != any(bad) {
		t.Errorf("impossible datetime: %#v", got)
	}
	if got := echoed(t, map[string]any{"x": DateTime{Year: 10000, Month: 1, Day: 1}}); got != any(DateTime{Year: 10000, Month: 1, Day: 1}) {
		t.Errorf("datetime of year 10000: %#v", got)
	}
	// A path with an index, in an error's context.
	strict := ruleSet(t, []RuleSetOption{WithStrict(true)}, rule(t, "r", Field("items", 2, "n").Gt(1)))
	_, err := strict.Evaluate(map[string]any{"items": []any{1}})
	var e *Error
	if !errors.As(err, &e) || !reflect.DeepEqual(e.Path, []any{"items", 2, "n"}) {
		t.Errorf("path with an index: %#v", err)
	}
	// An action's output is an empty map for a rule without a payload.
	rs := ruleSet(t, nil, rule(t, "r", Const(true), WithAction("a")))
	var output map[string]any
	bound := must(rs.Bind(Actions{"a": func(ctx *ActionContext) error { output = ctx.Output(); return nil }}))(t)
	must(bound.Run(nil))(t)
	if output == nil || len(output) != 0 {
		t.Errorf("output %#v", output)
	}
	// A time zone the core does not know keeps the offset.
	odd := time.Date(2026, 1, 1, 0, 0, 0, 0, time.FixedZone("Mars/Olympus", 3600))
	if got := echoed(t, map[string]any{"x": odd}).(time.Time); !got.Equal(odd) {
		t.Errorf("odd zone: %v", got)
	}
}
