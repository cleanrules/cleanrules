package cleanrules

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"reflect"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"
)

// ---- Actions that misbehave.

type brokenError struct{ detail *string }

// Error dereferences its field: it panics for a typed nil or a nil field.
func (e *brokenError) Error() string { return *e.detail }

// TestActionErrorsThatPanic checks that an error whose Error method panics,
// or a typed nil one, never lets a panic reach the native frames.
func TestActionErrorsThatPanic(t *testing.T) {
	rs := ruleSet(t, nil, rule(t, "r", Const(true), WithAction("act")))
	for name, returned := range map[string]error{
		"typed nil":           (*brokenError)(nil),
		"Error method panics": &brokenError{},
	} {
		t.Run(name, func(t *testing.T) {
			bound := must(rs.Bind(Actions{"act": func(*ActionContext) error { return returned }}))(t)
			_, err := bound.Run(nil)
			var e *Error
			if !errors.As(err, &e) || e.Code != CodeAction || e.Cause == "" || errors.Unwrap(err) != returned {
				t.Fatalf("got %#v", err)
			}
			t.Logf("cause: %s", e.Cause)
		})
	}
	// The rule set still works.
	if d := must(rs.Evaluate(nil))(t); !d.Results()[0].Fired {
		t.Error("after the runs")
	}
}

// TestActionGoexit checks that runtime.Goexit in an action (what t.FailNow
// does) stops the run without crossing the native frames, and that Run
// then calls runtime.Goexit on its own goroutine.
func TestActionGoexit(t *testing.T) {
	rs := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyRuleOrder)},
		rule(t, "a", Const(true), WithAction("exit")),
		rule(t, "b", Const(true), WithAction("later")))
	later := false
	bound := must(rs.Bind(Actions{
		"exit":  func(*ActionContext) error { runtime.Goexit(); return nil },
		"later": func(*ActionContext) error { later = true; return nil },
	}))(t)
	returned, deferred := false, false
	var wg sync.WaitGroup
	wg.Add(1)
	go func() {
		defer wg.Done()
		defer func() { deferred = true }()
		_, _ = bound.Run(nil)
		returned = true
	}()
	wg.Wait()
	if returned || !deferred || later {
		t.Errorf("returned %v, deferred %v, later action ran %v", returned, deferred, later)
	}
	// The same rule set runs again, and a panic is still raised again.
	bound = must(rs.Bind(Actions{"exit": func(*ActionContext) error { return nil }, "later": func(*ActionContext) error { panic(7) }}))(t)
	func() {
		defer func() {
			if v := recover(); v != 7 {
				t.Errorf("recovered %v", v)
			}
		}()
		_, _ = bound.Run(nil)
	}()
}

// ---- Decisions after their rule set is closed, and JSON forms.

func TestDecisionAfterRuleSetClose(t *testing.T) {
	rs := pricing(t)
	d := must(rs.Evaluate(map[string]any{"customer": map[string]any{"years": 3}}))(t)
	_ = rs.Close()
	why := must(d.WhyNot("vip"))(t) // the explanation is made after Close
	if why.Rule != "vip" || why.Status != StatusNotFired {
		t.Errorf("WhyNot after Close: %+v", why)
	}
	_, err := d.WhyNot("gold")
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeRuleNotFound || e.Name != "gold" ||
		!reflect.DeepEqual(e.Available, []string{"base", "loyal", "vip"}) || e.Message != `no rule named "gold"` ||
		e.Hint != "the rules are: base, loyal, vip" || !strings.Contains(e.JSON, `"available":["base","loyal","vip"]`) {
		t.Errorf("rule_not_found: %#v", err)
	}
	// The same error as the core's lookup on an open rule set.
	_, core := pricing(t).Rule("gold")
	if c := core.(*Error); c.Message != e.Message || c.Hint != e.Hint || c.JSON != e.JSON {
		t.Errorf("Go-made %s, core %s", e.JSON, c.JSON)
	}
}

// pointer resolves a JSON Pointer in a decoded document.
func pointer(t *testing.T, doc any, p string) any {
	t.Helper()
	for _, token := range strings.Split(p, "/")[1:] {
		switch node := doc.(type) {
		case map[string]any:
			doc = node[token]
		case []any:
			i, err := strconv.Atoi(token)
			if err != nil || i >= len(node) {
				t.Fatalf("pointer %s", p)
			}
			doc = node[i]
		default:
			t.Fatalf("pointer %s", p)
		}
	}
	return doc
}

