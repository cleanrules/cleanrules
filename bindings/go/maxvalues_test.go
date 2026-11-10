package cleanrules

import (
	"encoding/json"
	"errors"
	"math"
	"math/big"
	"strings"
	"testing"
	"time"
)

// The limit max_values on the Go data an evaluation reads
// (spec/data-model.md, Reading native data, Limits).

// limitError checks that err is the core's limit_exceeded for max_values,
// with row (-1 outside a batch) and rule ("" for none).
func limitError(t *testing.T, what string, err error, limit, row int, rule string) {
	t.Helper()
	var e *Error
	if !errors.As(err, &e) {
		t.Errorf("%s: %v, want limit_exceeded", what, err)
		return
	}
	if e.Code != CodeLimitExceeded || e.Name != "max_values" || e.Row != row || e.Rule != rule ||
		!errors.Is(err, ErrLimitExceeded) || !errors.Is(err, ErrFormat) {
		t.Errorf("%s: %#v", what, e)
	}
	if want := "the native data holds more than " + itoa(limit) + " values (max_values)"; e.Message != want {
		t.Errorf("%s: message %q, want %q", what, e.Message, want)
	}
	var form map[string]any
	if json.Unmarshal([]byte(e.JSON), &form) != nil || form["code"] != "limit_exceeded" || form["name"] != "max_values" {
		t.Errorf("%s: JSON form %s", what, e.JSON)
	}
	if _, ok := form["row"]; ok != (row >= 0) {
		t.Errorf("%s: JSON form %s, row %d", what, e.JSON, row)
	}
}

func itoa(n int) string { return big.NewInt(int64(n)).String() }

// maxValues is WithLimits with MaxValues alone.
func maxValues(n int) LimitsOption { return WithLimits(Limits{MaxValues: n}) }

// echoing is a rule set whose payload reads each path (a payload reads its
// field like a condition does), in strict mode or not.
func echoing(t *testing.T, strict bool, paths ...[]any) *RuleSet {
	t.Helper()
	var options []RuleOption
	for i, path := range paths {
		options = append(options, Then("v"+itoa(i), Field(path...)))
	}
	return ruleSet(t, []RuleSetOption{WithStrict(strict)}, rule(t, "r", Const(true), options...))
}

// diamond is data that shares one object twice at each of depth levels: 2^(depth+1) - 1
// values read whole, made of the kind of container given.
func diamond(kind string, depth int) any {
	type node struct{ L, R any }
	var level any = 1
	for range depth {
		switch kind {
		case "slice":
			level = []any{level, level}
		case "map":
			level = map[string]any{"l": level, "r": level}
		default:
			level = &node{level, level}
		}
	}
	return level
}

// TestMaxValuesDiamond checks that data shared as a diamond 30 levels deep,
// 2^31 - 1 values read whole, is refused at once, strict or not, by every
// evaluation, and that a small one counts each copy whole.
func TestMaxValuesDiamond(t *testing.T) {
	for _, c := range []struct {
		kind   string
		strict bool
	}{{"slice", false}, {"slice", true}, {"map", false}, {"pointer", true}} {
		rs := ruleSet(t, []RuleSetOption{WithStrict(c.strict)}, rule(t, "r", Field("d").IsNull()))
		start := time.Now()
		_, err := rs.Evaluate(map[string]any{"d": diamond(c.kind, 30)})
		limitError(t, c.kind+" diamond", err, 1_000_000, -1, "")
		elapsed := time.Since(start)
		if elapsed > 30*time.Second {
			t.Errorf("%s diamond: %v to refuse", c.kind, elapsed)
		}
		t.Logf("%s diamond, strict %v: refused in %v", c.kind, c.strict, elapsed)
	}
	for _, kind := range []string{"slice", "map", "pointer"} {
		// 2^4 - 1 values at depth 3, each level converted once and copied.
		rs := echoing(t, false, []any{"d"})
		data := map[string]any{"d": diamond(kind, 3)}
		if _, err := rs.Evaluate(data, maxValues(15)); err != nil {
			t.Errorf("%s diamond at the limit: %v", kind, err)
		}
		_, err := rs.Evaluate(data, maxValues(14))
		limitError(t, kind+" diamond one over", err, 14, -1, "")
	}
	// The other evaluations refuse it too (under a lower limit, for time).
	data := map[string]any{"d": diamond("map", 30)}
	table := must(NewDecisionTable([]any{"d", "THEN y"}, [][]any{{"is null", 1}}))(t)
	tree := Field("d").IsNull()
	limit := maxValues(10_000)
	for name, err := range map[string]error{
		"EvaluateMany":       func() error { _, err := echoing(t, true, []any{"d"}).EvaluateMany([]any{data}, limit); return err }(),
		"Run":                func() error { _, err := echoing(t, false, []any{"d"}).Run(data, limit); return err }(),
		"table Evaluate":     func() error { _, err := table.Evaluate(data, limit); return err }(),
		"table EvaluateMany": func() error { _, err := table.EvaluateMany([]any{data}, limit); return err }(),
		"EvaluateTree":       func() error { _, err := tree.EvaluateTree(data, limit); return err }(),
		"EvaluateValueTree":  func() error { _, err := Field("d").EvaluateValueTree(data, limit); return err }(),
	} {
		row := -1
		if strings.HasSuffix(name, "EvaluateMany") {
			row = 0
		}
		limitError(t, name, err, 10_000, row, "")
	}
}

