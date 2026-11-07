package cleanrules

import (
	"errors"
	"fmt"
	"reflect"
	"strings"
	"sync"
	"testing"
)

func actionRules(t *testing.T) *RuleSet {
	t.Helper()
	return ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyRuleOrder)},
		rule(t, "first", Field("x").Gt(0), Then("n", Field("x")), WithAction("record")),
		rule(t, "plain", Const(true)),
		rule(t, "second", Field("x").Gt(1), Then("n", Field("x").Mul(2)), WithAction("record")),
		rule(t, "never", Field("x").Lt(0), WithAction("explode")))
}

// TestRun checks that Run calls the actions of the selected rules in rule
// order, with their context, and gives back the decision they saw.
func TestRun(t *testing.T) {
	type call struct {
		rule     string
		position int
		output   map[string]any
		fired    bool
	}
	var calls []call
	var seen *Decision
	data := map[string]any{"x": 3}
	bound := must(actionRules(t).Bind(Actions{
		"record": func(ctx *ActionContext) error {
			calls = append(calls, call{ctx.Rule().Name(), ctx.Position(), ctx.Output(), ctx.Result().Fired})
			if !reflect.DeepEqual(ctx.Data(), data) {
				t.Errorf("data %v", ctx.Data())
			}
			seen = ctx.Decision()
			return nil
		},
		"explode": func(*ActionContext) error { panic("never selected") },
	}))(t)
	d := must(bound.Run(data))(t)
	want := []call{{"first", 0, map[string]any{"n": int64(3)}, true}, {"second", 2, map[string]any{"n": int64(6)}, true}}
	if !reflect.DeepEqual(calls, want) {
		t.Errorf("calls %+v", calls)
	}
	if seen != d || !reflect.DeepEqual(d.Fired(), []string{"first", "plain", "second"}) {
		t.Error("Run gives back the decision the actions saw")
	}
	if _, err := must(d.Explain())(t), error(nil); err != nil || d.Results()[0].Output["n"] != int64(3) {
		t.Error("the decision of a run stays usable")
	}
	// Evaluate never runs actions; Run without them selected gives a new decision.
	calls = nil
	must(bound.Evaluate(data))(t)
	if d := must(bound.Run(map[string]any{"x": 0}))(t); len(calls) != 0 || len(d.Results()) != 4 {
		t.Errorf("no action ran: %v", calls)
	}
}

// TestRunRegistry checks that names are checked before anything runs.
func TestRunRegistry(t *testing.T) {
	ran := false
	record := func(*ActionContext) error { ran = true; return nil }
	_, err := actionRules(t).Bind(Actions{"record": record})
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeRegistry || e.Name != "explode" || e.Rule != "never" ||
		!reflect.DeepEqual(e.Available, []string{"record"}) {
		t.Fatalf("Bind: %#v", err)
	}
	// A nil function is not registered.
	if _, err := actionRules(t).Bind(Actions{"record": record, "explode": nil}); codeOf(err) != CodeRegistry {
		t.Errorf("nil action: %v", err)
	}
	// Run without Bind: the core checks before evaluating.
	_, err = actionRules(t).Run(map[string]any{"x": 3})
	if codeOf(err) != CodeRegistry || ran {
		t.Errorf("Run unbound: %v, ran %v", err, ran)
	}
	// The name "", which no rule can give, is left out.
	if _, err := actionRules(t).Bind(Actions{"": record, "record": record, "explode": record}); err != nil {
		t.Errorf("empty name: %v", err)
	}
	// A name that is not UTF-8: the core's format_error itself, not wrapped.
	_, err = actionRules(t).Bind(Actions{string([]byte{0xff}): record})
	if e, ok := err.(*Error); !ok || e.Code != CodeFormat || strings.HasPrefix(e.Message, "cr_") {
		t.Errorf("a name that is not UTF-8: %#v", err)
	}
	// A rule set without actions runs with none.
	plain := ruleSet(t, nil, rule(t, "a", Const(true)))
	if d := must(plain.Run(nil))(t); !d.Results()[0].Fired {
		t.Error("a run without actions")
	}
}