func TestJSONForms(t *testing.T) {
	d := must(pricing(t).Evaluate(map[string]any{"customer": map[string]any{"years": 7}, "order": map[string]any{"total": 500}}))(t)
	var decision, explanation map[string]any
	if err := json.Unmarshal([]byte(must(d.JSON())(t)), &decision); err != nil || decision["version"] != 1.0 {
		t.Fatalf("Decision.JSON: %v", err)
	}
	e := must(d.Explain())(t)
	if err := json.Unmarshal([]byte(e.JSON()), &explanation); err != nil || explanation["explanation"] == nil {
		t.Fatalf("Explanation.JSON: %v", err)
	}
	// Each cause's At points at a result with a truth in Decision.JSON.
	for _, rule := range e.Rules {
		for _, cause := range rule.Causes() {
			result, _ := pointer(t, decision, cause.At).(map[string]any)
			if _, ok := result["truth"]; !ok {
				t.Errorf("%s: %s is not a result with a truth", rule.Rule, cause.At)
			}
		}
	}
	if r := must(e.ForRule("loyal"))(t); r != e.Rules[1] {
		t.Error("ForRule")
	}
	if _, err := e.ForRule("x"); codeOf(err) != CodeRuleNotFound {
		t.Errorf("ForRule(x): %v", err)
	}
	_ = d.Close()
	if _, err := d.JSON(); !errors.Is(err, ErrClosed) {
		t.Errorf("JSON after Close: %v", err)
	}
	if e.JSON() == "" || must(d.WhyNot("vip"))(t) == nil {
		t.Error("an explanation made before Close")
	}
}

// ---- Errors that user input must never turn into invalid_argument.

func TestNoInvalidArgument(t *testing.T) {
	bad := string([]byte{'a', 0xff})
	x := Field("x").Gt(1)
	rs := ruleSet(t, nil, rule(t, "a", x))
	errs := map[string]error{
		"closed":      Field("x").BetweenClosed(1, 2, "sides").Err(),
		"rule name":   func() error { _, err := NewRule(bad, x); return err }(),
		"key":         func() error { _, err := NewRule("r", x, Then(bad, 1)); return err }(),
		"description": func() error { _, err := NewRule("r", x, WithDescription(bad)); return err }(),
		"action":      func() error { _, err := NewRule("r", x, WithAction(bad)); return err }(),
		"logic":       func() error { _, err := NewRuleSet(nil, WithLogic(bad)); return err }(),
		"hit policy":  func() error { _, err := NewRuleSet(nil, WithHitPolicy(HitPolicy(bad))); return err }(),
		"lookup":      func() error { _, err := rs.Rule(bad); return err }(),
		"decimal":     func() error { _, err := ParseDecimal("abc"); return err }(),
		"text test":   Field("x").Contains(bad).Err(),
		"zero option": func() error { _, err := NewRule("r", x, RuleOption{}); return err }(),
		"zero set option": func() error {
			_, err := NewRuleSet(nil, RuleSetOption{})
			return err
		}(),
	}
	for name, err := range errs {
		var e *Error
		if !errors.As(err, &e) || e.Code == CodeInvalidArgument || e.Code == CodeInternal {
			t.Errorf("%s: %#v", name, err)
			continue
		}
		if strings.Contains(e.Message, "cr_") || strings.Contains(e.JSON, `"message":"cr_`) {
			t.Errorf("%s: a C function in %s", name, e.JSON)
		}
	}
}

// TestErrorPlaces checks Pointer (nil when it does not apply, "" for the
// root), Line and Column.
func TestErrorPlaces(t *testing.T) {
	_, err := readNode(`{"op":`)
	if e := err.(*Error); e.Code != CodeFormat || e.Pointer != nil || e.Line != 1 || e.Column != 7 {
		t.Errorf("syntax: %+v", e)
	}
	_, err = readNode(`[]`)
	if e := err.(*Error); e.Pointer == nil || *e.Pointer != "" || e.Line != 0 {
		t.Errorf("root: %+v", e)
	}
	_, err = readNode(`{"op":"gt","left":{"op":"field","path":[]},"right":{"op":"const","value":1}}`)
	if e := err.(*Error); e.Code != CodeExpression || e.Pointer == nil || *e.Pointer != "/left" {
		t.Errorf("node: %+v", e)
	}
	if e := Field().Err().(*Error); e.Pointer != nil {
		t.Errorf("no pointer: %+v", e)
	}
}

// ---- Values.