// TestMaxValuesBoundaries checks what counts, each case exactly at its limit
// and one value over it, in both modes.
func TestMaxValuesBoundaries(t *testing.T) {
	type pair struct {
		X int
		Y []int
	}
	type wrapper struct {
		K     int
		Other []int
	}
	shared := []any{1, 2}
	point := &pair{1, []int{2}}
	cycle := []any{nil}
	cycle[0] = cycle
	huge := make([]int, 1000)
	cases := []struct {
		name  string
		paths [][]any
		data  any
		count int
	}{
		{"list where a path ends", [][]any{{"a"}}, map[string]any{"a": []any{1, []any{2, 3}}}, 5},
		{"map where a path ends", [][]any{{"a"}}, map[string]any{"a": map[string]any{"b": 1, "c": map[string]int{"d": 2, "e": 3}}}, 5},
		{"struct where a path ends", [][]any{{"a"}}, map[string]any{"a": pair{1, []int{2, 3}}}, 5},
		{"array where a path ends", [][]any{{"a"}}, map[string]any{"a": [3]int{1, 2, 3}}, 4},
		{"a shared list, twice", [][]any{{"a"}, {"b"}}, map[string]any{"a": shared, "b": shared}, 6},
		{"a shared pointer, twice", [][]any{{"a"}, {"b"}}, map[string]any{"a": point, "b": point}, 8},
		{"a shared list at two depths", [][]any{{"a"}, {"b"}}, map[string]any{"a": shared, "b": []any{shared}}, 7},
		{"a slice walked through", [][]any{{"xs", 1}}, map[string]any{"xs": []int{10, 20, 30}}, 4},
		{"an array walked through", [][]any{{"xs", 1}}, map[string]any{"xs": [3]int{10, 20, 30}}, 4},
		{"two positions of a slice", [][]any{{"xs", 0}, {"xs", 2}}, map[string]any{"xs": []int{10, 20, 30}}, 5},
		{"an index beyond the length", [][]any{{"xs", 5}}, map[string]any{"xs": []int{10, 20, 30}}, 3},
		{"a list at a walked position", [][]any{{"xs", 0}}, map[string]any{"xs": []any{[]int{1, 2}}}, 4},
		{"a map walked through", [][]any{{"m", "k"}, {"x"}}, map[string]any{"m": map[string]any{"k": 1, "other": huge}, "x": 1}, 2},
		{"a struct walked through", [][]any{{"w", "K"}, {"x"}}, map[string]any{"w": wrapper{1, huge}, "x": 1}, 2},
		{"a prefix path", [][]any{{"a"}, {"a", "b"}}, map[string]any{"a": map[string]any{"b": []int{1, 2}}}, 4},
		{"scalars where paths stop", [][]any{{"a", "b"}, {"c", 0}}, map[string]any{"a": 5, "c": "text"}, 2},
		{"nulls where paths stop", [][]any{{"a", "b"}, {"c", "d"}}, map[string]any{"a": nil, "c": (*pair)(nil)}, 2},
		{"missing values count nothing", [][]any{{"a", "b"}, {"c"}, {"x"}}, map[string]any{"x": 1}, 1},
		{"a nil input", [][]any{{"a"}, {"b"}}, nil, 1},
		{"nulls with reasons", [][]any{{"a"}}, map[string]any{"a": []any{uint64(math.MaxUint64), math.NaN(), new(big.Int).Lsh(big.NewInt(1), 70)}}, 4},
		{"opaque values", [][]any{{"a"}}, map[string]any{"a": []any{make(chan int), []byte("x"), time.Second}}, 4},
		{"an opaque value where a path stops", [][]any{{"a", "b"}, {"x"}}, map[string]any{"a": make(chan int), "x": 1}, 2},
		{"a cycle", [][]any{{"a"}}, map[string]any{"a": cycle}, 2},
	}
	for _, c := range cases {
		lenient := echoing(t, false, c.paths...)
		if _, err := lenient.Evaluate(c.data, maxValues(c.count)); err != nil {
			t.Errorf("%s at %d: %v", c.name, c.count, err)
		}
		if c.count == 1 {
			continue // a limit of 0 is the default
		}
		for _, strict := range []bool{false, true} {
			_, err := echoing(t, strict, c.paths...).Evaluate(c.data, maxValues(c.count-1))
			limitError(t, c.name+" one over", err, c.count-1, -1, "")
		}
	}
}