var errDeclined = errors.New("the payment service declined")

// TestRunActionError checks that an action's error stops the run and is
// wrapped in an action_error with the rule, the executed rules and the
// decision.
func TestRunActionError(t *testing.T) {
	count := 0
	bound := must(actionRules(t).Bind(Actions{
		"record": func(ctx *ActionContext) error {
			count++
			if ctx.Rule().Name() == "second" {
				return fmt.Errorf("charging %v: %w", ctx.Output()["n"], errDeclined)
			}
			return nil
		},
		"explode": func(*ActionContext) error { return nil },
	}))(t)
	d, err := bound.Run(map[string]any{"x": 3})
	var e *Error
	if d != nil || !errors.As(err, &e) || e.Code != CodeAction || e.Rule != "second" ||
		!reflect.DeepEqual(e.Executed, []string{"first"}) || count != 2 {
		t.Fatalf("got %v, %#v", d, err)
	}
	if !errors.Is(err, errDeclined) || !errors.Is(err, ErrAction) || errors.Unwrap(err) == nil {
		t.Error("the action's error is wrapped")
	}
	if e.Cause != "charging 6: the payment service declined" || e.Message == "" || e.JSON == "" {
		t.Errorf("cause %q, message %q", e.Cause, e.Message)
	}
	if e.Decision == nil || !reflect.DeepEqual(e.Decision.Fired(), []string{"first", "plain", "second"}) {
		t.Fatal("the error has the decision")
	}
	if _, err := e.Decision.Explain(); err != nil {
		t.Errorf("the error's decision explains: %v", err)
	}
	if err := e.Decision.Close(); err != nil {
		t.Error(err)
	}
}

// TestRunPanic checks that a panic in an action never crosses into C: the
// run stops, and Run panics again with the same value.
func TestRunPanic(t *testing.T) {
	type boom struct{ n int }
	rs := actionRules(t)
	bound := must(rs.Bind(Actions{
		"record":  func(*ActionContext) error { panic(boom{7}) },
		"explode": func(*ActionContext) error { return nil },
	}))(t)
	func() {
		defer func() {
			if got := recover(); got != (boom{7}) {
				t.Errorf("recovered %v", got)
			}
		}()
		_, _ = bound.Run(map[string]any{"x": 3})
		t.Error("Run did not panic")
	}()
	// A runtime error too, and the rule set is still usable.
	bound = must(rs.Bind(Actions{
		"record":  func(ctx *ActionContext) error { var m map[string]int; m["x"] = 1; return nil },
		"explode": func(*ActionContext) error { return nil },
	}))(t)
	func() {
		defer func() {
			if _, ok := recover().(error); !ok {
				t.Error("a runtime error is panicked again")
			}
		}()
		_, _ = bound.Run(map[string]any{"x": 3})
	}()
	if d := must(rs.Evaluate(map[string]any{"x": 3}))(t); len(d.Fired()) != 3 {
		t.Error("the rule set after a panic")
	}
}

// TestRunNested checks an action that runs rules again, and runs on many
// goroutines at once.
func TestRunNested(t *testing.T) {
	inner := ruleSet(t, nil, rule(t, "inner", Field("y").Gt(0), WithAction("leaf")))
	var mu sync.Mutex
	leaves := 0
	innerBound := must(inner.Bind(Actions{"leaf": func(*ActionContext) error {
		mu.Lock()
		leaves++
		mu.Unlock()
		return nil
	}}))(t)
	outer := ruleSet(t, nil, rule(t, "outer", Const(true), WithAction("nest")))
	outerBound := must(outer.Bind(Actions{"nest": func(ctx *ActionContext) error {
		d, err := innerBound.Run(map[string]any{"y": 1})
		if err != nil || !d.Results()[0].Fired {
			return fmt.Errorf("inner run: %v", err)
		}
		return nil
	}}))(t)
	var wg sync.WaitGroup
	for range 8 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for range 20 {
				if _, err := outerBound.Run(nil); err != nil {
					t.Error(err)
					return
				}
			}
		}()
	}
	wg.Wait()
	if leaves != 160 {
		t.Errorf("leaves %d", leaves)
	}
}