func TestTextForms(t *testing.T) {
	d := must(ruleSet(t, nil, rule(t, "r", Const(true),
		Then("rate", MustDecimal("0.15")),
		Then("day", Date{2026, 10, 31}),
		Then("at", DateTime{2026, 10, 31, 9, 30, 0, 5}))).Evaluate(nil))(t)
	out, _ := d.Output()
	text, err := json.Marshal(map[string]any{"out": out, "truth": d.Results()[0].Truth})
	if err != nil || string(text) != `{"out":{"at":"2026-10-31T09:30:00.000005","day":"2026-10-31","rate":"0.15"},"truth":"true"}` {
		t.Errorf("%s %v", text, err)
	}
	var back struct {
		Rate  Decimal
		Day   Date
		At    DateTime
		Truth Truth
	}
	if err := json.Unmarshal([]byte(`{"Rate":"1.50","Day":"2026-02-28","At":"2026-02-28T01:02:03.5","Truth":"unknown"}`), &back); err != nil ||
		back.Rate != MustDecimal("1.50") || back.Day != (Date{2026, 2, 28}) || back.At != (DateTime{2026, 2, 28, 1, 2, 3, 500000}) || back.Truth != TruthUnknown {
		t.Errorf("%+v %v", back, err)
	}
	for _, bad := range []string{`{"Rate":"x"}`, `{"Day":"2026-02-30"}`, `{"At":"2026-02-28 01:02"}`, `{"Truth":"maybe"}`} {
		if err := json.Unmarshal([]byte(bad), &back); err == nil {
			t.Errorf("%s read", bad)
		}
	}
	// The zero Decimal is 0, under ==, whatever made the 0.
	zero := must(ruleSet(t, nil, rule(t, "r", Const(true), Then("z", MustDecimal("0")))).Evaluate(nil))(t).Results()[0].Output["z"]
	if zero != (Decimal{}) || MustDecimal("0") != (Decimal{}) || MustDecimal("0e5") != (Decimal{}) || MustDecimal("0.0") == (Decimal{}) {
		t.Errorf("zero %#v", zero)
	}
}

type keyText string

// TestMapKeys checks maps whose keys are not of kind string: their string
// keys are read, the others left out.
func TestMapKeys(t *testing.T) {
	data := map[any]any{"a": 1, 2: "two", keyText("b"): true, nil: 3, "c": map[any]any{"d": 4}}
	if got := echoed(t, map[string]any{"x": data}); !reflect.DeepEqual(got, map[string]any{"a": int64(1), "b": true, "c": map[string]any{"d": int64(4)}}) {
		t.Errorf("map[any]any: %#v", got)
	}
	// When two keys have the same text, the one of type string wins.
	if got := echoed(t, map[string]any{"x": map[any]any{keyText("a"): 1, "a": 2}}); !reflect.DeepEqual(got, map[string]any{"a": int64(2)}) {
		t.Errorf("same text: %#v", got)
	}
	if got := echoed(t, map[string]any{"x": map[int]string{1: "a"}}); !reflect.DeepEqual(got, map[string]any{}) {
		t.Errorf("map[int]string: %#v", got)
	}
	rs := ruleSet(t, nil, rule(t, "r", Field("cfg", "c", "d").Eq(4).And(Field("cfg", "a").Eq(1))))
	if d := must(rs.Evaluate(map[string]any{"cfg": data}))(t); !d.Results()[0].Fired {
		t.Error("paths through map[any]any")
	}
	cause := firstCause(t, must(rs.Evaluate(map[string]any{"cfg": map[int]int{1: 1}}))(t))
	if cause.Reason == nil || cause.Reason.Code != "missing_field" {
		t.Errorf("map[int]int: %+v", cause.Reason)
	}
}

// TestKeyOrder checks the order of the keys of maps and structs read
// whole, which the core keeps in decisions: sorted for a Go map, in
// declaration order for a struct, whatever the order of iteration.
func TestKeyOrder(t *testing.T) {
	type record struct {
		Zeta, Alpha, Mid int
	}
	rs := ruleSet(t, nil, rule(t, "r", Field("m").IsNull().Or(Field("s").IsNull())))
	m := map[string]int{}
	for _, k := range []string{"k", "c", "x", "a", "q", "b", "z", "é", "m", "d"} {
		m[k] = len(k)
	}
	want := `"value":{"a":1,"b":1,"c":1,"d":1,"k":1,"m":1,"q":1,"x":1,"z":1,"é":2}`
	for range 10 {
		text := must(must(rs.Evaluate(map[string]any{"m": m, "s": record{1, 2, 3}}))(t).JSON())(t)
		if !strings.Contains(text, want) || !strings.Contains(text, `"value":{"Zeta":1,"Alpha":2,"Mid":3}`) {
			t.Fatalf("order: %s", text)
		}
	}
}