// TestMaxValuesLengths checks that a list counts its length before it is
// allocated: lengths far beyond the limit are refused at once.
func TestMaxValuesLengths(t *testing.T) {
	// Zero-size elements: a slice and an array of any length cost no memory.
	slice := make([]struct{}, math.MaxInt)
	array := new([math.MaxInt32]struct{})
	for name, data := range map[string]any{"slice": slice, "array": array} {
		for _, path := range [][]any{{"xs"}, {"xs", 0}, {"xs", 1 << 20}} {
			start := time.Now()
			_, err := echoing(t, false, path).Evaluate(map[string]any{"xs": data})
			limitError(t, name, err, 1_000_000, -1, "")
			if elapsed := time.Since(start); elapsed > time.Second {
				t.Errorf("%s along %v: %v to refuse", name, path, elapsed)
			}
		}
	}

	// A long slice at the default limit: itself and 999,999 elements fit,
	// one more does not.
	long := make([]int, 1_000_000)
	rs := echoing(t, false, []any{"xs"})
	d, err := rs.Evaluate(map[string]any{"xs": long[:999_999]})
	if err != nil || len(d.Results()[0].Output["v0"].([]any)) != 999_999 {
		t.Errorf("999,999 elements: %v", err)
	}
	_, err = rs.Evaluate(map[string]any{"xs": long})
	limitError(t, "1,000,000 elements", err, 1_000_000, -1, "")

	// Nested maps: 1 + 100 + 100 * 100 values.
	outer := map[string]any{}
	for i := range 100 {
		inner := map[string]int{}
		for j := range 100 {
			inner[itoa(j)] = j
		}
		outer[itoa(i)] = inner
	}
	if _, err := rs.Evaluate(map[string]any{"xs": outer}, maxValues(10_101)); err != nil {
		t.Errorf("nested maps at the limit: %v", err)
	}
	_, err = rs.Evaluate(map[string]any{"xs": outer}, maxValues(10_100))
	limitError(t, "nested maps", err, 10_100, -1, "")
}

// TestMaxValuesBatches checks that each row counts on its own, and that the
// first row beyond the limit gives its row, before any error of the
// evaluation.
func TestMaxValuesBatches(t *testing.T) {
	rs := echoing(t, false, []any{"a"})
	small, large := map[string]any{"a": []int{1, 2}}, map[string]any{"a": []int{1, 2, 3}}
	if _, err := rs.EvaluateMany([]any{small, small, small}, maxValues(3)); err != nil {
		t.Errorf("rows at the limit: %v", err)
	}
	_, err := rs.EvaluateMany([]any{small, large, large}, maxValues(3))
	limitError(t, "batch", err, 3, 1, "")
	if decisions, err := rs.EvaluateMany(nil, maxValues(1)); err != nil || len(decisions) != 0 {
		t.Errorf("no rows: %v %v", decisions, err)
	}
	// The number of rows is not counted (each row is an input), but a list
	// of them that cannot be allocated is an error, not a panic.
	_, err = rs.EvaluateMany(make([]struct{}, math.MaxInt))
	var e *Error
	if !errors.As(err, &e) || e.Code != CodeEvaluation || e.Row != -1 || !strings.Contains(e.Message, "rows is more than memory can hold") {
		t.Errorf("rows beyond memory: %v", err)
	}

	// A list shared by the rows: each row counts its copy whole.
	shared := []int{1, 2}
	rows := []map[string]any{{"a": shared}, {"a": shared}}
	if _, err := rs.EvaluateMany(rows, maxValues(3)); err != nil {
		t.Errorf("shared rows: %v", err)
	}
	_, err = rs.EvaluateMany(rows, maxValues(2))
	limitError(t, "shared rows", err, 2, 0, "")

	// Row 0 fails in strict mode, row 1 is beyond the limit: data is read
	// before it is evaluated.
	strict := echoing(t, true, []any{"a"})
	_, err = strict.EvaluateMany([]any{map[string]any{}, large}, maxValues(3))
	limitError(t, "before the evaluation", err, 3, 1, "")
	_, err = strict.EvaluateMany([]any{map[string]any{}, small}, maxValues(3))
	if codeOf(err) != CodeMissingField {
		t.Errorf("within the limit: %v", err)
	}

	// Tables, runs and trees.
	table := must(NewDecisionTable([]any{"a", "THEN y"}, [][]any{{"is not null", 1}}))(t)
	if _, err := table.EvaluateMany([]any{small, small}, maxValues(3)); err != nil {
		t.Errorf("table rows: %v", err)
	}
	_, err = table.EvaluateMany([]any{small, large}, maxValues(3))
	limitError(t, "table batch", err, 3, 1, "")
	if _, err := table.Evaluate(small, maxValues(3)); err != nil {
		t.Errorf("table: %v", err)
	}
	_, err = table.Evaluate(large, maxValues(3))
	limitError(t, "table", err, 3, -1, "")

	ran := false
	bound := must(echoing(t, false, []any{"a"}).Bind(Actions{}))(t)
	actions := must(NewRuleSet([]*Rule{rule(t, "r", Const(true), Then("v", Field("a")), WithAction("act"))}))(t)
	bound = must(actions.Bind(Actions{"act": func(*ActionContext) error { ran = true; return nil }}))(t)
	_, err = bound.Run(large, maxValues(3))
	limitError(t, "run", err, 3, -1, "")
	if ran {
		t.Error("an action ran on an input beyond the limit")
	}
	if _, err := bound.Run(small, maxValues(3)); err != nil || !ran {
		t.Errorf("run within the limit: %v", err)
	}

	condition := Field("a").IsNull()
	if _, err := condition.EvaluateTree(small, maxValues(3)); err != nil {
		t.Errorf("tree: %v", err)
	}
	_, err = condition.EvaluateTree(large, maxValues(3), WithStrict(true))
	limitError(t, "tree", err, 3, -1, "")
	if _, err := Field("a").EvaluateValueTree(small, maxValues(3)); err != nil {
		t.Errorf("value tree: %v", err)
	}
	_, err = Field("a").EvaluateValueTree(large, WithStrict(true), maxValues(3))
	limitError(t, "value tree", err, 3, -1, "")
}

// TestMaxValuesConstants checks that constants, payloads and metadata are
// converted under the default limit, the metadata's error naming its rule.
func TestMaxValuesConstants(t *testing.T) {
	endless := make([]struct{}, math.MaxInt)
	long := make([]int, 1_000_000)
	for name, err := range map[string]error{
		"Const":         Const(endless).Err(),
		"Const of ints": Const(long).Err(),
		"IsIn":          Field("x").IsIn(long).Err(),
		"IsIn endless":  Field("x").IsIn(endless).Err(),
		"Then": func() error {
			_, err := NewRule("r", Const(true), Then("v", endless))
			return err
		}(),
	} {
		limitError(t, name, err, 1_000_000, -1, "")
	}
	_, err := NewRule("big", Const(true), WithMetadata(map[string]any{"xs": endless}))
	limitError(t, "metadata", err, 1_000_000, -1, "big")
	r := rule(t, "other", Const(true))
	_, err = r.With(WithMetadata(map[string]any{"xs": long}))
	limitError(t, "metadata by With", err, 1_000_000, -1, "other")
	_, err = NewRule("tagged", Const(true), WithTags(make([]string, 1_000_000)...))
	limitError(t, "tags", err, 1_000_000, -1, "tagged")

	// Within the default, as before; the evaluation's limit does not reach
	// them.
	meta := rule(t, "m", Field("x").IsIn(long[:10]), WithMetadata(map[string]any{"xs": long[:10]}))
	rs := ruleSet(t, nil, meta)
	if _, err := rs.Evaluate(map[string]any{"x": 0}, maxValues(1)); err != nil {
		t.Errorf("an evaluation within its limit: %v", err)
	}
}

// TestMaxValuesMessages checks that no message names a C function: the C
// ABI no longer writes them (B.11), and the package no longer strips them.
func TestMaxValuesMessages(t *testing.T) {
	for _, err := range []error{
		maxValuesError(5, 2, "r"),
		Const(math.NaN()).Err(),
		Field().Err(),
		func() error { _, err := Loads("{"); return err }(),
		func() error { _, err := RuleFromText("\xff"); return err }(),
	} {
		var e *Error
		if !errors.As(err, &e) || strings.HasPrefix(e.Message, "cr_") || strings.Contains(e.JSON, `"message":"cr_`) {
			t.Errorf("%#v", err)
		}
	}
	limitError(t, "maxValuesError", maxValuesError(5, 2, "r"), 5, 2, "r")
}