// TestDeepAndShared checks the depth beyond which containers are opaque, and
// that shared containers are read once and come out whole at every place.
func TestDeepAndShared(t *testing.T) {
	var deep any = "leaf"
	levels := []any{}
	for range 300 {
		deep = []any{deep}
		levels = append(levels, deep)
	}
	got := echoed(t, map[string]any{"x": deep})
	depth := 0
	for {
		list, ok := got.([]any)
		if !ok || len(list) != 1 {
			t.Fatalf("depth %d: %#v", depth, got)
		}
		original := levels[len(levels)-1-depth].([]any)
		if depth == maxDepth {
			// The original slice itself: opaque.
			if &list[0] != &original[0] {
				t.Errorf("at depth %d, not the native slice", depth)
			}
			break
		}
		if &list[0] == &original[0] {
			t.Fatalf("opaque at depth %d", depth)
		}
		got, depth = list[0], depth+1
	}
	// A diamond of shared slices 14 deep: 2^14 leaves, each level read once.
	var shared any = 1
	for range 14 {
		shared = []any{shared, shared}
	}
	start := time.Now()
	out := echoed(t, map[string]any{"x": shared})
	for range 14 {
		pair := out.([]any)
		if len(pair) != 2 || !reflect.DeepEqual(pair[0], pair[1]) {
			t.Fatal("a shared slice differs between its places")
		}
		out = pair[1]
	}
	if out != int64(1) {
		t.Errorf("leaf %#v", out)
	}
	t.Logf("2^14 leaves in %v", time.Since(start))
}

// ---- Structs, as encoding/json.

type Left struct{ X, L int }
type Right struct{ X, R int }
type Twice struct{ X int }
type Inner1 struct{ Twice }
type Inner2 struct{ Twice }
type taggedA struct {
	X int `json:"X"`
}
type plainB struct{ X int }

type structCases struct {
	Diamond struct {
		Inner1
		Inner2
	}
	Untagged struct {
		Left
		Right
	}
	Tagged struct {
		taggedA
		plainB
	}
	Tags struct {
		Own     int `cleanrules:"own" json:"-"`
		Empty   int `cleanrules:",omitempty" json:"json_name"`
		Skip    int `cleanrules:"-" json:"kept"`
		Invalid int `json:"bad\"name"`
		JSON    int `json:"json"`
		Dash    int `json:"-,"`
	}
}

// TestStructsAsJSON checks the fields read against encoding/json's choice,
// and the cleanrules tag winning over the json one.
func TestStructsAsJSON(t *testing.T) {
	var c structCases
	c.Diamond.Inner1.X, c.Diamond.Inner2.X = 1, 2
	c.Untagged.Left = Left{X: 1, L: 2}
	c.Untagged.Right = Right{X: 3, R: 4}
	c.Tagged.taggedA.X, c.Tagged.plainB.X = 5, 6
	for _, part := range []any{c.Diamond, c.Untagged, c.Tagged} {
		var viaJSON map[string]any
		text, _ := json.Marshal(part)
		_ = json.Unmarshal(text, &viaJSON)
		got := echoed(t, map[string]any{"x": part}).(map[string]any)
		if len(got) != len(viaJSON) {
			t.Errorf("%T: got %v, encoding/json %v", part, got, viaJSON)
		}
		for k, v := range viaJSON {
			if fmt.Sprint(got[k]) != fmt.Sprint(v) {
				t.Errorf("%T.%s: got %v, encoding/json %v", part, k, got[k], v)
			}
		}
	}
	got := echoed(t, map[string]any{"x": c.Tags})
	want := map[string]any{"own": int64(0), "Empty": int64(0), "Invalid": int64(0), "json": int64(0), "-": int64(0)}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("tags: %#v", got)
	}
}

// ---- Batches, outputs and ownership.

func TestBatchesOfNothing(t *testing.T) {
	rs := ruleSet(t, nil, rule(t, "a", Field("x").Gt(1)))
	for name, rows := range map[string]any{"nil": nil, "nil slice": []map[string]any(nil), "nil pointer": (*[]int)(nil)} {
		d, err := rs.EvaluateMany(rows)
		if err != nil || len(d) != 0 {
			t.Errorf("%s: %v %v", name, d, err)
		}
	}
}