// TestEvaluationLimits checks that only MaxValues acts on the Go data of an
// evaluation (native data has a limit of its own, spec/json-format.md,
// Errors and limits): MaxNesting, MaxSize and MaxDepth do not.
func TestEvaluationLimits(t *testing.T) {
	rs := echoing(t, true, []any{"a"})
	data := map[string]any{"a": []any{[]any{[]any{1}}}}
	others := WithLimits(Limits{MaxNesting: 1, MaxSize: 1, MaxDepth: 1, MaxTableSize: 1, MaxYAMLSize: 1, MaxTextSize: 1})
	table := must(NewDecisionTable([]any{"a", "THEN y"}, [][]any{{"is not null", 1}}))(t)
	for name, err := range map[string]error{
		"Evaluate":           func() error { _, err := rs.Evaluate(data, others); return err }(),
		"EvaluateMany":       func() error { _, err := rs.EvaluateMany([]any{data, data}, others); return err }(),
		"Run":                func() error { _, err := rs.Run(data, others); return err }(),
		"table Evaluate":     func() error { _, err := table.Evaluate(data, others); return err }(),
		"table EvaluateMany": func() error { _, err := table.EvaluateMany([]any{data}, others); return err }(),
		"EvaluateTree":       func() error { _, err := Field("a").IsNull().EvaluateTree(data, others); return err }(),
		"EvaluateValueTree":  func() error { _, err := Field("a").EvaluateValueTree(data, others); return err }(),
	} {
		if err != nil {
			t.Errorf("%s: %v", name, err)
		}
	}
	d := must(rs.Evaluate(data, others))(t)
	if v := d.Results()[0].Output["v0"]; !equalValues(v, []any{[]any{[]any{int64(1)}}}) {
		t.Errorf("read %#v", v)
	}
}

// TestNoABINames checks that no message or hint of a Go error names a C ABI
// function (cr_...) or constant (CR_...), among the errors of evaluations,
// limits, options and construction.
func TestNoABINames(t *testing.T) {
	rs := echoing(t, true, []any{"a"})
	deep := map[string]any{}
	path := []any{}
	var nested any = 1
	for range 300 {
		nested = []any{nested}
	}
	var data any = nested
	for range 900 {
		path = append(path, "k")
		data = map[string]any{"k": data}
	}
	far := ruleSet(t, nil, rule(t, "far", Const(true), Then("v", Field(path...))))
	for name, err := range map[string]error{
		"max_values": func() error { _, err := rs.Evaluate(map[string]any{"a": []int{1, 2}}, maxValues(2)); return err }(),
		"batch max_values": func() error {
			_, err := rs.EvaluateMany([]any{deep, map[string]any{"a": []int{1}}}, maxValues(1))
			return err
		}(),
		"metadata": func() error {
			_, err := NewRule("m", Const(true), WithMetadata(map[string]any{"x": make([]struct{}, math.MaxInt)}))
			return err
		}(),
		"missing field":  func() error { _, err := rs.Evaluate(deep); return err }(),
		"negative limit": func() error { _, err := rs.Evaluate(nil, WithLimits(Limits{MaxNesting: -1})); return err }(),
		"nil option":     func() error { _, err := rs.Evaluate(nil, nil); return err }(),
		"not a batch":    func() error { _, err := rs.EvaluateMany(3); return err }(),
		"nesting":        func() error { _, err := far.Evaluate(data); return err }(),
		"NaN":            Const(math.NaN()).Err(),
		"empty path":     Field().Err(),
		"bad document":   func() error { _, err := Loads("{"); return err }(),
		"deep document":  func() error { _, err := Loads(`[[[1]]]`, WithLimits(Limits{MaxNesting: 1})); return err }(),
		"text not UTF-8": func() error { _, err := RuleFromText("\xff"); return err }(),
		"table cell":     func() error { _, err := NewDecisionTable([]any{"x", "THEN y"}, [][]any{{math.NaN(), 1}}); return err }(),
	} {
		var e *Error
		if !errors.As(err, &e) {
			t.Errorf("%s: %v", name, err)
			continue
		}
		for _, text := range []string{e.Message, e.Hint} {
			if strings.Contains(text, "cr_") || strings.Contains(text, "CR_") {
				t.Errorf("%s: %q names the C ABI", name, text)
			}
		}
	}
}