func TestOutputsAndCopies(t *testing.T) {
	rs := ruleSet(t, []RuleSetOption{WithHitPolicy(HitPolicyRuleOrder)},
		rule(t, "with", Const(true), Then("n", 1), Then("list", Field("l"))),
		rule(t, "without", Const(true)))
	d := must(rs.Evaluate(map[string]any{"l": []int{1, 2}}))(t)
	outputs := d.Outputs()
	if len(outputs) != 2 || outputs[1] == nil || len(outputs[1]) != 0 {
		t.Fatalf("outputs %#v", outputs)
	}
	if out := d.Results()[1].Output; out == nil || len(out) != 0 {
		t.Errorf("a selected rule without a payload has an empty one: %#v", out)
	}
	// Changing what a decision gives changes nothing in it.
	outputs[0]["n"] = "changed"
	outputs[0]["list"].([]any)[0] = "changed"
	results := d.Results()
	results[0].Output["n"] = "changed"
	d.Selected()[0].Output["n"] = "changed"
	if d.Outputs()[0]["n"] != int64(1) || d.Outputs()[0]["list"].([]any)[0] != int64(1) || d.Results()[0].Output["n"] != int64(1) {
		t.Error("a decision changed")
	}
}

func TestBindOwnership(t *testing.T) {
	for _, keep := range []string{"bound", "original"} {
		t.Run("keep "+keep, func(t *testing.T) {
			var kept *RuleSet
			var h *handle
			func() {
				rs := ruleSet(t, nil, rule(t, "a", Const(true), WithAction("act")))
				bound := must(rs.Bind(Actions{"act": func(*ActionContext) error { return nil }}))(t)
				kept, h = bound, rs.h
				if keep == "original" {
					kept = rs
				}
			}()
			for range 5 {
				runtime.GC()
				time.Sleep(5 * time.Millisecond)
			}
			if _, err := kept.Evaluate(nil); err != nil {
				t.Fatalf("the rule set kept: %v", err)
			}
			h.mu.Lock()
			owners := h.owners
			h.mu.Unlock()
			if owners != 1 {
				t.Errorf("%d owners left", owners)
			}
			runtime.KeepAlive(kept)
		})
	}
}

// TestZeroValues checks that nil and zero values give errors, never panics.
func TestZeroValues(t *testing.T) {
	var nilRS *RuleSet
	var nilD *Decision
	for name, err := range map[string]error{
		"zero RuleSet":    func() error { _, err := (&RuleSet{}).Evaluate(nil); return err }(),
		"nil RuleSet":     func() error { _, err := nilRS.Run(nil); return err }(),
		"nil Summary":     func() error { _, err := nilRS.Summary(); return err }(),
		"nil FieldPaths":  func() error { _, err := nilRS.FieldPaths(); return err }(),
		"zero Bind":       func() error { _, err := (&RuleSet{}).Bind(nil); return err }(),
		"zero Decision":   func() error { _, err := (&Decision{}).Explain(); return err }(),
		"nil Decision":    func() error { _, err := nilD.JSON(); return err }(),
		"nil WhyNot":      func() error { _, err := nilD.WhyNot("a"); return err }(),
		"nil Output":      func() error { _, err := nilD.Output(); return err }(),
		"zero Decision 2": func() error { _, err := (&Decision{}).JSON(); return err }(),
	} {
		if !errors.Is(err, ErrClosed) {
			t.Errorf("%s: %v", name, err)
		}
	}
	if nilRS.Close() != nil || nilD.Close() != nil || (&RuleSet{}).Close() != nil || nilRS.Len() != 0 || nilRS.Rules() != nil ||
		nilRS.HitPolicy() != "" || nilD.Results() != nil || nilD.Fired() != nil || nilD.Outputs() != nil || nilD.Selected() != nil ||
		nilD.Aggregated() != nil || nilD.RuleSet() != nil || nilD.HitPolicy() != "" {
		t.Error("nil receivers")
	}
	if _, err := NewRuleSet([]*Rule{{}}); codeOf(err) != CodeRuleDefinition {
		t.Errorf("zero Rule: %v", err)
	}
	if (&Rule{}).Priority() != 0 || (&Expr{}).String() == "" || (&Expr{}).Tag() != "" {
		t.Error("zero Rule and Expr")
	}
}

// TestFirstError checks that IsIn reports its receiver's error first.
func TestFirstError(t *testing.T) {
	broken := Field("")
	if err := broken.IsIn(math.NaN()).Err(); err != broken.Err() {
		t.Errorf("got %v", err)
	}
	if err := broken.BetweenClosed(1, 2, "x").Err(); err != broken.Err() {
		t.Errorf("between: %v", err)
	}
}
